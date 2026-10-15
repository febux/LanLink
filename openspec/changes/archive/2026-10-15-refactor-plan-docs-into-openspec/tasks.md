# Tasks

## 1. Migrate completed plans to archived changes

- [x] 1.1 Create `archive/2026-10-15-noise-identity` (proposal/design/tasks marked done) from `docs/noise-identity-plan.md` and verify `openspec validate` accepts it
- [x] 1.2 Create `archive/2026-10-15-tcp-fallback` from the implemented TCP sections of `docs/tcp-teredo-plan.md` and verify it validates
- [x] 1.3 Create `archive/2026-10-15-tui-status-and-frontend` from `docs/tui-plan.md` and verify it validates
- [x] 1.4 Create `archive/2026-10-15-webui-frontend` from `docs/webui-plan.md` and verify it validates

## 2. Migrate remaining work to an active change

- [x] 2.1 Create active change `windows-parity` (proposal + tasks, `skip_specs: true`) covering the unshipped items of `docs/windows-plan.md` plus the deferred Teredo/IPv6 item, and verify `openspec validate` passes

## 3. Remove scaffolding docs and fix references

- [x] 3.1 Delete the five `docs/*-plan.md` files and verify none remain (`ls docs/*-plan.md` fails)
- [x] 3.2 Update `docs/SUMMARY.md` (drop plan entries) and `PLAN.md` milestone-table links to point to `openspec/changes/`; verify no dangling links via `grep -r "plan.md" docs/ PLAN.md`

## 4. Integration checks

- [x] 4.1 Run `openspec list` (shows `windows-parity` and `refactor-plan-docs-into-openspec` active) and `openspec validate` on all five new changes; verify the docs build script still lists only existing files

## Workflow follow-up

- Archive this change and `windows-parity` (when shipped) after review.
