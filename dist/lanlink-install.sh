#!/usr/bin/env bash
# LanLink install wizard — phase 2 (roles + modes + docs).
#
# One-line install (once published):
#   curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | bash
#
# Local dry-run against the repository's dist/ output:
#   bash scripts/install.sh --dry-run --role coordinator
#
# Options:
#   --role ROLE            coordinator | peer | relay (default: env LANLINK_ROLE)
#   --coord-url URL        coordinator base URL (env: LANLINK_COORD_URL)
#   --listen ADDR          coordinator/relay listen address (env: LANLINK_LISTEN)
#   --relay-server ADDR    relay server address advertised to peers (env: LANLINK_RELAY_SERVER)
#   --network NAME         peer network name (env: LANLINK_NETWORK)
#   --password PASS        peer network password (env: LANLINK_PASSWORD)
#   --password-file FILE   file containing peer password (env: LANLINK_PASSWORD_FILE)
#   --data-dir PATH        coordinator data directory (env: LANLINK_DATA_DIR)
#   --gui SELECTOR         none | tui | web | desktop | all (env: LANLINK_GUI)
#   --pkg TYPE             deb | rpm | aur | tarball (env: LANLINK_PKG)
#   --version VERSION      exact release version (env: LANLINK_VERSION)
#   --channel CHANNEL      release channel (env: LANLINK_CHANNEL)
#   --prefix PATH          install prefix for tarballs (env: LANLINK_INSTALL_PREFIX)
#   --yes                  non-interactive mode (env: LANLINK_YES)
#   --dry-run              print actions without executing them
#   --uninstall            remove role-specific configuration and units
#   --reconfigure          overwrite existing config files
#   --with-relay           enable lanlink-relayd for coordinator role
#   --purge                remove package during uninstall
#   -h, --help             show this help
#
# The script installs the correct release artifact for the current Linux
# distro/arch, verifies checksums against the published SHA256SUMS file, and
# then configures the requested role. It uses only bash, curl and sha256sum.
set -euo pipefail

REPO="${LANLINK_REPO:-febux/LanLink}"
DEFAULT_PREFIX="/usr/local"
DEFAULT_CHANNEL="stable"
DEFAULT_GUI="none"
DEFAULT_DATA_DIR="/var/lib/lanlink"
DEFAULT_LISTEN_COORD="0.0.0.0:8080"
DEFAULT_LISTEN_RELAY="0.0.0.0:3478"

SCRIPT_DIR=""
if [ -n "${BASH_SOURCE[0]:-}" ]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd 2>/dev/null || true)"
fi

log() { printf '[lanlink-install] %s\n' "$*" >&2; }
warn() { printf '[lanlink-install] WARNING: %s\n' "$*" >&2; }
die() { printf '[lanlink-install] ERROR: %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<'EOF'
LanLink install wizard — phase 2 (roles + modes + docs).

One-line install (once published):
  curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | bash

Local dry-run against the repository's dist/ output:
  bash scripts/install.sh --dry-run --role coordinator

Options:
  --role ROLE            coordinator | peer | relay (default: env LANLINK_ROLE)
  --coord-url URL        coordinator base URL (env: LANLINK_COORD_URL)
  --listen ADDR          coordinator/relay listen address (env: LANLINK_LISTEN)
  --relay-server ADDR    relay server address advertised to peers (env: LANLINK_RELAY_SERVER)
  --network NAME         peer network name (env: LANLINK_NETWORK)
  --password PASS        peer network password (env: LANLINK_PASSWORD)
  --password-file FILE   file containing peer password (env: LANLINK_PASSWORD_FILE)
  --data-dir PATH        coordinator data directory (env: LANLINK_DATA_DIR)
  --gui SELECTOR         none | tui | web | desktop | all (env: LANLINK_GUI)
  --pkg TYPE             deb | rpm | aur | tarball (env: LANLINK_PKG)
  --version VERSION      exact release version (env: LANLINK_VERSION)
  --channel CHANNEL      release channel (env: LANLINK_CHANNEL)
  --prefix PATH          install prefix for tarballs (env: LANLINK_INSTALL_PREFIX)
  --yes                  non-interactive mode (env: LANLINK_YES)
  --dry-run              print actions without executing them
  --uninstall            remove role-specific configuration and units
  --reconfigure          overwrite existing config files
  --with-relay           enable lanlink-relayd for coordinator role
  --purge                remove package during uninstall
  -h, --help             show this help

The script installs the correct release artifact for the current Linux
distro/arch, verifies checksums against the published SHA256SUMS file, and
then configures the requested role. It uses only bash, curl and sha256sum.
EOF
}

# ---------------------------------------------------------------------------
# Defaults and argument parsing. Environment variables win over CLI flags.
# ---------------------------------------------------------------------------

ROLE="${LANLINK_ROLE:-}"
PKG="${LANLINK_PKG:-}"
VERSION="${LANLINK_VERSION:-}"
CHANNEL="${LANLINK_CHANNEL:-${DEFAULT_CHANNEL}}"
PREFIX="${LANLINK_INSTALL_PREFIX:-${DEFAULT_PREFIX}}"
GUI="${LANLINK_GUI:-${DEFAULT_GUI}}"
COORD_URL="${LANLINK_COORD_URL:-}"
NETWORK="${LANLINK_NETWORK:-}"
PASSWORD="${LANLINK_PASSWORD:-}"
PASSWORD_FILE="${LANLINK_PASSWORD_FILE:-}"
DATA_DIR="${LANLINK_DATA_DIR:-${DEFAULT_DATA_DIR}}"
LISTEN="${LANLINK_LISTEN:-}"
RELAY_SERVER="${LANLINK_RELAY_SERVER:-}"
YES="${LANLINK_YES:-0}"
DRY_RUN=0
UNINSTALL=0
RECONFIGURE=0
WITH_RELAY=0
PURGE=0

while [ $# -gt 0 ]; do
  case "$1" in
    --role)
      [ -z "${LANLINK_ROLE:-}" ] && ROLE="$2"
      shift 2
      ;;
    --coord-url)
      [ -z "${LANLINK_COORD_URL:-}" ] && COORD_URL="$2"
      shift 2
      ;;
    --listen)
      [ -z "${LANLINK_LISTEN:-}" ] && LISTEN="$2"
      shift 2
      ;;
    --relay-server)
      [ -z "${LANLINK_RELAY_SERVER:-}" ] && RELAY_SERVER="$2"
      shift 2
      ;;
    --network)
      [ -z "${LANLINK_NETWORK:-}" ] && NETWORK="$2"
      shift 2
      ;;
    --password)
      [ -z "${LANLINK_PASSWORD:-}" ] && PASSWORD="$2"
      shift 2
      ;;
    --password-file)
      [ -z "${LANLINK_PASSWORD_FILE:-}" ] && PASSWORD_FILE="$2"
      shift 2
      ;;
    --data-dir)
      [ -z "${LANLINK_DATA_DIR:-}" ] && DATA_DIR="$2"
      shift 2
      ;;
    --gui)
      [ -z "${LANLINK_GUI:-}" ] && GUI="$2"
      shift 2
      ;;
    --pkg)
      [ -z "${LANLINK_PKG:-}" ] && PKG="$2"
      shift 2
      ;;
    --version)
      [ -z "${LANLINK_VERSION:-}" ] && VERSION="$2"
      shift 2
      ;;
    --channel)
      [ -z "${LANLINK_CHANNEL:-}" ] && CHANNEL="$2"
      shift 2
      ;;
    --prefix)
      [ -z "${LANLINK_INSTALL_PREFIX:-}" ] && PREFIX="$2"
      shift 2
      ;;
    --yes)
      [ -z "${LANLINK_YES:-}" ] && YES=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --uninstall)
      UNINSTALL=1
      shift
      ;;
    --reconfigure)
      RECONFIGURE=1
      shift
      ;;
    --with-relay)
      WITH_RELAY=1
      shift
      ;;
    --purge)
      PURGE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      break
      ;;
    -*)
      die "unknown option: $1 (try --help)"
      ;;
    *)
      die "unexpected argument: $1"
      ;;
  esac
done

# ---------------------------------------------------------------------------
# Version / tag helpers
# ---------------------------------------------------------------------------

version_from_cargo() {
  local cargo_toml
  cargo_toml="${SCRIPT_DIR}/../Cargo.toml"
  if [ -n "${SCRIPT_DIR}" ] && [ -f "${cargo_toml}" ]; then
    awk -F'"' '/^version =/{print $2; exit}' "${cargo_toml}"
  fi
}

resolve_version() {
  if [ -n "${VERSION}" ]; then
    printf '%s\n' "${VERSION}"
    return 0
  fi

  local cargo_version
  cargo_version="$(version_from_cargo)"
  if [ -n "${cargo_version}" ]; then
    printf '%s\n' "${cargo_version}"
    return 0
  fi

  if [ "${DRY_RUN}" = 1 ]; then
    warn "DRY_RUN: cannot query GitHub for the latest version"
    printf '%s\n' "latest"
    return 0
  fi

  if ! command -v curl >/dev/null 2>&1; then
    die "curl is required to resolve the latest release"
  fi

  local url
  if [ "${CHANNEL}" = "stable" ]; then
    url="https://api.github.com/repos/${REPO}/releases/latest"
  else
    url="https://api.github.com/repos/${REPO}/releases?per_page=1"
  fi

  local tag
  tag="$(curl -fsSL "${url}" 2>/dev/null \
    | sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' \
    | head -n1)"
  if [ -z "${tag}" ]; then
    die "could not determine the latest release (set --version or LANLINK_VERSION)"
  fi
  printf '%s\n' "${tag#v}"
}

resolve_tag() {
  printf 'v%s\n' "$1"
}

# ---------------------------------------------------------------------------
# OS / architecture detection
# ---------------------------------------------------------------------------

detect_arch() {
  local arch
  if command -v dpkg >/dev/null 2>&1; then
    arch="$(dpkg --print-architecture)"
  else
    arch="$(uname -m)"
  fi

  case "${arch}" in
    amd64|x86_64)     printf 'x86_64\n' ;;
    arm64|aarch64)    printf 'aarch64\n' ;;
    armhf|armv7l|armv7) printf 'armv7\n' ;;
    *)                printf '%s\n' "${arch}" ;;
  esac
}

deb_arch() {
  case "$1" in
    x86_64)  printf 'amd64\n' ;;
    aarch64) printf 'arm64\n' ;;
    armv7)   printf 'armhf\n' ;;
    *)       printf '%s\n' "$1" ;;
  esac
}

rpm_arch() {
  case "$1" in
    x86_64)  printf 'x86_64\n' ;;
    aarch64) printf 'aarch64\n' ;;
    armv7)   printf 'armv7hl\n' ;;
    *)       printf '%s\n' "$1" ;;
  esac
}

tarball_arch() {
  printf '%s\n' "$1"
}

detect_pkg_type() {
  if command -v apt-get >/dev/null 2>&1 && command -v dpkg >/dev/null 2>&1; then
    printf 'deb\n'
    return 0
  fi
  if command -v dnf >/dev/null 2>&1 || command -v yum >/dev/null 2>&1 || command -v rpm >/dev/null 2>&1; then
    printf 'rpm\n'
    return 0
  fi
  if command -v pacman >/dev/null 2>&1; then
    printf 'aur\n'
    return 0
  fi
  printf 'tarball\n'
}

# ---------------------------------------------------------------------------
# Release asset URLs. Prefer a matching local file under dist/*/out/ when the
# wizard is run from the repository, so dry-run URLs match the local layout.
# ---------------------------------------------------------------------------

find_local_asset() {
  local pkg="$1" version="$2"
  local repo_dir
  repo_dir="$(cd "${SCRIPT_DIR}/.." 2>/dev/null && pwd || true)"
  local out_dir="${repo_dir}/dist/${pkg}/out"
  if [ -z "${repo_dir}" ] || [ ! -d "${out_dir}" ]; then
    return 1
  fi

  case "${pkg}" in
    deb)
      find "${out_dir}" -maxdepth 1 -type f -name "lanlink_${version}-1_*.deb" 2>/dev/null | head -n1
      ;;
    rpm)
      find "${out_dir}" -maxdepth 1 -type f -name "lanlink-${version}-1.*.rpm" 2>/dev/null | head -n1
      ;;
    tarball)
      find "${out_dir}" -maxdepth 1 -type f -name "lanlink-${version}-linux-*.tar.gz" 2>/dev/null | head -n1
      ;;
    aur)
      find "${out_dir}" -maxdepth 1 -type f -name 'PKGBUILD' 2>/dev/null | head -n1
      ;;
    *)
      return 1
      ;;
  esac
}

asset_basename() {
  local pkg="$1" version="$2" arch="$3"
  case "${pkg}" in
    deb)     printf 'lanlink_%s-1_%s.deb\n' "${version}" "$(deb_arch "${arch}")" ;;
    rpm)     printf 'lanlink-%s-1.%s.rpm\n' "${version}" "$(rpm_arch "${arch}")" ;;
    aur)     printf 'PKGBUILD\n' ;;
    tarball) printf 'lanlink-%s-linux-%s.tar.gz\n' "${version}" "$(tarball_arch "${arch}")" ;;
    *)       die "unknown package type: ${pkg}" ;;
  esac
}

asset_url() {
  local pkg="$1" version="$2" arch="$3"
  local tag
  tag="$(resolve_tag "${version}")"

  local local_path
  local_path="$(find_local_asset "${pkg}" "${version}")"

  local base
  if [ -n "${local_path}" ]; then
    base="$(basename "${local_path}")"
  else
    base="$(asset_basename "${pkg}" "${version}" "${arch}")"
  fi

  case "${pkg}" in
    aur)
      printf 'https://raw.githubusercontent.com/%s/%s/dist/aur/%s\n' \
        "${REPO}" "${tag}" "${base}"
      ;;
    *)
      printf 'https://github.com/%s/releases/download/%s/%s\n' \
        "${REPO}" "${tag}" "${base}"
      ;;
  esac
}

# ---------------------------------------------------------------------------
# Sudo / command execution helpers
# ---------------------------------------------------------------------------

need_sudo() {
  if [ "$(id -u)" -ne 0 ]; then
    if command -v sudo >/dev/null 2>&1; then
      printf 'sudo\n'
    else
      die "root privileges are required; install sudo or run as root"
    fi
  fi
}

SUDO=""
if [ "${DRY_RUN}" = 0 ]; then
  SUDO="$(need_sudo)"
fi

run() {
  if [ "${DRY_RUN}" = 1 ]; then
    printf 'DRY_RUN: %s\n' "$*"
    return 0
  fi
  log "$*"
  if [ -n "${SUDO}" ]; then
    ${SUDO} "$@"
  else
    "$@"
  fi
}

# ---------------------------------------------------------------------------
# Network / checksum helpers
# ---------------------------------------------------------------------------

curl_get() {
  local url="$1" out="$2"
  if ! command -v curl >/dev/null 2>&1; then
    die "curl is required"
  fi
  curl -fsSL -o "${out}" "${url}" || return 1
  [ -s "${out}" ] || return 1
  return 0
}

download_file() {
  local url="$1" out="$2"
  if [ "${DRY_RUN}" = 1 ]; then
    printf 'DRY_RUN: would download %s -> %s\n' "${url}" "${out}"
    return 0
  fi
  log "downloading ${url}"
  curl_get "${url}" "${out}" || die "failed to download ${url}"
}

fetch_checksums() {
  local tag="$1" out="$2"
  local url="https://github.com/${REPO}/releases/download/${tag}/SHA256SUMS"
  if [ "${DRY_RUN}" = 1 ]; then
    printf 'DRY_RUN: would download %s -> %s\n' "${url}" "${out}"
    return 0
  fi
  curl_get "${url}" "${out}" || return 1
}

verify_checksum() {
  local asset="$1" sums="$2"
  if [ ! -f "${sums}" ]; then
    warn "no SHA256SUMS available; skipping checksum verification"
    return 0
  fi

  local base expected
  base="$(basename "${asset}")"
  expected="$(awk -v b="${base}" '$2 == b {print $1; exit}' "${sums}")"
  if [ -z "${expected}" ]; then
    warn "no checksum entry for ${base}; skipping"
    return 0
  fi

  local actual
  actual="$(sha256sum "${asset}" | awk '{print $1}')"
  if [ "${expected}" != "${actual}" ]; then
    die "checksum mismatch for ${base} (expected ${expected}, got ${actual})"
  fi
  log "checksum OK: ${base}"
}

# ---------------------------------------------------------------------------
# Package install functions
# ---------------------------------------------------------------------------

tmpdir=""
cleanup_tmpdir() {
  if [ -n "${tmpdir}" ] && [ -d "${tmpdir}" ]; then
    rm -rf "${tmpdir}"
  fi
}
trap cleanup_tmpdir EXIT

installed_version() {
  local pkg_type="$1"
  case "${pkg_type}" in
    deb)
      if command -v dpkg >/dev/null 2>&1; then
        dpkg-query -W -f='${Version}' lanlink 2>/dev/null || true
      fi
      ;;
    rpm)
      if command -v rpm >/dev/null 2>&1; then
        rpm -q --queryformat '%{VERSION}' lanlink 2>/dev/null || true
      fi
      ;;
    *)
      # Tarball installs do not keep a version registry; rely on binary --version.
      if command -v lanlinkd >/dev/null 2>&1; then
        lanlinkd --version 2>/dev/null | head -n1 || true
      fi
      ;;
  esac
}

version_newer() {
  local a="$1" b="$2"
  # Lexicographic sort on normalized dotted version is sufficient for our
  # release tags (e.g. 0.6.1 vs 0.6.0).  Return 0 if a is newer than b.
  [ "$(printf '%s\n%s\n' "${a}" "${b}" | sort -V | tail -n1)" = "${a}" ] && [ "${a}" != "${b}" ]
}

install_deb() {
  local version="$1" arch="$2"
  local url base current
  url="$(asset_url deb "${version}" "${arch}")"
  base="$(basename "${url}")"

  current="$(installed_version deb)"
  if [ -n "${current}" ] && ! version_newer "${version}" "${current}"; then
    log "lanlink ${current} is already installed (not upgrading to ${version})"
    return 0
  fi

  tmpdir="$(mktemp -d)"
  local asset="${tmpdir}/${base}"

  local local_asset
  if local_asset="$(find_local_asset deb "${version}")" && [ -n "${local_asset}" ]; then
    asset="${local_asset}"
  else
    download_file "${url}" "${asset}"
  fi

  local sums="${tmpdir}/SHA256SUMS"
  local tag
  tag="$(resolve_tag "${version}")"
  if fetch_checksums "${tag}" "${sums}"; then
    verify_checksum "${asset}" "${sums}"
  else
    warn "could not fetch SHA256SUMS; continuing without checksum verification"
  fi

  if [ "${DRY_RUN}" = 1 ]; then
    printf 'DRY_RUN: dpkg -i %s; apt-get install -f -y\n' "${asset}"
    return 0
  fi

  run dpkg -i "${asset}" || true
  if command -v apt-get >/dev/null 2>&1; then
    run apt-get install -f -y
  fi
}

install_rpm() {
  local version="$1" arch="$2"
  local url base current
  url="$(asset_url rpm "${version}" "${arch}")"
  base="$(basename "${url}")"

  current="$(installed_version rpm)"
  if [ -n "${current}" ] && ! version_newer "${version}" "${current}"; then
    log "lanlink ${current} is already installed (not upgrading to ${version})"
    return 0
  fi

  tmpdir="$(mktemp -d)"
  local asset="${tmpdir}/${base}"

  local local_asset
  if local_asset="$(find_local_asset rpm "${version}")" && [ -n "${local_asset}" ]; then
    asset="${local_asset}"
  else
    download_file "${url}" "${asset}"
  fi

  local sums="${tmpdir}/SHA256SUMS"
  local tag
  tag="$(resolve_tag "${version}")"
  if fetch_checksums "${tag}" "${sums}"; then
    verify_checksum "${asset}" "${sums}"
  else
    warn "could not fetch SHA256SUMS; continuing without checksum verification"
  fi

  if [ "${DRY_RUN}" = 1 ]; then
    printf 'DRY_RUN: dnf install -y %s (or yum/rpm)\n' "${asset}"
    return 0
  fi

  if command -v dnf >/dev/null 2>&1; then
    run dnf install -y "${asset}"
  elif command -v yum >/dev/null 2>&1; then
    run yum install -y "${asset}"
  else
    run rpm -Uvh "${asset}" || run rpm -i "${asset}" || true
  fi
}

install_aur() {
  local version="$1" arch="$2"
  local url base current
  url="$(asset_url aur "${version}" "${arch}")"
  base="$(basename "${url}")"

  current="$(installed_version aur)"
  if [ -n "${current}" ] && ! version_newer "${version}" "${current}"; then
    log "lanlink ${current} is already installed (not upgrading to ${version})"
    return 0
  fi

  tmpdir="$(mktemp -d)"
  local asset="${tmpdir}/${base}"

  local local_asset
  if local_asset="$(find_local_asset aur "${version}")" && [ -n "${local_asset}" ]; then
    asset="${local_asset}"
  else
    download_file "${url}" "${asset}"
  fi

  # The AUR PKGBUILD does not participate in the GitHub release SHA256SUMS file
  # in the same way as a binary artifact, so we only verify it when a local
  # SHA256SUMS entry exists.
  local sums="${tmpdir}/SHA256SUMS"
  local tag
  tag="$(resolve_tag "${version}")"
  if fetch_checksums "${tag}" "${sums}"; then
    verify_checksum "${asset}" "${sums}"
  fi

  if [ "${DRY_RUN}" = 1 ]; then
    printf 'DRY_RUN: makepkg -si in %s\n' "${tmpdir}"
    return 0
  fi

  if ! command -v makepkg >/dev/null 2>&1; then
    die "makepkg is required for AUR installs (install base-devel)"
  fi
  (cd "${tmpdir}" && run makepkg -si --noconfirm)
}

install_tarball() {
  local version="$1" arch="$2"
  local url base current
  url="$(asset_url tarball "${version}" "${arch}")"
  base="$(basename "${url}")"

  current="$(installed_version tarball)"
  if [ -n "${current}" ] && ! version_newer "${version}" "${current}"; then
    log "lanlink ${current} is already installed (not upgrading to ${version})"
    return 0
  fi

  tmpdir="$(mktemp -d)"
  local asset="${tmpdir}/${base}"

  local local_asset
  if local_asset="$(find_local_asset tarball "${version}")" && [ -n "${local_asset}" ]; then
    asset="${local_asset}"
  else
    download_file "${url}" "${asset}"
  fi

  local sums="${tmpdir}/SHA256SUMS"
  local tag
  tag="$(resolve_tag "${version}")"
  if fetch_checksums "${tag}" "${sums}"; then
    verify_checksum "${asset}" "${sums}"
  else
    warn "could not fetch SHA256SUMS; continuing without checksum verification"
  fi

  if [ "${DRY_RUN}" = 1 ]; then
    printf 'DRY_RUN: tar -xzf %s -C %s; %s/install-local.sh %s\n' \
      "${asset}" "${tmpdir}" "${tmpdir}/lanlink-${version}-$(tarball_arch "${arch}")" "${PREFIX}"
    return 0
  fi

  local extracted
  extracted="${tmpdir}/lanlink-${version}-$(tarball_arch "${arch}")"
  tar -xzf "${asset}" -C "${tmpdir}"
  [ -d "${extracted}" ] || die "unexpected tarball layout"
  (cd "${extracted}" && run ./install-local.sh "${PREFIX}")
}

install_package() {
  local pkg="$1" version="$2" arch="$3"
  case "${pkg}" in
    deb)     install_deb "${version}" "${arch}" ;;
    rpm)     install_rpm "${version}" "${arch}" ;;
    aur)     install_aur "${version}" "${arch}" ;;
    tarball) install_tarball "${version}" "${arch}" ;;
    *)       die "unsupported package type: ${pkg}" ;;
  esac
}

# ---------------------------------------------------------------------------
# systemd helpers
# ---------------------------------------------------------------------------

systemctl_available() {
  if command -v systemctl >/dev/null 2>&1; then
    if systemctl >/dev/null 2>&1; then
      return 0
    fi
  fi
  return 1
}

run_systemctl() {
  if [ "${DRY_RUN}" = 1 ]; then
    printf 'DRY_RUN: systemctl %s\n' "$*"
    return 0
  fi
  if systemctl_available; then
    run systemctl "$@"
  else
    warn "systemctl not available; skipping 'systemctl $*'"
    return 1
  fi
}

unit_is_active() {
  local unit="$1"
  if [ "${DRY_RUN}" = 1 ]; then
    return 1
  fi
  systemctl is-active --quiet "${unit}" 2>/dev/null || return 1
}

unit_is_enabled() {
  local unit="$1"
  if [ "${DRY_RUN}" = 1 ]; then
    return 1
  fi
  systemctl is-enabled --quiet "${unit}" 2>/dev/null || return 1
}

# ---------------------------------------------------------------------------
# Unit-file helpers
# ---------------------------------------------------------------------------

find_unit_source() {
  local unit="$1"
  local repo_dir=""
  if [ -n "${SCRIPT_DIR}" ]; then
    repo_dir="$(cd "${SCRIPT_DIR}/.." 2>/dev/null && pwd || true)"
  fi
  local c
  for c in "${repo_dir}/dist/systemd/${unit}" \
           "/lib/systemd/system/${unit}" \
           "/usr/lib/systemd/system/${unit}" \
           "/etc/systemd/system/${unit}"; do
    if [ -n "${c}" ] && [ -f "${c}" ]; then
      printf '%s\n' "${c}"
      return 0
    fi
  done
  return 1
}

write_unit() {
  local path="$1" content="$2"
  if [ "${DRY_RUN}" = 1 ]; then
    printf 'DRY_RUN: would write unit %s\n' "${path}"
    return 0
  fi
  run install -d -m 0755 "$(dirname "${path}")"
  printf '%s\n' "${content}" > "${path}"
  run chmod 0644 "${path}"
}

# ---------------------------------------------------------------------------
# Generic file write helpers (dry-run aware)
# ---------------------------------------------------------------------------

ensure_dir() {
  local path="$1" mode="${2:-0755}"
  if [ "${DRY_RUN}" = 1 ]; then
    printf 'DRY_RUN: would create directory %s (mode %s)\n' "${path}" "${mode}"
    return 0
  fi
  run install -d -m "${mode}" "${path}"
}

write_config_file() {
  local path="$1" mode="${2:-0644}"
  if [ "${DRY_RUN}" = 1 ]; then
    printf 'DRY_RUN: would write config %s (mode %s)\n' "${path}" "${mode}"
    return 0
  fi
  run install -d -m 0755 "$(dirname "${path}")"
  run chmod "${mode}" "${path}"
}

# ---------------------------------------------------------------------------
# Firewall helpers
# ---------------------------------------------------------------------------

open_port() {
  local port="$1" proto="${2:-tcp}"
  if [ "${DRY_RUN}" = 1 ]; then
    printf 'DRY_RUN: would open firewall port %s/%s\n' "${port}" "${proto}"
    return 0
  fi
  if command -v firewall-cmd >/dev/null 2>&1; then
    run firewall-cmd --permanent --add-port="${port}/${proto}" 2>/dev/null || true
    run firewall-cmd --reload 2>/dev/null || true
    return 0
  fi
  if command -v ufw >/dev/null 2>&1; then
    run ufw allow "${port}/${proto}" 2>/dev/null || true
    return 0
  fi
  if command -v iptables >/dev/null 2>&1; then
    run iptables -I INPUT -p "${proto}" --dport "${port}" -j ACCEPT 2>/dev/null || true
    return 0
  fi
  warn "no supported firewall tool found; port ${port}/${proto} may be blocked"
}

# ---------------------------------------------------------------------------
# Prompt helpers
# ---------------------------------------------------------------------------

prompt() {
  local var="$1" question="$2" default="${3:-}"
  if [ "${YES}" = 1 ]; then
    if [ -n "${default}" ]; then
      printf -v "${var}" '%s' "${default}"
      return 0
    fi
    die "non-interactive mode requires ${question} to be set via env"
  fi
  local input
  if [ -n "${default}" ]; then
    printf '%s [%s]: ' "${question}" "${default}" >&2
  else
    printf '%s: ' "${question}" >&2
  fi
  read -r input
  if [ -z "${input}" ] && [ -n "${default}" ]; then
    input="${default}"
  fi
  printf -v "${var}" '%s' "${input}"
}

prompt_secret() {
  local var="$1" question="$2"
  if [ "${YES}" = 1 ]; then
    die "non-interactive mode requires ${question} to be set via env or --password-file"
  fi
  local input
  printf '%s: ' "${question}" >&2
  read -rs input
  printf '\n' >&2
  printf -v "${var}" '%s' "${input}"
}

# ---------------------------------------------------------------------------
# Service process helpers (used when systemd is unavailable)
# ---------------------------------------------------------------------------

SERVICE_PIDS=()
register_pid() {
  local pid="$1"
  SERVICE_PIDS+=("${pid}")
}

cleanup_service_pids() {
  local pid
  for pid in "${SERVICE_PIDS[@]}"; do
    kill "${pid}" 2>/dev/null || true
  done
}

# ---------------------------------------------------------------------------
# Coordinator role
# ---------------------------------------------------------------------------

configure_coordinator() {
  local listen_addr="${LISTEN:-${DEFAULT_LISTEN_COORD}}"
  local relay_addr=""
  local relay_listen=""
  log "configuring coordinator role (listen=${listen_addr})"

  ensure_dir "${DATA_DIR}" 0750
  if [ "${DRY_RUN}" = 0 ] && getent group lanlink >/dev/null 2>&1; then
    run chown lanlink:lanlink "${DATA_DIR}"
  fi

  if [ "${WITH_RELAY}" = 1 ]; then
    if [ -n "${RELAY_SERVER}" ]; then
      relay_addr="${RELAY_SERVER}"
    else
      local host
      host="${listen_addr%:*}"
      relay_addr="${host}:3478"
    fi
    relay_listen="${DEFAULT_LISTEN_RELAY}"
    log "coordinator will advertise relay at ${relay_addr}"
  fi

  local unit_src unit_path unit_content coord_bin
  coord_bin="$(command -v lanlink-coordinator 2>/dev/null || printf '/usr/bin/lanlink-coordinator')"
  unit_src="$(find_unit_source lanlink-coordinator.service)" \
    || die "lanlink-coordinator.service template not found"
  unit_path="/etc/systemd/system/lanlink-coordinator.service"
  unit_content="$(sed -e "s|^ExecStart=.*|ExecStart=${coord_bin} --listen ${listen_addr} --db ${DATA_DIR}/coordinator.db|" "${unit_src}")"
  if [ -n "${relay_addr}" ]; then
    unit_content="$(printf '%s\n' "${unit_content}" | sed -e "s|\(--db ${DATA_DIR}/coordinator.db\)|\1 --relay-server ${relay_addr}|")"
  fi
  write_unit "${unit_path}" "${unit_content}"

  run_systemctl daemon-reload

  local was_active=0
  unit_is_active lanlink-coordinator.service && was_active=1
  run_systemctl enable --now lanlink-coordinator.service
  if [ "${was_active}" = 1 ]; then
    log "coordinator unit was already active; restarted after reconfiguration"
  fi

  if [ "${WITH_RELAY}" = 1 ]; then
    configure_relay "${relay_listen}"
  fi

  local coord_port
  coord_port="${listen_addr##*:}"
  open_port "${coord_port}" tcp
  if [ "${WITH_RELAY}" = 1 ]; then
    open_port "${relay_listen##*:}" udp
  fi

  local coord_host
  coord_host="${listen_addr%:*}"
  if [ "${coord_host}" = "0.0.0.0" ]; then
    coord_host="127.0.0.1"
  fi

  cat <<EOF

Coordinator is ready.

  URL:     http://${coord_host}:${coord_port}
  Data:    ${DATA_DIR}/coordinator.db
  Unit:    lanlink-coordinator.service
EOF
  if [ "${WITH_RELAY}" = 1 ]; then
    printf '  Relay:   %s\n' "${relay_addr}"
  fi
  cat <<EOF

Peer join command:
  LANLINK_PASSWORD='your-password' lanlinkd service up <network-name> --coordinator http://${coord_host}:${coord_port}

Or run the install wizard on each peer:
  curl -fsSL https://github.com/${REPO}/releases/download/v${VERSION}/lanlink-install.sh | bash -s -- --role peer --coord-url http://${coord_host}:${coord_port} --yes
EOF
}

# ---------------------------------------------------------------------------
# Relay role
# ---------------------------------------------------------------------------

configure_relay() {
  local listen_addr="${1:-${DEFAULT_LISTEN_RELAY}}"
  log "configuring relay role (listen=${listen_addr})"

  local unit_src unit_path unit_content relay_bin
  relay_bin="$(command -v lanlink-relayd 2>/dev/null || printf '/usr/bin/lanlink-relayd')"
  unit_src="$(find_unit_source lanlink-relayd.service)" \
    || die "lanlink-relayd.service template not found"
  unit_path="/etc/systemd/system/lanlink-relayd.service"
  unit_content="$(sed -e "s|^ExecStart=.*|ExecStart=${relay_bin} --listen ${listen_addr}|" "${unit_src}")"
  write_unit "${unit_path}" "${unit_content}"

  run_systemctl daemon-reload

  local was_active=0
  unit_is_active lanlink-relayd.service && was_active=1
  run_systemctl enable --now lanlink-relayd.service
  if [ "${was_active}" = 1 ]; then
    log "relayd unit was already active; restarted after reconfiguration"
  fi

  local relay_port
  relay_port="${listen_addr##*:}"
  open_port "${relay_port}" udp

  log "relay health check"
  if [ "${DRY_RUN}" = 1 ]; then
    printf 'DRY_RUN: would probe UDP port %s\n' "${relay_port}"
    return 0
  fi

  local host
  host="${listen_addr%:*}"
  if [ "${host}" = "0.0.0.0" ]; then
    host="127.0.0.1"
  fi
  local tries=0
  while [ "${tries}" -lt 10 ]; do
    if timeout 1 bash -c "exec 3<>/dev/udp/${host}/${relay_port}" 2>/dev/null; then
      log "relay UDP port ${host}:${relay_port} is reachable"
      return 0
    fi
    sleep 0.5
    tries=$((tries + 1))
  done
  warn "relay health check could not confirm UDP port ${host}:${relay_port} is listening"
}

# ---------------------------------------------------------------------------
# Peer status socket helpers
# ---------------------------------------------------------------------------

# Discover the daemon status socket for a peer. The socket is named by
# network_id (e.g., /run/lanlink/lanlinkd-1.sock), not by network name.
peer_status_socket() {
  local config_path="$1"
  local network_id=""

  if [ -f "${config_path}" ]; then
    network_id="$(sed -n 's/^[[:space:]]*network_id[[:space:]]*=[[:space:]]*\([0-9][0-9]*\).*/\1/p' "${config_path}" | head -n1)"
  fi

  if [ -n "${network_id}" ] && [ -S "/run/lanlink/lanlinkd-${network_id}.sock" ]; then
    printf '/run/lanlink/lanlinkd-%s.sock\n' "${network_id}"
    return 0
  fi

  # Fallback: any daemon status socket in the run directory.
  find /run/lanlink -maxdepth 1 -type s -name 'lanlinkd-*.sock' 2>/dev/null | head -n1
}

# Query the daemon status socket and return 0 if it replies with a snapshot
# that indicates the network is joined (assigned_ip is present).
peer_status_online() {
  local socket_path="$1"
  local payload='{"cmd":"snapshot"}'
  local reply=""

  if [ ! -S "${socket_path}" ]; then
    return 1
  fi

  if command -v socat >/dev/null 2>&1; then
    reply="$(printf '%s\n' "${payload}" | socat - UNIX-CONNECT:"${socket_path}" 2>/dev/null || true)"
  elif command -v nc >/dev/null 2>&1; then
    reply="$(printf '%s\n' "${payload}" | nc -U "${socket_path}" 2>/dev/null || true)"
  fi

  [ -n "${reply}" ] && printf '%s\n' "${reply}" | grep -q '"assigned_ip"'
}

# ---------------------------------------------------------------------------
# Peer role
# ---------------------------------------------------------------------------

configure_peer() {
  log "configuring peer role"

  if [ -z "${COORD_URL}" ]; then
    prompt COORD_URL "Coordinator URL" "http://localhost:8080"
  fi
  if [ -z "${NETWORK}" ]; then
    prompt NETWORK "Network name" ""
  fi
  if [ -z "${NETWORK}" ]; then
    die "network name is required"
  fi

  local secret_path="/etc/lanlink/${NETWORK}.secret"
  local config_path="/etc/lanlink/${NETWORK}.toml"

  if [ -n "${PASSWORD_FILE}" ]; then
    if [ "${DRY_RUN}" = 0 ]; then
      if [ ! -f "${PASSWORD_FILE}" ]; then
        die "password file not found: ${PASSWORD_FILE}"
      fi
      PASSWORD="$(cat "${PASSWORD_FILE}")"
    fi
  elif [ -z "${PASSWORD}" ]; then
    prompt_secret PASSWORD "Network password"
  fi

  if [ -z "${PASSWORD}" ]; then
    die "network password is required"
  fi

  # In idempotent re-runs, preserve an existing operator-edited config unless
  # --reconfigure was requested.  The secret is always refreshed because it is
  # required for the daemon to start.
  if [ -f "${config_path}" ] && [ "${RECONFIGURE}" = 0 ] && [ "${DRY_RUN}" = 0 ]; then
    log "preserving existing config ${config_path} (use --reconfigure to overwrite)"
  fi

  # Verify GUI binaries are present when a front-end was requested.
  case "${GUI}" in
    tui)  command -v lanlink-tui >/dev/null 2>&1 || warn 'lanlink-tui not found after install' ;;
    web)  command -v lanlink-web >/dev/null 2>&1 || warn 'lanlink-web not found after install' ;;
    desktop) command -v lanlink-desktop >/dev/null 2>&1 || warn 'lanlink-desktop not found after install' ;;
    all)
      command -v lanlink-tui >/dev/null 2>&1 || warn 'lanlink-tui not found after install'
      command -v lanlink-web >/dev/null 2>&1 || warn 'lanlink-web not found after install'
      command -v lanlink-desktop >/dev/null 2>&1 || warn 'lanlink-desktop not found after install'
      ;;
    none|"");;
    *) warn "unknown GUI selector: ${GUI}" ;;
  esac

  if [ "${DRY_RUN}" = 1 ]; then
    printf 'DRY_RUN: would run lanlinkd service up %s --coordinator %s\n' \
      "${NETWORK}" "${COORD_URL}"
    printf 'DRY_RUN: would write secret %s\n' "${secret_path}"
    return 0
  fi

  # Write the secret root-owned 0600 and the config via lanlinkd service up.
  ensure_dir "/etc/lanlink" 0750
  printf '%s\n' "${PASSWORD}" > "${secret_path}"
  run chmod 0600 "${secret_path}"
  if getent group lanlink >/dev/null 2>&1; then
    run chown root:lanlink "${secret_path}" || true
  fi

  LANLINK_PASSWORD="${PASSWORD}" run lanlinkd service up "${NETWORK}" \
    --coordinator "${COORD_URL}"

  # The daemon names the status socket by network_id, not network name.
  # Discover the real socket path and verify the daemon is reachable.
  local status_socket=""
  local deadline tries
  deadline=$(($(date +%s) + 30))
  tries=0
  while [ "$(date +%s)" -lt "${deadline}" ]; do
    status_socket="$(peer_status_socket "${config_path}")"
    if [ -n "${status_socket}" ] && [ -S "${status_socket}" ]; then
      log "peer status socket ready: ${status_socket}"
      break
    fi
    sleep 0.5
    tries=$((tries + 1))
  done

  if [ -n "${status_socket}" ] && [ -S "${status_socket}" ]; then
    if peer_status_online "${status_socket}"; then
      log "peer reports online status"
    else
      warn "peer status socket did not return an online snapshot"
    fi
  else
    warn "peer status socket did not appear"
  fi

  cat <<EOF

Peer service is ready.

  Config:  ${config_path}
  Secret:  ${secret_path}
  Unit:    lanlinkd@${NETWORK}.service
  Status:  ${status_socket:-/run/lanlink/lanlinkd-<network_id>.sock}

Management commands:
  lanlinkd service status
  lanlinkd service stop ${NETWORK}
  lanlinkd service start ${NETWORK}
EOF
}

# ---------------------------------------------------------------------------
# Uninstall
# ---------------------------------------------------------------------------

uninstall_role() {
  local role="${1:-${ROLE:-}}"
  if [ -z "${role}" ]; then
    die "--uninstall requires a role (set --role or LANLINK_ROLE)"
  fi

  if [ "${YES}" = 0 ] && [ "${DRY_RUN}" = 0 ]; then
    local answer
    printf 'Remove %s role-specific configuration? [y/N] ' "${role}" >&2
    read -r answer
    case "${answer}" in
      y|Y) ;;
      *) log "uninstall cancelled"; return 0 ;;
    esac
  fi

  log "uninstalling ${role} role"

  case "${role}" in
    coordinator)
      run_systemctl stop lanlink-coordinator.service || true
      run_systemctl disable lanlink-coordinator.service || true
      if [ "${PURGE}" = 1 ] && [ "${DRY_RUN}" = 0 ]; then
        run rm -f /etc/systemd/system/lanlink-coordinator.service
        run_systemctl daemon-reload || true
      fi
      ;;
    relay)
      run_systemctl stop lanlink-relayd.service || true
      run_systemctl disable lanlink-relayd.service || true
      if [ "${PURGE}" = 1 ] && [ "${DRY_RUN}" = 0 ]; then
        run rm -f /etc/systemd/system/lanlink-relayd.service
        run_systemctl daemon-reload || true
      fi
      ;;
    peer)
      local net="${NETWORK:-default}"
      run_systemctl stop "lanlinkd@${net}.service" || true
      run_systemctl disable "lanlinkd@${net}.service" || true
      if [ "${DRY_RUN}" = 0 ]; then
        run rm -f "/etc/lanlink/${net}.toml"
        run rm -f "/etc/lanlink/${net}.secret"
      else
        printf 'DRY_RUN: would remove /etc/lanlink/%s.toml and /etc/lanlink/%s.secret\n' "${net}" "${net}"
      fi
      ;;
    *)
      die "unknown role for uninstall: ${role}"
      ;;
  esac

  if [ "${PURGE}" = 1 ]; then
    case "${PKG}" in
      deb)
        if command -v apt-get >/dev/null 2>&1; then
          run apt-get remove -y lanlink || true
        else
          run dpkg -r lanlink || true
        fi
        ;;
      rpm)
        if command -v dnf >/dev/null 2>&1; then
          run dnf remove -y lanlink || true
        elif command -v yum >/dev/null 2>&1; then
          run yum remove -y lanlink || true
        else
          run rpm -e lanlink || true
        fi
        ;;
      *) warn "--purge is not implemented for package type ${PKG}" ;;
    esac
  fi

  log "${role} role uninstalled"
}

# ---------------------------------------------------------------------------
# Main flow
# ---------------------------------------------------------------------------

main() {
  VERSION="$(resolve_version)"
  ARCH="$(detect_arch)"
  if [ -z "${PKG}" ]; then
    PKG="$(detect_pkg_type)"
    log "auto-detected package type: ${PKG}"
  fi

  if [ "${UNINSTALL}" = 1 ]; then
    uninstall_role
    return 0
  fi

  if [ -z "${ROLE}" ] && [ "${YES}" = 0 ]; then
    prompt ROLE "Role (coordinator/peer/relay)" ""
  fi
  if [ -z "${ROLE}" ]; then
    die "role is required (set --role or LANLINK_ROLE)"
  fi
  case "${ROLE}" in
    coordinator|peer|relay) ;;
    *) die "unknown role: ${ROLE} (expected coordinator, peer, or relay)" ;;
  esac

  log "installing LanLink ${VERSION} (pkg=${PKG}, arch=${ARCH}, channel=${CHANNEL}, role=${ROLE})"

  install_package "${PKG}" "${VERSION}" "${ARCH}"

  case "${ROLE}" in
    coordinator) configure_coordinator ;;
    peer)        configure_peer ;;
    relay)       configure_relay ;;
  esac

  if [ "${DRY_RUN}" = 1 ]; then
    log "dry-run finished; no changes were made"
  else
    log "installation complete"
  fi
}

# Only run the wizard when this file is executed directly, not when sourced.
if [ "${BASH_SOURCE[0]:-}" = "${0:-}" ]; then
  main "$@"
fi
