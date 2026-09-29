use iran_split_mihomo::TrafficSample;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TrafficTotals {
    pub sent: u64,
    pub received: u64,
}

/// In-memory session counters. Lifetime is the desktop process, not a file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionAccumulator {
    sent: u64,
    received: u64,
    /// Mihomo lifetime totals at the previous sample of this generation.
    last_total: Option<(u64, u64)>,
    /// Bytes already seen on each open process-bypass connection.
    overhead_seen: HashMap<String, (u64, u64)>,
}

impl SessionAccumulator {
    #[must_use]
    pub fn totals(&self) -> TrafficTotals {
        TrafficTotals {
            sent: self.sent,
            received: self.received,
        }
    }
}

/// Folds a Mihomo `/connections` sample into process-scoped totals.
///
/// Mihomo counts every byte crossing TUN, including the proxy client's own
/// encrypted upstream (`PROCESS-NAME` bypass rules to DIRECT). Traffic sent
/// through Hiddify is therefore counted twice: once as the app's connection
/// into Hiddify and again as Hiddify's connection to its server, and every
/// Connect added Hiddify's reconnect/URL-test burst on top. Bytes on
/// connections matched by `bypass_names` are subtracted from the delta.
///
/// A repeated sample adds nothing. A counter decrease is a new Mihomo
/// generation: only its baseline is added. `None` (the controller did not
/// answer) keeps the total. Disconnect keeps the total and clears the
/// cursor so the next Connect starts a fresh generation.
pub fn accumulate(
    store: &mut SessionAccumulator,
    sample: Option<&TrafficSample>,
    bypass_names: &BTreeSet<String>,
    connected: bool,
) -> TrafficTotals {
    if !connected {
        store.last_total = None;
        store.overhead_seen.clear();
        return store.totals();
    }
    let Some(sample) = sample else {
        return store.totals();
    };
    let total = (sample.upload_total, sample.download_total);
    let delta_total = match store.last_total {
        Some((sent, received)) if total.0 >= sent && total.1 >= received => {
            (total.0 - sent, total.1 - received)
        }
        _ => {
            store.overhead_seen.clear();
            total
        }
    };
    let mut overhead = (0_u64, 0_u64);
    let mut seen = HashMap::new();
    for connection in sample
        .connections
        .iter()
        .filter(|connection| connection.is_process_bypass(bypass_names))
    {
        let (previous_sent, previous_received) = store
            .overhead_seen
            .get(&connection.id)
            .copied()
            .unwrap_or((0, 0));
        overhead.0 = overhead
            .0
            .saturating_add(connection.upload.saturating_sub(previous_sent));
        overhead.1 = overhead
            .1
            .saturating_add(connection.download.saturating_sub(previous_received));
        seen.insert(
            connection.id.clone(),
            (connection.upload, connection.download),
        );
    }
    store.overhead_seen = seen;
    store.last_total = Some(total);
    store.sent = store
        .sent
        .saturating_add(delta_total.0.saturating_sub(overhead.0));
    store.received = store
        .received
        .saturating_add(delta_total.1.saturating_sub(overhead.1));
    store.totals()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iran_split_mihomo::ConnectionBytes;

    fn names() -> BTreeSet<String> {
        BTreeSet::from(["hiddify".to_owned()])
    }

    fn sample(up: u64, down: u64, connections: Vec<ConnectionBytes>) -> TrafficSample {
        TrafficSample {
            upload_total: up,
            download_total: down,
            connections,
        }
    }

    fn connection(id: &str, payload: &str, up: u64, down: u64) -> ConnectionBytes {
        ConnectionBytes {
            id: id.into(),
            rule: if payload.is_empty() {
                "Match".into()
            } else {
                "ProcessName".into()
            },
            rule_payload: payload.into(),
            upload: up,
            download: down,
        }
    }

    #[test]
    fn disconnect_keeps_the_displayed_session_total() {
        let mut store = SessionAccumulator::default();
        let connected = accumulate(
            &mut store,
            Some(&sample(1_000, 2_000, vec![])),
            &names(),
            true,
        );
        assert_eq!(
            connected,
            TrafficTotals {
                sent: 1_000,
                received: 2_000
            }
        );
        let disconnected = accumulate(&mut store, None, &names(), false);
        assert_eq!(disconnected, connected);
        let reconnected = accumulate(&mut store, Some(&sample(50, 75, vec![])), &names(), true);
        assert_eq!(
            reconnected,
            TrafficTotals {
                sent: 1_050,
                received: 2_075
            }
        );
    }

    #[test]
    fn a_repeated_sample_adds_nothing() {
        let mut store = SessionAccumulator::default();
        let one = sample(500, 800, vec![connection("h", "hiddify", 100, 200)]);
        let first = accumulate(&mut store, Some(&one), &names(), true);
        let again = accumulate(&mut store, Some(&one), &names(), true);
        assert_eq!(first, again);
        assert_eq!(
            first,
            TrafficTotals {
                sent: 400,
                received: 600
            }
        );
    }

    #[test]
    fn a_mihomo_restart_folds_only_the_new_generation() {
        let mut store = SessionAccumulator::default();
        accumulate(&mut store, Some(&sample(500, 500, vec![])), &names(), true);
        let after_restart = accumulate(&mut store, Some(&sample(10, 10, vec![])), &names(), true);
        assert_eq!(
            after_restart,
            TrafficTotals {
                sent: 510,
                received: 510
            }
        );
    }

    #[test]
    fn proxied_traffic_is_not_counted_twice_through_the_client_upstream() {
        // A 1 MB download through Hiddify: Mihomo sees 1 MB on the app's
        // connection into Hiddify and ~1 MB on Hiddify's own upstream.
        let mut store = SessionAccumulator::default();
        let before = sample(
            100,
            100,
            vec![
                connection("app", "", 50, 50),
                connection("up", "hiddify", 50, 50),
            ],
        );
        let after = sample(
            200,
            2_000_100,
            vec![
                connection("app", "", 100, 1_000_050),
                connection("up", "hiddify", 100, 1_000_050),
            ],
        );
        accumulate(&mut store, Some(&before), &names(), true);
        let totals = accumulate(&mut store, Some(&after), &names(), true);
        assert_eq!(
            totals,
            TrafficTotals {
                sent: 100,
                received: 1_000_050
            }
        );
    }

    #[test]
    fn a_reconnect_burst_of_the_client_itself_adds_nothing() {
        let mut store = SessionAccumulator::default();
        accumulate(
            &mut store,
            Some(&sample(1_000, 1_000, vec![])),
            &names(),
            true,
        );
        accumulate(&mut store, None, &names(), false);
        let burst = sample(
            300_000,
            900_000,
            vec![
                connection("t1", "hiddify", 150_000, 450_000),
                connection("t2", "hiddify", 150_000, 450_000),
            ],
        );
        assert_eq!(
            accumulate(&mut store, Some(&burst), &names(), true),
            TrafficTotals {
                sent: 1_000,
                received: 1_000
            }
        );
    }

    #[test]
    fn an_unanswered_sample_keeps_the_total() {
        let mut store = SessionAccumulator::default();
        let first = accumulate(&mut store, Some(&sample(7, 9, vec![])), &names(), true);
        assert_eq!(accumulate(&mut store, None, &names(), true), first);
        assert_eq!(
            accumulate(&mut store, Some(&sample(8, 9, vec![])), &names(), true),
            TrafficTotals {
                sent: 8,
                received: 9
            }
        );
    }

    #[test]
    fn a_legacy_totals_file_is_not_loaded() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("traffic-totals.json");
        std::fs::write(
            &path,
            br#"{"lifetime_sent":9000,"lifetime_received":8000,"last_session_sent":100,"last_session_received":200}"#,
        )
        .expect("legacy");
        assert!(path.is_file());
        let mut store = SessionAccumulator::default();
        assert_eq!(
            accumulate(&mut store, None, &names(), false),
            TrafficTotals::default()
        );
    }
}
