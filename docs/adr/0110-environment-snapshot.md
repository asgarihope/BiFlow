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

## Functional signals (6.2.48)

Host inventory alone did not explain the recurring functional reports.
Examples: "Windscribe does not connect", "Pause cuts my internet", and
"Connect fails" when the user had changed Hiddify behind BiFlow. The
snapshot now also records the following.

- **Runtime:** the live `StackSnapshot` (component phases, messages, the
  per-client status, and the `last_error` code with technical details) and
  the helper's availability, authorization, and version. Exit IPs are
  dropped and addresses in messages are replaced with their class.
- **Hiddify as it really runs:** from `shared_preferences.json`, only
  `service-mode`, `region`, and `started_by_user`. From
  `data/current-config.json`, only inbound types, listen classes, and
  ports, plus the clash-API port. Both files' modification times are
  included. Profiles, outbounds, and subscriptions are never read. The
  executable location is a class; it is not run, because it is a GUI.
- **Side tunnels:** the OpenVPN binary location and `--version`. Per
  profile: the helper audit result, `proto`, `dev`, the remote count with
  host classes and port, `auth-user-pass` against saved credentials,
  cipher directives, compression, inline blocks, the number of referenced
  key or certificate files missing next to the profile, and whether the
  filtered remote is pinned in `side-tunnel-remotes.json`.
- **The Mihomo binary** location and `-v`.
- **Clock skew** against an HTTP `Date` header. A skewed clock breaks
  vmess/reality and OpenVPN TLS.
- **Install:** the kind, dev profile, elevated (root or Administrator),
  uptime, and locale.
- **Linux:** `ip rule`, and `ip route show table all` for IPv4 and IPv6
  with addresses classified. Also `rp_filter`, `/dev/net/tun`,
  `disable_ipv6`, `ip_forward`, and the relevant loaded modules; the
  active NetworkManager connection types; active services plus the helper
  unit state; and listener owners from socket inodes.
- **Windows:** the IPv6 default and split-default routes, route counts per
  interface, and service states. Also the `BiFlowHelper` scheduled-task
  state, and enabled firewall block rules that name a VPN, proxy, or
  BiFlow executable.
- **Pause and Disconnect** are snapshot triggers
  (`stack_paused`/`stack_stopped`).

New findings include:

- Runtime: `helper_version_mismatch`, `helper_unreachable`,
  `helper_service`, `stack_last_error`, and `client_status`.
- Hiddify: `hiddify_tun_mode_conflicts_with_biflow_tun` and
  `hiddify_port_mismatch`.
- Side tunnels: `side_tunnel_profile_rejected`,
  `side_tunnel_profile_missing_files`, `side_tunnel_credentials_missing`,
  `side_tunnel_remote_not_pinned`, `side_tunnel_tap_without_adapter`, and
  `openvpn_binary_missing`.
- Pause and Disconnect: `pause_cleared_hiddify_system_proxy` and the
  `leftover_*` family (TUN adapter, TUN routes, fake-ip DNS, and an OS
  proxy pointing at Mihomo).
- Routing: `system_proxy_bypasses_split_routing` and
  `ipv6_default_route_outside_tun`.
- Host: `rp_filter_strict`, `tun_device_missing`,
  `other_vpn_service_running`, `firewall_blocks_program`, `clock_skew_seconds`,
  and `app_running_elevated`.

## Consequences

Every report says what else is on the host, even after the log was cleared.
A new host-specific failure should add the signal that would have exposed
it to the collector and to `findings`, so the next report shows the cause
directly.
