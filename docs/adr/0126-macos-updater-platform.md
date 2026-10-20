# ADR 0126: Publish the Apple Silicon disk image as an updater platform

Status: Accepted (6.2.75)

About → Check for updates on Apple Silicon asks the Tauri updater for platform
`macos-aarch64`. Release 6.2.74 uploaded `BiFlow_6.2.74_aarch64.dmg` but
`latest.json` listed only `linux-deb-x86_64`, `linux-x86_64`, and
`windows-x86_64`. The updater then failed with "the platform macos-aarch64 was
not found in the response platforms object" before the disk image could be
installed.

The macOS release job signs `artifacts/macos/BiFlow_<version>_aarch64.dmg`
with the same minisign key used for the Debian package and uploads the `.sig`
beside the disk image. `scripts/generate-latest-json.mjs` publishes that pair
as `platforms["macos-aarch64"]`. The key stays identical to
`signed_target(InstallKind::Dmg)` on `aarch64`. Intel Macs are still not an
updater target; this release builds only the Apple Silicon disk image.

An already published release cannot grow this platform after the fact: the
6.2.74 disk image has no signature. The next version bump publishes a manifest
that installed Apple Silicon builds can install.
