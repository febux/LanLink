# Proposal

## Why

Feature work in this repo has been tracked as free-form plan documents under `docs/` (`tui-plan.md`, `webui-plan.md`, `noise-identity-plan.md`, `tcp-teredo-plan.md`, `windows-plan.md`). The repo now uses OpenSpec; these plans predate it and duplicate its purpose. Four of the five describe **completed** work (M9 TUI, M9+ WebUI, Noise identity, TCP fallback shipped per git history and PLAN.md milestones), and the fifth (Windows) mixes shipped state with remaining gaps. Leaving them in `docs/` splits planning state across two systems and clutters the published docs site with scaffolding.

## What Changes

- Convert the four **completed** plans into archived OpenSpec changes under `openspec/changes/archive/2026-10-15-*`: `tui-status-and-frontend`, `webui-frontend`, `noise-identity`, `tcp-fallback`. Each gets `proposal.md` (why/what), `design.md` (the original technical plan content), and `tasks.md` with checkboxes marked done.
- Convert the **remaining** work from `docs/windows-plan.md` and the deferred Teredo item of `docs/tcp-teredo-plan.md` into one active OpenSpec change `windows-parity` (proposal + tasks, specs deferred as `skip_specs` until requirements firm up).
- Remove the five scaffolding `docs/*-plan.md` files after their content is migrated.
- Update `docs/SUMMARY.md` and cross-references (`PLAN.md` milestone table, `docs/README.md` if it links plans) to point at `openspec/changes/` instead of deleted files.
- Keep `PLAN.md` (root) and all non-plan docs (`intro.md`, `protocol.md`, `api.md`, `ops.md`, `threat-model.md`, `install.md`, `release.md`) untouched.

## Capabilities

Docs/tooling reorganization only; no runtime behavior changes. `skip_specs: true` is set in `.openspec.yaml`.

### New Capabilities

- None.

### Modified Capabilities

- None.

## Impact

- Deleted: `docs/tui-plan.md`, `docs/webui-plan.md`, `docs/noise-identity-plan.md`, `docs/tcp-teredo-plan.md`, `docs/windows-plan.md`.
- Added: 4 archived changes + 1 active change under `openspec/changes/`.
- Edited: `docs/SUMMARY.md`, `PLAN.md` (milestone-table plan links only).
- `docs/build.sh` / `_site/` may need regeneration after SUMMARY changes (site build is manual).
