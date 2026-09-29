# Windows loopback routing

## Status

Accepted; the `::1` half is corrected by ADR 0112 (the block was a WFP filter, not a route).

## Context

Windows TUN `strict-route` can disrupt local services even when Mihomo's rule
list sends `localhost` and `127.0.0.0/8` to DIRECT. Local development servers
must remain usable without disconnecting BiFlow.

## Decision

- Exclude IPv4 and IPv6 loopback destinations (`127.0.0.0/8` and `::1/128`)
  from Windows TUN routes. These addresses stay local and do not enter Mihomo.
- Keep the route exclusion Windows-only. Linux routing is unchanged.
- Application routes, including `kubectl.exe`, are configured by the user in
  List Management (ADR 0109) instead of being hard-coded to DIRECT.

## Consequences

- Localhost services on any port remain reachable while BiFlow is connected.
- The loopback exception does not change routing for other process traffic.
