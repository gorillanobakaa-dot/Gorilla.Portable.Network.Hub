# Version: 1.0.0 · updated 26-09-23-13-30
#
# Does the hub bring its wifi network back by itself when it goes away?
#
# Tested with a phone on 2026-09-23: the laptop's wifi was switched off, which
# took the hotspot with it, and the hub went on showing codes for a network
# that was not there. Windows also switches the hotspot off after five minutes
# with nobody on it. The hub now has a guard that looks every few seconds.
#
#   A  the hotspot is switched off from outside (as Windows' power saving does)
#   B  the laptop's wifi radio is switched off (what the tester did)
#
# Each: time until the hotspot is On again with nobody touching anything.
# The finally block switches the wifi radio back ON whatever happened (this
# laptop's own internet goes through it), ends the hub it started, and checks
# the watchman then switches the hotspot off.
#
#   powershell -ExecutionPolicy Bypass -File bench\hotspot-guard-test.ps1 -HubExe <hub.exe> -Folder <folder>

param(
    [Parameter(Mandatory)] [string]$HubExe,
    [Parameter(Mandatory)] [string]$Folder
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$opT = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
function Await($o, [Type]$t) { $x = $opT.MakeGenericMethod($t).Invoke($null, @($o)); $null = $x.Wait(30000); $x.Result }
$null = [Windows.Networking.Connectivity.NetworkInformation,Windows.Networking.Connectivity,ContentType=WindowsRuntime]
$null = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager,Windows.Networking,ContentType=WindowsRuntime]
$null = [Windows.Devices.Radios.Radio,Windows.System.Devices,ContentType=WindowsRuntime]
$Res = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult]
$NI = [Windows.Networking.Connectivity.NetworkInformation]
$TM = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager]

function State {
    foreach ($p in $NI::GetConnectionProfiles()) {
        try { $m = $TM::CreateFromConnectionProfile($p) } catch { continue }
        if ($m.TetheringOperationalState -ne 'Off') { return [string]$m.TetheringOperationalState }
    }
    'Off'
}
function StopAll {
    foreach ($p in $NI::GetConnectionProfiles()) {
        try { $m = $TM::CreateFromConnectionProfile($p) } catch { continue }
        if ($m.TetheringOperationalState -ne 'Off') { $null = Await ($m.StopTetheringAsync()) $Res }
    }
}
function Radio {
    $null = Await ([Windows.Devices.Radios.Radio]::RequestAccessAsync()) ([Windows.Devices.Radios.RadioAccessStatus])
    (Await ([Windows.Devices.Radios.Radio]::GetRadiosAsync()) ([System.Collections.Generic.IReadOnlyList[Windows.Devices.Radios.Radio]])) | Where-Object { $_.Kind -eq 'WiFi' } | Select-Object -First 1
}
function SetRadio($state) { $r = Radio; Await ($r.SetStateAsync($state)) ([Windows.Devices.Radios.RadioAccessStatus]) }
function WaitFor([scriptblock]$cond, $seconds) {
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $seconds) {
        if (& $cond) { return [int]((Get-Date) - $t0).TotalMilliseconds }
        Start-Sleep -Milliseconds 250
    }
    -1
}

try {
    Get-Process hub -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id $_.Id -Force }
    StopAll; Start-Sleep -Seconds 2
    $p = Start-Process $HubExe -ArgumentList 'serve', "`"$Folder`"", '--name', '"Gorilla Hub"', '--password', 'gorilla127' -WindowStyle Minimized -PassThru
    $up = WaitFor { (State) -eq 'On' } 40
    "hub started; hotspot On after $up ms"
    Start-Sleep -Seconds 12   # past the hub's own setup, so the guard is running

    StopAll
    "A: hotspot switched off from outside; state now $(State)"
    $back = WaitFor { (State) -eq 'On' } 60
    "A: back On by itself after $back ms"

    Start-Sleep -Seconds 5
    "B: wifi radio set Off: $(SetRadio 'Off'); hotspot $(State)"
    $back = WaitFor { (State) -eq 'On' -and (Radio).State -eq 'On' } 90
    "B: radio and hotspot back On by themselves after $back ms (radio now $((Radio).State))"
}
finally {
    if ((Radio).State -ne 'On') { "*** had to switch the wifi radio back on myself: $(SetRadio 'On') ***" }
    Get-Process hub -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id $_.Id -Force }
    $off = WaitFor { (State) -eq 'Off' } 30
    "after the hub ended, the watchman switched the hotspot off after $off ms"
    if ((State) -ne 'Off') { StopAll; '*** had to switch the hotspot off myself ***' }
    "final: hotspot $(State), wifi radio $((Radio).State)"
}
