# Version: 1.0.0 · updated 26-09-23-18-00
#
# Which channel does the Windows hotspot actually broadcast on, with the
# laptop joined to NO network (the bush case), band Auto versus 2.4 GHz?
#
# Two phones saw nothing while Windows reported the hotspot On, every time it
# was started with the laptop joined to nothing. This asks the wifi card for
# the channel of the hotspot's own interface (WlanQueryInterface, opcode 8,
# channel number) in each case.
#
# It disconnects the laptop from its current wifi first and reconnects to it
# at the end whatever happens (the finally block), because that link may be
# the only way the person running it can be reached. Afterwards the band is
# left at 2.4 GHz, which is what the hub sets anyway.
#
#   powershell -ExecutionPolicy Bypass -File bench\hotspot-channel-test.ps1

$ErrorActionPreference = 'Stop'
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class Wlan {
  [DllImport("wlanapi.dll")] public static extern int WlanOpenHandle(uint v, IntPtr r, out uint neg, out IntPtr h);
  [DllImport("wlanapi.dll")] public static extern int WlanCloseHandle(IntPtr h, IntPtr r);
  [DllImport("wlanapi.dll")] public static extern int WlanQueryInterface(IntPtr h, ref Guid g, int op, IntPtr r, out int size, out IntPtr data, IntPtr type);
  [DllImport("wlanapi.dll")] public static extern void WlanFreeMemory(IntPtr p);
  public static int Channel(Guid g) {
    uint neg; IntPtr h; if (WlanOpenHandle(2, IntPtr.Zero, out neg, out h) != 0) return -1;
    try { int size; IntPtr data; int rc = WlanQueryInterface(h, ref g, 8, IntPtr.Zero, out size, out data, IntPtr.Zero);
          if (rc != 0) return -rc; int ch = Marshal.ReadInt32(data); WlanFreeMemory(data); return ch; }
    finally { WlanCloseHandle(h, IntPtr.Zero); }
  }
}
'@
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$ext = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 }
$opT = $ext | Where-Object { $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' } | Select-Object -First 1
$actT = $ext | Where-Object { $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncAction' } | Select-Object -First 1
$null = [Windows.Networking.Connectivity.NetworkInformation,Windows.Networking.Connectivity,ContentType=WindowsRuntime]
$null = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager,Windows.Networking,ContentType=WindowsRuntime]
$Res = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult]
function Op($o) { $t = $opT.MakeGenericMethod($Res).Invoke($null, @($o)); $null = $t.Wait(30000); $t.Result }
function Act($o) { $t = $actT.Invoke($null, @($o)); $null = $t.Wait(30000) }
$NI = [Windows.Networking.Connectivity.NetworkInformation]
$TM = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager]

# The hotspot's interface is the one named in Windows' "hosted network" events.
$ev = Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Windows-WLAN-AutoConfig/Operational'; Id = 8006 } -MaxEvents 1
$guid = [Guid](($ev.Message | Select-String '\{[0-9a-fA-F-]+\}').Matches[0].Value)
$wasOn = ((netsh wlan show interfaces) -match '^\s+SSID\s+:' | Select-Object -First 1) -replace '^\s+SSID\s+:\s*', ''
"hotspot interface $guid; laptop currently joined to '$wasOn'"

function Mgr {
    foreach ($p in $NI::GetConnectionProfiles()) { try { return $TM::CreateFromConnectionProfile($p) } catch { } }
}
try {
    $m = Mgr
    if ($m.TetheringOperationalState -ne 'Off') { $null = Op ($m.StopTetheringAsync()) }
    netsh wlan disconnect | Out-Null
    Start-Sleep -Seconds 3
    foreach ($band in 'Auto', 'TwoPointFourGigahertz') {
        $m = Mgr
        $cfg = $m.GetCurrentAccessPointConfiguration(); $cfg.Band = $band; Act ($m.ConfigureAccessPointAsync($cfg))
        $r = Op ($m.StartTetheringAsync())
        Start-Sleep -Seconds 4
        $ch = [Wlan]::Channel($guid)
        $ghz = if ($ch -ge 1 -and $ch -le 14) { '2.4 GHz' } elseif ($ch -gt 14) { '5 GHz' } else { 'unknown' }
        "band $band -> start $($r.Status), state $($m.TetheringOperationalState), broadcasting on channel $ch ($ghz)"
        $null = Op ($m.StopTetheringAsync())
        Start-Sleep -Seconds 2
    }
}
finally {
    $m = Mgr
    $cfg = $m.GetCurrentAccessPointConfiguration(); $cfg.Band = 'TwoPointFourGigahertz'; Act ($m.ConfigureAccessPointAsync($cfg))
    if ($wasOn) { netsh wlan connect name="$wasOn" | Out-Null }
    Start-Sleep -Seconds 8
    "reconnected to: " + (((netsh wlan show interfaces) -match '^\s+SSID\s+:' | Select-Object -First 1) -replace '^\s+SSID\s+:\s*', '')
}
