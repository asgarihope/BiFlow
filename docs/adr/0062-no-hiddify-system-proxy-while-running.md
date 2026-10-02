# ADR 0062: The system proxy never points at Hiddify while running

## Status

Accepted

## Context

Hiddify sets the desktop system proxy (GNOME/KDE on Linux, WinINET on
Windows, `networksetup` on macOS) to its own mixed port when it starts or
reconnects. Browsers honor that proxy
and send every request to Hiddify over loopback, which the TUN never sees, so
Mihomo's split routing is bypassed entirely: DIRECT domains exit through the
tunnel's foreign IP and Iranian hosts refuse or time out.

Live evidence on a running stack (preset Mokhaberat, DNS returning real IPs):
`https://iran.ir/?v=1` and `https://console.kavenegar.com/` returned 200
through the TUN and through Mihomo's mixed port, but timed out or failed TLS
through the Hiddify proxy the desktop was configured to use.

The engine only cleared the Hiddify system proxy on Stop and Pause, never on
Start — and Resume actively _restored_ it, re-breaking DIRECT domains after
every Pause → Resume cycle.

## Decision

- `start_steps` clears a Hiddify-pointing system proxy once the stack reaches
  Running (covers Start and Resume). Failure to clear logs a warning and does
  not fail the start.
- Resume no longer restores the snapshot. The snapshot is only ever a Hiddify
  endpoint (`points_at_hiddify` gates the write), so restoring it can only
  reintroduce the bypass.
- `refresh_health` re-clears while the phase is Running or Degraded, because
  Hiddify re-asserts the proxy when it reconnects. A successful clear logs
  `system_proxy.hijack_cleared` at WARN — the evidence trail for "a DIRECT
  domain stopped opening" reports.
- `PlatformBackend::clear_hiddify_system_proxy` now returns whether something
  was cleared so the engine can log with evidence. Non-Hiddify (corporate)
  proxies are untouched, as before.
- On macOS, `apply_disabled` must pass `off` for HTTP, HTTPS, and SOCKS. The
  first port replayed each service's `*_enabled` flag from the snapshot, so
  "clear" left Hiddify's proxy up. Safari/Chrome then sent `localhost` through
  Hiddify and got HTTP 502 even though the TUN loopback path was fine.

## Consequences

- Browsers follow the desktop's live proxy settings, so they recover without a
  restart; terminals that captured `HTTP(S)_PROXY` env vars keep them until
  re-login.
- While Paused, the system proxy stays cleared; operators who want plain-VPN
  browsing during a pause must configure it themselves.
- `system_proxy.hijack_cleared` appearing repeatedly in debug.log means
  Hiddify keeps re-asserting its proxy — expected during reconnect churn.
