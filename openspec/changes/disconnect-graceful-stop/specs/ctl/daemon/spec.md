# Spec Delta

## Purpose

Provide a CLI surface for local daemon control so users can check status, disconnect, reconnect, and stop the daemon without using signals or the service manager.

## ADDED Requirements

### Requirement: `lanlinkctl` supports a `daemon` subcommand group

`lanlinkctl` SHALL accept `daemon status`, `daemon disconnect`, `daemon connect`, and `daemon stop` subcommands. All commands SHALL communicate with the daemon's local control socket.

#### Scenario: Request daemon status
- **WHEN** the user runs `lanlinkctl daemon status`
- **THEN** it prints the daemon status snapshot or a clear offline message

#### Scenario: Disconnect from the current network
- **WHEN** the user runs `lanlinkctl daemon disconnect`
- **THEN** the daemon leaves the network and the command exits 0 after cleanup

#### Scenario: Reconnect to a network
- **WHEN** the user runs `lanlinkctl daemon connect --config /etc/lanlink/foo.toml`
- **THEN** the daemon joins the network described by the config without a process restart

#### Scenario: Stop the daemon gracefully
- **WHEN** the user runs `lanlinkctl daemon stop`
- **THEN** the daemon cleans up and exits, and the command returns after the process is gone
