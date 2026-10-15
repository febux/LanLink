# Proposal

## Why

The repository working tree currently carries two uncommitted hygiene changes: local AI-agent tool directories (`.cursor/`, `.hermes/skills/`, `.omp/`, `.opencode/`, `.qwen/`, `.zcode/`) are being created by developer tooling and show up as untracked noise in `git status`, and the project has just adopted OpenSpec with an `openspec/` scaffold but no process in place to formalize these kinds of changes. This change records both in a reviewable way so they can be committed deliberately instead of lingering in the working tree.

## What Changes

- Add six agent-tool ignore patterns to `.gitignore`: `.cursor/`, `.hermes/skills/`, `.omp/`, `.opencode/`, `.qwen/`, `.zcode/`.
- Commit the existing `openspec/` scaffold (`config.yaml` with the `spec-driven` schema, empty `specs/` and `changes/archive/` placeholders) so OpenSpec workflows are versioned with the repo.

## Capabilities

This change declares no capabilities: it modifies developer tooling configuration only (ignore rules and project scaffolding) with no externally observable system behavior. `skip_specs: true` is set in `.openspec.yaml`.

### New Capabilities

- None.

### Modified Capabilities

- None.

## Impact

- `.gitignore`: six new entries; no existing entries removed or reordered.
- `openspec/config.yaml`, `openspec/specs/.gitkeep`, `openspec/changes/archive/.gitkeep`: new files committed as-is from the working tree.
- No build, API, dependency, or runtime impact (Rust crates under `crates/` untouched).
