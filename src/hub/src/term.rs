// Version: 0.1.0 · updated 26-08-24-22-30
//
// The terminal layer: raw mode, size, keys, and a frame that is EXACTLY the
// height of the screen.
//
// WHY THERE IS NO TUI LIBRARY HERE. This whole program has no dependencies, and
// the reason is the audience: a download is minutes of somebody's life on a
// single-digit-KB/s line. The three subcommands merged came to 514 KB. A
// mainstream Rust TUI stack (a terminal backend plus a widget layer) is several
// megabytes of source and adds hundreds of KB to the binary to draw text that
// ANSI has drawn since 1979. Everything below is about 300 lines.
//
// WHY NO BORDERS, BOXES OR RULES ANYWHERE. A terminal has a fixed row and
// column budget and every border subtracts from it. Layout engines do not error
// when content exceeds its container, they WRAP, so the failure shows up as
// height in a different place from the cause. This draws a frame that is
// exactly `rows` tall and exactly `cols` wide and truncates by DISPLAY COLUMNS,
// because a CJK character is one char and two columns.

use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::OnceLock;
use std::time::Duration;

// ---------------------------------------------------------------- raw mode

/// Puts the terminal in raw mode and takes it out again on the way past.
///
/// `panic = "abort"` is set in the release profile, which means Drop does NOT
/// run on a panic. So the restore is ALSO installed as a panic hook. Getting
/// this wrong leaves a teacher with a terminal that echoes nothing and needs
/// `reset` typed blind, which is exactly the kind of thing that makes somebody
/// stop using a tool for good.
pub struct Raw {
    #[cfg(unix)]
    saved: Option<String>,
    #[cfg(windows)]
    saved: Option<(u32, u32)>,
}

impl Raw {
    /// None when this is not a terminal at all.
    ///
    /// `hub > out.txt` or `hub | less` has no terminal to put in raw mode, and
    /// drawing a screen into a pipe produces a file full of escape sequences
    /// that looks like the program is broken. Refusing is the honest answer;
    /// the caller prints the usage text instead.
    pub fn new() -> Option<Raw> {
        let saved = enter_raw()?;
        // Restore before the process dies, however it dies.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            leave_screen();
            restore_from_saved();
            prev(info);
        }));
        enter_screen();
        Some(Raw { saved: Some(saved) })
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        leave_screen();
        #[cfg(unix)]
        if let Some(s) = &self.saved {
            let _ = std::process::Command::new("stty").arg(s).stdin(std::process::Stdio::inherit()).status();
            return;
        }
        #[cfg(windows)]
        if let Some((i, o)) = self.saved {
            unsafe {
                SetConsoleMode(GetStdHandle(STD_INPUT), i);
                SetConsoleMode(GetStdHandle(STD_OUTPUT), o);
            }
            return;
        }
        restore_from_saved();
    }
}

/// The panic path cannot reach the guard's fields, so the settings are also
/// stashed here at entry.
static SAVED_UNIX: OnceLock<String> = OnceLock::new();
#[cfg(windows)]
static SAVED_WIN: OnceLock<(u32, u32)> = OnceLock::new();

fn restore_from_saved() {
    #[cfg(unix)]
    {
        let arg = SAVED_UNIX.get().cloned().unwrap_or_else(|| "sane".to_string());
        let _ = std::process::Command::new("stty").arg(arg).stdin(std::process::Stdio::inherit()).status();
    }
    #[cfg(windows)]
    if let Some(&(i, o)) = SAVED_WIN.get() {
        unsafe {
            SetConsoleMode(GetStdHandle(STD_INPUT), i);
            SetConsoleMode(GetStdHandle(STD_OUTPUT), o);
        }
    }
}

#[cfg(unix)]
fn enter_raw() -> Option<String> {
    // `stty -g` then `stty <that>` rather than `stty sane`, because sane resets
    // settings the user may have chosen deliberately. Shelling out to stty
    // instead of calling tcsetattr avoids hand-writing the termios struct
    // layout, which differs per architecture and is silently wrong when it is
    // wrong.
    let out = std::process::Command::new("stty").arg("-g").stdin(std::process::Stdio::inherit()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let saved = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let _ = SAVED_UNIX.set(saved.clone());
    let ok = std::process::Command::new("stty")
        .args(["raw", "-echo"])
        .stdin(std::process::Stdio::inherit())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok { Some(saved) } else { None }
}

#[cfg(windows)]
fn enter_raw() -> Option<(u32, u32)> {
    unsafe {
        let hin = GetStdHandle(STD_INPUT);
        let hout = GetStdHandle(STD_OUTPUT);
        let (mut i, mut o) = (0u32, 0u32);
        if GetConsoleMode(hin, &mut i) == 0 || GetConsoleMode(hout, &mut o) == 0 {
            return None;
        }
        let _ = SAVED_WIN.set((i, o));
        // Line input and echo off, so keys arrive one at a time.
        let raw_in = i & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT);
        // Without ENABLE_VIRTUAL_TERMINAL_PROCESSING, Windows prints the escape
        // sequences as literal text instead of acting on them, and the screen
        // fills with garbage. Present since Windows 10 1511.
        let raw_out = o | ENABLE_VIRTUAL_TERMINAL_PROCESSING;
        SetConsoleMode(hin, raw_in);
        SetConsoleMode(hout, raw_out);
        Some((i, o))
    }
}

#[cfg(not(any(unix, windows)))]
fn enter_raw() -> Option<String> { None }

fn enter_screen() {
    // 1049h = alternate screen: the teacher's scrollback survives untouched.
    // 25l = hide the cursor.
    print!("\x1b[?1049h\x1b[?25l");
    let _ = std::io::stdout().flush();
}

fn leave_screen() {
    print!("\x1b[?25h\x1b[?1049l");
    let _ = std::io::stdout().flush();
}

// ---------------------------------------------------------------- size

#[cfg(unix)]
#[repr(C)]
struct Winsize {
    row: u16,
    col: u16,
    xpixel: u16,
    ypixel: u16,
}

#[cfg(unix)]
extern "C" {
    fn ioctl(fd: i32, request: u64, ...) -> i32;
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct Coord {
    x: i16,
    y: i16,
}
#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct SmallRect {
    left: i16,
    top: i16,
    right: i16,
    bottom: i16,
}
#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct ScreenBufferInfo {
    size: Coord,
    cursor: Coord,
    attributes: u16,
    window: SmallRect,
    max_window: Coord,
}

#[cfg(windows)]
const STD_INPUT: u32 = -10i32 as u32;
#[cfg(windows)]
const STD_OUTPUT: u32 = -11i32 as u32;
#[cfg(windows)]
const ENABLE_PROCESSED_INPUT: u32 = 0x0001;
#[cfg(windows)]
const ENABLE_LINE_INPUT: u32 = 0x0002;
#[cfg(windows)]
const ENABLE_ECHO_INPUT: u32 = 0x0004;
#[cfg(windows)]
const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetStdHandle(which: u32) -> isize;
    fn GetConsoleMode(h: isize, mode: *mut u32) -> i32;
    fn SetConsoleMode(h: isize, mode: u32) -> i32;
    fn GetConsoleScreenBufferInfo(h: isize, info: *mut ScreenBufferInfo) -> i32;
}

/// (rows, cols). Queried every frame, so a resized window is picked up without
/// signal handling: this is two syscalls, not a process spawn.
pub fn size() -> (usize, usize) {
    #[cfg(unix)]
    unsafe {
        let mut ws = Winsize { row: 0, col: 0, xpixel: 0, ypixel: 0 };
        const TIOCGWINSZ: u64 = 0x5413;
        if ioctl(1, TIOCGWINSZ, &mut ws as *mut Winsize) == 0 && ws.row > 0 && ws.col > 0 {
            return (ws.row as usize, ws.col as usize);
        }
    }
    #[cfg(windows)]
    unsafe {
        let mut info = ScreenBufferInfo::default();
        if GetConsoleScreenBufferInfo(GetStdHandle(STD_OUTPUT), &mut info) != 0 {
            let rows = (info.window.bottom - info.window.top + 1).max(1) as usize;
            let cols = (info.window.right - info.window.left + 1).max(1) as usize;
            return (rows, cols);
        }
    }
    (24, 80)
}

// ---------------------------------------------------------------- keys

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Esc,
    Backspace,
    Tab,
    Char(char),
    Quit,
    None,
}

/// Blocking reads live on their own thread and arrive as bytes on a channel, so
/// the draw loop can wait with a timeout and keep redrawing live numbers.
/// A thread rather than poll()/WaitForSingleObject because it is the same code
/// on both platforms and has no FFI to get wrong.
pub struct Keys {
    rx: Receiver<u8>,
}

impl Keys {
    pub fn new() -> Keys {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut stdin = std::io::stdin();
            let mut b = [0u8; 1];
            while let Ok(1) = stdin.read(&mut b) {
                if tx.send(b[0]).is_err() {
                    return;
                }
            }
        });
        Keys { rx }
    }

    /// Waits up to `timeout` for a key. Returns Key::None on a timeout, which is
    /// the normal case: it is what lets the screen refresh while nobody types.
    pub fn next(&self, timeout: Duration) -> Key {
        let b = match self.rx.recv_timeout(timeout) {
            Ok(b) => b,
            Err(RecvTimeoutError::Timeout) => return Key::None,
            Err(RecvTimeoutError::Disconnected) => return Key::Quit,
        };
        match b {
            0x03 => Key::Quit,      // ctrl-c: raw mode means we must handle it
            b'\r' | b'\n' => Key::Enter,
            0x7f | 0x08 => Key::Backspace,
            b'\t' => Key::Tab,
            0x1b => self.escape(),
            // A bare escape byte with nothing behind it is the Esc key. Anything
            // else printable is a character; UTF-8 continuation bytes are
            // gathered so an accented or Arabic name can be typed.
            c if c >= 0x20 => self.utf8(c),
            _ => Key::None,
        }
    }

    fn escape(&self) -> Key {
        // 40ms: long enough that a terminal's arrow-key bytes arrive together,
        // short enough that pressing Esc alone does not feel stuck.
        let gap = Duration::from_millis(40);
        match self.rx.recv_timeout(gap) {
            Ok(b'[') | Ok(b'O') => match self.rx.recv_timeout(gap) {
                Ok(b'A') => Key::Up,
                Ok(b'B') => Key::Down,
                Ok(b'C') => Key::Right,
                Ok(b'D') => Key::Left,
                // Consume the tail of longer sequences (Home, F-keys, mouse)
                // rather than letting the digits fall through as typed text.
                Ok(c) if c.is_ascii_digit() => {
                    while let Ok(t) = self.rx.recv_timeout(gap) {
                        if t.is_ascii_alphabetic() || t == b'~' {
                            break;
                        }
                    }
                    Key::None
                }
                _ => Key::None,
            },
            _ => Key::Esc,
        }
    }

    fn utf8(&self, first: u8) -> Key {
        let extra = match first {
            0x00..=0x7f => 0,
            0xc0..=0xdf => 1,
            0xe0..=0xef => 2,
            0xf0..=0xf7 => 3,
            _ => return Key::None,
        };
        let mut buf = vec![first];
        for _ in 0..extra {
            match self.rx.recv_timeout(Duration::from_millis(40)) {
                Ok(b) => buf.push(b),
                Err(_) => return Key::None,
            }
        }
        match std::str::from_utf8(&buf) {
            Ok(s) => s.chars().next().map(Key::Char).unwrap_or(Key::None),
            Err(_) => Key::None,
        }
    }
}

// ---------------------------------------------------------------- width

/// Display columns, not characters and not bytes.
///
/// A CJK ideograph is one char and occupies TWO columns; slicing a string by
/// bytes can also cut a character in half and produce a broken rune. Both
/// mistakes show up as a layout that is correct on the machine it was written
/// on and wrong on somebody else's. The ranges below are the wide and
/// fullwidth blocks of East Asian Width; ambiguous characters are treated as
/// one, which is why nothing here draws with box characters in the first place.
pub fn width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

fn char_width(c: char) -> usize {
    let c = c as u32;
    match c {
        0x1100..=0x115F        // Hangul Jamo
        | 0x2E80..=0x303E      // CJK radicals, Kangxi, CJK symbols
        | 0x3041..=0x33FF      // kana, Hangul compat, CJK compat
        | 0x3400..=0x4DBF      // CJK ext A
        | 0x4E00..=0x9FFF      // CJK unified
        | 0xA000..=0xA4CF      // Yi
        | 0xAC00..=0xD7A3      // Hangul syllables
        | 0xF900..=0xFAFF      // CJK compat ideographs
        | 0xFE30..=0xFE6F      // CJK compat forms
        | 0xFF00..=0xFF60      // fullwidth forms
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F    // emoji
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

/// Cut to at most `cols` display columns, never mid-character.
pub fn truncate(s: &str, cols: usize) -> String {
    if width(s) <= cols {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let w = char_width(ch);
        if used + w > cols.saturating_sub(1) {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('~');
    out
}

// ---------------------------------------------------------------- frame

/// A screen of exactly `rows` lines, each exactly `cols` columns.
///
/// The height is a BUDGET decided before anything is drawn, not the sum of
/// whatever the content turned out to be. `push` past the last row is dropped
/// rather than allowed to scroll the screen, because a frame one row too tall
/// makes the terminal scroll and every subsequent redraw tears.
pub struct Frame {
    pub rows: usize,
    pub cols: usize,
    lines: Vec<String>,
}

impl Frame {
    pub fn new(rows: usize, cols: usize) -> Frame {
        Frame { rows, cols, lines: Vec::with_capacity(rows) }
    }

    pub fn push(&mut self, s: &str) {
        if self.lines.len() < self.rows {
            self.lines.push(truncate(s, self.cols));
        }
    }

    pub fn blank(&mut self) {
        self.push("");
    }

    /// Highlighted line: reverse video costs zero columns, unlike a box.
    pub fn push_selected(&mut self, s: &str) {
        if self.lines.len() >= self.rows {
            return;
        }
        let t = truncate(s, self.cols);
        let pad = self.cols.saturating_sub(width(&t));
        self.lines.push(format!("\x1b[7m{t}{}\x1b[0m", " ".repeat(pad)));
    }

    pub fn push_dim(&mut self, s: &str) {
        if self.lines.len() >= self.rows {
            return;
        }
        self.lines.push(format!("\x1b[2m{}\x1b[0m", truncate(s, self.cols)));
    }

    pub fn used(&self) -> usize {
        self.lines.len()
    }

    /// Pad so the last line lands on the bottom row.
    pub fn fill_to(&mut self, row_from_bottom: usize) {
        let target = self.rows.saturating_sub(row_from_bottom);
        while self.lines.len() < target {
            self.lines.push(String::new());
        }
    }

    /// One write for the whole screen. Drawing line by line with a flush each
    /// time is what makes a terminal UI flicker.
    pub fn draw(&self) {
        let mut buf = String::with_capacity(self.rows * (self.cols + 8));
        buf.push_str("\x1b[H"); // home, without clearing: clearing first is the flicker
        for (i, line) in self.lines.iter().enumerate() {
            buf.push_str(line);
            buf.push_str("\x1b[K"); // wipe to end of line
            if i + 1 < self.rows {
                buf.push_str("\r\n");
            }
        }
        for i in self.lines.len()..self.rows {
            buf.push_str("\x1b[K");
            if i + 1 < self.rows {
                buf.push_str("\r\n");
            }
        }
        let mut out = std::io::stdout();
        let _ = out.write_all(buf.as_bytes());
        let _ = out.flush();
    }
}
