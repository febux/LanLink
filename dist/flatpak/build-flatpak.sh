#!/usr/bin/env bash
# Build a LanLink Flatpak for Linux.
#
# Usage: dist/flatpak/build-flatpak.sh [output-dir]
#
# Produces: dist/flatpak/out/lanlink-<version>-<arch>.flatpak
#
# Requires: flatpak-builder, cargo, Rust target, org.gnome.Platform runtime.
# See dist/flatpak/com.lanlink.LanLink.yml for the manifest.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_DIR}"

VERSION="${LANLINK_VERSION:-$(awk -F'"' '/^version =/{print $2; exit}' Cargo.toml)}"
VERSION="${VERSION:-0.1.0}"
OUT="${1:-${SCRIPT_DIR}/out}"

log() { printf '[flatpak] %s\n' "$*"; }
die() { printf '[flatpak] ERROR: %s\n' "$*" >&2; exit 1; }

command -v flatpak-builder >/dev/null 2>&1 || die "flatpak-builder not found"

mkdir -p "${OUT}"
REPO="${SCRIPT_DIR}/repo"
rm -rf "${REPO}"

flatpak-builder --force-clean --repo="${REPO}" \
  --disable-rofiles-fuse \
  "${SCRIPT_DIR}/build" \
  "${SCRIPT_DIR}/com.lanlink.LanLink.yml" \
  || die "flatpak-builder failed"

flatpak build-bundle "${REPO}" "${OUT}/lanlink-${VERSION}-linux-x86_64.flatpak" com.lanlink.LanLink || die "build-bundle failed"
log "wrote ${OUT}/lanlink-${VERSION}-linux-x86_64.flatpak"
