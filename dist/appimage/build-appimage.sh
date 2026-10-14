#!/usr/bin/env bash
# Build a LanLink AppImage for Linux via the Tauri desktop bundler.
#
# Usage: dist/appimage/build-appimage.sh [output-dir]
#
# Produces: dist/appimage/out/lanlink_<version>_amd64.AppImage
#
# The AppImage is the easiest way for a desktop user to try LanLink:
#   chmod +x lanlink_*.AppImage
#   ./lanlink_*.AppImage
#
# Environment:
#   LANLINK_VERSION  package version (default: workspace version)
#   TARGET           Rust target triple (default: x86_64-unknown-linux-gnu)
#   SKIP_BUILD=1     Reuse already-built binaries.
#   TAURI_CLI        Path to tauri-cli binary (default: cargo tauri after
#                    installing via cargo install tauri-cli)
#
# Requires: cargo, tauri-cli, Linux build deps (see Tauri docs:
# https://tauri.app/start/prerequisites/)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_DIR}"

VERSION="${LANLINK_VERSION:-$(awk -F'"' '/^version =/{print $2; exit}' Cargo.toml)}"
VERSION="${VERSION:-0.1.0}"
OUT="${1:-${SCRIPT_DIR}/out}"
TARGET="${TARGET:-x86_64-unknown-linux-gnu}"
TAURI_CLI="${TAURI_CLI:-}"

log() { printf '[appimage] %s\n' "$*"; }
die() { printf '[appimage] ERROR: %s\n' "$*" >&2; exit 1; }

mkdir -p "${OUT}"

# Ensure the target is installed.
if ! rustup target list --installed 2>/dev/null | grep -qx "${TARGET}"; then
  log "installing Rust target ${TARGET}"
  rustup target add "${TARGET}" || die "failed to add target ${TARGET}"
fi

# Ensure tauri-cli is available.
if [ -n "${TAURI_CLI}" ]; then
  [ -x "${TAURI_CLI}" ] || die "TAURI_CLI=${TAURI_CLI} is not executable"
  TauriBuild() { "${TAURI_CLI}" build "$@"; }
else
  if ! command -v cargo >/dev/null 2>&1; then
    die "cargo not found"
  fi
  if ! cargo tauri --version >/dev/null 2>&1; then
    log "installing tauri-cli"
    cargo install tauri-cli --locked || die "failed to install tauri-cli"
  fi
  TauriBuild() { cargo tauri build "$@"; }
fi

if [ "${SKIP_BUILD:-0}" != "1" ]; then
  log "building LanLink desktop (AppImage target)"
  (
    cd crates/lanlink-desktop
    RUSTFLAGS="${RUSTFLAGS:-}" TauriBuild --target "${TARGET}" --bundles appimage
  )
fi

BUILT_APPIMAGE="target/${TARGET}/release/bundle/appimage/lanlink_${VERSION}_amd64.AppImage"
if [ ! -f "${BUILT_APPIMAGE}" ]; then
  BUILT_APPIMAGE="target/${TARGET}/release/bundle/appimage/LanLink_${VERSION}_amd64.AppImage"
fi
[ -f "${BUILT_APPIMAGE}" ] || die "AppImage not found at ${BUILT_APPIMAGE}"

cp "${BUILT_APPIMAGE}" "${OUT}/lanlink-${VERSION}-linux-x86_64.AppImage"
chmod +x "${OUT}/lanlink-${VERSION}-linux-x86_64.AppImage"
log "wrote ${OUT}/lanlink-${VERSION}-linux-x86_64.AppImage"
