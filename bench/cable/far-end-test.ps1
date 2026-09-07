# A longer offline window, to see whether the Debian box takes a lease.
#
# The 40-second window in the first harness was not long enough: a Linux
# machine that has already given up and self-assigned only asks again when
# something changes. Three minutes, and the cable is bounced to make it ask.

$ErrorActionPreference = 'Continue'
$SSID = 'ASK4 Wireless'
$LOG  = "$env:USERPROFILE\Desktop\DEBIAN-TEST-log.txt"
$EXE  = "$env:LOCALAPPDATA\Programs\PortableNetworkHub\hub.exe"
$SEND = 'C:\Users\gorilla1\Desktop\CABLE-TEST'
$PEER = '169.254.87.1'

function Say($s) { Write-Output "$s"; Add-Content -Path $LOG -Value "$s" }

$fallback = "PNH-wifi-rescue"
schtasks /Delete /TN $fallback /F 2>$null | Out-Null
schtasks /Create /TN $fallback /SC ONCE /ST ((Get-Date).AddMinutes(14).ToString("HH:mm")) /TR "netsh wlan connect name=`"$SSID`"" /F 2>$null | Out-Null

"" | Add-Content $LOG
Say "================================================================"
Say "debian lease test  $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Say "================================================================"

try {
  Get-Process hub -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep -Seconds 1

  netsh wlan disconnect | Out-Null
  ipconfig /release "Wi-Fi" | Out-Null
  Start-Sleep -Seconds 6
  Say "wifi off at $(Get-Date -Format 'HH:mm:ss')"

  Start-Process $EXE -ArgumentList 'cable',$SEND,'--name','Teacher' `
    -RedirectStandardOutput "$env:TEMP\debian-hub.txt" -NoNewWindow
  Start-Sleep -Seconds 10

  $dhcp = (Get-NetUDPEndpoint -LocalPort 67 -ErrorAction SilentlyContinue | Measure-Object).Count
  Say "address server running: $(if($dhcp -gt 0){'yes'}else{'NO'})"

  Say ""
  Say "bouncing the cable so the far end asks again"
  # Disable/enable needs rights we may not have; a release is enough to make
  # Windows re-announce, and the far end notices carrier either way.
  ipconfig /release "Ethernet" 2>&1 | Out-Null
  Start-Sleep -Seconds 3
  ipconfig /renew "Ethernet" 2>&1 | Out-Null
  Start-Sleep -Seconds 5

  Say ""
  Say "watching for three minutes"
  $seen = $false
  for ($i = 1; $i -le 18; $i++) {
    Start-Sleep -Seconds 10
    $arp = (arp -a | Select-String "169\.254\." | Where-Object { $_ -notmatch "ff-ff-ff" -and $_ -notmatch "Interface" })
    $ping = Test-Connection -ComputerName $PEER -Count 1 -Quiet -ErrorAction SilentlyContinue
    $line = "  t+{0,3}s  arp:{1,-3}  {2} replies: {3}" -f ($i*10), $(if($arp){"yes"}else{"no"}), $PEER, $(if($ping){"YES"}else{"no"})
    Say $line
    if ($arp) { foreach ($a in $arp) { Say ("          " + $a.ToString().Trim()) } }
    if ($ping) { $seen = $true; break }
  }

  Say ""
  if ($seen) {
    Say "THE FAR END IS UP. Fetching the page as it would."
    try {
      $r = Invoke-WebRequest -Uri "http://169.254.87.61/" -UseBasicParsing -TimeoutSec 10
      Say "  the page serves: HTTP $($r.StatusCode)"
    } catch { Say "  page failed: $($_.Exception.Message)" }
  } else {
    Say "THE FAR END DID NOT COME UP in three minutes."
  }

  Say ""
  Say "what the hub said:"
  Get-Content "$env:TEMP\debian-hub.txt" -ErrorAction SilentlyContinue | ForEach-Object { Say ("  " + $_) }
}
finally {
  Get-Process hub -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  netsh wlan connect name="$SSID" | Out-Null
  Start-Sleep -Seconds 12
  ipconfig /renew "Wi-Fi" | Out-Null
  Start-Sleep -Seconds 6
  $wifi = (Get-NetIPAddress -AddressFamily IPv4 -InterfaceAlias "Wi-Fi" -ErrorAction SilentlyContinue | Select-Object -First 1).IPAddress
  Say ""
  Say "wifi back: $(if($wifi){$wifi}else{'NONE - needs attention'})"
  schtasks /Delete /TN $fallback /F 2>$null | Out-Null
}
