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
#   BIN_DIR      directory with staged binaries (default: stage/ under this dir)
#   WINTUN_DLL   path to a wintun.dll to bundle
#   WINTUN_URL   zip download URL (default wintun 0.14.1)
#   WINTUN_SHA256  optional pinned sha256 of the zip
#   SKIP_BUILD=1 skip stage 1 (useful when binaries are already staged)
#   LL_GUI       Comma-separated: tui, web, desktop, all (default: all)
#   TARGET       Rust target triple (default: x86_64-pc-windows-msvc)
#   DEBUG_NO_AUTOSTART=1
#                build a debug MSI that does NOT auto-start the service
#                (passes -d DebugNoAutostart=1 to wix). Install completes
#                without the 1920 start failure / rollback; start it manually
#                with:  sc start LanLinkDaemon
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
OUT_DIR="${SCRIPT_DIR}/out"
STAGE_DIR="${SCRIPT_DIR}/stage"
TARGET_TRIPLE="${TARGET:-x86_64-pc-windows-msvc}"
WINTUN_URL_DEFAULT="https://www.wintun.net/builds/wintun-0.14.1.zip"

VERSION="${VERSION:-$(awk -F'"' '/^version =/{print $2; exit}' "${REPO_DIR}/Cargo.toml")}"
BIN_DIR="${BIN_DIR:-${STAGE_DIR}}"
WINTUN_URL="${WINTUN_URL:-${WINTUN_URL_DEFAULT}}"
LL_GUI="${LL_GUI:-all}"

log() { printf '[build-msi] %s\n' "$*"; }
die() { printf '[build-msi] ERROR: %s\n' "$*" >&2; exit 1; }

command -v wix >/dev/null 2>&1 \
    || die "WiX CLI not found. Install with: dotnet tool install --global wix"

mkdir -p "${OUT_DIR}" "${STAGE_DIR}"

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

# ---- Stage 1: Windows binaries -------------------------------------------
if [ "${SKIP_BUILD:-0}" != "1" ]; then
    rm -f "${STAGE_DIR}"/*.exe
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*)
            log "building natively on Windows (release)..."
            cargo build --release --target "${TARGET_TRIPLE}" "${CARGO_PACKAGES[@]}"
            ;;
        *)
            if command -v cargo-xwin >/dev/null 2>&1; then
                log "cross-building via cargo-xwin..."
                cargo xwin build --release --target "${TARGET_TRIPLE}" "${CARGO_PACKAGES[@]}"
            elif rustup target list --installed 2>/dev/null | grep -q "${TARGET_TRIPLE}"; then
                log "cross-building with target ${TARGET_TRIPLE} (requires MSVC libs; consider cargo-xwin)..."
                cargo build --release --target "${TARGET_TRIPLE}" "${CARGO_PACKAGES[@]}"
            else
                die "no Windows toolchain found on $(uname -s).
  Options:
    a) install cargo-xwin:  cargo install cargo-xwin
    b) add the target:      rustup target add ${TARGET_TRIPLE}
    c) pre-stage binaries into ${STAGE_DIR} in another job and re-run with SKIP_BUILD=1"
            fi
            ;;
    esac
    # Copy all built binaries into the stage dir.
    for bin in lanlinkd lanlinkctl lanlink-relayd lanlink-coordinator; do
        cp "target/${TARGET_TRIPLE}/release/${bin}.exe" "${STAGE_DIR}/" || die "missing ${bin}.exe"
    done
    for bin in "${GUI_BINS[@]}"; do
        cp "target/${TARGET_TRIPLE}/release/${bin}.exe" "${STAGE_DIR}/" || die "missing ${bin}.exe"
    done
fi

for bin in lanlinkd lanlinkctl; do
    [ -f "${BIN_DIR}/${bin}.exe" ] || die "${BIN_DIR}/${bin}.exe missing (stage binaries or set SKIP_BUILD=0)"
done

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
WIX_EULA_ID="${WIX_EULA_ID:-wix7}"

# Pass WiX variables for optional GUI components.
WIX_GUI_DEFS=()
[ "${WITH_TUI}" = 1 ] && WIX_GUI_DEFS+=(-d "IncludeTui=1")
[ "${WITH_WEB}" = 1 ] && WIX_GUI_DEFS+=(-d "IncludeWeb=1")
[ "${WITH_DESKTOP}" = 1 ] && WIX_GUI_DEFS+=(-d "IncludeDesktop=1")
if [ "${DEBUG_NO_AUTOSTART:-0}" = "1" ]; then
    WIX_GUI_DEFS+=(-d "DebugNoAutostart=1")
    log "DEBUG build: service will NOT auto-start (sc start LanLinkDaemon)"
fi

wix build -arch x64 -acceptEula "${WIX_EULA_ID}" "${SCRIPT_DIR}/lanlink.wxs" \
    -d "Version=${VERSION}" \
    -d "BinDir=${BIN_DIR}" \
    -d "StageDir=${STAGE_DIR}" \
    "${WIX_GUI_DEFS[@]}" \
    -o "${MSI}"

log "done: ${MSI}"
