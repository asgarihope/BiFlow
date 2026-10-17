//! Keep the original system DNS until every restore has succeeded. The
//! recovery file is private, root-owned, and never copied into diagnostics.
use super::HelperServiceError;
use std::{collections::BTreeMap, fs, io::Write, net::IpAddr, path::Path};
use tracing::warn;

type Snapshot = BTreeMap<String, Vec<String>>;
const SNAPSHOT_FILE: &str = "system-dns.toml";

#[cfg(target_os = "macos")]
pub(crate) fn apply(runtime: &Path) -> Result<(), HelperServiceError> {
    run_action(runtime, true)
}

#[cfg(target_os = "macos")]
pub(crate) fn restore(runtime: &Path) -> Result<(), HelperServiceError> {
    run_action(runtime, false)
}

#[cfg(target_os = "macos")]
fn run_action(runtime: &Path, apply: bool) -> Result<(), HelperServiceError> {
    let trace_id = uuid::Uuid::new_v4();
    let operation = if apply { "apply" } else { "restore" };
    tracing::info!(event = "helper.dns_started", section = "helper_dns",
        initiator = "helper_process", cause = "dns_lifecycle", %trace_id, operation,
        trace_route = "helper_process->macos_dns->networksetup", "updating macOS system DNS");
    let result = if apply {
        apply_with(
            runtime,
            &mut networksetup,
            &mut scutil_dns,
            &mut default_gateway_text,
        )
    } else {
        restore_with(
            runtime,
            &mut networksetup,
            &mut scutil_dns,
            &mut default_gateway_text,
        )
    };
    if let Ok(services) = &result {
        tracing::info!(event = "helper.dns_completed", section = "helper_dns",
            initiator = "helper_process", cause = "dns_update_succeeded", %trace_id, operation, services,
            trace_route = "helper_process->macos_dns->networksetup", "macOS system DNS updated");
    } else {
        tracing::warn!(event = "helper.dns_failed", section = "helper_dns",
            initiator = "helper_process", cause = "networksetup_or_recovery_file_failed", %trace_id, operation,
            trace_route = "helper_process->macos_dns->networksetup", "macOS DNS recovery remains pending");
    }
    result.map(|_| ())
}

#[cfg(target_os = "macos")]
fn networksetup(args: &[String]) -> Result<String, HelperServiceError> {
    let output = std::process::Command::new("/usr/sbin/networksetup")
        .args(args)
        .env("LC_ALL", "C")
        .output()?;
    if !output.status.success() {
        return Err(HelperServiceError::Process(
            "macOS DNS command failed".into(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn apply_with(
    runtime: &Path,
    command: &mut impl FnMut(&[String]) -> Result<String, HelperServiceError>,
    live_dns: &mut impl FnMut() -> Result<String, HelperServiceError>,
    gateway: &mut impl FnMut() -> Result<String, HelperServiceError>,
) -> Result<usize, HelperServiceError> {
    let output = command(&["-listallnetworkservices".into()])?;
    let services = parse_services(&output);
    if services.is_empty() {
        return Err(HelperServiceError::Process(
            "no active macOS network services".into(),
        ));
    }
    // Reuse the pre-takeover values after a repeated start/helper restart.
    // A previous session may have stored 127.0.0.1; that is our listener,
    // not the machine's resolver, so it must not be written back later.
    let mut snapshot = read_snapshot(runtime)?.unwrap_or_default();
    let (sanitized, discarded) = without_loopback_snapshot(&snapshot);
    if discarded > 0 {
        note_loopback_discarded(discarded);
        snapshot = sanitized;
    }
    for service in &services {
        if snapshot.contains_key(service) {
            continue;
        }
        let output = command(&["-getdnsservers".into(), service.clone()])?;
        let servers = parse_dns(&output)?;
        let recoverable = without_loopback(&servers);
        if recoverable.len() != servers.len() {
            note_loopback_discarded(1);
        }
        snapshot.insert(service.clone(), recoverable);
    }
    write_snapshot(runtime, &snapshot)?;
    for service in &services {
        if let Err(error) = command(&dns_arguments(service, &["127.0.0.1".into()])) {
            if restore_with(runtime, command, live_dns, gateway).is_err() {
                return Err(HelperServiceError::Process(
                    "macOS DNS apply failed; recovery remains pending".into(),
                ));
            }
            return Err(error);
        }
    }
    Ok(services.len())
}

fn restore_with(
    runtime: &Path,
    command: &mut impl FnMut(&[String]) -> Result<String, HelperServiceError>,
    live_dns: &mut impl FnMut() -> Result<String, HelperServiceError>,
    gateway: &mut impl FnMut() -> Result<String, HelperServiceError>,
) -> Result<usize, HelperServiceError> {
    let Some(snapshot) = read_snapshot(runtime)? else {
        return Ok(0);
    };
    let (snapshot, discarded) = without_loopback_snapshot(&snapshot);
    if discarded > 0 {
        note_loopback_discarded(discarded);
        write_snapshot(runtime, &snapshot)?;
    }
    let mut first_error = None;
    for (service, servers) in &snapshot {
        if let Err(error) = command(&dns_arguments(service, servers)) {
            first_error.get_or_insert(error);
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    if still_loopback(command, live_dns, snapshot.keys())? {
        let gateway = parse_default_gateway(&gateway()?);
        let Some(gateway) = gateway else {
            warn!(
                event = "helper.dns_still_loopback",
                section = "helper_dns",
                initiator = "helper_process",
                cause = "no_private_gateway",
                trace_route = "helper_process->macos_dns->restore",
                "macOS DNS still uses the tunnel resolver"
            );
            return Err(HelperServiceError::Process(
                "macOS DNS still uses the tunnel resolver".into(),
            ));
        };
        warn!(
            event = "helper.dns_router_fallback",
            section = "helper_dns",
            initiator = "helper_process",
            cause = "live_resolver_still_loopback",
            services = snapshot.len(),
            trace_route = "helper_process->macos_dns->restore",
            "live macOS DNS stayed on the tunnel resolver; using the LAN gateway"
        );
        for service in snapshot.keys() {
            command(&dns_arguments(service, &[gateway.clone()]))?;
        }
        if still_loopback(command, live_dns, snapshot.keys())? {
            return Err(HelperServiceError::Process(
                "macOS DNS still uses the tunnel resolver".into(),
            ));
        }
    }
    fs::remove_file(runtime.join(SNAPSHOT_FILE))?;
    #[cfg(unix)]
    fs::File::open(runtime)?.sync_all()?;
    Ok(snapshot.len())
}

fn still_loopback<'a>(
    command: &mut impl FnMut(&[String]) -> Result<String, HelperServiceError>,
    live_dns: &mut impl FnMut() -> Result<String, HelperServiceError>,
    services: impl Iterator<Item = &'a String>,
) -> Result<bool, HelperServiceError> {
    if primary_is_loopback(&live_dns()?) {
        return Ok(true);
    }
    for service in services {
        let output = command(&["-getdnsservers".into(), service.clone()])?;
        if parse_dns(&output)?
            .iter()
            .any(|server| is_loopback_resolver(server))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn note_loopback_discarded(services: usize) {
    warn!(
        event = "helper.dns_loopback_discarded",
        section = "helper_dns",
        initiator = "helper_process",
        cause = "tunnel_resolver_in_snapshot",
        services,
        trace_route = "helper_process->macos_dns->snapshot",
        "saved macOS DNS pointed at the tunnel resolver; DHCP will be restored"
    );
}

fn without_loopback_snapshot(snapshot: &Snapshot) -> (Snapshot, usize) {
    let mut discarded = 0usize;
    let mut clean = Snapshot::new();
    for (service, servers) in snapshot {
        let recoverable = without_loopback(servers);
        if recoverable.len() != servers.len() {
            discarded += 1;
        }
        clean.insert(service.clone(), recoverable);
    }
    (clean, discarded)
}

fn without_loopback(servers: &[String]) -> Vec<String> {
    servers
        .iter()
        .filter(|server| !is_loopback_resolver(server))
        .cloned()
        .collect()
}

fn is_loopback_resolver(server: &str) -> bool {
    server
        .parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

/// The first `scutil --dns` resolver is the one unqualified names use.
fn primary_nameservers(scutil_dns: &str) -> Vec<String> {
    let mut servers = Vec::new();
    let mut in_first = false;
    for line in scutil_dns.lines() {
        let line = line.trim();
        if line.starts_with("resolver #") {
            if in_first {
                break;
            }
            in_first = true;
            continue;
        }
        if !in_first {
            continue;
        }
        let Some(rest) = line.strip_prefix("nameserver[") else {
            continue;
        };
        let Some((_, address)) = rest.split_once(':') else {
            continue;
        };
        let address = address.trim();
        if !address.is_empty() {
            servers.push(address.to_owned());
        }
    }
    servers
}

fn primary_is_loopback(scutil_dns: &str) -> bool {
    primary_nameservers(scutil_dns)
        .iter()
        .any(|server| is_loopback_resolver(server))
}

/// A private IPv4 default gateway can answer DNS when DHCP restore does not
/// clear a stuck `127.0.0.1`. The tunnel gateway `198.18.0.1` is not private.
fn parse_default_gateway(output: &str) -> Option<String> {
    for line in output.lines() {
        let Some(rest) = line.trim().strip_prefix("gateway:") else {
            continue;
        };
        let candidate = rest.trim();
        let Ok(addr) = candidate.parse::<IpAddr>() else {
            return None;
        };
        return match addr {
            IpAddr::V4(ip) if ip.is_private() => Some(candidate.to_owned()),
            _ => None,
        };
    }
    None
}

#[cfg(target_os = "macos")]
fn scutil_dns() -> Result<String, HelperServiceError> {
    let output = std::process::Command::new("/usr/sbin/scutil")
        .arg("--dns")
        .env("LC_ALL", "C")
        .output()?;
    if !output.status.success() {
        return Err(HelperServiceError::Process(
            "macOS DNS status could not be read".into(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(target_os = "macos")]
fn default_gateway_text() -> Result<String, HelperServiceError> {
    let output = std::process::Command::new("/sbin/route")
        .args(["-n", "get", "default"])
        .env("LC_ALL", "C")
        .output()?;
    if !output.status.success() {
        return Err(HelperServiceError::Process(
            "macOS default route could not be read".into(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_services(output: &str) -> Vec<String> {
    output
        .lines()
        .skip(1)
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('*'))
        .map(str::to_owned)
        .collect()
}

fn parse_dns(output: &str) -> Result<Vec<String>, HelperServiceError> {
    if output
        .trim()
        .starts_with("There aren't any DNS Servers set")
    {
        return Ok(Vec::new());
    }
    let servers: Vec<String> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    if servers.is_empty()
        || servers
            .iter()
            .any(|server| server.parse::<IpAddr>().is_err())
    {
        return Err(HelperServiceError::Process(
            "unrecognized macOS DNS response".into(),
        ));
    }
    Ok(servers)
}

fn dns_arguments(service: &str, servers: &[String]) -> Vec<String> {
    let mut args = vec!["-setdnsservers".into(), service.into()];
    if servers.is_empty() {
        args.push("Empty".into());
    } else {
        args.extend_from_slice(servers);
    }
    args
}

fn read_snapshot(runtime: &Path) -> Result<Option<Snapshot>, HelperServiceError> {
    let text = match fs::read_to_string(runtime.join(SNAPSHOT_FILE)) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let snapshot: Snapshot = toml::from_str(&text)
        .map_err(|_| HelperServiceError::Process("invalid macOS DNS recovery file".into()))?;
    if snapshot.is_empty()
        || snapshot.iter().any(|(service, servers)| {
            service.is_empty()
                || servers
                    .iter()
                    .any(|server| server.parse::<IpAddr>().is_err())
        })
    {
        return Err(HelperServiceError::Process(
            "invalid macOS DNS recovery values".into(),
        ));
    }
    Ok(Some(snapshot))
}

fn write_snapshot(runtime: &Path, snapshot: &Snapshot) -> Result<(), HelperServiceError> {
    fs::create_dir_all(runtime)?;
    let text = toml::to_string(snapshot)
        .map_err(|_| HelperServiceError::Process("could not encode macOS DNS recovery".into()))?;
    let mut file = tempfile::NamedTempFile::new_in(runtime)?;
    file.write_all(text.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(runtime.join(SNAPSHOT_FILE))
        .map_err(|error| error.error)?;
    #[cfg(unix)]
    fs::File::open(runtime)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        apply_with, dns_arguments, parse_default_gateway, parse_dns, parse_services,
        primary_is_loopback, read_snapshot, restore_with, SNAPSHOT_FILE,
    };
    use crate::HelperServiceError;
    #[cfg(unix)]
    use std::fs;

    fn network_response(args: &[String]) -> Result<String, HelperServiceError> {
        match args[0].as_str() {
            "-listallnetworkservices" => {
                Ok("An asterisk denotes a disabled service.\nWi-Fi\n".into())
            }
            "-getdnsservers" => Ok("There aren't any DNS Servers set on Wi-Fi.\n".into()),
            "-setdnsservers" => Ok(String::new()),
            _ => Err(HelperServiceError::Process(
                "unexpected fixture command".into(),
            )),
        }
    }

    #[test]
    fn dhcp_dns_is_restored_with_empty_and_the_snapshot_is_private() {
        let root = tempfile::tempdir().expect("tempdir");
        apply_with(
            root.path(),
            &mut network_response,
            &mut healthy_resolver,
            &mut no_gateway,
        )
        .expect("apply");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(root.path().join(SNAPSHOT_FILE))
                    .expect("snapshot")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let mut calls = Vec::new();
        restore_with(
            root.path(),
            &mut |args| {
                calls.push(args.to_vec());
                network_response(args)
            },
            &mut healthy_resolver,
            &mut no_gateway,
        )
        .expect("restore");
        assert!(calls
            .iter()
            .any(|call| { call.as_slice() == ["-setdnsservers", "Wi-Fi", "Empty"] }));
        assert!(!calls
            .iter()
            .any(|call| call.iter().any(|part| part == "127.0.0.1")));
        assert!(!root.path().join(SNAPSHOT_FILE).exists());
    }

    #[test]
    fn failed_restore_keeps_recovery_for_a_new_helper() {
        let root = tempfile::tempdir().expect("tempdir");
        apply_with(
            root.path(),
            &mut network_response,
            &mut healthy_resolver,
            &mut no_gateway,
        )
        .expect("apply");
        assert!(restore_with(
            root.path(),
            &mut |_| Err(HelperServiceError::Process("fixture".into())),
            &mut healthy_resolver,
            &mut no_gateway,
        )
        .is_err());
        assert!(root.path().join(SNAPSHOT_FILE).is_file());
        restore_with(
            root.path(),
            &mut network_response,
            &mut healthy_resolver,
            &mut no_gateway,
        )
        .expect("retry");
        assert!(!root.path().join(SNAPSHOT_FILE).exists());
    }

    #[test]
    fn repeated_start_preserves_the_original_resolvers() {
        let root = tempfile::tempdir().expect("tempdir");
        apply_with(
            root.path(),
            &mut |args| {
                if args[0] == "-getdnsservers" {
                    Ok("8.8.8.8\n2001:4860:4860::8888\n".into())
                } else {
                    network_response(args)
                }
            },
            &mut healthy_resolver,
            &mut no_gateway,
        )
        .expect("first start");
        apply_with(
            root.path(),
            &mut |args| {
                assert_ne!(args[0], "-getdnsservers", "must not recapture loopback DNS");
                network_response(args)
            },
            &mut healthy_resolver,
            &mut no_gateway,
        )
        .expect("restarted helper");
        let snapshot = read_snapshot(root.path())
            .expect("snapshot")
            .expect("present");
        assert_eq!(snapshot["Wi-Fi"], vec!["8.8.8.8", "2001:4860:4860::8888"]);
        restore_with(
            root.path(),
            &mut network_response,
            &mut healthy_resolver,
            &mut no_gateway,
        )
        .expect("restore");
    }

    #[test]
    fn partial_apply_rolls_back_every_service() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut restores = Vec::new();
        let result = apply_with(
            root.path(),
            &mut |args| {
                if args[0] == "-listallnetworkservices" {
                    return Ok("header\nWi-Fi\nEthernet\n".into());
                }
                if args[0] == "-setdnsservers" && args[2] == "127.0.0.1" && args[1] == "Ethernet" {
                    return Err(HelperServiceError::Process("fixture".into()));
                }
                if args[0] == "-setdnsservers" && args[2] == "Empty" {
                    restores.push(args[1].clone());
                }
                network_response(args)
            },
            &mut healthy_resolver,
            &mut no_gateway,
        );
        assert!(result.is_err());
        assert_eq!(restores, vec!["Ethernet", "Wi-Fi"]);
        assert!(!root.path().join(SNAPSHOT_FILE).exists());
    }

    #[test]
    fn invalid_resolver_output_stops_before_any_mutation() {
        let root = tempfile::tempdir().expect("tempdir");
        assert!(apply_with(
            root.path(),
            &mut |args| {
                assert_ne!(args[0], "-setdnsservers");
                if args[0] == "-getdnsservers" {
                    Ok("unexpected error text".into())
                } else {
                    network_response(args)
                }
            },
            &mut healthy_resolver,
            &mut no_gateway,
        )
        .is_err());
        assert!(!root.path().join(SNAPSHOT_FILE).exists());
        assert_eq!(
            parse_services("header\nWi-Fi\n*Disabled service\n\n"),
            vec!["Wi-Fi"]
        );
        assert!(parse_dns("invalid").is_err());
        assert_eq!(
            dns_arguments("Wi-Fi", &["1.1.1.1".into()]),
            vec!["-setdnsservers", "Wi-Fi", "1.1.1.1"]
        );
    }

    #[test]
    fn loopback_dns_is_stored_as_dhcp_and_not_written_back() {
        let root = tempfile::tempdir().expect("tempdir");
        apply_with(
            root.path(),
            &mut |args| {
                if args[0] == "-getdnsservers" {
                    Ok("127.0.0.1\n::1\n".into())
                } else {
                    network_response(args)
                }
            },
            &mut healthy_resolver,
            &mut no_gateway,
        )
        .expect("apply");
        let snapshot = read_snapshot(root.path())
            .expect("snapshot")
            .expect("present");
        assert!(snapshot["Wi-Fi"].is_empty());
        let mut sets = Vec::new();
        restore_with(
            root.path(),
            &mut |args| {
                if args[0] == "-setdnsservers" {
                    sets.push(args.to_vec());
                }
                network_response(args)
            },
            &mut healthy_resolver,
            &mut no_gateway,
        )
        .expect("restore");
        assert_eq!(sets, vec![vec!["-setdnsservers", "Wi-Fi", "Empty"]]);
    }

    #[test]
    fn stuck_loopback_resolver_uses_the_lan_gateway() {
        use std::cell::{Cell, RefCell};
        use std::rc::Rc;

        let root = tempfile::tempdir().expect("tempdir");
        apply_with(
            root.path(),
            &mut |args| {
                if args[0] == "-getdnsservers" {
                    Ok("127.0.0.1\n".into())
                } else {
                    network_response(args)
                }
            },
            &mut healthy_resolver,
            &mut no_gateway,
        )
        .expect("apply");
        let using_gateway = Rc::new(Cell::new(false));
        let sets = Rc::new(RefCell::new(Vec::new()));
        restore_with(
            root.path(),
            &mut {
                let using_gateway = Rc::clone(&using_gateway);
                let sets = Rc::clone(&sets);
                move |args| {
                    if args[0] == "-setdnsservers" {
                        sets.borrow_mut().push(args[2..].to_vec());
                        if args[2] == "10.0.0.1" {
                            using_gateway.set(true);
                        }
                    }
                    if args[0] == "-getdnsservers" {
                        return if using_gateway.get() {
                            Ok("10.0.0.1\n".into())
                        } else {
                            Ok("127.0.0.1\n".into())
                        };
                    }
                    Ok(String::new())
                }
            },
            &mut {
                let using_gateway = Rc::clone(&using_gateway);
                move || {
                    if using_gateway.get() {
                        Ok("resolver #1\n  nameserver[0] : 10.0.0.1\n".into())
                    } else {
                        Ok("resolver #1\n  nameserver[0] : 127.0.0.1\n".into())
                    }
                }
            },
            &mut || Ok("   gateway: 10.0.0.1\n".into()),
        )
        .expect("restore");
        let sets = sets.borrow();
        assert!(sets.iter().any(|servers| servers == &["Empty".to_owned()]));
        assert!(sets
            .iter()
            .any(|servers| servers == &["10.0.0.1".to_owned()]));
        assert!(!root.path().join(SNAPSHOT_FILE).exists());
    }

    #[test]
    fn stuck_loopback_without_a_lan_gateway_keeps_recovery() {
        let root = tempfile::tempdir().expect("tempdir");
        apply_with(
            root.path(),
            &mut |args| {
                if args[0] == "-getdnsservers" {
                    Ok("127.0.0.1\n".into())
                } else {
                    network_response(args)
                }
            },
            &mut healthy_resolver,
            &mut no_gateway,
        )
        .expect("apply");
        let error = restore_with(
            root.path(),
            &mut |args| {
                if args[0] == "-getdnsservers" {
                    Ok("127.0.0.1\n".into())
                } else {
                    Ok(String::new())
                }
            },
            &mut || Ok("resolver #1\n  nameserver[0] : 127.0.0.1\n".into()),
            &mut || Ok("   gateway: 198.18.0.1\n".into()),
        )
        .expect_err("tunnel resolver remains");
        assert!(error.to_string().contains("tunnel resolver"));
        let snapshot = read_snapshot(root.path()).expect("snapshot").expect("kept");
        assert!(snapshot["Wi-Fi"].is_empty());
        assert!(!snapshot["Wi-Fi"].iter().any(|server| server == "127.0.0.1"));
    }

    #[test]
    fn live_dns_and_gateway_parsers_ignore_the_tunnel() {
        assert!(primary_is_loopback(
            "resolver #1\n  nameserver[0] : 127.0.0.1\n\nresolver #2\n  domain : local\n  nameserver[0] : 8.8.8.8\n"
        ));
        assert!(!primary_is_loopback(
            "resolver #1\n  nameserver[0] : 9.9.9.9\n"
        ));
        assert_eq!(
            parse_default_gateway("   gateway: 10.0.0.1\n  interface: en0\n").as_deref(),
            Some("10.0.0.1")
        );
        assert_eq!(parse_default_gateway("   gateway: 198.18.0.1\n"), None);
        assert_eq!(parse_default_gateway("   gateway: 8.8.8.8\n"), None);
    }

    // Production DNS callbacks return `Result`. These fixtures match that
    // signature even though the fixture itself cannot fail.
    #[allow(clippy::unnecessary_wraps)]
    fn healthy_resolver() -> Result<String, HelperServiceError> {
        Ok("resolver #1\n  nameserver[0] : 9.9.9.9\n".into())
    }

    #[allow(clippy::unnecessary_wraps)]
    fn no_gateway() -> Result<String, HelperServiceError> {
        Ok(String::new())
    }
}
