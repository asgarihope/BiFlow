# 0104: A TAP side tunnel needs the OpenVPN gateway

## Status

Accepted

## Context

Setting the default route to Windscribe reloaded Mihomo onto
`interface-name: OpenVPN TAP-Windows6` and installed `0.0.0.0/0` with
nexthop `0.0.0.0`. TAP is a layer-2 adapter. That on-link route makes
Windows ARP for every destination, including DNS, and each lookup stays
unreachable, so no site opens. A DCO or TUN adapter can use an on-link
route. TAP cannot.

`route-gateway` is in OpenVPN's stderr `PUSH_REPLY`. The helper was reading
only stdout, so the gateway was missing and the fallback on-link route was
installed.

## Decision

- Read the OpenVPN session log from stdout and stderr, and keep
  `route-gateway`.
- On a TAP adapter, install the high-metric default route only via that
  gateway, and delete an existing on-link `0.0.0.0/0` on the same adapter.
  If the gateway was not learned, fail the route install instead of
  blackholing traffic.
- On DCO and TUN, keep the on-link fallback.

## Consequences

Unmatched traffic bound to a Windscribe TAP adapter is forwarded to the
tunnel gateway. A missing gateway is reported on the apply instead of
looking like a successful connect with a dead network.
