mod clients;
mod profile;

use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    net::IpAddr,
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;
use thiserror::Error;

pub use clients::{
    is_custom_client_generation_file, ClientConfig, ClientId, ClientInstance, DefaultRoute,
    EgressKind, PresetId, PresetSpec, PresetStatus,
};
pub use profile::{
    BaseDirs, PrivilegedOverrides, ProfileEnv, ResolvedProfile, RuntimeProfile, UserResources,
    DEV_HELPER_ENDPOINT_VAR, DEV_MIHOMO_BINARY_VAR, DEV_PROFILE_VAR, DEV_SYSTEM_RUNTIME_VAR,
};

pub const CURRENT_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub schema_version: u32,
    pub revision: u64,
    pub clients: Vec<ClientInstance>,
    pub default_route: DefaultRoute,
    pub mihomo: MihomoConfig,
    pub rules: RulesConfig,
    pub behavior: BehaviorConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        let hiddify_id = ClientId::new();
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            revision: 0,
            clients: vec![ClientInstance::hiddify_default(hiddify_id)],
            default_route: DefaultRoute::client(hiddify_id),
            mihomo: MihomoConfig::default(),
            rules: RulesConfig::default(),
            behavior: BehaviorConfig::default(),
        }
    }
}

impl AppConfig {
    /// First enabled Hiddify instance, if the user still has one.
    #[must_use]
    pub fn hiddify_client(&self) -> Option<&ClientInstance> {
        self.clients
            .iter()
            .find(|client| client.preset == PresetId::Hiddify)
    }

    /// Enabled instances that should be started and emitted into Mihomo.
    #[must_use]
    pub fn enabled_clients(&self) -> Vec<&ClientInstance> {
        self.clients
            .iter()
            .filter(|client| client.enabled)
            .collect()
    }

    #[must_use]
    pub fn client(&self, id: ClientId) -> Option<&ClientInstance> {
        self.clients.iter().find(|client| client.id == id)
    }

    /// Loopback SOCKS endpoint of the Hiddify instance, or the catalog default.
    #[must_use]
    pub fn hiddify_endpoint(&self) -> (String, u16) {
        match self.hiddify_client().map(|client| &client.config) {
            Some(ClientConfig::LocalProxy { host, port, .. }) => (host.clone(), *port),
            _ => (
                PresetId::Hiddify.spec().default_host.into(),
                PresetId::Hiddify.spec().default_port.unwrap_or(12_334),
            ),
        }
    }

    #[must_use]
    pub fn hiddify_start_timeout(&self) -> u64 {
        match self.hiddify_client().map(|client| &client.config) {
            Some(ClientConfig::LocalProxy {
                start_timeout_seconds,
                ..
            }) => *start_timeout_seconds,
            _ => 45,
        }
    }

    #[must_use]
    pub fn hiddify_stop_with_stack(&self) -> bool {
        match self.hiddify_client().map(|client| &client.config) {
            Some(ClientConfig::LocalProxy {
                stop_with_stack, ..
            }) => *stop_with_stack,
            _ => true,
        }
    }

    #[must_use]
    pub fn hiddify_executable(&self) -> ExecutableSetting {
        match self.hiddify_client().map(|client| &client.config) {
            Some(ClientConfig::LocalProxy { executable, .. }) => executable.clone(),
            _ => ExecutableSetting::Auto,
        }
    }

    /// Falls back to [`DefaultRoute::Direct`] when the MATCH target is gone or disabled.
    pub fn sanitize_default_route(&mut self) -> bool {
        let DefaultRoute::Client { client_id } = self.default_route else {
            return false;
        };
        let usable = self
            .clients
            .iter()
            .any(|client| client.id == client_id && client.enabled);
        if usable {
            return false;
        }
        self.default_route = DefaultRoute::Direct;
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutableSetting {
    #[default]
    Auto,
    Path(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MihomoConfig {
    pub controller_host: String,
    pub controller_port: u16,
    pub controller_secret: String,
    pub mixed_port: u16,
    pub dns_port: u16,
    pub tun_name: String,
    pub log_level: LogLevel,
    pub direct_dns_preset: DirectDnsPreset,
    /// Used when [`DirectDnsPreset::Custom`]. Named presets ignore this list.
    pub direct_dns_servers: Vec<String>,
}

impl Default for MihomoConfig {
    fn default() -> Self {
        Self {
            controller_host: "127.0.0.1".into(),
            controller_port: PRODUCTION_CONTROLLER_PORT,
            controller_secret: generate_secret(),
            mixed_port: PRODUCTION_MIXED_PORT,
            dns_port: PRODUCTION_DNS_PORT,
            tun_name: PRODUCTION_TUN_NAME.into(),
            log_level: LogLevel::Info,
            direct_dns_preset: DirectDnsPreset::default(),
            direct_dns_servers: Vec::new(),
        }
    }
}

/// Installed-app defaults. A `BIFLOW_DEV_PROFILE` run must not share them.
pub const PRODUCTION_CONTROLLER_PORT: u16 = 19_090;
pub const DEV_PROFILE_CONTROLLER_PORT: u16 = 19_091;
pub const PRODUCTION_MIXED_PORT: u16 = 17_890;
pub const DEV_PROFILE_MIXED_PORT: u16 = 17_891;
pub const PRODUCTION_DNS_PORT: u16 = 1_053;
pub const DEV_PROFILE_DNS_PORT: u16 = 2_053;
/// macOS sends DNS queries to the configured nameserver. The router (LAN
/// resolver) bypasses the TUN and cannot resolve blocked domains, so the
/// system DNS must point at Mihomo. The system resolver always uses port 53,
/// so Mihomo must listen on `127.0.0.1:53` (it runs as root via the helper).
pub const MACOS_DNS_PORT: u16 = 53;
pub const PRODUCTION_TUN_NAME: &str = "clash-iran";
pub const DEV_PROFILE_TUN_NAME: &str = "biflow-dev";
/// macOS `utun` devices must be named `utunN`; the kernel rejects arbitrary
/// names such as `clash-iran`. A high unit avoids the system's low-numbered
/// utun adapters (iCloud/Back-to-Mac, other VPNs).
pub const MACOS_TUN_NAME: &str = "utun9";

impl MihomoConfig {
    /// Move off the installed app's loopback ports and TUN when this process
    /// runs a development profile. The profile is supplied by the caller so
    /// this crate never reads process-global environment (ADR 0115). Returns
    /// whether any field changed.
    pub fn isolate_from_installed_app(&mut self, profile: &RuntimeProfile) -> bool {
        self.isolate_from_installed_app_if(profile.is_development())
    }

    /// On macOS the TUN device name must match the kernel `utunN` convention;
    /// a stored `clash-iran` (or any non-`utun` name) makes Mihomo fail to
    /// create the interface and the readiness TUN check never sees it.
    /// Returns whether the name was rewritten.
    #[must_use]
    pub fn normalize_tun_name_for_platform(&mut self) -> bool {
        if cfg!(target_os = "macos") && !is_valid_macos_tun_name(&self.tun_name) {
            self.tun_name = MACOS_TUN_NAME.into();
            return true;
        }
        false
    }

    /// On macOS the system DNS resolver always uses port 53, and the LAN
    /// resolver bypasses the TUN (so Mihomo's `dns-hijack` never sees it).
    /// Mihomo must listen on `127.0.0.1:53` so the system DNS can point at
    /// it; it runs as root via the helper. Returns whether the port changed.
    #[must_use]
    pub fn normalize_dns_port_for_platform(&mut self) -> bool {
        if cfg!(target_os = "macos") && self.dns_port != MACOS_DNS_PORT {
            self.dns_port = MACOS_DNS_PORT;
            return true;
        }
        false
    }

    /// Same remap as [`Self::isolate_from_installed_app`], with the env check
    /// supplied by the caller so tests do not mutate process environment.
    pub fn isolate_from_installed_app_if(&mut self, isolate: bool) -> bool {
        if !isolate {
            return false;
        }
        let mut changed = false;
        if self.controller_port == PRODUCTION_CONTROLLER_PORT {
            self.controller_port = DEV_PROFILE_CONTROLLER_PORT;
            changed = true;
        }
        if self.mixed_port == PRODUCTION_MIXED_PORT {
            self.mixed_port = DEV_PROFILE_MIXED_PORT;
            changed = true;
        }
        if self.dns_port == PRODUCTION_DNS_PORT {
            self.dns_port = DEV_PROFILE_DNS_PORT;
            changed = true;
        }
        if self.tun_name == PRODUCTION_TUN_NAME {
            self.tun_name = DEV_PROFILE_TUN_NAME.into();
            changed = true;
        }
        changed
    }
}

/// A macOS `utun` device name must be `utun` followed by 1-3 decimal digits
/// (for example `utun9`). Arbitrary names such as `clash-iran` are rejected by
/// the kernel `com.apple.net.utun` control and Mihomo never creates the TUN.
#[must_use]
pub fn is_valid_macos_tun_name(name: &str) -> bool {
    let Some(suffix) = name.strip_prefix("utun") else {
        return false;
    };
    !suffix.is_empty() && suffix.len() <= 3 && suffix.bytes().all(|byte| byte.is_ascii_digit())
}

/// Resolvers for Iranian and user-pinned DIRECT domains (not VPN `DoH`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DirectDnsPreset {
    /// Mihomo fake-ip plus Cloudflare `DoH`. No Iranian nameserver policy.
    #[default]
    FakeIp,
    Shecan,
    Electro,
    Radar,
    Mokhaberat,
    Custom,
}

impl DirectDnsPreset {
    /// Built-in IPv4 resolvers. `FakeIp` and `Custom` have none.
    #[must_use]
    pub const fn servers(self) -> &'static [&'static str] {
        match self {
            Self::FakeIp | Self::Custom => &[],
            Self::Shecan => &["178.22.122.100", "185.51.200.2"],
            Self::Electro => &["78.157.42.100", "78.157.42.101"],
            Self::Radar => &["10.202.10.10", "10.202.10.11"],
            Self::Mokhaberat => &["5.200.200.200"],
        }
    }

    /// Whether DIRECT domains skip fake-ip and use these resolvers.
    #[must_use]
    pub const fn applies_direct_policy(self) -> bool {
        !matches!(self, Self::FakeIp)
    }
}

impl std::fmt::Display for DirectDnsPreset {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FakeIp => "fake_ip",
            Self::Shecan => "shecan",
            Self::Electro => "electro",
            Self::Radar => "radar",
            Self::Mokhaberat => "mokhaberat",
            Self::Custom => "custom",
        })
    }
}

impl MihomoConfig {
    /// Addresses written into Mihomo `direct-nameserver` and nameserver-policy.
    #[must_use]
    pub fn direct_dns_resolvers(&self) -> Vec<String> {
        if self.direct_dns_preset == DirectDnsPreset::Custom {
            return self
                .direct_dns_servers
                .iter()
                .map(|server| server.trim().to_owned())
                .filter(|server| !server.is_empty())
                .collect();
        }
        self.direct_dns_preset
            .servers()
            .iter()
            .map(|server| (*server).to_owned())
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
        };
        formatter.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RulesConfig {
    pub refresh_interval_minutes: u64,
    pub upstream_refresh_hours: u64,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            refresh_interval_minutes: 15,
            upstream_refresh_hours: 24,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each field is an independent user-facing behavior toggle"
)]
pub struct BehaviorConfig {
    pub launch_at_login: bool,
    pub connect_at_launch: bool,
    pub close_to_tray: bool,
    /// When a client's egress is down, REJECT its traffic (and unmatched
    /// traffic bound for it) instead of leaking the real IP over DIRECT.
    pub fail_closed: bool,
    /// Render a tiny live `D:` / `U:` overlay next to the taskbar clock
    /// (Windows-only at the moment, ADR 0114).
    pub taskbar_traffic_enabled: bool,
    /// Upper unit the formatter is allowed to grow into when `Auto`, or the
    /// fixed unit it must use otherwise. Cross-platform: any future macOS
    /// / Linux menubar / panel widget reuses the same formatter.
    pub taskbar_traffic_unit: TaskbarTrafficUnit,
    /// Multiply the raw byte rate by 8 and append `b` instead of `B`.
    pub taskbar_traffic_bits: bool,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            connect_at_launch: false,
            close_to_tray: true,
            fail_closed: true,
            taskbar_traffic_enabled: true,
            taskbar_traffic_unit: TaskbarTrafficUnit::default(),
            taskbar_traffic_bits: false,
        }
    }
}

/// User-facing unit choice for the live taskbar overlay.
///
/// The `kb` / `mb` / `gb` aliases keep existing user settings written
/// before the rename alive — deserializing `"kb"` lands on `Kilobytes`
/// without breaking startup. New code should still serialize the full
/// name through `rename_all = "snake_case"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskbarTrafficUnit {
    /// Auto-scale up through `B`, `KB`, `MB`, `GB` (or `b`/`Kb`/... when bits).
    Auto,
    #[serde(alias = "b")]
    /// Always render raw bytes (or bits).
    Bytes,
    #[serde(alias = "kb")]
    /// Always render kilobytes (or kilobits).
    Kilobytes,
    #[serde(alias = "mb")]
    /// Always render megabytes (or megabits).
    Megabytes,
    #[serde(alias = "gb")]
    /// Always render gigabytes (or gigabits).
    Gigabytes,
}

impl Default for TaskbarTrafficUnit {
    fn default() -> Self {
        Self::Auto
    }
}

impl TaskbarTrafficUnit {
    /// Decimal exponent this unit pins to. `Auto` is reported as `None` so
    /// callers fall back to the auto-scaling ladder.
    #[must_use]
    pub const fn exponent(self) -> Option<u32> {
        match self {
            Self::Auto => None,
            Self::Bytes => Some(0),
            Self::Kilobytes => Some(1),
            Self::Megabytes => Some(2),
            Self::Gigabytes => Some(3),
        }
    }
}

/// What the live overlay shows for one direction of traffic.
///
/// `Auto` resolves to the largest unit that still leaves a value of at
/// least 1, falling back to `Gigabytes` when even `GB` would round down
/// to zero (e.g. a 200 MB/s link with the user-pinned `Megabytes`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskbarTrafficDisplay {
    pub unit: TaskbarTrafficUnit,
    pub bits: bool,
}

impl From<&BehaviorConfig> for TaskbarTrafficDisplay {
    fn from(value: &BehaviorConfig) -> Self {
        Self {
            unit: value.taskbar_traffic_unit,
            bits: value.taskbar_traffic_bits,
        }
    }
}

/// Render `bytes_per_second` for the live overlay.
///
/// `Auto` picks the largest decimal unit that leaves a value of at least
/// `1`.
///
/// `bits` multiplies the byte rate by `8` and swaps the suffix from
/// `B` / `KB` / `MB` / `GB` to `b` / `Kb` / `Mb` / `Gb`.
///
/// Returns `"0 B"` (or `"0 b"` when bits) when the rate is zero — an
/// honest idle reading rather than a flashing `0.0 B` ↔ `1.0 KB`.
#[must_use]
pub fn format_bandwidth_rate(bytes_per_second: u64, display: TaskbarTrafficDisplay) -> String {
    let multiplier = if display.bits { 8 } else { 1 };
    let raw = bytes_per_second.saturating_mul(multiplier);
    let exponent = display
        .unit
        .exponent()
        .unwrap_or_else(|| auto_exponent(raw));
    let (value, label) = render_at_exponent(raw, exponent);
    if display.bits {
        format_bandwidth(value, label, 'b')
    } else {
        format_bandwidth(value, label, 'B')
    }
}

/// Render a `(download, upload)` pair for the overlay in one call.
#[must_use]
pub fn format_bandwidth_pair(
    download_bps: u64,
    upload_bps: u64,
    display: TaskbarTrafficDisplay,
) -> (String, String) {
    (
        format_bandwidth_rate(download_bps, display),
        format_bandwidth_rate(upload_bps, display),
    )
}

fn auto_exponent(raw: u64) -> u32 {
    if raw < 1_000 {
        0
    } else if raw < 1_000_000 {
        1
    } else if raw < 1_000_000_000 {
        2
    } else {
        3
    }
}

/// Convert `raw` bytes (or bits) into a `(value, label)` pair at the
/// chosen decimal exponent. Always renders one decimal place so a 900
/// KB/s reading and a 1.0 MB/s reading stay distinguishable.
///
/// `u64 → f64` only loses precision above 9 PB/s — far beyond any
/// realistic TUN link — and even then the rounded mantissa is still
/// fine for the human-facing label this drives. The cast is therefore
/// acceptable for the overlay.
fn render_at_exponent(raw: u64, exponent: u32) -> (f64, &'static str) {
    let denominator = 10_u64.checked_pow(exponent.saturating_mul(3)).unwrap_or(1);
    #[expect(
        clippy::cast_precision_loss,
        reason = "overlay formatter accepts the f64 rounding above 9 PB/s"
    )]
    let value = if denominator == 0 {
        raw as f64
    } else {
        raw as f64 / denominator as f64
    };
    let label = match exponent {
        0 => "",
        1 => "K",
        2 => "M",
        _ => "G",
    };
    (value, label)
}

fn format_bandwidth(value: f64, label: &str, suffix: char) -> String {
    if value < 0.05 {
        // Treat anything that rounds to "0.0" as honest zero so the
        // overlay does not flicker between "0.0 B" and "1.0 KB".
        return format!("0 {label}{suffix}");
    }
    if value >= 100.0 {
        // Three or more leading digits: a single decimal steals useful
        // precision on the overlay, so drop it.
        format!("{value:.0} {label}{suffix}")
    } else {
        format!("{value:.1} {label}{suffix}")
    }
}

/// Compact representation of a single-direction rate, designed to fit a
/// 32×32 tray icon row without losing readability.
///
/// `digits` holds the integer value right-aligned in three slots:
/// `Some(d)` paints the digit `d`, `None` leaves the slot blank so
/// `16` renders as `· 1 6` instead of `0 1 6`. `unit` carries the
/// decimal unit letter (`B` / `K` / `M` / `G` for bytes, the same
/// capital letters for bits) so the operator still knows what the
/// bar's fill ratio means after auto-scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TaskbarCompactRate {
    pub hundreds: Option<u8>,
    pub tens: Option<u8>,
    pub ones: Option<u8>,
    pub unit: Option<char>,
}

impl TaskbarCompactRate {
    /// True when every slot is empty — used by renderers to skip the
    /// row entirely when the rate has not produced a sample yet.
    #[must_use]
    pub const fn is_blank(&self) -> bool {
        self.hundreds.is_none() && self.tens.is_none() && self.ones.is_none() && self.unit.is_none()
    }
}

/// Render `bytes_per_second` as a [`TaskbarCompactRate`] suitable for
/// the 32×32 tray icon.
///
/// Same auto-scaling ladder as [`format_bandwidth_rate`] but every
/// reading is rounded to a whole number and capped at `999 + unit`
/// so a saturated 256 MB/s line still paints as `16M` rather than
/// overflowing the icon. A pinned unit is honoured; values that
/// round to zero at the chosen unit (e.g. 200 KB/s with the
/// `Megabytes` pin) collapse to `0`.
#[must_use]
pub fn format_bandwidth_rate_compact(
    bytes_per_second: u64,
    display: TaskbarTrafficDisplay,
) -> TaskbarCompactRate {
    let multiplier = if display.bits { 8 } else { 1 };
    let raw = bytes_per_second.saturating_mul(multiplier);
    let exponent = display
        .unit
        .exponent()
        .unwrap_or_else(|| compact_auto_exponent(raw));
    let divisor = 10_u64
        .checked_pow(exponent.saturating_mul(3))
        .unwrap_or(1)
        .max(1);
    let value = raw / divisor;
    let value = value.min(999);
    let unit_letter = match exponent {
        0 => Some(if display.bits { 'b' } else { 'B' }),
        1 => Some('K'),
        2 => Some('M'),
        _ => Some('G'),
    };
    let ones = u8::try_from(value % 10).unwrap_or(0);
    let tens = u8::try_from((value / 10) % 10).unwrap_or(0);
    let hundreds = u8::try_from(value / 100).unwrap_or(0);
    TaskbarCompactRate {
        hundreds: if value >= 100 { Some(hundreds) } else { None },
        tens: if value >= 10 { Some(tens) } else { None },
        ones: Some(ones),
        unit: unit_letter,
    }
}

fn compact_auto_exponent(raw: u64) -> u32 {
    if raw < 1_000 {
        0
    } else if raw < 1_000_000 {
        1
    } else if raw < 1_000_000_000 {
        2
    } else {
        3
    }
}

#[cfg(test)]
mod behavior_tests {
    use super::{
        format_bandwidth_pair, format_bandwidth_rate, format_bandwidth_rate_compact,
        BehaviorConfig, TaskbarTrafficDisplay, TaskbarTrafficUnit,
    };

    #[test]
    fn default_behavior_keeps_the_overlay_on_with_auto_bytes() {
        let config = BehaviorConfig::default();
        assert!(config.taskbar_traffic_enabled);
        assert_eq!(config.taskbar_traffic_unit, TaskbarTrafficUnit::Auto);
        assert!(!config.taskbar_traffic_bits);
    }

    #[test]
    fn zero_bytes_renders_an_idle_label() {
        let display = TaskbarTrafficDisplay::default();
        assert_eq!(format_bandwidth_rate(0, display), "0 B");
        let bits = TaskbarTrafficDisplay {
            bits: true,
            ..display
        };
        assert_eq!(format_bandwidth_rate(0, bits), "0 b");
    }

    #[test]
    fn auto_mode_climbs_through_kb_mb_and_gb() {
        let display = TaskbarTrafficDisplay::default();
        assert_eq!(format_bandwidth_rate(500, display), "500 B");
        assert_eq!(format_bandwidth_rate(2_400, display), "2.4 KB");
        assert_eq!(format_bandwidth_rate(7_500_000, display), "7.5 MB");
        assert_eq!(format_bandwidth_rate(2_300_000_000, display), "2.3 GB");
    }

    #[test]
    fn fixed_units_skip_the_auto_ladder() {
        let display = TaskbarTrafficDisplay {
            unit: TaskbarTrafficUnit::Megabytes,
            bits: false,
        };
        assert_eq!(format_bandwidth_rate(2_400_000, display), "2.4 MB");
        assert_eq!(format_bandwidth_rate(7_500, display), "0 MB");
    }

    #[test]
    fn bits_multiply_by_eight_and_swap_the_suffix() {
        let display = TaskbarTrafficDisplay {
            bits: true,
            ..TaskbarTrafficDisplay::default()
        };
        assert_eq!(format_bandwidth_rate(125_000, display), "1.0 Mb");
        assert_eq!(format_bandwidth_rate(2_400, display), "19.2 Kb");
    }

    #[test]
    fn hundred_or_more_drops_the_decimal() {
        let display = TaskbarTrafficDisplay::default();
        assert_eq!(format_bandwidth_rate(120_000_000, display), "120 MB");
        let bits = TaskbarTrafficDisplay {
            bits: true,
            ..display
        };
        assert_eq!(format_bandwidth_rate(15_000_000, bits), "120 Mb");
    }

    #[test]
    fn a_pair_returns_download_and_upload_in_order() {
        let display = TaskbarTrafficDisplay::default();
        let (down, up) = format_bandwidth_pair(2_400_000, 350_000, display);
        assert_eq!(down, "2.4 MB");
        assert_eq!(up, "350 KB");
    }

    #[test]
    fn display_can_be_built_from_behavior() {
        // ADR 0114: the formatter is the same regardless of platform,
        // and the settings store is the single source of truth.
        let config = BehaviorConfig {
            taskbar_traffic_unit: TaskbarTrafficUnit::Megabytes,
            taskbar_traffic_bits: true,
            ..BehaviorConfig::default()
        };
        let display = TaskbarTrafficDisplay::from(&config);
        assert_eq!(display.unit, TaskbarTrafficUnit::Megabytes);
        assert!(display.bits);
        assert_eq!(format_bandwidth_rate(125_000, display), "1.0 Mb");
    }

    #[test]
    fn unit_aliases_keep_existing_settings_alive() {
        // The first scaffolded settings wrote the short form
        // (`kb`/`mb`/`gb`/`b`); reading them must still land on the
        // full enum so a startup panic does not brick the installed
        // application (this file's regression fix).
        #[derive(serde::Deserialize)]
        struct Wrapper {
            unit: TaskbarTrafficUnit,
        }
        for (text, variant) in [
            ("b", TaskbarTrafficUnit::Bytes),
            ("kb", TaskbarTrafficUnit::Kilobytes),
            ("mb", TaskbarTrafficUnit::Megabytes),
            ("gb", TaskbarTrafficUnit::Gigabytes),
            ("auto", TaskbarTrafficUnit::Auto),
            ("bytes", TaskbarTrafficUnit::Bytes),
            ("kilobytes", TaskbarTrafficUnit::Kilobytes),
        ] {
            let parsed: Wrapper =
                toml::from_str(&format!("unit = \"{text}\"")).expect("alias parses");
            assert_eq!(parsed.unit, variant, "alias {text:?} should map correctly");
        }
    }

    fn assert_compact(
        bytes_per_second: u64,
        display: TaskbarTrafficDisplay,
        hundreds: Option<u8>,
        tens: Option<u8>,
        ones: Option<u8>,
        unit: Option<char>,
    ) {
        let compact = format_bandwidth_rate_compact(bytes_per_second, display);
        assert_eq!(
            compact,
            crate::TaskbarCompactRate {
                hundreds,
                tens,
                ones,
                unit,
            },
            "compact mismatch for {bytes_per_second} bps with {display:?}"
        );
    }

    #[test]
    fn compact_rate_right_aligns_short_values_and_paints_the_unit_letter() {
        let display = TaskbarTrafficDisplay::default();
        // 0 → "0B"
        assert_compact(0, display, None, None, Some(0), Some('B'));
        // 500 B/s → "500B"
        assert_compact(500, display, Some(5), Some(0), Some(0), Some('B'));
        // 2_400 B/s → "2K" (auto scale; integer-rounded)
        assert_compact(2_400, display, None, None, Some(2), Some('K'));
        // 7_500_000 B/s → "7M"
        assert_compact(7_500_000, display, None, None, Some(7), Some('M'));
        // 2_300_000_000 B/s → "2G"
        assert_compact(2_300_000_000, display, None, None, Some(2), Some('G'));
    }

    #[test]
    fn compact_rate_caps_saturated_links_at_three_digits_with_their_unit() {
        // 256 MB/s still fits in 3 digits at the megabyte unit; the
        // cap stops the renderer from painting more slots than the
        // icon physically has. Decimal MB (1 MB = 1_000_000 B) keeps
        // the rounding visible in the regression test.
        let display = TaskbarTrafficDisplay::default();
        assert_compact(
            256 * 1_000_000,
            display,
            Some(2),
            Some(5),
            Some(6),
            Some('M'),
        );
        // 999_999 MB/s rolls up into the gigabyte unit rather than
        // overflowing the digits column.
        assert_compact(
            999_999 * 1_000_000,
            display,
            Some(9),
            Some(9),
            Some(9),
            Some('G'),
        );
    }

    #[test]
    fn compact_rate_honours_a_pinned_unit() {
        let display = TaskbarTrafficDisplay {
            unit: TaskbarTrafficUnit::Megabytes,
            bits: false,
        };
        // 1 MB pinned → "1M"
        assert_compact(1024 * 1024, display, None, None, Some(1), Some('M'));
        // 850 KB at the MB pinned unit rounds to "0M" (honest zero).
        assert_compact(850 * 1024, display, None, None, Some(0), Some('M'));
    }

    #[test]
    fn compact_rate_uses_lowercase_b_for_bits() {
        let display = TaskbarTrafficDisplay {
            bits: true,
            ..TaskbarTrafficDisplay::default()
        };
        // 125_000 B/s = 1_000_000 b/s = 1 Mb at the megabit unit.
        assert_compact(125_000, display, None, None, Some(1), Some('M'));
        let bytes = TaskbarTrafficDisplay::default();
        // 125_000 B/s in auto mode lands on 125 KB at the kilobyte unit.
        assert_compact(125_000, bytes, Some(1), Some(2), Some(5), Some('K'));
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub field: String,
    pub code: String,
    pub message: String,
}

impl AppConfig {
    #[must_use]
    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        validate_loopback(
            "mihomo.controller_host",
            &self.mihomo.controller_host,
            &mut issues,
        );

        if self.mihomo.controller_secret.trim().len() < 32 {
            issues.push(issue(
                "mihomo.controller_secret",
                "SECRET_TOO_SHORT",
                "controller secret must contain at least 32 characters",
            ));
        }
        if self.rules.refresh_interval_minutes == 0 || self.rules.upstream_refresh_hours == 0 {
            issues.push(issue(
                "rules",
                "OUT_OF_RANGE",
                "refresh intervals must be non-zero",
            ));
        }
        if self.mihomo.tun_name.trim().is_empty() || self.mihomo.tun_name.len() > 64 {
            issues.push(issue(
                "mihomo.tun_name",
                "INVALID_TUN_NAME",
                "TUN name must contain between 1 and 64 characters",
            ));
        }
        validate_direct_dns(&self.mihomo, &mut issues);
        clients::validate_clients(&self.clients, self.default_route, &mut issues);

        let mut ports = vec![
            (
                "mihomo.controller_port".to_owned(),
                self.mihomo.controller_port,
            ),
            ("mihomo.mixed_port".to_owned(), self.mihomo.mixed_port),
            ("mihomo.dns_port".to_owned(), self.mihomo.dns_port),
        ];
        for (index, client) in self.clients.iter().enumerate() {
            if let ClientConfig::LocalProxy { port, .. } = client.config {
                ports.push((format!("clients.{index}.port"), port));
            }
        }
        for (index, (field, port)) in ports.iter().enumerate() {
            if *port == 0 {
                issues.push(issue(field, "INVALID_PORT", "port cannot be zero"));
            }
            if ports[..index].iter().any(|(_, previous)| previous == port) {
                issues.push(issue(
                    field,
                    "PORT_CONFLICT",
                    "configured ports must be unique",
                ));
            }
        }
        issues
    }

    #[must_use]
    pub fn redacted(&self) -> Self {
        let mut value = self.clone();
        value.mihomo.controller_secret = "[REDACTED]".into();
        for client in &mut value.clients {
            client.config = client.config.redacted();
        }
        value
    }
}

fn validate_direct_dns(mihomo: &MihomoConfig, issues: &mut Vec<ValidationIssue>) {
    if !mihomo.direct_dns_preset.applies_direct_policy() {
        return;
    }
    let servers = mihomo.direct_dns_resolvers();
    if servers.is_empty() {
        issues.push(issue(
            "mihomo.direct_dns_servers",
            "DIRECT_DNS_REQUIRED",
            "DIRECT DNS needs at least one resolver address",
        ));
        return;
    }
    if servers.len() > 4 {
        issues.push(issue(
            "mihomo.direct_dns_servers",
            "DIRECT_DNS_TOO_MANY",
            "DIRECT DNS accepts at most four resolver addresses",
        ));
    }
    for server in servers {
        match server.parse::<IpAddr>() {
            Ok(address) if is_usable_direct_dns(address) => {}
            Ok(_) => issues.push(issue(
                "mihomo.direct_dns_servers",
                "DIRECT_DNS_INVALID",
                "DIRECT DNS cannot be loopback, unspecified, multicast, or fake-ip",
            )),
            Err(_) => issues.push(issue(
                "mihomo.direct_dns_servers",
                "DIRECT_DNS_INVALID",
                "DIRECT DNS resolvers must be IP addresses, not host names",
            )),
        }
    }
}

fn is_usable_direct_dns(address: IpAddr) -> bool {
    if address.is_loopback() || address.is_unspecified() || address.is_multicast() {
        return false;
    }
    match address {
        IpAddr::V4(value) => {
            let octets = value.octets();
            !(octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
        }
        IpAddr::V6(_) => true,
    }
}

pub(crate) fn validate_loopback(field: &str, value: &str, issues: &mut Vec<ValidationIssue>) {
    match value.parse::<IpAddr>() {
        Ok(address) if address.is_loopback() => {}
        _ => issues.push(issue(
            field,
            "LOOPBACK_REQUIRED",
            "address must be a numeric loopback address",
        )),
    }
}

pub(crate) fn issue(field: &str, code: &str, message: &str) -> ValidationIssue {
    ValidationIssue {
        field: field.into(),
        code: code.into(),
        message: message.into(),
    }
}

fn generate_secret() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("configuration I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("configuration is not valid TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("configuration could not be serialized: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("unsupported configuration schema version {0}")]
    UnsupportedSchema(u32),
    #[error("configuration validation failed")]
    Validation(Vec<ValidationIssue>),
    #[error("configuration revision conflict: expected {expected}, found {actual}")]
    RevisionConflict { expected: u64, actual: u64 },
    #[error("atomic configuration publication failed: {0}")]
    Persist(#[from] tempfile::PersistError),
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
    profile: RuntimeProfile,
}

impl ConfigStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, profile: RuntimeProfile) -> Self {
        Self {
            path: path.into(),
            profile,
        }
    }

    /// Profile this store isolates itself against.
    #[must_use]
    pub fn profile(&self) -> &RuntimeProfile {
        &self.profile
    }

    /// Loads the stored configuration, creating a validated default when absent.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when the file cannot be read, parsed, migrated,
    /// validated, or atomically persisted.
    pub fn load_or_create(&self) -> Result<AppConfig, ConfigError> {
        if !self.path.exists() {
            let mut config = AppConfig::default();
            config.mihomo.isolate_from_installed_app(&self.profile);
            self.write_atomic(&config)?;
            return Ok(config);
        }
        self.load()
    }

    /// Loads, migrates, and validates the stored configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] for I/O or TOML failures, unsupported schema
    /// versions, failed migrations, or invalid configuration values.
    pub fn load(&self) -> Result<AppConfig, ConfigError> {
        let source = fs::read_to_string(&self.path)?;
        let mut value: toml::Value = toml::from_str(&source)?;
        let schema = value
            .get("schema_version")
            .and_then(toml::Value::as_integer)
            .unwrap_or(0);
        let schema = u32::try_from(schema).unwrap_or(u32::MAX);
        if schema > CURRENT_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchema(schema));
        }
        if schema < CURRENT_SCHEMA_VERSION {
            self.backup()?;
            migrate(&mut value, schema)?;
        }
        let mut config: AppConfig = value.try_into()?;
        let isolated = config.mihomo.isolate_from_installed_app(&self.profile);
        let tun_normalized = config.mihomo.normalize_tun_name_for_platform();
        let dns_normalized = config.mihomo.normalize_dns_port_for_platform();
        let issues = config.validate();
        if !issues.is_empty() {
            return Err(ConfigError::Validation(issues));
        }
        if schema < CURRENT_SCHEMA_VERSION || isolated || tun_normalized || dns_normalized {
            self.write_atomic(&config)?;
        }
        Ok(config)
    }

    /// Validates and atomically saves a configuration at the expected revision.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when the current configuration cannot be loaded,
    /// the revision conflicts, validation fails, or atomic persistence fails.
    pub fn save(
        &self,
        mut config: AppConfig,
        expected_revision: u64,
    ) -> Result<AppConfig, ConfigError> {
        let current = self.load()?;
        if current.revision != expected_revision {
            return Err(ConfigError::RevisionConflict {
                expected: expected_revision,
                actual: current.revision,
            });
        }
        let issues = config.validate();
        if !issues.is_empty() {
            return Err(ConfigError::Validation(issues));
        }
        config.schema_version = CURRENT_SCHEMA_VERSION;
        config.revision = current.revision.saturating_add(1);
        self.write_atomic(&config)?;
        Ok(config)
    }

    fn backup(&self) -> Result<(), ConfigError> {
        let backup = self.path.with_extension("toml.bak");
        fs::copy(&self.path, backup)?;
        Ok(())
    }

    fn write_atomic(&self, config: &AppConfig) -> Result<(), ConfigError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let content = toml::to_string_pretty(config)?;
        let mut temporary = NamedTempFile::new_in(parent)?;
        temporary.write_all(content.as_bytes())?;
        temporary.as_file().sync_all()?;
        #[cfg(unix)]
        set_private_permissions(temporary.path())?;
        #[cfg(not(unix))]
        set_private_permissions(temporary.path());
        temporary.persist(&self.path)?;
        #[cfg(unix)]
        sync_directory(parent)?;
        #[cfg(not(unix))]
        sync_directory(parent);
        Ok(())
    }
}

fn migrate(value: &mut toml::Value, from: u32) -> Result<(), ConfigError> {
    if from >= CURRENT_SCHEMA_VERSION {
        return Err(ConfigError::UnsupportedSchema(from));
    }
    let table = value
        .as_table_mut()
        .ok_or_else(|| ConfigError::UnsupportedSchema(from))?;
    if from == 0 {
        table.entry("revision").or_insert(toml::Value::Integer(0));
    }
    if from < 2 {
        if let Some(mihomo) = table.get_mut("mihomo").and_then(toml::Value::as_table_mut) {
            if mihomo
                .get("direct_dns_preset")
                .and_then(toml::Value::as_str)
                == Some("shecan")
            {
                mihomo.insert(
                    "direct_dns_preset".into(),
                    toml::Value::String("fake_ip".into()),
                );
            }
        }
    }
    if from < 3 {
        migrate_clients_v3(table);
    }
    table.insert(
        "schema_version".into(),
        toml::Value::Integer(i64::from(CURRENT_SCHEMA_VERSION)),
    );
    Ok(())
}

fn migrate_clients_v3(table: &mut toml::map::Map<String, toml::Value>) {
    if table
        .get("clients")
        .and_then(toml::Value::as_array)
        .is_some()
    {
        table.remove("hiddify");
        table.remove("openvpn");
        if !table.contains_key("default_route") {
            table.insert("default_route".into(), default_route_direct());
        }
        return;
    }

    let mut clients = Vec::new();
    let mut default_route = default_route_direct();

    if let Some(hiddify) = table.remove("hiddify") {
        let id = ClientId::new();
        clients.push(toml::Value::Table(local_proxy_client_table(
            id,
            "hiddify",
            hiddify.as_table(),
        )));
        default_route = default_route_client(id);
    }

    if let Some(openvpn) = table.remove("openvpn") {
        let id = ClientId::new();
        clients.push(toml::Value::Table(side_tunnel_client_table(
            id,
            "openvpn",
            openvpn.as_table(),
        )));
        if clients.len() == 1 {
            default_route = default_route_client(id);
        }
    }

    if clients.is_empty() {
        let id = ClientId::new();
        clients.push(toml::Value::Table(local_proxy_client_table(
            id, "hiddify", None,
        )));
        default_route = default_route_client(id);
    }

    table.insert("clients".into(), toml::Value::Array(clients));
    table.insert("default_route".into(), default_route);
}

fn default_route_direct() -> toml::Value {
    let mut table = toml::map::Map::new();
    table.insert("kind".into(), toml::Value::String("direct".into()));
    toml::Value::Table(table)
}

fn default_route_client(id: ClientId) -> toml::Value {
    let mut table = toml::map::Map::new();
    table.insert("kind".into(), toml::Value::String("client".into()));
    table.insert("client_id".into(), toml::Value::String(id.as_hyphenated()));
    toml::Value::Table(table)
}

fn local_proxy_client_table(
    id: ClientId,
    preset: &str,
    source: Option<&toml::map::Map<String, toml::Value>>,
) -> toml::map::Map<String, toml::Value> {
    let spec = PresetId::Hiddify.spec();
    let mut config = toml::map::Map::new();
    config.insert("kind".into(), toml::Value::String("local_proxy".into()));
    config.insert(
        "host".into(),
        source
            .and_then(|table| table.get("host").cloned())
            .unwrap_or_else(|| toml::Value::String(spec.default_host.into())),
    );
    config.insert(
        "port".into(),
        source
            .and_then(|table| table.get("port").cloned())
            .unwrap_or_else(|| toml::Value::Integer(i64::from(spec.default_port.unwrap_or(1080)))),
    );
    config.insert(
        "executable".into(),
        source
            .and_then(|table| table.get("executable").cloned())
            .unwrap_or_else(|| toml::Value::String("auto".into())),
    );
    config.insert(
        "start_timeout_seconds".into(),
        source
            .and_then(|table| table.get("start_timeout_seconds").cloned())
            .unwrap_or(toml::Value::Integer(45)),
    );
    config.insert(
        "stop_with_stack".into(),
        source
            .and_then(|table| table.get("stop_with_stack").cloned())
            .unwrap_or(toml::Value::Boolean(true)),
    );
    client_table(id, preset, config)
}

fn side_tunnel_client_table(
    id: ClientId,
    preset: &str,
    source: Option<&toml::map::Map<String, toml::Value>>,
) -> toml::map::Map<String, toml::Value> {
    let mut config = toml::map::Map::new();
    config.insert(
        "kind".into(),
        toml::Value::String("owned_side_tunnel".into()),
    );
    if let Some(path) = source.and_then(|table| table.get("profile_path").cloned()) {
        config.insert("profile_path".into(), path);
    }
    config.insert(
        "executable".into(),
        source
            .and_then(|table| table.get("executable").cloned())
            .unwrap_or_else(|| toml::Value::String("auto".into())),
    );
    config.insert(
        "start_timeout_seconds".into(),
        source
            .and_then(|table| table.get("start_timeout_seconds").cloned())
            .unwrap_or(toml::Value::Integer(45)),
    );
    client_table(id, preset, config)
}

fn client_table(
    id: ClientId,
    preset: &str,
    config: toml::map::Map<String, toml::Value>,
) -> toml::map::Map<String, toml::Value> {
    let mut table = toml::map::Map::new();
    table.insert("id".into(), toml::Value::String(id.as_hyphenated()));
    table.insert("preset".into(), toml::Value::String(preset.into()));
    table.insert("enabled".into(), toml::Value::Boolean(true));
    table.insert("config".into(), toml::Value::Table(config));
    table
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) {}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), std::io::Error> {
    let directory = fs::OpenOptions::new().read(true).open(path)?;
    directory.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Production store for tests that are not about profile isolation. The
    /// profile is explicit, so these tests no longer depend on the ambient
    /// `BIFLOW_DEV_PROFILE` of the machine running them.
    fn store(path: impl Into<PathBuf>) -> ConfigStore {
        ConfigStore::new(path, RuntimeProfile::Production)
    }

    #[test]
    fn defaults_are_valid_and_secret_is_random() {
        let first = AppConfig::default();
        let second = AppConfig::default();
        assert!(first.validate().is_empty());
        assert_ne!(
            first.mihomo.controller_secret,
            second.mihomo.controller_secret
        );
        assert_eq!(first.mihomo.controller_secret.len(), 64);
        assert_eq!(first.mihomo.direct_dns_preset, DirectDnsPreset::FakeIp);
        assert_eq!(first.mihomo.direct_dns_preset.to_string(), "fake_ip");
        assert!(first.mihomo.direct_dns_resolvers().is_empty());
    }

    #[test]
    fn isolate_from_installed_app_follows_the_supplied_profile() {
        let production = RuntimeProfile::Production;
        let development = RuntimeProfile::Development {
            root: PathBuf::from("/tmp/dev-profile"),
        };

        let mut mihomo = MihomoConfig::default();
        assert!(!mihomo.isolate_from_installed_app(&production));
        assert_eq!(mihomo.controller_port, PRODUCTION_CONTROLLER_PORT);
        assert_eq!(mihomo.tun_name, PRODUCTION_TUN_NAME);

        assert!(mihomo.isolate_from_installed_app(&development));
        assert_eq!(mihomo.controller_port, DEV_PROFILE_CONTROLLER_PORT);
        assert_eq!(mihomo.mixed_port, DEV_PROFILE_MIXED_PORT);
        assert_eq!(mihomo.dns_port, DEV_PROFILE_DNS_PORT);
        assert_eq!(mihomo.tun_name, DEV_PROFILE_TUN_NAME);
        assert!(!mihomo.isolate_from_installed_app(&development));
    }

    #[test]
    fn a_development_store_isolates_a_freshly_created_config() {
        // Regression: the store used to read `BIFLOW_DEV_PROFILE` itself, so
        // isolation silently depended on the ambient environment of whatever
        // process loaded the config. It is now an explicit input.
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config.toml");
        let store = ConfigStore::new(
            &path,
            RuntimeProfile::Development {
                root: directory.path().to_path_buf(),
            },
        );
        let created = store.load_or_create().expect("create config");
        assert_eq!(created.mihomo.controller_port, DEV_PROFILE_CONTROLLER_PORT);
        assert_eq!(created.mihomo.tun_name, DEV_PROFILE_TUN_NAME);

        // The remap is persisted, so a restart of the same development run
        // keeps the isolated identity instead of drifting back onto the
        // installed app's ports and TUN.
        let reloaded = store.load().expect("reload as development");
        assert_eq!(reloaded.mihomo.controller_port, DEV_PROFILE_CONTROLLER_PORT);
        assert_eq!(reloaded.mihomo.mixed_port, DEV_PROFILE_MIXED_PORT);
        assert_eq!(reloaded.mihomo.tun_name, DEV_PROFILE_TUN_NAME);
        let persisted = toml::from_str::<toml::Value>(&fs::read_to_string(&path).expect("read"))
            .expect("parse")
            .get("mihomo")
            .and_then(|mihomo| mihomo.get("controller_port"))
            .and_then(toml::Value::as_integer)
            .unwrap_or_default();
        assert_eq!(
            u32::try_from(persisted).unwrap_or(u32::MAX),
            u32::from(DEV_PROFILE_CONTROLLER_PORT)
        );
    }

    #[test]
    fn a_production_store_keeps_production_identity() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config.toml");
        let created = store(&path).load_or_create().expect("create config");
        assert_eq!(created.mihomo.controller_port, PRODUCTION_CONTROLLER_PORT);
        assert_eq!(created.mihomo.tun_name, PRODUCTION_TUN_NAME);
    }

    #[test]
    fn isolate_from_installed_app_if_moves_default_ports_and_tun() {
        let mut mihomo = MihomoConfig::default();
        assert!(!mihomo.isolate_from_installed_app_if(false));
        assert_eq!(mihomo.controller_port, PRODUCTION_CONTROLLER_PORT);
        assert!(mihomo.isolate_from_installed_app_if(true));
        assert_eq!(mihomo.controller_port, DEV_PROFILE_CONTROLLER_PORT);
        assert_eq!(mihomo.mixed_port, DEV_PROFILE_MIXED_PORT);
        assert_eq!(mihomo.dns_port, DEV_PROFILE_DNS_PORT);
        assert_eq!(mihomo.tun_name, DEV_PROFILE_TUN_NAME);
        assert!(!mihomo.isolate_from_installed_app_if(true));

        let mut custom = MihomoConfig {
            controller_port: 19_095,
            ..MihomoConfig::default()
        };
        assert!(custom.isolate_from_installed_app_if(true));
        assert_eq!(custom.controller_port, 19_095);
        assert_eq!(custom.mixed_port, DEV_PROFILE_MIXED_PORT);
    }

    #[test]
    fn macos_tun_name_validation_accepts_utun_and_rejects_arbitrary_names() {
        assert!(is_valid_macos_tun_name("utun9"));
        assert!(is_valid_macos_tun_name("utun0"));
        assert!(is_valid_macos_tun_name("utun123"));
        assert!(!is_valid_macos_tun_name("clash-iran"));
        assert!(!is_valid_macos_tun_name("biflow-dev"));
        assert!(!is_valid_macos_tun_name("utun"));
        assert!(!is_valid_macos_tun_name("utun9999"));
        assert!(!is_valid_macos_tun_name("utun9a"));
    }

    #[test]
    fn macos_normalize_tun_name_rewrites_clash_iran_to_utun() {
        let mut mihomo = MihomoConfig::default();
        assert_eq!(mihomo.tun_name, PRODUCTION_TUN_NAME);
        let changed = mihomo.normalize_tun_name_for_platform();
        if cfg!(target_os = "macos") {
            assert!(changed);
            assert_eq!(mihomo.tun_name, MACOS_TUN_NAME);
        } else {
            assert!(!changed);
            assert_eq!(mihomo.tun_name, PRODUCTION_TUN_NAME);
        }

        // A manually set utun name is preserved.
        mihomo.tun_name = "utun7".into();
        assert!(!mihomo.normalize_tun_name_for_platform());
        assert_eq!(mihomo.tun_name, "utun7");
    }

    #[test]
    fn macos_normalize_dns_port_moves_off_loopback_high_port() {
        let mut mihomo = MihomoConfig::default();
        // The production default is a high loopback port; on macOS we need 53.
        let changed = mihomo.normalize_dns_port_for_platform();
        if cfg!(target_os = "macos") {
            assert!(changed);
            assert_eq!(mihomo.dns_port, MACOS_DNS_PORT);
        } else {
            assert!(!changed);
        }

        // A manually set 53 is preserved on macOS, and untouched elsewhere.
        mihomo.dns_port = MACOS_DNS_PORT;
        assert!(!mihomo.normalize_dns_port_for_platform());
        assert_eq!(mihomo.dns_port, MACOS_DNS_PORT);
    }

    #[test]
    fn happ_primary_without_hiddify_is_a_valid_config() {
        // The operator removed the Hiddify instance and promoted Happ to the
        // default route: the config must validate and the legacy Hiddify
        // endpoint helpers must fall back to catalog defaults, not panic.
        let mut config = AppConfig::default();
        config.clients.clear();
        let happ = ClientInstance::from_preset(PresetId::Happ);
        config.default_route = DefaultRoute::client(happ.id);
        config.clients.push(happ);
        assert!(config.validate().is_empty());
        assert!(config.hiddify_client().is_none());
        assert_eq!(config.enabled_clients().len(), 1);
        assert_eq!(config.hiddify_endpoint(), ("127.0.0.1".into(), 12_334));
    }

    #[test]
    fn mokhaberat_and_radar_presets_are_valid_addresses() {
        let mut config = AppConfig::default();
        config.mihomo.direct_dns_preset = DirectDnsPreset::Mokhaberat;
        assert!(config.validate().is_empty());
        assert_eq!(config.mihomo.direct_dns_resolvers(), ["5.200.200.200"]);
        config.mihomo.direct_dns_preset = DirectDnsPreset::Radar;
        assert!(config.validate().is_empty());
        assert_eq!(
            config.mihomo.direct_dns_resolvers(),
            ["10.202.10.10", "10.202.10.11"]
        );
    }

    #[test]
    fn custom_direct_dns_rejects_empty_and_loopback() {
        let mut config = AppConfig::default();
        config.mihomo.direct_dns_preset = DirectDnsPreset::Custom;
        assert!(config
            .validate()
            .iter()
            .any(|item| item.code == "DIRECT_DNS_REQUIRED"));
        config.mihomo.direct_dns_servers = vec!["127.0.0.1".into()];
        assert!(config
            .validate()
            .iter()
            .any(|item| item.code == "DIRECT_DNS_INVALID"));
        config.mihomo.direct_dns_servers = vec!["5.200.200.200".into(), "1.1.1.1".into()];
        assert!(config.validate().is_empty());
    }

    #[test]
    fn schema_v1_implicit_shecan_migrates_to_fake_ip() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config.toml");
        let mut config = AppConfig {
            schema_version: 1,
            ..AppConfig::default()
        };
        config.mihomo.direct_dns_preset = DirectDnsPreset::Shecan;
        fs::write(&path, toml::to_string(&config).expect("toml")).expect("write");
        let loaded = store(&path).load().expect("load");
        assert_eq!(loaded.schema_version, 3);
        assert_eq!(loaded.mihomo.direct_dns_preset, DirectDnsPreset::FakeIp);
        assert_eq!(loaded.clients.len(), 1);
        assert_eq!(loaded.clients[0].preset, PresetId::Hiddify);
        assert!(matches!(loaded.default_route, DefaultRoute::Client { .. }));
    }

    #[test]
    fn schema_v1_mokhaberat_survives_direct_dns_migration() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config.toml");
        let mut config = AppConfig {
            schema_version: 1,
            ..AppConfig::default()
        };
        config.mihomo.direct_dns_preset = DirectDnsPreset::Mokhaberat;
        fs::write(&path, toml::to_string(&config).expect("toml")).expect("write");
        let loaded = store(&path).load().expect("load");
        assert_eq!(loaded.schema_version, 3);
        assert_eq!(loaded.mihomo.direct_dns_preset, DirectDnsPreset::Mokhaberat);
    }

    #[test]
    fn rejects_remote_controller_and_conflicting_ports() {
        let mut config = AppConfig::default();
        config.mihomo.controller_host = "0.0.0.0".into();
        config.mihomo.mixed_port = config.mihomo.controller_port;
        let issues = config.validate();
        assert!(issues.iter().any(|item| item.code == "LOOPBACK_REQUIRED"));
        assert!(issues.iter().any(|item| item.code == "PORT_CONFLICT"));
    }

    #[test]
    fn persists_atomically_and_checks_revision() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config.toml");
        let config_store = store(&path);
        let mut config = config_store.load_or_create().expect("create config");
        config.behavior.connect_at_launch = true;
        let saved = config_store.save(config.clone(), 0).expect("save config");
        assert_eq!(saved.revision, 1);
        assert!(config_store.save(config, 0).is_err());
        assert!(
            config_store
                .load()
                .expect("reload")
                .behavior
                .connect_at_launch
        );
    }

    #[test]
    fn redaction_removes_secret_and_parent_path() {
        let mut config = AppConfig::default();
        if let Some(client) = config.clients.first_mut() {
            client.config = ClientConfig::LocalProxy {
                host: "127.0.0.1".into(),
                port: 12_334,
                executable: ExecutableSetting::Path(PathBuf::from("/home/alice/Hiddify.AppImage")),
                start_timeout_seconds: 45,
                stop_with_stack: true,
            };
        }
        let redacted = config.redacted();
        assert_eq!(redacted.mihomo.controller_secret, "[REDACTED]");
        assert_eq!(
            redacted.clients[0].config,
            ClientConfig::LocalProxy {
                host: "127.0.0.1".into(),
                port: 12_334,
                executable: ExecutableSetting::Path(PathBuf::from("Hiddify.AppImage")),
                start_timeout_seconds: 45,
                stop_with_stack: true,
            }
        );
    }

    #[test]
    fn schema_v2_hiddify_migrates_to_a_client_match_default() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config.toml");
        let contents = r#"
schema_version = 2
revision = 4

[hiddify]
host = "127.0.0.1"
port = 12334
executable = "auto"
start_timeout_seconds = 45
stop_with_stack = true

[mihomo]
controller_host = "127.0.0.1"
controller_port = 19090
controller_secret = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
mixed_port = 17890
dns_port = 1053
tun_name = "clash-iran"
log_level = "info"
direct_dns_preset = "fake_ip"
direct_dns_servers = []

[rules]
refresh_interval_minutes = 15
upstream_refresh_hours = 24

[behavior]
launch_at_login = false
connect_at_launch = false
close_to_tray = true
"#;
        fs::write(&path, contents).expect("write");
        let loaded = store(&path).load().expect("load");
        assert_eq!(loaded.schema_version, 3);
        assert_eq!(loaded.clients.len(), 1);
        assert_eq!(loaded.clients[0].preset, PresetId::Hiddify);
        assert!(loaded.clients[0].enabled);
        let DefaultRoute::Client { client_id } = loaded.default_route else {
            panic!("expected MATCH to the migrated Hiddify instance");
        };
        assert_eq!(client_id, loaded.clients[0].id);
    }

    #[test]
    fn schema_v2_openvpn_blob_becomes_a_side_tunnel_instance() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("config.toml");
        let contents = r#"
schema_version = 2
revision = 1

[hiddify]
host = "127.0.0.1"
port = 12334
executable = "auto"
start_timeout_seconds = 45
stop_with_stack = true

[openvpn]
profile_path = "/tmp/office.ovpn"

[mihomo]
controller_host = "127.0.0.1"
controller_port = 19090
controller_secret = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
mixed_port = 17890
dns_port = 1053
tun_name = "clash-iran"
log_level = "info"
direct_dns_preset = "fake_ip"

[rules]
refresh_interval_minutes = 15
upstream_refresh_hours = 24

[behavior]
close_to_tray = true
"#;
        fs::write(&path, contents).expect("write");
        let loaded = store(&path).load().expect("load");
        assert_eq!(loaded.clients.len(), 2);
        assert!(loaded
            .clients
            .iter()
            .any(|client| client.preset == PresetId::Openvpn));
        assert!(matches!(
            loaded
                .clients
                .iter()
                .find(|client| client.preset == PresetId::Openvpn)
                .map(|client| &client.config),
            Some(ClientConfig::OwnedSideTunnel { .. })
        ));
        assert!(matches!(loaded.default_route, DefaultRoute::Client { .. }));
    }

    #[test]
    fn disabling_the_match_client_falls_back_to_direct() {
        let mut config = AppConfig::default();
        let id = config.clients[0].id;
        assert_eq!(config.default_route, DefaultRoute::client(id));
        config.clients[0].enabled = false;
        assert!(config.sanitize_default_route());
        assert_eq!(config.default_route, DefaultRoute::Direct);
    }

    #[cfg(unix)]
    #[test]
    fn private_permissions_restrict_to_owner_read_write() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("secret.toml");
        fs::write(&path, "x").expect("write");
        set_private_permissions(&path).expect("chmod");
        let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(not(unix))]
    #[test]
    fn private_permissions_are_a_noop_off_unix() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("secret.toml");
        fs::write(&path, "x").expect("write");
        set_private_permissions(&path);
        assert!(path.is_file());
    }
}
