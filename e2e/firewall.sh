#!/usr/bin/env bash
# E2E: public-network firewall drops blocked inbound TCP, allows ping.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

require_root

PEER_COUNT=2
trap 'cleanup_scenario $PEER_COUNT' EXIT

start_scenario "firewall"
setup_topology "$PEER_COUNT"
start_coordinator
start_relayd
# Public network so the firewall profile is applied.
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
ns2="$(peer_ns_name 2)"

# Start a TCP listener on a blocked port inside peer 2's namespace.
listener_log="${E2E_WORK_DIR}/listener.log"
if command -v socat >/dev/null 2>&1; then
    ip netns exec "$ns2" socat TCP-LISTEN:7777 /dev/null > "$listener_log" 2>&1 &
    listener_pid=$!
elif command -v nc >/dev/null 2>&1; then
    ip netns exec "$ns2" nc -l 7777 > "$listener_log" 2>&1 &
    listener_pid=$!
else
    fail_scenario "$E2E_ASSERTION_FAILED" "need socat or nc to run TCP listener"
fi
sleep 1

# Connection attempt from peer 1 must fail / time out.
if ip netns exec "$ns1" timeout 3 bash -c "exec 3>/dev/tcp/${ip2}/7777"; then
    kill "$listener_pid" 2>/dev/null || true
    wait "$listener_pid" 2>/dev/null || true
    fail_scenario "$E2E_ASSERTION_FAILED" "TCP connection to blocked port 7777 succeeded"
fi
kill "$listener_pid" 2>/dev/null || true
wait "$listener_pid" 2>/dev/null || true

# ICMP echo must still be allowed.
wait_for_ping 1 "$ip2" || fail_scenario "$E2E_ASSERTION_FAILED" "ping to peer 2 was blocked by firewall"

report_section "Result" "- TCP port 7777 inbound dropped\n- ICMP echo to peer 2 still allowed"
finish_scenario
