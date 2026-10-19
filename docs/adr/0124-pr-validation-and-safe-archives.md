# ADR 0124: Cross-platform PR validation and safe archive dependencies

Status: Accepted (6.2.73)

PR 11 failed Linux and Windows Clippy because macOS route parsers were compiled
outside their native caller. Compile those helpers on macOS and in tests only;
the existing route ownership and deletion tests still run on every host.
Run the Rust CI job on macos-15 as well as Linux and Windows so the native menu
bar boundary is checked before merge, rather than first compiling during release.

RustSec also rejected rustls 0.23.45 and zip 4.6.1. Pin the resolved rustls to
0.23.46 and move the shared ZIP reader to version 9. Keep Rust 1.88 support.

BiFlow uses tauri-plugin-updater to authenticate metadata and download signed
bytes. Its own github_update module stages and installs the exact native package.
Disable the plugin's unused archive installation feature so its transitive ZIP 4
reader is absent, while retaining rustls TLS and signature verification. Native
package installation and the signed target names stay unchanged.

Graceful-stop failures emit structured warnings before the existing bounded
force-stop fallback. Unix ESRCH is an intentional no-op when the child has
already exited between the status read and signal delivery.

Validate ordinary ZIP extraction and parent traversal rejection alongside the
existing signed-update and cross-platform route tests, then run the full local
CI gate and native CI jobs before merging.
