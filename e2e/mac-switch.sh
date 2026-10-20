#!/usr/bin/env bash
# E2E: ARP resolution + ICMP between peer TAPs, MAC-table unicast.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

require_root

PEER_COUNT=2
trap 'cleanup_scenario $PEER_COUNT' EXIT

start_scenario "mac-switch"
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
wait_for_ping 1 "$ip2" || fail_scenario "$E2E_ASSERTION_FAILED" "ICMP echo to peer 2 failed"

# ARP entry for peer 2 should be reachable on peer 1's TAP.
ns1="$(peer_ns_name 1)"
arp="$(ip -n "$ns1" neigh show dev ll0 "$ip2" 2>/dev/null || true)"
printf '%s\n' "$arp" | grep -qiE 'REACHABLE|STALE|DELAY' || fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 did not learn peer 2 MAC (ARP: $arp)"

report_section "Result" "- ICMP echo peer1 -> peer2 succeeded\n- ARP entry for ${ip2} is ${arp}"
finish_scenario
