# 0120: Release the network when BiFlow is not in use

## Status

Accepted

## Context

Quit left Mihomo running. The privileged helper owns that process with
`kill_on_drop(false)`, and the desktop owns the local proxy client. Closing
the app therefore dropped the proxy while the TUN kept capturing every
packet. macOS system DNS stayed on `127.0.0.1`, and a system proxy left on
`localhost` or another client port kept sending browsers at a closed socket.
The same machine worked again only after BiFlow was opened and connected.

Disconnect could also skip TUN cleanup when clearing the system proxy
failed, and `SIGKILL` did not let Mihomo remove its own routes. Fake-ip
answers in `198.18.0.0/16` stayed cached after the tunnel was gone.

## Decision

- Tray Quit and every other non-update exit run Disconnect to completion
  before the process leaves: stop Mihomo, stop an owned local proxy, clear
  every managed loopback system proxy, restore DNS, and delete the TUN.
- An in-app update still only pauses, so Hiddify can keep running while the
  package is replaced.
- `localhost`, `127.0.0.1`, and `::1` are one loopback host. Clear a system
  proxy when it points at any configured local-proxy client, including a
  disabled one. Leave a non-loopback corporate proxy alone.
- A system-proxy clear failure is logged and does not skip TUN cleanup.
- Stop sends `SIGTERM` (and `taskkill` without `/F` on Windows), waits two
  seconds, then force-kills. Cleanup then flushes the OS DNS cache.
- If the desktop has not contacted the helper for 90 seconds and a capture
  is still up, the helper releases the network itself. Health refresh is
  every 10 seconds, so a running app is not treated as gone. A command in
  flight is never interrupted.

## Consequences

- Internet works again after Quit, Disconnect, or a crash, without opening
  BiFlow.
- The absence watch needs the updated helper. Quit already uses the existing
  stop and cleanup commands, so the first exit after an upgrade restores the
  network even before Connect installs that helper.
- Native DNS-flush tools still have to be proven on each OS; a missing tool
  does not fail cleanup.
