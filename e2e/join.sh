#!/usr/bin/env bash
# E2E: two peers join the same network and receive distinct virtual IPs.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

require_root

PEER_COUNT=2
trap 'cleanup_scenario $PEER_COUNT' EXIT

start_scenario "join"
setup_topology "$PEER_COUNT"
start_coordinator
start_relayd
create_network

write_peer_config 1 "${NETWORK_ID}" 9000
write_peer_config 2 "${NETWORK_ID}" 9001
start_peer 1
start_peer 2

for i in 1 2; do
    wait_for_peer_tap "$i" || fail_scenario "$E2E_PEER_START" "peer ${i} TAP did not acquire an IP"
done

ip1="$(peer_tap_ip 1)"
ip2="$(peer_tap_ip 2)"
[ -n "$ip1" ] || fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 has no virtual IP"
[ -n "$ip2" ] || fail_scenario "$E2E_ASSERTION_FAILED" "peer 2 has no virtual IP"
[ "$ip1" != "$ip2" ] || fail_scenario "$E2E_ASSERTION_FAILED" "peers received the same virtual IP"

report_section "Result" "- peer 1 virtual IP: ${ip1}\n- peer 2 virtual IP: ${ip2}"
finish_scenario
