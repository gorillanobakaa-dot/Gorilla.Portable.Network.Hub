# isolate.ps1 - is WFDSConMgrSvc actually needed for the hotspot?
#
# The previous run set all three services to Manual and the hotspot worked, so
# it proved the SET works and proved nothing about any member of it. This one
# changes exactly two and leaves the third Disabled.
#
#   icssvc          Disabled -> Manual
#   SharedAccess    Disabled -> Manual
#   WFDSConMgrSvc   Disabled -> LEFT DISABLED
#
# If the hotspot still starts, that service comes out of the message the program
# prints and everybody gets one less line to type.
#
# Everything goes back to Disabled and Stopped in the finally, and $started is
# set BEFORE the call rather than after it succeeds, which is the bug that left
# a hotspot broadcasting last time.

$log = Join-Path $PSScriptRoot 'isolate.log'
Remove-Item $log -ErrorAction SilentlyContinue
function say($m) { $m | Out-File -FilePath $log -Append -Encoding utf8; $m | Out-Host }

$all = 'icssvc','SharedAccess','WFDSConMgrSvc'
$change = 'icssvc','SharedAccess'

function state($tag) {
    say "== $tag =="
    foreach ($s in $all) {
        $svc = Get-Service $s -ErrorAction SilentlyContinue
        $start = (Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Services\$s" -ErrorAction SilentlyContinue).Start
        say ("  {0,-15} Start={1}  {2,-9} {3}" -f $s, $start, $svc.Status, $svc.StartType)
    }
}

state 'BEFORE'

$started = $false
$mgr = $null
$verdict = 'inconclusive'

try {
    say ""
    say "== APPLYING (two of three) =="
    foreach ($s in $change) {
        try { Set-Service $s -StartupType Manual -ErrorAction Stop; say "  $s -> Manual" }
        catch { say "  $s FAILED: $($_.Exception.Message)" }
    }
    say "  WFDSConMgrSvc left Disabled on purpose"
    try { Start-Service icssvc -ErrorAction Stop; say "  Start-Service icssvc   ok" }
    catch { say "  Start-Service icssvc FAILED: $($_.Exception.Message)" }
    Start-Sleep -Seconds 2
    say ""
    state 'STATE UNDER TEST'

    say ""
    say "== CAN WINDOWS CONFIGURE A HOTSPOT? =="
    $null = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager,Windows.Networking,ContentType=WindowsRuntime]
    $prof = [Windows.Networking.Connectivity.NetworkInformation]::GetInternetConnectionProfile()
    say "  internet profile: $(if ($prof) { $prof.ProfileName } else { 'NONE' })"
    try {
        $mgr = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager]::CreateFromConnectionProfile($prof)
        say "  manager created: YES"
        $cfg = $mgr.GetCurrentAccessPointConfiguration()
        say "  ssid: '$($cfg.Ssid)'   passphrase length: $($cfg.Passphrase.Length)   band: $($cfg.Band)"
    } catch {
        say "  manager created: NO"
        say "  error: $($_.Exception.Message)"
        $verdict = 'NEEDED - the manager will not even build without it'
    }

    if ($mgr) {
        say ""
        say "== DOES IT ACTUALLY START? =="
        $started = $true          # set BEFORE the call. See the header.
        try {
            $op = $mgr.StartTetheringAsync()
            $deadline = (Get-Date).AddSeconds(45)
            while ("$($op.Status)" -eq 'Started' -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 250 }
            say "  async status: $($op.Status)"
            try { say "  result: $($op.GetResults().Status)" } catch { say "  result unreadable: $($_.Exception.Message)" }
            Start-Sleep -Seconds 4
            $now = "$($mgr.TetheringOperationalState)"
            say "  operational state: $now"
            $virt = Get-NetAdapter | Where-Object { $_.Status -eq 'Up' } | ForEach-Object { $_.Name }
            say "  adapters up: $($virt -join ', ')"
            # The virtual access point appears as its own adapter when a
            # hotspot is genuinely running. State alone could be optimistic;
            # an adapter that is up is harder to argue with.
            $hasVirtual = @($virt | Where-Object { $_ -like 'Local Area Connection*' }).Count -gt 0
            say "  virtual access point present: $hasVirtual"
            if ($now -eq 'On' -and $hasVirtual) {
                $verdict = 'NOT NEEDED - the hotspot started with it left Disabled'
            } elseif ($now -eq 'On') {
                $verdict = 'NOT NEEDED (state On, but no virtual adapter seen)'
            } else {
                $verdict = "NEEDED - configurable, but would not start (state $now)"
            }
        } catch {
            say "  starting FAILED: $($_.Exception.Message)"
            $verdict = 'NEEDED - starting threw'
        }
    }
}
finally {
    say ""
    say "== RESTORING =="
    if ($started -and $mgr) {
        try {
            $op = $mgr.StopTetheringAsync()
            $deadline = (Get-Date).AddSeconds(45)
            while ("$($op.Status)" -eq 'Started' -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 250 }
            Start-Sleep -Seconds 3
            say "  tethering stopped, state: $($mgr.TetheringOperationalState)"
        } catch { say "  stopping FAILED: $($_.Exception.Message)" }
    }
    foreach ($s in $all) {
        try { Stop-Service $s -Force -ErrorAction Stop; say "  $s stopped" }
        catch { if ((Get-Service $s).Status -ne 'Stopped') { say "  $s would not stop: $($_.Exception.Message)" } }
    }
    foreach ($s in $all) {
        try { Set-Service $s -StartupType Disabled -ErrorAction Stop }
        catch { Set-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Services\$s" -Name Start -Value 4 -ErrorAction SilentlyContinue }
    }
    Start-Sleep -Seconds 2
    say ""
    state 'AFTER RESTORE'
    $clean = $true
    foreach ($s in $all) {
        $svc = Get-Service $s -ErrorAction SilentlyContinue
        $start = (Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Services\$s" -ErrorAction SilentlyContinue).Start
        if ($start -ne 4 -or $svc.Status -ne 'Stopped') { $clean = $false }
    }
    say ""
    if ($clean) { say "RESTORED: all three Disabled and Stopped." }
    else        { say "*** NOT FULLY RESTORED ***" }
    say ""
    say "VERDICT for WFDSConMgrSvc: $verdict"
    say ""
    say "== NETWORK =="
    say ((netsh wlan show interfaces | Select-String 'SSID|State' | Select-Object -First 2) -join ' | ')
}
