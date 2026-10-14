# ---------------------------------------------------------------------------
# LanLink workspace Makefile
# ---------------------------------------------------------------------------
# Usage:
#   make <target> [<VAR>=<value> ...]
#
# Common variables:
#   VERSION      Workspace version override (default: read from Cargo.toml)
#   MODE         GUI mode for `make gui` (tui, web, desktop; default: tui)
#   PUBLISH_OPTS Extra options for dist-publish / dist-publish-ci
#
# Quick reference:
#   make check / test / fmt / clippy     Development quality checks
#   make run-daemon / run-coordinator      Run binaries in help mode
#   make gui MODE=web                      Start GUI launcher
#   make build-deb / build-rpm / ...       Build platform packages
#   make build-tarball                     Build portable Linux tarball
#   make build-appimage                    Build Tauri AppImage
#   make build-flatpak                     Build Flatpak bundle
#   make build-snap                        Build Snap package
#   make dist                              Build all packages for host
#   make dist-publish-ci                   Push a release tag (CI will publish)
#   make bump-version VERSION=0.3.0        Bump workspace version
# ---------------------------------------------------------------------------

.PHONY: help
help: ## Show this help message
	@grep -E '^[a-zA-Z0-9_-]+:.*## ' $(MAKEFILE_LIST) | \
	  awk 'BEGIN {FS = ":.*## "}; {printf "  %-20s %s\n", $$1, $$2}'

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

MODE ?= tui

# Default version is read from the workspace package version in Cargo.toml.
VERSION ?= $(shell awk -F'"' '/^version =/{print $$2; exit}' Cargo.toml)

# Paths to builder scripts.
BUILD_DEB      := dist/deb/build-deb.sh
BUILD_RPM      := dist/rpm/build-rpm.sh
BUILD_MSI      := dist/windows/build-msi.sh
BUILD_PKG      := dist/macos/build-pkg.sh
BUILD_TARBALL  := dist/tarball/build-tarball.sh
BUILD_APPIMAGE := dist/appimage/build-appimage.sh
BUILD_FLATPAK  := dist/flatpak/build-flatpak.sh
BUILD_SNAP     := dist/snap/snapcraft.yaml
PUBLISH        := dist/publish-release.sh
GUI_LAUNCH     := scripts/gui-start.sh

# ---------------------------------------------------------------------------
# Group: Development / quality
# ---------------------------------------------------------------------------

.PHONY: check test fmt clippy clean

check: ## Run cargo check on the workspace
	cargo check --workspace

test: ## Run tests on the workspace
	cargo test --workspace

fmt: ## Format all Rust code
	cargo fmt --all

clippy: ## Run Clippy with warnings-as-errors
	cargo clippy --workspace --all-targets -- -D warnings

clean: ## Clean build artifacts
	cargo clean

# ---------------------------------------------------------------------------
# Group: Runners
# ---------------------------------------------------------------------------

.PHONY: run run-daemon run-coordinator run-tui run-web run-desktop gui dev web

run: run-daemon ## Default runner alias (daemon help)

run-daemon: ## Run lanlinkd --help
	cargo run -p lanlinkd -- --help

run-coordinator: ## Run coordinator --help
	cargo run -p coordinator -- --help

run-tui: ## Start TUI launcher
	bash $(GUI_LAUNCH) --mode tui

run-web: ## Start web GUI launcher
	bash $(GUI_LAUNCH) --mode web

run-desktop: ## Start native desktop (Tauri) launcher
	bash $(GUI_LAUNCH) --mode desktop

gui: ## Start GUI launcher (MODE=tui|web|desktop)
	bash $(GUI_LAUNCH) --mode $(MODE)

# Legacy aliases.
dev: run-tui ## Legacy alias for TUI launcher
web: run-web ## Legacy alias for web launcher

# ---------------------------------------------------------------------------
# Group: Builders
# ---------------------------------------------------------------------------

.PHONY: build build-deb build-rpm build-msi build-pkg build-tarball build-appimage build-flatpak build-snap dist

build: check ## Run buildable checks (alias for cargo check)

build-deb: ## Build Debian package
	bash $(BUILD_DEB)

build-rpm: ## Build RPM package
	bash $(BUILD_RPM)

build-msi: ## Build Windows MSI package
	bash $(BUILD_MSI)

build-pkg: ## Build macOS pkg installer
	bash $(BUILD_PKG)

build-tarball: ## Build portable Linux tarball
	bash $(BUILD_TARBALL)

build-appimage: ## Build Tauri AppImage
	bash $(BUILD_APPIMAGE)

build-flatpak: ## Build Flatpak bundle
	bash $(BUILD_FLATPAK)

build-snap: ## Build Snap package
	snapcraft pack dist/snap --use-lxd

dist: ## Build all distribution packages available on this host
	$(MAKE) build-deb || true
	$(MAKE) build-rpm || true
	$(MAKE) build-msi || true
	$(MAKE) build-pkg || true
	$(MAKE) build-tarball || true
	$(MAKE) build-appimage || true
	$(MAKE) build-flatpak || true
	$(MAKE) build-snap || true

# ---------------------------------------------------------------------------
# Group: Publishers
# ---------------------------------------------------------------------------

.PHONY: dist-publish dist-publish-ci dist-clean bump-version

dist-publish: ## Build and publish a GitHub release (VERSION required for safety)
	bash $(PUBLISH) $(VERSION) $(PUBLISH_OPTS)

dist-publish-ci: ## Push release tag only; CI will build and publish
	CI_PUBLISH=1 bash $(PUBLISH) $(VERSION) $(PUBLISH_OPTS)

dist-clean: ## Remove distribution staging/output directories
	rm -rf dist/deb/root dist/deb/out dist/macos/root dist/macos/out \
	       dist/windows/out dist/windows/stage dist/rpm/out

bump-version: ## Bump workspace version, e.g. make bump-version VERSION=0.3.0
ifndef VERSION
	$(error VERSION is required (e.g., make bump-version VERSION=0.3.0))
endif
	@echo "Bumping workspace version to $(VERSION)"
	sed -i.bak -E '/^\[workspace\.package\]/,/^\[/ s/^version = "[^"]*"/version = "$(VERSION)"/' Cargo.toml
	rm -f Cargo.toml.bak
	cargo update --workspace
