//! macOS system proxy snapshot/restore via `networksetup`.
//!
//! Hiddify can publish an `HTTP`/`SOCKS` proxy on the macOS network services so
//! browsers bypass the TUN split routing. `BiFlow` snapshots that state, clears
//! it while connected, and restores it on disconnect — the same contract as
//! the GNOME/KDE backend on Linux, but driven by `networksetup` per network
//! service (Wi-Fi, Ethernet, …).

use iran_split_core::CoreError;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::process::Command;
use tracing::{info, warn};

/// Absolute path: a GUI `PATH` can omit `/usr/sbin`, and then every
/// `networksetup` call silently looks like "no proxy configured".
const NETWORKSETUP: &str = "/usr/sbin/networksetup";

/// One network service's proxy configuration, as reported by `networksetup`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ServiceProxy {
    pub service: String,
    pub web_enabled: bool,
    pub secure_web_enabled: bool,
    pub socks_enabled: bool,
    pub http_host: String,
    pub http_port: String,
    pub https_host: String,
    pub https_port: String,
    pub socks_host: String,
    pub socks_port: String,
}

/// A snapshot of every network service's proxy state, used to restore later.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub mode: String,
    pub services: Vec<ServiceProxy>,
}

#[must_use]
pub fn points_at_hiddify(snapshot: &Snapshot, host: &str, port: u16) -> bool {
    if matches!(snapshot.mode.as_str(), "none" | "") {
        return false;
    }
    let needle = format!("{host}:{port}");
    snapshot.services.iter().any(|service| {
        service.web_enabled && format!("{}:{}", service.http_host, service.http_port) == needle
            || service.secure_web_enabled
                && format!("{}:{}", service.https_host, service.https_port) == needle
            || service.socks_enabled
                && format!("{}:{}", service.socks_host, service.socks_port) == needle
    })
}

pub fn snapshot_path(user_data_dir: &Path) -> PathBuf {
    user_data_dir.join("system-proxy-snapshot.json")
}

/// Clears every network service's proxy when it points at Hiddify.
///
/// # Errors
///
/// Returns a platform error when `networksetup` cannot be read or written.
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
    apply_disabled(&snapshot).await?;
    info!(
        event = "system_proxy.cleared",
        section = "system_proxy",
        initiator = "macos_platform_backend",
        cause = "hiddify_endpoint",
        trace_route = "engine->macos_platform_backend->system_proxy",
        "cleared a Hiddify system proxy without logging its endpoint"
    );
    Ok(Some(snapshot))
}

/// Restores a previously cleared Hiddify system proxy.
///
/// # Errors
///
/// Returns a platform error when `networksetup` cannot reapply the snapshot.
pub async fn restore(persist: &Path) -> Result<(), CoreError> {
    let Some(snapshot) = read_snapshot(persist)? else {
        return Ok(());
    };
    apply_snapshot(&snapshot).await?;
    let _ = std::fs::remove_file(persist);
    info!(
        event = "system_proxy.restored",
        section = "system_proxy",
        initiator = "macos_platform_backend",
        cause = "resume",
        trace_route = "engine->macos_platform_backend->system_proxy",
        "restored the previous Hiddify system proxy"
    );
    Ok(())
}

/// Reads the proxy state of every network service.
async fn read_current() -> Result<Option<Snapshot>, CoreError> {
    if !Path::new(NETWORKSETUP).is_file() {
        return Ok(None);
    }
    let services = list_services().await?;
    let mut captured = Vec::with_capacity(services.len());
    let mut any_enabled = false;
    for service in services {
        let proxy = read_service(&service).await?;
        if proxy.web_enabled || proxy.secure_web_enabled || proxy.socks_enabled {
            any_enabled = true;
        }
        captured.push(proxy);
    }
    Ok(Some(Snapshot {
        mode: if any_enabled {
            "manual".into()
        } else {
            "none".into()
        },
        services: captured,
    }))
}

async fn apply_disabled(snapshot: &Snapshot) -> Result<(), CoreError> {
    // Always off. Replaying each service's saved enabled flag left Hiddify's
    // HTTP/SOCKS proxy in place while "connected", so Safari/Chrome sent
    // `localhost` through Hiddify and got 502 (ADR 0062).
    for service in &snapshot.services {
        set_proxy_state(&service.service, ProxyKind::Web, false).await?;
        set_proxy_state(&service.service, ProxyKind::SecureWeb, false).await?;
        set_proxy_state(&service.service, ProxyKind::Socks, false).await?;
    }
    Ok(())
}

async fn apply_snapshot(snapshot: &Snapshot) -> Result<(), CoreError> {
    for service in &snapshot.services {
        if service.web_enabled {
            set_web_proxy(&service.service, &service.http_host, &service.http_port).await?;
        }
        set_proxy_state(&service.service, ProxyKind::Web, service.web_enabled).await?;
        if service.secure_web_enabled {
            set_secure_web_proxy(&service.service, &service.https_host, &service.https_port)
                .await?;
        }
        set_proxy_state(
            &service.service,
            ProxyKind::SecureWeb,
            service.secure_web_enabled,
        )
        .await?;
        if service.socks_enabled {
            set_socks_proxy(&service.service, &service.socks_host, &service.socks_port).await?;
        }
        set_proxy_state(&service.service, ProxyKind::Socks, service.socks_enabled).await?;
    }
    Ok(())
}

#[derive(Copy, Clone)]
enum ProxyKind {
    Web,
    SecureWeb,
    Socks,
}

impl ProxyKind {
    const fn state_flag(self) -> &'static str {
        match self {
            Self::Web => "webproxystate",
            Self::SecureWeb => "securewebproxystate",
            Self::Socks => "socksfirewallproxystate",
        }
    }
}

async fn set_proxy_state(service: &str, kind: ProxyKind, enabled: bool) -> Result<(), CoreError> {
    let flag = format!("-set{}", kind.state_flag());
    run_ok(
        NETWORKSETUP,
        &[&flag, service, if enabled { "on" } else { "off" }],
    )
    .await
}

async fn set_web_proxy(service: &str, host: &str, port: &str) -> Result<(), CoreError> {
    run_ok(NETWORKSETUP, &["-setwebproxy", service, host, port]).await
}

async fn set_secure_web_proxy(service: &str, host: &str, port: &str) -> Result<(), CoreError> {
    run_ok(NETWORKSETUP, &["-setsecurewebproxy", service, host, port]).await
}

async fn set_socks_proxy(service: &str, host: &str, port: &str) -> Result<(), CoreError> {
    run_ok(
        NETWORKSETUP,
        &["-setsocksfirewallproxy", service, host, port],
    )
    .await
}

/// Lists network services, skipping the header line `networksetup` prints.
async fn list_services() -> Result<Vec<String>, CoreError> {
    let output = Command::new(NETWORKSETUP)
        .arg("-listallnetworkservices")
        .output()
        .await
        .map_err(|error| CoreError::Platform(error.to_string()))?;
    if !output.status.success() {
        return Err(CoreError::Platform(
            "could not list macOS network services".into(),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut services = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("An asterisk") {
            continue;
        }
        services.push(trimmed.to_owned());
    }
    Ok(services)
}

async fn read_service(service: &str) -> Result<ServiceProxy, CoreError> {
    let web = read_proxy(service, "-getwebproxy").await?;
    let secure_web = read_proxy(service, "-getsecurewebproxy").await?;
    let socks = read_proxy(service, "-getsocksfirewallproxy").await?;
    Ok(ServiceProxy {
        service: service.to_owned(),
        web_enabled: web.0,
        secure_web_enabled: secure_web.0,
        socks_enabled: socks.0,
        http_host: web.1,
        http_port: web.2,
        https_host: secure_web.1,
        https_port: secure_web.2,
        socks_host: socks.1,
        socks_port: socks.2,
    })
}

/// Parses `networksetup -get<kind> <service>` output:
/// ```text
/// Enabled: No
/// Server: 127.0.0.1
/// Port: 12334
/// Authenticated Proxy Enabled: 0
/// ```
async fn read_proxy(service: &str, kind: &str) -> Result<(bool, String, String), CoreError> {
    let output = Command::new(NETWORKSETUP)
        .args([kind, service])
        .output()
        .await
        .map_err(|error| CoreError::Platform(error.to_string()))?;
    if !output.status.success() {
        warn!(
            event = "system_proxy.read_failed",
            section = "system_proxy",
            initiator = "macos_platform_backend",
            cause = "networksetup_failed",
            trace_route = "engine->macos_platform_backend->system_proxy",
            service,
            kind,
            "networksetup could not read a proxy entry"
        );
        return Ok((false, String::new(), String::new()));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut enabled = false;
    let mut host = String::new();
    let mut port = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("Enabled:") {
            enabled = matches!(value.trim(), "Yes" | "1" | "true");
        } else if let Some(value) = trimmed.strip_prefix("Server:") {
            value.trim().clone_into(&mut host);
        } else if let Some(value) = trimmed.strip_prefix("Port:") {
            value.trim().clone_into(&mut port);
        }
    }
    Ok((enabled, host, port))
}

async fn run_ok(program: &str, args: &[&str]) -> Result<(), CoreError> {
    let status = Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|error| CoreError::Platform(error.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(CoreError::Platform(
            "could not update the macOS system proxy".into(),
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

    fn service(name: &str, host: &str, port: &str) -> ServiceProxy {
        ServiceProxy {
            service: name.into(),
            web_enabled: true,
            secure_web_enabled: false,
            socks_enabled: false,
            http_host: host.into(),
            http_port: port.into(),
            https_host: String::new(),
            https_port: String::new(),
            socks_host: String::new(),
            socks_port: String::new(),
        }
    }

    #[test]
    fn matches_hiddify_web_proxy() {
        let snapshot = Snapshot {
            mode: "manual".into(),
            services: vec![service("Wi-Fi", "127.0.0.1", "12334")],
        };
        assert!(points_at_hiddify(&snapshot, "127.0.0.1", 12334));
        assert!(!points_at_hiddify(&snapshot, "10.0.0.1", 8080));
    }

    #[test]
    fn disabled_mode_never_matches() {
        let snapshot = Snapshot {
            mode: "none".into(),
            services: vec![service("Wi-Fi", "127.0.0.1", "12334")],
        };
        assert!(!points_at_hiddify(&snapshot, "127.0.0.1", 12334));
    }

    #[test]
    fn ignores_unrelated_corporate_proxy() {
        let snapshot = Snapshot {
            mode: "manual".into(),
            services: vec![service("Wi-Fi", "proxy.corp.example", "8080")],
        };
        assert!(!points_at_hiddify(&snapshot, "127.0.0.1", 12334));
    }

    #[test]
    fn apply_disabled_turns_proxies_off_instead_of_replaying_snapshot() {
        // `include_str` of this file would match the assertion itself if we
        // scanned `mod tests`. Only the production half is the contract.
        let production = include_str!("system_proxy.rs")
            .split("mod tests")
            .next()
            .expect("production");
        let body = production
            .split("async fn apply_disabled")
            .nth(1)
            .and_then(|rest| rest.split("async fn apply_snapshot").next())
            .expect("apply_disabled body");
        assert!(
            body.contains("ProxyKind::Web, false")
                && body.contains("ProxyKind::SecureWeb, false")
                && body.contains("ProxyKind::Socks, false"),
            "apply_disabled must force every proxy kind off"
        );
        assert!(
            !body.contains("service.web_enabled"),
            "replaying snapshot enabled flags leaves the Hiddify proxy up"
        );
    }

    #[test]
    fn round_trips_snapshot_json() {
        let snapshot = Snapshot {
            mode: "manual".into(),
            services: vec![service("Wi-Fi", "127.0.0.1", "12334")],
        };
        let bytes = serde_json::to_vec(&snapshot).expect("serialize");
        let back: Snapshot = serde_json::from_slice(&bytes).expect("deserialize");
        assert_eq!(snapshot, back);
    }
}
