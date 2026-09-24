// console-keys: type into ONE console window, and nowhere else.
//
// WHY THIS EXISTS. The first screenshot tour sent keys with SendKeys after
// SetForegroundWindow. Windows does not let a background process take the
// foreground, so every key went to whatever window the person at the laptop was
// using: arrows, Enter, thirty backspaces and a password landed in their chat,
// 2026-09-24. Nothing reached the program being photographed.
//
// This attaches to the target's console and writes key events straight into
// that console's input buffer (WriteConsoleInputW). No window is focused, no
// window is needed, and it cannot reach any other program: the only input
// buffer it can write to is the one it attached to.
//
// Usage: console-keys <pid attached to the console> <text>
//   In <text>: \e Escape, \r Enter, \b Backspace, \u \d \l \x the arrows
//   (up, down, left, right), \\ a backslash. Everything else is typed as is.
//
// Built by screenshot-tour.ps1 with Add-Type -OutputType ConsoleApplication.

using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public static class ConsoleKeys {
    [DllImport("kernel32.dll")] static extern bool FreeConsole();
    [DllImport("kernel32.dll")] static extern bool AttachConsole(uint pid);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr sec, uint disp, uint flags, IntPtr tmpl);
    [DllImport("kernel32.dll")] static extern bool WriteConsoleInputW(IntPtr h, INPUT_RECORD[] recs, uint n, out uint written);

    [StructLayout(LayoutKind.Explicit)]
    struct INPUT_RECORD {
        [FieldOffset(0)] public ushort EventType;
        [FieldOffset(4)] public int bKeyDown;
        [FieldOffset(8)] public ushort wRepeatCount;
        [FieldOffset(10)] public ushort wVirtualKeyCode;
        [FieldOffset(12)] public ushort wVirtualScanCode;
        [FieldOffset(14)] public char UnicodeChar;
        [FieldOffset(16)] public uint dwControlKeyState;
    }

    static INPUT_RECORD Key(char c, bool down) {
        var r = new INPUT_RECORD();
        r.EventType = 1; r.bKeyDown = down ? 1 : 0; r.wRepeatCount = 1; r.UnicodeChar = c;
        return r;
    }

    public static int Main(string[] args) {
        if (args.Length < 2) { Console.Error.WriteLine("usage: console-keys <pid> <text>"); return 2; }
        uint pid = uint.Parse(args[0]);
        string t = args[1];
        var chars = new List<char>();
        for (int i = 0; i < t.Length; i++) {
            if (t[i] == '\\' && i + 1 < t.Length) {
                char n = t[++i];
                switch (n) {
                    case 'e': chars.Add('\x1b'); break;
                    case 'r': chars.Add('\r'); break;
                    case 'b': chars.Add('\x7f'); break;
                    case 'u': chars.AddRange("\x1b[A"); break;
                    case 'd': chars.AddRange("\x1b[B"); break;
                    case 'x': chars.AddRange("\x1b[C"); break;
                    case 'l': chars.AddRange("\x1b[D"); break;
                    default: chars.Add(n); break;
                }
            } else chars.Add(t[i]);
        }
        FreeConsole();
        if (!AttachConsole(pid)) { return 3; }
        // CONIN$: this console's input buffer, and only this console's.
        IntPtr h = CreateFileW("CONIN$", 0xC0000000, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
        if (h == new IntPtr(-1)) { return 4; }
        var recs = new List<INPUT_RECORD>();
        foreach (char c in chars) { recs.Add(Key(c, true)); recs.Add(Key(c, false)); }
        uint written;
        return WriteConsoleInputW(h, recs.ToArray(), (uint)recs.Count, out written) ? 0 : 5;
    }
}
