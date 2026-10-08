//! Tauri lifecycle bridge. Native handles remain inside the boundary crate.
use crate::{services, show_main};
use iran_split_taskbar::Display;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{AppHandle, Manager, Runtime};

pub struct Taskbar(Mutex<Option<Display>>, AtomicBool);
static ADAPTER_STATE: Mutex<&'static str> = Mutex::new("disabled");

pub fn adapter_state() -> &'static str {
    *ADAPTER_STATE.lock().expect("adapter state")
}

pub fn setup<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
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
    app.manage(Taskbar(Mutex::new(display), AtomicBool::new(false)));
    if let Ok(config) = services(app).and_then(|services| {
        services
            .config_store
            .load_or_create()
            .map_err(|_| "configuration_unavailable".to_owned())
    }) {
        set_enabled(app, config.behavior.show_taskbar_speed);
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

pub fn set_enabled<R: Runtime>(app: &AppHandle<R>, enabled: bool) {
    if let Some(state) = app.try_state::<Taskbar>() {
        state.1.store(enabled, Ordering::Release);
        if let Some(display) = state.0.lock().expect("taskbar state").as_ref() {
            display.set_enabled(enabled);
            tracing::info!(
                event = "taskbar.preference_applied",
                section = "taskbar",
                initiator = "settings",
                cause = "saved_preference",
                trace_route = "settings->taskbar",
                enabled,
                "taskbar preference applied"
            );
        }
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
            if state.0.lock().expect("taskbar state").is_none() {
                break;
            }
            let enabled = state.1.load(Ordering::Acquire);
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
            if let Some(display) = state.0.lock().expect("taskbar state").as_ref() {
                display.update(
                    rates.map(|rates| rates.upload),
                    rates.map(|rates| rates.download),
                );
            };
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
            trace_route = "ip_helper->shared_stats->taskbar",
            source = "physical_interface",
            "physical interface source changed"
        );
    } else {
        tracing::warn!(
            event = "traffic.interface_sample_unavailable",
            section = "traffic",
            initiator = "interface_sampler",
            cause = status,
            trace_route = "ip_helper->shared_stats->taskbar",
            "physical interface statistics unavailable"
        );
    }
}

pub fn shutdown<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<Taskbar>() {
        drop(state.0.lock().expect("taskbar state").take());
    }
}
