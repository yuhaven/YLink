<#
.SYNOPSIS
    Build a portable CastPane folder and zip.

.DESCRIPTION
    Produces dist/CastPane/ containing the app, every plugin, and any vendored
    engines, plus dist/CastPane-v<version>-win64.zip.

    In release builds the app looks for plugins next to its own executable,
    so this layout is what makes the portable copy self-contained.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\package.ps1
    powershell -ExecutionPolicy Bypass -File scripts\package.ps1 -SkipBuild
#>
[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [string]$RepoRoot
)

if (-not $RepoRoot) {
    $RepoRoot = Split-Path -Parent $PSScriptRoot
}

$ErrorActionPreference = 'Stop'
Push-Location $RepoRoot
try {
    if (-not $SkipBuild) {
        & cargo build --release -p castpane -p castpane-plugin-android
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed with $LASTEXITCODE" }
    }

    $confPath = Join-Path $RepoRoot 'apps/desktop/src-tauri/tauri.conf.json'
    $version = (Get-Content $confPath -Raw | ConvertFrom-Json).version

    $dist = Join-Path $RepoRoot 'dist/CastPane'
    if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
    New-Item -ItemType Directory -Path $dist -Force | Out-Null

    # App
    $app = Join-Path $RepoRoot 'target/release/castpane.exe'
    if (-not (Test-Path $app)) { throw "app binary not found: $app" }
    Copy-Item $app (Join-Path $dist 'CastPane.exe')

    $cli = Join-Path $RepoRoot 'target/release/castpane-cli.exe'
    if (Test-Path $cli) {
        Copy-Item $cli (Join-Path $dist 'CastPane-cli.exe')
    }

    # Plugins: manifest + plugin binary + whatever engines are vendored.
    foreach ($id in @('android', 'airplay')) {
        $src = Join-Path $RepoRoot "plugins/$id"
        if (-not (Test-Path $src)) { continue }
        $dst = Join-Path $dist "plugins/$id"
        New-Item -ItemType Directory -Path $dst -Force | Out-Null
        Copy-Item (Join-Path $src 'plugin.json') $dst

        $binSrc = Join-Path $src 'bin'
        if (Test-Path $binSrc) {
            Copy-Item $binSrc -Destination $dst -Recurse -Force
        }

        $built = Join-Path $RepoRoot "target/release/$id-plugin.exe"
        if (Test-Path $built) {
            $binDir = Join-Path $dst 'bin'
            New-Item -ItemType Directory -Path $binDir -Force | Out-Null
            Copy-Item $built $binDir -Force
        }
    }

    Copy-Item (Join-Path $RepoRoot 'LICENSE') $dist
    Copy-Item (Join-Path $RepoRoot 'README.md') $dist

    $zip = Join-Path $RepoRoot "dist/CastPane-v$version-win64.zip"
    if (Test-Path $zip) { Remove-Item $zip -Force }
    Compress-Archive -Path "$dist/*" -DestinationPath $zip -CompressionLevel Optimal

    Write-Host ''
    Write-Host "portable folder : $dist"
    Write-Host "zip             : $zip ($([math]::Round((Get-Item $zip).Length/1MB,1)) MB)"
    Write-Host ''
    Write-Host 'Contents:'
    Get-ChildItem $dist -Recurse -File | ForEach-Object {
        "  " + $_.FullName.Replace($dist + '\', '')
    }
}
finally {
    Pop-Location
}
