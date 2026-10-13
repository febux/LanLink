#!/usr/bin/env bash
# M7 memory-safety audit: miri + optional valgrind on the LanLink workspace.
#
# Run this in CI or before a release tag. It is intentionally strict about the
# parser/switch/crypto crates that handle hostile network input.
#
# Usage:
#   ./tests/memory-audit.sh
#
# Environment:
#   MIRI_TOOLCHAIN  rust toolchain with miri (default: nightly)
#   VALGRIND_OPTS   extra valgrind flags (default: --leak-check=full --error-exitcode=1)
#   NO_VALGRIND=1   skip valgrind even when available (e.g. in a sandbox)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
MIRI_TOOLCHAIN="${MIRI_TOOLCHAIN:-nightly}"
VALGRIND_OPTS="${VALGRIND_OPTS:---leak-check=full --error-exitcode=1}"
REPORT="${REPORT:-${REPO_DIR}/target/memory-audit-report.md}"

mkdir -p "$(dirname "$REPORT")"
: > "$REPORT"

log()  { printf '[memory-audit] %s\n' "$*"; }
fail() { printf '[memory-audit] FATAL: %s\n' "$*" >&2; exit 1; }
append() { printf '%s\n' "$*" >> "$REPORT"; }

log "toolchain: ${MIRI_TOOLCHAIN}"
command -v rustup >/dev/null 2>&1 || command -v "cargo" >/dev/null 2>&1 \
    || fail "rustup / cargo not found"

# Ensure miri is installed for the selected toolchain.
rustup run "$MIRI_TOOLCHAIN" cargo miri --version >/dev/null 2>&1 \
    || { log "installing miri for ${MIRI_TOOLCHAIN}..."; rustup component add --toolchain "$MIRI_TOOLCHAIN" miri; }

append "# LanLink memory-audit report"
append ""
append "- date: $(date -Is)"
append "- miri toolchain: ${MIRI_TOOLCHAIN}"
append ""

# ---------------------------------------------------------------------------
# Miri pass: parser and switch crates (pure-ish, no kernel/argon2 heavy work).
# The crypto crate is tested selectively below because full Argon2 under miri
# is prohibitively slow (intentional memory-hard behaviour).
# ---------------------------------------------------------------------------
run_miri() {
    local crate="$1"
    shift
    log "miri: ${crate} $@"
    if cargo +"$MIRI_TOOLCHAIN" miri test -p "$crate" "$@" >>"$REPORT" 2>&1; then
        append ""
        append "- miri ${crate} $@: PASS"
    else
        append ""
        append "- miri ${crate} $@: FAIL"
        fail "miri failed for ${crate} $@ (see ${REPORT})"
    fi
}

run_miri lanlink-proto
run_miri lanlink-switch
# Replay window is pure integer logic; it is the most useful crypto test under miri.
run_miri lanlink-crypto replay_window

# ---------------------------------------------------------------------------
# Valgrind pass: native test binaries for the same crates.
# We build the test binary, then run it under valgrind so all allocations made
# by dependencies are checked too.  Skipped when not installed.
# ---------------------------------------------------------------------------
if [ "${NO_VALGRIND:-0}" = "1" ]; then
    log "valgrind skipped (NO_VALGRIND=1)"
    append "- valgrind: SKIPPED (NO_VALGRIND=1)"
elif command -v valgrind >/dev/null 2>&1; then
    run_valgrind() {
        local crate="$1"
        log "valgrind: ${crate}"
        local bin
        bin="$(cargo test -p "$crate" --no-run --message-format=json 2>/dev/null \
            | jq -r 'select(.target.kind[] == "lib" or .target.kind[] == "test") | .executable' \
            | tail -1)"
        [ -n "$bin" ] && [ -x "$bin" ] || fail "could not locate test binary for ${crate}"
        if valgrind $VALGRIND_OPTS "$bin" >>"$REPORT" 2>&1; then
            append "- valgrind ${crate}: PASS"
        else
            append "- valgrind ${crate}: FAIL"
            fail "valgrind failed for ${crate} (see ${REPORT})"
        fi
    }

    command -v jq >/dev/null 2>&1 || fail "jq required for valgrind pass (or set NO_VALGRIND=1)"
    run_valgrind lanlink-proto
    run_valgrind lanlink-switch
    run_valgrind lanlink-crypto
else
    log "valgrind not installed; skipping native leak check"
    append "- valgrind: SKIPPED (not installed)"
fi

append ""
append "## Verdict: PASS"
log "report: ${REPORT}"
