<#
.SYNOPSIS
    End-to-end check of the cast path through the real UI.

.DESCRIPTION
    Launches the packaged app, drives it with UI Automation (the same buttons a
    user clicks), then measures and drags the resulting cast window.

    ASCII-only on purpose: Windows PowerShell 5.1 reads .ps1 files as ANSI, so
    non-ASCII literals would be mangled and never match the UI.

    Buttons are located by position: the card order is AirPlay then Android, so
    the last enabled button is the Android "cast" action.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\e2e_cast.ps1 -Orientation landscape
#>
[CmdletBinding()]
param(
    [ValidateSet('portrait', 'landscape')]
    [string]$Orientation = 'landscape',
    [int]$WaitSeconds = 9,
    [switch]$SkipDrag
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'dist\CastPane\CastPane.exe'

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--force-renderer-accessibility'

Get-Process castpane, scrcpy, android-plugin, msedgewebview2 -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue
# A stale settings.json would silently change the outcome; start from defaults.
Remove-Item (Join-Path $root 'dist\CastPane\settings.json') -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 600

$app = Start-Process -FilePath $exe -PassThru
$result = [ordered]@{
    app_started   = $false
    orientation   = $Orientation
    combo_found   = $false
    combo_changed = $false
    cast_clicked  = $false
    window        = $null
    drag          = $null
}

function Get-AppWindow($processId, $timeoutSeconds = 25) {
    $rootElement = [System.Windows.Automation.AutomationElement]::RootElement
    $cond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $processId)
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        foreach ($candidate in $rootElement.FindAll(
                [System.Windows.Automation.TreeScope]::Children, $cond)) {
            # The process owns a few helper windows; wait for the one that
            # actually contains the web content.
            $kids = $candidate.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                [System.Windows.Automation.Condition]::TrueCondition)
            if ($kids.Count -gt 5) { return $candidate }
        }
        Start-Sleep -Milliseconds 500
    }
    return $null
}

function Find-OrientationCombo($win, $timeoutSeconds = 15) {
    $cond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::ComboBox)
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        # Card order is AirPlay then Android: the renderer combo comes first,
        # the orientation combo second.
        $combos = @($win.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond))
        if ($combos.Count -ge 2) {
            $value = $combos[1].GetCurrentPattern(
                [System.Windows.Automation.ValuePattern]::Pattern).Current.Value
            if ($value -in @('portrait', 'landscape', 'auto')) { return $combos[1] }
        }
        Start-Sleep -Milliseconds 500
    }
    return $null
}

try {
    $win = Get-AppWindow $app.Id
    if (-not $win) { throw 'app window not found via UIA' }
    $result.app_started = $true
    $all = $win.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.Condition]::TrueCondition)
    Write-Host "uia descendants=$($all.Count)"

    $combo = Find-OrientationCombo $win
    if ($combo) {
        $result.combo_found = $true
        $expand = $combo.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
        $expand.Expand()
        Start-Sleep -Milliseconds 700
        $itemCond = New-Object System.Windows.Automation.PropertyCondition(
            [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
            [System.Windows.Automation.ControlType]::ListItem)
        $items = $combo.FindAll([System.Windows.Automation.TreeScope]::Descendants, $itemCond)
        Write-Host ("combo items: " + (($items | ForEach-Object { $_.Current.Name }) -join ', '))
        foreach ($item in $items) {
            if ($item.Current.Name -eq $Orientation) {
                $item.GetCurrentPattern(
                    [System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
            }
        }
        try { $expand.Collapse() } catch { }
        Start-Sleep -Milliseconds 1200
        $now = $combo.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value
        $result.combo_changed = ($now -eq $Orientation)
        if (-not $result.combo_changed) { Write-Host "combo stayed at '$now'" }
    }

    # Wait for the device list to settle, then press the Android cast button by
    # name. The label is built from code points because this file must stay
    # ASCII (Windows PowerShell 5.1 reads scripts as ANSI).
    Start-Sleep -Seconds 5
    $castLabel = [string][char]0x6295 + [char]0x5C4F   # "cast"
    $btnCond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::Button)
    $castBtn = $null
    $names = @()
    foreach ($b in $win.FindAll([System.Windows.Automation.TreeScope]::Descendants, $btnCond)) {
        $names += ("{0}({1})" -f $b.Current.Name, $b.Current.IsEnabled)
        if ($b.Current.Name -eq $castLabel -and $b.Current.IsEnabled) { $castBtn = $b }
    }
    Write-Host ("buttons: " + ($names -join ', '))
    if ($castBtn) {
        $castBtn.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        $result.cast_clicked = $true
    } else {
        throw 'no enabled cast button (device not listed?)'
    }

    Start-Sleep -Seconds $WaitSeconds

    $json = & python (Join-Path $PSScriptRoot 'e2e_probe.py') measure
    $result.window = $json | ConvertFrom-Json

    if (-not $SkipDrag -and $result.window) {
        $dragJson = & python (Join-Path $PSScriptRoot 'e2e_probe.py') drag 160
        $result.drag = $dragJson | ConvertFrom-Json
    }
}
catch {
    $result.error = $_.Exception.Message
}
finally {
    Get-Process castpane, scrcpy, android-plugin, msedgewebview2 -ErrorAction SilentlyContinue |
        Stop-Process -Force -ErrorAction SilentlyContinue
}

$result | ConvertTo-Json -Depth 6

$ok = $true
if (-not $result.window) { Write-Host 'FAIL: no cast window'; $ok = $false }
else {
    $ratio = [double]$result.window.ratio
    if ($Orientation -eq 'landscape' -and $ratio -lt 1.2) {
        Write-Host "FAIL: expected landscape ratio, got $ratio"; $ok = $false
    }
    if ($Orientation -eq 'portrait' -and $ratio -gt 0.8) {
        Write-Host "FAIL: expected portrait ratio, got $ratio"; $ok = $false
    }
}
if (-not $SkipDrag) {
    if (-not $result.drag -or -not $result.drag.moved) {
        Write-Host 'FAIL: window did not move when dragged'; $ok = $false
    }
}
if ($ok) { Write-Host 'PASS'; exit 0 } else { exit 1 }
