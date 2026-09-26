# 0103: Ubuntu 22.04 is built beside Ubuntu 24.04

## Status

Accepted

## Context

Clippy on the Linux runner rejected a Windows-only route helper as dead code,
because `cargo clippy --workspace` compiles only the host `cfg`. The same
release also shipped a single Linux package built on Ubuntu 24.04. That
binary needs a newer glibc than Ubuntu 22.04 has, so it does not run there.
A package built on 22.04 installs there, and it is not a substitute for the
24.04 updater package: Ubuntu 24.04 renamed libraries such as GTK to the t64
packages.

## Decision

- Compile the Windows route-acceptance helper only for Windows and for tests
  (`cfg(any(windows, test))`), so the Linux library build does not see it as
  unused and the matcher still runs in tests.
- Run the CI Rust job on `ubuntu-22.04`, `ubuntu-24.04`, and `windows-2025`.
- On a tag, build `.deb` and AppImage on both Ubuntu runners. Install
  `libfuse2` on 22.04 and `libfuse2t64` on 24.04. Rename the 22.04 files to
  `*_ubuntu2204_*` before signing the Debian package, and upload them as
  extra release assets. `latest.json` keeps the Ubuntu 24.04 deb and AppImage.

## Consequences

Ubuntu 22.04 users install `BiFlow_<version>_ubuntu2204_amd64.deb` or the
matching AppImage from the GitHub release. The in-app updater still follows
the Ubuntu 24.04 packages. A Linux Clippy failure and an Ubuntu 22.04
packaging failure are both visible; neither cancels the Windows job.
