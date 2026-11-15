<#
.SYNOPSIS
    Vendor the casting engines into plugins/*/bin.

.DESCRIPTION
    Downloads the latest Windows builds of scrcpy and uxplay-windows and
    unpacks them where the plugins expect their engines. Binaries are not
    committed (see .gitignore); run this once after cloning.

    PowerShell execution policy may block this file. If so:
        powershell -ExecutionPolicy Bypass -File scripts\fetch-engines.ps1

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\fetch-engines.ps1
    powershell -ExecutionPolicy Bypass -File scripts\fetch-engines.ps1 -Only android
#>
[CmdletBinding()]
param(
    [ValidateSet('all', 'android', 'airplay')]
    [string]$Only = 'all',
    [string]$RepoRoot = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Get-LatestRelease([string]$Repo) {
    $api = "https://api.github.com/repos/$Repo/releases/latest"
    $headers = @{ 'User-Agent' = 'castpane-fetch' }
    Invoke-RestMethod -Uri $api -Headers $headers -TimeoutSec 60
}

function Install-Engine {
    param(
        [string]$Repo,
        [string]$AssetPattern,
        [string]$Destination,
        [string]$Label
    )

    Write-Host "==> $Label ($Repo)"
    $release = Get-LatestRelease $Repo
    $asset = $release.assets | Where-Object { $_.name -like $AssetPattern } | Select-Object -First 1
    if (-not $asset) {
        throw "no asset matching '$AssetPattern' in $($release.tag_name)"
    }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("castpane-" + [guid]::NewGuid().ToString('N') + ".zip")
    Write-Host "    downloading $($asset.name) ($([math]::Round($asset.size/1MB,1)) MB)"
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $tmp -TimeoutSec 900 -Headers @{ 'User-Agent' = 'castpane-fetch' }

    if (Test-Path $Destination) { Remove-Item $Destination -Recurse -Force }
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    Expand-Archive -Path $tmp -DestinationPath $Destination -Force
    Remove-Item $tmp -Force

    # Flatten a single top-level directory so bin/scrcpy/adb.exe (not bin/scrcpy/scrcpy-win64-x/adb.exe).
    $children = Get-ChildItem $Destination
    if ($children.Count -eq 1 -and $children[0].PSIsContainer) {
        $inner = $children[0].FullName
        Get-ChildItem $inner -Force | Move-Item -Destination $Destination -Force
        Remove-Item $inner -Recurse -Force
    }

    Write-Host "    installed $($release.tag_name) -> $Destination"
}

if ($Only -in @('all', 'android')) {
    Install-Engine -Repo 'Genymobile/scrcpy' `
        -AssetPattern 'scrcpy-win64-*.zip' `
        -Destination (Join-Path $RepoRoot 'plugins/android/bin/scrcpy') `
        -Label 'scrcpy'
}

if ($Only -in @('all', 'airplay')) {
    Install-Engine -Repo 'leapbtw/uxplay-windows' `
        -AssetPattern 'uxplay-windows.zip' `
        -Destination (Join-Path $RepoRoot 'plugins/airplay/bin/uxplay') `
        -Label 'uxplay-windows'
}

Write-Host ''
Write-Host 'Done. Engines are in plugins/*/bin/ (git-ignored).'
Write-Host 'Next: the plugin wrappers (bin/*-plugin.exe) are built from crates/plugin-* in a later step.'
