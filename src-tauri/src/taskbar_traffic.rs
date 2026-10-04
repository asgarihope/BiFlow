//! Live download/upload badge rendered into the `BiFlow` tray icon.
//!
//! The tray icon itself is the carrier — there is no separate overlay
//! window next to the clock. On each 2 s traffic sample we recompute
//! the bytes-per-second rate and ask the `BiFlow` tray icon to redraw
//! itself with a stacked `D` / `U` row.
//!
//! The rate sampler, formatter, and event-payload shape are all
//! platform-neutral so macOS / Linux can reuse them when their menu bar
//! widget lands (no changes needed beyond a different renderer).

use iran_split_config::{
    format_bandwidth_pair, format_bandwidth_rate_compact, AppConfig, BehaviorConfig,
    TaskbarTrafficDisplay,
};
use serde::Serialize;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Runtime};
use tracing::warn;

#[cfg(target_os = "windows")]
use iran_split_helper_winacl::tray_icon as windows_tray_icon;

/// Bytes-per-second between the previous successful sample and the most
/// recent one. `None` when no previous sample exists yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateSample {
    pub download_bps: u64,
    pub upload_bps: u64,
}

/// Tracks the previous Mihomo cumulative bytes and the wall-clock at
/// that sample, so the caller can convert successive lifetime totals
/// into bytes-per-second without coupling to the controller time.
#[derive(Debug, Clone, Default)]
pub struct RateSampler {
    last_total: Option<(u64, u64, Instant)>,
}

impl RateSampler {
    /// Feed a fresh `(upload_total, download_total)` sample taken at
    /// `now`. Returns the rates measured since the previous call, or
    /// `None` when this is the first sample so the icon can stay
    /// "idle" until a second one arrives.
    #[must_use]
    pub fn observe(
        &mut self,
        upload_total: u64,
        download_total: u64,
        now: Instant,
    ) -> Option<RateSample> {
        match self.last_total {
            Some((prev_up, prev_down, prev_at))
                if now > prev_at && upload_total >= prev_up && download_total >= prev_down =>
            {
                let elapsed = now.duration_since(prev_at).as_secs_f64();
                // Guard against a paused virtual clock so we never divide
                // by zero or a tiny value that explodes into MB/s spikes.
                if elapsed < 0.5 {
                    return None;
                }
                let up_delta = upload_total - prev_up;
                let down_delta = download_total - prev_down;
                let rates = RateSample {
                    download_bps: per_second(down_delta, elapsed),
                    upload_bps: per_second(up_delta, elapsed),
                };
                self.last_total = Some((upload_total, download_total, now));
                Some(rates)
            }
            _ => {
                self.last_total = Some((upload_total, download_total, now));
                None
            }
        }
    }

    /// Force the sampler to forget the previous baseline, e.g. when the
    /// controller restarts and the totals drop.
    pub fn reset(&mut self) {
        self.last_total = None;
    }
}

fn per_second(delta: u64, elapsed_secs: f64) -> u64 {
    if elapsed_secs <= 0.0 {
        return 0;
    }
    // The `f64` rounding and the `u64` clamp accept the precision loss
    // (any rate above ~9 PB/s loses sub-megahertz precision, which the
    // tray icon could never display anyway). The double `as` cast is
    // therefore safe here.
    #[expect(
        clippy::cast_precision_loss,
        reason = "rate formatter rounds to whole bytes per second"
    )]
    let rate = delta as f64 / elapsed_secs;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "round + max(0) guarantees a non-negative integer value"
    )]
    let rate = rate.round().max(0.0) as u64;
    rate
}

/// Payload pushed to listeners when the live rate changes. Used by
/// any future cross-platform widget that mirrors the tray icon.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TaskbarTrafficView {
    pub download: String,
    pub upload: String,
}

/// Render the live payload using the user-facing formatter. Pure;
/// reused by every backend (Tauri command, loop, tests).
#[must_use]
pub fn render_rates(rates: RateSample, behavior: &BehaviorConfig) -> TaskbarTrafficView {
    let display = TaskbarTrafficDisplay::from(behavior);
    let (download, upload) = format_bandwidth_pair(rates.download_bps, rates.upload_bps, display);
    TaskbarTrafficView { download, upload }
}

/// Same as [`render_rates`] but takes a full `AppConfig` for callers
/// (Settings preview, Tauri commands) that already have one in scope.
#[must_use]
pub fn preview_rates(rates: RateSample, config: &AppConfig) -> TaskbarTrafficView {
    render_rates(rates, &config.behavior)
}

/// Build the multi-line Windows tray tooltip.
///
/// Windows tray tooltips are line-break aware (`\r\n`) and the GDI
/// renderer truncates anything longer than 127 chars; the two rate
/// lines stay short so neither line ever gets clipped.
#[must_use]
pub fn tray_tooltip_text(rates: RateSample, behavior: &BehaviorConfig) -> String {
    let view = render_rates(rates, behavior);
    format!(
        "BiFlow — live traffic\r\nD: {}\r\nU: {}",
        view.download, view.upload
    )
}

/// Redraw the `BiFlow` tray icon with the current rates and refresh
/// its hover tooltip with the matching formatted rates.
///
/// On Windows the icon is rendered into a 32×32 RGBA buffer by
/// `iran_split_helper_winacl::tray_icon::render_rate_icon` and pushed
/// into Tauri via `TrayIcon::set_icon`; the tooltip is updated via
/// `TrayIcon::set_title`. macOS / Linux currently keep the default
/// `BiFlow` icon and only forward the formatted strings on the
/// `taskbar-traffic` event channel for a future widget.
pub fn publish_to_tray<R: Runtime>(
    app: &AppHandle<R>,
    rates: RateSample,
    behavior: &BehaviorConfig,
) {
    let tooltip = tray_tooltip_text(rates, behavior);
    let display = TaskbarTrafficDisplay::from(behavior);
    let download_compact = format_bandwidth_rate_compact(rates.download_bps, display);
    let upload_compact = format_bandwidth_rate_compact(rates.upload_bps, display);
    #[cfg(target_os = "windows")]
    {
        use tauri::image::Image;
        let bytes = windows_tray_icon::render_rate_icon(
            rates.download_bps,
            rates.upload_bps,
            download_compact,
            upload_compact,
        );
        let image = Image::new_owned(
            bytes,
            windows_tray_icon::ICON_WIDTH,
            windows_tray_icon::ICON_HEIGHT,
        );
        if let Some(tray) = app.tray_by_id("main") {
            if let Err(cause) = tray.set_icon(Some(image)) {
                warn!(
                    event = "taskbar.tray_icon_update_failed",
                    section = "taskbar",
                    initiator = "traffic_loop",
                    cause = %cause,
                    trace_route = "traffic_loop->tray.set_icon",
                    "tray icon image could not be replaced"
                );
            }
            if let Err(cause) = tray.set_title(Some(tooltip)) {
                warn!(
                    event = "taskbar.tray_title_update_failed",
                    section = "taskbar",
                    initiator = "traffic_loop",
                    cause = %cause,
                    trace_route = "traffic_loop->tray.set_title",
                    "tray icon tooltip could not be replaced"
                );
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        let _ = tooltip;
    }
    // Always forward the formatted payload so future cross-platform
    // widgets and the Settings preview agree on the same string.
    if let Err(cause) = app.emit(
        "taskbar-traffic",
        &TaskbarTrafficView {
            download: rates.download_bps.to_string(),
            upload: rates.upload_bps.to_string(),
        },
    ) {
        warn!(
            event = "taskbar.traffic_emit_failed",
            section = "taskbar",
            initiator = "traffic_loop",
            cause = %cause,
            trace_route = "traffic_loop->frontend_event",
            "taskbar traffic event could not be emitted"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn first_sample_returns_none() {
        let mut sampler = RateSampler::default();
        let now = Instant::now();
        assert!(sampler.observe(1_000, 2_000, now).is_none());
    }

    #[test]
    fn second_sample_returns_bytes_per_second() {
        let mut sampler = RateSampler::default();
        let t0 = Instant::now();
        assert!(sampler.observe(0, 0, t0).is_none());
        let t1 = t0 + Duration::from_secs(2);
        let rates = sampler.observe(2_000, 4_000, t1).expect("rates");
        assert_eq!(rates.download_bps, 2_000);
        assert_eq!(rates.upload_bps, 1_000);
    }

    #[test]
    fn counter_reset_resets_the_baseline() {
        let mut sampler = RateSampler::default();
        let t0 = Instant::now();
        assert!(sampler.observe(5_000, 5_000, t0).is_none());
        let t1 = t0 + Duration::from_secs(2);
        assert!(sampler.observe(100, 100, t1).is_none());
        let t2 = t1 + Duration::from_secs(2);
        let rates = sampler.observe(2_100, 4_100, t2).expect("rates");
        assert_eq!(rates.download_bps, 2_000);
        assert_eq!(rates.upload_bps, 1_000);
    }

    #[test]
    fn a_paused_clock_returns_none_so_we_never_explode_into_spikes() {
        let mut sampler = RateSampler::default();
        let t0 = Instant::now();
        let _ = sampler.observe(0, 0, t0);
        assert!(sampler.observe(1_000, 1_000, t0).is_none());
    }

    #[test]
    fn render_routes_through_the_user_facing_formatter() {
        let behavior = BehaviorConfig {
            taskbar_traffic_unit: iran_split_config::TaskbarTrafficUnit::Megabytes,
            taskbar_traffic_bits: true,
            ..BehaviorConfig::default()
        };
        let view = render_rates(
            RateSample {
                download_bps: 125_000,
                upload_bps: 31_250,
            },
            &behavior,
        );
        assert_eq!(view.download, "1.0 Mb");
        assert_eq!(view.upload, "0.2 Mb");
    }

    #[test]
    fn tray_tooltip_text_includes_both_directions() {
        let behavior = BehaviorConfig::default();
        let tooltip = tray_tooltip_text(
            RateSample {
                download_bps: 2_400_000,
                upload_bps: 350_000,
            },
            &behavior,
        );
        // Windows GDI truncates anything past 127 chars; pin the cap
        // so a future unit change cannot silently break the tooltip.
        assert!(
            tooltip.len() < 127,
            "tooltip must stay under the GDI 127-char limit"
        );
        assert!(tooltip.starts_with("BiFlow — live traffic\r\n"));
        assert!(tooltip.contains("D: 2.4 MB"));
        assert!(tooltip.contains("U: 350 KB"));
    }

    #[test]
    fn tray_tooltip_text_stays_short_when_rate_pinned_to_huge_unit() {
        // A 9 PB/s reading must not blow past the GDI truncation
        // limit just because the unit made the digits explode.
        let behavior = BehaviorConfig {
            taskbar_traffic_unit: iran_split_config::TaskbarTrafficUnit::Gigabytes,
            taskbar_traffic_bits: false,
            ..BehaviorConfig::default()
        };
        let tooltip = tray_tooltip_text(
            RateSample {
                download_bps: u64::MAX,
                upload_bps: u64::MAX,
            },
            &behavior,
        );
        assert!(tooltip.len() < 127);
    }
}
