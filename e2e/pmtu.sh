#!/usr/bin/env bash
# E2E: large ICMP echo payloads are fragmented and delivered without corruption.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

require_root

PEER_COUNT=2
trap 'cleanup_scenario $PEER_COUNT' EXIT

start_scenario "pmtu"
setup_topology "$PEER_COUNT"
start_coordinator
start_relayd
create_network

write_peer_config 1 "${NETWORK_ID}" 9000
write_peer_config 2 "${NETWORK_ID}" 9001
start_peer 1
start_peer 2

for i in 1 2; do
    wait_for_peer_tap "$i" || fail_scenario "$E2E_PEER_START" "peer ${i} TAP did not come up"
done

log "waiting for direct path..."
sleep 35

ip2="$(peer_tap_ip 2)"
ns1="$(peer_ns_name 1)"

# Payload larger than the inner 1400 MTU so fragmentation/reassembly must work.
ip netns exec "$ns1" ping -c 3 -s 2000 -W 2 "$ip2" > "${E2E_WORK_DIR}/ping.log" 2>&1 || fail_scenario "$E2E_ASSERTION_FAILED" "large ping failed"
report_section "Result" "- 2000-byte ICMP echo exchange with peer 2 succeeded"
finish_scenario
