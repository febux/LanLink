#!/usr/bin/env bash
# One-line installer for LanLink on Linux.
#
# Install the latest release:
#   curl -fsSL https://raw.githubusercontent.com/lanlink/lanlink/main/dist/install.sh | sh
#
# With options:
#   curl -fsSL ... | sh -s -- --gui desktop --channel stable
#
# Options:
#   --dir PATH       Install prefix (default: /usr/local)
#   --channel stable|beta|nightly  Release channel (default: stable)
#   --gui none|tui|web|desktop|all  GUI variant (default: desktop)
#   --pkg deb|rpm|tarball|appimage|flatpak|snap  Package type (default: auto)
#   --version X.Y.Z  Exact version to install (default: latest)
#   --no-extract     For AppImage/Flatpak/Snap: only download, do not install
#   --dry-run        Print what would be done without doing it
#   -h, --help       Show this help
#
# This script runs as root (or via sudo) and installs binaries, systemd units,
# and (when using native packages) the lanlink system user.
set -euo pipefail

REPO="${LANLINK_REPO:-lanlink/lanlink}"
DEFAULT_DIR="/usr/local"
DEFAULT_CHANNEL="stable"
DEFAULT_GUI="desktop"

log() { printf '[lanlink-install] %s\n' "$*"; }
die() { printf '[lanlink-install] ERROR: %s\n' "$*" >&2; exit 1; }

# Parse args.
INSTALL_DIR="${DEFAULT_DIR}"
CHANNEL="${DEFAULT_CHANNEL}"
GUI="${DEFAULT_GUI}"
PKG=""
VERSION=""
NO_EXTRACT=0
DRY_RUN=0

while [ $# -gt 0 ]; do
  case "$1" in
    --dir) INSTALL_DIR="$2"; shift 2 ;;
    --channel) CHANNEL="$2"; shift 2 ;;
    --gui) GUI="$2"; shift 2 ;;
    --pkg) PKG="$2"; shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    --no-extract) NO_EXTRACT=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help)
      sed -n '2,21p' "$0" | sed -E 's/^# ?//'
      exit 0
      ;;
    *)
      if [ "$1" = "--" ]; then shift; break; fi
      die "unknown option: $1"
      ;;
  esac
done

need_sudo() {
  if [ "$(id -u)" -ne 0 ]; then
    if command -v sudo >/dev/null 2>&1; then
      echo "sudo"
    else
      die "this script must run as root"
    fi
  fi
}

SUDO="$(need_sudo)"

run() {
  if [ "${DRY_RUN}" = "1" ]; then
    echo "DRY_RUN: $*"
  else
    log "$*"
    if [ -n "${SUDO}" ] && [ "$(id -u)" -ne 0 ]; then
      ${SUDO} "$@"
    else
      "$@"
    fi
  fi
}

# Detect machine architecture (Debian-style where possible).
detect_arch() {
  local arch
  if command -v dpkg >/dev/null 2>&1; then
    arch="$(dpkg --print-architecture)"
  else
    arch="$(uname -m)"
    case "${arch}" in
      x86_64) arch="amd64" ;;
      aarch64|arm64) arch="arm64" ;;
      armv7l) arch="armhf" ;;
    esac
  fi
  echo "${arch}"
}

ARCH="$(detect_arch)"

# Map arch to tarball-friendly name.
tarball_arch() {
  case "${ARCH}" in
    amd64) echo "x86_64" ;;
    arm64) echo "aarch64" ;;
    armhf) echo "armv7" ;;
    *) echo "${ARCH}" ;;
  esac
}

# Auto-detect package type if not specified.
if [ -z "${PKG}" ]; then
  if command -v apt-get >/dev/null 2>&1 && command -v dpkg >/dev/null 2>&1; then
    PKG="deb"
  elif command -v dnf >/dev/null 2>&1 || command -v yum >/dev/null 2>&1 || command -v rpm >/dev/null 2>&1; then
    PKG="rpm"
  elif command -v pacman >/dev/null 2>&1; then
    PKG="tarball"
  else
    PKG="tarball"
  fi
  log "auto-detected package type: ${PKG}"
fi

# Resolve latest version via GitHub Releases API.
if [ -z "${VERSION}" ]; then
  log "resolving latest ${CHANNEL} release from GitHub"
  TAG="$(${SUDO:+} curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p')"
  if [ -z "${TAG}" ]; then
    die "could not determine latest release (check network or set --version)"
  fi
  VERSION="${TAG#v}"
fi

log "installing LanLink ${VERSION} (${PKG}, gui=${GUI}, arch=${ARCH})"

# Download helpers.
tmpdir="$(mktemp -d)"
trap 'rm -rf "${tmpdir}"' EXIT

download() {
  local url="$1" out="$2"
  if [ "${DRY_RUN}" = "1" ]; then
    echo "DRY_RUN: would download ${url} to ${out}"
    return 0
  fi
  log "downloading ${url}"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL -o "${out}" "${url}"
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O "${out}" "${url}"
  else
    die "curl or wget is required"
  fi
}

install_deb() {
  local deb="lanlink_${VERSION}-1_${ARCH}.deb"
  local url="https://github.com/${REPO}/releases/download/v${VERSION}/${deb}"
  download "${url}" "${tmpdir}/${deb}"
  if [ "${DRY_RUN}" = "1" ]; then
    echo "DRY_RUN: dpkg -i ${tmpdir}/${deb}; apt-get install -f"
  else
    run dpkg -i "${tmpdir}/${deb}" || true
    if command -v apt-get >/dev/null 2>&1; then
      run apt-get install -f -y
    fi
  fi
}

install_rpm() {
  local rpm_arch="${ARCH}"
  case "${ARCH}" in
    amd64) rpm_arch="x86_64" ;;
    arm64) rpm_arch="aarch64" ;;
    armhf) rpm_arch="armv7hl" ;;
  esac
  local rpm="lanlink-${VERSION}-1.${rpm_arch}.rpm"
  local url="https://github.com/${REPO}/releases/download/v${VERSION}/${rpm}"
  download "${url}" "${tmpdir}/${rpm}"
  if command -v dnf >/dev/null 2>&1; then
    run dnf install -y "${tmpdir}/${rpm}"
  elif command -v yum >/dev/null 2>&1; then
    run yum install -y "${tmpdir}/${rpm}"
  else
    run rpm -i "${tmpdir}/${rpm}"
  fi
}

install_tarball() {
  local tarch="$(tarball_arch)"
  local tar="lanlink-${VERSION}-linux-${tarch}.tar.gz"
  local url="https://github.com/${REPO}/releases/download/v${VERSION}/${tar}"
  download "${url}" "${tmpdir}/${tar}"

  if [ "${DRY_RUN}" = "1" ]; then
    echo "DRY_RUN: extract ${tar} to ${tmpdir} and run install-local.sh ${INSTALL_DIR}"
    return
  fi

  tar -xzf "${tmpdir}/${tar}" -C "${tmpdir}"
  local extracted="${tmpdir}/lanlink-${VERSION}-${tarch}"
  [ -d "${extracted}" ] || die "unexpected tarball layout"

  # Install binaries and units.
  run install -d "${INSTALL_DIR}/bin"
  for bin in "${extracted}"/bin/*; do
    run install -m 0755 "${bin}" "${INSTALL_DIR}/bin/$(basename "${bin}")"
  done

  run install -d "/etc/systemd/system"
  for unit in "${extracted}"/systemd/*.service; do
    run install -m 0644 "${unit}" "/etc/systemd/system/"
  done

  run install -d -m 0750 /etc/lanlink
  if [ ! -f /etc/lanlink/lanlinkd.example.toml ]; then
    run install -m 0644 "${extracted}/share/doc/lanlinkd.example.toml" /etc/lanlink/
  fi

  run systemctl daemon-reload
  run useradd -r -s /usr/sbin/nologin -M lanlink 2>/dev/null || true
  log "installed to ${INSTALL_DIR}/bin"
}

install_appimage() {
  local app="lanlink-${VERSION}-linux-x86_64.AppImage"
  local url="https://github.com/${REPO}/releases/download/v${VERSION}/${app}"
  download "${url}" "${tmpdir}/${app}"
  if [ "${NO_EXTRACT}" = "1" ]; then
    run install -Dm0755 "${tmpdir}/${app}" "${INSTALL_DIR}/bin/lanlink-desktop"
  else
    run chmod +x "${tmpdir}/${app}"
    log "AppImage is ready at ${tmpdir}/${app}"
    log "copy it to a permanent location, e.g.:"
    log "  ${SUDO:+${SUDO} }cp ${tmpdir}/${app} ~/Applications/"
  fi
}

install_flatpak() {
  command -v flatpak >/dev/null 2>&1 || die "flatpak is required for --pkg flatpak"
  local fp="lanlink-${VERSION}-linux-x86_64.flatpak"
  local url="https://github.com/${REPO}/releases/download/v${VERSION}/${fp}"
  download "${url}" "${tmpdir}/${fp}"
  if [ "${NO_EXTRACT}" = "1" ]; then
    run install -Dm0644 "${tmpdir}/${fp}" "${INSTALL_DIR}/share/lanlink/${fp}"
  else
    run flatpak install -y "${tmpdir}/${fp}"
  fi
}

install_snap() {
  command -v snap >/dev/null 2>&1 || die "snap is required for --pkg snap"
  local snap="lanlink_${VERSION}_amd64.snap"
  local url="https://github.com/${REPO}/releases/download/v${VERSION}/${snap}"
  download "${url}" "${tmpdir}/${snap}"
  if [ "${NO_EXTRACT}" = "1" ]; then
    run install -Dm0644 "${tmpdir}/${snap}" "${INSTALL_DIR}/share/lanlink/${snap}"
  else
    run snap install --dangerous --devmode "${tmpdir}/${snap}"
  fi
}

case "${PKG}" in
  deb) install_deb ;;
  rpm) install_rpm ;;
  tarball) install_tarball ;;
  appimage) install_appimage ;;
  flatpak) install_flatpak ;;
  snap) install_snap ;;
  *) die "unsupported package type: ${PKG}" ;;
esac

log "installation complete"
log "next steps:"
log "  1. start the daemon:    sudo lanlinkd service up <network-name>"
log "  2. open the desktop UI: lanlink-desktop"
log "  3. or the WebUI:        lanlink-web"
