# ADR 0117: Restore networking on exit and resume; complete macOS updates

Status: Accepted

## Context

Quit previously ended the desktop without stopping the privileged Mihomo process,
side tunnels, or restoring DNS. A proxy stop error could also skip owned network
cleanup. The health loop refreshed component labels but never rebuilt a stale TUN
after Windows lock/unlock or system sleep. macOS updates left the old process
running, always wrote to `/Applications`, and the release pipeline omitted Mac
packages and their signed manifest entries entirely.

## Decision

- Intercept every Tauri exit request, including tray Quit and updater exit. Cancel
  pending engine operations, reject new operations, drain the worker, and detach
  owned network state before exiting. Keep the app available when cleanup fails.
  Closing the window still hides it to the tray; that intentionally keeps routing
  active. Force termination and power loss cannot run desktop exit handlers.
- Stop and cleanup run even from an Error phase. Optional proxy failures are
  retained and reported after privileged cleanup, rather than skipping it.
- A native health timer detects sleep gaps over 30 seconds, two consecutive
  unhealthy Mihomo/TUN/DNS samples, and Windows session unlock. The Windows query
  checks only LogonUI in the desktop process's session, with a three-second bound,
  hidden console, and no process/user content in logs. A recovery has a 60-second
  cooldown and runs under the existing lifecycle lock. Paused/stopped intent is
  preserved. Recovery removes owned routing, invalidates egress caches, then
  rebuilds and checks readiness. Failure detaches networking and leaves Pause
  available for an explicit retry. Timer and component checks also run on Unix.
- Environment findings identify an active stack with missing runtime components.
  Structured events identify recovery, cleanup, session-query and exit failures.
- Install the verified Mac bundle at the location of the running BiFlow.app;
  a copy launched from a mounted image or App Translocation installs to `/Applications`. Use a private
  mount directory, validate the staged bundle version and executable, and retain
  the old bundle until the replacement rename succeeds. Never recursively remove
  a mount after a failed detach. A user-session launcher waits for the old PID to
  exit, then opens the updated bundle with `open -n`; it does not launch as root.
- Publish the existing supported Apple Silicon DMG and its updater signature in
  the same release as Linux and Windows, and require its `macos-aarch64` entry in
  latest.json. CI runs native macOS tests and Clippy on `macos-15`, the Apple
  Silicon label documented in the [GitHub runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
  This does not add an Intel Mac release or bypass Apple's signing policy.

## Verification

Engine tests cover cancelling Connect during Quit, cleanup from Error despite a
proxy failure, rebuilding a lost TUN, paused intent, and cleanup after recovery
failure. Desktop tests cover timer gaps, unlock, cooldown, idle intent, session
output parsing, safe bundle replacement, version validation, relaunch ordering,
and environment findings. Manifest tests require the signed Mac package. The
primary connection e2e flow disconnects, verifies internet status, and reconnects.
Native lock/sleep and authenticated macOS installation still need OS smoke tests;
browser mock e2e tests cannot exercise privileged adapters or Launch Services.
