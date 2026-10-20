#!/usr/bin/env bash
# E2E: status snapshot reports a Direct UDP path between two joined peers.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

require_root

PEER_COUNT=2
trap 'cleanup_scenario $PEER_COUNT' EXIT

start_scenario "direct"
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

# Allow the first candidate refresh / punch cycle to run (30s interval).
log "waiting for direct path classification..."
sleep 35

wait_for_status_value "$(peer_status_socket 1)" "path" "Direct" || fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 does not see Direct path to peer 2"
wait_for_status_value "$(peer_status_socket 2)" "path" "Direct" || fail_scenario "$E2E_ASSERTION_FAILED" "peer 2 does not see Direct path to peer 1"

ip2="$(peer_tap_ip 2)"
wait_for_ping 1 "$ip2" || fail_scenario "$E2E_ASSERTION_FAILED" "ping from peer 1 to peer 2 (${ip2}) failed"

report_section "Result" "- both peers classify each other as Direct\n- ICMP echo peer1 -> peer2 (${ip2}) succeeded"
finish_scenario
