//! Tauri lifecycle bridge. Native handles remain inside the boundary crate.
use crate::services;
#[cfg(windows)]
use crate::show_main;
use iran_split_config::SpeedUnit;
#[cfg(windows)]
use iran_split_taskbar::Display;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{AppHandle, Manager, Runtime};

pub struct Taskbar {
    #[cfg(windows)]
    display: Mutex<Option<Display>>,
    enabled: AtomicBool,
    stopped: AtomicBool,
    unit: Mutex<SpeedUnit>,
}
static ADAPTER_STATE: Mutex<&'static str> = Mutex::new("disabled");
#[cfg(any(target_os = "linux", target_os = "macos"))]
static PANEL_STATE: Mutex<&'static str> = Mutex::new("disabled");

pub fn adapter_state() -> &'static str {
    *ADAPTER_STATE.lock().expect("adapter state")
}

pub fn setup<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(windows)]
    let handle = app.clone();
    #[cfg(windows)]
    let display = Display::start(move || {
        let main = handle.clone();
        tracing::info!(
            event = "taskbar.clicked",
            section = "taskbar",
            initiator = "native_window",
            cause = "user_click",
            trace_route = "taskbar->show_main",
            "taskbar activated main window"
        );
        if handle.run_on_main_thread(move || show_main(&main)).is_err() {
            tracing::warn!(
                event = "taskbar.focus_failed",
                section = "taskbar",
                initiator = "native_window",
                cause = "main_thread_unavailable",
                trace_route = "taskbar->show_main",
                "main window focus failed"
            );
        }
    });
    #[cfg(windows)]
    let display = if let Ok(display) = display {
        Some(display)
    } else {
        tracing::warn!(
            event = "taskbar.start_failed",
            section = "taskbar",
            initiator = "tauri_setup",
            cause = "thread_spawn_failed",
            trace_route = "startup->taskbar",
            "native taskbar worker unavailable"
        );
        None
    };
    app.manage(Taskbar {
        #[cfg(windows)]
        display: Mutex::new(display),
        enabled: AtomicBool::new(false),
        stopped: AtomicBool::new(false),
        unit: Mutex::new(SpeedUnit::default()),
    });
    if let Ok(config) = services(app).and_then(|services| {
        services
            .config_store
            .load_or_create()
            .map_err(|_| "configuration_unavailable".to_owned())
    }) {
        apply_preferences(
            app,
            config.behavior.show_taskbar_speed,
            config.behavior.taskbar_speed_unit,
        );
    } else {
        tracing::warn!(
            event = "taskbar.preference_failed",
            section = "taskbar",
            initiator = "tauri_setup",
            cause = "configuration_unavailable",
            trace_route = "startup->taskbar",
            "taskbar preference could not be read"
        );
    }
    spawn_interface_sampler(app.clone());
}

pub fn apply_preferences<R: Runtime>(app: &AppHandle<R>, enabled: bool, unit: SpeedUnit) {
    if let Some(state) = app.try_state::<Taskbar>() {
        state.enabled.store(enabled, Ordering::Release);
        *state.unit.lock().expect("speed unit") = unit;
        #[cfg(windows)]
        if let Some(display) = state.display.lock().expect("taskbar state").as_ref() {
            display.set_unit(unit);
            display.set_enabled(enabled);
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        update_panel(app, enabled, None, unit);
        tracing::info!(
            event = "taskbar.preference_applied",
            section = "taskbar",
            initiator = "settings",
            cause = "saved_preference",
            trace_route = "settings->taskbar",
            enabled,
            unit = ?unit,
            "taskbar preference applied"
        );
    }
}

fn spawn_interface_sampler<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let mut tracker = iran_split_netstats::RateTracker::default();
        let mut previous = "disabled";
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(1));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            timer.tick().await;
            let Some(state) = app.try_state::<Taskbar>() else {
                break;
            };
            if state.stopped.load(Ordering::Acquire) {
                break;
            }
            #[cfg(windows)]
            if state.display.lock().expect("taskbar state").is_none() {
                break;
            }
            let enabled = state.enabled.load(Ordering::Acquire);
            let result = if enabled {
                tauri::async_runtime::spawn_blocking(iran_split_netstats::sample)
                    .await
                    .unwrap_or(Err("interface_sampler_join_failed"))
            } else {
                Ok(None)
            };
            let (sample, status) = match result {
                Ok(Some(sample)) => (Some(sample), sample.class),
                Ok(None) => (
                    None,
                    if enabled {
                        "no_active_physical_adapter"
                    } else {
                        "disabled"
                    },
                ),
                Err(cause) => (None, cause),
            };
            if status != previous {
                adapter_transition(status);
                previous = status;
            }
            *ADAPTER_STATE.lock().expect("adapter state") = status;
            let rates = tracker.update(sample, std::time::Instant::now());
            #[cfg(windows)]
            if let Some(display) = state.display.lock().expect("taskbar state").as_ref() {
                display.update(
                    rates.map(|rates| rates.upload),
                    rates.map(|rates| rates.download),
                );
            };
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            {
                let unit = *state.unit.lock().expect("speed unit");
                update_panel(&app, state.enabled.load(Ordering::Acquire), rates, unit);
            }
        }
        tracing::info!(
            event = "traffic.interface_sampler_stopped",
            section = "traffic",
            initiator = "application_shutdown",
            cause = "taskbar_shutdown",
            trace_route = "shutdown->interface_stats",
            "physical interface sampler stopped"
        );
    });
}

fn adapter_transition(status: &'static str) {
    if matches!(status, "wifi" | "ethernet" | "disabled") {
        tracing::info!(
            event = "traffic.interface_source_changed",
            section = "traffic",
            initiator = "interface_sampler",
            cause = status,
            trace_route = "os_interface->shared_stats->taskbar",
            source = "physical_interface",
            "physical interface source changed"
        );
    } else {
        tracing::warn!(
            event = "traffic.interface_sample_unavailable",
            section = "traffic",
            initiator = "interface_sampler",
            cause = status,
            trace_route = "os_interface->shared_stats->taskbar",
            "physical interface statistics unavailable"
        );
    }
}

pub fn shutdown<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<Taskbar>() {
        state.stopped.store(true, Ordering::Release);
        state.enabled.store(false, Ordering::Release);
        #[cfg(windows)]
        drop(state.display.lock().expect("taskbar state").take());
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let unit = *state.unit.lock().expect("speed unit");
            update_panel(app, false, None, unit);
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn panel_state() -> &'static str {
    *PANEL_STATE.lock().expect("panel state")
}

#[cfg(target_os = "linux")]
const TRAY_LABEL_ROUTE: &str = "shared_stats->tray_label->appindicator";
#[cfg(target_os = "macos")]
const TRAY_LABEL_ROUTE: &str = "shared_stats->tray_label->menu_bar";

#[cfg(target_os = "linux")]
fn panel_label(
    enabled: bool,
    rates: Option<iran_split_netstats::Rates>,
    unit: SpeedUnit,
) -> Option<String> {
    enabled.then(|| {
        format!(
            "↑ {}  ↓ {}",
            iran_split_taskbar::format_rate_in(rates.map(|rates| rates.upload), unit),
            iran_split_taskbar::format_rate_in(rates.map(|rates| rates.download), unit),
        )
    })
}

#[cfg(target_os = "macos")]
fn panel_label(
    enabled: bool,
    rates: Option<iran_split_netstats::Rates>,
    unit: SpeedUnit,
) -> Option<String> {
    enabled.then(|| {
        iran_split_taskbar::menu_bar_label(
            rates.map(|rates| rates.upload),
            rates.map(|rates| rates.download),
            unit,
        )
    })
}

#[cfg(target_os = "linux")]
fn update_panel<R: Runtime>(
    app: &AppHandle<R>,
    enabled: bool,
    rates: Option<iran_split_netstats::Rates>,
    unit: SpeedUnit,
) {
    let status = match app.tray_by_id("main") {
        Some(tray) => match tray.set_title(panel_label(enabled, rates, unit)) {
            Ok(()) => {
                if enabled {
                    "label_set"
                } else {
                    "disabled"
                }
            }
            Err(_) => "failed",
        },
        None => "unavailable",
    };
    record_panel_state(status);
}

#[cfg(target_os = "macos")]
fn update_panel<R: Runtime>(
    app: &AppHandle<R>,
    enabled: bool,
    rates: Option<iran_split_netstats::Rates>,
    unit: SpeedUnit,
) {
    let label = panel_label(enabled, rates, unit);
    let status = if iran_split_taskbar::menu_bar_is_main_thread() {
        menu_bar_submission(label.as_deref())
    } else {
        submit_menu_bar(app, label.clone())
    };
    let status = if status == "unavailable" {
        plain_menu_bar_title(app, label)
    } else {
        status
    };
    record_panel_state(status);
}

#[cfg(target_os = "macos")]
fn plain_menu_bar_title<R: Runtime>(app: &AppHandle<R>, label: Option<String>) -> &'static str {
    let enabled = label.is_some();
    match app.tray_by_id("main") {
        Some(tray) => match tray.set_title(label) {
            Ok(()) => {
                if enabled {
                    "plain_title"
                } else {
                    "disabled"
                }
            }
            Err(_) => "failed",
        },
        None => "unavailable",
    }
}

#[cfg(target_os = "macos")]
fn menu_bar_submission(label: Option<&str>) -> &'static str {
    if iran_split_taskbar::apply_menu_bar_label(label) {
        if label.is_some() {
            "label_set"
        } else {
            "disabled"
        }
    } else {
        "unavailable"
    }
}

#[cfg(target_os = "macos")]
fn submit_menu_bar<R: Runtime>(app: &AppHandle<R>, label: Option<String>) -> &'static str {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let queued = app.run_on_main_thread(move || {
        let status = menu_bar_submission(label.as_deref());
        if sender.send(status).is_err() {
            tracing::warn!(
                event = "taskbar.panel_unavailable",
                section = "taskbar",
                initiator = "interface_sampler",
                cause = "waiter_dropped",
                trace_route = TRAY_LABEL_ROUTE,
                "menu bar label result had no waiter"
            );
        }
    });
    if queued.is_err() {
        return "failed";
    }
    receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap_or("failed")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn record_panel_state(status: &'static str) {
    let mut previous = PANEL_STATE.lock().expect("panel state");
    if status != *previous {
        if status == "plain_title" {
            tracing::warn!(
                event = "taskbar.panel_unavailable",
                section = "taskbar",
                initiator = "interface_sampler",
                cause = status,
                trace_route = TRAY_LABEL_ROUTE,
                "menu bar title fell back to a plain label"
            );
        } else if matches!(status, "failed" | "unavailable") {
            tracing::warn!(
                event = "taskbar.panel_unavailable",
                section = "taskbar",
                initiator = "interface_sampler",
                cause = status,
                trace_route = TRAY_LABEL_ROUTE,
                "panel speed label could not be updated"
            );
        } else {
            tracing::info!(
                event = "taskbar.panel_state_changed",
                section = "taskbar",
                initiator = "interface_sampler",
                cause = status,
                trace_route = TRAY_LABEL_ROUTE,
                "panel speed label state changed"
            );
        }
        *previous = status;
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn panel_label_uses_shared_bits_and_clears_when_disabled() {
        assert_eq!(
            panel_label(true, None, SpeedUnit::AutoBits).as_deref(),
            Some("↑ — bps  ↓ — bps")
        );
        let rates = Some(iran_split_netstats::Rates {
            upload: 14_000.0,
            download: 450_000.0,
        });
        assert_eq!(
            panel_label(true, rates, SpeedUnit::AutoBits).as_deref(),
            Some("↑ 112 Kbps  ↓ 3.6 Mbps")
        );
        assert_eq!(
            panel_label(true, rates, SpeedUnit::Kilobytes).as_deref(),
            Some("↑ 14 KB/s  ↓ 450 KB/s")
        );
        assert_eq!(
            panel_label(true, None, SpeedUnit::Megabytes).as_deref(),
            Some("↑ — MB/s  ↓ — MB/s")
        );
        assert_eq!(panel_label(false, rates, SpeedUnit::Megabytes), None);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn panel_label_stacks_equal_width_lines_and_clears_when_disabled() {
        let idle = panel_label(true, None, SpeedUnit::AutoBits).expect("idle label");
        let idle_lines: Vec<_> = idle.lines().collect();
        assert_eq!(idle_lines.len(), 2);
        assert_eq!(idle_lines[0].chars().count(), idle_lines[1].chars().count());
        assert!(idle_lines[0].ends_with("— bps ↑"));
        assert!(idle_lines[1].ends_with("— bps ↓"));
        let rates = Some(iran_split_netstats::Rates {
            upload: 14_000.0,
            download: 450_000.0,
        });
        let live = panel_label(true, rates, SpeedUnit::AutoBits).expect("live label");
        let live_lines: Vec<_> = live.lines().collect();
        assert_eq!(live_lines[0].chars().count(), idle_lines[0].chars().count());
        assert_eq!(live_lines[1].chars().count(), idle_lines[0].chars().count());
        assert!(live_lines[0].ends_with("112 Kbps ↑"));
        assert!(live_lines[1].ends_with("3.6 Mbps ↓"));
        assert_eq!(panel_label(false, rates, SpeedUnit::Megabytes), None);
    }
}
