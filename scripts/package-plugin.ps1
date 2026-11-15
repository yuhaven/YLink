<#
.SYNOPSIS
    Build a plugin and place its executable where the host expects it.

.DESCRIPTION
    The host loads plugins from plugins/<id>/plugin.json, whose `entry` is
    bin/<name>.exe. Cargo puts binaries in target/<profile>/, so this script
    copies them across.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\package-plugin.ps1 -Plugin android
    powershell -ExecutionPolicy Bypass -File scripts\package-plugin.ps1 -Plugin android -Release
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet('android', 'airplay')]
    [string]$Plugin,
    [switch]$Release,
    [string]$RepoRoot
)

if (-not $RepoRoot) {
    $RepoRoot = Split-Path -Parent $PSScriptRoot
}

$ErrorActionPreference = 'Stop'

$map = @{
    android = @{ Package = 'castpane-plugin-android'; Binary = 'android-plugin' }
    airplay = @{ Package = 'castpane-plugin-airplay'; Binary = 'airplay-plugin' }
}

$spec = $map[$Plugin]
$profileDir = if ($Release) { 'release' } else { 'debug' }
$cargoArgs = @('build', '-p', $spec.Package)
if ($Release) { $cargoArgs += '--release' }

Push-Location $RepoRoot
try {
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with $LASTEXITCODE" }

    $src = Join-Path $RepoRoot "target/$profileDir/$($spec.Binary).exe"
    if (-not (Test-Path $src)) { throw "binary not found: $src" }

    $destDir = Join-Path $RepoRoot "plugins/$Plugin/bin"
    New-Item -ItemType Directory -Path $destDir -Force | Out-Null
    $dest = Join-Path $destDir "$($spec.Binary).exe"
    Copy-Item $src $dest -Force

    Write-Host "installed $src"
    Write-Host "        -> $dest"
}
finally {
    Pop-Location
}
