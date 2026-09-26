# 0105: Probe side-tunnel egress and replace an on-link TAP route

## Status

Accepted

## Context

Choosing Windscribe reloads Mihomo onto `OpenVPN TAP-Windows6`, and the
helper reports the interface route as installed. The adapter's default
gateway stays `0.0.0.0`. A connection from that address to `1.1.1.1:80`
times out, so no site opens. `netsh` answers "already exists" for the
old on-link route, and that reply was treated as success. The desktop log
then has no reason a person can act on.

## Decision

- On a TAP adapter, delete the on-link default route and install the
  OpenVPN gateway. Success is a `show addresses` gateway that is not
  `0.0.0.0`. "Already exists" is not success.
- `BiFlow.exe probe windscribe` and Diagnostics → **Test tunnel egress**
  connect to `1.1.1.1:80` from the Mihomo `interface-name` and write the
  reason to `probe-report.txt`. The report names the adapter and whether
  the route is on-link. It does not include the gateway address, the
  controller secret, or the target host the operator typed.

## Consequences

A TAP tunnel that cannot deliver packets fails the route install with that
reason, and the same check can be repeated without guessing from the
dashboard.
