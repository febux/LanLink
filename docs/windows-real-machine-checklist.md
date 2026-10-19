# Windows real-machine checklist (windows-parity 3.2)

The automated portion of this checklist lives in
[`scripts/windows-preflight.ps1`](../scripts/windows-preflight.ps1). Run it
elevated on **both** Windows machines after installing the MSI and joining the
same network:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\windows-preflight.ps1
# on the machine acting as gateway, additionally:
powershell -ExecutionPolicy Bypass -File scripts\windows-preflight.ps1 -Gateway
```

Manual steps that need two physical PCs (single checklist run, ~15 min):

| # | Step | Pass criteria |
|---|------|---------------|
| 1 | Clean-install the MSI on a fresh Windows VM/PC | `lanlinkd` service starts, `sc query lanlink` reports RUNNING, node joins the network |
| 2 | Open `ncpa.cpl` | Adapter `lanlink0` present with the expected `26.x.x.x` address (preflight check 1) |
| 3 | TUI/Web/Desktop status | `lanlink-tui` (or the web UI) shows peers and daemon status on Windows — TCP loopback status transport |
| 4 | Ping both directions | `ping <peer-26-ip>` works from both PCs; `ping <gateway-LAN-host>` works from the remote peer through the gateway node |
| 5 | Firewall self-test (public/gaming network) | Preflight check 3 passes: LanLink rule group has block rules for TCP 139/445/3389/22 + UDP 137/138; ping still works; `Test-NetConnection <peer-26-ip> -Port 445` fails while a game port succeeds |
| 6 | Gateway mode | On the gateway node: `Get-NetNat` shows `LanLinkGateway` with the virtual `/24` prefix, `Get-NetIPInterface -Forwarding` is Enabled on `lanlink0`; the remote peer reaches a physical-LAN host by IP (e.g. RDP/SSH/SMB to `192.168.1.x`) through the gateway |
| 7 | IP-based LAN game | Start an IP-based LAN game on both PCs using the `26.x.x.x` addresses; connect and play one round |
| 8 | Clean shutdown | Leave the network / stop the service: firewall rule group removed, NetNAT instance removed, no stale `NetMgmt` routes on `lanlink0` |

Known limitation (per Wintun, layer-3): raw Ethernet/ARP/broadcast discovery
is not available on Windows; only IP-based game discovery works.

Hardware run status: **not yet executed** — this checklist (with the preflight
script) is the packaged deliverable of windows-parity task 3.2; record the
actual run results in this table when performed.
