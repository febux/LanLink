.PHONY: check test fmt clippy clean run-daemon run-coordinator gui dev web dist-deb dist-rpm dist-msi dist-pkg dist-publish dist-clean

# Default GUI mode for `make gui` (tui, web, or desktop).
MODE ?= tui

# Version used by distribution builds (default: read from Cargo.toml).
VERSION ?= $(shell awk -F'"' '/^version =/{print $$2; exit}' Cargo.toml)

check:
	cargo check --workspace

test:
	cargo test --workspace

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

clean:
	cargo clean

run-daemon:
	cargo run -p lanlinkd -- --help

run-coordinator:
	cargo run -p coordinator -- --help

gui:
	bash scripts/gui-start.sh --mode $(MODE)

# Legacy aliases, now mapped to the unified launcher.
dev:
	bash scripts/gui-start.sh --mode tui

web:
	bash scripts/gui-start.sh --mode web

dist-deb:
	bash dist/deb/build-deb.sh

dist-rpm:
	bash dist/rpm/build-rpm.sh

dist-msi:
	bash dist/windows/build-msi.sh

dist-pkg:
	bash dist/macos/build-pkg.sh

dist-publish:
	bash dist/publish-release.sh $(VERSION) $(PUBLISH_OPTS)

dist-publish-ci:
	CI_PUBLISH=1 bash dist/publish-release.sh $(VERSION) $(PUBLISH_OPTS)

dist-clean:
	rm -rf dist/deb/root dist/deb/out dist/macos/root dist/macos/out dist/windows/out dist/windows/stage dist/rpm/out
