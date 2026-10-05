# BiFlow Windows 11 Widgets Board provider

A **third-party widget** for the Windows 11 Widgets Board (the panel that opens
from the left edge of the taskbar, or with `Win+W`).

This directory is the entire widget. The BiFlow desktop app is a Tauri v2
binary and cannot implement `IWidgetProvider` — that is a WinRT/COM contract
that only an out-of-process COM server can satisfy — so this is a small,
separate, **optional** companion. It shows live split-tunnel status and
traffic totals; the desktop app itself knows nothing about it and does not
depend on it.

## What the user sees

One card, **BiFlow Traffic**, offered by the board's `+` ("Add a widget")
menu, in `small` and `medium` sizes. It shows the routing state, the total
downloaded and uploaded since Mihomo started, and a caption naming the active
client roster. Bytes use binary units (`KiB`, `MiB`, `GiB`) so the same number
never appears with two different suffixes in the widget and in the app.

When the controller is unreachable the card says **Disconnected** and names the
reason, rather than showing a silent `0`.

## How the data is read

`BiFlowData.cpp` reads exactly two things:

- `%APPDATA%\biflow\config.toml` — controller endpoint, shared secret, and the
  `[[clients]]` roster
- the local Mihomo REST controller, `GET http://127.0.0.1:<port>/connections`
  with the shared secret as a `Bearer` token

The controller is already authoritative, already authenticated, and only
answers while Mihomo runs, so **a successful response is itself the "connected"
signal** — there is no stack-phase handshake to read.

The HTTP request is issued over a raw `StreamSocket` rather than
`Windows.Web.Http`, because `HttpClient` honours the system proxy and BiFlow
itself installs the tunnel. A raw loopback socket cannot loop back into our own
TUN.

The desktop therefore needs **zero** widget-facing changes, and session traffic
totals stay an in-memory accumulator inside the desktop process (ADR 0111).

## Why C++/WinRT and not C#

The provider must register a real `IClassFactory` and expose a real
`IWidgetProvider` vtable.

- `ComExport` was removed in .NET Core, so the classic managed COM route is gone.
- The Windows App SDK ships a `CWidgetProvider` helper, but **only in the native
  `Microsoft.Windows.Widgets.dll`** — it is not in the `.winmd` and is never
  projected into the C# projection, in any Windows App SDK version.
- Doing it in C#/WinRT means hand-writing the vtable and the marshaling for six
  methods.

In C++/WinRT, `winrt::implements` generates both. That is the whole reason for
the language choice; the rest of the provider is ordinary C++.

## Building

```powershell
.\widgets\WindowsWidgetProvider\build.ps1            # build + pack only
.\widgets\WindowsWidgetProvider\build.ps1 -Install   # build + sign + install
.\widgets\WindowsWidgetProvider\build.ps1 -Stage     # stage for the NSIS installer
```

Requires:

- Visual Studio Build Tools with the **C++ x64 toolset** (`cl.exe`, `link.exe`)
- Windows SDK: `cppwinrt.exe`, `makeappx.exe`, `signtool.exe`, `UnionMetadata`
- The `Microsoft.WindowsAppSDK` package present in the NuGet cache (only its
  `Microsoft.Windows.Widgets` component is used)

The script generates the C++/WinRT projection from the `.winmd` at build time,
so there is no checked-in generated code to drift.

### Signing and trust

The package is signed with a self-signed development certificate
(`CN=BiFlow Widget Provider`, created on first run and reused afterwards).
**The manifest's `Identity/@Publisher` must equal the certificate subject** or
the MSIX subject interface fails with a bare `SignerSign() failed`.

An _unsigned_ package does not install on current Windows: `Add-AppxPackage
-AllowUnsigned` rejects it with `0x80073D2C`. For a signed release, pass
`-CertificateThumbprint`.

The self-signed certificate must also be trusted as a **root** before the
package will install, and importing into a root store always shows a
confirmation dialog. Run this once, interactively or elevated:

```powershell
Import-Certificate -FilePath widgets\WindowsWidgetProvider\build\signing.cer `
                   -CertStoreLocation Cert:\LocalMachine\Root
```

## Pinning the widget

Install the app (or run `build.ps1 -Install`), then `Win+W` → `+` → **BiFlow** →
**BiFlow Traffic**. Restart the Widgets Board if a freshly installed package
does not appear immediately.

## Bundling it with the BiFlow installer

`build.ps1 -Stage` copies the MSIX to `packaging\staged\windows\widget\`.
`tauri.conf.json` maps that directory to `$INSTDIR\widget`, and the NSIS hook
runs `Add-AppxPackage` after install and `Remove-AppxPackage` before uninstall.

The widget step is **best-effort and never aborts the install** — Windows 10,
an untrusted certificate, and a missing MSIX are all ordinary conditions on a
perfectly good BiFlow installation. Only the privileged Helper is allowed to
abort, because a machine without it cannot connect.

## Windows 10

The package has `MinVersion 10.0.17763.0`, so it installs on Windows 10 1809
and later — and is **inert** there: Windows 10 has no Widgets Board, so the
shell never activates the COM class and the provider never runs. The desktop is
a separate NSIS installer and never probes for the widget, so Windows 10 does
not break. See ADR 0116.
