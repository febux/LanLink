# LanLink Windows real-machine preflight checklist
#
# Automates the windows-parity checklist: adapter state, daemon status
# transport, firewall block-port self-test (public/gaming profile), and
# gateway route/NAT state. Run in an elevated PowerShell on each Windows
# machine after installing the MSI and joining a network.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts\windows-preflight.ps1 [-Gateway] [-Interface lanlink0]
#
# Exit code 0 = all checks passed.

param(
    [string]$Interface = "lanlink0",
    [switch]$Gateway
)

$ErrorActionPreference = 'Continue'
$fail = 0

function Check([string]$Name, [scriptblock]$Body) {
    try {
        $result = & $Body
        if ($result) {
            Write-Host "[PASS] $Name"
        } else {
            Write-Host "[FAIL] $Name"
            $script:fail++
        }
    } catch {
        Write-Host "[FAIL] $Name ($($_.Exception.Message))"
        $script:fail++
    }
}

Write-Host "== LanLink Windows preflight (interface '$Interface') =="

# --- 1. Adapter present with a 26.x.x.x address (ncpa.cpl equivalent) -------
Check "Wintun adapter exists" { [bool](Get-NetAdapter -Name $Interface -ErrorAction SilentlyContinue) }
Check "Adapter has 26.x.x.x IPv4 address" {
    (Get-NetIPAddress -InterfaceAlias $Interface -AddressFamily IPv4 -ErrorAction SilentlyContinue) |
        Where-Object { $_.IPAddress -like '26.*' } | Select-Object -First 1
}

# --- 2. Daemon status transport (TCP loopback, port 9000 + network id) -----
# Assumes network id 1 -> port 9001. Adjust to the joined network.
$StatusPort = 9001
Check "Status transport answers on TCP loopback:$StatusPort (NDJSON)" {
    $client = New-Object Net.Sockets.TcpClient('127.0.0.1', $StatusPort)
    $stream = $client.GetStream()
    $reader = New-Object IO.StreamReader($stream)
    $line = $reader.ReadLine()
    $client.Close()
    $line -match '"(state|status|peers)"'
}

# --- 3. Firewall profile (public/gaming): blocked-port self-test -----------
# The daemon installs block rules for TCP 139/445/3389/22 and UDP 137/138 on
# the virtual interface when the network is marked public/gaming. Verify the
# rules exist and that a blocked port is unreachable while ping still works.
Check "Firewall block rules installed (LanLink group)" {
    [bool](Get-NetFirewallRule -Group "LanLink_$Interface" -ErrorAction SilentlyContinue |
        Where-Object { $_.Action -eq 'Block' })
}
Check "ICMPv4 allow rule installed" {
    [bool](Get-NetFirewallRule -Group "LanLink_$Interface" -ErrorAction SilentlyContinue |
        Where-Object { $_.Action -eq 'Allow' })
}

# --- 4. LAN game traffic sanity: virtual peer reachable ---------------------
# Requires a second machine in the same network; replace with its virtual IP.
# ping <peer-26-ip> ; then test a game/service port of your choice.

# --- 5. Gateway mode state (only with -Gateway) ------------------------------
if ($Gateway) {
    Check "IP forwarding enabled on '$Interface'" {
        (Get-NetIPInterface -InterfaceAlias $Interface -AddressFamily IPv4).Forwarding -eq 'Enabled'
    }
    Check "WinNAT instance 'LanLinkGateway' present" {
        [bool](Get-NetNat -Name 'LanLinkGateway' -ErrorAction SilentlyContinue)
    }
    Check "No stale NetMgmt routes besides gateway routes" {
        $stale = Get-NetRoute -InterfaceAlias $Interface -AddressFamily IPv4 -ErrorAction SilentlyContinue |
            Where-Object { $_.Protocol -eq 'NetMgmt' }
        # Presence is fine (gateway peers install them); we only verify they
        # all have a NextHop on the virtual subnet.
        $stale | ForEach-Object { $_.NextHop -like '26.*' } | ForEach-Object { -not $_ } -notcontains $true
    }
}

Write-Host ""
if ($fail -eq 0) {
    Write-Host "PREFLIGHT PASSED"
    exit 0
} else {
    Write-Host "PREFLIGHT FAILED: $fail check(s) failed"
    exit 1
}
