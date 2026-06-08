# build-connector.ps1
# Run this on a Windows machine with Rust + Node.js installed.
# Produces franchisetechConnectorSetup.exe and uploads it to production.
#
# Prerequisites:
#   winget install Rustlang.Rustup
#   winget install OpenJS.NodeJS
#   rustup target add x86_64-pc-windows-msvc
#
# Usage:
#   .\connector\build-connector.ps1
#   .\connector\build-connector.ps1 -Deploy -Host franchisetech.ro -User root

param(
    [switch]$Deploy,
    [string]$Host = "franchisetech.ro",
    [string]$User = "root",
    [string]$RemotePath = "/var/www/fridgeproof/public/downloads/franchisetechConnectorSetup.exe"
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

Write-Host "=== franchisetech Connector 0.3.0 Build ===" -ForegroundColor Cyan
Write-Host "Working directory: $ScriptDir"

# 1. Run Rust tests
Write-Host "`n[1/4] Running Rust tests..." -ForegroundColor Yellow
Push-Location "$ScriptDir"
cargo test --release
if ($LASTEXITCODE -ne 0) { throw "Rust tests failed" }
Pop-Location

# 2. Install Node deps
Write-Host "`n[2/4] Installing Node.js dependencies..." -ForegroundColor Yellow
Push-Location "$ScriptDir\desktop"
npm ci
if ($LASTEXITCODE -ne 0) { throw "npm ci failed" }

# 3. Build Tauri NSIS installer
Write-Host "`n[3/4] Building Tauri NSIS installer..." -ForegroundColor Yellow
npx tauri build --target x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { throw "Tauri build failed" }

# 4. Locate and rename installer
Write-Host "`n[4/4] Locating installer..." -ForegroundColor Yellow
$InstallerDir = "src-tauri\target\x86_64-pc-windows-msvc\release\bundle\nsis"
$SourceExe = Get-ChildItem -Path $InstallerDir -Filter "*.exe" | Select-Object -First 1
if (-not $SourceExe) { throw "Installer not found in $InstallerDir" }

Write-Host "Found: $($SourceExe.FullName)"
$OutPath = "$ScriptDir\desktop\franchisetechConnectorSetup.exe"
Copy-Item $SourceExe.FullName $OutPath -Force

# SHA-256
$Hash = (Get-FileHash $OutPath -Algorithm SHA256).Hash.ToLower()
Write-Host "`nInstaller: $OutPath"
Write-Host "SHA-256:   $Hash"
Write-Host "Size:      $([math]::Round((Get-Item $OutPath).Length / 1MB, 2)) MB"

Pop-Location

# Optional: deploy to production
if ($Deploy) {
    Write-Host "`nDeploying to $User@$Host`:$RemotePath ..." -ForegroundColor Yellow
    scp $OutPath "${User}@${Host}:${RemotePath}"
    if ($LASTEXITCODE -ne 0) { throw "scp deploy failed" }
    Write-Host "Deployed. Updating manifest..." -ForegroundColor Green
    $Script = @"
python3 -c "
import json, hashlib
sha = hashlib.sha256(open('$RemotePath','rb').read()).hexdigest()
m = json.load(open('/var/www/fridgeproof/public/downloads/franchisetech-connector-version.json'))
m['version'] = '0.3.0'
m['binaryVersion'] = '0.3.0'
m['notes'] = 'Beta 0.3.0 - guided pairing, setup wizard, hardware verification, printer discovery, autostart.'
m['platforms']['windows']['sha256'] = sha
open('/var/www/fridgeproof/public/downloads/franchisetech-connector-version.json','w').write(json.dumps(m,indent=2))
print('manifest updated sha=' + sha[:16])
"
"@
    ssh "${User}@${Host}" $Script
}

Write-Host "`n=== Build complete ===" -ForegroundColor Green
Write-Host "SHA-256: $Hash"
Write-Host ""
Write-Host "Next steps:"
Write-Host "  1. Test on a clean Windows machine"
Write-Host "  2. Confirm app shows version 0.3.0 and 'franchisetech Connector'"
Write-Host "  3. Run: .\build-connector.ps1 -Deploy"
