//! Shared physical-interface statistics, separate from taskbar rendering.
use std::time::{Duration, Instant};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::sample;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::sample;

/// One adapter's complete hardware-interface counters. Never sum TUN + NIC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub identity: u64,
    pub class: &'static str,
    pub sent: u64,
    pub received: u64,
}

/// Bytes/second internally; the renderer converts to decimal bits/second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rates {
    pub upload: f64,
    pub download: f64,
}

#[derive(Default)]
pub struct RateTracker {
    previous: Option<(Sample, Instant)>,
}

impl RateTracker {
    #[expect(
        clippy::cast_precision_loss,
        reason = "throughput is an approximate display rate"
    )]
    pub fn update(&mut self, sample: Option<Sample>, now: Instant) -> Option<Rates> {
        let Some(sample) = sample else {
            self.previous = None;
            return None;
        };
        let previous = self.previous.replace((sample, now));
        let (before, time) = previous?;
        let elapsed = now.checked_duration_since(time)?;
        if before.identity != sample.identity
            || sample.sent < before.sent
            || sample.received < before.received
            || elapsed.is_zero()
            || elapsed > Duration::from_secs(3)
        {
            return None;
        }
        Some(Rates {
            upload: (sample.sent - before.sent) as f64 / elapsed.as_secs_f64(),
            download: (sample.received - before.received) as f64 / elapsed.as_secs_f64(),
        })
    }
}

#[cfg(any(windows, target_os = "linux", test))]
#[derive(Clone, Copy)]
struct Candidate {
    sample: Sample,
    hardware: bool,
    up: bool,
    default_metric: Option<u64>,
}

#[cfg(any(windows, target_os = "linux", test))]
fn select(candidates: &[Candidate]) -> Option<Sample> {
    let eligible: Vec<_> = candidates
        .iter()
        .filter(|candidate| {
            candidate.hardware
                && candidate.up
                && matches!(candidate.sample.class, "wifi" | "ethernet")
        })
        .collect();
    if let Some(best) = eligible
        .iter()
        .filter_map(|candidate| {
            candidate
                .default_metric
                .map(|metric| (metric, candidate.sample.identity, candidate.sample))
        })
        .min_by_key(|(metric, identity, _)| (*metric, *identity))
    {
        return Some(best.2);
    }
    if eligible.len() == 1 {
        Some(eligible[0].sample)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn candidate(id: u64, class: &'static str, metric: Option<u64>) -> Candidate {
        Candidate {
            sample: Sample {
                identity: id,
                class,
                sent: 100,
                received: 200,
            },
            hardware: true,
            up: true,
            default_metric: metric,
        }
    }

    #[test]
    fn selects_one_physical_default_even_when_vpn_has_lower_metric() {
        let mut vpn = candidate(1, "ethernet", Some(0));
        vpn.hardware = false;
        let wifi = candidate(2, "wifi", Some(35));
        let ethernet = candidate(3, "ethernet", Some(10));
        assert_eq!(select(&[vpn, wifi, ethernet]), Some(ethernet.sample));
        assert_eq!(select(&[vpn, wifi]), Some(wifi.sample));
        let mut disconnected = ethernet;
        disconnected.up = false;
        assert_eq!(select(&[disconnected, wifi]), Some(wifi.sample));
    }

    #[test]
    fn no_default_requires_a_single_eligible_adapter() {
        let wifi = candidate(1, "wifi", None);
        assert_eq!(select(&[wifi]), Some(wifi.sample));
        assert_eq!(select(&[wifi, candidate(2, "ethernet", None)]), None);
        assert_eq!(select(&[candidate(3, "tunnel", Some(0))]), None);
    }

    #[test]
    fn elapsed_time_and_receive_resets_control_the_displayed_rate() {
        let mut tracker = RateTracker::default();
        let start = Instant::now();
        let mut sample = candidate(1, "ethernet", None).sample;
        assert_eq!(tracker.update(Some(sample), start), None);
        sample.sent += 14_000;
        sample.received += 13_000;
        assert_eq!(
            tracker.update(Some(sample), start + Duration::from_secs(2)),
            Some(Rates {
                upload: 7_000.0,
                download: 6_500.0
            })
        );
        sample.received = 0;
        assert_eq!(
            tracker.update(Some(sample), start + Duration::from_secs(3)),
            None
        );
        assert_eq!(
            tracker.update(Some(sample), start + Duration::from_secs(3)),
            None
        );
        assert_eq!(
            tracker.update(Some(sample), start + Duration::from_secs(2)),
            None
        );
    }

    #[test]
    fn rates_use_complete_adapter_deltas_and_reset_on_switch_loss_or_sleep() {
        let mut tracker = RateTracker::default();
        let start = Instant::now();
        let mut sample = candidate(1, "wifi", None).sample;
        assert_eq!(tracker.update(Some(sample), start), None);
        sample.sent += 14_000;
        sample.received += 13_000;
        assert_eq!(
            tracker.update(Some(sample), start + Duration::from_secs(1)),
            Some(Rates {
                upload: 14_000.0,
                download: 13_000.0
            })
        );
        sample.identity = 2;
        assert_eq!(
            tracker.update(Some(sample), start + Duration::from_secs(2)),
            None
        );
        sample.sent = 0;
        assert_eq!(
            tracker.update(Some(sample), start + Duration::from_secs(3)),
            None
        );
        assert_eq!(tracker.update(None, start + Duration::from_secs(4)), None);
        assert_eq!(
            tracker.update(Some(sample), start + Duration::from_secs(5)),
            None
        );
        assert_eq!(
            tracker.update(Some(sample), start + Duration::from_secs(20)),
            None
        );
    }
}
