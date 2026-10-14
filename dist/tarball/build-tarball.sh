#!/usr/bin/env bash
# Build a portable LanLink tarball for Linux.
#
# Usage: dist/tarball/build-tarball.sh [output-dir]
#
# Produces: dist/tarball/out/lanlink-<version>-linux-<arch>.tar.gz
#
# Contents:
#   - bin/lanlinkd, bin/lanlinkctl, bin/lanlink-relayd, bin/lanlink-coordinator
#   - bin/lanlink-gui, bin/lanlink-tui, bin/lanlink-web, bin/lanlink-desktop (if built)
#   - systemd/lanlinkd@.service, systemd/lanlink-coordinator.service,
#     systemd/lanlink-relayd.service
#   - share/doc/lanlinkd.example.toml
#   - install.sh (local copy that can install systemd units from the tarball)
#
# Environment:
#   LANLINK_VERSION  package version (default: workspace version)
#   TARGET           Rust target triple (default: host target; common:
#                    x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu,
#                    x86_64-unknown-linux-musl)
#   MUSL=1           Use musl for a more portable static-ish build (installs the
#                    target and sets RUSTFLAGS if musl-tools is available)
#   SKIP_BUILD=1     Reuse already-built binaries in target/<target>/release.
#   LL_GUI           Comma-separated: tui, web, desktop, all (default: all)
#
# Requires: cargo, tar, gzip. Optional: musl-tools for MUSL=1.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_DIR}"

VERSION="${LANLINK_VERSION:-$(awk -F'"' '/^version =/{print $2; exit}' Cargo.toml)}"
VERSION="${VERSION:-0.1.0}"
OUT="${1:-${SCRIPT_DIR}/out}"
STAGE="${SCRIPT_DIR}/stage"

TARGET="${TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
MUSL="${MUSL:-0}"
LL_GUI="${LL_GUI:-all}"

log() { printf '[tarball] %s\n' "$*"; }
die() { printf '[tarball] ERROR: %s\n' "$*" >&2; exit 1; }

# Map target triple to a friendly arch string for the filename.
arch_name() {
  case "$1" in
    x86_64-unknown-linux-gnu|x86_64-unknown-linux-musl) echo "x86_64" ;;
    aarch64-unknown-linux-gnu|aarch64-unknown-linux-musl) echo "aarch64" ;;
    armv7-unknown-linux-gnueabihf|armv7-unknown-linux-musleabihf) echo "armv7" ;;
    *) echo "${1%%-*}" ;;
  esac
}

ARCH="$(arch_name "${TARGET}")"

# ---- musl setup -----------------------------------------------------------
if [ "${MUSL}" = "1" ]; then
  case "${TARGET}" in
    *-musl)
      log "target ${TARGET} is already musl"
      ;;
    x86_64-unknown-linux-gnu)
      TARGET="x86_64-unknown-linux-musl"
      ;;
    aarch64-unknown-linux-gnu)
      TARGET="aarch64-unknown-linux-musl"
      ;;
    *)
      die "MUSL=1 is not supported for target ${TARGET}"
      ;;
  esac
  export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static"
  if command -v musl-gcc >/dev/null 2>&1; then
    export CC="${CC:-musl-gcc}"
    export CXX="${CXX:-musl-g++}"
  elif command -v musl-clang >/dev/null 2>&1; then
    export CC="${CC:-musl-clang}"
  else
    log "WARNING: musl-gcc not found; trying to install the Rust target anyway"
  fi
fi

# Make sure the target is installed.
if ! rustup target list --installed 2>/dev/null | grep -qx "${TARGET}"; then
  log "installing Rust target ${TARGET}"
  rustup target add "${TARGET}" || die "failed to add target ${TARGET}"
fi

# ---- Resolve GUI selector -------------------------------------------------
WITH_TUI=0
WITH_WEB=0
WITH_DESKTOP=0
case ",${LL_GUI}," in
  *,all,*) WITH_TUI=1; WITH_WEB=1; WITH_DESKTOP=1 ;;
esac
case ",${LL_GUI}," in
  *,tui,*) WITH_TUI=1 ;;
esac
case ",${LL_GUI}," in
  *,web,*) WITH_WEB=1 ;;
esac
case ",${LL_GUI}," in
  *,desktop,*) WITH_DESKTOP=1 ;;
esac

GUI_BINS=()
[ "${WITH_TUI}" = 1 ] && GUI_BINS+=(lanlink-tui)
[ "${WITH_WEB}" = 1 ] && GUI_BINS+=(lanlink-web)
[ "${WITH_DESKTOP}" = 1 ] && GUI_BINS+=(lanlink-desktop)
[ "${#GUI_BINS[@]}" -gt 0 ] && GUI_BINS+=(lanlink-gui)

CARGO_PACKAGES=(-p lanlinkd -p lanlinkctl -p lanlink-relay -p coordinator)
for bin in "${GUI_BINS[@]}"; do
  CARGO_PACKAGES+=(-p "${bin}")
done

# ---- Build binaries -------------------------------------------------------
if [ "${SKIP_BUILD:-0}" != "1" ]; then
  log "building release binaries for ${TARGET}"
  cargo build --release --target "${TARGET}" "${CARGO_PACKAGES[@]}"
fi

BIN_DIR="target/${TARGET}/release"
for bin in lanlinkd lanlinkctl lanlink-relayd lanlink-coordinator; do
  [ -f "${BIN_DIR}/${bin}" ] || die "missing ${BIN_DIR}/${bin}"
done
for bin in "${GUI_BINS[@]}"; do
  [ -f "${BIN_DIR}/${bin}" ] || die "missing ${BIN_DIR}/${bin}"
done

# ---- Stage tarball contents -----------------------------------------------
log "staging tarball at ${STAGE}"
rm -rf "${STAGE}"
install -d -m 0755 "${STAGE}/lanlink-${VERSION}-${ARCH}/bin"
install -d -m 0755 "${STAGE}/lanlink-${VERSION}-${ARCH}/systemd"
install -d -m 0755 "${STAGE}/lanlink-${VERSION}-${ARCH}/share/doc"

for bin in lanlinkd lanlinkctl lanlink-relayd lanlink-coordinator; do
  install -m 0755 "${BIN_DIR}/${bin}" "${STAGE}/lanlink-${VERSION}-${ARCH}/bin/${bin}"
  strip --strip-unneeded "${STAGE}/lanlink-${VERSION}-${ARCH}/bin/${bin}" 2>/dev/null || true
done
for bin in "${GUI_BINS[@]}"; do
  install -m 0755 "${BIN_DIR}/${bin}" "${STAGE}/lanlink-${VERSION}-${ARCH}/bin/${bin}"
  strip --strip-unneeded "${STAGE}/lanlink-${VERSION}-${ARCH}/bin/${bin}" 2>/dev/null || true
done

install -m 0644 dist/systemd/lanlinkd@.service "${STAGE}/lanlink-${VERSION}-${ARCH}/systemd/"
install -m 0644 dist/systemd/lanlink-coordinator.service "${STAGE}/lanlink-${VERSION}-${ARCH}/systemd/"
install -m 0644 dist/systemd/lanlink-relayd.service "${STAGE}/lanlink-${VERSION}-${ARCH}/systemd/"
install -m 0644 dist/lanlinkd.example.toml "${STAGE}/lanlink-${VERSION}-${ARCH}/share/doc/"
install -m 0644 README.md "${STAGE}/lanlink-${VERSION}-${ARCH}/share/doc/" 2>/dev/null || true
install -m 0644 LICENSE "${STAGE}/lanlink-${VERSION}-${ARCH}/share/doc/" 2>/dev/null || true

# A local install helper that works from an extracted tarball.
cat > "${STAGE}/lanlink-${VERSION}-${ARCH}/install-local.sh" <<'EOF'
#!/usr/bin/env bash
# Install LanLink from an extracted tarball.
# Run as root (or with sudo) from inside the extracted directory.
set -euo pipefail
PREFIX="${1:-/usr/local}"
UNIT_DIR="${2:-/etc/systemd/system}"
[ "$(id -u)" -ne 0 ] && { echo "run as root or with sudo"; exit 1; }
install -d "${PREFIX}/bin"
cp -v bin/* "${PREFIX}/bin/"
install -d "${UNIT_DIR}"
cp -v systemd/* "${UNIT_DIR}/"
install -d /etc/lanlink
cp -n share/doc/lanlinkd.example.toml /etc/lanlink/ 2>/dev/null || true
systemctl daemon-reload
useradd -r -s /usr/sbin/nologin -M lanlink 2>/dev/null || true
echo "LanLink installed to ${PREFIX}/bin"
echo "Next: lanlinkd service up <network-name>"
EOF
chmod 0755 "${STAGE}/lanlink-${VERSION}-${ARCH}/install-local.sh"

# ---- Tar ------------------------------------------------------------------
mkdir -p "${OUT}"
TARBALL="${OUT}/lanlink-${VERSION}-linux-${ARCH}.tar.gz"
log "creating ${TARBALL}"
tar -czf "${TARBALL}" -C "${STAGE}" "lanlink-${VERSION}-${ARCH}"
log "wrote ${TARBALL}"
