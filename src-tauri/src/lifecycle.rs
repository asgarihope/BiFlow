//! Process exit owns network cleanup; sleep recovery is independent of webview timers.
use iran_split_core::{ComponentPhase, StackPhase, StackSnapshot};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime},
};
use tauri::{AppHandle, Manager, Runtime};
use tracing::{error, info};

#[derive(Default)]
pub struct ExitState {
    pending: AtomicBool,
    ready: AtomicBool,
}

pub fn handle_run_event<R: Runtime>(app: &AppHandle<R>, event: tauri::RunEvent) {
    match event {
        tauri::RunEvent::ExitRequested { code, api, .. } => {
            crate::diagnostics::exit_requested(code);
            handle_exit(app, &api, code);
        }
        tauri::RunEvent::Exit => crate::diagnostics::close_session(),
        _ => {}
    }
}

fn handle_exit<R: Runtime>(app: &AppHandle<R>, request: &tauri::ExitRequestApi, code: Option<i32>) {
    let state = app.state::<ExitState>();
    if state.ready.load(Ordering::SeqCst) {
        return;
    }
    request.prevent_exit();
    if state.pending.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = crate::diagnostics::trace_action(
            "lifecycle",
            "exit_request",
            "disconnect_before_exit",
            async {
                crate::services(&app)?
                    .engine
                    .shutdown()
                    .await
                    .map_err(|cause| cause.to_string())
            },
        )
        .await;
        match result {
            Ok(()) => {
                info!(
                    event = "session.network_cleanup_completed",
                    section = "lifecycle",
                    initiator = "exit_request",
                    cause = "owned_state_removed",
                    trace_route = "application_exit->engine->cleanup",
                    "network detached before exit"
                );
                app.state::<ExitState>().ready.store(true, Ordering::SeqCst);
                crate::diagnostics::flush();
                app.exit(code.unwrap_or(0));
            }
            Err(cause) => {
                error!(event = "session.network_cleanup_failed", section = "lifecycle",
                    initiator = "exit_request", %cause, trace_route = "application_exit->engine->cleanup",
                    "application remains available so network cleanup can be retried");
                app.state::<ExitState>()
                    .pending
                    .store(false, Ordering::SeqCst);
                crate::show_main(&app);
            }
        }
    });
}

#[derive(Default)]
pub struct RecoveryMonitor {
    last_tick: Option<SystemTime>,
    failed_ticks: u8,
    last_attempt: Option<SystemTime>,
    wake_pending: bool,
}

impl RecoveryMonitor {
    pub fn observe(&mut self, now: SystemTime, snapshot: &StackSnapshot, unlocked: bool) -> bool {
        let resumed = self.last_tick.replace(now).is_some_and(|last| {
            now.duration_since(last)
                .is_ok_and(|gap| gap > Duration::from_secs(30))
        });
        if !matches!(snapshot.phase, StackPhase::Running | StackPhase::Degraded) {
            self.failed_ticks = 0;
            self.wake_pending = false;
            return false;
        }
        self.wake_pending |= resumed || unlocked;
        if snapshot.busy.is_some() {
            return false;
        }
        let broken = [&snapshot.mihomo, &snapshot.tun, &snapshot.dns]
            .iter()
            .any(|status| status.phase != ComponentPhase::Running);
        self.failed_ticks = if broken {
            self.failed_ticks.saturating_add(1)
        } else {
            0
        };
        let cooldown = self.last_attempt.is_some_and(|last| {
            now.duration_since(last)
                .is_ok_and(|gap| gap < Duration::from_secs(60))
        });
        if (self.wake_pending || self.failed_ticks >= 2) && !cooldown {
            self.last_attempt = Some(now);
            self.failed_ticks = 0;
            self.wake_pending = false;
            return true;
        }
        false
    }

    /// Exclude health-query/recovery duration from the next timer-gap check.
    pub fn finish_tick(&mut self, now: SystemTime) {
        self.last_tick = Some(now);
    }
}

/// Query only the current session's secure-desktop process. No process list
/// or account information crosses this boundary.
#[cfg(windows)]
pub async fn session_locked() -> Result<bool, &'static str> {
    let mut command = tokio::process::Command::new("powershell.exe");
    command.creation_flags(0x0800_0000).kill_on_drop(true).args([
        "-NoProfile", "-NonInteractive", "-Command",
        "$s=(Get-Process -Id $PID).SessionId; if (Get-Process -Name LogonUI -ErrorAction SilentlyContinue | Where-Object SessionId -eq $s) { 'locked' } else { 'unlocked' }",
    ]);
    let output = tokio::time::timeout(Duration::from_secs(3), command.output())
        .await
        .map_err(|_| "session_query_timeout")?
        .map_err(|_| "session_query_spawn_failed")?;
    if !output.status.success() {
        return Err("session_query_failed");
    }
    parse_session_state(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(any(windows, test))]
fn parse_session_state(output: &str) -> Result<bool, &'static str> {
    match output.trim() {
        "locked" => Ok(true),
        "unlocked" => Ok(false),
        _ => Err("session_query_invalid"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iran_split_core::ComponentStatus;

    fn running() -> StackSnapshot {
        StackSnapshot {
            phase: StackPhase::Running,
            mihomo: ComponentStatus::new(ComponentPhase::Running, None),
            tun: ComponentStatus::new(ComponentPhase::Running, None),
            dns: ComponentStatus::new(ComponentPhase::Running, None),
            ..StackSnapshot::default()
        }
    }

    #[test]
    fn sleep_rebuilds_even_when_controller_reports_a_stale_healthy_tun() {
        let mut monitor = RecoveryMonitor::default();
        let now = SystemTime::UNIX_EPOCH;
        assert!(!monitor.observe(now, &running(), false));
        assert!(monitor.observe(now + Duration::from_secs(90), &running(), false));
        assert!(!monitor.observe(now + Duration::from_secs(130), &running(), false));
    }

    #[test]
    fn persistent_failure_recovers_but_transient_failure_and_idle_intent_do_not() {
        let mut monitor = RecoveryMonitor::default();
        let now = SystemTime::UNIX_EPOCH;
        let mut snapshot = running();
        snapshot.tun.phase = ComponentPhase::Stopped;
        assert!(!monitor.observe(now, &snapshot, false));
        assert!(monitor.observe(now + Duration::from_secs(10), &snapshot, false));
        snapshot.phase = StackPhase::Paused;
        assert!(!monitor.observe(now + Duration::from_secs(100), &snapshot, true));
        snapshot.phase = StackPhase::Stopped;
        assert!(!monitor.observe(now + Duration::from_secs(200), &snapshot, true));
    }

    #[test]
    fn unlock_triggers_recovery_without_a_sleep_gap() {
        let mut monitor = RecoveryMonitor::default();
        let now = SystemTime::UNIX_EPOCH;
        assert!(!monitor.observe(now, &running(), false));
        assert!(monitor.observe(now + Duration::from_secs(10), &running(), true));
        assert_eq!(parse_session_state("locked\r\n"), Ok(true));
        assert_eq!(parse_session_state("unlocked\r\n"), Ok(false));
        assert!(parse_session_state("").is_err());
    }

    #[test]
    fn unlock_waits_for_rule_apply_and_slow_recovery_does_not_look_like_sleep() {
        let mut monitor = RecoveryMonitor::default();
        let now = SystemTime::UNIX_EPOCH;
        let mut snapshot = running();
        snapshot.busy = Some(iran_split_core::LifecycleBusy::ApplyingRules);
        assert!(!monitor.observe(now, &snapshot, true));
        snapshot.busy = None;
        assert!(monitor.observe(now + Duration::from_secs(10), &snapshot, false));
        monitor.finish_tick(now + Duration::from_secs(120));
        assert!(!monitor.observe(now + Duration::from_secs(130), &snapshot, false));
    }
}
