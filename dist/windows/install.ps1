# One-line installer for LanLink on Windows.
#
# Run in an elevated PowerShell:
#   irm https://raw.githubusercontent.com/febux/LanLink/master/dist/windows/install.ps1 | iex
#
# Parameters (via environment variables because iex can't pass args cleanly):
#   $env:LANLINK_VERSION = "0.3.1"   # default: latest release
#   $env:LANLINK_GUI = "desktop"     # none | tui | web | desktop | all (default: desktop)
#
# This script downloads the latest MSI from GitHub Releases and installs it
# silently. It then prints the next steps.
param(
    [string]$Version = $env:LANLINK_VERSION,
    [string]$Gui = $env:LANLINK_GUI
)

$Repo = $env:LANLINK_REPO
if (-not $Repo) { $Repo = "febux/LanLink" }
if (-not $Gui) { $Gui = "desktop" }

function Die($msg) {
    Write-Host "[lanlink-install] ERROR: $msg" -ForegroundColor Red
    exit 1
}

function Log($msg) {
    Write-Host "[lanlink-install] $msg"
}

# Requires elevation.
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Die "please run this script as Administrator"
}

# Resolve version.
if (-not $Version) {
    Log "resolving latest release from GitHub"
    $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -UseBasicParsing
    $Version = $releases.tag_name.TrimStart('v')
}

Log "installing LanLink $Version (gui=$Gui)"

$arch = "x64"
$msi = "lanlink-$Version-$arch.msi"
$url = "https://github.com/$Repo/releases/download/v$Version/$msi"
$tmp = New-TemporaryFile
$msiPath = "$tmp.msi"

Log "downloading $url"
try {
    Invoke-WebRequest -Uri $url -OutFile $msiPath -UseBasicParsing
} catch {
    Die "failed to download $url : $_"
}

Log "installing MSI silently"
$proc = Start-Process -FilePath msiexec.exe -ArgumentList "/i", "$msiPath", "/quiet", "/norestart" -Wait -PassThru
if ($proc.ExitCode -ne 0) {
    Die "MSI install failed with exit code $($proc.ExitCode)"
}

Remove-Item -Force $msiPath -ErrorAction SilentlyContinue

Log "installation complete"
Log "next steps:"
Log "  1. Start a network:  lanlinkd service up --name my-net --password secret"
Log "  2. Or open the desktop app from the Start Menu: LanLink"
Log "  3. Or open the WebUI: lanlink-web"
