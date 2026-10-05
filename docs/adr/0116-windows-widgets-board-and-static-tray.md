# ADR 0116 — Windows 11 Widgets Board provider, and a static tray logo

Date: 2026-10-05
Status: Accepted

## Context

BiFlow shows Iranian / non-Iranian split routing on Windows. Two earlier
attempts carried live bandwidth into the shell:

1. A taskbar overlay **window** (ADR 0114 era) drifted away from the
   system tray, overlapped neighbouring tray icons, and jumped when the
   display DPI changed.
2. A dynamically generated 32×32 **tray icon** that painted `D:` / `U:`
   letters, bars, and a numeric rate directly into the BiFlow shell icon
   (`crates/iran-split-helper-winacl/src/tray_icon.rs`).

Approach 2 works but is the wrong surface. A tray icon is an
**application identity marker**, not a data channel: it must be the
application logo, it must not change when the network changes, and it
must stay legible at the 16×16 size Windows actually allocates. Painting
rates into it violates all three, and it also means the icon is a moving
target for the eye — the operator looks for the logo, not for a number
that changes ten times a minute.

Windows 11 has a first-class surface for exactly this: the **Widgets
Board**, reachable from the left edge of the taskbar, where a third party
can publish a card that the shell renders, refreshes, and lays out.

## Decision

**1. The tray icon is the application's own bundled logo again.**

`setup_tray` uses `app.default_window_icon()` and a plain `"BiFlow"`
tooltip. The generated-bitmap renderer, the `RateSampler`, the
`taskbar_traffic` module, the three `BehaviorConfig` fields, and the
Settings UI for them are all removed rather than left dormant — a
disabled-by-default path to a painted shell icon is a regression waiting
to be re-enabled. `scripts/tauri-contract.test.mjs` now asserts the
absence of `initial_tray_icon`, `render_rate_icon`, and `set_icon(`, so
the decision cannot silently invert.

**2. Live status moves to a real Windows 11 widget, built as a separate
optional MSIX in `widgets/WindowsWidgetProvider`.**

`IWidgetProvider` is a WinRT/COM interface shipped by the Windows App SDK
(2.3.1 is the earliest release with a stable `Microsoft.Windows.Widgets.Providers`
surface). It cannot be implemented by the Tauri desktop binary: a Rust
process cannot register the `windows.widgets` app-extension category, and
the interface is not pluggable into an existing webview. The official
route for any host that is not a C#/C++ WinUI app is a small
out-of-process provider returning **Adaptive Card** JSON. That is the
companion, and it is deliberately minimal: one COM server, no UI, no
window, no message pump beyond serving the host's calls.

The package declares two manifest extensions and nothing else:

- `uap3:AppExtension Category="windows.widgets"` — puts BiFlow in the
  board's **+** menu.
- `com:Extension Category="windows.comServer"` — registers the CLSID the
  widget host activates.

**3. The provider reads the Mihomo controller directly, so the desktop
changes nothing.**

`BiFlowDataSource` reads `%APPDATA%\biflow\config.toml` for the
controller endpoint and client roster, then issues
`GET http://127.0.0.1:19090/connections` with the shared secret as a
Bearer token — the same request, on the same loopback endpoint, that the
desktop's own 2 s traffic loop already issues.

This is the load-bearing choice. The obvious alternative — have the
desktop export a state file or expose a new Tauri command for the widget
to read — would couple two artifacts that should stay independent, and
would have to solve file-locking, staleness, and a second IPC channel.
The controller is already authoritative, already authenticated, and only
answers while Mihomo runs, so **a successful response is itself the
"connected" signal** and no stack-phase handshake is needed.

It also keeps ADR 0111 intact: session traffic totals remain an in-memory
delta accumulator inside the desktop process on purpose, and nothing
reintroduces a `traffic-totals.json` round-trip.

## Consequences

- The desktop is a plain NSIS installer again with a static tray logo. No
  runtime CPU cost, no icon churn, no 250 MB log parse.
- The widget is a **separate artifact**. Installing, updating, or removing
  it never touches the desktop installer, and the desktop never probes for
  it. A user who never builds the MSIX sees no difference.
- On **Windows 10** the package installs (MinVersion 1809) but is inert:
  there is no Widgets Board, so the shell never activates the COM class,
  the provider process never starts, and no runtime feature detection is
  required in Rust precisely because the two artifacts share nothing.
- The C# provider is not compiled by the Rust or Node gates. It needs the
  .NET SDK plus the Windows SDK and is built by `widgets/WindowsWidgetProvider/build.ps1`.
- The card reports **Disconnected** with a reason (timeout, HTTP status,
  invalid JSON) rather than a bare `0`, so a silent zero is never
  ambiguous — the failure mode of the discarded tray badge.
- Byte formatting uses binary units (`KiB`, `MiB`, `GiB`) to match the
  desktop status bar exactly, so the same number never appears with two
  different suffixes in two places.
