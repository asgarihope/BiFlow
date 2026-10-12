# ADR 0119: Dedicated speed display settings and units

Status: Accepted (6.2.66)

Move the existing taskbar/panel toggle from Behavior into Settings → Speed display
on Windows and Linux. This tab contains only the enable toggle and unit selector.
Other platforms preserve both preferences without exposing an unsupported tab.
Reuse the shared tab/form/save flow and English/Persian localization.

Persist `behavior.taskbar_speed_unit` as a typed `SpeedUnit` with serde defaults.
Existing configuration documents retain automatic decimal bits per second;
no schema migration is needed. The selector offers automatic bits or bytes and
fixed bits/bytes, kilo, mega and giga units. `MB/s` means 1,000,000 bytes per second,
while `Mbps` means 1,000,000 bits per second. Unknown saved units are rejected.

Both native renderers consume the same formatter and config enum. The physical
sampler continues to supply bytes per second; conversion happens only at display.
Fixed units remain fixed, including for unavailable samples, which use a dash
with the selected suffix. Automatic units retain the existing magnitude-based
scaling. Save applies preferences without restarting the stack or requesting a
Mihomo reload. Windows updates its native mailbox; Ubuntu updates the tray label.
Copy preferences before submitting to the main thread so no settings mutex is
held across a synchronous tray operation. Diagnostic preference events include
only the safe unit enum and enabled flag.

Regression coverage includes legacy defaults, persistence and invalid units,
bit/byte conversions, fixed/automatic scales and unavailable rates, Linux label
units, settings separation, unsupported-platform preservation, and Windows/Linux
English/Persian e2e save/navigation flows with no network reload banner.
