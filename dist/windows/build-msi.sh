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
#   VCREDIST_EXE   path to a pre-downloaded VC++ redist (x64) to bundle
#   VCREDIST_URL   download URL (default: aka.ms VC++ 2022 redist x64)
#   VCREDIST_SHA256  optional pinned sha256 of the exe; pin it in release CI
#   SKIP_VCREDIST=1  build without the bundled VC++ redist
#   SKIP_BUILD=1 skip stage 1 (useful when binaries are already staged)
#   LL_GUI       Comma-separated: tui, web, desktop, all (default: all)
#   TARGET       Rust target triple (default: x86_64-pc-windows-msvc)
#   DEBUG_NO_AUTOSTART=1
#                Deprecated no-op. The MSI no longer starts the service during
#                install (this was causing error 1920 / rollback on clean
#                machines). Kept for backward compatibility with old CI scripts.
#   WIX_DOCKER_IMAGE  Docker image used when native WiX is unavailable or
#                     unsupported on this platform (default: jkroepke/wixtoolset).
#   WIX_CONTAINER_RUNTIME  `docker` or `podman` (default: auto-detect).
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

# Determine the container runtime, if any.
CONTAINER_RUNTIME="${WIX_CONTAINER_RUNTIME:-}"
if [ -z "${CONTAINER_RUNTIME}" ]; then
    if command -v docker >/dev/null 2>&1; then
        CONTAINER_RUNTIME=docker
    elif command -v podman >/dev/null 2>&1; then
        CONTAINER_RUNTIME=podman
    fi
fi

# Determine how to invoke WiX. On Linux the .NET WiX CLI installs but is
# unsupported and emits undefined behavior; fall back to a container image.
WIX_DOCKER_IMAGE="${WIX_DOCKER_IMAGE:-jkroepke/wixtoolset}"
WIX_USE_DOCKER=0
if command -v wix >/dev/null 2>&1; then
    if wix --version 2>&1 | grep -q "only supports Windows"; then
        if [ -n "${CONTAINER_RUNTIME}" ]; then
            log "native WiX reports it only supports Windows; using ${CONTAINER_RUNTIME} image ${WIX_DOCKER_IMAGE}"
            WIX_USE_DOCKER=1
        else
            die "native WiX only supports Windows and no container runtime was found. Install Docker or run this on Windows."
        fi
    else
        log "using native WiX CLI"
    fi
else
    if [ -n "${CONTAINER_RUNTIME}" ]; then
        log "WiX CLI not found; using ${CONTAINER_RUNTIME} image ${WIX_DOCKER_IMAGE}"
        WIX_USE_DOCKER=1
    else
        die "WiX CLI not found and no container runtime was found. Install one of:
  - Windows:  dotnet tool install --global wix
  - Linux:    Docker or Podman, then re-run this script"
    fi
fi

run_wix() {
    if [ "${WIX_USE_DOCKER}" = 1 ]; then
        "${CONTAINER_RUNTIME}" run --rm -v "${REPO_DIR}:/src" -w /src "${WIX_DOCKER_IMAGE}" wix "$@"
    else
        wix "$@"
    fi
}

# The -acceptEula switch is required by WiX 7+ but rejected by WiX 5/6.
WIX_EULA_ID="${WIX_EULA_ID:-wix7}"
WIX_VERSION=$(run_wix --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -n1 || true)
WIX_MAJOR=$(echo "${WIX_VERSION:-0}" | cut -d. -f1)
WIX_ACCEPT_EULA=()
if [ "${WIX_MAJOR:-0}" -ge 7 ] 2>/dev/null && [ -n "${WIX_EULA_ID}" ]; then
    WIX_ACCEPT_EULA=(-acceptEula "${WIX_EULA_ID}")
fi
log "WiX version detected: ${WIX_VERSION:-unknown}"

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

# ---- Stage 2b: VC++ redistributable (optional, bundled by default) ---------
VCREDIST_URL="${VCREDIST_URL:-https://aka.ms/vs/17/release/vc_redist.x64.exe}"
WITH_VCREDIST=1
if [ "${SKIP_VCREDIST:-0}" = "1" ]; then
    WITH_VCREDIST=0
    log "SKIP_VCREDIST=1: building without the bundled VC++ redistributable"
elif [ -n "${VCREDIST_EXE:-}" ]; then
    log "using VCREDIST_EXE=${VCREDIST_EXE}"
    cp "${VCREDIST_EXE}" "${STAGE_DIR}/vc_redist.x64.exe"
else
    log "downloading ${VCREDIST_URL}..."
    curl -fsSL -o "${STAGE_DIR}/vc_redist.x64.exe" "${VCREDIST_URL}"
fi
if [ "${WITH_VCREDIST}" = 1 ]; then
    [ -f "${STAGE_DIR}/vc_redist.x64.exe" ] || die "vc_redist.x64.exe staging failed"
    if [ -n "${VCREDIST_SHA256:-}" ]; then
        echo "${VCREDIST_SHA256}  ${STAGE_DIR}/vc_redist.x64.exe" | sha256sum -c - \
            || die "vc_redist sha256 mismatch (expected ${VCREDIST_SHA256})"
    else
        log "WARNING: VCREDIST_SHA256 not set; redist not pinned (sha256: $(sha256sum "${STAGE_DIR}/vc_redist.x64.exe" | cut -d' ' -f1))"
    fi
fi

# ---- Stage 3: MSI ----------------------------------------------------------
MSI="${OUT_DIR}/lanlink-${VERSION}-x64.msi"
log "building ${MSI} (version ${VERSION})..."

# Pass WiX variables for optional GUI components.
WIX_GUI_DEFS=()
[ "${WITH_TUI}" = 1 ] && WIX_GUI_DEFS+=(-d "IncludeTui=1")
[ "${WITH_WEB}" = 1 ] && WIX_GUI_DEFS+=(-d "IncludeWeb=1")
[ "${WITH_DESKTOP}" = 1 ] && WIX_GUI_DEFS+=(-d "IncludeDesktop=1")
[ "${WITH_VCREDIST}" = 1 ] && WIX_GUI_DEFS+=(-d "VCRedistExe=1")
# DEBUG_NO_AUTOSTART is deprecated; the MSI now never starts the service
# during install. Keep the variable check so old CI scripts do not break.
if [ "${DEBUG_NO_AUTOSTART:-0}" = "1" ]; then
    log "DEBUG_NO_AUTOSTART is deprecated; the service is no longer started during install"
fi

# WiX invocation paths. When using the Docker path the container mounts the
# repo at /src and runs from there, so we pass paths relative to REPO_DIR.
WIX_WXS_REL=$(realpath --relative-to="${REPO_DIR}" "${SCRIPT_DIR}/lanlink.wxs")
WIX_BIN_DIR_REL=$(realpath --relative-to="${REPO_DIR}" "${BIN_DIR}")
WIX_STAGE_DIR_REL=$(realpath --relative-to="${REPO_DIR}" "${STAGE_DIR}")
WIX_MSI_REL=$(realpath --relative-to="${REPO_DIR}" "${MSI}")

if [ "${WIX_USE_DOCKER}" = 1 ]; then
    for p in "${WIX_WXS_REL}" "${WIX_BIN_DIR_REL}" "${WIX_STAGE_DIR_REL}" "${WIX_MSI_REL}"; do
        case "$p" in
            ../*) die "Docker build requires BinDir/StageDir/output to be inside the repo root (got ${p})" ;;
        esac
    done
fi

(
    cd "${REPO_DIR}"
    run_wix build -arch x64 "${WIX_ACCEPT_EULA[@]}" "${WIX_WXS_REL}" \
        -d "Version=${VERSION}" \
        -d "BinDir=${WIX_BIN_DIR_REL}" \
        -d "StageDir=${WIX_STAGE_DIR_REL}" \
        "${WIX_GUI_DEFS[@]}" \
        -o "${WIX_MSI_REL}"
)

log "done: ${MSI}"
