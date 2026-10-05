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

`IWidgetProvider` is a WinRT/COM interface. It cannot be implemented by the
Tauri desktop binary: a Rust process cannot register the `windows.widgets`
app-extension category, and the interface is not pluggable into an existing
webview. The official route for any host that is not a C#/C++ WinUI app is a
small out-of-process provider returning **Adaptive Card** JSON. That is the
companion, and it is deliberately minimal: one COM server, no UI, no window.

The package declares two manifest extensions and nothing else:

- `com:Extension Category="windows.comServer"` — registers the CLSID the
  widget host activates.
- `uap3:Extension Category="windows.appExtension"` with
  `Name="com.microsoft.windows.widgets"` — puts BiFlow in the board's **+** menu.
  There is **no** `windows.widgets` category in the packaging schema: a
  `uap3:AppExtension` element, or this extension at package level instead of
  under `<Application><Extensions>`, is rejected by `makeappx` with C00CE014.
  Everything under `uap3:Properties` is the widget host's own contract and the
  packaging schema only requires it to be well-formed XML.

**2b. The provider is C++/WinRT, not C#.**

This is forced, not preferred. The provider needs a real `IClassFactory` and a
real `IWidgetProvider` vtable. `ComExport` was removed in .NET Core, and the
Windows App SDK's `CWidgetProvider` helper exists **only in the native
`Microsoft.Windows.Widgets.dll`** — it is absent from the `.winmd` and is never
projected into managed code, in any Windows App SDK release. A C# provider would
mean hand-writing the vtable and marshaling for six methods, which is exactly
the kind of code that compiles and then never activates.

`winrt::implements` generates both, so the provider is C++/WinRT. The projection
headers are generated from the `.winmd` by `cppwinrt.exe` at build time, so there
is no checked-in generated code. Two ordering rules are load-bearing and pinned
by a contract test: `<unknwn.h>` must precede every C++/WinRT header, and the
generated `winrt\base.h` must be kept out of the include path because it is an
older snapshot that silently removes `winrt::make` and several stream APIs.

**3. The provider reads the Mihomo controller directly, so the desktop
changes nothing.**

`BiFlowDataSource` reads `%APPDATA%\biflow\config.toml` for the
controller endpoint and client roster, then issues
`GET http://127.0.0.1:19090/connections` with the shared secret as a
Bearer token — the same request, on the same loopback endpoint, that the
desktop's own 2 s traffic loop already issues.

Reading that config is a parser contract, not a formality. The desktop writes
`key = value` with spaces, so every key goes through one reader that tolerates
whitespace around `=`. An exact `key=value` match reads nothing and falls back
to defaults — and because the default host and port happen to be the correct
ones, only the empty secret shows up, as an HTTP 401 and a `0 B` card. A
config reader that can fall back must be able to say so.

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

**4. The MSIX is bundled as an installer resource and registered by the NSIS
hook, on a best-effort basis.**

`bundle.resources` maps `../packaging/staged/windows/widget/` to `widget/`, so
the package lands at `$INSTDIR\widget\BiFlow.WidgetProvider.msix`. The
`NSIS_HOOK_POSTINSTALL` macro runs `Add-AppxPackage` only when that file
exists, and `NSIS_HOOK_PREUNINSTALL` removes the package by identity
(`IranSplit.BiFlow.WidgetProvider`) because `Remove-AppxPackage` matches the
package, not the file, and the file is already gone by then.

Tauri v2's `NsisConfig` has **no** `installerFiles` key, so the extra payload
has to travel through `bundle.resources`; the contract test asserts the key is
absent so nobody reaches for it again.

The failure policy is deliberately asymmetric. The privileged Helper must
succeed or the install `Abort`s — a machine without it cannot connect. The
widget step must **never** abort: Windows 10, developer mode being off, and an
unsigned package the user has not trusted are all ordinary conditions on a
perfectly good BiFlow installation. The hook prints the real reason and points
at `build.ps1 -Install` instead of failing.

A build without the .NET SDK stages no MSIX, so `IfFileExists` skips the block
and the installer ships a working desktop with no widget. That is the common
case for a source build, and it is why staging is a build-time step rather
than a checked-in artifact.

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
- The C++/WinRT provider is not compiled by the Rust or Node gates. It needs the
  Windows SDK plus the Visual Studio C++ toolset and is built by
  `widgets/WindowsWidgetProvider/build.ps1`.
- The card is an **Adaptive Cards 1.0** document assembled by string
  concatenation, and the host treats it as a strict contract: a property
  appended after the object closes, an enum value that is not camelCase, or a
  declared version above 1.5 each produce a widget that renders as **nothing**,
  with no error anywhere. The contract test pins the invariants, and when the
  provider has been built it also runs `--dump-card` and `JSON.parse`s the real
  payload.
- The provider therefore ships `--dump-card`. The shell is the only thing that
  starts it, and the shell starts it only once a widget is pinned, so without
  that mode a card which renders as nothing cannot be inspected at all.
- The card reports **Disconnected** with a reason (timeout, HTTP status,
  invalid JSON) rather than a bare `0`, so a silent zero is never
  ambiguous — the failure mode of the discarded tray badge.
- Byte formatting uses binary units (`KiB`, `MiB`, `GiB`) to match the
  desktop status bar exactly, so the same number never appears with two
  different suffixes in two places.
- Automatic registration needs a **signed** package, and an app package is
  refused unless its signature chains to a root the machine trusts
  (`0x800B0109`); an unsigned one is refused outright (`0x80073D2C`). The
  installer therefore ships the trust anchor beside the MSIX and adds it with
  `certutil -addstore -f` before `Add-AppxPackage`. The NSIS installer is
  perMachine, so it is already elevated — this costs the user no second UAC
  prompt and no certificate-warning dialog, which is what makes "install the
  app, get the widget" true for an ordinary user rather than only for a
  developer machine. The uninstall hook removes the root again, because a
  certificate left behind keeps trusting everything it ever signed.

  The whole trust block sits behind `IfFileExists`, so a build signed by a real
  CA certificate — or published through the Store — needs none of it. Bundling
  a self-signed root is the right answer for internal and beta distribution and
  is _not_ what a public release should do; that build should pass
  `-CertificateThumbprint` and carry a CA-issued chain instead.

  `Identity/@Publisher` must equal the certificate subject DN or the MSIX
  subject interface fails with a bare `SignerSign() failed` (0x8007000B), and
  the manifest's Publisher is therefore generated with the certificate rather
  than hand-written.

- A widget provider cannot be deployed with `Add-AppxPackage -Register`. A
  loose registration reports `Status: Ok` and appears in `Get-AppxPackage`, so
  every superficial check passes, but it leaves an AppModel repository entry
  with **no subkeys** while healthy packages carry `App`/`Extensions` ones. The
  app extension is never indexed, the Widgets Board never lists the provider,
  and the package is completely inert. Install the real MSIX.
- `Screenshots` inside `ThemeResources` is **required**, and it is the element
  the widget picker actually renders. Without it the package installs, the COM
  class registers, the provider runs — and the host has nothing to draw in the
  Add Widgets dialog, so it drops the provider silently. It must be 300×304
  with transparent rounded corners. The picker requirement lives outside the
  card layout, which is exactly why it is easy to miss on an otherwise
  functional widget.
- Fixing the hook exposed a latent install bug: the Helper line used
  `"$PROGRAMDATA\iran-split\staging"`, and NSIS has no `$PROGRAMDATA` — the
  token expanded to empty, leaving `--staging-dir` valueless, which
  `install_from_args` rejects, so the Windows install aborted after copying
  every file. `makensis` reported it only as warning 6000 and a contract test
  asserted the typo itself, so both had to be corrected. The hook is now
  compiled directly with `makensis` to surface every warning.
