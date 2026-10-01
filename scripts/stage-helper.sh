#!/usr/bin/env bash
# Build iran-split-helper and copy it to packaging/staged for Tauri bundles.
set -euo pipefail

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
# shellcheck disable=SC1091
if [[ -f "${HOME}/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "${HOME}/.cargo/env"
fi
export PATH="${HOME}/.cargo/bin:${PATH}"

STAGED="${ROOT}/packaging/staged"
TARGET_DIR="${CARGO_TARGET_DIR:-${ROOT}/target}"
mkdir -p -- "${STAGED}"
cd -- "${ROOT}"

host_is_windows() {
  case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*) return 0 ;;
  esac
  return 1
}

# MSVC cross-compiles on Linux/macOS must use cargo-xwin (lld-link). Plain
# `cargo build --target *-windows-msvc` looks for link.exe and fails.
helper_cargo_build() {
  local -a cmd=(cargo)
  local -a args=(build --release -p iran-split-helper)
  if [[ -n "${STAGE_HELPER_TARGET:-}" ]]; then
    args+=(--target "${STAGE_HELPER_TARGET}")
    if [[ "${STAGE_HELPER_TARGET}" == *windows-msvc ]] && ! host_is_windows; then
      if ! command -v cargo-xwin >/dev/null 2>&1; then
        echo "cargo-xwin is required to cross-compile iran-split-helper for Windows MSVC (link.exe is not on this host)" >&2
        exit 1
      fi
      cmd=(cargo xwin)
    fi
  fi
  "${cmd[@]}" "${args[@]}"
}

if [[ "${1:-}" == "windows" ]]; then
  helper_cargo_build
  if [[ -n "${STAGE_HELPER_TARGET:-}" ]]; then
    src="${TARGET_DIR}/${STAGE_HELPER_TARGET}/release/iran-split-helper.exe"
  else
    src="${TARGET_DIR}/release/iran-split-helper.exe"
  fi
  [[ -f "${src}" ]] || { echo "missing Windows helper: ${src}" >&2; exit 1; }
  cp -f "${src}" "${STAGED}/iran-split-helper.exe"
  echo "Staged ${STAGED}/iran-split-helper.exe"
  exit 0
fi

helper_cargo_build
src="${TARGET_DIR}/release/iran-split-helper"
[[ -x "${src}" ]] || { echo "missing Linux helper: ${src}" >&2; exit 1; }
cp -f "${src}" "${STAGED}/iran-split-helper"
chmod 0755 "${STAGED}/iran-split-helper"
echo "Staged ${STAGED}/iran-split-helper"
