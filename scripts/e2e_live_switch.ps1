<#
.SYNOPSIS
    Verify live orientation switching and re-casting after closing the window.

.DESCRIPTION
    The user's requirement: changing the orientation in the console must rotate
    the window that is already casting, without pressing cast again. And after
    closing the cast window, the cast button must become usable again.

    ASCII-only: Windows PowerShell 5.1 reads .ps1 as ANSI.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'dist\CastPane\CastPane.exe'

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms, System.Drawing
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--force-renderer-accessibility'

$adb = Join-Path $root 'plugins\android\bin\scrcpy\adb.exe'
$devices = & $adb devices 2>&1 | Select-String -Pattern '\sdevice$'
if (-not $devices) { Write-Host 'PRECONDITION FAILED: no device'; exit 2 }

Get-Process castpane, scrcpy, android-plugin, msedgewebview2 -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $root 'dist\CastPane\settings.json') -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 700

$app = Start-Process -FilePath $exe -PassThru
$result = [ordered]@{
    ratio_before     = $null
    ratio_after      = $null
    switched_live    = $false
    closed           = $false
    recast_enabled   = $false
}

function Get-AppWindow($processId, $timeoutSeconds = 25) {
    $rootElement = [System.Windows.Automation.AutomationElement]::RootElement
    $cond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $processId)
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        foreach ($candidate in $rootElement.FindAll(
                [System.Windows.Automation.TreeScope]::Children, $cond)) {
            $kids = $candidate.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                [System.Windows.Automation.Condition]::TrueCondition)
            if ($kids.Count -gt 5) { return $candidate }
        }
        Start-Sleep -Milliseconds 500
    }
    return $null
}

function Measure-Cast {
    $json = & python (Join-Path $PSScriptRoot 'e2e_probe.py') measure
    if (-not $json -or $json -eq 'null') { return $null }
    return $json | ConvertFrom-Json
}

function Set-Orientation($combo, $value) {
    # The cast window may overlap the console; raise the console first so the
    # click lands on the dropdown instead of the cast window.
    Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);' -Name F2 -Namespace W
    [W.F2]::SetForegroundWindow([IntPtr]$win.Current.NativeWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 400
    try { $combo.GetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern).ScrollIntoView() } catch { }
    Start-Sleep -Milliseconds 500
    $rect = $combo.Current.BoundingRectangle
    [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point(
        [int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
    Start-Sleep -Milliseconds 250
    Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern void mouse_event(int f,int dx,int dy,int c,int e);' -Name M -Namespace W
    [W.M]::mouse_event(2, 0, 0, 0, 0)
    [W.M]::mouse_event(4, 0, 0, 0, 0)
    Start-Sleep -Milliseconds 500
    [System.Windows.Forms.SendKeys]::SendWait($value)
    Start-Sleep -Milliseconds 350
    [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
    Start-Sleep -Milliseconds 700
}

try {
    $win = Get-AppWindow $app.Id
    if (-not $win) { throw 'app window not found' }
    Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);' -Name F -Namespace W
    [W.F]::SetForegroundWindow([IntPtr]$win.Current.NativeWindowHandle) | Out-Null
    Start-Sleep -Seconds 3

    $comboCond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::ComboBox)
    $combos = @($win.FindAll([System.Windows.Automation.TreeScope]::Descendants, $comboCond))
    if ($combos.Count -lt 2) { throw 'orientation combo not found' }
    $combo = $combos[1]
    $valuePattern = $combo.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)

    # Make sure we start in portrait.
    Set-Orientation $combo 'p'
    Write-Host "orientation start: $($valuePattern.Current.Value)"

    # Cast once.
    $castLabel = [string][char]0x6295 + [char]0x5C4F
    $btnCond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::Button)
    $castBtn = $null
    $deadline = (Get-Date).AddSeconds(20)
    while ((Get-Date) -lt $deadline -and -not $castBtn) {
        foreach ($b in $win.FindAll([System.Windows.Automation.TreeScope]::Descendants, $btnCond)) {
            if ($b.Current.Name -eq $castLabel -and $b.Current.IsEnabled) { $castBtn = $b }
        }
        if (-not $castBtn) { Start-Sleep -Milliseconds 700 }
    }
    if (-not $castBtn) { throw 'cast button not available' }
    $castBtn.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()

    $deadline = (Get-Date).AddSeconds(20)
    while ((Get-Date) -lt $deadline -and -not $result.ratio_before) {
        Start-Sleep -Milliseconds 400
        $m = Measure-Cast
        if ($m) { $result.ratio_before = $m.ratio }
    }
    if (-not $result.ratio_before) { throw 'first cast never appeared' }
    Write-Host "ratio before: $($result.ratio_before)"

    # Change orientation WITHOUT pressing cast again.
    Set-Orientation $combo 'l'
    Write-Host "orientation now: $($valuePattern.Current.Value)"

    Start-Sleep -Seconds 1
    $deadline = (Get-Date).AddSeconds(20)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 400
        $m = Measure-Cast
        if ($m) { $result.ratio_after = $m.ratio }
        if ($result.ratio_after -and $result.ratio_after -gt 1.2) { break }
    }
    Write-Host "ratio after: $($result.ratio_after)"
    $result.switched_live = ($result.ratio_after -and $result.ratio_after -gt 1.2)

    # Close the cast window and check the cast button comes back.
    $closeJson = & python (Join-Path $PSScriptRoot 'e2e_probe.py') close
    $closed = $closeJson | ConvertFrom-Json
    $result.closed = [bool]$closed.closed
    Start-Sleep -Seconds 2

    $deadline = (Get-Date).AddSeconds(10)
    while ((Get-Date) -lt $deadline -and -not $result.recast_enabled) {
        foreach ($b in $win.FindAll([System.Windows.Automation.TreeScope]::Descendants, $btnCond)) {
            if ($b.Current.Name -eq $castLabel -and $b.Current.IsEnabled) { $result.recast_enabled = $true }
        }
        if (-not $result.recast_enabled) { Start-Sleep -Milliseconds 500 }
    }
}
catch {
    $result.error = $_.Exception.Message
}
finally {
    Get-Process castpane, scrcpy, android-plugin, msedgewebview2 -ErrorAction SilentlyContinue |
        Stop-Process -Force -ErrorAction SilentlyContinue
}

$result | ConvertTo-Json -Depth 4

$ok = $result.ratio_before -and $result.switched_live -and $result.closed -and $result.recast_enabled
if (-not $result.switched_live) { Write-Host 'FAIL: window did not switch orientation live' }
if (-not $result.recast_enabled) { Write-Host 'FAIL: cast button stayed disabled after closing the window' }
if ($ok) { Write-Host 'PASS'; exit 0 } else { exit 1 }
