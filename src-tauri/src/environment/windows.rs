//! Windows collector: one bounded `PowerShell` script, parsed on every host
//! so the parser is unit-tested from Linux too.

#[cfg(windows)]
use super::run_command;
use super::{
    adapter_kind, classify_host, no_proxy_setting, proxy_setting, Adapter, DnsServers,
    EnvironmentReport, FirewallProfile, Listener, PlatformData, ProxySetting, RouteInfo,
    SecurityProduct, ServiceState,
};

/// Scheduled task that runs the privileged helper (`iran-split-helper`).
const HELPER_TASK: &str = "BiFlowHelper";

#[cfg(windows)]
pub(super) async fn collect_platform(
    report: &mut EnvironmentReport,
    _tun_name: &str,
) -> PlatformData {
    // `-EncodedCommand` needs no temp file and is not subject to the
    // script execution policy; a GPO `AllSigned`/`Restricted` policy made
    // `-File` exit 1 with no output on a field machine.
    let encoded = encoded_command(WINDOWS_SCRIPT);
    let output = run_command(
        "powershell",
        &[
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded,
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

/// `powershell -EncodedCommand` takes Base64 of the UTF-16LE script.
fn encoded_command(script: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = chunk.iter().enumerate().fold(0_u32, |acc, (index, byte)| {
            acc | (u32::from(*byte) << (16 - 8 * index))
        });
        for index in 0..4 {
            if index <= chunk.len() {
                let sextet = (triple >> (18 - 6 * index)) & 0x3f;
                encoded.push(char::from(ALPHABET[sextet as usize]));
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

/// Emits one JSON object. Every probe is independent and silently empty when
/// a cmdlet is missing (Server SKUs have no `SecurityCenter2`, for example).
const WINDOWS_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
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
Get-NetIPInterface | ForEach-Object { $metrics["$($_.ifIndex)/$($_.AddressFamily)"] = [int]$_.InterfaceMetric }
$o.routes = @(Get-NetRoute | Where-Object { $_.DestinationPrefix -in @('0.0.0.0/0','0.0.0.0/1','128.0.0.0/1','::/0','::/1','8000::/1') } | ForEach-Object { [ordered]@{ prefix = "$($_.DestinationPrefix)"; interface = "$($_.InterfaceAlias)"; next_hop = "$($_.NextHop)"; route_metric = [int]$_.RouteMetric; interface_metric = $metrics["$($_.ifIndex)/$($_.AddressFamily)"] } })
$o.route_counts = @(Get-NetRoute | Group-Object InterfaceAlias | ForEach-Object { [ordered]@{ interface = "$($_.Name)"; count = [int]$_.Count } })
$o.is_admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$o.uptime_minutes = [int64]((Get-Date) - $os.LastBootUpTime).TotalMinutes
$o.services = @(Get-Service | ForEach-Object { [ordered]@{ name = "$($_.Name)"; state = "$($_.Status)" } })
$task = Get-ScheduledTask -TaskName 'BiFlowHelper'
$o.helper_task = if ($task) { "$($task.State)" } else { 'missing' }
$o.block_rules = @(Get-NetFirewallRule -Enabled True -Action Block | ForEach-Object { $rule = $_; $_ | Get-NetFirewallApplicationFilter | Where-Object { "$($_.Program)" -match 'openvpn|mihomo|hiddify|biflow|iran-split|v2ray|xray|happ|sing-box|nekoray' } | ForEach-Object { [ordered]@{ direction = "$($rule.Direction)"; program = [IO.Path]::GetFileName("$($_.Program)") } } })
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

fn json_array(value: &serde_json::Value, key: &str) -> Vec<serde_json::Value> {
    match value.get(key) {
        Some(serde_json::Value::Array(items)) => items.clone(),
        Some(serde_json::Value::Null) | None => Vec::new(),
        Some(single) => vec![single.clone()],
    }
}

fn json_str(value: &serde_json::Value, key: &str) -> String {
    match value.get(key) {
        Some(serde_json::Value::String(text)) => text.trim().to_owned(),
        Some(serde_json::Value::Number(number)) => number.to_string(),
        Some(serde_json::Value::Bool(flag)) => flag.to_string(),
        _ => String::new(),
    }
}

fn json_u32(value: &serde_json::Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|number| u32::try_from(number).ok())
}

fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

/// `productState` byte 2 is `0x10` (on) or `0x11` (snoozed but running).
fn security_product_enabled(state: i64) -> Option<bool> {
    (state > 0).then_some(matches!((state >> 8) & 0xff, 0x10 | 0x11))
}

/// Reads `netsh winhttp show proxy`. The text is localized, so the only
/// reliable signal is a `host:port` token.
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

/// Routing summary, privilege, uptime, services, and firewall block rules.
fn parse_windows_host(
    value: &serde_json::Value,
    report: &mut EnvironmentReport,
) -> Vec<ServiceState> {
    report.routing.ipv6_default_interfaces = report
        .default_routes
        .iter()
        .filter(|route| route.prefix == "::/0")
        .map(|route| route.interface.clone())
        .collect();
    report.routing.routes = json_array(value, "route_counts")
        .iter()
        .map(|entry| {
            format!(
                "{}: {} routes",
                json_str(entry, "interface"),
                json_u32(entry, "count").unwrap_or(0)
            )
        })
        .collect();
    report.install.elevated = value.get("is_admin").and_then(serde_json::Value::as_bool);
    report.install.uptime_minutes = value
        .get("uptime_minutes")
        .and_then(serde_json::Value::as_u64);
    report.firewall_block_rules = json_array(value, "block_rules")
        .iter()
        .map(|rule| {
            format!(
                "{}:{}",
                json_str(rule, "direction"),
                json_str(rule, "program")
            )
        })
        .collect();
    let mut services: Vec<ServiceState> = json_array(value, "services")
        .iter()
        .map(|service| ServiceState {
            name: json_str(service, "name"),
            state: json_str(service, "state"),
        })
        .collect();
    services.push(ServiceState {
        name: HELPER_TASK.into(),
        state: non_empty(json_str(value, "helper_task")).unwrap_or_else(|| "unknown".into()),
    });
    services
}

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
    let services = parse_windows_host(value, report);
    PlatformData {
        services,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::{
        findings, interesting_ports, known_services, known_vpn_processes, relevant_listeners,
    };
    use iran_split_config::AppConfig;

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
            "route_counts": {"interface": "Wi-Fi", "count": 14},
            "is_admin": false,
            "uptime_minutes": 1440,
            "services": [{"name": "WindscribeService", "state": "Running"}, {"name": "Spooler", "state": "Running"}],
            "helper_task": "Running",
            "block_rules": {"direction": "Outbound", "program": "openvpn.exe"},
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

        assert_eq!(report.routing.routes, vec!["Wi-Fi: 14 routes"]);
        assert_eq!(report.install.elevated, Some(false));
        assert_eq!(report.install.uptime_minutes, Some(1440));
        assert_eq!(report.firewall_block_rules, vec!["Outbound:openvpn.exe"]);

        let config = AppConfig::default();
        let ports = interesting_ports(Some(&config));
        report.vpn_processes = known_vpn_processes(&platform.processes);
        report.listeners = relevant_listeners(platform.listeners, &ports);
        report.services = known_services(platform.services);
        assert_eq!(report.services.len(), 2);
        report.findings = findings(&report, Some(&config));
        let joined = report.findings.join("\n");
        assert!(
            joined.contains("system_proxy_unknown_local_port:windows_internet_settings:http:10809")
        );
        assert!(joined.contains("env_proxy_set:registry_env:user:HTTPS_PROXY"));
        assert!(joined.contains("other_vpn_service_running:WindscribeService"));
        assert!(joined.contains("firewall_blocks_program:Outbound:openvpn.exe"));
        assert!(!joined.contains("helper_service"));
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
            "route_counts",
            "is_admin",
            "uptime_minutes",
            "services",
            "helper_task",
            "block_rules",
        ] {
            assert!(WINDOWS_SCRIPT.contains(&format!("$o.{key} =")), "{key}");
        }
    }

    #[test]
    fn encoded_command_is_utf16le_base64_within_the_command_line_limit() {
        // "ab" -> 61 00 62 00 -> YQBiAA==
        assert_eq!(encoded_command("ab"), "YQBiAA==");
        assert_eq!(encoded_command("abc"), "YQBiAGMA");
        assert!(encoded_command(WINDOWS_SCRIPT).len() < 30_000);
    }
}
