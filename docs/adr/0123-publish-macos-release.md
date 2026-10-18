# ADR 0123: Publish the macOS disk image with the version release

Status: Accepted (6.2.72)

The release workflow built Linux and Windows packages only after a `v*` tag.
Merging to `main` therefore left the macOS disk image with no download, and a
follow-up tag push was easy to forget.

A push to `main` publishes `v<version>` when that GitHub release is missing or
still a draft. An existing finished release is left alone, so a later commit
that does not change `version` does not rebuild. A `v*` tag that matches the
version file still publishes, including a re-run. The macOS job runs
`./build.sh ci-macos` on `macos-15`, which keeps the headless disk-image
fallback, and the publish job uploads `BiFlow_<version>_aarch64.dmg` next to
the signed Linux and Windows packages.

The disk image is a manual download. It is not an updater target: `latest.json`
still lists only the signed Debian package, AppImage, and NSIS installer.

The publish job creates the missing tag itself. A tag pushed with the
workflow token does not start another workflow, so the tag cannot be relied on
to launch this release.
