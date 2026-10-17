use iran_split_core::CoreError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::info;

/// `CREATE_NO_WINDOW`: `PowerShell` must not flash a console behind the UI.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Snapshot {
    pub proxy_enable: u32,
    pub proxy_server: String,
}

#[must_use]
pub fn points_at_hiddify(snapshot: &Snapshot, host: &str, port: u16) -> bool {
    if snapshot.proxy_enable == 0 {
        return false;
    }
    iran_split_core::proxy_server_matches(&snapshot.proxy_server, host, port)
}

pub fn snapshot_path(user_data_dir: &Path) -> PathBuf {
    user_data_dir.join("system-proxy-snapshot.json")
}

/// Clears `HKCU` Internet Settings when they point at Hiddify.
///
/// # Errors
///
/// Returns a platform error when `PowerShell` cannot read or write the keys.
pub async fn clear_if_hiddify(
    host: &str,
    port: u16,
    persist: &Path,
) -> Result<Option<Snapshot>, CoreError> {
    let Some(snapshot) = read_current().await? else {
        return Ok(None);
    };
    if !points_at_hiddify(&snapshot, host, port) {
        return Ok(None);
    }
    write_snapshot(persist, &snapshot)?;
    apply_disabled().await?;
    info!(
        event = "system_proxy.cleared",
        section = "system_proxy",
        initiator = "windows_platform_backend",
        cause = "hiddify_endpoint",
        trace_route = "engine->windows_platform_backend->system_proxy",
        "cleared a Hiddify system proxy without logging its endpoint"
    );
    Ok(Some(snapshot))
}

/// Restores a previously cleared Hiddify system proxy.
///
/// # Errors
///
/// Returns a platform error when `PowerShell` cannot write the keys.
pub async fn restore(persist: &Path) -> Result<(), CoreError> {
    let Some(snapshot) = read_snapshot(persist)? else {
        return Ok(());
    };
    apply_snapshot(&snapshot).await?;
    let _ = std::fs::remove_file(persist);
    info!(
        event = "system_proxy.restored",
        section = "system_proxy",
        initiator = "windows_platform_backend",
        cause = "resume",
        trace_route = "engine->windows_platform_backend->system_proxy",
        "restored the previous Hiddify system proxy"
    );
    Ok(())
}

async fn read_current() -> Result<Option<Snapshot>, CoreError> {
    let output = powershell(
        "$p = Get-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings'; Write-Output (\"$($p.ProxyEnable)|$($p.ProxyServer)\")",
    )
    .await?;
    let Some((enable, server)) = output.split_once('|') else {
        return Ok(None);
    };
    let proxy_enable = enable.trim().parse().unwrap_or(0);
    Ok(Some(Snapshot {
        proxy_enable,
        proxy_server: server.trim().to_owned(),
    }))
}

async fn apply_disabled() -> Result<(), CoreError> {
    powershell(&write_script(0, "")).await?;
    Ok(())
}

async fn apply_snapshot(snapshot: &Snapshot) -> Result<(), CoreError> {
    powershell(&write_script(
        snapshot.proxy_enable,
        &snapshot.proxy_server.replace('\'', "''"),
    ))
    .await?;
    Ok(())
}

fn write_script(enable: u32, server: &str) -> String {
    format!(
        "Set-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings' -Name ProxyEnable -Value {enable}; Set-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings' -Name ProxyServer -Value '{server}'; Add-Type -TypeDefinition 'using System.Runtime.InteropServices; public class W {{ [DllImport(\"wininet.dll\")] public static extern bool InternetSetOption(System.IntPtr h, int o, System.IntPtr b, int l); }}'; [W]::InternetSetOption([IntPtr]::Zero, 39, [IntPtr]::Zero, 0) | Out-Null; [W]::InternetSetOption([IntPtr]::Zero, 37, [IntPtr]::Zero, 0) | Out-Null"
    )
}

async fn powershell(script: &str) -> Result<String, CoreError> {
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .map_err(|error| CoreError::Platform(error.to_string()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(CoreError::Platform(
            "could not update the Windows system proxy".into(),
        ))
    }
}

fn write_snapshot(path: &Path, snapshot: &Snapshot) -> Result<(), CoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| CoreError::Platform(error.to_string()))?;
    }
    std::fs::write(
        path,
        serde_json::to_vec(snapshot).map_err(|error| CoreError::Platform(error.to_string()))?,
    )
    .map_err(|error| CoreError::Platform(error.to_string()))
}

fn read_snapshot(path: &Path) -> Result<Option<Snapshot>, CoreError> {
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(path).map_err(|error| CoreError::Platform(error.to_string()))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| CoreError::Platform(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_enabled_hiddify_endpoint() {
        let snapshot = Snapshot {
            proxy_enable: 1,
            proxy_server: "127.0.0.1:12334".into(),
        };
        assert!(points_at_hiddify(&snapshot, "127.0.0.1", 12334));
        assert!(!points_at_hiddify(&snapshot, "10.0.0.1", 8080));
        assert!(points_at_hiddify(
            &Snapshot {
                proxy_enable: 1,
                proxy_server: "localhost:10808".into(),
            },
            "127.0.0.1",
            10808
        ));
    }

    #[test]
    fn ignores_disabled_or_corporate_proxy() {
        assert!(!points_at_hiddify(
            &Snapshot {
                proxy_enable: 0,
                proxy_server: "127.0.0.1:12334".into(),
            },
            "127.0.0.1",
            12334
        ));
        assert!(!points_at_hiddify(
            &Snapshot {
                proxy_enable: 1,
                proxy_server: "proxy.corp.example:8080".into(),
            },
            "127.0.0.1",
            12334
        ));
    }
}
