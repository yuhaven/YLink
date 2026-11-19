<#
.SYNOPSIS
    L3 UI end-to-end check for the USB-assisted wireless cast flow.

.DESCRIPTION
    Drives the packaged (or debug) app through real UIA clicks. Scenarios:

      mock-usb     USB phone -> click "enable wireless" -> streaming (mock adb/scrcpy)
      mock-empty   no device -> empty state with the Huawei USB-first steps
      mock-network phone on another subnet -> clear failure + retry
      real         real adb device: USB bootstrap or wireless reconnect -> cast window

    ASCII-only on purpose: Windows PowerShell 5.1 reads .ps1 as ANSI, so Chinese
    literals would be mangled. UI names are built from code points below.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\e2e_wireless.ps1 -Scenario mock-usb
    powershell -ExecutionPolicy Bypass -File scripts\e2e_wireless.ps1 -Scenario real
#>
[CmdletBinding()]
param(
    [ValidateSet('mock-usb', 'mock-empty', 'mock-network', 'mock-reconnect', 'real')]
    [string]$Scenario = 'mock-usb',
    [string]$AppPath,
    [switch]$KeepProcess
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $AppPath) {
    $AppPath = Join-Path $root 'dist\CastPane\CastPane.exe'
    if (-not (Test-Path $AppPath)) {
        $AppPath = Join-Path $root 'target\debug\castpane.exe'
    }
}
if (-not (Test-Path $AppPath)) { throw "app not found: $AppPath" }

$outDir = Join-Path $root 'build\e2e-wireless'
New-Item -ItemType Directory -Path $outDir -Force | Out-Null

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class W32 {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(int flags, int dx, int dy, int data, int extra);
}
"@

$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--force-renderer-accessibility'

function CN([int[]]$codes) {
    return (-join ($codes | ForEach-Object { [char]$_ }))
}

$PrimaryLabels = @{
    Start     = (CN @(0x5F00, 0x59CB, 0x6295, 0x5C4F))          # start cast
    Stop      = (CN @(0x505C, 0x6B62, 0x6295, 0x5C4F))          # stop cast
    Enable    = (CN @(0x4E00, 0x952E, 0x5F00, 0x542F, 0x65E0, 0x7EBF)) # enable wireless
    Detect    = (CN @(0x68C0, 0x6D4B, 0x624B, 0x673A))          # detect phone
    Retry     = (CN @(0x91CD, 0x65B0, 0x68C0, 0x6D4B))          # re-check
}

function Stop-TestProcesses {
    Get-Process castpane, android-plugin, scrcpy, mock-scrcpy -ErrorAction SilentlyContinue |
        Stop-Process -Force -ErrorAction SilentlyContinue
}

function Get-AppWindow($processId, $timeoutSeconds = 25) {
    $rootElement = [System.Windows.Automation.AutomationElement]::RootElement
    $cond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $processId)
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        foreach ($candidate in $rootElement.FindAll(
                [System.Windows.Automation.TreeScope]::Children, $cond)) {
            $kids = $candidate.FindAll(
                [System.Windows.Automation.TreeScope]::Descendants,
                [System.Windows.Automation.Condition]::TrueCondition)
            if ($kids.Count -gt 5) { return $candidate }
        }
        Start-Sleep -Milliseconds 400
    }
    return $null
}

function Get-ElementById($win, $id) {
    $cond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::AutomationIdProperty, $id)
    return $win.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
}

function Get-State($win) {
    $all = $win.FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($element in $all) {
        $name = $element.Current.Name
        if ($name -and $name.StartsWith('state=')) {
            return $name.Substring(6)
        }
    }
    return $null
}

function Wait-State($win, $state, $timeoutSeconds = 20) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if ((Get-State $win) -eq $state) { return $true }
        Start-Sleep -Milliseconds 200
    }
    return $false
}

function Click-Element($win, $element) {
    [W32]::SetForegroundWindow([IntPtr]$win.Current.NativeWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 350
    $rect = $element.Current.BoundingRectangle
    $x = [int]($rect.X + $rect.Width / 2)
    $y = [int]($rect.Y + $rect.Height / 2)
    [W32]::SetCursorPos($x, $y) | Out-Null
    Start-Sleep -Milliseconds 220
    [W32]::mouse_event(0x0002, 0, 0, 0, 0)
    [W32]::mouse_event(0x0004, 0, 0, 0, 0)
}

function Shot($target, $name) {
    $path = Join-Path $outDir $name
    & python (Join-Path $PSScriptRoot 'window_shot.py') $target $path | Out-Null
    return $path
}

function Get-LocalWifiIp {
    $wifi = Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue |
        Where-Object { $_.InterfaceAlias -match 'WLAN|Wi-Fi' -and $_.IPAddress -notlike '169.254.*' } |
        Select-Object -First 1
    if ($wifi) { return $wifi.IPAddress }
    return '192.168.1.6'
}

function Get-RealDevice {
    $adb = Join-Path $root 'plugins\android\bin\scrcpy\adb.exe'
    if (-not (Test-Path $adb)) { return $null }
    $lines = & $adb devices -l 2>&1
    foreach ($line in $lines) {
        $parts = ($line -split '\s+') | Where-Object { $_ }
        if ($parts.Count -ge 2 -and $parts[1] -eq 'device') {
            return [pscustomobject]@{ Serial = $parts[0]; State = $parts[1] }
        }
    }
    return $null
}

function Start-TestApp($extraEnv) {
    Stop-TestProcesses
    Start-Sleep -Milliseconds 500
    $settings = Join-Path (Split-Path -Parent $AppPath) 'settings.json'
    Remove-Item $settings -ErrorAction SilentlyContinue
    $webview = Join-Path $env:LOCALAPPDATA 'dev.castpane.app\EBWebView'
    if (Test-Path $webview) { Remove-Item $webview -Recurse -Force -ErrorAction SilentlyContinue }

    foreach ($key in $extraEnv.Keys) { Set-Item -Path "env:$key" -Value $extraEnv[$key] }
    $proc = Start-Process -FilePath $AppPath -PassThru
    $win = Get-AppWindow $proc.Id
    if (-not $win) { throw 'app window not found via UIA' }
    return @{ Process = $proc; Window = $win }
}

$result = [ordered]@{
    scenario = $Scenario
    app = $AppPath
    passed = $false
    state = $null
    elapsed_seconds = $null
    primary = $null
    screenshots = @()
    error = $null
}

try {
    if ($Scenario -eq 'real') {
        $device = Get-RealDevice
        if (-not $device) {
            Write-Host 'PRECONDITION FAILED: no Android device in "adb devices".'
            Write-Host 'Connect the phone by USB, enable USB debugging, allow it, then rerun.'
            exit 2
        }
        $result.device = $device.Serial
        $app = Start-TestApp @{}
        $win = $app.Window
        $state = Get-State $win
        if ($state -ne 'ready' -and $state -ne 'usb') {
            if (-not (Wait-State $win 'ready' 8) -and -not (Wait-State $win 'usb' 2)) {
                throw "device did not become ready or usb (state=$(Get-State $win))"
            }
        }
        $primary = Get-ElementById $win 'primary-action'
        if (-not $primary) { throw 'primary action not found' }
        $result.primary = $primary.Current.Name
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        Click-Element $win $primary

        if (-not (Wait-State $win 'streaming' 20)) {
            throw "app did not reach streaming (state=$(Get-State $win))"
        }
        $result.elapsed_seconds = [math]::Round($sw.Elapsed.TotalSeconds, 2)
        $result.state = Get-State $win
        $result.screenshots += Shot $app.Process.Id 'real-home.png'

        # Wait for the actual scrcpy window; the plugin announces streaming
        # slightly before the window is mapped.
        $windowDeadline = (Get-Date).AddSeconds(15)
        $lastProbe = $null
        while ((Get-Date) -lt $windowDeadline) {
            $lastProbe = & python (Join-Path $PSScriptRoot 'e2e_probe.py') measure
            if ($lastProbe -and $lastProbe -ne 'null') { break }
            Start-Sleep -Milliseconds 300
        }
        if (-not $lastProbe -or $lastProbe -eq 'null') { throw 'cast window never appeared' }
        $result.cast_window = $lastProbe | ConvertFrom-Json
        $result.elapsed_to_window = [math]::Round($sw.Elapsed.TotalSeconds, 2)
        $result.screenshots += Shot 'scrcpy.exe' 'real-cast.png'

        if ($result.elapsed_seconds -gt 15) {
            throw "time to streaming was $($result.elapsed_seconds)s (>15s)"
        }
        if ($result.elapsed_to_window -gt 15) {
            throw "time to cast window was $($result.elapsed_to_window)s (>15s)"
        }

        & python (Join-Path $PSScriptRoot 'e2e_probe.py') close | Out-Null
        $null = Wait-State $win 'ready' 8
        $result.screenshots += Shot $app.Process.Id 'real-after-close.png'
    } else {
        $localIp = Get-LocalWifiIp
        $octets = $localIp.Split('.')
        $sameSubnet = "$($octets[0]).$($octets[1]).$($octets[2]).50"
        $stateDir = Join-Path $outDir ("state-" + $Scenario)
        if (Test-Path $stateDir) { Remove-Item $stateDir -Recurse -Force }
        New-Item -ItemType Directory -Path $stateDir -Force | Out-Null

        $envs = @{
            CASTPANE_ADB = (Join-Path $root 'target\debug\mock-adb-usb.exe')
            CASTPANE_SCRCPY = (Join-Path $root 'target\debug\mock-scrcpy.exe')
            CASTPANE_STATE_DIR = $stateDir
            MOCK_ADB_STATE_FILE = (Join-Path $stateDir 'adb.txt')
            MOCK_USB_PHONE_IP = $sameSubnet
            MOCK_ENGINE_SECONDS = '90'
        }
        if ($Scenario -eq 'mock-empty') {
            $envs.MOCK_ADB_MODE = 'none'
        } elseif ($Scenario -eq 'mock-network') {
            $envs.MOCK_USB_PHONE_IP = '10.0.0.5'
        } elseif ($Scenario -eq 'mock-reconnect') {
            # Simulate a previous successful bootstrap: the phone is remembered
            # but tcpip is up and the Wi-Fi connection is currently absent.
            $utf8 = New-Object System.Text.UTF8Encoding($false)
            $known = @{
                devices = @(
                    @{
                        id = "$($sameSubnet):5555"
                        name = 'Mock Phone'
                        ip = $sameSubnet
                        lastSeen = 1
                        tcpip = $true
                    }
                )
            } | ConvertTo-Json -Depth 5
            [System.IO.File]::WriteAllText(
                (Join-Path $stateDir 'android-wireless.json'), $known, $utf8)
            [System.IO.File]::WriteAllText(
                $envs.MOCK_ADB_STATE_FILE, "tcpip=1`nconnected=0`n", $utf8)
        }
        if (-not (Test-Path $envs.CASTPANE_ADB)) {
            throw "mock adb missing: $($envs.CASTPANE_ADB) (run cargo test/build first)"
        }

        $app = Start-TestApp $envs
        $win = $app.Window

        switch ($Scenario) {
            'mock-usb' {
                if (-not (Wait-State $win 'usb' 20)) {
                    throw "mock USB phone not detected (state=$(Get-State $win))"
                }
                $primary = Get-ElementById $win 'primary-action'
                $result.primary = $primary.Current.Name
                if ($result.primary -ne $PrimaryLabels.Enable) {
                    throw "expected '$($PrimaryLabels.Enable)', got '$($result.primary)'"
                }
                $result.screenshots += Shot $app.Process.Id 'mock-usb-home.png'
                $sw = [System.Diagnostics.Stopwatch]::StartNew()
                Click-Element $win $primary
                $null = Wait-State $win 'bootstrapping' 3
                if (-not (Wait-State $win 'streaming' 20)) {
                    throw "mock bootstrap did not stream (state=$(Get-State $win))"
                }
                $result.elapsed_seconds = [math]::Round($sw.Elapsed.TotalSeconds, 2)
                $result.state = Get-State $win
                $result.screenshots += Shot $app.Process.Id 'mock-usb-streaming.png'
                if ($result.elapsed_seconds -gt 15) {
                    throw "mock bootstrap took $($result.elapsed_seconds)s (>15s)"
                }
            }
            'mock-empty' {
                if (-not (Wait-State $win 'not-found' 20)) {
                    throw "expected empty state (state=$(Get-State $win))"
                }
                $primary = Get-ElementById $win 'primary-action'
                $result.primary = $primary.Current.Name
                if ($result.primary -ne $PrimaryLabels.Detect) {
                    throw "expected '$($PrimaryLabels.Detect)', got '$($result.primary)'"
                }
                $result.state = Get-State $win
                $result.screenshots += Shot $app.Process.Id 'mock-empty-home.png'
            }
            'mock-network' {
                if (-not (Wait-State $win 'error' 20)) {
                    throw "expected network error (state=$(Get-State $win))"
                }
                $primary = Get-ElementById $win 'primary-action'
                $result.primary = $primary.Current.Name
                if ($result.primary -ne $PrimaryLabels.Retry) {
                    throw "expected '$($PrimaryLabels.Retry)', got '$($result.primary)'"
                }
                $result.state = Get-State $win
                $result.screenshots += Shot $app.Process.Id 'mock-network-home.png'
            }
            'mock-reconnect' {
                if (-not (Wait-State $win 'ready' 20)) {
                    throw "auto-reconnect did not reach ready (state=$(Get-State $win))"
                }
                $primary = Get-ElementById $win 'primary-action'
                $result.primary = $primary.Current.Name
                if ($result.primary -ne $PrimaryLabels.Start) {
                    throw "expected '$($PrimaryLabels.Start)', got '$($result.primary)'"
                }
                $result.state = Get-State $win
                $result.screenshots += Shot $app.Process.Id 'mock-reconnect-home.png'
                $sw = [System.Diagnostics.Stopwatch]::StartNew()
                Click-Element $win $primary
                if (-not (Wait-State $win 'streaming' 20)) {
                    throw "reconnect cast did not stream (state=$(Get-State $win))"
                }
                $result.elapsed_seconds = [math]::Round($sw.Elapsed.TotalSeconds, 2)
                $result.screenshots += Shot $app.Process.Id 'mock-reconnect-streaming.png'
            }
        }
    }
    $result.passed = $true
} catch {
    $result.error = $_.Exception.Message
} finally {
    if (-not $KeepProcess) { Stop-TestProcesses }
}

$result | ConvertTo-Json -Depth 6
if ($result.passed) { Write-Host 'PASS'; exit 0 } else { Write-Host 'FAIL'; exit 1 }
