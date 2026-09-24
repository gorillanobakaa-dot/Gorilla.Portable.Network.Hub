# Version: 1.2.0 · updated 26-09-24-12-30
#
# A tour of the hub's screens on real Windows, photographed for the release
# pages: drives the real program with keypresses and photographs its window.
# Kept for reuse at every release; meant to become part of the harness.
#
# Privacy, because these pictures are published:
#   - Only the hub's own window is captured (PrintWindow on that window), never
#     the desktop, so nothing else on the machine can appear.
#   - The hub runs with USERPROFILE pointed at a demo folder, so every path on
#     screen is the demo's, not the machine owner's.
#   - The network password shown is a demo one, typed in for the tour. The
#     Windows hotspot's saved name and password are read first and put back at
#     the end, whatever happens (finally), so the owner's devices still rejoin.
#
# KEYS GO TO THE HUB'S CONSOLE ONLY. Version 1.0.0 used SendKeys after
# SetForegroundWindow. Windows does not let a background process take the
# foreground, so every key (arrows, Enter, 30 backspaces, the demo password)
# went into the window the person at the laptop was using, 2026-09-24. Now
# console-keys.cs writes into the hub's own console input buffer, which needs
# no focus and cannot reach any other window, and Keys refuses to type at all
# unless the hub this tour started is still running.
#
# Steps: first screen; the fix screen; the start screen on the band row and on
# Start; the file list (columns, then the big-folder question); handing out
# with the codes and the measured channel; the h help page; work sent in from
# this laptop through the page (curl, as a phone would); the phone pages
# (screenshot-phone-page.py: name, page, downloads help, choosing, sending,
# arrived); the waiting screen; accept all. Then Stop and quit.
#
# Pictures are named windows-<version>-<step>.png and phone-<version>-<step>.png
# with the version read from the hub itself. 1.1.0 had 0.9.9 written in.
#
#   powershell -ExecutionPolicy Bypass -File bench\screenshot-tour.ps1 -Hub <hub.exe> -Demo C:\GorillaHubDemo -OutDir docs\screenshots\gallery
#
# The demo folder is <Demo>\Teacher\Documents\Year 7 science, with a folder of
# 20+ files in it (the big-folder question), and <Demo>\Teacher\Documents
# existing so the received folder lands there.

param(
    [Parameter(Mandatory)] [string]$Hub,
    [Parameter(Mandatory)] [string]$Demo,
    [Parameter(Mandatory)] [string]$OutDir,
    [int]$Cols = 112,
    [int]$Rows = 44,
    # 'main' (the tour above) or 'not-ready' (first screen with services
    # switched off, the offer to fix, and the result). See the not-ready block.
    [ValidateSet('main', 'not-ready')] [string]$Scene = 'main',
    # Read from `hub --version` when not given.
    [string]$Ver = ''
)
$ErrorActionPreference = 'Stop'
if (-not $Ver) { $Ver = ((& $Hub --version) -split '\s+')[1] }
"pictures for hub $Ver"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public struct RECT { public int Left, Top, Right, Bottom; }
public static class TourWin {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int size);
    public static RECT Rect(IntPtr h) { RECT r; if (DwmGetWindowAttribute(h, 9, out r, Marshal.SizeOf(typeof(RECT))) == 0) return r; GetWindowRect(h, out r); return r; }
    public static RECT Outer(IntPtr h) { RECT r; GetWindowRect(h, out r); return r; }
    public static IntPtr Find(string t) { IntPtr f = IntPtr.Zero; EnumWindows((h, l) => { if (!IsWindowVisible(h)) return true;
        var sb = new StringBuilder(512); GetWindowText(h, sb, 512); if (sb.ToString().Contains(t)) { f = h; return false; } return true; }, IntPtr.Zero); return f; }
}
"@

$keysExe = Join-Path $env:TEMP 'console-keys.exe'
Add-Type -TypeDefinition (Get-Content (Join-Path $PSScriptRoot 'console-keys.cs') -Raw) -OutputType ConsoleApplication -OutputAssembly $keysExe
$script:hubPid = 0

# In $k: \e Escape, \r Enter, \b Backspace, \u \d the up and down arrows.
function Keys([string]$k, [int]$after = 700) {
    if (-not $script:hubPid -or -not (Get-Process -Id $script:hubPid -ErrorAction SilentlyContinue)) {
        throw "the tour's hub is not running; not sending '$k' anywhere"
    }
    & $keysExe $script:hubPid $k
    if ($LASTEXITCODE -ne 0) { throw "could not type into the hub's console (code $LASTEXITCODE)" }
    Start-Sleep -Milliseconds $after
}
function Shot([string]$name) {
    if (-not (Get-Process -Id $script:hubPid -ErrorAction SilentlyContinue)) { throw "the tour's hub has gone; no picture of $name" }
    # PrintWindow with PW_RENDERFULLCONTENT (2): the window's own pixels, even
    # if something else overlaps it. Cropped to the visible frame.
    $o = [TourWin]::Outer($script:win); $v = [TourWin]::Rect($script:win)
    $bmp = New-Object System.Drawing.Bitmap ($o.Right - $o.Left), ($o.Bottom - $o.Top)
    $g = [System.Drawing.Graphics]::FromImage($bmp); $hdc = $g.GetHdc()
    [void][TourWin]::PrintWindow($script:win, $hdc, 2); $g.ReleaseHdc($hdc); $g.Dispose()
    $crop = New-Object System.Drawing.Rectangle ($v.Left - $o.Left), ($v.Top - $o.Top), ($v.Right - $v.Left), ($v.Bottom - $v.Top)
    $out = $bmp.Clone($crop, $bmp.PixelFormat); $bmp.Dispose()
    $path = Join-Path $OutDir "$name.png"; $out.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $out.Dispose()
    "  shot  $name.png"
}

# The saved hotspot settings, to put back at the end.
$hs = @'
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$ext = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 }
$actT = $ext | Where-Object { $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncAction' } | Select-Object -First 1
$null = [Windows.Networking.Connectivity.NetworkInformation,Windows.Networking.Connectivity,ContentType=WindowsRuntime]
$null = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager,Windows.Networking,ContentType=WindowsRuntime]
foreach ($p in [Windows.Networking.Connectivity.NetworkInformation]::GetConnectionProfiles()) { try { $m = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager]::CreateFromConnectionProfile($p); break } catch { } }
$c = $m.GetCurrentAccessPointConfiguration()
if ($env:HS_SET) { $c.Ssid = $env:HS_NAME; $c.Passphrase = $env:HS_PASS; $t = $actT.Invoke($null, @($m.ConfigureAccessPointAsync($c))); $null = $t.Wait(30000); 'restored' }
else { "$($c.Ssid)`n$($c.Passphrase)" }
'@
$hsFile = Join-Path $env:TEMP 'tour-hotspot.ps1'; Set-Content $hsFile $hs -Encoding utf8
$saved = @((powershell -NoProfile -ExecutionPolicy Bypass -File $hsFile 2>$null) -split "`n")
"saved hotspot settings read (name '$($saved[0].Trim())')"

$home_ = Join-Path $Demo 'Teacher'
$lesson = Join-Path $home_ 'Documents\Year 7 science'
$title = 'HUB-SCREENSHOT-TOUR'
# PSModuleAnalysisCachePath: with USERPROFILE moved, the PowerShell the hub
# runs wrote its module cache to Microsoft\Windows\PowerShell in the CURRENT
# folder, which here is the lesson folder, and the next tour photographed a
# "Microsoft" folder among the lesson's files (2026-09-24). Only under the
# tour's moved profile; normal runs leave nothing behind. Sent to TEMP instead.
$stray = Join-Path $lesson 'Microsoft\Windows\PowerShell\ModuleAnalysisCache'
if (Test-Path $stray) { Remove-Item $stray; Remove-Item (Join-Path $lesson 'Microsoft') -Recurse -Force }
$inner = "title $title & mode con: cols=$Cols lines=$Rows & set USERPROFILE=$home_& set PSModuleAnalysisCachePath=$env:TEMP\tour-ps-cache& cd /d `"$lesson`" & `"$Hub`""
$proc = Start-Process conhost.exe -ArgumentList "cmd.exe /c `"$inner`"" -PassThru
try {
    $tries = 0
    do { Start-Sleep -Milliseconds 250; $script:win = [TourWin]::Find($title); $tries++ } until ($script:win -ne [IntPtr]::Zero -or $tries -gt 40)
    if ($script:win -eq [IntPtr]::Zero) { throw "the tour window never appeared" }
    # The hub this tour started: a hub.exe whose parent cmd carries our title.
    for ($i = 0; $i -lt 40 -and -not $script:hubPid; $i++) {
        Start-Sleep -Milliseconds 250
        $script:hubPid = (Get-CimInstance Win32_Process -Filter "Name='hub.exe'" | Where-Object {
            $pp = Get-CimInstance Win32_Process -Filter "ProcessId=$($_.ParentProcessId)"
            $pp -and $pp.CommandLine -like "*$title*" } | Select-Object -First 1).ProcessId
    }
    if (-not $script:hubPid) { throw "could not find the tour's hub process" }
    "tour hub pid $script:hubPid"
    [void][TourWin]::MoveWindow($script:win, 20, 20, 1100, 900, $true)
    Start-Sleep -Seconds 4                                   # the services check at start

    if ($Scene -eq 'not-ready') {
        # Run after `hub services --put-back` has switched the hotspot
        # services off, as a tweak list leaves them. The fix itself raises a
        # Windows permission prompt that the person at the laptop answers.
        Shot "windows-$Ver-not-ready"
        Keys '\r' 1500                                       # wifi: stops at the offer to fix
        Shot "windows-$Ver-fix-offer"
        Keys '\r' 1000                                       # switch them on: UAC appears
        "  waiting for the permission prompt to be answered..."
        $t0 = Get-Date
        do { Start-Sleep -Seconds 2; $done = (& $Hub services) -match 'Everything the hub needs is switched on' } until ($done -or ((Get-Date) - $t0).TotalSeconds -gt 180)
        Start-Sleep -Seconds 3
        Shot "windows-$Ver-fix-done"
        Keys '\r' 800; Keys '\e' 1200; Keys 'q' 2000
        return
    }

    Shot "windows-$Ver-first-screen"
    Keys '\d\d\d\r' 3000                                     # Fix problems with this computer
    Shot "windows-$Ver-fix-problems"
    Keys '\e' 1200
    Keys '\u\u\u\r' 1500                                     # Hand out files over wifi
    # A demo password, typed, so the owner's real one never appears.
    Keys '\d\d\r'
    Keys ('\b' * 30)
    Keys 'leafy7green\r'
    Keys '\d' 800                                            # the Wifi band row
    Shot "windows-$Ver-start-screen-band"
    Keys '\d\d\d' 800                                        # onto Start, which says what happens next
    Shot "windows-$Ver-start-screen-start"
    Keys '\r' 2500                                           # Start handing out -> what gets handed out
    Keys 'n' 800                                             # untick all, to show choosing
    Shot "windows-$Ver-folder-view"
    Keys '\d' 500
    Keys ' ' 800                                             # a folder of 36 files: asks first
    Shot "windows-$Ver-big-folder-question"
    Keys ' ' 800                                             # yes, all of them
    Keys 'a' 500; Keys 'a' 800                               # and everything else here (asks, yes)
    Keys '\u\u\u\u\u\u' 500                                  # up to CONTINUE
    Keys '\r' 18000                                          # the network comes up; the channel is measured
    Shot "windows-$Ver-handing-out-codes"
    Keys 'h' 1000                                            # the help page: every key in words
    Shot "windows-$Ver-help"
    Keys '\r' 800

    # Work sent in from this laptop, through the page, as a phone would.
    $jar = Join-Path $env:TEMP 'tour-cookies.txt'
    curl.exe -s -c $jar -b $jar -o NUL -d "who=Amina" http://127.0.0.1/name
    $page = curl.exe -s -c $jar -b $jar http://127.0.0.1/
    $token = ([regex]::Match(($page -join "`n"), 'action="/handin".*?name="token" value="([^"]+)"', 'Singleline')).Groups[1].Value
    $tmp = Join-Path $env:TEMP 'tour-work'; New-Item -ItemType Directory -Force $tmp | Out-Null
    foreach ($n in 'Leaf drawing.jpg', 'Photosynthesis answers.docx', 'Label the leaf.pdf') { Set-Content (Join-Path $tmp $n) ('demo ' * 2000) }
    curl.exe -s -o NUL -c $jar -b $jar -F "token=$token" -F "work=@$tmp\Leaf drawing.jpg" -F "work=@$tmp\Photosynthesis answers.docx" -F "work=@$tmp\Label the leaf.pdf" http://127.0.0.1/handin
    "  sent three pieces of work through the page"
    python (Join-Path $PSScriptRoot 'screenshot-phone-page.py') --url http://127.0.0.1/ --prefix (Join-Path $OutDir "phone-$Ver")
    Start-Sleep -Seconds 3
    Shot "windows-$Ver-work-arrived"
    Keys 'w' 1200
    Shot "windows-$Ver-waiting-accept-all"
    Keys 'e' 1500
    Shot "windows-$Ver-accepted-all"
    Keys '\r' 800
    Keys '\e' 1200
    Keys 'q' 4000                                            # Stop
    Keys '\r' 800
    Keys 'q' 2000                                            # quit
}
finally {
    # Only what this tour started: its hub by pid, then its console.
    if ($script:hubPid -and (Get-Process -Id $script:hubPid -ErrorAction SilentlyContinue)) { Stop-Process -Id $script:hubPid -Force; '  ended the tour hub' }
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Seconds 8                                   # the watchman switches the network off
    # Only put back what was actually read. With the hotspot services
    # switched off (the not-ready scene) nothing can be read, and "putting
    # back" empty values would try to wipe the owner's settings.
    if ($saved.Count -ge 2 -and $saved[0].Trim() -and $saved[1].Trim()) {
        $env:HS_SET = '1'; $env:HS_NAME = $saved[0].Trim(); $env:HS_PASS = $saved[1].Trim()
        "hotspot settings put back: $(powershell -NoProfile -ExecutionPolicy Bypass -File $hsFile)"
        Remove-Item Env:HS_SET, Env:HS_NAME, Env:HS_PASS -ErrorAction SilentlyContinue
    } else {
        "hotspot settings could not be read at the start, so nothing was put back"
    }
}
