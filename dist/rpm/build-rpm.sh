#!/usr/bin/env bash
# Build a LanLink .rpm package.
#
# Usage: dist/rpm/build-rpm.sh [output-dir]
#
# The script tries, in order:
#   1. Native rpmbuild on Fedora/RHEL/openSUSE.
#   2. Docker or Podman with the official Fedora image.
#   3. Manual instructions if neither is available.
#
# Environment:
#   LANLINK_VERSION  package version (default: workspace version)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_DIR}"

VERSION="${LANLINK_VERSION:-$(awk -F'"' '/^version =/{print $2; exit}' Cargo.toml)}"
VERSION="${VERSION:-0.1.0}"
OUT="${1:-dist/rpm/out}"
SPEC_DIR="${SCRIPT_DIR}"

mkdir -p "${OUT}"

build_native() {
    echo "[rpm] building with native rpmbuild"
    local topdir
    topdir="$(cd "${OUT}" && pwd)/rpmbuild"
    mkdir -p "${topdir}"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
    tar czf "${topdir}/SOURCES/lanlink-${VERSION}.tar.gz" \
        --transform "s,^$(basename "${REPO_DIR}"),lanlink," \
        --exclude='.git' --exclude='target' --exclude='dist/*/root' --exclude='dist/*/out' \
        -C "${REPO_DIR}/.." "$(basename "${REPO_DIR}")"
    sed -E "s/^Version:[[:space:]]+[^[:space:]]+/Version: ${VERSION}/" \
        "${SPEC_DIR}/lanlink.spec" > "${topdir}/SPECS/lanlink.spec"
    rpmbuild --define "_topdir ${topdir}" -ba "${topdir}/SPECS/lanlink.spec"
    find "${topdir}/RPMS" -name "lanlink-*.rpm" \
        ! -name "*-debuginfo-*" ! -name "*-debugsource-*" \
        -exec cp {} "${OUT}/" \;
}

build_container() {
    local engine="$1"
    echo "[rpm] building in ${engine} (Fedora)"
    local img="fedora:latest"
    local abs_repo_dir abs_out_dir
    abs_repo_dir="$(cd "${REPO_DIR}" && pwd)"
    abs_out_dir="$(cd "${OUT}" && pwd)"
    "${engine}" run --rm \
        -v "${abs_repo_dir}:/src:ro" \
        -v "${abs_out_dir}:/out" \
        -e "LANLINK_VERSION=${VERSION}" \
        "${img}" bash -c '
            dnf install -y rpm-build cargo git nftables iproute systemd-rpm-macros
            mkdir -p /root/rpmbuild/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
            tar czf /root/rpmbuild/SOURCES/lanlink-${LANLINK_VERSION}.tar.gz \
                --transform "s,^$(basename /src),lanlink," \
                --exclude=.git --exclude=target --exclude="dist/*/root" --exclude="dist/*/out" \
                -C /src/.. $(basename /src)
            sed -E "s/^Version:[[:space:]]+[^[:space:]]+/Version: ${LANLINK_VERSION}/" \
                /src/dist/rpm/lanlink.spec > /root/rpmbuild/SPECS/lanlink.spec
            rpmbuild --define "_topdir /root/rpmbuild" -ba /root/rpmbuild/SPECS/lanlink.spec
            find /root/rpmbuild/RPMS -name "lanlink-*.rpm" \
                ! -name "*-debuginfo-*" ! -name "*-debugsource-*" \
                -exec cp {} /out/ \;
        '
}

if command -v rpmbuild >/dev/null 2>&1; then
    build_native
elif command -v docker >/dev/null 2>&1; then
    build_container docker
elif command -v podman >/dev/null 2>&1; then
    build_container podman
else
    echo "[rpm] ERROR: rpmbuild, docker, or podman is required to build the .rpm" >&2
    echo "[rpm] On Fedora: sudo dnf install rpm-build" >&2
    echo "[rpm] Or install Docker/Podman and re-run." >&2
    exit 1
fi

echo "[rpm] wrote ${OUT}/lanlink-${VERSION}-1.*.rpm"
