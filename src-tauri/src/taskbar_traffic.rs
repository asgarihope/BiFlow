//! Live download/upload overlay next to the system tray.
//!
//! The overlay is a tiny frameless, transparent, always-on-top window that
//! loads `taskbar.html` (a separate entry, not the main React shell) and
//! renders `D:` / `U:` rates sampled by the same 2 s loop that already
//! accumulates the session totals.
//!
//! Position, size, and even the rate formatter are all platform-neutral;
//! only the per-platform anchoring logic and window creation carry `cfg`
//! gates. macOS and Linux will reuse `format_rates`, `RateSampler`, and
//! the `taskbar-traffic` event when their overlay targets land.

use iran_split_config::{format_bandwidth_pair, AppConfig, BehaviorConfig, TaskbarTrafficDisplay};
use serde::Serialize;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tracing::warn;

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
    /// `None` when this is the first sample so the overlay can show
    /// `—` until a second one arrives.
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
    // overlay label could never display anyway). The double `as` cast is
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

/// Payload pushed into the overlay webview. Kept identical to the
/// TypeScript `TaskbarTrafficView` type the overlay subscribes to.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TaskbarTrafficView {
    pub download: String,
    pub upload: String,
}

/// Render the overlay payload using the user-facing formatter. Pure;
/// reused by every backend (Tauri command, loop, tests).
#[must_use]
pub fn render_rates(rates: RateSample, behavior: &BehaviorConfig) -> TaskbarTrafficView {
    let display = TaskbarTrafficDisplay::from(behavior);
    let (download, upload) = format_bandwidth_pair(rates.download_bps, rates.upload_bps, display);
    TaskbarTrafficView { download, upload }
}

/// Render the overlay payload using the user-facing formatter. Pure;
/// reused by every backend (Tauri command, loop, tests).
#[must_use]
pub fn preview_rates(rates: RateSample, config: &AppConfig) -> TaskbarTrafficView {
    render_rates(rates, &config.behavior)
}

/// Emit a fresh payload to the `taskbar-traffic` window. Skipped when
/// the window is missing (overlay disabled or not yet created on this
/// platform) and when no listener attached; both are expected states
/// during a development run with no display.
pub fn emit_to_window<R: Runtime>(app: &AppHandle<R>, payload: &TaskbarTrafficView) {
    if app.get_webview_window("taskbar-traffic").is_none() {
        return;
    }
    if let Err(cause) = app.emit("taskbar-traffic", payload) {
        warn!(
            event = "taskbar.overlay_emit_failed",
            section = "taskbar",
            initiator = "traffic_loop",
            cause = %cause,
            trace_route = "traffic_loop->frontend_event",
            "taskbar overlay event could not be emitted"
        );
    }
}

/// Tauri label of the overlay webview. The capability file
/// (`src-tauri/capabilities/taskbar-traffic.json`) and the
/// `taskbar.html` entry share this label.
pub const OVERLAY_WINDOW_LABEL: &str = "taskbar-traffic";

/// Physical width of the overlay window in CSS pixels at the typical
/// Windows scale. The CSS adapts to higher DPI; the window itself is
/// not resizable.
pub const OVERLAY_WIDTH: f64 = 96.0;
/// Physical height of the overlay. Sized for two compact `D:` / `U:`
/// lines plus a one-pixel border so it never clips text.
pub const OVERLAY_HEIGHT: f64 = 38.0;

/// Create the overlay webview if the user enabled it and the host
/// platform has an implementation. Idempotent — closing the existing
/// window first means repeated calls during a `save_settings` toggle
/// do not leak phantom windows.
///
/// Windows: anchors immediately left of the Explorer language / clock
/// / notification control (ADR 0114). Linux and macOS return `false`
/// for now; their menubar / panel widgets are planned in a later
/// phase and will reuse the same `RateSampler` and event channel.
pub fn ensure_overlay_window<R: Runtime>(app: &AppHandle<R>) -> bool {
    if !overlay_supported() {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        match windows::ensure_overlay_window(app) {
            Ok(true) => true,
            Ok(false) => false,
            Err(cause) => {
                warn!(
                    event = "taskbar.window_create_failed",
                    section = "taskbar",
                    initiator = "ensure_overlay_window",
                    cause = %cause,
                    trace_route = "ensure_overlay_window->windows_impl",
                    "taskbar overlay window could not be created"
                );
                false
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        false
    }
}

/// Destroy the overlay webview. No-op when the overlay is not supported
/// on this host or the window was never created.
pub fn destroy_overlay_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
        if let Err(cause) = window.destroy() {
            warn!(
                event = "taskbar.window_destroy_failed",
                section = "taskbar",
                initiator = "destroy_overlay_window",
                cause = %cause,
                trace_route = "destroy_overlay_window->window.destroy",
                "taskbar overlay window could not be destroyed"
            );
        }
    }
}

/// `true` on hosts that already know how to place the overlay.
#[cfg(target_os = "windows")]
pub fn overlay_supported() -> bool {
    true
}

#[cfg(not(target_os = "windows"))]
pub fn overlay_supported() -> bool {
    // macOS and Linux widgets are scheduled for a later phase; until
    // then the settings toggle is honoured as a no-op so the UI can
    // ship consistently across every host.
    false
}

/// Move the overlay to match the latest work-area rect without
/// re-creating the window. Safe to call from any code path; the only
/// observable change is the `(left, top)` of a frameless window.
#[cfg(target_os = "windows")]
pub fn reposition_overlay_window<R: Runtime>(app: &AppHandle<R>) {
    windows::reposition_overlay_window(app);
}

/// Move the overlay to match the latest work-area rect without
/// re-creating the window. No-op on hosts whose overlay support is
/// still pending.
#[cfg(not(target_os = "windows"))]
pub fn reposition_overlay_window<R: Runtime>(app: &AppHandle<R>) {
    let _ = app;
}

#[cfg(target_os = "windows")]
mod windows {
    use super::{OVERLAY_HEIGHT, OVERLAY_WIDTH, OVERLAY_WINDOW_LABEL};
    use iran_split_helper_winacl::notify::{overlay_placement, PhysicalRect};
    use tauri::{
        AppHandle, LogicalSize, Manager, PhysicalPosition, Runtime, WebviewUrl,
        WebviewWindowBuilder,
    };
    use tracing::{info, warn};

    /// Default physical size used when we cannot talk to Explorer yet.
    /// Width / height come from the CSS pixel size at 100 % DPI.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "overlay size stays well below i32::MAX even on 8K displays"
    )]
    fn physical_overlay_size<R: Runtime>(app: &AppHandle<R>) -> (i32, i32) {
        let scale = app
            .primary_monitor()
            .ok()
            .flatten()
            .map_or(1.0, |monitor| monitor.scale_factor());
        let width = (OVERLAY_WIDTH * scale).round().max(1.0) as i32;
        let height = (OVERLAY_HEIGHT * scale).round().max(1.0) as i32;
        (width, height)
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_lossless,
        reason = "monitor coordinates fit comfortably in i32 at any realistic DPI"
    )]
    fn monitor_work_area<R: Runtime>(app: &AppHandle<R>) -> Option<PhysicalRect> {
        let monitor = app.primary_monitor().ok().flatten()?;
        let factor = monitor.scale_factor();
        let position = monitor.position();
        let size = monitor.size();
        let factor_i32: i32 = factor as i32;
        let left = position.x / factor_i32;
        let top = position.y / factor_i32;
        let right = left + size.width as i32 / factor_i32;
        let bottom = top + size.height as i32 / factor_i32;
        Some(PhysicalRect {
            left,
            top,
            right,
            bottom,
        })
    }

    pub(super) fn ensure_overlay_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<bool> {
        if app.get_webview_window(OVERLAY_WINDOW_LABEL).is_some() {
            reposition_overlay_window(app);
            return Ok(true);
        }
        let (width, height) = physical_overlay_size(app);
        let mut builder = WebviewWindowBuilder::new(
            app,
            OVERLAY_WINDOW_LABEL,
            WebviewUrl::App("taskbar.html".into()),
        )
        .title("BiFlow traffic")
        .decorations(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .focused(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .transparent(true)
        .visible(false)
        .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT);
        if let Some(monitor) = monitor_work_area(app) {
            if let Some(placement) = overlay_placement(monitor, width, height) {
                builder = builder.position(f64::from(placement.left), f64::from(placement.top));
                info!(
                    event = "taskbar.window_anchored",
                    section = "taskbar",
                    initiator = "ensure_overlay_window",
                    cause = "anchor_resolved",
                    trace_route = "ensure_overlay_window->overlay_placement",
                    left = placement.left,
                    top = placement.top,
                    "taskbar overlay window anchored to the system tray"
                );
            }
        }
        let window = builder.build()?;
        // Tauri 2 sets `focused(false)` for the construction path; a
        // brief noop call keeps `position` and `size` from being
        // re-applied by the framework on first show.
        let _ = window.set_size(LogicalSize::new(OVERLAY_WIDTH, OVERLAY_HEIGHT));
        if let Err(cause) = window.show() {
            warn!(
                event = "taskbar.window_show_failed",
                section = "taskbar",
                initiator = "ensure_overlay_window",
                cause = %cause,
                trace_route = "ensure_overlay_window->window.show",
                "taskbar overlay window could not be shown"
            );
        }
        Ok(true)
    }

    pub(super) fn reposition_overlay_window<R: Runtime>(app: &AppHandle<R>) {
        let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) else {
            return;
        };
        let Some(monitor) = monitor_work_area(app) else {
            return;
        };
        let (width, height) = physical_overlay_size(app);
        let Some(placement) = overlay_placement(monitor, width, height) else {
            return;
        };
        let _ = window.set_position(PhysicalPosition::new(placement.left, placement.top));
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
        // A controller restart sends a brand-new lifetime total.
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
        // Same instant → elapsed == 0; must not panic and must not
        // emit a synthetic GB/s.
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
        // 125_000 B/s × 8 = 1_000_000 b/s = 1.0 Mb at the pinned MB scale.
        // 31_250 B/s × 8 = 250_000 b/s = 0.25 Mb → 0.2 Mb after rounding.
        assert_eq!(view.download, "1.0 Mb");
        assert_eq!(view.upload, "0.2 Mb");
    }

    #[test]
    fn overlay_label_matches_capability_window() {
        // Cross-platform capability consistency check (ADR 0114).
        assert_eq!(OVERLAY_WINDOW_LABEL, "taskbar-traffic");
    }

    #[test]
    fn overlay_supported_is_true_only_on_windows_for_now() {
        // Linux and macOS widgets are scheduled for a later phase.
        if cfg!(target_os = "windows") {
            assert!(overlay_supported());
        } else {
            assert!(!overlay_supported());
        }
    }
}
