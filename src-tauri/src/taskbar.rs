//! Tauri lifecycle bridge. Native handles remain inside the boundary crate.
use crate::{services, show_main, traffic};
use iran_split_taskbar::Display;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, Runtime};

pub struct Taskbar(Mutex<Option<Display>>);

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
    app.manage(Taskbar(Mutex::new(display)));
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
}

pub fn set_enabled<R: Runtime>(app: &AppHandle<R>, enabled: bool) {
    if let Some(state) = app.try_state::<Taskbar>() {
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

pub async fn publish<R: Runtime>(
    app: &AppHandle<R>,
    tracker: &mut traffic::RateTracker,
    totals: traffic::TrafficTotals,
) {
    let available = if let Ok(services) = services(app) {
        services.traffic.lock().await.sample_available()
    } else {
        tracing::warn!(
            event = "taskbar.sample_unavailable",
            section = "taskbar",
            initiator = "traffic_sampler",
            cause = "services_unavailable",
            trace_route = "traffic->taskbar",
            "taskbar sample unavailable"
        );
        false
    };
    let (upload, download) = tracker.sample(totals, available, std::time::Instant::now());
    if let Some(state) = app.try_state::<Taskbar>() {
        if let Some(display) = state.0.lock().expect("taskbar state").as_ref() {
            display.update(upload, download);
        }
    }
}

pub fn shutdown<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<Taskbar>() {
        drop(state.0.lock().expect("taskbar state").take());
    }
}
