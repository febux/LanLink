# Proposal

## Why

The M9 TUI covers terminal users only; a browser-based interface mirrors the same capabilities (live daemon status, peer list, public directory, create/join) and is accessible without SSH or a terminal, reusing the M9 status socket and coordinator REST API with no new control-plane semantics.

## What Changes

- New `lanlink-web` crate: single axum binary with a vanilla-JS SPA embedded via `include_str!` (no Node/npm toolchain), bound to `127.0.0.1:8686` by default (`--bind` override).
- Read-only daemon proxy (`StatusRequest::Snapshot` only) + write-through coordinator operations identical to `lanlinkctl`/`lanlink-tui`; auto-discovers daemon sockets.
- Screens: dashboard, peers, networks (directory + create/join forms), help.

## Capabilities

Implemented as milestone M9+ (see PLAN.md §1.1, together with the Tauri desktop app and `lanlink-gui` launcher); archived historical change, `skip_specs: true`.

### New Capabilities

- None.

### Modified Capabilities

- None.

## Impact

- New `crates/lanlink-web` (main, api, embedded assets); README usage instructions.
