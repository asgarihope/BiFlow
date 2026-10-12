# ADR 0118: Physical adapter speeds in the Ubuntu panel

Status: Accepted (6.2.65)

Ubuntu's AppIndicator host already supports text beside BiFlow's tray icon.
Tauri's `TrayIcon::set_title` uses that label on Linux. Reuse the existing tray,
menu, theme and lifecycle rather than installing a GNOME extension or drawing
a separate window. Display `↑ upload  ↓ download` in decimal bits per second,
using ADR 0117's formatter. The label follows the saved `show_taskbar_speed`
preference immediately; Linux exposes it as “Show network speed in panel”.
It is off by default. Disabling clears only the text, retaining the tray menu.

Extend `iran-split-netstats` with a read-only Linux provider. Read the physical
Wi-Fi/Ethernet adapter's complete `tx_bytes`/`rx_bytes` from sysfs; require a
hardware `device`, Ethernet link type and operational-up state. Select one
physical default using IPv4/IPv6 route metrics and interface index as tie breaker;
exclude virtual adapters even when their default route has a lower metric.
A sole eligible interface can be used without a default; ambiguity is unavailable.
No addresses, names or routes leave the provider. No privileges, shell subprocess,
network change or new dependency is needed at runtime.

Share the desktop's one-second sampler, monotonic rate tracker and safe diagnostic
transitions with Windows. Sampling continues while disconnected and while the
WebView is hidden. Missing/reset/switched counters and suspend gaps clear the
baseline. Unavailable rates render dashes. Dashboard totals keep their separate
Mihomo semantics. Initialize after the tray exists and stop sampling on exit.
Environment snapshots include the adapter classification, AppIndicator renderer
and label submission status. `label_set` proves API submission, not shell rendering;
an unsupported panel host may ignore labels. Ubuntu's built-in AppIndicator host
supports them on both Wayland and X11; other hosts depend on their label support.

Coverage includes synthetic sysfs fixtures, default route parsing for IPv4/IPv6,
virtual/down exclusions, malformed counters, host reads without elevation,
shared rates and units, unavailable/disabled label states, Linux settings unit
tests and English/Persian Windows/Linux settings-save e2e flows. Native validation
must also inspect the Ubuntu panel, updates while the main window is hidden,
disable/re-enable and removal on exit.
