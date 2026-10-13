#!/usr/bin/env bash
# Build a LanLink macOS .pkg installer (root daemon, launchd-managed).
#
# Usage: dist/macos/build-pkg.sh [output-dir]
# Requires: cargo, pkgbuild, productbuild. Run from the repository root
# on macOS. The TAP backend is still WIP on macOS — this packages the
# current build for development/testing.
set -euo pipefail

VERSION="${LANLINK_VERSION:-0.1.0}"
OUT="${1:-dist/macos/out}"
PKGROOT="dist/macos/root"

echo "==> building release binaries (macOS universal recommended: build per-arch and lipo)"
cargo build --release -p lanlinkd -p lanlinkctl -p lanlink-relay -p coordinator

echo "==> assembling package root at ${PKGROOT}"
rm -rf "${PKGROOT}"
install -d -m 0755 "${PKGROOT}/usr/local/bin" \
    "${PKGROOT}/Library/LaunchDaemons" \
    "${PKGROOT}/etc/lanlink" \
    "${PKGROOT}/var/lib/lanlink"

install -m 0755 target/release/lanlinkd "${PKGROOT}/usr/local/bin/"
install -m 0755 target/release/lanlinkctl "${PKGROOT}/usr/local/bin/"
install -m 0755 target/release/lanlink-relayd "${PKGROOT}/usr/local/bin/"
install -m 0755 target/release/lanlink-coordinator "${PKGROOT}/usr/local/bin/"

install -m 0644 dist/macos/com.lanlink.lanlinkd.plist "${PKGROOT}/Library/LaunchDaemons/"
install -m 0644 dist/lanlinkd.example.toml "${PKGROOT}/etc/lanlink/"

echo "==> building .pkg"
mkdir -p "${OUT}"
pkgbuild \
    --root "${PKGROOT}" \
    --identifier com.lanlink.lanlink \
    --version "${VERSION}" \
    --scripts dist/macos/scripts \
    "${OUT}/lanlink-${VERSION}.pkg"
echo "==> wrote ${OUT}/lanlink-${VERSION}.pkg"
echo
echo "Install with: sudo installer -pkg ${OUT}/lanlink-${VERSION}.pkg -target /"
