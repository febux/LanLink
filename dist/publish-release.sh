#!/usr/bin/env bash
# One-command build + publish to GitHub Releases.
#
# Usage:
#   bash dist/publish-release.sh [VERSION]
#   make dist-publish VERSION=0.2.0
#
# Options:
#   --dry-run          Build everything but do not create the release or upload.
#   --skip-build       Use already-built artifacts in dist/*/out/.
#   --skip-tag         Do not create or push a git tag.
#   --platforms LIST   Comma-separated: deb,rpm,pkg,msi (default: auto-detect).
#   --notes TEXT       Release notes (default: generated from version).
#   --notes-file FILE  Read release notes from file.
#   --prerelease       Mark the release as a prerelease.
#
# Environment:
#   VERSION            Override version (also first positional arg).
#   LL_GUI             Build GUI front-ends: tui,web,desktop,all.
#   GH_REPO            Override repo (default: detected from git origin).
#   GITHUB_TOKEN       Required by gh CLI in non-interactive environments.
#
# Requires: git, gh CLI. Platform-specific builders are used when available:
#   .deb  -> dpkg-deb on Linux
#   .rpm  -> rpmbuild or docker/podman on Linux
#   .pkg  -> pkgbuild on macOS
#   .msi  -> cargo-xwin + WiX on Linux/macOS, or native on Windows
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_DIR}"

DRY_RUN=0
SKIP_BUILD=0
SKIP_TAG=0
PRERELEASE=0
PLATFORMS=""
NOTES=""
NOTES_FILE=""
VERSION="${VERSION:-}"

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --skip-build)
      SKIP_BUILD=1
      shift
      ;;
    --skip-tag)
      SKIP_TAG=1
      shift
      ;;
    --platforms)
      PLATFORMS="$2"
      shift 2
      ;;
    --notes)
      NOTES="$2"
      shift 2
      ;;
    --notes-file)
      NOTES_FILE="$2"
      shift 2
      ;;
    --prerelease)
      PRERELEASE=1
      shift
      ;;
    --help|-h)
      sed -n '2,35p' "$0" | sed -E 's/^# ?//'
      exit 0
      ;;
    -*)
      echo "Unknown option: $1" >&2
      echo "Usage: $0 [--dry-run] [--skip-build] [--skip-tag] [--platforms deb,rpm,pkg,msi] [--notes TEXT|--notes-file FILE] [VERSION]" >&2
      exit 1
      ;;
    *)
      if [ -z "${VERSION}" ]; then
        VERSION="$1"
      else
        echo "Version already set to ${VERSION}, ignoring $1" >&2
      fi
      shift
      ;;
  esac
done

# Resolve version from Cargo.toml if not provided.
if [ -z "${VERSION}" ]; then
  VERSION="$(awk -F'"' '/^version =/{print $2; exit}' Cargo.toml)"
fi
if [ -z "${VERSION}" ]; then
  echo "ERROR: could not determine version from Cargo.toml" >&2
  exit 1
fi

echo "[publish] version: ${VERSION}"

TAG="v${VERSION}"
ARTIFACTS=()

# Auto-detect platforms when not explicitly requested.
if [ -z "${PLATFORMS}" ]; then
  case "$(uname -s)" in
    Linux)
      PLATFORMS="deb,msi"
      if command -v rpmbuild >/dev/null 2>&1 || command -v docker >/dev/null 2>&1 || command -v podman >/dev/null 2>&1; then
        PLATFORMS="deb,rpm,msi"
      fi
      ;;
    Darwin)
      PLATFORMS="pkg"
      ;;
    MINGW*|MSYS*|CYGWIN*)
      PLATFORMS="msi"
      ;;
    *)
      PLATFORMS=""
      ;;
  esac
  echo "[publish] auto-detected platforms: ${PLATFORMS:-none}"
else
  echo "[publish] requested platforms: ${PLATFORMS}"
fi

has_platform() {
  case ",${PLATFORMS}," in
    *,"$1",*) return 0 ;;
  esac
  return 1
}

build_deb() {
  if ! command -v dpkg-deb >/dev/null 2>&1; then
    echo "[publish] SKIP .deb: dpkg-deb not found (install on Debian/Ubuntu)"
    return 0
  fi
  echo "[publish] building .deb package"
  bash "${SCRIPT_DIR}/deb/build-deb.sh"
  for f in "${SCRIPT_DIR}/deb/out"/*.deb; do
    if [ -f "$f" ]; then
      ARTIFACTS+=("$f")
    fi
  done
}

build_rpm() {
  if [ -f "${SCRIPT_DIR}/rpm/out/lanlink-${VERSION}-1.*.rpm" ]; then
    for f in "${SCRIPT_DIR}/rpm/out"/lanlink-${VERSION}-1.*.rpm; do
      [ -f "$f" ] && ARTIFACTS+=("$f")
    done
    return 0
  fi
  if ! command -v rpmbuild >/dev/null 2>&1 && ! command -v docker >/dev/null 2>&1 && ! command -v podman >/dev/null 2>&1; then
    echo "[publish] SKIP .rpm: rpmbuild/docker/podman not found"
    return 0
  fi
  echo "[publish] building .rpm package"
  bash "${SCRIPT_DIR}/rpm/build-rpm.sh"
  for f in "${SCRIPT_DIR}/rpm/out"/*.rpm; do
    if [ -f "$f" ]; then
      ARTIFACTS+=("$f")
    fi
  done
}

build_pkg() {
  if ! command -v pkgbuild >/dev/null 2>&1; then
    echo "[publish] SKIP .pkg: pkgbuild not found (run on macOS)"
    return 0
  fi
  echo "[publish] building macOS .pkg"
  bash "${SCRIPT_DIR}/macos/build-pkg.sh"
  for f in "${SCRIPT_DIR}/macos/out"/*.pkg; do
    if [ -f "$f" ]; then
      ARTIFACTS+=("$f")
    fi
  done
}

build_msi() {
  if ! command -v wix >/dev/null 2>&1; then
    echo "[publish] SKIP .msi: wix not found (install: dotnet tool install --global wix)"
    return 0
  fi
  echo "[publish] building Windows MSI"
  bash "${SCRIPT_DIR}/windows/build-msi.sh"
  for f in "${SCRIPT_DIR}/windows/out"/*.msi; do
    if [ -f "$f" ]; then
      ARTIFACTS+=("$f")
    fi
  done
}

collect_existing() {
  local dirs=("${SCRIPT_DIR}/deb/out" "${SCRIPT_DIR}/rpm/out" "${SCRIPT_DIR}/macos/out" "${SCRIPT_DIR}/windows/out")
  for d in "${dirs[@]}"; do
    for f in "$d"/*.{deb,rpm,pkg,msi}; do
      [ -f "$f" ] && ARTIFACTS+=("$f")
    done
  done
}

if [ "$SKIP_BUILD" = 1 ]; then
  echo "[publish] skipping builds, collecting existing artifacts"
  collect_existing
else
  has_platform deb && build_deb
  has_platform rpm && build_rpm
  has_platform pkg && build_pkg
  has_platform msi && build_msi
fi

if [ "${#ARTIFACTS[@]}" -eq 0 ]; then
  echo "[publish] ERROR: no artifacts found or built" >&2
  exit 1
fi

echo "[publish] artifacts:"
for f in "${ARTIFACTS[@]}"; do
  echo "  - $f"
done

if [ "$DRY_RUN" = 1 ]; then
  echo "[publish] DRY RUN: would create release ${TAG} and upload ${#ARTIFACTS[@]} asset(s)"
  exit 0
fi

# Verify gh CLI.
if ! command -v gh >/dev/null 2>&1; then
  echo "[publish] ERROR: gh CLI not found. Install: https://cli.github.com/" >&2
  exit 1
fi

# Create and push tag unless skipped.
if [ "$SKIP_TAG" = 0 ]; then
  if git rev-parse "${TAG}" >/dev/null 2>&1; then
    echo "[publish] tag ${TAG} already exists"
  else
    echo "[publish] creating tag ${TAG}"
    git tag -a "${TAG}" -m "Release ${VERSION}"
  fi
  echo "[publish] pushing tag ${TAG}"
  git push origin "${TAG}"
fi

# Build gh release-create args as an array so spaces in notes are preserved.
RELEASE_ARGS=("${TAG}" --title "LanLink ${VERSION}")
[ "$PRERELEASE" = 1 ] && RELEASE_ARGS+=(--prerelease)
if [ -n "${NOTES_FILE}" ]; then
  RELEASE_ARGS+=(--notes-file "${NOTES_FILE}")
elif [ -n "${NOTES}" ]; then
  RELEASE_ARGS+=(--notes "${NOTES}")
else
  RELEASE_ARGS+=(--generate-notes)
fi

# Create the release if it does not exist, then upload artifacts.
echo "[publish] creating GitHub release ${TAG}"
if ! gh release view "${TAG}" >/dev/null 2>&1; then
  gh release create "${RELEASE_ARGS[@]}"
fi

for f in "${ARTIFACTS[@]}"; do
  echo "[publish] uploading $(basename "$f")"
  gh release upload "${TAG}" "$f" --clobber
done

echo "[publish] done: https://github.com/${GH_REPO:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}/releases/tag/${TAG}"
