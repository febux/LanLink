#!/usr/bin/env bash
# Build a LanLink .deb package from release binaries.
#
# Usage: dist/deb/build-deb.sh [options] [output-dir]
#
# Options:
#   --with-tui      Include the terminal UI (lanlink-tui + lanlink-gui launcher)
#   --with-web      Include the browser WebUI (lanlink-web + lanlink-gui launcher)
#   --with-desktop  Include the native desktop UI (lanlink-desktop + lanlink-gui launcher)
#   --with-gui      Include all GUI frontends and the unified launcher
#
# Environment:
#   LL_GUI          Comma-separated list: tui, web, desktop, all (default: none)
#   LANLINK_VERSION Override the package version
#
# Requires: cargo, dpkg-deb, strip. Run from the repository root.
#
# The package:
#   - installs lanlinkd/lanlinkctl/lanlink-relayd/lanlink-coordinator to /usr/bin
#   - optionally installs GUI binaries (lanlink-gui, lanlink-tui, lanlink-web,
#     lanlink-desktop) when requested
#   - installs systemd units (daemon template + coordinator + relay)
#   - creates the `lanlink` system user via postinst (service runs
#     unprivileged with CAP_NET_ADMIN + CAP_NET_RAW)
#   - ships /etc/lanlink/lanlinkd.example.toml
set -euo pipefail

VERSION="${LANLINK_VERSION:-$(cargo metadata --no-deps --format-version 1 2>/dev/null | sed -n 's/.*"version":"\([^"]*\)".*/\1/p' | head -1)}"
VERSION="${VERSION:-0.1.0}"
OUT="dist/deb/out"
PKGROOT="dist/deb/root"

WITH_TUI=0
WITH_WEB=0
WITH_DESKTOP=0

while [ $# -gt 0 ]; do
  case "$1" in
    --with-tui)
      WITH_TUI=1
      shift
      ;;
    --with-web)
      WITH_WEB=1
      shift
      ;;
    --with-desktop)
      WITH_DESKTOP=1
      shift
      ;;
    --with-gui)
      WITH_TUI=1
      WITH_WEB=1
      WITH_DESKTOP=1
      shift
      ;;
    --help|-h)
      sed -n '2,22p' "$0" | sed -E 's/^# ?//'
      exit 0
      ;;
    -*)
      echo "Unknown option: $1" >&2
      echo "Usage: $0 [--with-tui] [--with-web] [--with-desktop] [--with-gui] [output-dir]" >&2
      exit 1
      ;;
    *)
      OUT="$1"
      shift
      ;;
  esac
done

detect_arch() {
  if command -v dpkg >/dev/null 2>&1; then
    dpkg --print-architecture
    return
  fi
  if command -v dpkg-architecture >/dev/null 2>&1; then
    dpkg-architecture -qDEB_BUILD_ARCH
    return
  fi
  case "$(uname -m)" in
    x86_64) echo amd64 ;;
    aarch64|arm64) echo arm64 ;;
    armv7l) echo armhf ;;
    i*86) echo i386 ;;
    riscv64) echo riscv64 ;;
    ppc64le) echo ppc64el ;;
    s390x) echo s390x ;;
    *) echo unknown ;;
  esac
}

ARCH="$(detect_arch)"
if [ "${ARCH}" = "unknown" ]; then
  echo "ERROR: cannot determine Debian architecture (install dpkg, dpkg-dev, or run on a recognized machine)" >&2
  exit 1
fi

# LL_GUI env variable overrides flags: tui,web,desktop,all
if [ -n "${LL_GUI:-}" ]; then
  case "$LL_GUI" in
    *all*)
      WITH_TUI=1; WITH_WEB=1; WITH_DESKTOP=1 ;;
    *)
      case ",$LL_GUI," in
        *,tui,*) WITH_TUI=1 ;;
      esac
      case ",$LL_GUI," in
        *,web,*) WITH_WEB=1 ;;
      esac
      case ",$LL_GUI," in
        *,desktop,*) WITH_DESKTOP=1 ;;
      esac
      ;;
  esac
fi

GUI_BINS=()
[ "$WITH_TUI" = 1 ] && GUI_BINS+=(lanlink-tui)
[ "$WITH_WEB" = 1 ] && GUI_BINS+=(lanlink-web)
[ "$WITH_DESKTOP" = 1 ] && GUI_BINS+=(lanlink-desktop)
if [ "${#GUI_BINS[@]}" -gt 0 ]; then
  GUI_BINS+=(lanlink-gui)
fi

CARGO_PACKAGES=(-p lanlinkd -p lanlinkctl -p lanlink-relay -p coordinator)
for bin in "${GUI_BINS[@]}"; do
  CARGO_PACKAGES+=(-p "${bin}")
done

if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "ERROR: dpkg-deb is required to build .deb packages" >&2
  exit 1
fi

echo "==> building release binaries"
cargo build --release "${CARGO_PACKAGES[@]}"

echo "==> assembling package tree at ${PKGROOT}"
rm -rf "${PKGROOT}"
install -d -m 0755 "${PKGROOT}/usr/bin" \
    "${PKGROOT}/lib/systemd/system" \
    "${PKGROOT}/etc/lanlink" \
    "${PKGROOT}/usr/share/doc/lanlink" \
    "${PKGROOT}/var/lib/lanlink" \
    "${PKGROOT}/DEBIAN"

for bin in lanlinkd lanlinkctl lanlink-relayd lanlink-coordinator; do
    install -m 0755 "target/release/${bin}" "${PKGROOT}/usr/bin/${bin}"
    strip --strip-unneeded "${PKGROOT}/usr/bin/${bin}" 2>/dev/null || true
done

for bin in "${GUI_BINS[@]}"; do
    install -m 0755 "target/release/${bin}" "${PKGROOT}/usr/bin/${bin}"
    strip --strip-unneeded "${PKGROOT}/usr/bin/${bin}" 2>/dev/null || true
done

install -m 0644 dist/systemd/lanlinkd@.service "${PKGROOT}/lib/systemd/system/"
install -m 0644 dist/systemd/lanlink-coordinator.service "${PKGROOT}/lib/systemd/system/"
install -m 0644 dist/systemd/lanlink-relayd.service "${PKGROOT}/lib/systemd/system/"
install -m 0644 dist/lanlinkd.example.toml "${PKGROOT}/etc/lanlink/"
install -m 0644 README.md "${PKGROOT}/usr/share/doc/lanlink/"
install -m 0644 LICENSE "${PKGROOT}/usr/share/doc/lanlink/" 2>/dev/null || true

install -m 0644 dist/deb/conffiles "${PKGROOT}/DEBIAN/conffiles"
install -m 0755 dist/deb/postinst "${PKGROOT}/DEBIAN/postinst"
install -m 0755 dist/deb/prerm "${PKGROOT}/DEBIAN/prerm"

DESCRIPTION_EXTRA=""
if [ "${#GUI_BINS[@]}" -gt 0 ]; then
  DESCRIPTION_EXTRA=" This package also includes the lanlink-gui unified launcher and the selected frontends: ${GUI_BINS[*]}."
fi

cat > "${PKGROOT}/DEBIAN/control" <<EOF
Package: lanlink
Version: ${VERSION}
Section: net
Priority: optional
Architecture: ${ARCH}
Depends: libc6, nftables, iproute2
Recommends: stun-server | coturn
Maintainer: LanLink Contributors <lanlink@example.com>
Description: Cross-platform Radmin-VPN-style virtual LAN (daemon + tools)
 LanLink is a free layer-2 mesh VPN aimed at gaming, remote access and
 connecting distributed teams. It emulates a shared Ethernet segment (ARP,
 broadcast, multicast) over encrypted peer-to-peer UDP with NAT traversal
 and a content-blind relay fallback.
 .
 This package contains the lanlinkd daemon, lanlinkctl CLI, the
 lanlink-relayd relay server and the lanlink-coordinator membership server.${DESCRIPTION_EXTRA}
Homepage: https://github.com/lanlink/lanlink
EOF

# Config-file state: the example is a conffile; real per-network configs are
# created by the admin and picked up as lanlinkd@<name> instances.
echo "==> building .deb"
mkdir -p "${OUT}"
dpkg-deb --build "${PKGROOT}" "${OUT}/lanlink_${VERSION}-1_${ARCH}.deb"
echo "==> wrote ${OUT}/lanlink_${VERSION}-1_${ARCH}.deb"
