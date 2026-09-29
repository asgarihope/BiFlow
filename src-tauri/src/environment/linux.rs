//! Linux collector: procfs, sysfs, and a few bounded commands.

use super::{
    adapter_kind, apps::redact_addresses, classify_host, Adapter, DnsServers, EnvironmentReport,
    FirewallProfile, KernelInfo, Listener, PlatformData, ProxySetting, RouteInfo, RoutingInfo,
    ServiceState,
};
#[cfg(target_os = "linux")]
use super::{proxy_setting, run_command};
use std::{collections::HashMap, net::IpAddr};

/// Route lines per family; a host with hundreds of container routes must
/// not flood the log.
const MAX_ROUTE_LINES: usize = 60;

#[cfg(target_os = "linux")]
pub(super) async fn collect_platform(
    report: &mut EnvironmentReport,
    tun_name: &str,
) -> PlatformData {
    let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    report.system.os_name = os_release_field(&os_release, "PRETTY_NAME");
    report.system.os_version = os_release_field(&os_release, "VERSION_ID");
    report.system.kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|text| text.trim().to_owned());
    report.system.desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    report.system.session_type = std::env::var("XDG_SESSION_TYPE").ok();
    report.install.elevated = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|text| effective_uid(&text))
        .map(|uid| uid == 0);
    report.install.uptime_minutes = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok())
        .map(|seconds| {
            // Uptime is non-negative and far below u64::MAX minutes.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let minutes = (seconds / 60.0) as u64;
            minutes
        });

    report.firewall = linux_firewall().await;
    report.adapters = linux_adapters();
    report.default_routes = std::fs::read_to_string("/proc/net/route")
        .map(|text| parse_proc_route(&text))
        .unwrap_or_default();
    report.dns_servers = linux_dns();
    report.system_proxy = linux_system_proxy(&mut report.collection_errors).await;
    report.routing = linux_routing(&mut report.collection_errors).await;
    report.kernel = Some(linux_kernel(tun_name, &report.adapters).await);

    let owners = socket_owners();
    let mut listeners = Vec::new();
    for (file, v6) in [("/proc/net/tcp", false), ("/proc/net/tcp6", true)] {
        if let Ok(text) = std::fs::read_to_string(file) {
            listeners.extend(parse_proc_tcp_listeners(&text, v6).into_iter().map(
                |(mut listener, inode)| {
                    listener.process = owners.get(&inode).cloned();
                    listener
                },
            ));
        }
    }
    PlatformData {
        processes: linux_processes(),
        listeners,
        services: linux_services().await,
    }
}

fn effective_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[cfg(target_os = "linux")]
async fn linux_routing(errors: &mut Vec<String>) -> RoutingInfo {
    let mut routing = RoutingInfo::default();
    for (family, flag) in [("v4", "-4"), ("v6", "-6")] {
        match run_command("ip", &[flag, "-o", "rule", "show"]).await {
            Ok(text) => routing.rules.extend(parse_ip_rules(&text, family)),
            Err(error) => errors.push(error),
        }
        match run_command("ip", &[flag, "-o", "route", "show", "table", "all"]).await {
            Ok(text) => {
                let (routes, defaults) = parse_ip_routes(&text, family);
                routing.routes.extend(routes);
                if family == "v6" {
                    routing.ipv6_default_interfaces = defaults;
                }
            }
            Err(error) => errors.push(error),
        }
    }
    routing
}

fn parse_ip_rules(text: &str, family: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .map(|line| format!("{family} {}", redact_addresses(&line)))
        .collect()
}

/// Keeps routes that decide where traffic leaves: everything except
/// local/broadcast/multicast bookkeeping, link-local, and container veths.
/// Returns the redacted lines and the interfaces holding a default route.
fn parse_ip_routes(text: &str, family: &str) -> (Vec<String>, Vec<String>) {
    let mut routes = Vec::new();
    let mut defaults = Vec::new();
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        let first = line.split(' ').next().unwrap_or_default();
        if line.is_empty()
            || matches!(first, "local" | "broadcast" | "multicast" | "anycast")
            || first.starts_with("fe80:")
            || first.starts_with("ff00:")
        {
            continue;
        }
        let device = line
            .split(' ')
            .skip_while(|token| *token != "dev")
            .nth(1)
            .unwrap_or_default()
            .to_owned();
        if device.starts_with("veth") {
            continue;
        }
        if first == "default" && !device.is_empty() && !defaults.contains(&device) {
            defaults.push(device);
        }
        if routes.len() < MAX_ROUTE_LINES {
            routes.push(format!("{family} {}", redact_addresses(&line)));
        }
    }
    (routes, defaults)
}

#[cfg(target_os = "linux")]
async fn linux_kernel(tun_name: &str, adapters: &[Adapter]) -> KernelInfo {
    let read_u8 = |path: String| {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| text.trim().parse::<u8>().ok())
    };
    let mut interfaces = vec!["all".to_owned(), "default".to_owned()];
    interfaces.extend(
        adapters
            .iter()
            .filter(|adapter| adapter.name == tun_name || adapter.name.starts_with("tun-"))
            .map(|adapter| adapter.name.clone()),
    );
    let rp_filter = interfaces
        .into_iter()
        .filter_map(|interface| {
            let value = read_u8(format!("/proc/sys/net/ipv4/conf/{interface}/rp_filter"))?;
            Some((interface, value))
        })
        .collect();
    let modules = std::fs::read_to_string("/proc/modules")
        .map(|text| loaded_modules(&text))
        .unwrap_or_default();
    let network_manager_active = run_command(
        "nmcli",
        &["-t", "-f", "TYPE", "connection", "show", "--active"],
    )
    .await
    .map(|text| {
        let mut kinds: Vec<String> = text
            .lines()
            .map(|line| line.trim().to_owned())
            .filter(|line| !line.is_empty())
            .collect();
        kinds.sort();
        kinds.dedup();
        kinds
    })
    .unwrap_or_default();
    KernelInfo {
        tun_device: std::path::Path::new("/dev/net/tun").exists(),
        ipv6_disabled: read_u8("/proc/sys/net/ipv6/conf/all/disable_ipv6".into())
            .map(|value| value == 1),
        ip_forward: read_u8("/proc/sys/net/ipv4/ip_forward".into()),
        rp_filter,
        modules,
        network_manager_active,
    }
}

fn loaded_modules(text: &str) -> Vec<String> {
    const INTERESTING: &[&str] = &[
        "tun",
        "wireguard",
        "ovpn",
        "ovpn_dco_v2",
        "nf_tables",
        "ip_tables",
    ];
    text.lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| INTERESTING.contains(name))
        .map(str::to_owned)
        .collect()
}

#[cfg(target_os = "linux")]
async fn linux_services() -> Vec<ServiceState> {
    let mut services: Vec<ServiceState> = run_command(
        "systemctl",
        &[
            "list-units",
            "--type=service",
            "--state=active",
            "--no-legend",
            "--plain",
        ],
    )
    .await
    .map(|text| parse_active_units(&text))
    .unwrap_or_default();
    // The helper must be reported even when it is not running.
    let helper = run_command("systemctl", &["is-active", "iran-split-helper"])
        .await
        .unwrap_or_else(|_| "unknown".into());
    services.retain(|service| service.name != "iran-split-helper");
    services.push(ServiceState {
        name: "iran-split-helper".into(),
        state: helper.trim().to_owned(),
    });
    services
}

fn parse_active_units(text: &str) -> Vec<ServiceState> {
    text.lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter_map(|unit| unit.strip_suffix(".service"))
        .map(|name| ServiceState {
            name: name.to_owned(),
            state: "active".into(),
        })
        .collect()
}

/// Maps listening-socket inodes to the owning process name. Only processes
/// of the same user are readable, which covers the user's proxy clients.
#[cfg(target_os = "linux")]
fn socket_owners() -> HashMap<u64, String> {
    let mut owners = HashMap::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return owners;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.bytes().all(|byte| byte.is_ascii_digit()))
        {
            continue;
        }
        let Ok(fds) = std::fs::read_dir(path.join("fd")) else {
            continue;
        };
        let mut comm = None;
        for fd in fds.filter_map(Result::ok) {
            let Some(inode) = std::fs::read_link(fd.path())
                .ok()
                .and_then(|target| socket_inode(&target.to_string_lossy()))
            else {
                continue;
            };
            let name = comm.get_or_insert_with(|| {
                std::fs::read_to_string(path.join("comm"))
                    .map(|name| name.trim().to_owned())
                    .unwrap_or_default()
            });
            owners.entry(inode).or_insert_with(|| name.clone());
        }
    }
    owners
}

fn socket_inode(target: &str) -> Option<u64> {
    target
        .strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

fn os_release_field(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .map(|value| value.trim().trim_matches('"').to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(target_os = "linux")]
async fn linux_firewall() -> Vec<FirewallProfile> {
    let units = ["ufw", "firewalld", "nftables", "iptables"];
    let mut args = vec!["is-active"];
    args.extend(units);
    let states = run_command("systemctl", &args).await.unwrap_or_default();
    let mut profiles: Vec<FirewallProfile> = units
        .iter()
        .zip(states.lines().chain(std::iter::repeat("unknown")))
        .map(|(unit, state)| FirewallProfile {
            name: format!("{unit}.service"),
            enabled: state.trim() == "active",
        })
        .collect();
    if let Ok(text) = std::fs::read_to_string("/etc/ufw/ufw.conf") {
        profiles.push(FirewallProfile {
            name: "ufw.conf".into(),
            enabled: text
                .lines()
                .any(|line| line.trim().eq_ignore_ascii_case("ENABLED=yes")),
        });
    }
    profiles
}

#[cfg(target_os = "linux")]
fn linux_adapters() -> Vec<Adapter> {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
        return Vec::new();
    };
    let mut adapters: Vec<Adapter> = entries
        .filter_map(Result::ok)
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let base = entry.path();
            let read = |file: &str| {
                std::fs::read_to_string(base.join(file))
                    .map(|text| text.trim().to_owned())
                    .unwrap_or_default()
            };
            let devtype = read("uevent")
                .lines()
                .find_map(|line| line.strip_prefix("DEVTYPE=").map(str::to_owned))
                .unwrap_or_default();
            let description = if base.join("tun_flags").exists() {
                format!("tun {devtype}").trim().to_owned()
            } else if devtype.is_empty() {
                match read("type").as_str() {
                    "1" => "ethernet".to_owned(),
                    "772" => "loopback".to_owned(),
                    other => format!("type {other}"),
                }
            } else {
                devtype
            };
            Adapter {
                kind: adapter_kind(&name, &description),
                name,
                description,
                status: read("operstate"),
            }
        })
        .collect();
    adapters.sort_by(|left, right| left.name.cmp(&right.name));
    adapters
}

fn parse_proc_route(text: &str) -> Vec<RouteInfo> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let interface = *fields.first()?;
            let destination = u32::from_str_radix(fields.get(1)?, 16).ok()?;
            let gateway = u32::from_str_radix(fields.get(2)?, 16).ok()?;
            let metric = fields.get(6)?.parse::<u32>().ok();
            let mask = u32::from_str_radix(fields.get(7)?, 16).ok()?;
            let prefix_len = mask.count_ones();
            if prefix_len > 1 {
                return None;
            }
            let destination = std::net::Ipv4Addr::from(destination.swap_bytes());
            Some(RouteInfo {
                prefix: format!("{destination}/{prefix_len}"),
                interface: interface.to_owned(),
                gateway: classify_host(&std::net::Ipv4Addr::from(gateway.swap_bytes()).to_string()),
                route_metric: metric,
                interface_metric: None,
            })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn linux_dns() -> Vec<DnsServers> {
    let mut servers = Vec::new();
    for (label, file) in [
        ("resolv.conf", "/etc/resolv.conf"),
        (
            "systemd-resolved-upstream",
            "/run/systemd/resolve/resolv.conf",
        ),
    ] {
        if let Ok(text) = std::fs::read_to_string(file) {
            servers.push(DnsServers {
                interface: label.into(),
                servers: parse_resolv_conf(&text),
            });
        }
    }
    servers
}

fn parse_resolv_conf(text: &str) -> Vec<&'static str> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("nameserver"))
        .map(|server| classify_host(server.trim()))
        .collect()
}

#[cfg(target_os = "linux")]
async fn linux_system_proxy(errors: &mut Vec<String>) -> Vec<ProxySetting> {
    let mut settings = Vec::new();
    match run_command("gsettings", &["get", "org.gnome.system.proxy", "mode"]).await {
        Ok(mode) => {
            let mode = mode.trim().trim_matches('\'').to_owned();
            if mode == "manual" {
                for schema in ["http", "https", "socks"] {
                    let path = format!("org.gnome.system.proxy.{schema}");
                    let host = run_command("gsettings", &["get", &path, "host"])
                        .await
                        .unwrap_or_default();
                    let port = run_command("gsettings", &["get", &path, "port"])
                        .await
                        .unwrap_or_default();
                    let host = host.trim().trim_matches('\'');
                    if !host.is_empty() {
                        settings.push(proxy_setting(
                            &format!("gnome:{schema}"),
                            &format!("{host}:{}", port.trim()),
                            None,
                        ));
                    }
                }
            } else {
                settings.push(ProxySetting {
                    source: "gnome".into(),
                    enabled: mode == "auto",
                    scheme: None,
                    host: None,
                    port: None,
                    detail: Some(format!("mode={mode}")),
                });
            }
        }
        Err(error) => errors.push(error),
    }
    if let Some(config) = dirs::config_dir() {
        if let Ok(text) = std::fs::read_to_string(config.join("kioslaverc")) {
            if let Some(kind) = kde_proxy_type(&text) {
                settings.push(ProxySetting {
                    source: "kde".into(),
                    enabled: kind != "0",
                    scheme: None,
                    host: None,
                    port: None,
                    detail: Some(format!("proxy_type={kind}")),
                });
            }
        }
    }
    settings
}

/// `ProxyType` under `[Proxy Settings]`: 0 none, 1 manual, 2 PAC, 3 WPAD,
/// 4 environment variables.
fn kde_proxy_type(text: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_section = line == "[Proxy Settings]";
        } else if in_section {
            if let Some(value) = line.strip_prefix("ProxyType=") {
                let value = value.trim();
                if value.bytes().all(|byte| byte.is_ascii_digit()) && !value.is_empty() {
                    return Some(value.to_owned());
                }
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn linux_processes() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.bytes().all(|byte| byte.is_ascii_digit()))
        })
        .filter_map(|entry| std::fs::read_to_string(entry.path().join("comm")).ok())
        .map(|name| name.trim().to_owned())
        .collect()
}

/// `/proc/net/tcp{,6}` rows in state `0A` (LISTEN) with their socket
/// inode. Addresses are stored as little-endian 32-bit words.
fn parse_proc_tcp_listeners(text: &str, v6: bool) -> Vec<(Listener, u64)> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.get(3) != Some(&"0A") {
                return None;
            }
            let (address, port) = fields.get(1)?.split_once(':')?;
            let port = u16::from_str_radix(port, 16).ok()?;
            let ip: IpAddr = if v6 {
                let mut bytes = [0_u8; 16];
                for (index, chunk) in bytes.chunks_mut(4).enumerate() {
                    let word =
                        u32::from_str_radix(address.get(index * 8..index * 8 + 8)?, 16).ok()?;
                    chunk.copy_from_slice(&word.swap_bytes().to_be_bytes());
                }
                IpAddr::from(bytes)
            } else {
                IpAddr::from(
                    u32::from_str_radix(address, 16)
                        .ok()?
                        .swap_bytes()
                        .to_be_bytes(),
                )
            };
            let inode = fields
                .get(9)
                .and_then(|inode| inode.parse().ok())
                .unwrap_or(0);
            Some((
                Listener {
                    port,
                    address: classify_host(&ip.to_string()),
                    process: None,
                },
                inode,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kde_proxy_type_reads_only_the_proxy_section() {
        let text = "[General]\nProxyType=Settings\n[Proxy Settings]\nProxyType=1\n";
        assert_eq!(kde_proxy_type(text).as_deref(), Some("1"));
        assert_eq!(kde_proxy_type("[General]\nProxyType=2\n"), None);
    }

    #[test]
    fn proc_route_keeps_default_and_split_default_routes() {
        let text = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n\
                    wlp1s0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\n\
                    tun0\t00000000\t00000000\t0001\t0\t0\t0\t00000080\n\
                    wlp1s0\t0001A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\n";
        let routes = parse_proc_route(text);
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].prefix, "0.0.0.0/0");
        assert_eq!(routes[0].gateway, "private");
        assert_eq!(routes[0].route_metric, Some(600));
        assert_eq!(routes[1].prefix, "0.0.0.0/1");
    }

    #[test]
    fn proc_tcp_listeners_decode_little_endian_addresses() {
        let v4 = "  sl  local_address rem_address   st\n   0: 0100007F:3039 00000000:0000 0A\n   1: 0100007F:3039 0100007F:1234 01\n";
        let listeners = parse_proc_tcp_listeners(v4, false);
        assert_eq!(listeners.len(), 1);
        assert_eq!(listeners[0].0.port, 12345);
        assert_eq!(listeners[0].0.address, "loopback");
        let v6 = "  sl local\n   0: 00000000000000000000000001000000:0050 00000000000000000000000000000000:0000 0A\n";
        let listeners = parse_proc_tcp_listeners(v6, true);
        assert_eq!(listeners[0].0.address, "loopback");
        assert_eq!(listeners[0].0.port, 80);
    }

    #[test]
    fn os_release_and_resolv_conf_are_parsed() {
        assert_eq!(
            os_release_field("NAME=x\nPRETTY_NAME=\"Ubuntu 24.04 LTS\"\n", "PRETTY_NAME")
                .as_deref(),
            Some("Ubuntu 24.04 LTS")
        );
        assert_eq!(
            parse_resolv_conf("nameserver 127.0.0.53\noptions edns0\nnameserver 1.1.1.1\n"),
            vec!["loopback", "public_ip"]
        );
    }

    #[test]
    fn listener_inode_is_parsed() {
        let text = "  sl local rem st tx rx tr tm retr uid timeout inode\n   0: 0100007F:3039 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 424242 1\n";
        assert_eq!(parse_proc_tcp_listeners(text, false)[0].1, 424_242);
        assert_eq!(socket_inode("socket:[424242]"), Some(424_242));
        assert_eq!(socket_inode("/dev/null"), None);
    }

    #[test]
    fn policy_rules_and_routes_are_redacted_and_filtered() {
        let rules = parse_ip_rules(
            "9000:\tfrom all to 198.18.0.0/30 lookup 2022\n9001:\tfrom all iif clash-iran goto 9010\n",
            "v4",
        );
        assert_eq!(
            rules,
            vec![
                "v4 9000: from all to <fake_ip>/30 lookup 2022",
                "v4 9001: from all iif clash-iran goto 9010",
            ]
        );
        let (routes, defaults) = parse_ip_routes(
            "default via 192.168.100.1 dev enp15s0 proto dhcp src 192.168.100.185 metric 20100\n\
             default via 198.18.0.2 dev clash-iran table 2022\n\
             local 127.0.0.1 dev lo table local\n\
             fe80::/64 dev enp15s0 proto kernel\n\
             172.17.0.0/16 dev veth1234 proto kernel\n",
            "v6",
        );
        assert_eq!(routes.len(), 2);
        assert!(routes[0].starts_with("v6 default via <private> dev enp15s0"));
        assert!(!routes.join(" ").contains("192.168"));
        assert_eq!(defaults, vec!["enp15s0", "clash-iran"]);
    }

    #[test]
    fn status_uid_modules_and_units_are_parsed() {
        assert_eq!(effective_uid("Name:\tx\nUid:\t1000\t0\t0\t0\n"), Some(0));
        assert_eq!(
            loaded_modules("wireguard 1 0 - Live\nsnd 2 0 - Live\ntun 3 0 - Live\n"),
            vec!["wireguard", "tun"]
        );
        assert_eq!(
            parse_active_units(
                "tailscaled.service loaded active running Tailscale\nfoo.socket loaded\n"
            )
            .len(),
            1
        );
    }
}
