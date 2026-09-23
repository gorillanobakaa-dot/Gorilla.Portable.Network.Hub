# Version: 1.0.0 · updated 26-09-23-19-10
#
# How long does the hub take to switch the hotspot back on, and where does
# the time go? The monitor recorded a 40-second outage at 18:50 on
# 2026-09-23, with the laptop joined to no network (the bush setup), where
# the guard was expected to restore it in under ten.
#
# Times the hub's own script (extracted from net.rs to -Script), run the way
# the hub runs it (a fresh PowerShell each time), for:
#   read     starting PowerShell and finding a connection to share from
#   start    switching the hotspot on from off
# first with the laptop joined to its current wifi, then joined to nothing.
# Disconnects the laptop's wifi for the second half and reconnects in the
# finally block whatever happens. Leaves the hotspot off.
#
#   powershell -ExecutionPolicy Bypass -File bench\hotspot-restart-timing.ps1 -Script <extracted .ps1>

param([Parameter(Mandatory)] [string]$Script)
$ErrorActionPreference = 'Stop'
$wasOn = ((netsh wlan show interfaces) -match '^\s+SSID\s+:' | Select-Object -First 1) -replace '^\s+SSID\s+:\s*', ''

function Run($action) {
    $env:HUB_ACTION = $action; $env:HUB_SSID = 'Gorilla Hub'; $env:HUB_PASS = 'bench-pass-1'; $env:HUB_BAND = '2.4'
    $t = [Diagnostics.Stopwatch]::StartNew()
    $out = powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $Script 2>&1 | Out-String
    $t.Stop()
    $lines = ($out -split "`n" | Where-Object { $_ -match '^HUB_' } | ForEach-Object { $_.Trim() }) -join ' | '
    "{0,-6} {1,6} ms   {2}" -f $action, $t.ElapsedMilliseconds, $lines
}
function AddressUpWithin($seconds) {
    $t = [Diagnostics.Stopwatch]::StartNew()
    while ($t.Elapsed.TotalSeconds -lt $seconds) {
        $c = New-Object System.Net.Sockets.UdpClient
        try { $c.Connect('192.168.137.1', 9); if ($c.Client.LocalEndPoint.Address.ToString() -eq '192.168.137.1') { return $t.ElapsedMilliseconds } } catch { } finally { $c.Close() }
        Start-Sleep -Milliseconds 100
    }
    -1
}

try {
    foreach ($case in 'joined', 'not joined') {
        if ($case -eq 'not joined') { netsh wlan disconnect | Out-Null; Start-Sleep -Seconds 3 }
        $null = Run 'stop'; Start-Sleep -Seconds 2
        "---- laptop $case to a wifi network"
        Run 'read'
        Run 'start'
        "address usable a further $(AddressUpWithin 20) ms after the script returned"
        Run 'stop'
        Start-Sleep -Seconds 2
    }
}
finally {
    $null = Run 'stop'
    if ($wasOn) { netsh wlan connect name="$wasOn" | Out-Null }
    Start-Sleep -Seconds 8
    "reconnected to: " + (((netsh wlan show interfaces) -match '^\s+SSID\s+:' | Select-Object -First 1) -replace '^\s+SSID\s+:\s*', '')
}
