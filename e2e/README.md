# LanLink end-to-end scenarios

These shell scripts run real multi-peer topologies on a single Linux host. They
require **root** (`CAP_NET_ADMIN`) because they create network namespaces, veth
pairs, bridges, and TAP devices.

## Quick start

```bash
# Build release binaries first (the Makefile targets do this automatically).
cargo build --release -p lanlinkd -p lanlinkctl -p coordinator -p lanlink-relay

# Run one scenario with sudo.
sudo ./e2e/join.sh

# Or through the Makefile, which checks for root and rebuilds if needed.
sudo make e2e-join
```

## Topology

Each scenario creates:

* a host bridge (`ll_e2e_br` by default);
* a coordinator + relayd bound to the bridge address;
* one network namespace per peer, connected to the bridge via veth;
* a TAP device inside each namespace;
* release `lanlinkd` instances joined to a coordinator-created network.

All logs and a markdown report are written under `target/e2e/<scenario>.<pid>/`.

## Scenarios

| script        | what it checks                                        |
|---------------|--------------------------------------------------------|
| `join.sh`     | two peers join and get distinct virtual IPs            |
| `direct.sh`   | status snapshot reports `Direct` path after join       |
| `relay.sh`    | direct UDP blocked, ICMP flows via relay path          |
| `mac-switch.sh`| ARP + ICMP between peer TAPs, MAC-table unicast      |
| `pmtu.sh`     | large ICMP echo payloads are not corrupted             |
| `gateway.sh`  | a gateway peer routes a shared subnet to the LAN       |
| `status.sh`   | NDJSON status socket returns expected peers/paths      |
| `firewall.sh` | public-network firewall drops blocked TCP, allows ping |

## Requirements

* Linux with `iproute2`, `nftables` (for firewall scenarios), `socat` (for status
  socket queries), and `ping`.
* `sudo` or root shell.

## Notes

The scenarios are designed for a privileged self-hosted CI runner. If you are
running them interactively, inspect logs in `target/e2e/<scenario>.<pid>/` after
any failure.
