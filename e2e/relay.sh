#!/usr/bin/env bash
# E2E: block direct UDP between peers and verify ICMP still flows via relay.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

require_root

PEER_COUNT=2
trap 'cleanup_scenario $PEER_COUNT' EXIT

start_scenario "relay"
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

log "waiting for initial Direct path..."
sleep 35

# Confirm direct path is up before we break it.
wait_for_status_value "$(peer_status_socket 1)" "path" "Direct" || fail_scenario "$E2E_ASSERTION_FAILED" "direct path did not establish"

# Block direct UDP between the two peers while leaving the relay path open.
ns1="$(peer_ns_name 1)"
ns2="$(peer_ns_name 2)"
ip1="10.203.0.11"
ip2="10.203.0.12"
ip netns exec "$ns1" iptables -A OUTPUT -p udp -d "$ip2" --dport 9001 -j DROP
ip netns exec "$ns2" iptables -A OUTPUT -p udp -d "$ip1" --dport 9000 -j DROP

log "direct UDP blocked; waiting for relay fallback..."
sleep 35

wait_for_status_value "$(peer_status_socket 1)" "path" "Relay" || fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 did not fall back to Relay"
wait_for_status_value "$(peer_status_socket 2)" "path" "Relay" || fail_scenario "$E2E_ASSERTION_FAILED" "peer 2 did not fall back to Relay"

ip2_tap="$(peer_tap_ip 2)"
wait_for_ping 1 "$ip2_tap" || fail_scenario "$E2E_ASSERTION_FAILED" "ping over relay path failed"

report_section "Result" "- direct UDP blocked with iptables\n- both peers switched to Relay path\n- ICMP echo over relay path succeeded"
finish_scenario
