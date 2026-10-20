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
E2E_WORK_DIR="${E2E_WORK_DIR:-${E2E_DIR}/${E2E_SCENARIO}.\$\$}"
E2E_REPORT="${E2E_REPORT:-${E2E_WORK_DIR}/report.md}"

E2E_COORD_PORT="${E2E_COORD_PORT:-18080}"
E2E_RELAY_PORT="${E2E_RELAY_PORT:-13478}"
E2E_PASSWORD="${E2E_PASSWORD:-$(date +%s)}"
E2E_BR="${E2E_BR:-ll_e2e_br}"
E2E_BR_IP="${E2E_BR_IP:-10.203.0.1}"
E2E_BR_NET="${E2E_BR_NET:-10.203.0.0/24}"
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

report_section() {
    {
        echo
        echo "## $1"
        echo "$2"
    } >> "${E2E_REPORT}"
}

# ---- process liveness ------------------------------------------------------
proc_alive() {
    local pid="$1" state
    [ -n "$pid" ] || return 1
    state="$(cut -d' ' -f3 "/proc/${pid}/stat" 2>/dev/null)" || return 1
    [ -n "$state" ] && [ "$state" != "Z" ]
}

# ---- topology helpers ------------------------------------------------------
peer_ns_name() {
    printf 'e2e_peer%s' "$1"
}

peer_host_veth() {
    printf 'e2e_veth%s_host' "$1"
}

peer_ns_veth() {
    printf 'e2e_veth%s_ns' "$1"
}

setup_bridge() {
    ip link show "$E2E_BR" >/dev/null 2>&1 || ip link add "$E2E_BR" type bridge
    ip addr add "${E2E_BR_IP}/24" dev "$E2E_BR" 2>/dev/null || true
    ip link set "$E2E_BR" up
}

teardown_bridge() {
    ip link del "$E2E_BR" 2>/dev/null || true
}

create_peer_namespace() {
    local idx="$1"
    local ns host_veth ns_veth ns_ip
    ns="$(peer_ns_name "$idx")"
    host_veth="$(peer_host_veth "$idx")"
    ns_veth="$(peer_ns_veth "$idx")"
    ns_ip="10.203.0.$((10 + idx))"

    ip netns add "$ns" 2>/dev/null || true
    ip link add "$host_veth" type veth peer name "$ns_veth" netns "$ns"
    ip link set "$host_veth" master "$E2E_BR" up
    ip -n "$ns" link set lo up
    ip -n "$ns" addr add "${ns_ip}/24" dev "$ns_veth"
    ip -n "$ns" link set "$ns_veth" up
    ip -n "$ns" route add default via "$E2E_BR_IP"
}

destroy_peer_namespace() {
    local idx="$1"
    local ns
    ns="$(peer_ns_name "$idx")"
    ip netns del "$ns" 2>/dev/null || true
}

setup_topology() {
    local peer_count="$1"
    setup_bridge
    for i in $(seq 1 "$peer_count"); do
        create_peer_namespace "$i"
    done
    log "topology ready: ${peer_count} peer namespace(s) on ${E2E_BR}"
}

teardown_topology() {
    local peer_count="$1"
    for i in $(seq 1 "$peer_count"); do
        destroy_peer_namespace "$i"
    done
    teardown_bridge
}

# ---- scenario entry/exit -------------------------------------------------
start_scenario() {
    local name="$1"
    E2E_SCENARIO="$name"
    E2E_WORK_DIR="${E2E_DIR}/${name}.\$\$"
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

# ---- peer configuration and lifecycle --------------------------------------
peer_config_path() {
    printf '%s/peer%s.toml' "${E2E_WORK_DIR}" "$1"
}

peer_pidfile() {
    printf '%s/peer%s.pid' "${E2E_WORK_DIR}" "$1"
}

peer_status_socket() {
    printf '%s/peer%s.sock' "${E2E_WORK_DIR}" "$1"
}

write_peer_config() {
    local idx="$1"
    local network_id="$2"
    local bind_port="$3"
    local cfg tap status_socket
    cfg="$(peer_config_path "$idx")"
    tap="ll0"
    status_socket="$(peer_status_socket "$idx")"

    cat > "$cfg" <<EOF
[network]
coordinator = "http://${E2E_BR_IP}:${E2E_COORD_PORT}"
network_id = ${network_id}
password = "${E2E_PASSWORD}"
bind = "0.0.0.0:${bind_port}"
tap = "${tap}"
mtu = 1400
stun_servers = []

[status]
socket = "${status_socket}"

[telemetry]
enabled = false

[gateway]
enabled = false
EOF
}

start_peer() {
    local idx="$1"
    local ns cfg log pidfile
    ns="$(peer_ns_name "$idx")"
    cfg="$(peer_config_path "$idx")"
    log="${E2E_WORK_DIR}/peer${idx}.log"
    pidfile="$(peer_pidfile "$idx")"

    ip netns exec "$ns" sh -c "exec >\"$log\" 2>&1; printf '%s' \$\$ > \"$pidfile\"; exec \"$DAEMON_BIN\" run --config \"$cfg\"" &

    local deadline=$(("$(date +%s)" + 20))
    while [ "$(date +%s)" -lt "$deadline" ]; do
        [ -s "$pidfile" ] && proc_alive "$(cat "$pidfile")" && break
        sleep 0.2
    done

    if ! [ -s "$pidfile" ] || ! proc_alive "$(cat "$pidfile")"; then
        fail_scenario "$E2E_PEER_START" "peer ${idx} failed to start"
    fi
    log "peer ${idx} started (pid $(cat "$pidfile"))"
}

stop_peer() {
    local idx="$1"
    local pidfile
    pidfile="$(peer_pidfile "$idx")"
    if [ -f "$pidfile" ]; then
        local pid
        pid="$(cat "$pidfile")"
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
        rm -f "$pidfile"
    fi
}

# ---- coordinator / relay ---------------------------------------------------
start_coordinator() {
    local db_path="${E2E_WORK_DIR}/coordinator.db"
    "$COORD_BIN" --listen "0.0.0.0:${E2E_COORD_PORT}" \
        --db "$db_path" \
        --relay-server "${E2E_BR_IP}:${E2E_RELAY_PORT}" \
        > "${E2E_WORK_DIR}/coordinator.log" 2>&1 &
    echo $! > "${E2E_WORK_DIR}/coordinator.pid"
    sleep 1
    proc_alive "$(cat "${E2E_WORK_DIR}/coordinator.pid")" || fail_scenario "$E2E_COORD_START" "coordinator failed to start"
    log "coordinator listening on ${E2E_BR_IP}:${E2E_COORD_PORT}"
}

stop_coordinator() {
    if [ -f "${E2E_WORK_DIR}/coordinator.pid" ]; then
        kill "$(cat "${E2E_WORK_DIR}/coordinator.pid")" 2>/dev/null || true
        wait "$(cat "${E2E_WORK_DIR}/coordinator.pid")" 2>/dev/null || true
        rm -f "${E2E_WORK_DIR}/coordinator.pid"
    fi
}

start_relayd() {
    "$RELAY_BIN" --listen "0.0.0.0:${E2E_RELAY_PORT}" \
        > "${E2E_WORK_DIR}/relayd.log" 2>&1 &
    echo $! > "${E2E_WORK_DIR}/relayd.pid"
    sleep 1
    proc_alive "$(cat "${E2E_WORK_DIR}/relayd.pid")" || fail_scenario "$E2E_COORD_START" "relayd failed to start"
    log "relayd listening on ${E2E_BR_IP}:${E2E_RELAY_PORT}"
}

stop_relayd() {
    if [ -f "${E2E_WORK_DIR}/relayd.pid" ]; then
        kill "$(cat "${E2E_WORK_DIR}/relayd.pid")" 2>/dev/null || true
        wait "$(cat "${E2E_WORK_DIR}/relayd.pid")" 2>/dev/null || true
        rm -f "${E2E_WORK_DIR}/relayd.pid"
    fi
}

# ---- network provisioning --------------------------------------------------
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
    log "created network ${NETWORK_ID}"
}

# ---- status socket query ---------------------------------------------------
status_query() {
    local socket_path="$1"
    local payload="$2"
    if [ ! -S "$socket_path" ]; then
        echo "status socket not found: $socket_path" >&2
        return 1
    fi

    if command -v socat >/dev/null 2>&1; then
        printf '%s\n' "$payload" | socat - UNIX-CONNECT:"$socket_path"
        return 0
    fi

    if command -v nc >/dev/null 2>&1; then
        printf '%s\n' "$payload" | nc -U "$socket_path"
        return 0
    fi

    echo "need socat or nc (netcat) for status socket queries" >&2
    return 1
}

status_snapshot() {
    local socket_path="$1"
    status_query "$socket_path" '{"cmd":"snapshot"}'
}

wait_for_status_socket() {
    local socket_path="$1"
    local deadline=$(("$(date +%s)" + 30))
    while [ "$(date +%s)" -lt "$deadline" ]; do
        [ -S "$socket_path" ] && return 0
        sleep 0.2
    done
    return 1
}

wait_for_status_value() {
    local socket_path="$1"
    local key="$2"
    local value="$3"
    local deadline=$(("$(date +%s)" + 30))
    while [ "$(date +%s)" -lt "$deadline" ]; do
        if status_snapshot "$socket_path" 2>/dev/null | grep -q "\"${key}\".*\"${value}\""; then
            return 0
        fi
        sleep 0.5
    done
    return 1
}

# ---- TAP / connectivity probes ---------------------------------------------
wait_for_peer_tap() {
    local idx="$1"
    local ns
    ns="$(peer_ns_name "$idx")"
    local deadline=$(("$(date +%s)" + 30))
    while [ "$(date +%s)" -lt "$deadline" ]; do
        if ip -n "$ns" addr show dev ll0 2>/dev/null | grep -q 'inet '; then
            return 0
        fi
        sleep 0.5
    done
    return 1
}

wait_for_ping() {
    local src_idx="$1"
    local dst_ip="$2"
    local ns
    ns="$(peer_ns_name "$src_idx")"
    local deadline=$(("$(date +%s)" + 30))
    while [ "$(date +%s)" -lt "$deadline" ]; do
        if ip netns exec "$ns" ping -c 1 -W 1 "$dst_ip" >/dev/null 2>&1; then
            return 0
        fi
        sleep 1
    done
    return 1
}

peer_tap_ip() {
    local idx="$1"
    local ns
    ns="$(peer_ns_name "$idx")"
    ip -n "$ns" -4 -o addr show dev ll0 2>/dev/null | awk '{print $4}' | cut -d/ -f1
}

# ---- full cleanup ----------------------------------------------------------
cleanup_scenario() {
    local peer_count="${1:-0}"
    for i in $(seq 1 "$peer_count"); do
        stop_peer "$i" || true
    done
    stop_relayd || true
    stop_coordinator || true
    teardown_topology "$peer_count" || true
}
