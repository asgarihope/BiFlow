# BiFlow — Windows 11 Widgets Board provider

A **third-party Windows 11 widget** for BiFlow. Pin it from
_Widgets → Add a widget → BiFlow Traffic_ and it shows live split-tunnel
state plus download/upload totals next to the weather and finance widgets.

## What this is (and is not)

|        |                                                                                                                                                              |
| ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| ✅ Yes | An `IWidgetProvider` COM server registered under the `windows.widgets` app-extension category, rendered by the Windows 11 Widgets Board as an Adaptive Card. |
| ❌ No  | A desktop window, a toast notification, a taskbar overlay, or a replacement for the Windows Weather widget.                                                  |
| ❌ No  | Any change to the BiFlow desktop application. It ships and runs exactly as before, including its **static logo tray icon**.                                  |

The provider is a **separate, optional MSIX package**. The BiFlow desktop is
an unrelated NSIS installer. Uninstalling the widget package never touches
the desktop, and a machine that never installs the widget package behaves
exactly as BiFlow did before.

## Why a companion process is required

`IWidgetProvider` is a **WinRT/COM interface** shipped by the Windows App
SDK. Only a process built against that SDK can implement it — a Rust
binary compiled for the Tauri app cannot, and the interface is not
pluggable into an existing Tauri webview. The official guidance for
non-C#/C++ hosts is a small out-of-process provider that returns
**Adaptive Card** JSON. That is exactly this project, and it is the
smallest component that can satisfy the platform contract.

The provider holds **no state and no privilege**. Every value on the card
comes from sources the desktop already uses:

- `%APPDATA%\biflow\config.toml` — controller endpoint, client roster
- the local Mihomo REST controller (`127.0.0.1:19090`) — live counters

Because the controller is already the authoritative live source, the
desktop needs **no widget-facing state file and no new Tauri command**.
That keeps the change strictly additive (ADR 0111: session totals stay in
the desktop process on purpose).

## Files

```
widgets/WindowsWidgetProvider/
├── BiFlow.WidgetProvider.csproj   Windows App SDK 2.3.1, x64, headless
├── Program.cs                     COM server entry point (CoRegisterClassObject)
├── WidgetProviderFactory.cs       IClassFactory projection
├── WidgetProvider.cs              IWidgetProvider implementation
├── BiFlowDataSource.cs            config.toml + Mihomo controller reader
├── AdaptiveCardBuilder.cs         Adaptive Card JSON (small + medium)
├── WidgetIds.cs                   CLSID / widget-id single source of truth
├── Package.appxmanifest           windows.widgets + windows.comServer
├── app.manifest                   DPI awareness + Common Controls v6
├── Assets/                        StoreLogo / Square44 / Wide310 (from app logo)
└── build.ps1                      compile → stage → MakeAppx → optional install
```

## Build

Prerequisites: **.NET SDK 8.0+** (not just the runtime) and the Windows 10
SDK for `makeappx.exe`.

```powershell
winget install Microsoft.DotNet.SDK.8

# Unsigned development package, installed for the current user:
powershell -ExecutionPolicy Bypass -File .\build.ps1 -Install

# Signed release package:
powershell -ExecutionPolicy Bypass -File .\build.ps1 `
    -CertificateThumbprint ABCDEF0123456789ABCDEF0123456789ABCDEF01
```

Output: `widgets/WindowsWidgetProvider/dist/BiFlow.WidgetProvider.msix`.

## Install and pin the widget

1. Install the desktop app normally (`BiFlow_*_x64-setup.exe`) so
   `%APPDATA%\biflow\config.toml` exists and Mihomo can run.
2. Build and install the widget package (`build.ps1 -Install`).
   An unsigned package needs **Settings → Privacy & security → For
   developers → Developer mode** enabled, or an enterprise sideload policy.
3. Launch **BiFlow** and press **Connect** once so the controller answers.
4. Open the Widgets Board (`Win` + `W`, or the weather widget on the taskbar).
5. Click **+**, choose **BiFlow Traffic**, and drag it to the board.

The card refreshes on the board's own cadence. It reads the controller on
every refresh, so it needs neither the BiFlow window to be open nor a
polling timer inside the desktop.

## What the card shows

- **Split active / Idle / Disconnected** — derived from whether the Mihomo
  controller answers. A successful response _is_ the connected signal, so
  the card needs no separate stack-phase handshake.
- **Download / Upload** — lifetime byte counters from `/connections`,
  formatted with binary units (`KiB`, `MiB`, `GiB`) to match the desktop
  status bar exactly.
- **Routing caption** — enabled client count and the default preset, parsed
  from `config.toml`.
- On failure the card degrades to **Disconnected** with the reason
  (timeout, HTTP status, invalid JSON) instead of blanking out, so a
  silent zero is never ambiguous.

## Windows 10 behaviour

The package targets `MinVersion="10.0.17763.0"` (Windows 10 1809) and
declares only `runFullTrust`.

- On **Windows 10** the package installs normally, but Windows 10 has no
  Widgets Board. The shell never activates the COM class, the provider
  process never starts, and the package sits inert in
  _Settings → Apps_. It consumes no resources and does not appear in any
  notification area.
- The **BiFlow desktop application is unaffected** either way. It is
  installed by a completely separate NSIS installer, has no manifest
  dependency on this package, and does not probe for the widget. If the
  MSIX is never installed — or is removed — BiFlow behaves identically.
- No feature detection is needed in the Rust code precisely _because_ the
  two artifacts share nothing.

## Troubleshooting

| Symptom                             | Cause                                                                                                                                                                         |
| ----------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| BiFlow missing from the **+** menu  | `Package.appxmanifest` CLSID does not match `WidgetIds.ProviderClsid`, or the package failed to install. Check PowerShell: `Get-AppxPackage IranSplit.BiFlow.WidgetProvider`. |
| Card stuck on **Disconnected**      | BiFlow is not connected, or `controller_port` / `controller_secret` in `%APPDATA%\biflow\config.toml` do not match a running Mihomo.                                          |
| Provider process never starts       | Windows 10, or the package is not installed. On Windows 11 check that the Windows App SDK runtime is present.                                                                 |
| A console window flashes on refresh | The binary was built as a console subsystem app. It must be `WinExe` (set in the `.csproj`) or `<SubSystem>Windows</SubSystem>`.                                              |
