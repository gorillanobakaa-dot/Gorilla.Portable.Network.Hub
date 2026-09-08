# hotspot-services-test.ps1 - does enabling the two services actually fix the
# hotspot?
#
# Version: 1.1.0 - updated 26-09-08-14-45
#
# WHY THIS EXISTS. 0.9.7 tells somebody with a tuned laptop to run three
# Set-Service lines, and until this was run that advice was reasoning rather
# than knowledge: those are the right services, so turning them back on OUGHT
# to work. This settles it, and settles it the only way that counts, by putting
# the machine back afterwards and confirming the fault returns.
#
# Result on the machine it was written for, 2026-09-08:
#
#   before   CreateFromConnectionProfile -> 0x83120001
#   after    manager created, ssid 'WIN-VR6KGAH3RAF 5437', band Auto,
#            8 max clients, and the hotspot actually switched On
#   undone   CreateFromConnectionProfile -> 0x83120001
#
# It does NOT read the Settings window. It asks the API that window is a front
# end for, so the evidence is the same failure code the red bar is reporting.
#
# Turns three services from Disabled back to Manual, asks Windows whether the
# hotspot is configurable, optionally starts and stops it, and then puts all
# three back to Disabled whatever happens.
#
# The restore is in a finally, and the before-state is captured rather than
# assumed, because a test that leaves a machine changed is worse than no test.

$ErrorActionPreference = 'Continue'
$log = Join-Path $PSScriptRoot 'hotspot-test.log'
function say($m) { $m | Tee-Object -FilePath $log -Append | Out-Host }
Remove-Item $log -ErrorAction SilentlyContinue

$services = 'icssvc','SharedAccess','WFDSConMgrSvc'

# --- capture what we are about to change, so the restore is exact ----------
$before = @{}
foreach ($s in $services) {
    $key = "HKLM:\SYSTEM\CurrentControlSet\Services\$s"
    $before[$s] = @{
        Start  = (Get-ItemProperty $key -ErrorAction SilentlyContinue).Start
        Status = (Get-Service $s -ErrorAction SilentlyContinue).Status
    }
}
say "== BEFORE =="
foreach ($s in $services) { say ("  {0,-15} Start={1}  {2}" -f $s, $before[$s].Start, $before[$s].Status) }

$started = $false
$mgr = $null

try {
    # --- the fix, exactly as the program tells people to type it -----------
    say ""
    say "== APPLYING =="
    foreach ($s in $services) {
        try { Set-Service $s -StartupType Manual -ErrorAction Stop; say "  Set-Service $s -StartupType Manual   ok" }
        catch { say "  Set-Service $s FAILED: $($_.Exception.Message)" }
    }
    try { Start-Service icssvc -ErrorAction Stop; say "  Start-Service icssvc   ok" }
    catch { say "  Start-Service icssvc FAILED: $($_.Exception.Message)" }

    Start-Sleep -Seconds 2
    say ""
    say "== STATE AFTER APPLYING =="
    foreach ($s in $services) {
        $svc = Get-Service $s -ErrorAction SilentlyContinue
        say ("  {0,-15} {1,-9} {2}" -f $s, $svc.Status, $svc.StartType)
    }

    # --- the actual question: can Windows configure a hotspot now? ---------
    # Before the change this threw 0x83120001, which is the same failure the
    # Settings page shows as "We can't set up mobile hotspot" with empty boxes.
    say ""
    say "== TETHERING MANAGER =="
    $null = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager,Windows.Networking,ContentType=WindowsRuntime]
    $prof = [Windows.Networking.Connectivity.NetworkInformation]::GetInternetConnectionProfile()
    say "  internet profile: $(if ($prof) { $prof.ProfileName } else { 'NONE' })"
    try {
        $mgr = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager]::CreateFromConnectionProfile($prof)
        say "  manager created: YES"
        say "  operational state: $($mgr.TetheringOperationalState)"
        say "  max clients: $($mgr.MaxClientCount)"
        $cfg = $mgr.GetCurrentAccessPointConfiguration()
        say "  ssid: '$($cfg.Ssid)'"
        say "  passphrase length: $($cfg.Passphrase.Length)"
        say "  band: $($cfg.Band)"
    } catch {
        say "  manager created: NO"
        say "  error: $($_.Exception.Message)"
    }

    # --- and does it actually turn on -------------------------------------
    if ($mgr) {
        say ""
        say "== STARTING THE HOTSPOT (briefly) =="
        # $started is set BEFORE the call, not after it succeeds.
        #
        # It was the other way round on the first run of this script and that is
        # exactly how it left a hotspot broadcasting. The poll compared
        # $op.Status against the integer 1, which never matched, so the script
        # concluded the start had failed, so the finally skipped
        # StopTetheringAsync, so Stop-Service could not stop a service that was
        # mid-tether. It printed NOT FULLY RESTORED, which is the one thing it
        # got right.
        #
        # The check that decides whether to clean up must never be the same
        # check that decides whether the thing worked.
        $started = $true
        try {
            $op = $mgr.StartTetheringAsync()
            $deadline = (Get-Date).AddSeconds(30)
            while ("$($op.Status)" -eq 'Started' -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 250 }
            say "  StartTetheringAsync finished with: $($op.Status)"
            try { say "  result: $($op.GetResults().Status)   (Success is what you want)" } catch {}
            Start-Sleep -Seconds 3
            say "  operational state now: $($mgr.TetheringOperationalState)"
            say "  clients connected: $($mgr.ClientCount)"
            $ssid = (netsh wlan show hostednetwork) 2>$null
            $iface = Get-NetAdapter | Where-Object { $_.Status -eq 'Up' } | Select-Object -ExpandProperty Name
            say "  adapters up: $($iface -join ', ')"
        } catch {
            say "  starting FAILED: $($_.Exception.Message)"
        }
    }
}
finally {
    # --- put everything back, whatever happened above ---------------------
    say ""
    say "== RESTORING =="
    if ($started -and $mgr) {
        try {
            $op = $mgr.StopTetheringAsync()
            $deadline = (Get-Date).AddSeconds(30)
            while ($op.Status -eq 0 -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 250 }
            say "  hotspot stopped, state: $($mgr.TetheringOperationalState)"
        } catch { say "  stopping FAILED: $($_.Exception.Message)" }
    }
    foreach ($s in $services) {
        try { Stop-Service $s -Force -ErrorAction SilentlyContinue } catch {}
    }
    foreach ($s in $services) {
        try {
            Set-Service $s -StartupType Disabled -ErrorAction Stop
        } catch {
            # Set-Service can refuse where the raw registry value will not.
            Set-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Services\$s" -Name Start -Value 4 -ErrorAction SilentlyContinue
        }
    }
    Start-Sleep -Seconds 1
    say ""
    say "== AFTER RESTORE =="
    $clean = $true
    foreach ($s in $services) {
        $key = "HKLM:\SYSTEM\CurrentControlSet\Services\$s"
        $now = (Get-ItemProperty $key -ErrorAction SilentlyContinue).Start
        $svc = Get-Service $s -ErrorAction SilentlyContinue
        say ("  {0,-15} Start={1}  {2}   (was Start={3} {4})" -f $s, $now, $svc.Status, $before[$s].Start, $before[$s].Status)
        if ($now -ne $before[$s].Start -or $svc.Status -ne $before[$s].Status) { $clean = $false }
    }
    say ""
    if ($clean) { say "RESTORED EXACTLY: every service is back where it started." }
    else        { say "*** NOT FULLY RESTORED - read the lines above ***" }

    say ""
    say "== WIFI STILL UP? =="
    say ((netsh wlan show interfaces | Select-String 'SSID|State' | Select-Object -First 2) -join ' | ')
    say (((Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -notlike '127.*' }) | ForEach-Object { "$($_.IPAddress) on $($_.InterfaceAlias)" }) -join ' | ')
}
