# Design

## Context

The daemon currently uses `tracing_subscriber::fmt` with an `EnvFilter` for both console and Windows service file logging. Status snapshots are served as NDJSON over a Unix/TCP socket from `crates/lanlinkd/src/status.rs`; the types live in `crates/lanlink-proto/src/status.rs`. `lanlinkctl` already talks to the coordinator via HTTP and can be extended with a local-daemon subcommand. No existing PII masking exists; the telemetry module is aggregate-only and does not touch per-peer data.

See `proposal.md` for motivation and scope.

## Goals / Non-Goals

**Goals:**
- Provide a small, pure, testable anonymization API in the shared protocol crate.
- Add a tracing layer/formatter that masks PII in every emitted log line when enabled.
- Allow frontends to request an anonymized status snapshot.
- Add a `lanlinkctl bugreport` command that forces anonymization of logs and config.
- Cover all mask functions with unit tests including edge cases.

**Non-Goals:**
- Full structured-log field redaction (passwords are handled by never logging them; the anonymizer works on the rendered text).
- Anonymization of data-plane packet payloads.
- Encrypted or differentially-private anonymization; the design is deterministic salted hashing for correlation, not cryptographic privacy.
- Changing the default log format (JSON/pretty remain supported by the underlying subscriber).

## Decisions

### Where to place the anonymization module

**Decision:** Add `crates/lanlink-proto/src/anonymize.rs` as a new public module.

**Rationale:** `lanlink-proto` is already shared by `lanlinkd` and `lanlinkctl`, so both the daemon tracing layer and the bug-report collector can import it without adding a new crate. It is also the natural home for protocol-level constants such as default prefix lengths.

**Alternative considered:** A new `lanlink-privacy` crate. Rejected because the module is small and cross-cutting; a separate crate adds build overhead without clear boundaries yet.

### Hash function and salt handling

**Decision:** Use SHA-256 as the hash, and derive a per-process salt from (in order):
1. `LANLINK_ANON_SALT` environment variable (hex or base64).
2. A random 32-byte seed generated once at process startup.

**Rationale:** Deterministic hashing across one process run keeps correlations between log lines intact (e.g., the same peer IP always renders the same masked token). A per-process random seed prevents rainbow-table recovery of values across different reports while still allowing correlation inside one report. The env var lets operators pin a salt when they need reproducible cross-run correlation.

**Alternative considered:** Hard-coded salt. Rejected because it would make rainbow-table attacks trivial once the salt leaks.

### IP prefix lengths

**Decision:** Default to IPv4 /24 and IPv6 /64; make the prefix configurable through the anonymizer's constructor/initializer for tests and future tuning.

**Rationale:** /24 is the smallest common site prefix for IPv4 and preserves subnet-level correlation. /64 is the standard IPv6 network boundary. Configurability lets tests verify behavior at different prefix lengths without changing constants.

### Tracing integration shape

**Decision:** Implement an `AnonymizeLayer<S>` wrapping an inner `Layer<S>` and a shared `Anonymizer`. In `on_event` the layer formats the event to a temporary string through the inner layer's formatter, applies `anonymizer.mask_text`, and writes the result to the original writer.

**Rationale:** This works regardless of whether the subscriber emits plain text or JSON; the anonymizer operates on the already-serialized bytes. It also keeps the existing `EnvFilter` and formatting options untouched.

**Alternative considered:** Custom `FormatEvent`. Rejected because it ties the implementation to `tracing-subscriber`'s text formatter and is harder to reuse for the Windows service file appender, which already builds a layer manually.

### Status snapshot anonymization

**Decision:** Extend `StatusRequest::Snapshot` with an optional `privacy` field (default `false`). When `true`, the daemon builds the normal snapshot and then applies the anonymizer to every address, node ID, and network name in the response before serialization.

**Rationale:** A single request type keeps the NDJSON protocol simple and backward-compatible; older frontends that omit `privacy` continue to receive full snapshots.

**Alternative considered:** A separate `StatusRequest::SnapshotAnonymized` variant. Rejected because it duplicates snapshot logic and complicates frontend handlers.

### Bug report bundle contents

**Decision:** `lanlinkctl bugreport` accepts `--config <path>` and `--log-file <path>` (defaulting to the Windows service log path on Windows and to the systemd journal hint on Unix). It creates a timestamped directory containing:
- `logs.txt` — the provided log file run through the anonymizer with a freshly generated salt.
- `config.toml` — a sanitized copy of the config with `password`, `password_file`, and any `network.name` field replaced by `<redacted>` or a masked token.
- `snapshot.jsonl` — one NDJSON line: a privacy-enabled status snapshot from the daemon's status socket, if reachable.

**Rationale:** Forcing a fresh salt for the bundle ensures each bug report is independent; even if a previous report is de-anonymized, it does not expose values from another run. Redacting the config fields eliminates the most common accidental secret leakage.

## Risks / Trade-offs

- [Risk] Masking after serialization may alter JSON escape sequences if the regex matches escaped characters.  
  **Mitigation:** The anonymizer matches only well-formed IP/MAC/token patterns; it does not rewrite arbitrary substrings. Tests include JSON log lines to verify valid output.

- [Risk] A custom layer that buffers each event could reduce peak log throughput.  
  **Mitigation:** The buffer is a small in-memory `Vec<u8>` per event; the operation is CPU-bound hashing. Benchmarking can be added later if it becomes a concern, and the layer is off by default.

- [Risk] A fixed prefix exposes the subnet of an address, which may itself be sensitive in some deployments.  
  **Mitigation:** Prefixes are configurable; future work can add `/32` and `/128` "full mask" modes.

- [Risk] Passwords that appear in unexpected log messages (e.g., a user passes one on a CLI flag) will only be masked if they look like one of the supported patterns.  
  **Mitigation:** The password-CLI flags are already documented as discouraged; the bug-report command additionally redacts known config fields and scans for common secret patterns.

## Open Questions

- Should the TUI/WebUI request `privacy=true` automatically when the user toggles a "privacy mode" switch, or should it be a per-view opt-in? This is a frontend UX decision that does not change the daemon protocol or the spec.
