use super::{commands, HelperServiceError, HelperSettings, Supervisor};
use iran_split_ipc::{HelloReply, HelperCommand, HelperError, HelperReply, PROTOCOL_VERSION};
use nix::unistd::{chown, Gid, Uid};
use std::{
    collections::HashMap,
    fs,
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
};
use tokio::net::{UnixListener, UnixStream};
use tracing::{info, warn};

/// Snapshot of per-service DNS servers captured before `BiFlow` took over the
/// system resolver, restored on disconnect so the user gets the router DNS
/// back when the TUN is down.
static DNS_SNAPSHOT: Mutex<Option<HashMap<String, Vec<String>>>> = Mutex::new(None);

/// The absolute path to `networksetup`. The helper runs as a launchd daemon
/// with a minimal environment; relying on `PATH` lookup can fail silently and
/// leave the system DNS on the LAN router (which bypasses the TUN).
const NETWORKSETUP: &str = "/usr/sbin/networksetup";

/// Lists every network service `networksetup` reports (skipping the header
/// line and the asterisk-prefixed disabled-service annotation).
fn list_network_services() -> Vec<String> {
    let output = Command::new(NETWORKSETUP)
        .arg("-listallnetworkservices")
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .filter_map(|line| {
            let name = line.trim_start_matches('*').trim();
            if name.is_empty() {
                None
            } else {
                Some(name.to_owned())
            }
        })
        .collect()
}

fn get_dnsservers(service: &str) -> Vec<String> {
    let output = Command::new(NETWORKSETUP)
        .args(["-getdnsservers", service])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("There aren't"))
        .map(str::to_owned)
        .collect()
}

fn set_dnsservers(service: &str, servers: &[String]) {
    let mut args: Vec<String> = vec!["-setdnsservers".into(), service.into()];
    args.extend(servers.iter().cloned());
    let _ = Command::new(NETWORKSETUP).args(args).status();
}

/// Snapshots the current per-service DNS configuration and points every
/// network service at `127.0.0.1` so the system resolver queries Mihomo
/// (which listens on `127.0.0.1:53`). The LAN/router resolver bypasses the
/// TUN and cannot resolve blocked domains.
pub fn apply_system_dns() {
    let mut snapshot = HashMap::new();
    for service in list_network_services() {
        snapshot.insert(service.clone(), get_dnsservers(&service));
        set_dnsservers(&service, &["127.0.0.1".to_owned()]);
    }
    if snapshot.is_empty() {
        warn!(
            event = "helper.dns_apply_empty",
            section = "helper_dns",
            initiator = "helper_process",
            cause = "no_network_services",
            trace_route = "helper_process->networksetup->system_dns",
            "could not enumerate network services; system DNS left unchanged"
        );
    } else {
        info!(
            event = "helper.dns_applied",
            section = "helper_dns",
            initiator = "helper_process",
            cause = "stack_start",
            trace_route = "helper_process->networksetup->system_dns",
            services = snapshot.len(),
            "redirected the macOS system DNS to Mihomo"
        );
    }
    if let Ok(mut guard) = DNS_SNAPSHOT.lock() {
        *guard = Some(snapshot);
    }
}

/// Restores the DNS servers captured by [`apply_system_dns`] for every
/// service, clearing the override when the original had none.
pub fn restore_system_dns() {
    let snapshot = {
        let Ok(mut guard) = DNS_SNAPSHOT.lock() else {
            return;
        };
        guard.take()
    };
    let Some(snapshot) = snapshot else {
        return;
    };
    for (service, servers) in &snapshot {
        set_dnsservers(service, servers);
    }
    info!(
        event = "helper.dns_restored",
        section = "helper_dns",
        initiator = "helper_process",
        cause = "stack_cleanup",
        trace_route = "helper_process->networksetup->system_dns",
        services = snapshot.len(),
        "restored the macOS system DNS"
    );
}

/// Runs the macOS helper service and accepts authenticated local IPC clients.
///
/// # Errors
///
/// Returns an error when configuration, socket setup, or client acceptance
/// fails.
pub async fn run_macos(config_path: &Path) -> Result<(), HelperServiceError> {
    let settings = HelperSettings::load(config_path)?;
    let socket_path = settings.socket_path.clone();
    let socket_parent = socket_path
        .parent()
        .ok_or_else(|| HelperServiceError::UnsafeConfig("socket has no parent".into()))?;
    fs::create_dir_all(socket_parent)?;
    apply_socket_dir_permissions(socket_parent, settings.authorized_gid)?;
    if socket_path.exists() {
        let metadata = fs::symlink_metadata(&socket_path)?;
        if !metadata.file_type().is_socket() {
            return Err(HelperServiceError::UnsafeConfig(
                "refusing to replace a non-socket at socket_path".into(),
            ));
        }
        fs::remove_file(&socket_path)?;
    }
    let listener = UnixListener::bind(&socket_path)?;
    apply_socket_permissions(&socket_path, settings.authorized_gid)?;
    let supervisor = Arc::new(Supervisor::new(settings));
    info!(
        event = "helper.listening",
        section = "helper_ipc",
        initiator = "helper_process",
        cause = "startup_complete",
        trace_route = "helper_process->unix_socket->ipc_listener",
        path = %socket_path.display(),
        "helper listening"
    );

    loop {
        let (stream, _) = listener.accept().await?;
        let supervisor = Arc::clone(&supervisor);
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, supervisor).await {
                warn!(
                    event = "helper.connection_failed",
                    section = "helper_ipc",
                    initiator = "ipc_client",
                    cause = %error,
                    trace_route = "ipc_client->helper_connection->command_handler",
                    "helper connection closed with an error"
                );
            }
        });
    }
}

fn apply_socket_dir_permissions(path: &Path, gid: u32) -> Result<(), HelperServiceError> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o750))?;
    chown_root_group(path, gid)
}

fn apply_socket_permissions(path: &Path, gid: u32) -> Result<(), HelperServiceError> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o660))?;
    chown_root_group(path, gid)
}

fn chown_root_group(path: &Path, gid: u32) -> Result<(), HelperServiceError> {
    if gid == 0 || !nix::unistd::geteuid().is_root() {
        return Ok(());
    }
    chown(path, Some(Uid::from_raw(0)), Some(Gid::from_raw(gid)))
        .map_err(|error| HelperServiceError::Io(std::io::Error::other(error.to_string())))
}

/// Returns the peer's effective UID for audit logging.
///
/// The workspace forbids `unsafe`, so the helper cannot call `getpeereid(2)`
/// directly. Access control is enforced by the socket itself — it is owned by
/// `root:authorized_gid` with mode `0o660`, so only members of the authorized
/// group can connect. The audit identity is therefore the configured
/// authorized UID, which on a single-user macOS install is the desktop user
/// that owns the `BiFlow` profile.
fn peer_uid(supervisor: &Supervisor) -> u32 {
    supervisor.settings().authorized_uid
}

async fn handle_connection(
    mut stream: UnixStream,
    supervisor: Arc<Supervisor>,
) -> Result<(), HelperServiceError> {
    let peer_uid = peer_uid(&supervisor);

    let hello = commands::read_request(&mut stream).await?;
    let HelperCommand::Hello {
        supported_protocols,
        ..
    } = &hello.payload
    else {
        commands::send_reply(
            &mut stream,
            hello.reply(HelperReply::Error(HelperError {
                code: "HELLO_REQUIRED".into(),
                message: "the first command must negotiate the protocol".into(),
                retryable: false,
            })),
        )
        .await?;
        return Ok(());
    };
    if !supported_protocols.contains(&PROTOCOL_VERSION) {
        commands::send_reply(
            &mut stream,
            hello.reply(HelperReply::Error(HelperError {
                code: "PROTOCOL_MISMATCH".into(),
                message: "no supported protocol version overlaps".into(),
                retryable: false,
            })),
        )
        .await?;
        return Ok(());
    }
    commands::send_reply(
        &mut stream,
        hello.reply(HelperReply::Hello(HelloReply {
            helper_version: env!("CARGO_PKG_VERSION").into(),
            selected_protocol: PROTOCOL_VERSION,
            capabilities: vec![
                "runtime_generation_v1".into(),
                "owned_network_cleanup_v1".into(),
                "bounded_logs_v1".into(),
            ],
        })),
    )
    .await?;

    loop {
        let request = match commands::read_request(&mut stream).await {
            Ok(request) => request,
            Err(HelperServiceError::Protocol(iran_split_ipc::ProtocolError::Io(error)))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        request.payload.validate()?;
        commands::send_reply(
            &mut stream,
            commands::execute_audited(&supervisor, request, &peer_uid.to_string()).await,
        )
        .await?;
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_socket_dir_permissions, apply_socket_permissions};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn socket_permission_helpers_set_group_readable_modes() {
        let directory = tempfile::tempdir().expect("tempdir");
        let parent = directory.path();
        let socket = parent.join("helper.sock");
        fs::write(&socket, []).expect("socket fixture");
        apply_socket_dir_permissions(parent, 0).expect("dir mode");
        apply_socket_permissions(&socket, 0).expect("socket mode");
        assert_eq!(
            fs::metadata(parent)
                .expect("parent meta")
                .permissions()
                .mode()
                & 0o777,
            0o750
        );
        assert_eq!(
            fs::metadata(&socket)
                .expect("socket meta")
                .permissions()
                .mode()
                & 0o777,
            0o660
        );
    }
}
