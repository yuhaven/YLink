<#
.SYNOPSIS
    Vendor the casting engines into plugins/*/bin.

.DESCRIPTION
    Downloads pinned Windows builds of scrcpy and uxplay-windows and unpacks
    them where the plugins expect their engines. Binaries are not committed
    (see .gitignore); run this once after cloning.

    Versions and SHA-256 digests are pinned so two clones get byte-identical
    engines. GitHub API is deliberately not used: it fails TLS negotiation on
    some Windows PowerShell 5.1 setups.

    Execution policy may block this file. If so:
        powershell -ExecutionPolicy Bypass -File scripts\fetch-engines.ps1

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\fetch-engines.ps1 -Only android
#>
[CmdletBinding()]
param(
    [ValidateSet('all', 'android', 'airplay')]
    [string]$Only = 'all',
    [string]$RepoRoot
)

if (-not $RepoRoot) {
    $RepoRoot = Split-Path -Parent $PSScriptRoot
}

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

# Pinned engines. Update the digest together with the version.
$Engines = @{
    scrcpy = @{
        Label    = 'scrcpy'
        Version  = 'v5.0.1'
        Url      = 'https://github.com/Genymobile/scrcpy/releases/download/v5.0.1/scrcpy-win64-v5.0.1.zip'
        Sha256   = 'B12A2C4EE8BE317422451FC7DC1F8EE20A71B5EA7EF9AD73DDD825A25316227A5'
        Dest     = 'plugins/android/bin/scrcpy'
    }
    uxplay = @{
        Label    = 'uxplay-windows'
        Version  = '2.0.0.1736'
        Url      = 'https://github.com/leapbtw/uxplay-windows/releases/download/2.0.0.1736/uxplay-windows.zip'
        Sha256   = '9D3A51C15FC9DB857351195E7EB7BBB21700D9AE25D936A54BCF8536B62CCA18'
        Dest     = 'plugins/airplay/bin/uxplay'
    }
}

function Install-Engine {
    param([hashtable]$Spec)

    Write-Host "==> $($Spec.Label) $($Spec.Version)"
    $dest = Join-Path $RepoRoot $Spec.Dest
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("castpane-" + [guid]::NewGuid().ToString('N') + ".zip")

    try {
        Invoke-WebRequest -Uri $Spec.Url -OutFile $tmp -TimeoutSec 900 -UseBasicParsing
        $actual = (Get-FileHash -Path $tmp -Algorithm SHA256).Hash
        if ($actual -ne $Spec.Sha256) {
            throw "sha256 mismatch for $($Spec.Label): expected $($Spec.Sha256), got $actual"
        }
        Write-Host "    digest ok"

        if (Test-Path $dest) { Remove-Item $dest -Recurse -Force }
        New-Item -ItemType Directory -Path $dest -Force | Out-Null
        Expand-Archive -Path $tmp -DestinationPath $dest -Force

        # Flatten a single top-level directory so paths are predictable.
        $children = Get-ChildItem $dest
        if ($children.Count -eq 1 -and $children[0].PSIsContainer) {
            $inner = $children[0].FullName
            Get-ChildItem $inner -Force | Move-Item -Destination $dest -Force
            Remove-Item $inner -Recurse -Force
        }
        Write-Host "    installed -> $dest"
    }
    finally {
        if (Test-Path $tmp) { Remove-Item $tmp -Force }
    }
}

if ($Only -in @('all', 'android')) { Install-Engine -Spec $Engines.scrcpy }
if ($Only -in @('all', 'airplay')) { Install-Engine -Spec $Engines.uxplay }

Write-Host ''
Write-Host 'Done. Engines live in plugins/*/bin/ (git-ignored).'
