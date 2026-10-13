#!/usr/bin/env bash
# M1 smoke test: two Linux network namespaces connected by an encrypted virtual LAN.
# This script builds the daemon and verifies that a ping from node A to node B flows
# over the encrypted UDP tunnel.
#
# Usage: sudo ./tests/m1_ping_lab.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
DAEMON="${REPO_DIR}/target/debug/lanlinkd"

NODE_A="ll_a"
NODE_B="ll_b"
VETH_A="veth_a"
VETH_B="veth_b"
NET_PREFIX="26.0.0"

PASSWORD="m1-smoke-password"
NETWORK_ID="1"

cleanup() {
    set +e
    pkill -f "lanlinkd static-peer" 2>/dev/null || true
    ip netns del "${NODE_A}" 2>/dev/null || true
    ip netns del "${NODE_B}" 2>/dev/null || true
    ip link del "${VETH_A}" 2>/dev/null || true
}
trap cleanup EXIT

if [ "$(id -u)" -ne 0 ]; then
    echo "This test must run as root (CAP_NET_ADMIN required for TAP)." >&2
    exit 1
fi

echo "[m1] building daemon..."
cd "${REPO_DIR}"
cargo build --bin lanlinkd

echo "[m1] creating network namespaces..."
ip netns add "${NODE_A}"
ip netns add "${NODE_B}"

ip link add "${VETH_A}" type veth peer name "${VETH_B}"
ip link set "${VETH_A}" netns "${NODE_A}"
ip link set "${VETH_B}" netns "${NODE_B}"

ip netns exec "${NODE_A}" ip addr add 10.200.0.1/24 dev "${VETH_A}"
ip netns exec "${NODE_A}" ip link set "${VETH_A}" up
ip netns exec "${NODE_A}" ip link set lo up

ip netns exec "${NODE_B}" ip addr add 10.200.0.2/24 dev "${VETH_B}"
ip netns exec "${NODE_B}" ip link set "${VETH_B}" up
ip netns exec "${NODE_B}" ip link set lo up

echo "[m1] starting lanlinkd in namespace ${NODE_A}..."
ip netns exec "${NODE_A}" \
    "${DAEMON}" static-peer \
    --tap ll0 \
    --local-ip "${NET_PREFIX}.1" \
    --prefix 8 \
    --bind 10.200.0.1:9000 \
    --peer-addr 10.200.0.2:9001 \
    --node-id 1 \
    --peer-node-id 2 \
    --password "${PASSWORD}" \
    --network-id "${NETWORK_ID}" \
    &
A_PID=$!

echo "[m1] starting lanlinkd in namespace ${NODE_B}..."
ip netns exec "${NODE_B}" \
    "${DAEMON}" static-peer \
    --tap ll0 \
    --local-ip "${NET_PREFIX}.2" \
    --prefix 8 \
    --bind 10.200.0.2:9001 \
    --peer-addr 10.200.0.1:9000 \
    --node-id 2 \
    --peer-node-id 1 \
    --password "${PASSWORD}" \
    --network-id "${NETWORK_ID}" \
    &
B_PID=$!

echo "[m1] waiting for TAP interfaces to come up..."
sleep 2

echo "[m1] pinging ${NET_PREFIX}.2 from namespace ${NODE_A}..."
if ip netns exec "${NODE_A}" ping -c 3 -W 5 "${NET_PREFIX}.2"; then
    echo "[m1] SUCCESS: encrypted two-node ping works"
else
    echo "[m1] FAILURE: ping did not succeed" >&2
    # Show daemon logs if any are buffered? They go to the terminal; user can re-run with RUST_LOG.
    exit 1
fi

echo "[m1] stopping daemons..."
kill "${A_PID}" "${B_PID}" 2>/dev/null || true
wait "${A_PID}" "${B_PID}" 2>/dev/null || true

# cleanup is handled by trap
echo "[m1] done"
