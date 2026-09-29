# 0110: Log a host environment snapshot in every debug.log

## Status

Accepted

## Context

The same build connects on one Windows or Linux host and fails on another:
Windscribe does not come up, a local proxy port is unreachable while
connected, or `kubectl` leaves DIRECT. Reports arrive as `debug.log` files,
often after the operator pressed **Delete log**, so the file holds only
polling events. It does not say which OS build, firewall, antivirus, other
VPN, system proxy, or `HTTPS_PROXY` the host has. Each report then turns
into a round of questions.

## Decision

- `src-tauri/src/environment.rs` collects one report per trigger and logs
  it as `environment.snapshot`. The summary fields are the OS, firewall,
  VPN processes, and findings. `details` holds the full JSON. Each finding
  is also a `WARN` `environment.finding` event.
- Windows uses one bounded PowerShell script: `Win32_OperatingSystem`,
  `Get-NetFirewallProfile`, `SecurityCenter2`, `Get-NetAdapter`, default
  and split-default routes with metrics, DNS servers, Internet Settings,
  WinHTTP, user and machine proxy variables, processes, and TCP listeners
  with owners. Linux reads `/etc/os-release`, `/sys/class/net`,
  `/proc/net/{route,tcp,tcp6}`, `resolv.conf`, `/proc/*/comm`,
  `systemctl is-active`, GNOME `gsettings`, and KDE `kioslaverc`.
- On every host, the report adds the process proxy variables, the hosts
  file (entry count and the class that `localhost` maps to), each
  kubeconfig cluster (host class, port, `proxy-url`, and the route
  `RuleSet::decide` gives it), and a summary of the BiFlow configuration.
- `findings` are stable `name:detail` strings. Examples:
  `client_port_not_listening`, `port_conflict`,
  `system_proxy_unknown_local_port`, `env_proxy_set`,
  `kube_cluster_direct`, `other_tunnel_adapter_up`,
  `split_default_route`, `other_vpn_process`, `third_party_security`, and
  `hosts_localhost_not_loopback`.
- Triggers: startup, Connect, transitions to Running, Degraded, or Error,
  Diagnostics delete (the cached report is replayed at once, then
  refreshed), a log recreated after it was deleted outside the app, and
  support export (`environment.json`). Only one collection runs at a time.
- `debug.log` recreates itself when the file vanished under the open
  handle. Without this, Linux keeps writing into an unlinked inode and
  Windows into a delete-pending file.
- Privacy: only address classes and ports are logged. Nothing else
  leaves the host: no resolver, gateway, proxy, or cluster addresses, no
  hostnames or user paths, and no process names outside the known
  VPN/proxy list.

## Consequences

Every report says what else is on the host, even after the log was cleared.
A new host-specific failure should add the signal that would have exposed
it to the collector and to `findings`, so the next report shows the cause
directly.
