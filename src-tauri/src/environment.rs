//! Host environment snapshot for bug reports (ADR 0106).
//!
//! Every `debug.log` must say what else is running on the machine: operating
//! system, firewall and security products, other VPN/proxy clients, adapters,
//! default routes, system and environment proxies, listeners on the ports
//! the app relies on, and how a kubeconfig cluster would be routed. The same
//! build behaves differently per host, and without this context a report is
//! guesswork.
//!
//! Only classifications are logged: no hostnames, usernames, resolver or
//! proxy addresses, process lists beyond known VPN/proxy names, or cluster
//! hosts. Ports are kept because they are what identifies a conflict.

use iran_split_config::{AppConfig, ClientConfig, DefaultRoute};
use iran_split_rules::{Outbound, RuleSet};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tracing::{info, warn};
use uuid::Uuid;

#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
const COMMAND_TIMEOUT: Duration = Duration::from_secs(25);

/// Known VPN, proxy, and tunnel programs. Exact names are compared after
/// lowercasing and removing `.exe`; entries ending in `*` are prefixes.
/// Nothing outside this list is ever logged from the process table.
const KNOWN_VPN_PROCESSES: &[&str] = &[
    "hiddify*",
    "v2rayn*",
    "v2ray",
    "xray",
    "sing-box",
    "clash*",
    "verge-mihomo*",
    "mihomo*",
    "nekoray*",
    "nekobox*",
    "happ",
    "happd",
    "openvpn*",
    "windscribe*",
    "wireguard*",
    "wg-quick",
    "tailscale*",
    "zerotier*",
    "psiphon*",
    "outline*",
    "protonvpn*",
    "nordvpn*",
    "expressvpn*",
    "surfshark*",
    "warp-svc",
    "cloudflare warp",
    "vpnagent",
    "vpnui",
    "forticlient*",
    "fortitray",
    "pangps",
    "pritunl*",
    "vpnclient",
    "lantern*",
    "tor",
    "obfs4proxy",
    "shadowsocks*",
    "ss-local",
    "trojan*",
    "hysteria*",
    "tuic*",
    "naive",
    "geph*",
    "amnezia*",
    "proxifier*",
    "netch",
    "docker*",
    "com.docker.backend",
    "vmmem*",
    "wslservice",
];

#[derive(Debug, Clone, Serialize, Default)]
pub struct EnvironmentReport {
    pub collected_at: String,
    pub collection_ms: u64,
    pub system: SystemInfo,
    pub firewall: Vec<FirewallProfile>,
    pub security_products: Vec<SecurityProduct>,
    pub adapters: Vec<Adapter>,
    pub default_routes: Vec<RouteInfo>,
    pub dns_servers: Vec<DnsServers>,
    pub system_proxy: Vec<ProxySetting>,
    pub env_proxy: Vec<ProxySetting>,
    pub vpn_processes: Vec<String>,
    pub listeners: Vec<Listener>,
    /// `false` when the platform could not enumerate listening sockets, so a
    /// missing client port is unknown rather than closed.
    pub listener_scan: bool,
    pub hosts_file: HostsFileInfo,
    pub kube: KubeInfo,
    pub biflow: BiflowInfo,
    pub findings: Vec<String>,
    pub collection_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SystemInfo {
    pub os: &'static str,
    pub architecture: &'static str,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub os_build: Option<String>,
    pub kernel: Option<String>,
    pub desktop: Option<String>,
    pub session_type: Option<String>,
    pub app_version: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FirewallProfile {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SecurityProduct {
    pub kind: String,
    pub name: String,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Adapter {
    pub name: String,
    pub description: String,
    pub status: String,
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RouteInfo {
    pub prefix: String,
    pub interface: String,
    pub gateway: &'static str,
    pub route_metric: Option<u32>,
    pub interface_metric: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DnsServers {
    pub interface: String,
    pub servers: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProxySetting {
    pub source: String,
    pub enabled: bool,
    pub scheme: Option<String>,
    pub host: Option<&'static str>,
    pub port: Option<u16>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Listener {
    pub port: u16,
    pub address: &'static str,
    pub process: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct HostsFileInfo {
    pub readable: bool,
    pub entries: usize,
    pub localhost: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct KubeInfo {
    pub config_files: usize,
    pub clusters: Vec<KubeCluster>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct KubeCluster {
    pub host: &'static str,
    pub port: Option<u16>,
    pub proxy_url: bool,
    pub route: Option<String>,
    pub route_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct BiflowInfo {
    pub clients: Vec<ClientSummary>,
    pub default_route: String,
    pub mixed_port: u16,
    pub controller_port: u16,
    pub dns_port: u16,
    pub tun_name: String,
    pub direct_dns_preset: String,
    pub stack_phase: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClientSummary {
    pub preset: &'static str,
    pub kind: String,
    pub enabled: bool,
    pub is_default: bool,
    pub host: Option<&'static str>,
    pub port: Option<u16>,
    pub profile_selected: Option<bool>,
}

/// What the desktop knows that the host cannot tell us.
pub struct CollectContext {
    pub app_version: String,
    pub config: Option<AppConfig>,
    pub rules: Option<RuleSet>,
    pub stack_phase: String,
}

/// Raw platform data before the app-aware filtering.
#[derive(Debug, Default)]
struct PlatformData {
    processes: Vec<String>,
    listeners: Vec<Listener>,
}

static IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static LAST_REPORT: OnceLock<Mutex<Option<EnvironmentReport>>> = OnceLock::new();

fn last_slot() -> &'static Mutex<Option<EnvironmentReport>> {
    LAST_REPORT.get_or_init(|| Mutex::new(None))
}

/// Most recent report, for the support bundle.
pub fn last_report() -> Option<EnvironmentReport> {
    last_slot().lock().ok().and_then(|slot| slot.clone())
}

/// Logs the cached report again (for example right after the log was
/// cleared) so the file is never without host context.
pub fn replay_last(trigger: &str) -> bool {
    let Some(report) = last_report() else {
        return false;
    };
    emit(trigger, &report, true);
    true
}

/// Collects and logs a snapshot. A second request while one is running is
/// dropped; the running one already describes the same host.
pub async fn snapshot(trigger: &'static str, context: CollectContext) {
    if IN_FLIGHT.swap(true, Ordering::SeqCst) {
        info!(
            event = "environment.snapshot_skipped",
            section = "environment",
            initiator = "environment_snapshot",
            cause = "collection_in_progress",
            trace_route = "desktop->environment_snapshot",
            trigger,
            "environment snapshot already running"
        );
        return;
    }
    let report = collect(context).await;
    emit(trigger, &report, false);
    if let Ok(mut slot) = last_slot().lock() {
        *slot = Some(report);
    }
    IN_FLIGHT.store(false, Ordering::SeqCst);
}

fn emit(trigger: &str, report: &EnvironmentReport, cached: bool) {
    let details = serde_json::to_string(report)
        .unwrap_or_else(|error| format!("{{\"serialization_error\":\"{error}\"}}"));
    let other_vpn = report.vpn_processes.join(",");
    let findings = report.findings.join(";");
    let firewall = report
        .firewall
        .iter()
        .map(|profile| {
            format!(
                "{}={}",
                profile.name,
                if profile.enabled { "on" } else { "off" }
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    info!(
        event = "environment.snapshot",
        section = "environment",
        initiator = "environment_snapshot",
        cause = trigger,
        trace_id = %Uuid::new_v4(),
        trace_route = "desktop->environment_snapshot->debug.log",
        trigger,
        cached,
        collected_at = %report.collected_at,
        collection_ms = report.collection_ms,
        os = report.system.os,
        os_name = report.system.os_name.as_deref().unwrap_or("unknown"),
        os_version = report.system.os_version.as_deref().unwrap_or("unknown"),
        os_build = report.system.os_build.as_deref().unwrap_or("unknown"),
        firewall = %firewall,
        vpn_processes = %other_vpn,
        findings = %findings,
        details = %details,
        "host environment snapshot"
    );
    for finding in &report.findings {
        warn!(
            event = "environment.finding",
            section = "environment",
            initiator = "environment_snapshot",
            cause = %finding,
            trace_route = "desktop->environment_snapshot->findings",
            trigger,
            "host environment may conflict with BiFlow"
        );
    }
}

async fn collect(context: CollectContext) -> EnvironmentReport {
    let started = Instant::now();
    let mut report = EnvironmentReport {
        collected_at: chrono::Utc::now().to_rfc3339(),
        system: SystemInfo {
            os: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            app_version: context.app_version.clone(),
            ..SystemInfo::default()
        },
        ..EnvironmentReport::default()
    };
    report.biflow = biflow_info(context.config.as_ref(), &context.stack_phase);
    report.env_proxy = process_env_proxies(|name| std::env::var(name).ok());
    report.hosts_file = hosts_file_info(&hosts_path());
    report.kube = kube_info(context.config.as_ref(), context.rules.as_ref());

    let platform = collect_platform(&mut report).await;
    report.vpn_processes = known_vpn_processes(&platform.processes);
    report.listener_scan = !platform.listeners.is_empty();
    let ports = interesting_ports(context.config.as_ref());
    report.listeners = relevant_listeners(platform.listeners, &ports);
    report.findings = findings(&report, context.config.as_ref());
    report.collection_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    report
}

// ---------------------------------------------------------------------------
// Classification helpers (pure, tested on every host).
// ---------------------------------------------------------------------------

/// Reduces an address or hostname to a class that is safe to log.
fn classify_host(host: &str) -> &'static str {
    let trimmed = host.trim().trim_start_matches('[').trim_end_matches(']');
    if trimmed.is_empty() {
        return "empty";
    }
    if trimmed.eq_ignore_ascii_case("localhost") || trimmed.ends_with(".localhost") {
        return "loopback_name";
    }
    match trimmed.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            let [a, b, ..] = ip.octets();
            if ip.is_loopback() {
                "loopback"
            } else if ip.is_unspecified() {
                "unspecified"
            } else if a == 198 && (b == 18 || b == 19) {
                "fake_ip"
            } else if a == 100 && (64..128).contains(&b) {
                "cgnat"
            } else if ip.is_private() {
                "private"
            } else if ip.is_link_local() {
                "link_local"
            } else {
                "public_ip"
            }
        }
        Ok(IpAddr::V6(ip)) => {
            let first = ip.segments()[0];
            if ip.is_loopback() {
                "loopback"
            } else if ip.is_unspecified() {
                "unspecified"
            } else if first & 0xfe00 == 0xfc00 {
                "private"
            } else if first & 0xffc0 == 0xfe80 {
                "link_local"
            } else {
                "public_ip"
            }
        }
        Err(_) => {
            let lower = trimmed.to_ascii_lowercase();
            match lower.rsplit_once('.').map(|(_, tld)| tld) {
                Some("ir") => "domain_ir",
                Some("local" | "lan") | None => "local_name",
                Some(_) => "domain",
            }
        }
    }
}

fn is_loopback_class(class: &str) -> bool {
    matches!(class, "loopback" | "loopback_name" | "unspecified")
}

#[derive(Debug, PartialEq, Eq)]
struct Endpoint {
    scheme: Option<String>,
    host: String,
    port: Option<u16>,
}

/// Parses `scheme://user:pass@host:port/path`, `host:port`, or `[v6]:port`.
fn parse_endpoint(value: &str) -> Option<Endpoint> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (scheme, rest) = match value.split_once("://") {
        Some((scheme, rest)) => (Some(scheme.to_ascii_lowercase()), rest),
        None => (None, value),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let (host, port) = if let Some(stripped) = authority.strip_prefix('[') {
        let (host, tail) = stripped.split_once(']')?;
        (
            host,
            tail.strip_prefix(':').and_then(|port| port.parse().ok()),
        )
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => (host, port.parse().ok()),
            _ => (authority, None),
        }
    };
    if host.is_empty() {
        return None;
    }
    Some(Endpoint {
        scheme,
        host: host.to_owned(),
        port,
    })
}

fn proxy_setting(source: &str, value: &str, detail: Option<String>) -> ProxySetting {
    let endpoint = parse_endpoint(value);
    ProxySetting {
        source: source.to_owned(),
        enabled: endpoint.is_some(),
        scheme: endpoint
            .as_ref()
            .and_then(|endpoint| endpoint.scheme.clone()),
        host: endpoint
            .as_ref()
            .map(|endpoint| classify_host(&endpoint.host)),
        port: endpoint.as_ref().and_then(|endpoint| endpoint.port),
        detail,
    }
}

const PROXY_ENV_NAMES: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
];

/// Proxy variables in the app's own environment. CLI tools such as kubectl,
/// git, and curl honour them before TUN ever sees the connection.
fn process_env_proxies(read: impl Fn(&str) -> Option<String>) -> Vec<ProxySetting> {
    let mut settings = Vec::new();
    for name in PROXY_ENV_NAMES {
        if let Some(value) = read(name).filter(|value| !value.trim().is_empty()) {
            settings.push(proxy_setting(&format!("env:{name}"), &value, None));
        }
    }
    for name in ["NO_PROXY", "no_proxy"] {
        if let Some(value) = read(name).filter(|value| !value.trim().is_empty()) {
            settings.push(no_proxy_setting(&format!("env:{name}"), &value));
        }
    }
    settings
}

fn no_proxy_setting(source: &str, value: &str) -> ProxySetting {
    let entries = value
        .split([',', ';'])
        .filter(|entry| !entry.trim().is_empty())
        .count();
    ProxySetting {
        source: source.to_owned(),
        enabled: true,
        scheme: None,
        host: None,
        port: None,
        detail: Some(format!("entries={entries}")),
    }
}

fn adapter_kind(name: &str, description: &str) -> &'static str {
    let text = format!("{name} {description}").to_ascii_lowercase();
    let rules: &[(&str, &'static str)] = &[
        ("loopback", "loopback"),
        ("wireguard", "wireguard"),
        ("wintun", "wintun"),
        ("ovpn-dco", "openvpn_dco"),
        ("tap-windows", "tap"),
        ("tap-", "tap"),
        ("windscribe", "vpn"),
        ("tailscale", "vpn"),
        ("zerotier", "vpn"),
        ("cloudflare", "vpn"),
        ("fortinet", "vpn"),
        ("cisco", "vpn"),
        ("anyconnect", "vpn"),
        ("pangp", "vpn"),
        ("vpn", "vpn"),
        ("meta", "tun"),
        ("tun", "tun"),
        ("wsl", "wsl"),
        ("hyper-v", "hyperv"),
        ("vethernet", "hyperv"),
        ("docker", "docker"),
        ("veth", "container"),
        ("br-", "bridge"),
        ("vmware", "vmware"),
        ("virtualbox", "virtualbox"),
        ("bluetooth", "bluetooth"),
        ("wi-fi", "wifi"),
        ("wireless", "wifi"),
        ("wlan", "wifi"),
        ("ethernet", "ethernet"),
    ];
    rules
        .iter()
        .find(|(needle, _)| text.contains(needle))
        .map_or("other", |(_, kind)| kind)
}

fn normalize_process(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    lower
        .strip_suffix(".exe")
        .map_or(lower.clone(), str::to_owned)
}

fn is_known_vpn_process(name: &str) -> bool {
    let name = normalize_process(name);
    KNOWN_VPN_PROCESSES.iter().any(|pattern| {
        pattern
            .strip_suffix('*')
            .map_or(name == *pattern, |prefix| name.starts_with(prefix))
    })
}

fn known_vpn_processes(processes: &[String]) -> Vec<String> {
    processes
        .iter()
        .filter(|name| is_known_vpn_process(name))
        .map(|name| normalize_process(name))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn interesting_ports(config: Option<&AppConfig>) -> BTreeSet<u16> {
    let mut ports = BTreeSet::new();
    if let Some(config) = config {
        ports.extend([
            config.mihomo.mixed_port,
            config.mihomo.controller_port,
            config.mihomo.dns_port,
        ]);
        for client in &config.clients {
            if let ClientConfig::LocalProxy { port, .. } = client.config {
                ports.insert(port);
            }
            if let Some(port) = client.spec().default_port {
                ports.insert(port);
            }
        }
    }
    ports.extend([53, 1080, 2080, 7890, 7891, 7897, 9090, 10808, 10809, 12334]);
    ports
}

/// Keeps listeners on ports the app cares about plus any socket owned by a
/// known VPN/proxy process; the rest of the host's sockets are not logged.
fn relevant_listeners(listeners: Vec<Listener>, ports: &BTreeSet<u16>) -> Vec<Listener> {
    listeners
        .into_iter()
        .filter(|listener| {
            ports.contains(&listener.port)
                || listener
                    .process
                    .as_deref()
                    .is_some_and(is_known_vpn_process)
        })
        .map(|mut listener| {
            listener.process = listener.process.map(|name| {
                if is_known_vpn_process(&name) || ports.contains(&listener.port) {
                    normalize_process(&name)
                } else {
                    "other".into()
                }
            });
            listener
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn biflow_info(config: Option<&AppConfig>, stack_phase: &str) -> BiflowInfo {
    let Some(config) = config else {
        return BiflowInfo {
            stack_phase: stack_phase.to_owned(),
            ..BiflowInfo::default()
        };
    };
    let default_id = match config.default_route {
        DefaultRoute::Direct => None,
        DefaultRoute::Client { client_id } => Some(client_id),
    };
    BiflowInfo {
        clients: config
            .clients
            .iter()
            .map(|client| {
                let (host, port, profile_selected) = match &client.config {
                    ClientConfig::LocalProxy { host, port, .. } => {
                        (Some(classify_host(host)), Some(*port), None)
                    }
                    ClientConfig::OwnedSideTunnel { profile_path, .. } => {
                        (None, None, Some(profile_path.is_some()))
                    }
                    ClientConfig::Unsupported => (None, None, None),
                };
                ClientSummary {
                    preset: client.spec().id,
                    kind: format!("{:?}", client.spec().kind).to_ascii_lowercase(),
                    enabled: client.enabled,
                    is_default: default_id == Some(client.id),
                    host,
                    port,
                    profile_selected,
                }
            })
            .collect(),
        default_route: match default_id.and_then(|id| config.client(id)) {
            Some(client) => format!("client:{}", client.spec().id),
            None if default_id.is_some() => "client:missing".into(),
            None => "direct".into(),
        },
        mixed_port: config.mihomo.mixed_port,
        controller_port: config.mihomo.controller_port,
        dns_port: config.mihomo.dns_port,
        tun_name: config.mihomo.tun_name.clone(),
        direct_dns_preset: format!("{:?}", config.mihomo.direct_dns_preset).to_ascii_lowercase(),
        stack_phase: stack_phase.to_owned(),
    }
}

fn hosts_path() -> PathBuf {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        PathBuf::from(root)
            .join("System32")
            .join("drivers")
            .join("etc")
            .join("hosts")
    } else {
        PathBuf::from("/etc/hosts")
    }
}

fn hosts_file_info(path: &Path) -> HostsFileInfo {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HostsFileInfo::default();
    };
    parse_hosts(&text)
}

fn parse_hosts(text: &str) -> HostsFileInfo {
    let mut info = HostsFileInfo {
        readable: true,
        ..HostsFileInfo::default()
    };
    let mut localhost = BTreeSet::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        let mut fields = line.split_whitespace();
        let Some(address) = fields.next() else {
            continue;
        };
        info.entries += 1;
        if fields.any(|name| name.eq_ignore_ascii_case("localhost")) {
            localhost.insert(classify_host(address));
        }
    }
    info.localhost = localhost.into_iter().collect();
    info
}

fn kubeconfig_paths() -> Vec<PathBuf> {
    if let Some(value) = std::env::var_os("KUBECONFIG").filter(|value| !value.is_empty()) {
        return std::env::split_paths(&value).collect();
    }
    dirs::home_dir()
        .map(|home| vec![home.join(".kube").join("config")])
        .unwrap_or_default()
}

fn kube_info(config: Option<&AppConfig>, rules: Option<&RuleSet>) -> KubeInfo {
    let mut info = KubeInfo::default();
    for path in kubeconfig_paths() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        info.config_files += 1;
        info.clusters.extend(parse_kubeconfig(&text, |host| {
            route_for(config, rules, host)
        }));
    }
    info
}

fn route_for(
    config: Option<&AppConfig>,
    rules: Option<&RuleSet>,
    host: &str,
) -> Option<(String, String)> {
    let decision = rules?.decide(host).ok()?;
    let outbound = match decision.outbound {
        Outbound::Direct => "direct".to_owned(),
        Outbound::Client { client_id } => config
            .and_then(|config| config.client(client_id))
            .map_or_else(
                || "client:missing".into(),
                |client| format!("client:{}", client.spec().id),
            ),
    };
    Some((
        outbound,
        format!("{:?}", decision.reason).to_ascii_lowercase(),
    ))
}

/// Extracts `server:` and `proxy-url:` from a kubeconfig without a YAML
/// dependency. Each `cluster:` block holds one server.
fn parse_kubeconfig(
    text: &str,
    route: impl Fn(&str) -> Option<(String, String)>,
) -> Vec<KubeCluster> {
    let mut clusters: Vec<KubeCluster> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim().trim_start_matches("- ").trim();
        if let Some(value) = trimmed.strip_prefix("server:") {
            let value = value.trim().trim_matches(['"', '\'']);
            let endpoint = parse_endpoint(value);
            let host = endpoint
                .as_ref()
                .map_or("", |endpoint| endpoint.host.as_str());
            let decision = endpoint.as_ref().and_then(|_| route(host));
            clusters.push(KubeCluster {
                host: classify_host(host),
                port: endpoint.as_ref().and_then(|endpoint| {
                    endpoint.port.or(match endpoint.scheme.as_deref() {
                        Some("https") => Some(443),
                        Some("http") => Some(80),
                        _ => None,
                    })
                }),
                proxy_url: false,
                route: decision.as_ref().map(|(outbound, _)| outbound.clone()),
                route_reason: decision.map(|(_, reason)| reason),
            });
        } else if trimmed.starts_with("proxy-url:") {
            if let Some(cluster) = clusters.last_mut() {
                cluster.proxy_url = true;
            }
        }
    }
    clusters
}

/// Heuristics that point at the conflicts reports keep hitting. Each entry
/// is a stable `name:detail` string so logs can be grepped across users.
fn findings(report: &EnvironmentReport, config: Option<&AppConfig>) -> Vec<String> {
    let biflow_ports: BTreeSet<u16> = config
        .map(|config| {
            [
                config.mihomo.mixed_port,
                config.mihomo.controller_port,
                config.mihomo.dns_port,
            ]
            .into_iter()
            .collect()
        })
        .unwrap_or_default();
    let mut findings = Vec::new();
    if let Some(config) = config {
        client_findings(report, config, &mut findings);
    }
    port_findings(report, config, &biflow_ports, &mut findings);
    network_findings(report, config, &mut findings);
    host_findings(report, config, &mut findings);
    findings
}

fn client_findings(report: &EnvironmentReport, config: &AppConfig, findings: &mut Vec<String>) {
    for client in config.enabled_clients() {
        let ClientConfig::LocalProxy { port, host, .. } = &client.config else {
            continue;
        };
        let listening: Vec<&Listener> = report
            .listeners
            .iter()
            .filter(|listener| listener.port == *port)
            .collect();
        if listening.is_empty() && report.listener_scan {
            findings.push(format!(
                "client_port_not_listening:{}:{port}",
                client.spec().id
            ));
        }
        if classify_host(host) == "loopback_name"
            && !listening.is_empty()
            && listening
                .iter()
                .all(|listener| listener.address == "loopback")
        {
            findings.push(format!(
                "client_host_is_localhost_but_listener_is_ipv4_only:{}:{port}",
                client.spec().id
            ));
        }
    }
}

fn port_findings(
    report: &EnvironmentReport,
    config: Option<&AppConfig>,
    biflow_ports: &BTreeSet<u16>,
    findings: &mut Vec<String>,
) {
    let client_ports: BTreeSet<u16> = config
        .map(|config| {
            config
                .clients
                .iter()
                .filter_map(|client| match client.config {
                    ClientConfig::LocalProxy { port, .. } => Some(port),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    for listener in &report.listeners {
        if let (true, Some(process)) = (biflow_ports.contains(&listener.port), &listener.process) {
            if !process.starts_with("mihomo") && !process.starts_with("biflow") {
                findings.push(format!("port_conflict:{}:{process}", listener.port));
            }
        }
    }
    for setting in report.system_proxy.iter().filter(|setting| setting.enabled) {
        match (setting.host, setting.port) {
            (Some(host), Some(port)) if is_loopback_class(host) => {
                if !client_ports.contains(&port) && !biflow_ports.contains(&port) {
                    findings.push(format!(
                        "system_proxy_unknown_local_port:{}:{port}",
                        setting.source
                    ));
                }
            }
            (Some(host), _) => {
                findings.push(format!("system_proxy_not_local:{}:{host}", setting.source));
            }
            _ => {}
        }
    }
    for setting in &report.env_proxy {
        if setting.enabled && setting.host.is_some() {
            findings.push(format!("env_proxy_set:{}", setting.source));
        }
    }
}

fn network_findings(
    report: &EnvironmentReport,
    config: Option<&AppConfig>,
    findings: &mut Vec<String>,
) {
    for cluster in &report.kube.clusters {
        if cluster.proxy_url {
            findings.push(format!("kube_cluster_proxy_url:{}", cluster.host));
        }
        if cluster.route.as_deref() == Some("direct") {
            findings.push(format!(
                "kube_cluster_direct:{}:{}",
                cluster.host,
                cluster.route_reason.as_deref().unwrap_or("unknown")
            ));
        }
    }
    let tun_name = config.map(|config| config.mihomo.tun_name.to_ascii_lowercase());
    for adapter in &report.adapters {
        let up = matches!(
            adapter.status.to_ascii_lowercase().as_str(),
            "up" | "unknown"
        );
        let name = adapter.name.to_ascii_lowercase();
        let ours = tun_name.as_deref() == Some(name.as_str())
            || name == "meta"
            || is_side_tunnel_device(&name, config);
        if up && !ours && matches!(adapter.kind, "wireguard" | "vpn" | "tun") {
            findings.push(format!("other_tunnel_adapter_up:{}", adapter.name));
        }
    }
    let full_defaults = report
        .default_routes
        .iter()
        .filter(|route| route.prefix.ends_with("/0"))
        .count();
    if full_defaults > 1 {
        findings.push(format!("multiple_default_routes:{full_defaults}"));
    }
    for route in &report.default_routes {
        if route.prefix.ends_with("/1") {
            findings.push(format!("split_default_route:{}", route.interface));
        }
    }
}

/// The helper names side-tunnel devices `tun-<first 8 of client id>`.
fn is_side_tunnel_device(name: &str, config: Option<&AppConfig>) -> bool {
    let Some(suffix) = name.strip_prefix("tun-") else {
        return false;
    };
    config.is_some_and(|config| {
        config
            .clients
            .iter()
            .any(|client| client.id.as_hyphenated().starts_with(suffix))
    })
}

fn host_findings(
    report: &EnvironmentReport,
    config: Option<&AppConfig>,
    findings: &mut Vec<String>,
) {
    for process in &report.vpn_processes {
        // Only local-proxy clients run as their own named process. A side
        // tunnel such as Windscribe rides OpenVPN, so a running Windscribe
        // GUI is a second VPN (with its own firewall), not BiFlow's client.
        let ours = config.is_some_and(|config| {
            config.enabled_clients().iter().any(|client| {
                matches!(client.config, ClientConfig::LocalProxy { .. })
                    && process.starts_with(client.spec().id)
            })
        });
        if !ours
            && !process.starts_with("mihomo")
            && !process.starts_with("openvpn")
            && !process.starts_with("docker")
            && !process.starts_with("vmmem")
        {
            findings.push(format!("other_vpn_process:{process}"));
        }
    }
    for product in &report.security_products {
        let name = product.name.to_ascii_lowercase();
        if product.enabled != Some(false)
            && !name.contains("windows defender")
            && !name.contains("microsoft defender")
        {
            findings.push(format!(
                "third_party_security:{}:{}",
                product.kind, product.name
            ));
        }
    }
    for profile in &report.firewall {
        if !profile.enabled && cfg!(windows) {
            findings.push(format!("firewall_disabled:{}", profile.name));
        }
    }
    if report
        .hosts_file
        .localhost
        .iter()
        .any(|class| !is_loopback_class(class))
    {
        findings.push("hosts_localhost_not_loopback".into());
    }
}

// ---------------------------------------------------------------------------
// Platform collectors.
// ---------------------------------------------------------------------------

#[cfg(any(windows, target_os = "linux"))]
async fn run_command(program: &str, args: &[&str]) -> Result<String, String> {
    let mut command = tokio::process::Command::new(program);
    command.args(args).kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = tokio::time::timeout(COMMAND_TIMEOUT, command.output())
        .await
        .map_err(|_| format!("{program} timed out"))?
        .map_err(|error| format!("{program} could not start: {}", error.kind()))?;
    if !output.status.success() && output.stdout.is_empty() {
        return Err(format!("{program} exited with {}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(windows)]
async fn collect_platform(report: &mut EnvironmentReport) -> PlatformData {
    let mut script = match tempfile::Builder::new()
        .prefix("biflow-environment-")
        .suffix(".ps1")
        .tempfile()
    {
        Ok(file) => file,
        Err(error) => {
            report
                .collection_errors
                .push(format!("temp script: {}", error.kind()));
            return PlatformData::default();
        }
    };
    let written = {
        use std::io::Write;
        let file = script.as_file_mut();
        file.write_all(WINDOWS_SCRIPT.as_bytes())
            .and_then(|()| file.flush())
    };
    if let Err(error) = written {
        report
            .collection_errors
            .push(format!("temp script write: {}", error.kind()));
        return PlatformData::default();
    }
    let path = script.path().to_string_lossy().into_owned();
    let output = run_command(
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &path,
        ],
    )
    .await;
    match output {
        Ok(text) => match serde_json::from_str::<serde_json::Value>(text.trim()) {
            Ok(value) => parse_windows(&value, report),
            Err(error) => {
                report
                    .collection_errors
                    .push(format!("powershell json: {error}"));
                PlatformData::default()
            }
        },
        Err(error) => {
            report.collection_errors.push(error);
            PlatformData::default()
        }
    }
}

/// Emits one JSON object. Every probe is independent and silently empty when
/// a cmdlet is missing (Server SKUs have no `SecurityCenter2`, for example).
#[cfg(any(windows, test))]
const WINDOWS_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$o = [ordered]@{}
$os = Get-CimInstance Win32_OperatingSystem
$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$o.os_name = "$($os.Caption)"
$o.os_version = "$($cv.DisplayVersion)"
$o.os_build = "$($os.BuildNumber).$($cv.UBR)"
$o.firewall = @(Get-NetFirewallProfile | ForEach-Object { [ordered]@{ name = "$($_.Name)"; enabled = [bool]$_.Enabled } })
$o.security = @(foreach ($c in 'AntiVirusProduct','FirewallProduct') { Get-CimInstance -Namespace root/SecurityCenter2 -ClassName $c | ForEach-Object { [ordered]@{ kind = $c; name = "$($_.displayName)"; state = [int64]$_.productState } } })
$o.adapters = @(Get-NetAdapter | ForEach-Object { [ordered]@{ name = "$($_.Name)"; description = "$($_.InterfaceDescription)"; status = "$($_.Status)" } })
$metrics = @{}
Get-NetIPInterface -AddressFamily IPv4 | ForEach-Object { $metrics[[int]$_.ifIndex] = [int]$_.InterfaceMetric }
$o.routes = @(Get-NetRoute -AddressFamily IPv4 | Where-Object { $_.DestinationPrefix -in @('0.0.0.0/0','0.0.0.0/1','128.0.0.0/1') } | ForEach-Object { [ordered]@{ prefix = "$($_.DestinationPrefix)"; interface = "$($_.InterfaceAlias)"; next_hop = "$($_.NextHop)"; route_metric = [int]$_.RouteMetric; interface_metric = $metrics[[int]$_.ifIndex] } })
$o.dns = @(Get-DnsClientServerAddress -AddressFamily IPv4 | Where-Object { $_.ServerAddresses } | ForEach-Object { [ordered]@{ interface = "$($_.InterfaceAlias)"; servers = @($_.ServerAddresses) } })
$is = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
$o.inet = [ordered]@{ enable = [int]$is.ProxyEnable; server = "$($is.ProxyServer)"; pac = [bool]$is.AutoConfigURL; auto_detect = [int]$is.AutoDetect; bypass_local = ("$($is.ProxyOverride)" -like '*<local>*') }
$o.winhttp = (netsh winhttp show proxy | Out-String)
$o.env = @(foreach ($scope in 'User','Machine') { foreach ($n in 'HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','NO_PROXY') { $v = [Environment]::GetEnvironmentVariable($n, $scope); if ($v) { [ordered]@{ scope = $scope; name = $n; value = "$v" } } } })
$procs = @{}
Get-Process | ForEach-Object { $procs[[int]$_.Id] = "$($_.ProcessName)" }
$o.processes = @($procs.Values | Sort-Object -Unique)
$o.listeners = @(Get-NetTCPConnection -State Listen | ForEach-Object { [ordered]@{ address = "$($_.LocalAddress)"; port = [int]$_.LocalPort; process = $procs[[int]$_.OwningProcess] } })
$o | ConvertTo-Json -Compress -Depth 5
"#;

#[cfg(any(windows, test))]
fn json_array(value: &serde_json::Value, key: &str) -> Vec<serde_json::Value> {
    match value.get(key) {
        Some(serde_json::Value::Array(items)) => items.clone(),
        Some(serde_json::Value::Null) | None => Vec::new(),
        Some(single) => vec![single.clone()],
    }
}

#[cfg(any(windows, test))]
fn json_str(value: &serde_json::Value, key: &str) -> String {
    match value.get(key) {
        Some(serde_json::Value::String(text)) => text.trim().to_owned(),
        Some(serde_json::Value::Number(number)) => number.to_string(),
        Some(serde_json::Value::Bool(flag)) => flag.to_string(),
        _ => String::new(),
    }
}

#[cfg(any(windows, test))]
fn json_u32(value: &serde_json::Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|number| u32::try_from(number).ok())
}

#[cfg(any(windows, test))]
fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

/// `productState` byte 2 is `0x10` (on) or `0x11` (snoozed but running).
#[cfg(any(windows, test))]
fn security_product_enabled(state: i64) -> Option<bool> {
    (state > 0).then_some(matches!((state >> 8) & 0xff, 0x10 | 0x11))
}

/// Reads `netsh winhttp show proxy`. The text is localized, so the only
/// reliable signal is a `host:port` token.
#[cfg(any(windows, test))]
fn winhttp_setting(text: &str) -> ProxySetting {
    let token = text
        .split(|character: char| character.is_whitespace() || character == ';')
        .map(|token| token.rsplit_once('=').map_or(token, |(_, value)| value))
        .find(|token| {
            token
                .rsplit_once(':')
                .is_some_and(|(host, port)| !host.is_empty() && port.parse::<u16>().is_ok())
        });
    match token {
        Some(token) => proxy_setting("winhttp", token, None),
        None => ProxySetting {
            source: "winhttp".into(),
            enabled: false,
            scheme: None,
            host: None,
            port: None,
            detail: Some("direct".into()),
        },
    }
}

/// Windows Internet Settings `ProxyServer` may be `host:port` or a
/// per-protocol list such as `http=127.0.0.1:1;https=127.0.0.1:1`.
#[cfg(any(windows, test))]
fn internet_settings(inet: &serde_json::Value) -> Vec<ProxySetting> {
    let enabled = json_u32(inet, "enable").unwrap_or(0) != 0;
    let pac = inet.get("pac").and_then(serde_json::Value::as_bool) == Some(true);
    let bypass_local = inet
        .get("bypass_local")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let detail = format!(
        "pac={pac},auto_detect={},bypass_local={bypass_local}",
        json_u32(inet, "auto_detect").unwrap_or(0) != 0
    );
    let server = json_str(inet, "server");
    let mut settings: Vec<ProxySetting> = server
        .split(';')
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| {
            let (protocol, address) = entry.split_once('=').unwrap_or(("all", entry));
            let mut setting = proxy_setting(
                &format!("windows_internet_settings:{}", protocol.trim()),
                address,
                Some(detail.clone()),
            );
            setting.enabled = enabled && setting.enabled;
            setting
        })
        .collect();
    if settings.is_empty() {
        settings.push(ProxySetting {
            source: "windows_internet_settings".into(),
            enabled: false,
            scheme: None,
            host: None,
            port: None,
            detail: Some(detail),
        });
    }
    settings
}

#[cfg(any(windows, test))]
fn parse_windows(value: &serde_json::Value, report: &mut EnvironmentReport) -> PlatformData {
    report.system.os_name = non_empty(json_str(value, "os_name"));
    report.system.os_version = non_empty(json_str(value, "os_version"));
    report.system.os_build = non_empty(json_str(value, "os_build"));
    report.firewall = json_array(value, "firewall")
        .iter()
        .map(|profile| FirewallProfile {
            name: json_str(profile, "name"),
            enabled: profile.get("enabled").and_then(serde_json::Value::as_bool) == Some(true),
        })
        .collect();
    report.security_products = json_array(value, "security")
        .iter()
        .map(|product| SecurityProduct {
            kind: json_str(product, "kind"),
            name: json_str(product, "name"),
            enabled: product
                .get("state")
                .and_then(serde_json::Value::as_i64)
                .and_then(security_product_enabled),
        })
        .collect();
    report.adapters = json_array(value, "adapters")
        .iter()
        .map(|adapter| {
            let name = json_str(adapter, "name");
            let description = json_str(adapter, "description");
            Adapter {
                kind: adapter_kind(&name, &description),
                name,
                description,
                status: json_str(adapter, "status"),
            }
        })
        .collect();
    report.default_routes = json_array(value, "routes")
        .iter()
        .map(|route| RouteInfo {
            prefix: json_str(route, "prefix"),
            interface: json_str(route, "interface"),
            gateway: classify_host(&json_str(route, "next_hop")),
            route_metric: json_u32(route, "route_metric"),
            interface_metric: json_u32(route, "interface_metric"),
        })
        .collect();
    report.dns_servers = json_array(value, "dns")
        .iter()
        .map(|entry| DnsServers {
            interface: json_str(entry, "interface"),
            servers: json_array(entry, "servers")
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(classify_host)
                .collect(),
        })
        .collect();
    if let Some(inet) = value.get("inet") {
        report.system_proxy.extend(internet_settings(inet));
    }
    report
        .system_proxy
        .push(winhttp_setting(&json_str(value, "winhttp")));
    for entry in json_array(value, "env") {
        let source = format!(
            "registry_env:{}:{}",
            json_str(&entry, "scope").to_ascii_lowercase(),
            json_str(&entry, "name")
        );
        let raw = json_str(&entry, "value");
        report.env_proxy.push(if source.ends_with("NO_PROXY") {
            no_proxy_setting(&source, &raw)
        } else {
            proxy_setting(&source, &raw, None)
        });
    }
    PlatformData {
        processes: json_array(value, "processes")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect(),
        listeners: json_array(value, "listeners")
            .iter()
            .filter_map(|listener| {
                Some(Listener {
                    port: u16::try_from(listener.get("port")?.as_u64()?).ok()?,
                    address: classify_host(&json_str(listener, "address")),
                    process: non_empty(json_str(listener, "process")),
                })
            })
            .collect(),
    }
}

#[cfg(target_os = "linux")]
async fn collect_platform(report: &mut EnvironmentReport) -> PlatformData {
    let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    report.system.os_name = os_release_field(&os_release, "PRETTY_NAME");
    report.system.os_version = os_release_field(&os_release, "VERSION_ID");
    report.system.kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|text| text.trim().to_owned());
    report.system.desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    report.system.session_type = std::env::var("XDG_SESSION_TYPE").ok();

    report.firewall = linux_firewall().await;
    report.adapters = linux_adapters();
    report.default_routes = std::fs::read_to_string("/proc/net/route")
        .map(|text| parse_proc_route(&text))
        .unwrap_or_default();
    report.dns_servers = linux_dns();
    report.system_proxy = linux_system_proxy(&mut report.collection_errors).await;

    let mut listeners = Vec::new();
    for (file, v6) in [("/proc/net/tcp", false), ("/proc/net/tcp6", true)] {
        if let Ok(text) = std::fs::read_to_string(file) {
            listeners.extend(parse_proc_tcp_listeners(&text, v6));
        }
    }
    PlatformData {
        processes: linux_processes(),
        listeners,
    }
}

#[cfg(any(target_os = "linux", test))]
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

#[cfg(any(target_os = "linux", test))]
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

#[cfg(any(target_os = "linux", test))]
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
#[cfg(any(target_os = "linux", test))]
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

/// `/proc/net/tcp{,6}` rows in state `0A` (LISTEN). Addresses are stored as
/// little-endian 32-bit words.
#[cfg(any(target_os = "linux", test))]
fn parse_proc_tcp_listeners(text: &str, v6: bool) -> Vec<Listener> {
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
            Some(Listener {
                port,
                address: classify_host(&ip.to_string()),
                process: None,
            })
        })
        .collect()
}

#[cfg(not(any(windows, target_os = "linux")))]
#[allow(clippy::unused_async)]
async fn collect_platform(_report: &mut EnvironmentReport) -> PlatformData {
    PlatformData::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_host_never_returns_the_address() {
        assert_eq!(classify_host("127.0.0.1"), "loopback");
        assert_eq!(classify_host("localhost"), "loopback_name");
        assert_eq!(classify_host("[::1]"), "loopback");
        assert_eq!(classify_host("10.1.2.3"), "private");
        assert_eq!(classify_host("100.100.1.1"), "cgnat");
        assert_eq!(classify_host("198.18.0.5"), "fake_ip");
        assert_eq!(classify_host("8.8.8.8"), "public_ip");
        assert_eq!(classify_host("api.example.ir"), "domain_ir");
        assert_eq!(classify_host("k8s.example.com"), "domain");
        assert_eq!(classify_host("kubernetes"), "local_name");
        assert_eq!(classify_host("fd00::1"), "private");
    }

    #[test]
    fn parse_endpoint_handles_credentials_ipv6_and_bare_host_port() {
        assert_eq!(
            parse_endpoint("socks5h://user:pw@127.0.0.1:12334/x"),
            Some(Endpoint {
                scheme: Some("socks5h".into()),
                host: "127.0.0.1".into(),
                port: Some(12334),
            })
        );
        assert_eq!(
            parse_endpoint("[::1]:8080").map(|endpoint| (endpoint.host, endpoint.port)),
            Some(("::1".into(), Some(8080)))
        );
        assert_eq!(
            parse_endpoint("localhost:2080").map(|endpoint| endpoint.port),
            Some(Some(2080))
        );
        assert_eq!(parse_endpoint("  "), None);
    }

    #[test]
    fn env_proxies_are_classified_not_copied() {
        let settings = process_env_proxies(|name| match name {
            "HTTPS_PROXY" => Some("http://secret-user:pw@10.0.0.9:3128".into()),
            "NO_PROXY" => Some("localhost,.corp,10.0.0.0/8".into()),
            _ => None,
        });
        assert_eq!(settings.len(), 2);
        assert_eq!(settings[0].host, Some("private"));
        assert_eq!(settings[0].port, Some(3128));
        assert_eq!(settings[1].detail.as_deref(), Some("entries=3"));
        let encoded = serde_json::to_string(&settings).unwrap();
        assert!(!encoded.contains("10.0.0.9"));
        assert!(!encoded.contains("secret-user"));
        assert!(!encoded.contains(".corp"));
    }

    #[test]
    fn known_vpn_processes_ignore_unrelated_names() {
        let processes = vec![
            "Hiddify.exe".to_owned(),
            "v2rayN".to_owned(),
            "monitor".to_owned(),
            "editor".to_owned(),
            "Windscribe.exe".to_owned(),
            "chrome".to_owned(),
            "tor".to_owned(),
        ];
        assert_eq!(
            known_vpn_processes(&processes),
            vec!["hiddify", "tor", "v2rayn", "windscribe"]
        );
    }

    #[test]
    fn listeners_keep_only_relevant_ports_and_known_processes() {
        let ports = BTreeSet::from([12334]);
        let kept = relevant_listeners(
            vec![
                Listener {
                    port: 12334,
                    address: "loopback",
                    process: Some("SomethingElse".into()),
                },
                Listener {
                    port: 5432,
                    address: "unspecified",
                    process: Some("postgres".into()),
                },
                Listener {
                    port: 7777,
                    address: "loopback",
                    process: Some("xray.exe".into()),
                },
            ],
            &ports,
        );
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().all(|listener| listener.port != 5432));
        assert!(kept
            .iter()
            .any(|listener| listener.process.as_deref() == Some("xray")));
    }

    #[test]
    fn kubeconfig_servers_are_classified_and_routed() {
        let text = r#"
apiVersion: v1
clusters:
- cluster:
    certificate-authority-data: AAAA
    server: https://10.20.30.40:6443
  name: office
- cluster:
    server: "https://k8s.example.com"
    proxy-url: socks5://127.0.0.1:1080
  name: cloud
"#;
        let clusters = parse_kubeconfig(text, |host| {
            Some((
                if host.starts_with("10.") {
                    "direct".into()
                } else {
                    "client:hiddify".into()
                },
                "private".into(),
            ))
        });
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].host, "private");
        assert_eq!(clusters[0].port, Some(6443));
        assert_eq!(clusters[0].route.as_deref(), Some("direct"));
        assert!(!clusters[0].proxy_url);
        assert_eq!(clusters[1].host, "domain");
        assert_eq!(clusters[1].port, Some(443));
        assert!(clusters[1].proxy_url);
        assert!(!serde_json::to_string(&clusters)
            .unwrap()
            .contains("example.com"));
    }

    #[test]
    fn hosts_file_reports_count_and_localhost_class() {
        let info =
            parse_hosts("# c\n127.0.0.1 localhost\n::1 localhost\n10.0.0.2 localhost\n1.2.3.4 a\n");
        assert_eq!(info.entries, 4);
        assert_eq!(info.localhost, vec!["loopback", "private"]);
    }

    #[test]
    fn windows_payload_is_parsed_and_classified() {
        let value = serde_json::json!({
            "os_name": "Microsoft Windows 11 Pro",
            "os_version": "23H2",
            "os_build": "22631.4317",
            "firewall": [{"name": "Domain", "enabled": true}, {"name": "Public", "enabled": false}],
            "security": {"kind": "AntiVirusProduct", "name": "Kaspersky", "state": 266_240},
            "adapters": [
                {"name": "Wi-Fi", "description": "Intel Wireless", "status": "Up"},
                {"name": "OpenVPN Data Channel Offload", "description": "ovpn-dco", "status": "Disconnected"},
                {"name": "Meta", "description": "Meta Tunnel", "status": "Up"}
            ],
            "routes": [
                {"prefix": "0.0.0.0/0", "interface": "Wi-Fi", "next_hop": "192.168.1.1", "route_metric": 0, "interface_metric": 35},
                {"prefix": "0.0.0.0/1", "interface": "Other VPN", "next_hop": "0.0.0.0", "route_metric": 0, "interface_metric": 5}
            ],
            "dns": [{"interface": "Wi-Fi", "servers": ["192.168.1.1", "8.8.8.8"]}],
            "inet": {"enable": 1, "server": "http=127.0.0.1:10809;https=127.0.0.1:10809", "pac": false, "auto_detect": 0, "bypass_local": true},
            "winhttp": "\r\nCurrent WinHTTP proxy settings:\r\n\r\n    Direct access (no proxy server).\r\n",
            "env": [{"scope": "User", "name": "HTTPS_PROXY", "value": "http://127.0.0.1:10809"}],
            "processes": ["Hiddify", "v2rayN", "chrome"],
            "listeners": [
                {"address": "127.0.0.1", "port": 12334, "process": "Hiddify"},
                {"address": "0.0.0.0", "port": 19090, "process": "SomeService"}
            ]
        });
        let mut report = EnvironmentReport::default();
        let platform = parse_windows(&value, &mut report);
        assert_eq!(report.system.os_build.as_deref(), Some("22631.4317"));
        assert_eq!(report.firewall.len(), 2);
        assert_eq!(report.security_products[0].enabled, Some(true));
        assert_eq!(report.adapters[1].kind, "openvpn_dco");
        assert_eq!(report.default_routes[0].gateway, "private");
        assert_eq!(report.dns_servers[0].servers, vec!["private", "public_ip"]);
        assert_eq!(report.system_proxy.len(), 3);
        assert!(report.system_proxy[0].enabled);
        assert_eq!(report.system_proxy[0].port, Some(10809));
        assert!(!report.system_proxy[2].enabled);
        assert_eq!(report.env_proxy[0].source, "registry_env:user:HTTPS_PROXY");
        assert_eq!(platform.processes.len(), 3);
        assert_eq!(platform.listeners.len(), 2);

        let config = AppConfig::default();
        let ports = interesting_ports(Some(&config));
        report.vpn_processes = known_vpn_processes(&platform.processes);
        report.listeners = relevant_listeners(platform.listeners, &ports);
        report.findings = findings(&report, Some(&config));
        let joined = report.findings.join("\n");
        assert!(
            joined.contains("system_proxy_unknown_local_port:windows_internet_settings:http:10809")
        );
        assert!(joined.contains("env_proxy_set:registry_env:user:HTTPS_PROXY"));
        assert!(joined.contains("split_default_route:Other VPN"));
        assert!(joined.contains("third_party_security:AntiVirusProduct:Kaspersky"));
        assert!(joined.contains("other_vpn_process:v2rayn"));
        assert!(!joined.contains("other_vpn_process:hiddify"));
        assert!(!joined.contains("other_tunnel_adapter_up:Meta"));
        let encoded = serde_json::to_string(&report).unwrap();
        assert!(!encoded.contains("192.168.1.1"));
        assert!(!encoded.contains("8.8.8.8"));
    }

    #[test]
    fn winhttp_proxy_token_is_found_in_localized_text() {
        let setting =
            winhttp_setting("    Proxy-Server(s) :  http=10.1.1.1:8080;https=10.1.1.1:8080\n");
        assert!(setting.enabled);
        assert_eq!(setting.host, Some("private"));
        assert_eq!(setting.port, Some(8080));
        assert!(!winhttp_setting("Direct access (no proxy server).").enabled);
    }

    #[test]
    fn missing_client_listener_and_port_conflicts_are_findings() {
        let config = AppConfig::default();
        let report = EnvironmentReport {
            listener_scan: true,
            listeners: vec![Listener {
                port: config.mihomo.controller_port,
                address: "loopback",
                process: Some("otherapp".into()),
            }],
            ..EnvironmentReport::default()
        };
        let found = findings(&report, Some(&config)).join("\n");
        assert!(found.contains("client_port_not_listening:hiddify:12334"));
        assert!(found.contains(&format!(
            "port_conflict:{}:otherapp",
            config.mihomo.controller_port
        )));
    }

    #[test]
    fn windscribe_gui_is_a_second_vpn_even_with_a_windscribe_client() {
        let mut config = AppConfig::default();
        config.clients.push(iran_split_config::ClientInstance {
            id: iran_split_config::ClientId::new(),
            preset: iran_split_config::PresetId::Windscribe,
            enabled: true,
            allow_direct_when_down: false,
            config: ClientConfig::from_preset(iran_split_config::PresetId::Windscribe),
        });
        let report = EnvironmentReport {
            vpn_processes: vec!["windscribe".into(), "openvpn".into(), "hiddify".into()],
            ..EnvironmentReport::default()
        };
        let found = findings(&report, Some(&config));
        assert_eq!(found, vec!["other_vpn_process:windscribe".to_owned()]);
    }

    #[test]
    fn kde_proxy_type_reads_only_the_proxy_section() {
        let text = "[General]\nProxyType=Settings\n[Proxy Settings]\nProxyType=1\n";
        assert_eq!(kde_proxy_type(text).as_deref(), Some("1"));
        assert_eq!(kde_proxy_type("[General]\nProxyType=2\n"), None);
    }

    #[test]
    fn own_side_tunnel_adapter_is_not_another_vpn() {
        let config = AppConfig::default();
        let prefix = &config.clients[0].id.as_hyphenated()[..8];
        let adapter = |name: &str| Adapter {
            name: name.into(),
            description: "tun".into(),
            status: "unknown".into(),
            kind: "tun",
        };
        let report = EnvironmentReport {
            adapters: vec![adapter(&format!("tun-{prefix}")), adapter("tailscale0")],
            ..EnvironmentReport::default()
        };
        let found = findings(&report, Some(&config));
        assert_eq!(found, vec!["other_tunnel_adapter_up:tailscale0".to_owned()]);
    }

    #[test]
    fn biflow_summary_contains_no_secrets() {
        let config = AppConfig::default();
        let info = biflow_info(Some(&config), "running");
        let encoded = serde_json::to_string(&info).unwrap();
        assert!(!encoded.contains(&config.mihomo.controller_secret));
        assert_eq!(info.default_route, "client:hiddify");
        assert_eq!(info.clients[0].host, Some("loopback"));
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
        assert_eq!(listeners[0].port, 12345);
        assert_eq!(listeners[0].address, "loopback");
        let v6 = "  sl local\n   0: 00000000000000000000000001000000:0050 00000000000000000000000000000000:0000 0A\n";
        let listeners = parse_proc_tcp_listeners(v6, true);
        assert_eq!(listeners[0].address, "loopback");
        assert_eq!(listeners[0].port, 80);
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
    fn windows_script_emits_every_parsed_key() {
        for key in [
            "os_name",
            "firewall",
            "security",
            "adapters",
            "routes",
            "dns",
            "inet",
            "winhttp",
            "env",
            "processes",
            "listeners",
        ] {
            assert!(WINDOWS_SCRIPT.contains(&format!("$o.{key} =")), "{key}");
        }
    }

    #[tokio::test]
    async fn live_collection_completes_on_this_host_without_leaking_config() {
        let config = AppConfig::default();
        let report = collect(CollectContext {
            app_version: "0.0.0-test".into(),
            config: Some(config.clone()),
            rules: None,
            stack_phase: "stopped".into(),
        })
        .await;
        assert_eq!(report.system.os, std::env::consts::OS);
        assert_eq!(report.biflow.clients.len(), 1);
        let encoded = serde_json::to_string(&report).expect("report serializes");
        assert!(!encoded.contains(&config.mihomo.controller_secret));
        if std::env::var_os("BIFLOW_PRINT_ENVIRONMENT").is_some() {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).unwrap_or_default()
            );
        }
    }
}
