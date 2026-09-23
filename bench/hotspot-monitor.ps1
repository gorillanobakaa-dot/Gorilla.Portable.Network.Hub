# Version: 1.0.0 · updated 26-09-23-17-40
#
# A record of what the Windows hotspot is doing, written while nobody watches.
#
# Asked for by the owner on 2026-09-23 after a day of reconstructing, from
# screenshots and memory, what the network had done while a phone was being
# tested. This writes one line every two seconds, and only when something
# changed, plus every Windows wifi event as it happens:
#
#   hotspot   On / Off / InTransition, and how many devices are on it
#   address   whether 192.168.137.1 exists (what the hub hands out)
#   laptop    which wifi network the laptop itself is joined to
#   radio     whether the laptop's wifi radio is on
#   hub       whether the hub program is running
#
# Read-only: it switches nothing on or off.
#
#   powershell -ExecutionPolicy Bypass -File bench\hotspot-monitor.ps1 [-Log <file>]
#
# Default log: %LOCALAPPDATA%\PortableNetworkHub\hotspot-monitor.log

param([string]$Log = "$env:LOCALAPPDATA\PortableNetworkHub\hotspot-monitor.log")
$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force -Path (Split-Path $Log) | Out-Null

Add-Type -AssemblyName System.Runtime.WindowsRuntime
$opT = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
$null = [Windows.Networking.Connectivity.NetworkInformation,Windows.Networking.Connectivity,ContentType=WindowsRuntime]
$null = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager,Windows.Networking,ContentType=WindowsRuntime]
$null = [Windows.Devices.Radios.Radio,Windows.System.Devices,ContentType=WindowsRuntime]
$NI = [Windows.Networking.Connectivity.NetworkInformation]
$TM = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager]
function Await($o, [Type]$t) { $x = $opT.MakeGenericMethod($t).Invoke($null, @($o)); $null = $x.Wait(10000); $x.Result }
$null = Await ([Windows.Devices.Radios.Radio]::RequestAccessAsync()) ([Windows.Devices.Radios.RadioAccessStatus])

function Write-Line($text) {
    $line = "{0:yyyy-MM-dd HH:mm:ss}  {1}" -f (Get-Date), $text
    Add-Content -Path $Log -Value $line -Encoding utf8
    $line
}

function Snapshot {
    $state = 'Off'; $clients = 0; $name = ''
    foreach ($p in $NI::GetConnectionProfiles()) {
        try { $m = $TM::CreateFromConnectionProfile($p) } catch { continue }
        if ($m.TetheringOperationalState -ne 'Off') {
            $state = [string]$m.TetheringOperationalState; $clients = $m.ClientCount
            $name = $m.GetCurrentAccessPointConfiguration().Ssid; break
        }
    }
    $addr = [bool](Get-NetIPAddress -IPAddress 192.168.137.1 -ErrorAction SilentlyContinue)
    $ssid = ((netsh wlan show interfaces) -match '^\s+SSID\s+:' | Select-Object -First 1) -replace '^\s+SSID\s+:\s*', ''
    if (-not $ssid) { $ssid = '(not joined to any wifi)' }
    $radios = Await ([Windows.Devices.Radios.Radio]::GetRadiosAsync()) ([System.Collections.Generic.IReadOnlyList[Windows.Devices.Radios.Radio]])
    $radio = ($radios | Where-Object { $_.Kind -eq 'WiFi' } | Select-Object -First 1).State
    $hub = [bool](Get-Process hub -ErrorAction SilentlyContinue)
    "hotspot $state '$name' devices $clients | address 192.168.137.1 $(if ($addr) {'present'} else {'ABSENT'}) | laptop joined to: $ssid | wifi radio $radio | hub $(if ($hub) {'running'} else {'not running'})"
}

Write-Line "monitor started (pid $PID)"
$last = ''
$since = Get-Date
while ($true) {
    $now = Snapshot
    if ($now -ne $last) { Write-Line $now; $last = $now }
    # Windows' own wifi events since the last look, in its words.
    Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Windows-WLAN-AutoConfig/Operational'; StartTime = $since } -ErrorAction SilentlyContinue |
        Sort-Object TimeCreated | ForEach-Object { Write-Line ("  windows: {0} {1}" -f $_.Id, (($_.Message -split "`n")[0]).Trim()) }
    $since = Get-Date
    Start-Sleep -Seconds 2
}
