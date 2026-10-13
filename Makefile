.PHONY: check test fmt clippy clean run-daemon run-coordinator gui dev web dist-deb dist-msi dist-clean

# Default GUI mode for `make gui` (tui, web, or desktop).
MODE ?= tui

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

dist-msi:
	bash dist/windows/build-msi.sh

dist-clean:
	rm -rf dist/deb/root dist/deb/out dist/macos/root dist/macos/out dist/windows/out dist/windows/stage
