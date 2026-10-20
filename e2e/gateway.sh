#!/usr/bin/env bash
# E2E: a gateway peer routes a shared subnet to an attached LAN namespace.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

require_root

PEER_COUNT=2
trap 'cleanup_scenario $PEER_COUNT; teardown_lan' EXIT

SHARED_SUBNET="192.168.77.0/24"
LAN_HOST_IP="192.168.77.10"
LAN_NS="e2e_lan_host"
GW_PEER=1

teardown_lan() {
    ip netns del "$LAN_NS" 2>/dev/null || true
    local gw_ns host_veth
    gw_ns="$(peer_ns_name $GW_PEER)"
    host_veth="e2e_gw_lan_host"
    ip -n "$gw_ns" link del "${host_veth}_gw" 2>/dev/null || true
    ip link del "$host_veth" 2>/dev/null || true
}

create_lan() {
    ip netns add "$LAN_NS" 2>/dev/null || true
    local gw_ns host_veth gw_veth
    gw_ns="$(peer_ns_name $GW_PEER)"
    host_veth="e2e_gw_lan_host"
    gw_veth="e2e_gw_lan_gw"

    ip link add "$host_veth" type veth peer name "$gw_veth" netns "$gw_ns"
    ip -n "$LAN_NS" link set "$host_veth" netns "$LAN_NS"
    ip -n "$LAN_NS" addr add "${LAN_HOST_IP}/24" dev "$host_veth"
    ip -n "$LAN_NS" link set "$host_veth" up
    ip -n "$LAN_NS" link set lo up

    ip -n "$gw_ns" addr add "192.168.77.1/24" dev "$gw_veth"
    ip -n "$gw_ns" link set "$gw_veth" up
}

start_scenario "gateway"
setup_topology "$PEER_COUNT"
create_lan
start_coordinator
start_relayd
create_network

# Peer 1 is the gateway; peer 2 routes the shared subnet through it.
write_peer_config 1 "${NETWORK_ID}" 9000
cat >> "$(peer_config_path 1)" <<EOF

[gateway]
enabled = true
shared_subnet = "${SHARED_SUBNET}"
phys_interface = "e2e_gw_lan_gw"
EOF

write_peer_config 2 "${NETWORK_ID}" 9001
start_peer 1
start_peer 2

for i in 1 2; do
    wait_for_peer_tap "$i" || fail_scenario "$E2E_PEER_START" "peer ${i} TAP did not come up"
done

log "waiting for gateway route propagation..."
sleep 35

# Add a static route on peer 2 towards the LAN host via the gateway peer.
gw_virt_ip="$(peer_tap_ip 1)"
ns2="$(peer_ns_name 2)"
ip -n "$ns2" route add "${SHARED_SUBNET}" via "$gw_virt_ip" || true

# Allow forwarding on the gateway peer and enable NAT for the shared subnet.
gw_ns="$(peer_ns_name $GW_PEER)"
ip -n "$gw_ns" sysctl -w net.ipv4.ip_forward=1 >/dev/null
ip -n "$gw_ns" iptables -t nat -A POSTROUTING -s "${SHARED_SUBNET}" -o e2e_gw_lan_gw -j MASQUERADE 2>/dev/null || true

ip netns exec "$ns2" ping -c 3 -W 2 "$LAN_HOST_IP" > "${E2E_WORK_DIR}/lan_ping.log" 2>&1 || fail_scenario "$E2E_ASSERTION_FAILED" "ping from peer 2 to LAN host failed"

report_section "Result" "- gateway peer ${GW_PEER} shares ${SHARED_SUBNET}\n- peer 2 reached LAN host ${LAN_HOST_IP} through the gateway"
finish_scenario
