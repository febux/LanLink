# Testing LanLink

This document describes how to run the LanLink test suites locally.

## Rootless integration tests

The `integration_tests` crate lives in the workspace root and exercises the
crypto, switch, coordinator HTTP API, puncher, and relay crates without any
privileged networking. They run with normal `cargo test`:

```bash
# Run only the integration tests
cargo test -p integration_tests

# Run the full workspace test suite (integration tests are included; e2e code is not)
cargo test --workspace
```

No root privileges, network namespaces, or TAP devices are required.

## Privileged end-to-end scenarios

The `e2e/` directory contains shell scenarios that spin up real multi-peer
Linux topologies: network namespaces, veth pairs, a bridge, a coordinator, a
relay server, and TAP devices. They require **root** (`CAP_NET_ADMIN`) and are
opt-in through the Makefile targets.

Build the required release binaries first:

```bash
make e2e-build
```

Run a single scenario with `sudo`:

```bash
sudo make e2e-join
sudo make e2e-direct
sudo make e2e-relay
sudo make e2e-status
sudo make e2e-mac-switch
sudo make e2e-pmtu
sudo make e2e-gateway
sudo make e2e-firewall
```

Or run all scenarios:

```bash
sudo make e2e
```

Each scenario writes logs and a markdown report to
`target/e2e/<scenario>.<pid>/`.

### Requirements

* Linux with `iproute2` (`ip`, `ip netns`), `iptables`, `ping`, `socat` or `nc`.
* `sudo` or root shell.
* Release binaries built via `make e2e-build` (the `e2e-*` targets build them
  automatically if they are missing).

### Notes

* E2E scenarios do **not** run during `cargo test` or `make test`.
* The Makefile root guard prints an error if an E2E target is invoked without
  root.
* The relay and direct-path scenarios rely on the daemon's 30-second candidate
  refresh interval; each waits at least that long.
