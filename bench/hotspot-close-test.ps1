# Version: 1.0.0 · updated 26-09-23-13-00
#
# Does the Windows hotspot go OFF when the hub ends without stopping it?
#
# The hub switches Mobile Hotspot on itself (0.9.9). Closing its window with
# the X ends it without running any of its own code, so the hotspot used to
# stay on. A watchman process now waits for the hub to end and switches the
# hotspot off. This proves it, two ways:
#
#   kill   the hub is killed outright (Task Manager's End task)
#   close  the hub runs in its own window, and that window is sent WM_CLOSE,
#          which is exactly what clicking its X sends
#
# Each: start the hub serving over its own hotspot, check the hotspot is On
# and the watchman exists, end the hub, then time how long until the hotspot
# is Off. Whatever happens, the finally block switches the hotspot off and
# ends any hub it started, and says so if it had to.
#
#   powershell -ExecutionPolicy Bypass -File bench\hotspot-close-test.ps1 -Hub <path to hub.exe> -Folder <folder>

param(
    [Parameter(Mandatory)] [string]$Hub,
    [Parameter(Mandatory)] [string]$Folder
)
$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Runtime.WindowsRuntime
$opT = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
$null = [Windows.Networking.Connectivity.NetworkInformation,Windows.Networking.Connectivity,ContentType=WindowsRuntime]
$null = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager,Windows.Networking,ContentType=WindowsRuntime]
$Res = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult]
$NI = [Windows.Networking.Connectivity.NetworkInformation]
$TM = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager]

function State {
    # On if the tethering manager of ANY profile says On.
    foreach ($p in $NI::GetConnectionProfiles()) {
        try { $m = $TM::CreateFromConnectionProfile($p) } catch { continue }
        if ($m.TetheringOperationalState -ne 'Off') { return [string]$m.TetheringOperationalState }
    }
    'Off'
}
function StopAll {
    foreach ($p in $NI::GetConnectionProfiles()) {
        try { $m = $TM::CreateFromConnectionProfile($p) } catch { continue }
        if ($m.TetheringOperationalState -ne 'Off') {
            $t = $opT.MakeGenericMethod($Res).Invoke($null, @($m.StopTetheringAsync())); $null = $t.Wait(30000)
        }
    }
}
function WaitFor($what, [scriptblock]$cond, $seconds) {
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $seconds) {
        if (& $cond) { return [int]((Get-Date) - $t0).TotalMilliseconds }
        Start-Sleep -Milliseconds 250
    }
    -1
}
function Watchmen {
    @(Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object { $_.CommandLine -match 'EncodedCommand' -and $_.ProcessId -ne $PID })
}

Add-Type @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class Win {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  public static IntPtr Find(string title) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { if (!IsWindowVisible(h)) return true; var sb = new StringBuilder(256); GetWindowText(h, sb, 256);
      if (sb.ToString().Contains(title)) { found = h; return false; } return true; }, IntPtr.Zero);
    return found;
  }
}
'@

$results = @()
try {
    foreach ($how in 'kill', 'close') {
        StopAll; Start-Sleep -Seconds 2
        $title = "HUB-CLOSE-TEST-$how"
        # Its own console window, as a person running it would have.
        $inner = "title $title & `"$Hub`" serve `"$Folder`" --name `"Gorilla Hub`" --password `"gorilla126`""
        $shell = Start-Process cmd.exe -ArgumentList '/c', $inner -PassThru
        $up = WaitFor 'hotspot on' { (State) -eq 'On' } 40
        # The watchman is started once the hub has finished setting up, a few
        # seconds after the hotspot itself comes on.
        $null = WaitFor 'watchman' { @(Watchmen).Count -gt 0 } 30
        # Not $hub: PowerShell ignores case, and $hub IS the -Hub parameter.
        $running = Get-Process hub -ErrorAction SilentlyContinue | Select-Object -First 1
        $watch = @(Watchmen).Count
        "[$how] hotspot On after $up ms; hub running: $([bool]$running); watchmen: $watch"
        Start-Sleep -Seconds 2
        if ($how -eq 'kill') {
            Stop-Process -Id $running.Id -Force
        } else {
            $h = [Win]::Find($title)
            "[$how] window found: $($h -ne [IntPtr]::Zero)"
            $null = [Win]::PostMessage($h, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)   # WM_CLOSE, what the X sends
        }
        $gone = WaitFor 'hub gone' { -not (Get-Process hub -ErrorAction SilentlyContinue) } 15
        $off = WaitFor 'hotspot off' { (State) -eq 'Off' } 30
        $left = @(Watchmen).Count
        "[$how] hub ended after $gone ms; hotspot Off after $off ms (after the hub ended); watchmen left: $left"
        $results += [pscustomobject]@{ How = $how; HubEnded = ($gone -ge 0); HotspotOff = ($off -ge 0); WatchmenLeft = $left }
    }
}
finally {
    $stray = Get-Process hub -ErrorAction SilentlyContinue
    if ($stray) { $stray | ForEach-Object { Stop-Process -Id $_.Id -Force }; '*** had to end a hub this test started ***' }
    if ((State) -ne 'Off') { StopAll; '*** had to switch the hotspot off myself: the watchman did not ***' }
    "final hotspot state: $(State)"
}
$results | Format-Table -AutoSize
