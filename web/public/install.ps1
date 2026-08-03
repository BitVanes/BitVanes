# BitVanes installer — Windows (PowerShell).
# Usage:  irm https://bitvanes.com/install.ps1 | iex
#
# Downloads the latest bitvanes-x86_64-windows.zip from GitHub Releases and
# extracts it to ~/AppData/Local/BitVanes, then prints the PATH guidance.

$ErrorActionPreference = 'Stop'

$repo = 'BitVanes/releases'
$api = "https://api.github.com/repos/$repo/releases/latest"
$assetName = 'bitvanes-x86_64-windows.zip'

Write-Host 'Fetching latest release…' -ForegroundColor White
$release = Invoke-RestMethod -Uri $api -Headers @{ Accept = 'application/vnd.github+json' }

$asset = $release.assets | Where-Object { $_.name -eq $assetName } | Select-Object -First 1
if (-not $asset) {
    Write-Host "Asset '$assetName' not found in the latest release." -ForegroundColor Red
    Write-Host 'Build from source: https://github.com/'$repo'#readme' -ForegroundColor Red
    exit 1
}

$tmp = New-Item -ItemType Directory -Force -Path (Join-Path $env:TEMP "bitvanes-install-$(Get-Random)")
$zip = Join-Path $tmp $assetName

Write-Host "Downloading $assetName…" -ForegroundColor White
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip

$installDir = Join-Path $env:LOCALAPPDATA 'BitVanes'
New-Item -ItemType Directory -Force -Path $installDir | Out-Null

Write-Host "Extracting to $installDir" -ForegroundColor White
Expand-Archive -Path $zip -DestinationPath $installDir -Force

$exe = Join-Path $installDir 'bitvanes.exe'
if (Test-Path $exe) {
    Write-Host "Installed: $exe" -ForegroundColor Green
} else {
    Write-Host "Extracted to $installDir" -ForegroundColor Green
}

# PATH guidance.
$pathHasIt = ($env:Path -split ';') -contains $installDir
if (-not $pathHasIt) {
    Write-Host ''
    Write-Host "Add to PATH for this session:" -ForegroundColor Yellow
    Write-Host "    `$env:Path += ';$installDir'" -ForegroundColor Gray
    Write-Host "Or persist (run as admin / user):" -ForegroundColor Yellow
    Write-Host "    [Environment]::SetEnvironmentVariable('Path', `$env:Path + ';$installDir', 'User')" -ForegroundColor Gray
}

Write-Host ''
Write-Host 'Windows: if SmartScreen prompts, choose "More info" then "Run anyway".' -ForegroundColor Yellow
Write-Host 'Run:  bitvanes --help' -ForegroundColor White
