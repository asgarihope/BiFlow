//! Native Windows taskbar boundary. No statistics collection or `WebView`.

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::{diagnostic_renderer, diagnostic_status, Display, Status};

#[cfg(any(windows, test))]
fn rows(upload: Option<f64>, download: Option<f64>) -> [(&'static str, Option<f64>); 2] {
    [("↑", upload), ("↓", download)]
}

/// Chooses the rightmost verified gap, adjacent to the notification area.
/// All coordinates are taskbar-client physical pixels. Unknown layouts fail closed.
#[must_use]
pub fn find_gap(width: i32, occupied: &[(i32, i32)], requested: i32, margin: i32) -> Option<i32> {
    if width <= 0 || requested <= 0 || margin < 0 || occupied.is_empty() {
        return None;
    }
    let mut spans = occupied.to_vec();
    spans.sort_unstable();
    let mut cursor = margin;
    let mut best = None;
    for (left, right) in spans {
        if left < 0 || right < left || right > width {
            return None;
        }
        if left - cursor >= requested + margin {
            best = Some(left - requested - margin);
        }
        cursor = cursor.max(right + margin);
    }
    if width - cursor >= requested + margin {
        best = Some(width - requested - margin);
    }
    best
}

/// Decimal bits/second, matching Windows Task Manager's throughput units.
/// The shared provider supplies byte rates; this is the sole bit conversion.
#[must_use]
pub fn format_rate(rate: Option<f64>) -> String {
    let Some(mut value) = rate
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|bytes| bytes * 8.0)
        .filter(|bits| bits.is_finite())
    else {
        return "— bps".into();
    };
    let units = ["bps", "Kbps", "Mbps", "Gbps", "Tbps", "Pbps"];
    let mut unit = 0;
    while value >= 1000.0 && unit < units.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit <= 1 || value >= 100.0 {
        format!("{value:.0} {}", units[unit])
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_never_overlap_widgets_apps_or_tray() {
        let controls = [(0, 220), (500, 900), (1100, 1200)];
        assert_eq!(find_gap(1200, &controls, 160, 8), Some(932));
        assert_eq!(find_gap(800, &[(0, 650), (700, 800)], 160, 8), None);
        assert_eq!(find_gap(800, &[], 160, 8), None);
        assert_eq!(find_gap(800, &[(0, 900)], 160, 8), None);
        assert_eq!(
            find_gap(800, &[(0, 200), (100, 500), (700, 800)], 160, 8),
            Some(532)
        );
    }

    #[test]
    fn formatting_matches_task_manager_bits_and_unavailable_state() {
        assert_eq!(format_rate(Some(0.0)), "0 bps");
        assert_eq!(format_rate(Some(14_000.0)), "112 Kbps");
        assert_eq!(format_rate(Some(450_000.0)), "3.6 Mbps");
        assert_eq!(format_rate(None), "— bps");
        assert_eq!(format_rate(Some(f64::NAN)), "— bps");
    }

    #[test]
    fn upload_is_above_download_with_distinct_values() {
        assert_eq!(
            rows(Some(14_000.0), Some(13_000.0)),
            [("↑", Some(14_000.0)), ("↓", Some(13_000.0))]
        );
    }
}
