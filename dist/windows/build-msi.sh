#!/usr/bin/env bash
# Build the LanLink Windows MSI.
#
# Produces: dist/windows/out/lanlink-<version>-x64.msi
#
# The script has three stages, each overridable via environment variables:
#
#   1. Stage Windows binaries:
#        - On Windows (MSYS/Git-Bash): plain `cargo build --release`.
#        - On Linux/macOS: `cargo xwin build` (cargo-xwin) if available, else
#          plain cross `cargo build --target x86_64-pc-windows-msvc`, else
#          reuse a pre-staged $BIN_DIR (CI may build binaries in another job).
#   2. Stage wintun.dll:
#        - $WINTUN_DLL if set, else download $WINTUN_URL (default: the official
#          0.14.1 build from wintun.net) and extract amd64/wintun.dll.
#          When $WINTUN_SHA256 is set the zip is verified against it and the
#          build fails on mismatch; pin it in release CI.
#   3. `wix build` the MSI (WiX v4+ CLI; `dotnet tool install --global wix`).
#
# Environment variables:
#   VERSION      package version (default: workspace version from Cargo.toml)
#   BIN_DIR      directory with lanlinkd.exe / lanlinkctl.exe (default staged here)
#   WINTUN_DLL   path to a wintun.dll to bundle
#   WINTUN_URL   zip download URL (default wintun 0.14.1)
#   WINTUN_SHA256  optional pinned sha256 of the zip
#   SKIP_BUILD=1 skip stage 1 (useful when binaries are already staged)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
OUT_DIR="${SCRIPT_DIR}/out"
STAGE_DIR="${SCRIPT_DIR}/stage"
TARGET_TRIPLE="x86_64-pc-windows-msvc"
WINTUN_URL_DEFAULT="https://www.wintun.net/builds/wintun-0.14.1.zip"

VERSION="${VERSION:-$(awk -F'"' '/^version =/{print $2; exit}' "${REPO_DIR}/Cargo.toml")}"
BIN_DIR="${BIN_DIR:-${STAGE_DIR}}"
WINTUN_URL="${WINTUN_URL:-${WINTUN_URL_DEFAULT}}"

log() { printf '[build-msi] %s\n' "$*"; }
die() { printf '[build-msi] ERROR: %s\n' "$*" >&2; exit 1; }

command -v wix >/dev/null 2>&1 \
    || die "WiX CLI not found. Install with: dotnet tool install --global wix"

mkdir -p "${OUT_DIR}" "${STAGE_DIR}"

# ---- Stage 1: Windows binaries -------------------------------------------
if [ "${SKIP_BUILD:-0}" != "1" ]; then
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*)
            log "building natively on Windows (release)..."
            cargo build --release -p lanlinkd -p lanlinkctl
            cp "target/release/lanlinkd.exe" "target/release/lanlinkctl.exe" "${STAGE_DIR}/"
            ;;
        *)
            if command -v cargo-xwin >/dev/null 2>&1; then
                log "cross-building via cargo-xwin..."
                cargo xwin build --release --target "${TARGET_TRIPLE}" -p lanlinkd -p lanlinkctl
                cp "target/${TARGET_TRIPLE}/release/lanlinkd.exe" \
                   "target/${TARGET_TRIPLE}/release/lanlinkctl.exe" "${STAGE_DIR}/"
            elif rustup target list --installed 2>/dev/null | grep -q "${TARGET_TRIPLE}"; then
                log "cross-building with target ${TARGET_TRIPLE} (requires MSVC libs; consider cargo-xwin)..."
                cargo build --release --target "${TARGET_TRIPLE}" -p lanlinkd -p lanlinkctl
                cp "target/${TARGET_TRIPLE}/release/lanlinkd.exe" \
                   "target/${TARGET_TRIPLE}/release/lanlinkctl.exe" "${STAGE_DIR}/"
            else
                die "no Windows toolchain found on $(uname -s).
  Options:
    a) install cargo-xwin:  cargo install cargo-xwin
    b) add the target:      rustup target add ${TARGET_TRIPLE}
    c) pre-stage binaries into ${STAGE_DIR} in another job and re-run with SKIP_BUILD=1"
            fi
            ;;
    esac
fi

[ -f "${BIN_DIR}/lanlinkd.exe" ]  || die "${BIN_DIR}/lanlinkd.exe missing (stage binaries or set SKIP_BUILD=0)"
[ -f "${BIN_DIR}/lanlinkctl.exe" ] || die "${BIN_DIR}/lanlinkctl.exe missing"

# ---- Stage 2: wintun.dll ---------------------------------------------------
if [ -n "${WINTUN_DLL:-}" ]; then
    log "using WINTUN_DLL=${WINTUN_DLL}"
    cp "${WINTUN_DLL}" "${STAGE_DIR}/wintun.dll"
else
    log "downloading ${WINTUN_URL}..."
    zip_path="${STAGE_DIR}/wintun.zip"
    curl -fsSL -o "${zip_path}" "${WINTUN_URL}"
    if [ -n "${WINTUN_SHA256:-}" ]; then
        echo "${WINTUN_SHA256}  ${zip_path}" | sha256sum -c - \
            || die "wintun zip sha256 mismatch (expected ${WINTUN_SHA256})"
    else
        log "WARNING: WINTUN_SHA256 not set; zip not pinned (sha256: $(sha256sum "${zip_path}" | cut -d' ' -f1))"
    fi
    unzip -o -j "${zip_path}" "wintun/bin/amd64/wintun.dll" -d "${STAGE_DIR}" >/dev/null
    rm -f "${zip_path}"
fi
[ -f "${STAGE_DIR}/wintun.dll" ] || die "wintun.dll staging failed"

# ---- Stage 3: MSI ----------------------------------------------------------
MSI="${OUT_DIR}/lanlink-${VERSION}-x64.msi"
log "building ${MSI} (version ${VERSION})..."
wix build -arch x64 "${SCRIPT_DIR}/lanlink.wxs" \
    -d "Version=${VERSION}" \
    -d "BinDir=${STAGE_DIR}" \
    -o "${MSI}"

log "done: ${MSI}"
