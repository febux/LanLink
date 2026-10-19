# windows-gateway Specification

## Purpose
TBD - created by archiving change windows-parity. Update Purpose after archive.

## Requirements

### Requirement: Windows gateway mode

A gateway node on Windows SHALL share its physical LAN with the virtual
network by enabling IP forwarding on the virtual adapter and masquerading the
virtual subnet onto the physical network (WinNAT), mirroring the Linux
ip_forward + nftables MASQUERADE behavior.

#### Scenario: Remote peer reaches a physical-LAN host
- **WHEN** a non-gateway peer routes the gateway's shared subnet through the gateway's virtual IP
- **AND** sends IP traffic to a host on the gateway's physical LAN
- **THEN** the gateway forwards and masquerades the traffic so the LAN host's replies return to the peer

#### Scenario: Gateway state is cleaned up
- **WHEN** the gateway node disables gateway mode or the daemon stops
- **THEN** the NAT instance is removed and forwarding is disabled on the virtual adapter

### Requirement: Windows peer route management

On Windows, non-gateway peers SHALL install routes for gateway-shared subnets
through the gateway's virtual IP on the virtual adapter, and stale routes from
a previous run SHALL be removed at daemon startup.

#### Scenario: Routes follow gateway peers
- **WHEN** the coordinator advertises or withdraws gateway peers
- **THEN** the daemon adds or removes the corresponding routes on the virtual adapter

#### Scenario: Stale routes are flushed at startup
- **WHEN** the daemon starts and a previous crashed run left manually added routes on the virtual adapter
- **THEN** those routes are removed before the first route synchronization
