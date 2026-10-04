# 0114: Windows taskbar download and upload overlay

## Status

Accepted

## Context

Operators want live send/receive on the Windows 10/11 taskbar, in the
system-tray cluster next to the language / input indicator, without opening
the main window. Classic desk bands are gone on Windows 11, and this
workspace forbids `unsafe` outside `iran-split-helper-winacl`, so a COM
toolbar is not an option. Loading the main React app in a second webview
would duplicate the Zustand store and bootstrap.

## Decision

- Add `behavior.taskbar_traffic` (default on),
  `behavior.taskbar_traffic_unit` (`auto`, `b`, `kb`, `mb`, `gb`), and
  `behavior.taskbar_traffic_bits` (bytes by default). Missing fields keep
  those defaults; the config schema stays at 3.
- On Windows only, open a tiny frameless, transparent, skip-taskbar,
  always-on-top window that loads `taskbar.html` — not the main shell.
- Place it immediately left of Explorer's language control
  (`TrayInputIndicatorWClass` / `InputIndicatorButton`), else left of
  `TrayClockWClass`, else just right of a narrow `TrayNotifyWnd` chevron.
  Fall back to a 400 px reserve when the shell window is missing. Hide it
  when the taskbar gap disappears (auto-hide).
- Give that webview its own capability (`taskbar-traffic-window` with
  `core:event:default`). A window that is not listed in any capability
  cannot `listen`, so the overlay would stay at the HTML `D: 0B` / `U: 0B`
  placeholders. Emit rates to that window's label, not only as a global
  app event.
- Show throughput (`D:` download, `U:` upload) from the existing 2 s session
  traffic loop, not lifetime totals. Bytes use `B` / `KB` / `MB` / `GB`;
  bits multiply by eight and use `b` / `Kb` / `Mb` / `Gb`.
- Settings › Behavior toggles the overlay and the unit. Changing those
  fields does not reload Mihomo. Window close and size persistence stay
  gated to the `main` label so the overlay cannot hide or resize the app.
- The 2 s traffic loop may emit new `D:` / `U:` text, but it must not call
  `show()` or move the window unless the work-area rect changed. A repeated
  `ShowWindow` steals focus on Windows 10/11.

## Consequences

- Linux builds keep the settings and the formatter tests; they do not create
  a window.
- Exact language coordinates depend on Explorer exposing those HWNDs. Some
  Windows 11 XAML trays only expose the clock; the widget then sits left of
  the clock in the same cluster.
- Full-screen exclusive apps can still be covered by an always-on-top
  overlay; hide the widget from Settings when that matters.
