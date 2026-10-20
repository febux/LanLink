#!/usr/bin/env bash
# Shared helpers for the LanLink e2e scenario suite.
#
# The scenarios exercise real netns/TAP/coordinator/relay topologies and must
# run as root. Individual runners source this file and call the helpers; see
# e2e/README.md for the full topology and usage.
set -euo pipefail

# ---- paths -----------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
RELEASE_DIR="${REPO_DIR}/target/release"

# ---- default tunables (overridable by env) ---------------------------------
E2E_DIR="${E2E_DIR:-${REPO_DIR}/target/e2e}"
E2E_SCENARIO="${E2E_SCENARIO:-unknown}"
E2E_WORK_DIR="${E2E_WORK_DIR:-${E2E_DIR}/${E2E_SCENARIO}.$$}"
E2E_REPORT="${E2E_REPORT:-${E2E_WORK_DIR}/report.md}"

E2E_COORD_PORT="${E2E_COORD_PORT:-18080}"
E2E_RELAY_PORT="${E2E_RELAY_PORT:-13478}"
E2E_PASSWORD="${E2E_PASSWORD:-$(date +%s)}"
E2E_BR="${E2E_BR:-ll_e2e_br}"
E2E_BR_IP="${E2E_BR_IP:-10.203.0.1}"
E2E_NETWORK_NAME="${E2E_NETWORK_NAME:-e2e-$(date +%s)}"

# ---- exit codes ------------------------------------------------------------
E2E_OK=0
E2E_NO_ROOT=1
E2E_MISSING_BINARY=2
E2E_TOPOLOGY_SETUP=3
E2E_COORD_START=4
E2E_PEER_START=5
E2E_ASSERTION_FAILED=6
E2E_CLEANUP_FAILED=7

# ---- logging ---------------------------------------------------------------
log()  { printf '[e2e %s] %s\n' "$(date +%H:%M:%S)" "$*"; }
fail() { printf '[e2e] FATAL: %s\n' "$*" >&2; exit "${E2E_ASSERTION_FAILED}"; }

# ---- root guard ------------------------------------------------------------
require_root() {
    if [ "$(id -u)" -ne 0 ]; then
        echo "This scenario must run as root (netns/TAP require CAP_NET_ADMIN)." >&2
        exit "${E2E_NO_ROOT}"
    fi
}

# ---- binary discovery ------------------------------------------------------
find_release_binary() {
    local name="$1"
    local path="${RELEASE_DIR}/${name}"
    if [ -x "$path" ]; then
        printf '%s\n' "$path"
        return 0
    fi
    echo "missing release binary: ${path}; run: cargo build --release -p lanlinkd -p lanlinkctl -p coordinator -p lanlink-relay" >&2
    return 1
}

DAEMON_BIN="${RELEASE_DIR}/lanlinkd"
CTL_BIN="${RELEASE_DIR}/lanlinkctl"
COORD_BIN="${RELEASE_DIR}/lanlink-coordinator"
RELAY_BIN="${RELEASE_DIR}/lanlink-relayd"

discover_binaries() {
    DAEMON_BIN="$(find_release_binary lanlinkd || true)"
    CTL_BIN="$(find_release_binary lanlinkctl || true)"
    COORD_BIN="$(find_release_binary lanlink-coordinator || true)"
    RELAY_BIN="$(find_release_binary lanlink-relayd || true)"

    if [ -z "$DAEMON_BIN" ] || [ -z "$CTL_BIN" ] || [ -z "$COORD_BIN" ] || [ -z "$RELAY_BIN" ]; then
        exit "${E2E_MISSING_BINARY}"
    fi
}

# ---- work directory / report -----------------------------------------------
make_work_dir() {
    rm -rf "${E2E_WORK_DIR}"
    mkdir -p "${E2E_WORK_DIR}"
    : > "${E2E_WORK_DIR}/scenario.log"
}

write_report_header() {
    {
        echo "# E2E report: ${E2E_SCENARIO}"
        echo
        echo "- work dir: ${E2E_WORK_DIR}"
        echo "- started: $(date -Is)"
    } > "${E2E_REPORT}"
}

# ---- process liveness ------------------------------------------------------
proc_alive() {
    local pid="$1" state
    [ -n "$pid" ] || return 1
    state="$(cut -d' ' -f3 "/proc/${pid}/stat" 2>/dev/null)" || return 1
    [ -n "$state" ] && [ "$state" != "Z" ]
}

# ---- topology helpers (stubs expanded in group 3) --------------------------
setup_bridge() {
    ip link add "$E2E_BR" type bridge 2>/dev/null || true
    ip addr add "${E2E_BR_IP}/24" dev "$E2E_BR" 2>/dev/null || true
    ip link set "$E2E_BR" up 2>/dev/null || true
}

teardown_bridge() {
    ip link del "$E2E_BR" 2>/dev/null || true
}

# ---- scenario entry/exit -------------------------------------------------
start_scenario() {
    local name="$1"
    E2E_SCENARIO="$name"
    E2E_WORK_DIR="${E2E_DIR}/${name}.$$"
    E2E_REPORT="${E2E_WORK_DIR}/report.md"
    make_work_dir
    write_report_header
    discover_binaries
    log "scenario '${name}' work dir: ${E2E_WORK_DIR}"
}

fail_scenario() {
    local code="$1"
    local msg="$2"
    printf '[e2e] FAIL (%d): %s\n' "$code" "$msg" >&2
    printf '\n## Failure\n\n- exit code: %d\n- message: %s\n' "$code" "$msg" >> "${E2E_REPORT}"
    tail -n 20 "${E2E_WORK_DIR}"/*.log 2>/dev/null >&2 || true
    exit "$code"
}

finish_scenario() {
    {
        echo
        echo "## Verdict: PASS"
        echo "- finished: $(date -Is)"
    } >> "${E2E_REPORT}"
    log "report: ${E2E_REPORT}"
}

# ---- coordinator / network -------------------------------------------------
start_coordinator() {
    local db_path="${E2E_WORK_DIR}/coordinator.db"
    "$COORD_BIN" --listen "0.0.0.0:${E2E_COORD_PORT}" \
        --db "$db_path" \
        --relay-server "${E2E_BR_IP}:${E2E_RELAY_PORT}" \
        > "${E2E_WORK_DIR}/coordinator.log" 2>&1 &
    echo $! > "${E2E_WORK_DIR}/coordinator.pid"
    sleep 1
    proc_alive "$(cat "${E2E_WORK_DIR}/coordinator.pid")" || fail_scenario "$E2E_COORD_START" "coordinator failed to start"
}

start_relayd() {
    "$RELAY_BIN" --listen "0.0.0.0:${E2E_RELAY_PORT}" \
        > "${E2E_WORK_DIR}/relayd.log" 2>&1 &
    echo $! > "${E2E_WORK_DIR}/relayd.pid"
    sleep 1
    proc_alive "$(cat "${E2E_WORK_DIR}/relayd.pid")" || fail_scenario "$E2E_COORD_START" "relayd failed to start"
}

create_network() {
    local out
    out="$("$CTL_BIN" coord create \
        --coordinator "http://${E2E_BR_IP}:${E2E_COORD_PORT}" \
        --name "$E2E_NETWORK_NAME" \
        --password "$E2E_PASSWORD" \
        --public 2>&1)"
    printf '%s\n' "$out" >> "${E2E_WORK_DIR}/scenario.log"
    NETWORK_ID="$(printf '%s\n' "$out" | awk '{print $3; exit}')"
    case "$NETWORK_ID" in ''|*[!0-9]*) fail_scenario "$E2E_COORD_START" "could not parse network id from: $out";; esac
    export NETWORK_ID
}

# ---- status socket query ---------------------------------------------------
status_snapshot() {
    local socket_path="$1"
    if [ ! -S "$socket_path" ]; then
        echo "status socket not found: $socket_path" >&2
        return 1
    fi
    printf '{"cmd":"snapshot"}\n' | socat - UNIX-CONNECT:"$socket_path" 2>/dev/null || true
}
