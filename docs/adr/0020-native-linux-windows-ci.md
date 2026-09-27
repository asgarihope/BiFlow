# ADR 0020: Native Linux and Windows GitHub runners

## Status

Accepted

## Context

All GitHub Actions runs through `v1.1.4` failed. Ubuntu Clippy stopped the
workflow, and the Windows rust job was cancelled because the CI matrix used
the default `fail-fast: true`. Linux-only development never compiled
`#[cfg(windows)]` modules, so Windows Clippy (named pipes, helper service,
NSIS packaging) never ran.

Official sources:

- Tauri v2 GitHub pipeline: native OS matrix, `fail-fast: false`,
  `dtolnay/rust-toolchain`, `swatinem/rust-cache@v2`, WebKitGTK 4.1 on Ubuntu,
  `tauri-apps/tauri-action@v1` on each OS.
  https://v2.tauri.app/distribute/pipelines/github/
- Tauri v2 prerequisites: Linux `libwebkit2gtk-4.1-dev`; Windows MSVC +
  WebView2. https://v2.tauri.app/start/prerequisites/
- GitHub `windows-2025` (now `windows-latest`) does not ship NSIS. Windows
  Server 2022 did. https://github.com/actions/runner-images/issues/12677
- This workspace is a root Cargo workspace. Rust artifacts live in `./target`,
  not `src-tauri/target`. The Tauri template cache path
  `./src-tauri -> target` would miss the cache.

## Decision

- Keep native runners: `ubuntu-22.04` and `ubuntu-24.04` for Linux Clippy,
  tests, and packages, and `windows-2025` for Windows Clippy, tests, and NSIS.
  Do not cross-compile the GitHub Windows installer from Linux.
- Publish the Ubuntu 22.04 `.deb` and AppImage beside the Ubuntu 24.04
  updater assets. A binary built on 24.04 needs a newer glibc and does not
  run on 22.04; the 24.04 package stays the `latest.json` updater target
  because its library dependencies use the t64 names.
- Set `fail-fast: false` on the CI rust matrix so both OS jobs finish.
- Cache with `swatinem/rust-cache@v2` and `workspaces: ". -> target"`.
  Set `prefix-key` to the matrix OS (`ubuntu-22.04` vs `ubuntu-24.04`).
  `runner.os` is only `Linux`, and a shared cache makes Clippy report
  `can't find crate for tauri` on 22.04.
- Set `git config --global core.autocrlf false` **before** `actions/checkout`
  on Windows. `.gitattributes` `-text` still pins bundled rule bytes.
- Install NSIS with Chocolatey on `windows-2025` before `tauri-action`. After
  install, add the NSIS directory to `$GITHUB_PATH` **and** the current
  `$env:Path`; `GITHUB_PATH` only applies to later steps, so
  `Get-Command makensis` in the same step otherwise fails.
- Keep cfg-gated Rust as tail expressions so host Clippy on either OS does not
  hit `clippy::needless_return`.
- Local Linux packaging may still use `cargo-xwin`; GitHub release packaging
  stays on a native Windows runner.

## Consequences

Windows-only code is compiled on every CI push. A Linux Clippy failure no
longer hides a Windows failure. Tag releases can produce NSIS installers on
images that no longer preinstall `makensis`. The NSIS install step must be
able to resolve `makensis` immediately, not only in later jobs.
