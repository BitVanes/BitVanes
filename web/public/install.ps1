# BitVanes installer — Windows (PowerShell).
#
#   irm https://bitvanes.com/install.ps1 | iex
#
# Downloads the latest release for Windows from BitVanes/releases into
# %USERPROFILE%\.bitvanes and adds it to the user PATH. The bundle is
# self-contained (NER model + onnxruntime.dll + pdfium.dll included).
# For Tier-2 name redaction, also run `bitvanes-nerd` (and set
# BITVANES_LICENSE_KEY for paid features).

$ErrorActionPreference = 'Stop'

$Prefix = if ($env:BITVANES_INSTALL_PREFIX) { $env:BITVANES_INSTALL_PREFIX } else { Join-Path $env:USERPROFILE '.bitvanes' }
$Asset = 'bitvanes-x86_64-windows.zip'

Write-Host 'bitvanes: resolving latest release... ' -NoNewline
$api = Invoke-RestMethod -Uri 'https://api.github.com/repos/BitVanes/releases/releases/latest'
$url = ($api.assets | Where-Object { $_.name -eq $Asset }).browser_download_url
if (-not $url) { throw "could not find $Asset in the latest BitVanes release" }
Write-Host 'ok'

$tmp = New-Item -ItemType Directory -Force -Path (Join-Path $env:TEMP "bv-install-$(Get-Random)")
Write-Host "bitvanes: downloading $Asset... " -NoNewline
Invoke-WebRequest -Uri $url -OutFile (Join-Path $tmp.FullName $Asset)
Write-Host 'ok'

New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
Expand-Archive -Path (Join-Path $tmp.FullName $Asset) -DestinationPath $Prefix -Force
Write-Host "bitvanes: installed to $Prefix"

# Add to user PATH (idempotent).
$path = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($path -notlike "*$Prefix*") {
    [Environment]::SetEnvironmentVariable('Path', "$Prefix;$path", 'User')
    Write-Host "bitvanes: added $Prefix to user PATH (restart your terminal)"
}

Write-Host @"

bitvanes: next steps (reopen your terminal first):
  bitvanes --help            # the CLI (scrub / filter / daemon / tui)
  Start-Process bitvanes-nerd  # the Tier-2 NER sidecar (run for name redaction)

Tier-2 NER (personal names, orgs, locations) needs the sidecar running AND a
paid license key in BITVANES_LICENSE_KEY. Without it, BitVanes still fully
redacts email, SSN, phone, credit card, routing numbers, API keys, JWTs.
"@
