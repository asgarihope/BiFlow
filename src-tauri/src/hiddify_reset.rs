//! Backs up and clears Hiddify's regenerable state, then relaunches it.
//!
//! Hiddify occasionally opens on a blank window because a generated profile
//! config or its runtime state is corrupt. Removing those files fixes it, but
//! `db.sqlite` holds the subscriptions and `shared_preferences.json` holds the
//! settings, so neither is touched. Everything removed is moved to a
//! timestamped folder under `BiFlow`'s data directory first, never deleted.

use super::deps::{first_existing, hiddify_candidates};
use super::services;
use chrono::Utc;
use serde::Serialize;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};
use tauri::AppHandle;
use tokio::process::Command;
use tracing::{error, info};

/// Directory names cleared on a fresh start. Hiddify rebuilds both.
const CLEARED_DIRECTORIES: [&str; 2] = ["configs", "data"];
/// Files Hiddify needs to keep your subscriptions and settings.
pub const PRESERVED_FILES: [&str; 2] = ["db.sqlite", "shared_preferences.json"];
/// A directory only counts as Hiddify's if it holds one of these.
const MARKERS: [&str; 4] = ["db.sqlite", "shared_preferences.json", "configs", "app.log"];

#[derive(Debug, Clone, Serialize)]
pub struct FreshStartReport {
    pub data_dir: String,
    pub backup_dir: String,
    pub cleared: Vec<String>,
    pub preserved: Vec<String>,
    pub stopped: bool,
    pub started: bool,
}

/// Clears Hiddify's regenerable state and starts it again.
///
/// # Errors
///
/// Returns an error when Hiddify's data directory cannot be found, a file
/// cannot be moved into the backup, or the executable cannot be launched.
pub async fn fresh_start(app: &AppHandle) -> Result<FreshStartReport, String> {
    let services = services(app)?;
    let data_dir = resolve_data_dir().ok_or_else(|| {
        format!(
            "could not find Hiddify's data folder (looked in {})",
            describe(&data_dir_candidates())
        )
    })?;
    let executable = first_existing(&hiddify_candidates(&services.paths.data))
        .ok_or_else(|| "Hiddify is not installed".to_owned())?;
    info!(
        event = "hiddify.fresh_start_requested",
        section = "hiddify_reset",
        initiator = "tauri_command",
        cause = "user_requested",
        trace_route = "ui->tauri_command->fresh_start",
        "clearing Hiddify runtime state"
    );

    // Hiddify holds these files open, and a second launch would only focus the
    // existing window, so the running instance has to go first.
    let stopped = stop_hiddify(&executable).await;

    let backup_dir = services
        .paths
        .data
        .join("backups")
        .join(format!("hiddify-{}", Utc::now().format("%Y%m%d-%H%M%S")));
    let entries = plan_cleared_entries(&data_dir);
    if !entries.is_empty() {
        fs::create_dir_all(&backup_dir)
            .map_err(|error| format!("cannot create {}: {error}", backup_dir.display()))?;
    }
    let mut cleared = Vec::new();
    for entry in &entries {
        let name = file_name(entry)?;
        move_into_backup(entry, &backup_dir.join(&name))?;
        cleared.push(name);
    }

    let started = start_hiddify(&executable)?;
    let preserved = PRESERVED_FILES
        .iter()
        .filter(|name| data_dir.join(name).exists())
        .map(|name| (*name).to_owned())
        .collect();
    info!(
        event = "hiddify.fresh_start_completed",
        section = "hiddify_reset",
        initiator = "tauri_command",
        cause = "none",
        trace_route = "ui->tauri_command->fresh_start->launched",
        cleared = cleared.len(),
        stopped,
        "Hiddify restarted on clean runtime state"
    );

    Ok(FreshStartReport {
        data_dir: data_dir.display().to_string(),
        backup_dir: backup_dir.display().to_string(),
        cleared,
        preserved,
        stopped,
        started,
    })
}

/// Flutter's `getApplicationSupportDirectory` resolves differently per OS, so
/// probe the known layouts and accept only a directory that looks like
/// Hiddify's. Never guess: the caller deletes what this returns.
#[must_use]
pub fn data_dir_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(data) = dirs::data_dir() {
        candidates.push(data.join("hiddify"));
        candidates.push(data.join("Hiddify"));
        candidates.push(data.join("hiddify/hiddify"));
        candidates.push(data.join("Hiddify/Hiddify"));
        candidates.push(data.join("app.hiddify.com"));
    }
    if let Some(local) = dirs::data_local_dir() {
        candidates.push(local.join("hiddify"));
        candidates.push(local.join("Hiddify"));
        candidates.push(local.join("hiddify/hiddify"));
    }
    // On Linux both dirs resolve to $XDG_DATA_HOME, and `dedup` only drops
    // neighbours, so the same path would be probed and reported twice.
    let mut seen = HashSet::new();
    candidates.retain(|path| seen.insert(path.clone()));
    candidates
}

#[must_use]
pub fn resolve_data_dir() -> Option<PathBuf> {
    data_dir_candidates()
        .into_iter()
        .find(|path| is_hiddify_dir(path))
}

/// Guards against emptying an unrelated folder that merely shares the name.
#[must_use]
pub fn is_hiddify_dir(path: &Path) -> bool {
    path.is_dir() && MARKERS.iter().any(|marker| path.join(marker).exists())
}

/// Only regenerable state: the generated profile configs, the runtime folder,
/// and the logs. `db.sqlite` and `shared_preferences.json` are never listed.
#[must_use]
pub fn plan_cleared_entries(data_dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = CLEARED_DIRECTORIES
        .iter()
        .map(|name| data_dir.join(name))
        .filter(|path| path.is_dir())
        .collect();
    let Ok(listing) = fs::read_dir(data_dir) else {
        return entries;
    };
    let mut logs: Vec<PathBuf> = listing
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_log_file(path))
        .collect();
    logs.sort();
    entries.extend(logs);
    entries
}

fn is_log_file(path: &Path) -> bool {
    !PRESERVED_FILES
        .iter()
        .any(|kept| path.file_name().is_some_and(|name| name == *kept))
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("log"))
}

fn file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{} has no file name", path.display()))
}

/// `rename` fails across filesystems, so fall back to copy plus remove.
fn move_into_backup(source: &Path, destination: &Path) -> Result<(), String> {
    if fs::rename(source, destination).is_ok() {
        return Ok(());
    }
    if source.is_dir() {
        copy_dir(source, destination)?;
        fs::remove_dir_all(source)
    } else {
        fs::copy(source, destination).and_then(|_| fs::remove_file(source))
    }
    .map_err(|error| format!("cannot move {}: {error}", source.display()))
}

fn copy_dir(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn start_hiddify(executable: &Path) -> Result<bool, String> {
    Command::new(executable)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(false)
        .spawn()
        .map_err(|error| format!("cannot start Hiddify: {error}"))?;
    Ok(true)
}

/// Terminates only processes whose executable is the discovered Hiddify
/// binary. Matching on a name or a command-line substring would also hit
/// terminals and editors that merely mention Hiddify.
#[cfg(target_os = "linux")]
async fn stop_hiddify(executable: &Path) -> bool {
    let pids = linux_pids_for(executable);
    if pids.is_empty() {
        return false;
    }
    let mut command = Command::new("kill");
    command.arg("-TERM");
    for pid in &pids {
        command.arg(pid.to_string());
    }
    match command.status().await {
        Ok(status) if status.success() => {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            true
        }
        Ok(status) => {
            tracing::warn!(
                event = "hiddify.stop_failed",
                section = "hiddify_reset",
                initiator = "tauri_command",
                cause = "kill_exit_status",
                trace_route = "ui->tauri_command->fresh_start->stop",
                exit_code = status.code().unwrap_or(-1),
                "could not terminate the running Hiddify"
            );
            false
        }
        Err(error) => {
            error!(
                event = "hiddify.stop_failed",
                section = "hiddify_reset",
                initiator = "tauri_command",
                cause = %error,
                trace_route = "ui->tauri_command->fresh_start->stop",
                "could not run kill"
            );
            false
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_pids_for(executable: &Path) -> Vec<u32> {
    let Ok(listing) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids = Vec::new();
    for entry in listing.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if fs::read_link(entry.path().join("exe")).is_ok_and(|exe| exe == executable) {
            pids.push(pid);
        }
    }
    pids
}

#[cfg(target_os = "windows")]
async fn stop_hiddify(executable: &Path) -> bool {
    let Some(image) = executable
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
    else {
        return false;
    };
    // /T also ends the sing-box child the app supervises.
    match Command::new("taskkill")
        .args(["/IM", &image, "/T", "/F"])
        .creation_flags(super::helper_install::CREATE_NO_WINDOW)
        .status()
        .await
    {
        Ok(status) if status.success() => {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            true
        }
        Ok(_) => false,
        Err(error) => {
            error!(
                event = "hiddify.stop_failed",
                section = "hiddify_reset",
                initiator = "tauri_command",
                cause = %error,
                trace_route = "ui->tauri_command->fresh_start->stop",
                "could not run taskkill"
            );
            false
        }
    }
}

#[cfg(target_os = "macos")]
async fn stop_hiddify(executable: &Path) -> bool {
    // macOS has no `/proc`; `pgrep -f` matches the executable path against the
    // full command line of every process, the same intent as the Linux
    // `/proc/<pid>/exe` symlink check.
    let pids = macos_pids_for(executable).await;
    if pids.is_empty() {
        return false;
    }
    let mut command = Command::new("kill");
    command.arg("-TERM");
    for pid in &pids {
        command.arg(pid.to_string());
    }
    match command.status().await {
        Ok(status) if status.success() => {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            true
        }
        Ok(status) => {
            tracing::warn!(
                event = "hiddify.stop_failed",
                section = "hiddify_reset",
                initiator = "tauri_command",
                cause = "kill_exit_status",
                trace_route = "ui->tauri_command->fresh_start->stop",
                exit_code = status.code().unwrap_or(-1),
                "could not terminate the running Hiddify"
            );
            false
        }
        Err(error) => {
            error!(
                event = "hiddify.stop_failed",
                section = "hiddify_reset",
                initiator = "tauri_command",
                cause = %error,
                trace_route = "ui->tauri_command->fresh_start->stop",
                "could not run kill"
            );
            false
        }
    }
}

#[cfg(target_os = "macos")]
async fn macos_pids_for(executable: &Path) -> Vec<u32> {
    let needle = executable.to_string_lossy().into_owned();
    let Ok(output) = Command::new("pgrep").args(["-f", &needle]).output().await else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect()
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
async fn stop_hiddify(_executable: &Path) -> bool {
    false
}

fn describe(candidates: &[PathBuf]) -> String {
    candidates
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::{is_hiddify_dir, is_log_file, plan_cleared_entries, PRESERVED_FILES};
    use std::fs;

    fn hiddify_layout() -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path();
        fs::create_dir_all(root.join("configs")).expect("configs");
        fs::create_dir_all(root.join("data/AppSettings.db")).expect("data");
        fs::write(root.join("configs/profile.json"), "{}").expect("config");
        fs::write(root.join("data/clash.db"), "db").expect("clash");
        fs::write(root.join("db.sqlite"), "profiles").expect("sqlite");
        fs::write(root.join("shared_preferences.json"), "{}").expect("prefs");
        fs::write(root.join("app.log"), "log").expect("app log");
        fs::write(root.join("CrashReport-.log"), "").expect("crash log");
        directory
    }

    #[test]
    fn clears_regenerable_state_and_keeps_subscriptions() {
        let directory = hiddify_layout();
        let names: Vec<String> = plan_cleared_entries(directory.path())
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert!(names.contains(&"configs".to_owned()));
        assert!(names.contains(&"data".to_owned()));
        assert!(names.contains(&"app.log".to_owned()));
        assert!(names.contains(&"CrashReport-.log".to_owned()));
        for kept in PRESERVED_FILES {
            assert!(
                !names.contains(&kept.to_owned()),
                "{kept} must survive a fresh start"
            );
        }
    }

    #[test]
    fn plans_nothing_for_an_already_clean_directory() {
        let directory = tempfile::tempdir().expect("tempdir");
        fs::write(directory.path().join("db.sqlite"), "profiles").expect("sqlite");
        assert!(plan_cleared_entries(directory.path()).is_empty());
    }

    #[test]
    fn only_a_directory_holding_hiddify_state_is_accepted() {
        let hiddify = hiddify_layout();
        assert!(is_hiddify_dir(hiddify.path()));

        let unrelated = tempfile::tempdir().expect("tempdir");
        fs::write(unrelated.path().join("notes.txt"), "hello").expect("write");
        assert!(!is_hiddify_dir(unrelated.path()));
        assert!(!is_hiddify_dir(&unrelated.path().join("missing")));
    }

    #[test]
    fn probes_each_candidate_directory_once() {
        let candidates = super::data_dir_candidates();
        let mut unique = candidates.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(candidates.len(), unique.len(), "{candidates:?}");
        assert!(!candidates.is_empty());
    }

    #[test]
    fn log_matching_ignores_case_and_spares_kept_files() {
        let directory = tempfile::tempdir().expect("tempdir");
        assert!(is_log_file(&directory.path().join("box.LOG")));
        assert!(is_log_file(&directory.path().join("CrashReport-.log")));
        assert!(!is_log_file(&directory.path().join("db.sqlite")));
        assert!(!is_log_file(
            &directory.path().join("shared_preferences.json")
        ));
    }
}
