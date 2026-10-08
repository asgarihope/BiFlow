//! Native Windows taskbar boundary. No statistics collection or `WebView`.

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::{diagnostic_status, Display, Status};

/// Selects a gap with margins, preferring the weather side of the taskbar.
/// All coordinates are taskbar-client physical pixels. Unknown layouts fail closed.
#[must_use]
pub fn find_gap(width: i32, occupied: &[(i32, i32)], requested: i32, margin: i32) -> Option<i32> {
    if width <= 0 || requested <= 0 || margin < 0 || occupied.is_empty() {
        return None;
    }
    let mut spans = occupied.to_vec();
    spans.sort_unstable();
    let mut cursor = margin;
    for (left, right) in spans {
        if left < 0 || right < left || right > width {
            return None;
        }
        if left - cursor >= requested + margin {
            return Some(cursor);
        }
        cursor = cursor.max(right + margin);
    }
    (width - cursor >= requested + margin).then_some(cursor)
}

/// Binary byte units, matching `BiFlow`'s session counter display.
#[must_use]
pub fn format_rate(rate: Option<f64>) -> String {
    let Some(mut value) = rate.filter(|value| value.is_finite() && *value >= 0.0) else {
        return "— B/s".into();
    };
    let units = ["B/s", "KiB/s", "MiB/s", "GiB/s", "TiB/s", "PiB/s"];
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", units[unit])
    } else {
        let digits = usize::from(value < 100.0) + 1;
        format!("{value:.digits$} {}", units[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_never_overlap_widgets_apps_or_tray() {
        let controls = [(0, 220), (500, 900), (1100, 1200)];
        assert_eq!(find_gap(1200, &controls, 160, 8), Some(228));
        assert_eq!(find_gap(800, &[(0, 650), (700, 800)], 160, 8), None);
        assert_eq!(find_gap(800, &[], 160, 8), None);
        assert_eq!(find_gap(800, &[(0, 900)], 160, 8), None);
        assert_eq!(
            find_gap(800, &[(0, 200), (100, 500), (700, 800)], 160, 8),
            Some(508)
        );
    }

    #[test]
    fn formatting_matches_binary_session_units_and_unavailable_state() {
        assert_eq!(format_rate(Some(0.0)), "0 B/s");
        assert_eq!(format_rate(Some(1024.0)), "1.00 KiB/s");
        assert_eq!(format_rate(Some(350.0 * 1024.0)), "350.0 KiB/s");
        assert_eq!(format_rate(None), "— B/s");
        assert_eq!(format_rate(Some(f64::NAN)), "— B/s");
    }
}
