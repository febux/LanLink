#!/usr/bin/env bash
# Two-node netns lab for TAP / L2 smoke tests.
#
# Usage: sudo ./tests/netns-lab.sh
set -euo pipefail

NODE_A="ll_a"
NODE_B="ll_b"
BRIDGE="ll_br"

cleanup() {
    ip netns del "${NODE_A}" 2>/dev/null || true
    ip netns del "${NODE_B}" 2>/dev/null || true
    ip link del "${BRIDGE}" 2>/dev/null || true
}
trap cleanup EXIT

echo "TODO: create netns topology with two NAT'd 'hosts' and a shared bridge/STUN server"
echo "Target: lanlinkd in A and B join the same network and can ping each other."
