# Run the cable test with the wifi genuinely off, then put the wifi back.
#
# Claude cannot be online while the wifi is off, so this does the whole thing
# without supervision and writes down what it saw. It must restore the wifi
# whatever happens: a failure here leaves the machine with no way to reach
# anybody, which is worse than a failed test.

$ErrorActionPreference = 'Continue'
$SSID = 'ASK4 Wireless'
$LOG  = "$env:USERPROFILE\Desktop\OFFLINE-TEST-log.txt"
$EXE  = "$env:LOCALAPPDATA\Programs\PortableNetworkHub\hub.exe"
$SEND = 'C:\Users\gorilla1\Desktop\CABLE-TEST'

function Say($s) { $line = "$s"; Write-Output $line; Add-Content -Path $LOG -Value $line }

# Belt and braces: a scheduled task that turns the wifi back on in 12 minutes
# even if this script is killed outright.
$fallback = "PNH-wifi-rescue"
schtasks /Delete /TN $fallback /F 2>$null | Out-Null
$when = (Get-Date).AddMinutes(12).ToString("HH:mm")
schtasks /Create /TN $fallback /SC ONCE /ST $when /TR "netsh wlan connect name=`"$SSID`"" /F 2>$null | Out-Null

"" | Add-Content $LOG
Say "================================================================"
Say "offline cable test  $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Say "================================================================"

function Snapshot($tag) {
  Say ""
  Say "--- $tag  [$(Get-Date -Format 'HH:mm:ss')] ---"
  $ips = Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue |
         Where-Object { $_.IPAddress -notlike "127.*" }
  foreach ($i in $ips) { Say ("   configured   {0,-16} on {1}" -f $i.IPAddress, $i.InterfaceAlias) }
  $cfg = (ipconfig) -join "`n"
  foreach ($i in $ips) {
    $seen = $cfg -match [regex]::Escape($i.IPAddress)
    Say ("   connected?   {0,-16} {1}" -f $i.IPAddress, $(if($seen){"yes (ipconfig lists it)"}else{"NO (adapter is down)"}))
  }
  $gw = (Get-NetRoute -DestinationPrefix "0.0.0.0/0" -ErrorAction SilentlyContinue | Select-Object -First 1).NextHop
  Say ("   route out    {0}" -f $(if($gw){$gw}else{"none"}))
  foreach ($p in @(67,53,5353)) {
    $n = (Get-NetUDPEndpoint -LocalPort $p -ErrorAction SilentlyContinue | Measure-Object).Count
    $what = switch ($p) { 67 {"addresses (dhcp)"} 53 {"names (dns)"} 5353 {"gorilla.local"} }
    Say ("   {0,-16} {1}" -f $what, $(if($n -gt 0){"RUNNING"}else{"not running"}))
  }
  $arp = (arp -a | Select-String "169\.254\." | Where-Object { $_ -notmatch "ff-ff-ff" })
  if ($arp) { foreach ($a in $arp) { Say ("   on the cable {0}" -f $a.ToString().Trim()) } }
  else { Say "   on the cable nobody" }
}

try {
  Get-Process hub -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep -Seconds 1

  Say ""
  Say "STEP 1  start the hub while the wifi is still up"
  Start-Process $EXE -ArgumentList 'cable',$SEND,'--name','Teacher' `
    -RedirectStandardOutput "$env:TEMP\offline-hub.txt" -NoNewWindow
  Start-Sleep -Seconds 8
  Snapshot "with wifi UP"

  Say ""
  Say "STEP 2  turn the wifi off, the way a person does"
  netsh wlan disconnect | Out-Null
  # A disconnected adapter keeps its address on Windows, which is the whole
  # bug being tested. Release it too, so the machine is genuinely off-network.
  ipconfig /release "Wi-Fi" | Out-Null
  Start-Sleep -Seconds 6
  Snapshot "just after switching the wifi off"

  Say ""
  Say "STEP 3  wait for the supervisor to notice, up to 40 seconds"
  $started = $false
  for ($i = 0; $i -lt 8; $i++) {
    Start-Sleep -Seconds 5
    $n = (Get-NetUDPEndpoint -LocalPort 67 -ErrorAction SilentlyContinue | Measure-Object).Count
    if ($n -gt 0) { $started = $true; Say ("   noticed after about {0} seconds" -f (($i+1)*5)); break }
  }
  if (-not $started) { Say "   NOT NOTICED after 40 seconds" }
  Snapshot "after waiting"

  Say ""
  Say "STEP 4  what the hub itself is saying"
  Get-Content "$env:TEMP\offline-hub.txt" -ErrorAction SilentlyContinue |
    ForEach-Object { Say ("   " + $_) }

  Say ""
  Say "STEP 5  does gorilla.local answer, and does the page serve?"
  $cable = (Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue |
            Where-Object { $_.IPAddress -like "169.254.*" -and $_.InterfaceAlias -eq "Ethernet" } |
            Select-Object -First 1).IPAddress
  Say "   cable address: $cable"
  if ($cable) {
    $py = @"
import socket
q = bytearray([0,0,0,0,0,1,0,0,0,0,0,0])
for l in 'gorilla.local'.split('.'):
    q.append(len(l)); q += l.encode()
q.append(0); q += (1).to_bytes(2,'big') + (0x8001).to_bytes(2,'big')
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(('$cable', 0))
s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF, socket.inet_aton('$cable'))
s.settimeout(5)
s.sendto(bytes(q), ('224.0.0.251', 5353))
try:
    d, a = s.recvfrom(2048); n = len(d)
    print('gorilla.local -> ' + '.'.join(str(b) for b in d[n-4:n]))
except socket.timeout:
    print('gorilla.local -> NO ANSWER')
"@
    $py | Out-File -Encoding ascii "$env:TEMP\mdns.py"
    Say ("   " + (python "$env:TEMP\mdns.py" 2>&1))
    foreach ($h in @('gorilla.local','gorilla','hub.local')) {
      try {
        $r = Invoke-WebRequest -Uri "http://$cable/" -Headers @{Host=$h} -UseBasicParsing -TimeoutSec 8
        Say ("   Host: {0,-14} HTTP {1}" -f $h, $r.StatusCode)
      } catch { Say ("   Host: {0,-14} failed: {1}" -f $h, $_.Exception.Message) }
    }
    try {
      $r = Invoke-WebRequest -Uri "http://$cable/" -Headers @{Host='nmcheck.gnome.org'} -UseBasicParsing -TimeoutSec 8 -MaximumRedirection 0
      Say ("   captive probe   HTTP {0}" -f $r.StatusCode)
    } catch {
      $code = $_.Exception.Response.StatusCode.value__
      Say ("   captive probe   HTTP {0} (302 is what makes the prompt appear)" -f $code)
    }
  }

  Say ""
  Say "STEP 6  did the Debian box take an address from us?"
  Start-Sleep -Seconds 20
  $arp = (arp -a | Select-String "169\.254\." | Where-Object { $_ -notmatch "ff-ff-ff" })
  if ($arp) { foreach ($a in $arp) { Say ("   " + $a.ToString().Trim()) } } else { Say "   nothing on the cable yet" }
  $ping = (Test-Connection -ComputerName "169.254.87.1" -Count 2 -Quiet -ErrorAction SilentlyContinue)
  Say ("   169.254.87.1 replies: {0}" -f $(if($ping){"YES"}else{"no"}))
}
finally {
  Say ""
  Say "STEP 7  putting the wifi back"
  Get-Process hub -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  netsh wlan connect name="$SSID" | Out-Null
  Start-Sleep -Seconds 12
  ipconfig /renew "Wi-Fi" | Out-Null
  Start-Sleep -Seconds 6
  $wifi = (Get-NetIPAddress -AddressFamily IPv4 -InterfaceAlias "Wi-Fi" -ErrorAction SilentlyContinue | Select-Object -First 1).IPAddress
  Say ("   wifi address: {0}" -f $(if($wifi){$wifi}else{"NONE - needs attention"}))
  schtasks /Delete /TN $fallback /F 2>$null | Out-Null
  Say "done $(Get-Date -Format 'HH:mm:ss')"
}
