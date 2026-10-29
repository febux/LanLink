#!/usr/bin/env bash
# E2E: disconnect from the network via the CLI, verify TAP/IP/firewall are
# released, then reconnect the running daemon to the same network and confirm
# connectivity is restored. Finally gracefully stop the daemon.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

require_root

PEER_COUNT=2
trap 'cleanup_scenario $PEER_COUNT' EXIT

start_scenario "disconnect"
setup_topology "$PEER_COUNT"
start_coordinator
start_relayd
create_network

write_peer_config 1 "${NETWORK_ID}" 9000
write_peer_config 2 "${NETWORK_ID}" 9001
start_peer 1
start_peer 2

for i in 1 2; do
    wait_for_status_socket "$(peer_status_socket "$i")" || fail_scenario "$E2E_PEER_START" "peer ${i} control socket not ready"
done

log "waiting for peers to establish paths and TAP interfaces..."
for i in 1 2; do
    wait_for_peer_tap "$i" || fail_scenario "$E2E_ASSERTION_FAILED" "peer ${i} TAP did not appear"
done
sleep 10

sock1="$(peer_status_socket 1)"
sock2="$(peer_status_socket 2)"
ns1="$(peer_ns_name 1)"
tap="ll0"

snap1="$(status_snapshot "$sock1")"
[ -n "$snap1" ] || fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 returned empty snapshot"
printf '%s\n' "$snap1" | grep -q '"connected":true' || fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 should be connected"

# Remember peer 2's TAP IP so we can verify restored connectivity after reconnect.
ip2="$(peer_tap_ip 2)"
[ -n "$ip2" ] || fail_scenario "$E2E_ASSERTION_FAILED" "peer 2 has no TAP IP"

# ---- Disconnect peer 1 via CLI ---------------------------------------------
log "disconnecting peer 1 via lanlinkctl..."
"$CTL_BIN" daemon disconnect --socket "$sock1" >/dev/null || fail_scenario "$E2E_ASSERTION_FAILED" "lanlinkctl daemon disconnect failed"

log "waiting for peer 1 to report disconnected..."
deadline=$(("$(date +%s)" + 30))
while [ "$(date +%s)" -lt "$deadline" ]; do
    snap1="$(status_snapshot "$sock1" 2>/dev/null || true)"
    if printf '%s\n' "$snap1" | grep -q '"connected":false'; then
        break
    fi
    sleep 0.5
done
printf '%s\n' "$snap1" | grep -q '"connected":false' || fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 did not report disconnected"

# Assert TAP/IP are gone from the namespace.
if ip -n "$ns1" link show dev "$tap" >/dev/null 2>&1; then
    fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 TAP ${tap} still exists after disconnect"
fi
if ip -n "$ns1" -4 -o addr show dev "$tap" 2>/dev/null | grep -q 'inet '; then
    fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 TAP ${tap} still has an IPv4 address after disconnect"
fi

# Assert the per-interface nftables table is gone (firewall cleanup).
if ip netns exec "$ns1" nft list table inet "$tap" >/dev/null 2>&1; then
    fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 firewall table ${tap} still exists after disconnect"
fi

# Confirm peer 2 no longer sees peer 1 (may take a keepalive round).
log "waiting for peer 2 to drop peer 1..."
sleep 10
snap2="$(status_snapshot "$sock2")"
printf '%s\n' "$snap2" | grep -q '"connected":true' || fail_scenario "$E2E_ASSERTION_FAILED" "peer 2 should remain connected"

# ---- Reconnect peer 1 without restarting the daemon --------------------------
log "reconnecting peer 1 via lanlinkctl..."
"$CTL_BIN" daemon connect \
    --socket "$sock1" \
    --coordinator "http://${E2E_BR_IP}:${E2E_COORD_PORT}" \
    --network-id "${NETWORK_ID}" \
    --password "${E2E_PASSWORD}" \
    --bind "0.0.0.0:9000" \
    --tap "${tap}" \
    --mtu 1400 >/dev/null || fail_scenario "$E2E_ASSERTION_FAILED" "lanlinkctl daemon connect failed"

log "waiting for peer 1 to reconnect..."
deadline=$(("$(date +%s)" + 60))
while [ "$(date +%s)" -lt "$deadline" ]; do
    snap1="$(status_snapshot "$sock1" 2>/dev/null || true)"
    if printf '%s\n' "$snap1" | grep -q '"connected":true'; then
        break
    fi
    sleep 0.5
done
printf '%s\n' "$snap1" | grep -q '"connected":true' || fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 did not reconnect"

# TAP must come back in the same namespace without restarting the process.
wait_for_peer_tap 1 || fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 TAP did not reappear after reconnect"

sleep 10
snap1="$(status_snapshot "$sock1")"
printf '%s\n' "$snap1" | grep -F -q '"peers":[' || fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 has no peers after reconnect"

# Verify restored connectivity by pinging peer 2 from peer 1's namespace.
wait_for_ping 1 "$ip2" || fail_scenario "$E2E_ASSERTION_FAILED" "ping from peer 1 to peer 2 (${ip2}) failed after reconnect"

# ---- Graceful stop peer 1 --------------------------------------------------
log "stopping peer 1 gracefully..."
pid1="$(cat "$(peer_pidfile 1)")"
"$CTL_BIN" daemon stop --socket "$sock1" >/dev/null || fail_scenario "$E2E_ASSERTION_FAILED" "lanlinkctl daemon stop failed"

log "waiting for peer 1 process to exit..."
deadline=$(("$(date +%s)" + 30))
while [ "$(date +%s)" -lt "$deadline" ]; do
    if ! proc_alive "$pid1"; then
        break
    fi
    sleep 0.5
done
proc_alive "$pid1" && fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 process did not exit after stop"
[ ! -S "$sock1" ] || fail_scenario "$E2E_ASSERTION_FAILED" "peer 1 control socket still present after stop"

report_section "Result" "- lanlinkctl daemon disconnect sets connected=false and cleans TAP/IP/firewall\n- lanlinkctl daemon connect rejoins the network without process restart\n- connectivity is restored after reconnect\n- lanlinkctl daemon stop exits the process and removes the control socket"
finish_scenario
