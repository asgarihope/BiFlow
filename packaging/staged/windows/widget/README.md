Staged Windows 11 widget package for the NSIS installer.

`build.ps1 -Stage` from widgets/WindowsWidgetProvider writes
`BiFlow.WidgetProvider.msix` here; `tauri.conf.json` bundles this
directory to `$INSTDIR\widget\`, and `packaging/windows/installer-hooks.nsh`
runs `Add-AppxPackage` against it after install.

This file exists only so a fresh clone has the directory — a build
without the .NET SDK simply produces an empty folder, and the install
hook skips the widget silently.
