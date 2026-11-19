<#
.SYNOPSIS
    L3 UI end-to-end check for v0.3.0: AirPlay receiver + phone/PC file transfer.

.DESCRIPTION
    Drives the packaged app through real UIA mouse clicks. Scenarios:

      airplay   real uxplay engine: start -> mDNS advertised -> stop -> mDNS gone
      files     real wireless ADB: start cast -> send 10MB -> receive it back
      all       run both (files last, so the cast window is checked while files transfer)

    ASCII-only source: Windows PowerShell 5.1 reads .ps1 as ANSI. Chinese UI
    names are built from code points below, and actions use stable element ids.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\e2e_v03.ps1 -Scenario airplay
    powershell -ExecutionPolicy Bypass -File scripts\e2e_v03.ps1 -Scenario files
#>
[CmdletBinding()]
param(
    [ValidateSet('airplay', 'files', 'all')]
    [string]$Scenario = 'airplay',
    [string]$AppPath,
    [switch]$KeepProcess
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $AppPath) { $AppPath = Join-Path $root 'dist\CastPane\CastPane.exe' }
if (-not (Test-Path $AppPath)) { throw "app not found: $AppPath" }

$outDir = Join-Path $root 'build\e2e-v03'
New-Item -ItemType Directory -Path $outDir -Force | Out-Null
$evidenceDir = Join-Path $root 'docs\assets\evidence'
New-Item -ItemType Directory -Path $evidenceDir -Force | Out-Null

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class W32V3 {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(int flags, int dx, int dy, int data, int extra);
}
"@

$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--force-renderer-accessibility'

function CN([int[]]$codes) { return (-join ($codes | ForEach-Object { [char]$_ })) }

$Label = @{
    StartCast   = (CN @(0x5F00, 0x59CB, 0x6295, 0x5C4F))
    StartAir    = (CN @(0x542F, 0x52A8, 0x63A5, 0x6536, 0x7AEF))
    StopAir     = (CN @(0x505C, 0x6B62, 0x63A5, 0x6536, 0x7AEF))
    Phone       = (CN @(0x624B, 0x673A))
    Send        = (CN @(0x53D1, 0x9001, 0x5230, 0x624B, 0x673A))
    Download    = (CN @(0x4E0B, 0x8F7D, 0x5230, 0x7535, 0x8111))
    Open        = (CN @(0x6253, 0x5F00))
    File        = (CN @(0x6587, 0x4EF6))
    Allow       = (CN @(0x5141, 0x8BB8))
    Security    = (CN @(0x5B89, 0x5168, 0x4E2D, 0x5FC3))
}

function Stop-TestProcesses {
    Get-Process castpane, airplay-plugin, android-plugin, scrcpy, uxplay-windows, uxplay-bluetooth-beacon -ErrorAction SilentlyContinue |
        Stop-Process -Force -ErrorAction SilentlyContinue
}

function Get-AppWindow($processId, $timeoutSeconds = 30) {
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

function Get-ElementsByName($win, $name) {
    $all = $win.FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.Condition]::TrueCondition)
    $out = @()
    foreach ($element in $all) {
        if ($element.Current.Name -eq $name) { $out += $element }
    }
    return $out
}

function Get-ElementByName($win, $name) {
    $items = Get-ElementsByName $win $name
    if ($items.Count -gt 0) { return $items[0] }
    return $null
}

function Get-ElementByNameWithinList($win, $name) {
    $list = Get-ElementById $win 'files-list'
    if (-not $list) { return Get-ElementByName $win $name }
    $nodes = $list.FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($node in $nodes) {
        $nodeName = $node.Current.Name
        if ($nodeName -and ($nodeName -eq $name -or $nodeName.StartsWith($name + ' '))) {
            return $node
        }
    }
    return $null
}

function Wait-ElementByName($win, $name, $timeoutSeconds = 8) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $element = Get-ElementByName $win $name
        if ($element) { return $element }
        Start-Sleep -Milliseconds 250
    }
    return $null
}

function Wait-TextContains($win, $needle, $timeoutSeconds = 10) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $all = $win.FindAll(
            [System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.Condition]::TrueCondition)
        foreach ($element in $all) {
            $name = $element.Current.Name
            if ($name -and $name.Contains($needle)) { return $element }
        }
        Start-Sleep -Milliseconds 250
    }
    return $null
}

function Scroll-FilesList($win, $clicks = 3) {
    $list = Get-ElementById $win 'files-list'
    if ($list) {
        try {
            $scroll = $list.GetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern)
            $scroll.Scroll(
                [System.Windows.Automation.ScrollAmount]::NoAmount,
                [System.Windows.Automation.ScrollAmount]::LargeIncrement)
            Start-Sleep -Milliseconds 300
            return
        } catch { }
    }
    [W32V3]::SetForegroundWindow([IntPtr]$win.Current.NativeWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 150
    $rect = $win.Current.BoundingRectangle
    $x = [int]($rect.X + $rect.Width / 2)
    $y = [int]($rect.Y + $rect.Height * 0.55)
    [W32V3]::SetCursorPos($x, $y) | Out-Null
    for ($i = 0; $i -lt $clicks; $i++) {
        [W32V3]::mouse_event(0x0800, 0, 0, -120, 0)
        Start-Sleep -Milliseconds 70
    }
    Start-Sleep -Milliseconds 250
}

function Wait-ElementByScrolling($win, $name, $timeoutSeconds = 15) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $element = Get-ElementByNameWithinList $win $name
        if ($element) { return $element }
        Scroll-FilesList $win 3
    }
    return $null
}

function Get-TextStart($win, $prefix) {
    $all = $win.FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($element in $all) {
        $name = $element.Current.Name
        if ($name -and $name.StartsWith($prefix)) {
            return $name.Substring($prefix.Length)
        }
    }
    return $null
}

function Get-State($win) { return (Get-TextStart $win 'state=') }
function Get-Page($win) { return (Get-TextStart $win 'page=') }
function Get-Transfer($win) { return (Get-TextStart $win 'transfer=') }

function Wait-TextStart($win, $prefix, $predicate, $timeoutSeconds = 20) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $value = Get-TextStart $win $prefix
        if ($value -and (& $predicate $value)) { return $value }
        Start-Sleep -Milliseconds 200
    }
    return $null
}

function Wait-State($win, $state, $timeoutSeconds = 20) {
    return [bool](Wait-TextStart $win 'state=' { param($v) $v -eq $state } $timeoutSeconds)
}

function Wait-StateNot($win, $state, $timeoutSeconds = 20) {
    return [bool](Wait-TextStart $win 'state=' { param($v) $v -ne $state } $timeoutSeconds)
}

function Wait-Page($win, $pageName, $timeoutSeconds = 10) {
    return [bool](Wait-TextStart $win 'page=' { param($v) $v -eq $pageName } $timeoutSeconds)
}

function Wait-Transfer($win, $state, $timeoutSeconds = 120, $excludeId = $null) {
    $value = Wait-TextStart $win 'transfer=' {
        param($v)
        $parts = $v.Split(':')
        if ($parts.Count -lt 2) { return $false }
        if ($parts[1] -ne $state) { return $false }
        if ($excludeId -and $parts[0] -eq $excludeId) { return $false }
        return $true
    } $timeoutSeconds
    return $value
}

function Click-Element($win, $element) {
    if (-not $element) { throw 'click target not found' }
    try {
        if ($element.Current.IsOffscreen) {
            $scroll = $element.GetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern)
            $scroll.ScrollIntoView()
            Start-Sleep -Milliseconds 250
        }
    } catch { }
    [W32V3]::SetForegroundWindow([IntPtr]$win.Current.NativeWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 300
    $rect = $element.Current.BoundingRectangle
    $x = [int]($rect.X + $rect.Width / 2)
    $y = [int]($rect.Y + $rect.Height / 2)
    [W32V3]::SetCursorPos($x, $y) | Out-Null
    Start-Sleep -Milliseconds 180
    [W32V3]::mouse_event(0x0002, 0, 0, 0, 0)
    [W32V3]::mouse_event(0x0004, 0, 0, 0, 0)
}

function Shot($target, $name, $copyToEvidence = $false) {
    $path = Join-Path $outDir $name
    & python (Join-Path $PSScriptRoot 'window_shot.py') $target $path | Out-Null
    if ($copyToEvidence -and (Test-Path $path)) {
        Copy-Item $path (Join-Path $evidenceDir $name) -Force
    }
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
            return [pscustomobject]@{ Serial = $parts[0]; State = $parts[1]; Adb = $adb }
        }
    }
    return $null
}

function Start-TestApp {
    Stop-TestProcesses
    Start-Sleep -Milliseconds 600
    $settings = Join-Path (Split-Path -Parent $AppPath) 'settings.json'
    Remove-Item $settings -ErrorAction SilentlyContinue
    $webview = Join-Path $env:LOCALAPPDATA 'dev.castpane.app\EBWebView'
    if (Test-Path $webview) { Remove-Item $webview -Recurse -Force -ErrorAction SilentlyContinue }
    $proc = Start-Process -FilePath $AppPath -PassThru
    $win = Get-AppWindow $proc.Id
    if (-not $win) { throw 'app window not found via UIA' }
    return @{ Process = $proc; Window = $win }
}

function Get-FileDialog($processId, $timeoutSeconds = 15) {
    $rootElement = [System.Windows.Automation.AutomationElement]::RootElement
    $cond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $processId)
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        foreach ($candidate in $rootElement.FindAll(
                [System.Windows.Automation.TreeScope]::Children, $cond)) {
            if ($candidate.Current.ClassName -eq '#32770') { return $candidate }
        }
        Start-Sleep -Milliseconds 250
    }
    return $null
}

function Allow-FirewallPrompt($timeoutSeconds = 6) {
    $rootElement = [System.Windows.Automation.AutomationElement]::RootElement
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        foreach ($candidate in $rootElement.FindAll(
                [System.Windows.Automation.TreeScope]::Children,
                [System.Windows.Automation.Condition]::TrueCondition)) {
            $name = $candidate.Current.Name
            if (-not $name) { continue }
            if ($name.Contains($Label.Security) -or $name -like '*Windows Defender*') {
                $all = $candidate.FindAll(
                    [System.Windows.Automation.TreeScope]::Descendants,
                    [System.Windows.Automation.Condition]::TrueCondition)
                foreach ($element in $all) {
                    if ($element.Current.Name -eq $Label.Allow) {
                        $invoke = $element.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
                        $invoke.Invoke()
                        Start-Sleep -Milliseconds 500
                        return $true
                    }
                }
            }
        }
        Start-Sleep -Milliseconds 300
    }
    return $false
}

function Set-FileDialogPath($dialog, $path) {
    [W32V3]::SetForegroundWindow([IntPtr]$dialog.Current.NativeWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 400
    $edit = Get-ElementById $dialog '1148'
    if (-not $edit) {
        $edits = $dialog.FindAll(
            [System.Windows.Automation.TreeScope]::Descendants,
            (New-Object System.Windows.Automation.PropertyCondition(
                [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
                [System.Windows.Automation.ControlType]::Edit)))
        if ($edits.Count -gt 0) { $edit = $edits[$edits.Count - 1] }
    }
    if (-not $edit) { throw 'file dialog filename edit not found' }
    try {
        $pattern = $edit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
        $pattern.SetValue($path)
    } catch {
        $edit.SetFocus()
        Start-Sleep -Milliseconds 150
        [System.Windows.Forms.SendKeys]::SendWait('^a')
        [System.Windows.Forms.SendKeys]::SendWait($path)
        Start-Sleep -Milliseconds 200
    }
    Start-Sleep -Milliseconds 400
    try {
        $readback = $edit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value
        Write-Host "FILE DIALOG VALUE: $readback"
    } catch {
        Write-Host 'FILE DIALOG VALUE: (unreadable)'
    }
    $open = $null
    $buttons = $dialog.FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        (New-Object System.Windows.Automation.PropertyCondition(
            [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
            [System.Windows.Automation.ControlType]::Button)))
    $candidates = @()
    foreach ($button in $buttons) {
        if ($button.Current.Name -and $button.Current.Name.StartsWith($Label.Open)) {
            $candidates += $button
        }
    }
    if ($candidates.Count -gt 0) {
        $open = $candidates | Sort-Object { $_.Current.BoundingRectangle.Y } -Descending | Select-Object -First 1
    }
    if (-not $open) { throw 'file dialog open button not found' }
    Write-Host "FILE DIALOG OPEN: id=$($open.Current.AutomationId) name=$($open.Current.Name) enabled=$($open.Current.IsEnabled)"
    $invoke = $open.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
    $invoke.Invoke()
    Start-Sleep -Milliseconds 700

    # Some Explorer-style dialogs need the default button activated from the
    # filename edit; this also covers a failed InvokePattern.
    $still = Get-FileDialog ([int]$dialog.Current.ProcessId) 1
    if ($still) {
        $edit.SetFocus()
        Start-Sleep -Milliseconds 150
        [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
        Start-Sleep -Milliseconds 500
    }
}

function Get-CastWindow {
    $probe = & python (Join-Path $PSScriptRoot 'e2e_probe.py') measure
    if (-not $probe -or $probe -eq 'null') { return $null }
    return ($probe | ConvertFrom-Json)
}

function New-TestFile {
    $name = 'cp-e2e-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + (Get-Random -Minimum 1000 -Maximum 9999) + '.bin'
    $path = Join-Path $env:TEMP $name
    python -c "import os,sys; open(sys.argv[1],'wb').write(os.urandom(10*1024*1024))" $path
    return [pscustomobject]@{
        Name = $name
        Path = $path
        Hash = (Get-FileHash $path -Algorithm SHA256).Hash
        Size = (Get-Item $path).Length
    }
}

$result = [ordered]@{
    scenario = $Scenario
    app = $AppPath
    passed = $false
    error = $null
    airplay = $null
    files = $null
    screenshots = @()
    started = (Get-Date).ToString('s')
}

try {
    if ($Scenario -in @('files', 'all')) {
        $device = Get-RealDevice
        if (-not $device) {
            Write-Host 'PRECONDITION FAILED: no Android device in "adb devices".'
            exit 2
        }
        $result.device = $device.Serial
    }

    if ($Scenario -in @('airplay', 'all')) {
        $app = Start-TestApp
        $win = $app.Window
        if (-not (Wait-State $win 'ready' 30) -and -not (Wait-State $win 'airplay-selected' 2)) {
            throw "phone did not become ready (state=$(Get-State $win))"
        }
        $result.screenshots += Shot $app.Process.Id 'home-airplay.png' $true

        $row = Get-ElementById $win 'airplay-row'
        if (-not $row) { throw 'airplay row not found on first screen' }
        Click-Element $win $row
        $onAirplayPage = Wait-Page $win 'airplay' 3
        if (-not $onAirplayPage) {
            if (-not (Wait-State $win 'airplay-selected' 5)) {
                throw "airplay row did not select (state=$(Get-State $win))"
            }
        }

        $primary = if ($onAirplayPage) { Get-ElementById $win 'airplay-primary' } else { Get-ElementById $win 'primary-action' }
        if (-not $primary) { throw 'airplay start button not found' }
        if ($primary.Current.Name -ne $Label.StartAir) {
            throw "expected '$($Label.StartAir)', got '$($primary.Current.Name)'"
        }

        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        Click-Element $win $primary
        if (-not (Wait-State $win 'airplay-waiting' 25)) {
            throw "receiver did not reach waiting (state=$(Get-State $win))"
        }
        $waitSeconds = [math]::Round($sw.Elapsed.TotalSeconds, 2)
        $firewallPrompt = Allow-FirewallPrompt 8

        $mdns = & python (Join-Path $PSScriptRoot 'mdns_probe.py') 3 | ConvertFrom-Json
        $record = $mdns | Where-Object { $_.name -like 'CastPane*' } | Select-Object -First 1
        if (-not $record) { throw "mDNS did not advertise CastPane: $($mdns | ConvertTo-Json -Compress)" }
        if (-not $record.port -or $record.port -le 0) { throw "mDNS record has no port: $($record | ConvertTo-Json -Compress)" }

        $result.airplay = [ordered]@{
            state = Get-State $win
            seconds_to_waiting = $waitSeconds
            mdns_name = $record.name
            mdns_port = $record.port
            mdns_addresses = $record.addresses
            firewall_prompt_allowed = $firewallPrompt
            streaming_observed = $false
            note = 'No iPad on this network; video-window/streaming path awaits real iPad.'
        }

        $result.screenshots += Shot $app.Process.Id 'airplay-waiting.png' $true

        $stop = Get-ElementById $win 'airplay-primary'
        if (-not $stop) { throw 'airplay stop button not found' }
        $null = Allow-FirewallPrompt 1
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        Click-Element $win $stop
        if (-not (Wait-State $win 'airplay-selected' 12)) {
            throw "receiver did not stop (state=$(Get-State $win))"
        }
        $result.airplay.seconds_to_stop = [math]::Round($sw.Elapsed.TotalSeconds, 2)
        Start-Sleep -Seconds 2
        $after = & python (Join-Path $PSScriptRoot 'mdns_probe.py') 3 | ConvertFrom-Json
        $stillThere = $after | Where-Object { $_.name -like 'CastPane*' } | Select-Object -First 1
        if ($stillThere) { throw "mDNS still advertises after stop: $($stillThere | ConvertTo-Json -Compress)" }
        $result.airplay.mdns_after_stop = 'gone'
        $result.screenshots += Shot $app.Process.Id 'airplay-stopped.png' $true

        # Second use in the same app session: start again after a stop.
        $secondStart = Get-ElementById $win 'airplay-primary'
        if ($secondStart -and $secondStart.Current.Name -eq $Label.StartAir) {
            $sw = [System.Diagnostics.Stopwatch]::StartNew()
            Click-Element $win $secondStart
            if (-not (Wait-State $win 'airplay-waiting' 25)) {
                throw "second start did not reach waiting (state=$(Get-State $win))"
            }
            $result.airplay.second_start_seconds = [math]::Round($sw.Elapsed.TotalSeconds, 2)
            $result.screenshots += Shot $app.Process.Id 'airplay-second-waiting.png' $true
            $secondStop = Get-ElementById $win 'airplay-primary'
            $sw = [System.Diagnostics.Stopwatch]::StartNew()
            Click-Element $win $secondStop
            if (-not (Wait-State $win 'airplay-selected' 12)) {
                throw "second stop did not finish (state=$(Get-State $win))"
            }
            $result.airplay.second_stop_seconds = [math]::Round($sw.Elapsed.TotalSeconds, 2)
        }

        $app.Process | Stop-Process -Force -ErrorAction SilentlyContinue
    }

    if ($Scenario -in @('files', 'all')) {
        $device = Get-RealDevice
        $testFile = New-TestFile
        $result.test_file = [ordered]@{ name = $testFile.Name; size = $testFile.Size; sha256 = $testFile.Hash }

        $app = Start-TestApp
        $win = $app.Window
        if (-not (Wait-State $win 'ready' 30)) {
            throw "phone did not become ready (state=$(Get-State $win))"
        }

        # Start the real wireless cast first; file transfer must not break it.
        $primary = Get-ElementById $win 'primary-action'
        if ($primary.Current.Name -ne $Label.StartCast) {
            throw "expected '$($Label.StartCast)', got '$($primary.Current.Name)'"
        }
        $swCast = [System.Diagnostics.Stopwatch]::StartNew()
        Click-Element $win $primary
        if (-not (Wait-State $win 'streaming' 25)) {
            throw "cast did not reach streaming (state=$(Get-State $win))"
        }
        $castDeadline = (Get-Date).AddSeconds(20)
        $castWindow = $null
        while ((Get-Date) -lt $castDeadline) {
            $castWindow = Get-CastWindow
            if ($castWindow) { break }
            Start-Sleep -Milliseconds 300
        }
        if (-not $castWindow) { throw 'scrcpy window never appeared' }
        $result.cast = [ordered]@{
            seconds_to_streaming = [math]::Round($swCast.Elapsed.TotalSeconds, 2)
            window = $castWindow
        }
        $result.screenshots += Shot $app.Process.Id 'home-streaming.png' $true

        # Open the files page from the connected device.
        $filesButton = Get-ElementById $win 'files-button'
        if (-not $filesButton) { throw 'files entry not found on first screen' }
        Click-Element $win $filesButton
        if (-not (Wait-Page $win 'files' 10)) { throw "files page did not open (page=$(Get-Page $win))" }
        Start-Sleep -Milliseconds 800
        $result.screenshots += Shot $app.Process.Id 'files-list.png' $true

        # Send: native dialog -> path -> open.
        $send = Get-ElementById $win 'files-send'
        if (-not $send) { throw 'send button not found' }
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        Click-Element $win $send
        $dialog = Get-FileDialog $app.Process.Id 15
        if (-not $dialog) { throw 'file dialog did not appear' }
        Set-FileDialogPath $dialog $testFile.Path
        Start-Sleep -Milliseconds 800
        $stillDialog = Get-FileDialog $app.Process.Id 1
        if ($stillDialog) {
            $result.screenshots += Shot $app.Process.Id 'files-dialog-stuck.png' $true
            throw 'file dialog did not close after Open (see files-dialog-stuck.png)'
        }
        # A 10MB transfer over Wi-Fi can finish between two 900ms UI polls;
        # do not require observing the intermediate "running" state.
        $pushDone = Wait-Transfer $win 'done' 120
        if (-not $pushDone) { throw "push did not finish (probe=$(Get-Transfer $win))" }
        $pushId = $pushDone.Split(':')[0]
        $pushSeconds = [math]::Round($sw.Elapsed.TotalSeconds, 2)

        $remote = "/sdcard/Download/CastPane/$($testFile.Name)"
        $remoteHash = (& $device.Adb -s $device.Serial shell "sha256sum '$remote'" 2>&1 | Out-String).Trim()
        $remoteHash = ($remoteHash -split '\s+')[0]
        $remoteSize = (& $device.Adb -s $device.Serial shell "stat -c %s '$remote'" 2>&1 | Out-String).Trim()
        if ($remoteHash -ne $testFile.Hash.ToLower()) {
            throw "phone hash mismatch: $remoteHash vs $($testFile.Hash)"
        }
        if ([int64]$remoteSize -ne $testFile.Size) {
            throw "phone size mismatch: $remoteSize vs $($testFile.Size)"
        }
        $result.files = [ordered]@{
            push_seconds = $pushSeconds
            push_remote_path = $remote
            push_remote_size = [int64]$remoteSize
            push_remote_sha256 = $remoteHash
        }
        $result.screenshots += Shot $app.Process.Id 'files-sent.png' $true

        # Navigate through the common-folder chip to Download, then into CastPane.
        $quick = Get-ElementById $win 'quick-download'
        if (-not $quick) { throw 'quick Download chip not found' }
        Click-Element $win $quick
        Start-Sleep -Milliseconds 1200
        $result.screenshots += Shot $app.Process.Id 'files-download-root.png' $true
        $castFolder = Wait-ElementByScrolling $win 'CastPane' 15
        if (-not $castFolder) { throw 'CastPane folder not found under Download' }
        Click-Element $win $castFolder
        $fileRow = Wait-ElementByScrolling $win $testFile.Name 15
        if (-not $fileRow) { throw "uploaded file not listed: $($testFile.Name)" }
        Click-Element $win $fileRow
        Start-Sleep -Milliseconds 300

        $downloadButton = Get-ElementById $win 'files-download'
        if (-not $downloadButton) { throw 'download button not found' }
        $enabled = $false
        for ($i = 0; $i -lt 20; $i++) {
            $downloadButton = Get-ElementById $win 'files-download'
            if ($downloadButton -and $downloadButton.Current.IsEnabled) { $enabled = $true; break }
            Start-Sleep -Milliseconds 250
        }
        if (-not $enabled) { throw 'download button stayed disabled after selecting a file' }
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        Click-Element $win $downloadButton
        $pullDone = Wait-Transfer $win 'done' 120 $pushId
        if (-not $pullDone) { throw "pull did not finish (probe=$(Get-Transfer $win))" }
        $pullSeconds = [math]::Round($sw.Elapsed.TotalSeconds, 2)

        $downloadDir = Join-Path $env:USERPROFILE 'Downloads\CastPane'
        $candidates = @(Get-ChildItem $downloadDir -File -Filter "$($testFile.Name)*" -ErrorAction SilentlyContinue)
        if ($candidates.Count -eq 0) { throw "downloaded file not found in $downloadDir" }
        $localCopy = $candidates | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        $localHash = (Get-FileHash $localCopy.FullName -Algorithm SHA256).Hash
        if ($localHash -ne $testFile.Hash) { throw "local hash mismatch: $localHash vs $($testFile.Hash)" }
        if ($localCopy.Length -ne $testFile.Size) { throw "local size mismatch: $($localCopy.Length) vs $($testFile.Size)" }
        $result.files.pull_seconds = $pullSeconds
        $result.files.pull_local_path = $localCopy.FullName
        $result.files.pull_local_size = $localCopy.Length
        $result.files.pull_local_sha256 = $localHash
        $result.files.download_dir = $downloadDir
        $result.screenshots += Shot $app.Process.Id 'files-received.png' $true

        # The cast window must still be alive after both directions.
        $stillCast = Get-CastWindow
        if (-not $stillCast) { throw 'cast window disappeared during file transfer' }
        $result.files.cast_still_alive = $true
        $result.files.cast_window_after = $stillCast

        # Second use in the same app session: back home, reopen files, then
        # deliberately open a restricted directory to exercise the error card.
        $back = Get-ElementById $win 'files-back'
        if ($back) {
            Click-Element $win $back
            $null = Wait-Page $win 'home' 5
            $filesAgain = Get-ElementById $win 'files-button'
            if ($filesAgain) {
                Click-Element $win $filesAgain
                if (-not (Wait-Page $win 'files' 10)) { throw 'files page did not reopen' }
                $reopened = Wait-ElementByScrolling $win $testFile.Name 10
                if (-not $reopened) { throw 'files page reopened without the uploaded file' }
                $result.files.second_open = $true

                $phoneCrumb = Get-ElementByName $win $Label.Phone
                if ($phoneCrumb) {
                    Click-Element $win $phoneCrumb
                    $androidFolder = Wait-ElementByScrolling $win 'Android' 10
                    if ($androidFolder) {
                        Click-Element $win $androidFolder
                        $dataFolder = Wait-ElementByScrolling $win 'data' 10
                        if ($dataFolder) {
                            Click-Element $win $dataFolder
                            $permissionSeen = $null -ne (Wait-TextContains $win 'E_PERMISSION' 10)
                            if (-not $permissionSeen) {
                                $result.screenshots += Shot $app.Process.Id 'files-permission-fail.png' $true
                                throw 'restricted Android/data did not show the error card'
                            }
                            $result.files.permission_error = $true
                            $result.screenshots += Shot $app.Process.Id 'files-permission.png' $true
                        }
                    }
                }
            }
        }

        & python (Join-Path $PSScriptRoot 'e2e_probe.py') close | Out-Null
        $null = Wait-State $win 'ready' 10
        $result.screenshots += Shot $app.Process.Id 'files-after-cast-close.png'
        $app.Process | Stop-Process -Force -ErrorAction SilentlyContinue
        Remove-Item $testFile.Path -Force -ErrorAction SilentlyContinue
    }

    $result.passed = $true
} catch {
    $result.error = $_.Exception.Message
} finally {
    if (-not $KeepProcess) { Stop-TestProcesses }
}

$json = Join-Path $outDir ("result-" + $Scenario + ".json")
$result | ConvertTo-Json -Depth 8 | Set-Content $json -Encoding UTF8
$result | ConvertTo-Json -Depth 8
if ($result.passed) { Write-Host 'PASS'; exit 0 } else { Write-Host 'FAIL'; exit 1 }
