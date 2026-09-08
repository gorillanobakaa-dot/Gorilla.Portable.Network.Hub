# shot2.ps1 - photograph the hub's own screens on real Windows.
#
# WHY THE FIRST ATTEMPT FOUND NO WINDOW. It looked for the cmd.exe process's
# MainWindowHandle. A console window does not belong to cmd.exe: it belongs to
# conhost.exe, and on a Windows 11 where Windows Terminal is the default host it
# belongs to WindowsTerminal.exe and is a TAB rather than a window of its own.
# So the handle was legitimately zero.
#
# This finds the window by its TITLE instead, over every top-level window,
# whatever process owns it, and launches through conhost.exe explicitly so the
# result is one classic console window with a title we chose.
#
# It captures the window rectangle only, never the whole desktop, so nothing
# else on the machine ends up in a published image.

param([string]$OutDir = (Join-Path $PSScriptRoot 'shots'))

$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public struct RECT { public int Left, Top, Right, Bottom; }
public class Win {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int size);
    // The TRUE visible edge of the window.
    //
    // GetWindowRect returns a rectangle several pixels wider than what is drawn:
    // Windows keeps an invisible resize border outside the frame. Capturing that
    // rectangle picks up whatever is BEHIND the window in those pixels, so a
    // published screenshot carries a sliver of somebody else's window down each
    // side. DWMWA_EXTENDED_FRAME_BOUNDS (9) is the rectangle actually painted.
    public static RECT VisibleRect(IntPtr h) {
        RECT r;
        if (DwmGetWindowAttribute(h, 9, out r, Marshal.SizeOf(typeof(RECT))) == 0) return r;
        GetWindowRect(h, out r);
        return r;
    }
    public static IntPtr FindByTitle(string needle) {
        IntPtr found = IntPtr.Zero;
        EnumWindows(delegate(IntPtr h, IntPtr l) {
            if (!IsWindowVisible(h)) return true;
            StringBuilder sb = new StringBuilder(512);
            GetWindowText(h, sb, sb.Capacity);
            if (sb.ToString().IndexOf(needle, StringComparison.OrdinalIgnoreCase) >= 0) {
                found = h; return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
"@

function Capture-Console {
    param(
        [string]$Name,
        [string]$Title,
        [string]$WorkDir,
        [string]$Command,
        [int]$Cols = 84,
        [int]$Rows = 40,
        [int]$SettleSeconds = 4,
        [switch]$KeepRunning
    )

    # The title is what a reader sees in the published image, so it is set to
    # something a person would recognise rather than to a marker of mine. cmd
    # appends the command it is running to that title, which is why the working
    # directory is changed first: the suffix then reads " - hub.exe doctor"
    # rather than a sixty-character path.
    $inner = "cd /d `"$WorkDir`" & title $Title & mode con: cols=$Cols lines=$Rows & cls & $Command"
    $marker = $Title
    if (-not $KeepRunning) { $inner += " & pause > nul" }

    # conhost explicitly, so this is one classic console window and not a tab
    # inside whatever terminal happens to be the default.
    $p = Start-Process -FilePath conhost.exe -ArgumentList "cmd.exe /k `"$inner`"" -PassThru -WindowStyle Normal

    $h = [IntPtr]::Zero
    $deadline = (Get-Date).AddSeconds(25)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 400
        $h = [Win]::FindByTitle($marker)
        if ($h -ne [IntPtr]::Zero) { break }
    }
    if ($h -eq [IntPtr]::Zero) {
        Write-Host "  [$Name] no window found by title"
        try { $p | Stop-Process -Force -ErrorAction SilentlyContinue } catch {}
        return
    }

    [Win]::ShowWindow($h, 9) | Out-Null
    [Win]::SetForegroundWindow($h) | Out-Null

    # Move it to the top-left before measuring.
    #
    # A new console is placed by the cascade, well down the screen, so a window
    # tall enough to hold all of doctor's output ran off the bottom and the
    # capture rectangle covered the taskbar: the owner's tray icons, with an
    # unread badge on one of them, in an image about to be published. The size
    # was never the problem. The position was.
    $r0 = New-Object RECT
    [Win]::GetWindowRect($h, [ref]$r0) | Out-Null
    [Win]::MoveWindow($h, 12, 12, ($r0.Right - $r0.Left), ($r0.Bottom - $r0.Top), $true) | Out-Null
    Start-Sleep -Seconds $SettleSeconds

    $r = [Win]::VisibleRect($h)
    $w = $r.Right - $r.Left; $ht = $r.Bottom - $r.Top
    if ($w -le 0 -or $ht -le 0) { Write-Host "  [$Name] window has no size"; return }

    $bmp = New-Object System.Drawing.Bitmap $w, $ht
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $ht)))
    $g.Dispose()
    $path = Join-Path $OutDir "$Name.png"
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()

    # Close everything this spawned: the console host and any hub under it.
    try { $p | Stop-Process -Force -ErrorAction SilentlyContinue } catch {}
    Get-Process hub -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -like '*PortableNetworkHub*' } |
        Stop-Process -Force -ErrorAction SilentlyContinue

    Write-Host ("  [{0}] {1} x {2}, {3:N0} bytes" -f $Name, $w, $ht, (Get-Item $path).Length)
}

$hub = 'C:\Users\gorilla1\AppData\Local\Programs\PortableNetworkHub\hub.exe'

Write-Host "== capturing =="
$dir = Split-Path $hub -Parent
# -KeepRunning, so no "pause" is appended: cmd /k keeps the window open on its
# own and the title stays "hub doctor" instead of "hub doctor - pause".
# 52 rows, so nothing scrolls off the top: the version line is the first
# thing a reader should see. Fitting it needs the window moved to the top-left
# first, which Capture-Console now does.
Capture-Console -Name 'hub-doctor-switched-off-services' -Title 'hub doctor' -WorkDir $dir `
                -Command 'hub.exe doctor' -Cols 84 -Rows 52 -SettleSeconds 5 -KeepRunning
Capture-Console -Name 'hub-first-screen' -Title 'Portable Network Hub' -WorkDir $dir `
                -Command 'hub.exe' -Cols 84 -Rows 28 -SettleSeconds 6 -KeepRunning
Write-Host "== done =="
Get-ChildItem $OutDir -Filter *.png | Select-Object Name,Length | Format-Table -AutoSize | Out-String -Width 60
