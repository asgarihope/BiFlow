# 0107: Give desktop binary and library distinct Cargo artifact names

## Status

Accepted

## Context

The Tauri crate's binary name `iran-split-desktop` and library name
`iran_split_desktop` normalize to the same Windows PDB filename. Cargo emits an
output-collision warning for ordinary release builds and repeats it during the
NSIS build. Repository build gates treat project warnings as failures.

## Decision

- Rename the library target to `iran_split_desktop_lib`.
- Update the binary's two library references to the new crate name.
- Keep the package and executable names unchanged.

## Consequences

Cargo writes distinct PDB files for the binary and library, so PowerShell and
CI builds do not emit the collision warning. The shipped executable and
installer names remain unchanged.
