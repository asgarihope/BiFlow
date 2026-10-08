# ADR 0116: Native Windows 11 taskbar speeds

Status: Accepted

## Decision

Persist `behavior.show_taskbar_speed`, default false, with backward-compatible
serde defaults. Show the toggle only on Windows in Settings → Behavior, using
English/Persian localization. Save applies it immediately and does not request
a Mihomo reload. Other platforms preserve the preference without starting any
native worker.

The statistics authority remains `traffic::SessionAccumulator`, which folds
Mihomo `/connections` lifetime counters into process-session totals and subtracts
proxy-client process-bypass deltas (ADR 0111). The status bar shows these totals;
the taskbar derives bytes/second from successive corrected totals and elapsed
monotonic time. This includes DIRECT and client-routed traffic handled by Mihomo,
not all device adapters or only encrypted tunnel traffic. Binary B/KiB/MiB/GiB
units match `formatTrafficBytes`. No adapter monitor or extra controller polling
loop is introduced. The existing two-second desktop sampler also runs when the
WebView is hidden. Unavailable/disconnected samples, generation resets, first
samples, and gaps over six seconds invalidate rates (`— B/s`); session totals
retain their existing semantics.

`iran-split-taskbar` is a Windows-only native dependency of the desktop. Like
`helper-winacl`, it contains the unsafe Win32/COM boundary while the desktop
retains `unsafe_code = forbid`. A dedicated STA thread owns the hidden broadcast
host, the visible child HWND, GDI fonts/brushes, event hooks, and message pump.
A separate windowless MTA owns the UI Automation interfaces, following
[Microsoft threading guidance](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-threading).
Only copied geometry crosses the internal discovery channel; the owner never
blocks waiting for accessibility, and rejects results captured before shell
events or preference changes. It revalidates the parent, bounds, and DPI before
applying a result. A bounded latest-value mailbox carries rates and
the preference; it carries no HWND. Clicking posts main-window focus to Tauri's
main thread. Exit joins the owner after timer, hook, window, and GDI cleanup.

The child attaches directly to the primary `Shell_TrayWnd`, requiring the
Windows 11 XAML content bridge, notification area, and Start accessibility
element. Read-only cached UI Automation leaf bounds identify occupied regions.
Reserve the tray and a conservative 220-DIP weather/widgets region, then select
a 156-DIP gap with margins. Never move or resize Explorer controls, reserve
unsupported shell space, inject code, elevate, or fall back to a floating window.
The child uses `WS_EX_LAYERED` and constant alpha 255 so the compositor presents
its GDI surface above Explorer's XAML. The shared desktop/smoke manifest declares
Windows 10/11 compatibility (retaining common-controls v6), which Windows requires
for [layered child windows](https://learn.microsoft.com/en-us/windows/win32/winmsg/using-windows).
The owner timer invalidates and synchronously paints at most twice per second;
accessibility messages cannot cause a repaint loop. The first paint records
address-free clip geometry in `taskbar.paint_surface`.
The two GDI rows have fixed arrow/value areas and a monospace value font. Verify
worker-thread ownership before writing to a stored HWND, because Explorer can
destroy the child and Windows can recycle its handle during a shell restart. Theme
preferences are read without registry writes; missing preferences use system
colors. Geometry and font sizes scale with taskbar DPI.

Explorer create/hide/location WinEvents hide the child before requesting a
debounced layout check. A hidden top-level host receives `TaskbarCreated`,
display, DPI, settings, theme, and power broadcasts. A 500-ms mailbox timer
keeps settings responsive and expires stale values; layout validation falls
back to two seconds while healthy, with three two-second recovery retries before
backing off to 30 seconds. Provider calls
have one-second COM timeouts and the element traversal has a two-second budget.
Transient or unknown layout data fails closed. Lifecycle events record stable
causes without window titles, addresses, traffic targets, or user content.
`environment.snapshot`/`environment.json` include taskbar status and an
enabled-but-unavailable finding. The snapshot also reports `taskbar_renderer`
and flags unlayered/clipped surfaces even when the HWND reports attached.

## Reference and license

Reviewed TrafficMonitor master at
`930f17533d6098989ebad62f210aa97d75ef174b`, especially
[`Win11TaskbarDlg.cpp`](https://github.com/zhongyang219/TrafficMonitor/blob/930f17533d6098989ebad62f210aa97d75ef174b/TrafficMonitor/Win11TaskbarDlg.cpp)
and [`TaskBarDlg.cpp`](https://github.com/zhongyang219/TrafficMonitor/blob/930f17533d6098989ebad62f210aa97d75ef174b/TrafficMonitor/TaskBarDlg.cpp).
Its Windows 11 implementation uses the primary taskbar as the parent and
discovers Start/notification geometry. Its
[`Anti-996 license`](https://github.com/zhongyang219/TrafficMonitor/blob/930f17533d6098989ebad62f210aa97d75ef174b/LICENSE)
was checked before implementation. No source was copied; BiFlow independently
implements native parenting plus conservative cached accessibility discovery.
TrafficMonitor is never installed or launched.

## Compatibility and verification

This is unofficial embedding, with no supported Windows space-reservation API.
Only the primary horizontal Windows 11 taskbar is supported initially. Crowded
taskbars, inaccessible/changed XAML trees, third-party shell replacements,
cross-process DPI restrictions, and future Explorer versions can make the
feature unavailable. The preference remains saved; BiFlow and its tray continue
to work. Consult `taskbar.unavailable` causes in Diagnostics → debug.log.

Automated coverage includes gap selection/overlap rejection, binary formatting,
statistics reset/stale baselines, config persistence/legacy defaults, environment
findings, settings behavior, and a Windows-mock e2e save/toggle flow. Refresh the
README screenshots when changing Settings. Native smoke verification is opt-in:

```powershell
cargo test -p iran-split-taskbar --test windows_smoke -- --ignored --nocapture
```

The native smoke test initially proved only HWND attachment. A real 6.2.62 report
showed that the child was visible by Win32 flags and painted into its DC but
absent from the composed desktop. Version 6.2.63 adds layered composition and
Windows compatibility manifests; the regression samples only the test-owned
child rectangle with one `CAPTUREBLT` snapshot and checks visible glyph contrast
across periodic layout/rate updates, allowing bounded startup layout recovery.
It can run beside an installed BiFlow without touching that process's child.
The updated smoke test exercised actual composed glyphs at 96 DPI, rate updates,
disable/re-enable, periodic layout validation, and shutdown. It does not prove
visual correctness or full interactive Tauri routing. Before release, complete
these manual Windows 11 checks:

1. Enable and save, hide BiFlow, transfer routed traffic, and compare rates with
   status-bar counter deltas. Click the child to focus BiFlow. Pause/disconnect
   and unavailable-controller samples should show dashes without losing totals.
2. Toggle off/on without restarting. Relaunch and confirm the preference is
   retained. Quit and confirm no native child remains.
3. Restart Explorer and confirm recovery without duplicates. Switch centered
   and left alignment, add/remove taskbar apps, enable widgets, and crowd the
   taskbar: the display must reposition or disappear, never cover controls.
4. Check 100%, 150%, and 200% DPI, primary-display changes, light/dark themes,
   and auto-hide: readable rows, stable value areas, and normal shell hiding.
5. Sleep/resume, then verify stale values clear and fresh sampling recovers.
   Review taskbar events and the exported environment status after failures.
