# 0112: Windows TUN keeps IPv6 so strict-route does not block `::1`

## Status

Accepted. Corrects ADR 0108 and the Windows `ipv6: false` choice in ADR 0038.

## Context

On a Windows 11 host, `localhost:<port>` failed while connected. That
covered Docker Desktop ports, dev servers, and Hiddify's own port. The
host resolves `localhost` to `::1` first. The environment snapshot's
loopback self-test (ADR 0110) showed the change caused by Connect:

| probe                 | stopped | running                    |
| --------------------- | ------- | -------------------------- |
| ephemeral `127.0.0.1` | ok      | ok                         |
| ephemeral `[::1]`     | ok      | refused immediately (0 ms) |
| Hiddify `[::1]:12334` | ok      | refused                    |

ADR 0108's `route-exclude-address: ::1/128` could not help, because the
connections are not being routed: they are refused. With top-level
`ipv6: false`, Mihomo's `parseIPV6` clears `tun.inet6-address`. When
`strict-route` is on and there is no inet6 address, sing-tun adds a WFP
filter named "block ipv6" on `FWPM_LAYER_ALE_AUTH_CONNECT_V6`. The filter
has no conditions. The only permit filter above it is for Mihomo's own
application ID, so every other process loses all IPv6, loopback included.

## Decision

- Generate top-level `ipv6: true` on Windows, as Linux already does. The
  TUN keeps `fdfe:dcba:9876::1/126`, so the block filter is not installed.
- Keep `dns.ipv6: false` on Windows. Fake-ip answers stay IPv4-only, so
  domain routing is unchanged.
- Keep `strict-route` and the loopback route exclusions.
- IPv6 literal traffic (for example a browser's own DoH AAAA answers) now
  enters TUN and follows the rules instead of being dropped.

- Kubeconfig clusters in the snapshot also record their address family,
  a 3-second TCP connect result from the desktop, and the file names of
  `exec` credential plugins. `kube_cluster_unreachable` then tells "BiFlow
  breaks the cluster" (fails only while running) apart from "the cluster
  is unreachable from this network" (fails while stopped too).

## Consequences

`localhost` works while connected on hosts that prefer `::1`. A future
regression shows up as `loopback_ipv6_blocked` in the snapshot. Findings
no longer flag BiFlow's own split-default routes, one IPv4 plus one IPv6
default route, or a SYSTEM helper task that a standard user cannot list
while the helper is reachable.
