# Tasks

## 1. Gitignore updates

- [ ] 1.1 Verify `.gitignore` contains the six agent-tool entries (`.cursor/`, `.hermes/skills/`, `.omp/`, `.opencode/`, `.qwen/`, `.zcode/`) and that `git status --short` no longer lists those directories as untracked

## 2. OpenSpec scaffold

- [ ] 2.1 Verify the `openspec/` scaffold files (`config.yaml`, `specs/.gitkeep`, `changes/archive/.gitkeep`) are present and unmodified, and `openspec list` runs without error from the repo root

## 3. Integration checks

- [ ] 3.1 Stage `.gitignore` and `openspec/` together with this change and verify `git diff --cached --stat` shows only the intended files
- [ ] 3.2 Run `openspec validate --change "ignore-agent-dirs-add-openspec-scaffold"` and confirm it passes with `skip_specs: true`

## Workflow follow-up

- Archive the change after review and commit.
