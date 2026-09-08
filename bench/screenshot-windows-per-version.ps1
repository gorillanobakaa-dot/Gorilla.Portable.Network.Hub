# shot3.ps1 - one true screenshot per released version.
#
# A picture of 0.9.8 on the 0.9.6 release page would be a small lie, and this
# project's whole method is not telling those. Each release's own published zip
# is unpacked and run, so every page shows the binary it is actually offering.
#
# The progression is the point:
#   0.9.6  the wifi adapter block, and NO advice below the list
#   0.9.7  the advice, naming THREE services
#   0.9.8  the same advice, naming TWO, because the third was measured out

$ErrorActionPreference = 'Stop'
$root   = $PSScriptRoot
$OutDir = Join-Path $root 'shots'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public struct RECT { public int Left, Top, Right, Bottom; }
public class Win3 {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool rp);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int size);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr wp, IntPtr lp, uint fl, uint ms, out IntPtr res);
    // Close every leftover window whose title starts with this, so the desktop
    // is clean before anything is photographed.
    public static int CloseByTitlePrefix(string prefix) {
        int n = 0;
        EnumWindows(delegate(IntPtr h, IntPtr l) {
            StringBuilder sb = new StringBuilder(512);
            GetWindowText(h, sb, sb.Capacity);
            if (sb.ToString().StartsWith(prefix, StringComparison.OrdinalIgnoreCase)) {
                IntPtr res;
                SendMessageTimeout(h, 0x0010, IntPtr.Zero, IntPtr.Zero, 2, 2000, out res); // WM_CLOSE
                n++;
            }
            return true;
        }, IntPtr.Zero);
        return n;
    }
    public static RECT VisibleRect(IntPtr h) {
        RECT r;
        if (DwmGetWindowAttribute(h, 9, out r, Marshal.SizeOf(typeof(RECT))) == 0) return r;
        GetWindowRect(h, out r); return r;
    }
    public static IntPtr FindByTitle(string needle) {
        IntPtr found = IntPtr.Zero;
        EnumWindows(delegate(IntPtr h, IntPtr l) {
            if (!IsWindowVisible(h)) return true;
            StringBuilder sb = new StringBuilder(512);
            GetWindowText(h, sb, sb.Capacity);
            if (sb.ToString().IndexOf(needle, StringComparison.OrdinalIgnoreCase) >= 0) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
"@

function Shoot {
    param([string]$Name, [string]$Title, [string]$WorkDir, [string]$Command, [string]$ExeArgs = '', [int]$Cols, [int]$Rows, [int]$Settle = 5)

    # THE EXE IS NAMED BY ITS FULL PATH, and it has to be.
    #
    # This ran `cd /d <version folder> & hub.exe doctor` and got the INSTALLED
    # 0.9.8 every time, because C:\...\Programs\PortableNetworkHub is on PATH
    # and PATH won over the current directory. The window title said 0.9.6, the
    # prompt said the 0.9.6 folder, and the output was 0.9.8's. The only reason
    # it was caught is that 0.9.6 does not contain the advice text at all, and
    # the picture had it.
    #
    # Two wrong diagnoses were made before this one: a title collision, which
    # was real and fixed, and a z-order problem, which was not real but produced
    # a genuinely better capture method anyway. Neither was the cause.
    $inner = "cd /d `"$WorkDir`" & title $Title & mode con: cols=$Cols lines=$Rows & cls & `"$WorkDir\$Command`" $ExeArgs"
    $p = Start-Process -FilePath conhost.exe -ArgumentList "cmd.exe /k `"$inner`"" -PassThru -WindowStyle Normal

    $h = [IntPtr]::Zero
    $deadline = (Get-Date).AddSeconds(25)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 400
        $h = [Win3]::FindByTitle($Title)
        if ($h -ne [IntPtr]::Zero) { break }
    }
    if ($h -eq [IntPtr]::Zero) { Write-Host "  [$Name] no window"; try { $p | Stop-Process -Force } catch {}; return }

    [Win3]::ShowWindow($h, 9) | Out-Null
    [Win3]::SetForegroundWindow($h) | Out-Null
    $r0 = New-Object RECT
    [Win3]::GetWindowRect($h, [ref]$r0) | Out-Null
    [Win3]::MoveWindow($h, 12, 12, ($r0.Right - $r0.Left), ($r0.Bottom - $r0.Top), $true) | Out-Null
    Start-Sleep -Seconds $Settle

    # PrintWindow, NOT CopyFromScreen.
    #
    # CopyFromScreen reads the pixels that are on the screen inside a rectangle.
    # It has no idea which window they belong to. SetForegroundWindow is refused
    # to a process that is not itself in the foreground, which this one is not,
    # so the window being photographed sat BEHIND a leftover console at the same
    # coordinates and the capture returned the leftover. Right handle, right
    # rectangle, wrong pixels, and the picture filed as 0.9.6 showed advice that
    # 0.9.6 does not contain.
    #
    # PrintWindow asks the window to draw ITSELF into a bitmap, so z-order stops
    # mattering. Flag 2 is PW_RENDERFULLCONTENT, which is what makes it work for
    # windows drawn the modern way rather than returning a blank rectangle.
    $r = [Win3]::VisibleRect($h)
    $w = $r.Right - $r.Left; $ht = $r.Bottom - $r.Top
    $bmp = New-Object System.Drawing.Bitmap $w, $ht
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    $okp = [Win3]::PrintWindow($h, $hdc, 2)
    $g.ReleaseHdc($hdc)
    $g.Dispose()
    if (-not $okp) { Write-Host "  [$Name] PrintWindow refused" }
    $path = Join-Path $OutDir "$Name.png"
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()

    try { $p | Stop-Process -Force -ErrorAction SilentlyContinue } catch {}
    Get-Process hub -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Write-Host ("  [{0}] {1} x {2}, {3:N0} bytes" -f $Name, $w, $ht, (Get-Item $path).Length)
}

$vers = Join-Path $root 'vers'

Write-Host "== one screenshot per released version =="
$closed = [Win3]::CloseByTitlePrefix('hub doctor')
if ($closed -gt 0) { Write-Host "  closed $closed leftover console window(s) first" }
Start-Sleep -Seconds 2

# 0.9.6 prints no advice at all, so its output is short.
# THE TITLE CARRIES THE VERSION, and it has to.
#
# The first run of this gave all three windows the title "hub doctor", and
# FindByTitle returned the FIRST match: a console left over from an earlier
# capture. The picture filed as 0.9.6 was really 0.9.8, and the giveaway was
# that it showed advice text which 0.9.6 does not contain at all - checked with
# grep against the binary, 0 occurrences.
#
# It would have been a wrong screenshot on a release page, which is exactly the
# thing taking one picture per version was meant to avoid. A unique title
# cannot collide, and it is more use on the page as well.
Shoot -Name 'windows-doctor-0.9.6' -Title 'hub doctor 0.9.6' `
      -WorkDir (Join-Path $vers '0.9.6') -Command 'hub.exe' -ExeArgs 'doctor' -Cols 84 -Rows 24

# 0.9.7 names three services.
Shoot -Name 'windows-doctor-0.9.7' -Title 'hub doctor 0.9.7' `
      -WorkDir (Join-Path $vers '0.9.7') -Command 'hub.exe' -ExeArgs 'doctor' -Cols 84 -Rows 54

# 0.9.8 names two.
Shoot -Name 'windows-doctor-0.9.8' -Title 'hub doctor 0.9.8' `
      -WorkDir (Join-Path $vers '0.9.8') -Command 'hub.exe' -ExeArgs 'doctor' -Cols 84 -Rows 52

Write-Host "== done =="
Get-ChildItem $OutDir -Filter 'windows-doctor-0.9.*.png' | Select-Object Name,Length | Format-Table -AutoSize | Out-String -Width 60
