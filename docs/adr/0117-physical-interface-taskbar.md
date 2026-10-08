# ADR 0117: Physical interface throughput beside the Windows tray

Status: Accepted (6.2.64)

## Decision

The taskbar now measures the active physical network adapter's complete send
and receive traffic, including other applications and traffic while BiFlow is
disconnected. This supersedes ADR 0116's taskbar statistics and layout choices.
Dashboard/session totals retain their corrected Mihomo routing semantics.

The shared `iran-split-netstats` boundary reads Windows IP Helper `GetIfTable2`
`OutOctets` and `InOctets`. Only operational hardware Wi-Fi/Ethernet interfaces
are eligible. Among physical default-route candidates choose the lowest route
plus interface metric, with interface LUID as a deterministic tie breaker. A
sole active physical interface is usable without a default route; multiple
ambiguous candidates without a default produce an unavailable sample. Never
sum physical and virtual/TUN interfaces. No network configuration is modified.
The adapter identity is used privately for baseline changes and is never logged.

A desktop-owned one-second sampler runs independently of the routing engine and
WebView. OS calls run on the blocking pool, not the native message pump. The
shared tracker derives byte rates from monotonic elapsed time, then the renderer
converts once to decimal bps/Kbps/Mbps/Gbps. Missing samples, counter decreases,
adapter changes, first samples, zero elapsed time and gaps over three seconds
invalidate the baseline instead of producing artificial spikes. Read failures
and source transitions emit structured safe classifications; environment
snapshots expose adapter class/failure and an enabled-but-unavailable finding.

These are the full OS adapter counters underlying interface throughput, rather
than application/proxy counters. Task Manager and BiFlow have independent
sampling intervals and rounding, so their instantaneous numbers can differ
briefly even when they measure the same adapter. Task Manager's selected adapter
must be the active physical one for that comparison.

The native widget places upload above download in a 108-DIP box with a four-DIP
margin. Gap selection chooses the rightmost verified free space directly before
the notification area, failing closed when no safe space exists. A layered child
uses a dedicated color key for its background: Explorer's actual taskbar surface
shows through, including wallpaper tint and system transparency. Theme settings
control glyph contrast; no opaque black rectangle is painted. Keep ADR 0116's
Windows compatibility manifest, separate UIA thread, ownership checks, bounded
repaints and lifecycle recovery.

## Validation

Unit tests cover physical default selection despite a lower-metric VPN,
disconnected/non-hardware exclusions, ambiguous fallback, complete counter
deltas, adapter switches, resets, loss and suspend gaps. Formatting tests cover
Task Manager-style bits; a row-order test uses distinct send/receive values.
The Windows host test reads real IP Helper counters without elevation. The
interactive native test checks 108-DIP bounds, proximity to the tray, color-key
state, composed background continuity and visible glyphs through repeated
Explorer layout checks, then disable/re-enable/shutdown cleanup. Existing
English/Persian settings-save e2e tests cover preference application.

Manual comparison: select the physical Wi-Fi/Ethernet adapter in Task Manager,
set its update speed to Normal, generate traffic outside BiFlow and compare over
several one-second updates. Repeat with BiFlow disconnected, switching adapters,
and after sleep. Both tools should follow the same complete adapter throughput;
they are not synchronized to the same sampling instant.
