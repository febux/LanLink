# Tasks

From the original plan (`docs/webui-plan.md` — files/API/testing sections), all shipped:

- [x] 1 `crates/lanlink-web` crate: `main.rs` (CLI, server startup, static routes) + `api.rs` (handlers, daemon-socket client)
- [x] 2 Embedded SPA assets (`index.html`, `style.css`, `app.js`) with dashboard / peers / networks / help screens
- [x] 3 Read-only daemon proxy (`/api/snapshot`) with socket auto-discovery
- [x] 4 Coordinator write-through endpoints (`/api/directory`, `/api/networks`, `/api/networks/:id/join`)
- [x] 5 Local-only default binding (`127.0.0.1:8686`) with `--bind` override; `/api/health`
- [x] 6 Unit tests for the daemon-socket client against a fake Unix listener; README usage docs
