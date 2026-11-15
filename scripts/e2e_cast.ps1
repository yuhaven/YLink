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
    [int]$WaitSeconds = 6,
    [switch]$SkipDrag,
    # Write settings.json before launch instead of driving the dropdown.
    # WebView2's native <select> popup is not reachable by UIA when the card is
    # scrolled out of view, so the core chain is verified this way.
    [switch]$PresetSettings
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'dist\CastPane\CastPane.exe'

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--force-renderer-accessibility'

# Precondition: without an attached device there is nothing to cast, and the
# test would otherwise report a misleading "no cast window".
$adb = Join-Path $root 'plugins\android\bin\scrcpy\adb.exe'
if (Test-Path $adb) {
    $devices = & $adb devices 2>&1 | Select-String -Pattern '\sdevice$'
    if (-not $devices) {
        Write-Host 'PRECONDITION FAILED: no Android device in "adb devices".'
        Write-Host 'Connect the phone, enable USB debugging, authorise it, then rerun.'
        exit 2
    }
}

Get-Process castpane, scrcpy, android-plugin, msedgewebview2 -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue
# A stale settings.json would silently change the outcome; start from defaults.
Remove-Item (Join-Path $root 'dist\CastPane\settings.json') -ErrorAction SilentlyContinue
if ($PresetSettings) {
    $preset = @{
        android = @{
            maxSize          = 1080
            maxFps           = 60
            audio            = $true
            stayAwake        = $true
            lockOrientation  = $true
            lockPosition     = $false
            orientation      = $Orientation
        }
    } | ConvertTo-Json -Depth 4
    # Write without a BOM: serde_json rejects a leading BOM, and Set-Content
    # -Encoding UTF8 adds one on Windows PowerShell 5.1.
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText(
        (Join-Path $root 'dist\CastPane\settings.json'), $preset, $utf8NoBom)
}
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
        $valuePattern = $combo.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
        $startValue = $valuePattern.Current.Value
        Write-Host "combo initial value: $startValue"

        if ($PresetSettings) {
            # The UI must reflect what was loaded from the file.
            $result.combo_changed = ($startValue -eq $Orientation)
            if (-not $result.combo_changed) {
                Write-Host "combo shows '$startValue' instead of '$Orientation'"
            }
        }
        else {
        $rect = $combo.Current.BoundingRectangle
        $cx = [int]($rect.X + $rect.Width / 2)
        $cy = [int]($rect.Y + $rect.Height / 2)
        Add-Type -AssemblyName System.Windows.Forms
        [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point($cx, $cy)
        Start-Sleep -Milliseconds 200
        Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern void mouse_event(int f, int dx, int dy, int c, int e);' -Name U -Namespace W
        [W.U]::mouse_event(0x0002, 0, 0, 0, 0)
        [W.U]::mouse_event(0x0004, 0, 0, 0, 0)
        Start-Sleep -Milliseconds 500

        for ($i = 0; $i -lt 4; $i++) {
            if ($valuePattern.Current.Value -eq $Orientation) { break }
            [System.Windows.Forms.SendKeys]::SendWait('{DOWN}')
            Start-Sleep -Milliseconds 350
        }
        [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
        Start-Sleep -Milliseconds 700
        $now = $valuePattern.Current.Value
        $result.combo_changed = ($now -eq $Orientation)
        if (-not $result.combo_changed) { Write-Host "combo stayed at '$now'" }
        }
    }

    # Wait for the device list to settle, then press the Android cast button by
    # name. The label is built from code points because this file must stay
    # ASCII (Windows PowerShell 5.1 reads scripts as ANSI).
    $castLabel = [string][char]0x6295 + [char]0x5C4F   # "cast"
    $btnCond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::Button)

    $castBtn = $null
    $names = @()
    $deadline = (Get-Date).AddSeconds(20)
    while ((Get-Date) -lt $deadline) {
        $names = @()
        $castBtn = $null
        foreach ($b in $win.FindAll([System.Windows.Automation.TreeScope]::Descendants, $btnCond)) {
            $names += ("{0}({1})" -f $b.Current.Name, $b.Current.IsEnabled)
            if ($b.Current.Name -eq $castLabel -and $b.Current.IsEnabled) { $castBtn = $b }
        }
        if ($castBtn) { break }
        Start-Sleep -Milliseconds 800
    }
    Write-Host ("buttons: " + ($names -join ', '))
    if ($castBtn) {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $castBtn.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        $result.cast_clicked = $true

        # The UI must stay responsive: the window should show up quickly even
        # while the plugin is starting adb and scrcpy.
        $appeared = $false
        while ($sw.Elapsed.TotalSeconds -lt 15) {
            Start-Sleep -Milliseconds 300
            $probe = & python (Join-Path $PSScriptRoot 'e2e_probe.py') measure
            if ($probe -and $probe -ne 'null') { $appeared = $true; break }
        }
        $result.time_to_window = [math]::Round($sw.Elapsed.TotalSeconds, 1)
        if (-not $appeared) { throw 'cast window never appeared' }
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
    if (-not $result.window.caption) {
        Write-Host 'FAIL: cast window has no title bar'; $ok = $false
    }
    if (-not $result.window.minimize_box -or -not $result.window.maximize_box) {
        Write-Host 'FAIL: cast window lacks minimise/maximise buttons'; $ok = $false
    }
    if ($result.time_to_window -gt 5) {
        Write-Host "FAIL: UI took $($result.time_to_window)s to show the window (probably blocked)"
        $ok = $false
    }
}
if (-not $SkipDrag) {
    if (-not $result.drag -or -not $result.drag.moved) {
        Write-Host 'FAIL: window did not move when dragged'; $ok = $false
    }
}
if ($ok) { Write-Host 'PASS'; exit 0 } else { exit 1 }
