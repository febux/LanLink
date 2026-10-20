#!/usr/bin/env bash
# E2E: NDJSON status socket returns peers, IPs, and a non-Waiting path.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

require_root

PEER_COUNT=2
trap 'cleanup_scenario $PEER_COUNT' EXIT

start_scenario "status"
setup_topology "$PEER_COUNT"
start_coordinator
start_relayd
create_network

write_peer_config 1 "${NETWORK_ID}" 9000
write_peer_config 2 "${NETWORK_ID}" 9001
start_peer 1
start_peer 2

for i in 1 2; do
    wait_for_status_socket "$(peer_status_socket "$i")" || fail_scenario "$E2E_PEER_START" "peer ${i} status socket not ready"
done

log "waiting for peer list..."
sleep 35

snap1="$(status_snapshot "$(peer_status_socket 1)")"
[ -n "$snap1" ] || fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 returned empty snapshot"

printf '%s\n' "$snap1" | grep -q '"assigned_ip"' || fail_scenario "$E2E_ASSERTION_FAILED" "snapshot missing assigned_ip"
printf '%s\n' "$snap1" | grep -q '"peers"' || fail_scenario "$E2E_ASSERTION_FAILED" "snapshot missing peers list"
printf '%s\n' "$snap1" | grep -qE '"path":"(Direct|Relay|TcpDirect|TcpRelay|Punching)"' || fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 still sees Waiting path"

report_section "Result" "- status snapshot contains assigned_ip and peer list\n- at least one peer has a non-Waiting path"
finish_scenario
