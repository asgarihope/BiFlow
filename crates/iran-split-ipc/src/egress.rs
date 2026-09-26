//! What a Windows side-tunnel adapter can actually deliver.
//!
//! Mihomo binds `interface-name`. A TAP adapter with gateway `0.0.0.0` makes
//! Windows ARP for every destination, so sites never open. DCO can still
//! answer on an on-link route. The helper and the egress probe share this
//! classification so a successful route install and a live probe agree.

#![allow(clippy::must_use_candidate)]

use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
#[cfg(windows)]
use std::{
    net::{IpAddr, SocketAddr},
    process::Command,
    sync::mpsc,
    thread,
    time::Duration,
};

#[cfg(windows)]
const PROBE_TARGET: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 80);
#[cfg(windows)]
const PROBE_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultRouteKind {
    Missing,
    OnLink,
    Gateway,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgressProbe {
    pub adapter: String,
    pub address: String,
    pub route: DefaultRouteKind,
    pub ok: bool,
    pub detail: String,
}

/// Index of `adapter` in `netsh interface ipv4 show interfaces` output.
pub fn parse_interface_index(interfaces_table: &str, adapter: &str) -> Option<u32> {
    interfaces_table.lines().find_map(|line| {
        let line = line.trim();
        let (index, rest) = line.split_once(char::is_whitespace)?;
        let index = index.parse::<u32>().ok()?;
        let name = interface_name(rest)?;
        (name == adapter).then_some(index)
    })
}

/// Kind of the `0.0.0.0/0` row for `interface_index` in `netsh interface ipv4 show route`.
pub fn default_route_kind(route_table: &str, interface_index: u32) -> DefaultRouteKind {
    let Some(nexthop) = route_table.lines().find_map(|line| {
        let after = line.split("0.0.0.0/0").nth(1)?;
        let after = after.trim();
        let (index, nexthop) = after.split_once(char::is_whitespace)?;
        (index.parse::<u32>().ok() == Some(interface_index)).then(|| nexthop.trim().to_owned())
    }) else {
        return DefaultRouteKind::Missing;
    };
    if nexthop_is_gateway(&nexthop) {
        DefaultRouteKind::Gateway
    } else {
        DefaultRouteKind::OnLink
    }
}

/// `true` when `netsh interface ipv4 show addresses` has no usable gateway.
pub fn adapter_gateway_is_on_link(addresses: &str) -> bool {
    let Some(gateway) = addresses.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("Default Gateway:")?;
        Some(rest.trim())
    }) else {
        return true;
    };
    !nexthop_is_gateway(gateway)
}

/// IPv4 address from `netsh interface ipv4 show addresses`.
pub fn adapter_ipv4(addresses: &str) -> Option<Ipv4Addr> {
    addresses.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("IP Address:")?;
        rest.trim().parse().ok()
    })
}

/// `interface-name` values from a Mihomo config. The controller secret is ignored.
pub fn mihomo_interface_names(config_yaml: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in config_yaml.lines() {
        let Some(rest) = line.trim().strip_prefix("interface-name:") else {
            continue;
        };
        let name = rest.trim().trim_matches('"').trim();
        if !name.is_empty() && !names.iter().any(|existing| existing == name) {
            names.push(name.to_owned());
        }
    }
    names
}

pub fn explain_egress(
    adapter: &str,
    address: &str,
    route: DefaultRouteKind,
    connected: bool,
    failure: &str,
) -> String {
    let route_text = match route {
        DefaultRouteKind::OnLink => "on-link gateway 0.0.0.0",
        DefaultRouteKind::Gateway => "a real gateway",
        DefaultRouteKind::Missing => "no default route",
    };
    if connected {
        return format!(
            "{adapter} ({address}) can reach 1.1.1.1:80. Its default route uses {route_text}."
        );
    }
    let failure = if failure.is_empty() {
        "the connection failed"
    } else {
        failure
    };
    if route == DefaultRouteKind::OnLink && adapter.to_ascii_lowercase().contains("tap") {
        format!(
            "{adapter} ({address}) has an {route_text}. Connecting to 1.1.1.1:80 {failure}. A TAP adapter cannot deliver packets without the OpenVPN gateway, so sites on this client do not open."
        )
    } else {
        format!("{adapter} ({address}) uses {route_text}. Connecting to 1.1.1.1:80 {failure}.")
    }
}

/// Probes every `interface-name` in a Mihomo config.
pub fn probe_config(config_yaml: &str) -> Vec<EgressProbe> {
    let names = mihomo_interface_names(config_yaml);
    if names.is_empty() {
        return vec![EgressProbe {
            adapter: String::new(),
            address: String::new(),
            route: DefaultRouteKind::Missing,
            ok: false,
            detail: "Mihomo has no interface-name, so no side-tunnel adapter can be probed. Connect the client first.".into(),
        }];
    }
    names.iter().map(|name| probe_adapter(name)).collect()
}

pub fn probe_adapter(adapter: &str) -> EgressProbe {
    #[cfg(windows)]
    {
        probe_adapter_windows(adapter)
    }
    #[cfg(not(windows))]
    {
        EgressProbe {
            adapter: adapter.to_owned(),
            address: String::new(),
            route: DefaultRouteKind::Missing,
            ok: false,
            detail: "client egress probe runs on Windows".into(),
        }
    }
}

#[cfg(windows)]
fn probe_adapter_windows(adapter: &str) -> EgressProbe {
    let addresses = netsh_text(&[
        "interface",
        "ipv4",
        "show",
        "addresses",
        &format!("name={adapter}"),
    ]);
    let interfaces = netsh_text(&["interface", "ipv4", "show", "interfaces"]);
    let routes = netsh_text(&["interface", "ipv4", "show", "route"]);
    let address = adapter_ipv4(&addresses);
    let route = if adapter_gateway_is_on_link(&addresses) {
        match parse_interface_index(&interfaces, adapter) {
            Some(index) => default_route_kind(&routes, index),
            None => DefaultRouteKind::OnLink,
        }
    } else {
        DefaultRouteKind::Gateway
    };
    let address_text = address.map(|ip| ip.to_string()).unwrap_or_default();
    let (connected, failure) = match address {
        Some(ip) => match connect_from(ip) {
            Ok(()) => (true, String::new()),
            Err(error) => (false, error),
        },
        None => (false, "the adapter has no IPv4 address".into()),
    };
    let detail = explain_egress(adapter, &address_text, route, connected, &failure);
    EgressProbe {
        adapter: adapter.to_owned(),
        address: address_text,
        route,
        ok: connected,
        detail,
    }
}

#[cfg(windows)]
fn netsh_text(args: &[&str]) -> String {
    Command::new("netsh")
        .args(args)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

#[cfg(windows)]
fn connect_from(local: Ipv4Addr) -> Result<(), String> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let outcome = connect_from_runtime(local);
        let _ = sender.send(outcome);
    });
    match receiver.recv_timeout(PROBE_TIMEOUT + Duration::from_secs(1)) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err("timed out".into()),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err("the probe thread stopped".into()),
    }
}

#[cfg(windows)]
fn connect_from_runtime(local: Ipv4Addr) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async move {
        let socket = tokio::net::TcpSocket::new_v4().map_err(|error| error.to_string())?;
        socket
            .bind(SocketAddr::new(IpAddr::V4(local), 0))
            .map_err(|error| format!("could not bind {local}: {error}"))?;
        match tokio::time::timeout(PROBE_TIMEOUT, socket.connect(PROBE_TARGET)).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(error)) => Err(connect_failure(&error)),
            Err(_) => Err("timed out".into()),
        }
    })
}

#[cfg(windows)]
fn connect_failure(error: &std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::TimedOut {
        "timed out".into()
    } else {
        error.to_string()
    }
}

fn nexthop_is_gateway(nexthop: &str) -> bool {
    matches!(nexthop.parse::<Ipv4Addr>(), Ok(ip) if !ip.is_unspecified())
}

fn interface_name(rest: &str) -> Option<&str> {
    // `disconnected` contains the substring `connected`, so it has to win.
    let marker = if rest.contains("disconnected") {
        "disconnected"
    } else if rest.contains("connected") {
        "connected"
    } else {
        return None;
    };
    let name = rest.split_once(marker)?.1.trim();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::{
        adapter_gateway_is_on_link, adapter_ipv4, default_route_kind, explain_egress,
        mihomo_interface_names, parse_interface_index, DefaultRouteKind,
    };

    const INTERFACES: &str = "\
Idx     Met         MTU          State                Name
---  ----------  ----------  ------------  ---------------------------
 54           0        9000  connected     clash-iran
 13          25        1500  connected     OpenVPN Connect DCO Adapter
 56          25        1500  connected     OpenVPN TAP-Windows6
 61          25        1500  disconnected  OpenVPN Data Channel Offload
";

    const ROUTES: &str = "\
No       Manual    0    0.0.0.0/0                  54  198.18.0.2
No       Manual    9000  0.0.0.0/0                  56  OpenVPN TAP-Windows6
No       Manual    9000  0.0.0.0/0                  13  OpenVPN Connect DCO Adapter
";

    #[test]
    fn classifies_tap_on_link_and_tun_gateway_routes() {
        assert_eq!(
            parse_interface_index(INTERFACES, "OpenVPN TAP-Windows6"),
            Some(56)
        );
        assert_eq!(default_route_kind(ROUTES, 56), DefaultRouteKind::OnLink);
        assert_eq!(default_route_kind(ROUTES, 54), DefaultRouteKind::Gateway);
        assert_eq!(default_route_kind(ROUTES, 13), DefaultRouteKind::OnLink);
        assert_eq!(default_route_kind(ROUTES, 9), DefaultRouteKind::Missing);
    }

    #[test]
    fn reads_the_adapter_address_and_on_link_gateway() {
        let addresses = "\
    IP Address:                           10.138.172.26
    Default Gateway:                      0.0.0.0
";
        assert_eq!(
            adapter_ipv4(addresses),
            Some("10.138.172.26".parse().expect("address"))
        );
        assert!(adapter_gateway_is_on_link(addresses));
        assert!(!adapter_gateway_is_on_link(
            "    Default Gateway:                      10.138.172.1\n"
        ));
    }

    #[test]
    fn explains_a_tap_blackhole_without_a_gateway_address() {
        let detail = explain_egress(
            "OpenVPN TAP-Windows6",
            "10.138.172.26",
            DefaultRouteKind::OnLink,
            false,
            "timed out",
        );
        assert!(detail.contains("on-link gateway 0.0.0.0"));
        assert!(detail.contains("timed out"));
        assert!(detail.contains("OpenVPN gateway"));
        assert!(!detail.contains("10.138.172.1"));
    }

    #[test]
    fn reads_interface_names_and_skips_the_secret() {
        let yaml = "secret: hidden\ninterface-name: OpenVPN TAP-Windows6\ninterface-name: OpenVPN TAP-Windows6\n";
        assert_eq!(
            mihomo_interface_names(yaml),
            vec!["OpenVPN TAP-Windows6".to_owned()]
        );
    }
}
