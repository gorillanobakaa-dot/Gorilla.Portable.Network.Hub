// console-read: copy the text of ONE console window into a file.
//
// WHY THIS EXISTS. `hub serve` prints one line per finished download
// ("<phone> <bytes> bytes in <s>s = <MB/s>"), which is the ground truth for
// how many bytes the phone actually asked for. The bench reads it from the
// hub's own window instead of redirecting the hub's output, so the hub runs
// exactly as a person would run it. Read-only: it reads the screen buffer
// and changes nothing in that console.
//
// Usage: console-read <pid attached to the console> <output file>
// Build: Add-Type -TypeDefinition (Get-Content console-read.cs -Raw)
//          -OutputType ConsoleApplication -OutputAssembly console-read.exe

using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

public static class ConsoleRead {
    [DllImport("kernel32.dll")] static extern bool FreeConsole();
    [DllImport("kernel32.dll")] static extern bool AttachConsole(uint pid);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr sec, uint disp, uint flags, IntPtr tmpl);
    [DllImport("kernel32.dll")] static extern bool GetConsoleScreenBufferInfo(IntPtr h, out CSBI info);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool ReadConsoleOutputCharacterW(IntPtr h, StringBuilder buf, uint len, COORD at, out uint read);

    [StructLayout(LayoutKind.Sequential)] public struct COORD { public short X, Y; }
    [StructLayout(LayoutKind.Sequential)] public struct RECT16 { public short L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct CSBI { public COORD Size, Cursor; public ushort Attr; public RECT16 Window; public COORD Max; }

    public static int Main(string[] args) {
        if (args.Length < 2) { return 2; }
        FreeConsole();
        if (!AttachConsole(uint.Parse(args[0]))) { File.WriteAllText(args[1], "could not attach\n"); return 1; }
        IntPtr h = CreateFileW("CONOUT$", 0xC0000000, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
        CSBI info;
        if (!GetConsoleScreenBufferInfo(h, out info)) { File.WriteAllText(args[1], "no buffer\n"); return 1; }
        var all = new StringBuilder();
        for (short y = 0; y <= info.Cursor.Y; y++) {
            var line = new StringBuilder(info.Size.X);
            uint read;
            ReadConsoleOutputCharacterW(h, line, (uint)info.Size.X, new COORD { X = 0, Y = y }, out read);
            all.AppendLine(line.ToString(0, (int)read).TrimEnd());
        }
        File.WriteAllText(args[1], all.ToString(), Encoding.UTF8);
        return 0;
    }
}
