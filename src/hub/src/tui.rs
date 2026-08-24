// Version: 0.1.0 · updated 26-08-24-23-15
//
// The screen a teacher actually uses.
//
// Everything under this file already worked from a command line. That is not
// the same as being usable: it meant knowing what a subcommand is, what an IP
// address is, that the folder has to be handed out before anyone can ask for
// it, and which of six flags to type. This is the same machinery with the
// knowledge requirement removed.
//
// Design rules it follows, and why:
//
//   NO BORDERS, BOXES OR RULES. A terminal has a fixed row and column budget
//   and decoration spends it. Selection is shown with reverse video, which
//   costs nothing, and grouping is shown with blank lines, which cost one row
//   each and cannot wrap.
//
//   ONE THING TO READ PER LINE. The audience includes people reading their
//   third language, and the tool is used standing up in front of a class.
//
//   NO JARGON ON SCREEN. There is no "bind", no "SSID", no "socket", no
//   "checksum". The words are the ones a teacher would use.
//
//   EVERY ERROR SAYS WHAT TO DO NEXT. A message that only says what failed
//   leaves somebody stuck in a room with no internet and nobody to ask.

use crate::net;
use crate::serve;
use crate::term::{self, Frame, Key, Keys};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PORT: u16 = 8080;
/// Four redraws a second. Fast enough that a rate reading looks live, slow
/// enough to be invisible on a 2012 processor: a full frame is one write of a
/// few kilobytes.
const TICK: Duration = Duration::from_millis(250);

pub fn run() {
    let Some(_raw) = term::Raw::new() else {
        println!("This needs a terminal to draw on.");
        println!();
        println!("Run it without sending the output anywhere else:");
        println!("  hub");
        println!();
        println!("Or use it a command at a time:");
        println!("  hub serve <folder>     hand files out");
        println!("  hub get <address>      fetch one");
        println!("  hub doctor             what this computer can do");
        return;
    };
    let mut app = App::new();
    let keys = Keys::new();
    loop {
        let (rows, cols) = term::size();
        app.draw(rows, cols);
        match keys.next(TICK) {
            Key::None => {}
            k => {
                if app.key(k) {
                    break;
                }
            }
        }
    }
    // Drop order does the rest: the guard restores the terminal, and stopping
    // the hotspot happens here while the screen still exists to say so.
    app.shutdown();
}

// ---------------------------------------------------------------- state

enum Screen {
    Home,
    Send,
    Sending,
    Receive,
    ReceiveFiles,
    Receiving,
    Note(String),
}

/// What a probe thread has found so far. `None` means still looking, which is
/// a different thing from "found nothing" and has to look different on screen.
type Found = Arc<Mutex<Option<Vec<(std::net::Ipv4Addr, usize)>>>>;

struct App {
    screen: Screen,
    back: Screen,
    row: usize,
    editing: Option<String>,

    // the send form
    folder: String,
    ssid: String,
    password: String,
    helpers: usize,
    hotspot: Option<net::Hotspot>,
    started: Option<Instant>,
    addresses: Vec<std::net::Ipv4Addr>,

    // the receive form
    found: Found,
    server: Option<std::net::Ipv4Addr>,
    /// Not a constant. Someone who types "10.42.0.1:9000" means port 9000, and
    /// throwing it away produced "could not read the list of files" for an
    /// address that was perfectly correct.
    server_port: u16,
    typed_address: String,
    files: Vec<net::Entry>,
    save_into: String,
    at_once: usize,
    downloading: Option<String>,
    result: Arc<Mutex<Option<Result<f64, String>>>>,
    since: Option<Instant>,
}

impl App {
    fn new() -> App {
        let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        App {
            screen: Screen::Home,
            back: Screen::Home,
            row: 0,
            editing: None,
            folder: here.to_string_lossy().into_owned(),
            ssid: String::new(),
            // Offered, not imposed. A suggested password is the difference
            // between a teacher setting one and a teacher leaving the network
            // open because inventing a password is one more thing to do.
            password: net::suggest_password(),
            helpers: serve::default_helpers(),
            hotspot: None,
            started: None,
            addresses: Vec::new(),
            found: Arc::new(Mutex::new(None)),
            server: None,
            server_port: PORT,
            typed_address: String::new(),
            files: Vec::new(),
            save_into: here.to_string_lossy().into_owned(),
            at_once: 4,
            downloading: None,
            result: Arc::new(Mutex::new(None)),
            since: None,
        }
    }

    fn shutdown(&mut self) {
        if let Some(h) = self.hotspot.take() {
            h.down();
            h.disarm_restore();
            println!("The wifi has been put back the way it was.");
        }
        crate::fetch::cancel();
    }
}

// ---------------------------------------------------------------- drawing

impl App {
    fn draw(&mut self, rows: usize, cols: usize) {
        if rows < 10 || cols < 44 {
            let mut f = Frame::new(rows, cols);
            f.push("The window is too small.");
            f.push("Make it bigger and this will come back.");
            f.draw();
            return;
        }
        let mut f = Frame::new(rows, cols);
        match self.screen {
            Screen::Home => self.draw_home(&mut f),
            Screen::Send => self.draw_send(&mut f),
            Screen::Sending => self.draw_sending(&mut f),
            Screen::Receive => self.draw_receive(&mut f),
            Screen::ReceiveFiles => self.draw_files(&mut f),
            Screen::Receiving => self.draw_receiving(&mut f),
            Screen::Note(_) => self.draw_note(&mut f),
        }
        f.draw();
    }

    fn title(&self, f: &mut Frame, s: &str) {
        f.push(s);
        f.blank();
    }

    /// The hint line is always the last row, so a teacher looking for what to
    /// press always looks in the same place.
    fn hints(&self, f: &mut Frame, s: &str) {
        f.fill_to(1);
        f.push_dim(s);
    }

    fn draw_home(&self, f: &mut Frame) {
        self.title(f, "Gorilla Portable Network Hub");
        let items = [
            "Hand out files to the class",
            "Get files from another computer",
        ];
        for (i, it) in items.iter().enumerate() {
            if i == self.row {
                f.push_selected(&format!("  {it}"));
            } else {
                f.push(&format!("  {it}"));
            }
        }
        f.blank();
        f.push_dim("  This works with no internet and no router.");
        self.hints(f, "  up and down to choose    enter to open    q to quit");
    }

    fn send_fields(&self) -> Vec<(String, String)> {
        vec![
            ("Folder to hand out".into(), self.folder.clone()),
            (
                "Wifi network to make".into(),
                if self.ssid.is_empty() {
                    "none, use the network that is already here".into()
                } else {
                    self.ssid.clone()
                },
            ),
            (
                "Password for it".into(),
                if self.ssid.is_empty() {
                    "not needed".into()
                } else if self.password.is_empty() {
                    "none set, so the network cannot be made".into()
                } else {
                    self.password.clone()
                },
            ),
            ("Devices at once".into(), self.helpers.to_string()),
        ]
    }

    fn draw_send(&self, f: &mut Frame) {
        self.title(f, "Hand out files");
        let fields = self.send_fields();
        let label_width = 22;
        for (i, (label, value)) in fields.iter().enumerate() {
            let shown = if self.row == i {
                if let Some(buf) = &self.editing {
                    // A reverse-video space is the cursor. The real cursor is
                    // hidden because it flickers across a full redraw.
                    format!("{buf}\x1b[7m \x1b[0m")
                } else {
                    value.clone()
                }
            } else {
                value.clone()
            };
            let line = format!("  {label:<label_width$}{shown}");
            if self.row == i && self.editing.is_none() {
                f.push_selected(&line);
            } else {
                f.push(&line);
            }
        }
        f.blank();
        let start = "  Start handing out";
        if self.row == fields.len() {
            f.push_selected(start);
        } else {
            f.push(start);
        }
        f.blank();
        if self.ssid.is_empty() {
            f.push_dim("  Leave the network name empty if the class is already");
            f.push_dim("  on the same wifi as this computer.");
        } else {
            f.push_dim("  Making a network replaces this computer's own wifi");
            f.push_dim("  until you stop. It is put back when you do.");
        }
        if self.editing.is_some() {
            self.hints(f, "  type to change    enter to keep it    esc to leave it alone");
        } else {
            self.hints(f, "  up and down to move    enter to change or start    esc to go back");
        }
    }

    fn draw_sending(&mut self, f: &mut Frame) {
        // A hotspot does not have an address the instant nmcli returns; the
        // interface has to come up and be given one. Asking again while the
        // list is empty costs four UDP sockets and stops the screen from
        // saying nothing at the one moment the teacher needs the address.
        if self.addresses.is_empty() {
            self.addresses = net::local_addresses();
        }
        self.title(f, "Handing out files");
        if let Some(h) = &self.hotspot {
            f.push(&format!("  Wifi network      {}", h.ssid));
            f.push(&format!("  Password          {}", self.password));
        }
        for a in &self.addresses {
            f.push(&format!("  Address to type   http://{a}:{PORT}"));
        }
        f.blank();
        let live = serve::transfers();
        let sent = serve::total_sent();
        // "0 devices getting files" next to a row saying 100% done is a screen
        // arguing with itself. Count both states and say whichever is true.
        let active = live.iter().filter(|t| !t.finished).count();
        let complete = live.iter().filter(|t| t.finished).count();
        let devices = |n: usize| if n == 1 { "device" } else { "devices" };
        f.push(&match (active, complete) {
            (0, 0) => format!("  Nothing sent yet."),
            (0, c) => format!("  {c} {} finished, {} sent so far", devices(c), human(sent)),
            (a, 0) => format!("  {a} {} getting files, {} sent so far", devices(a), human(sent)),
            (a, c) => format!("  {a} {} getting files, {c} finished, {} sent so far",
                              devices(a), human(sent)),
        });
        f.blank();
        if live.is_empty() {
            f.push_dim("  Nobody has connected yet.");
            f.push_dim("  On their computer: open the same tool and choose");
            f.push_dim("  \"Get files from another computer\".");
        }
        // The list is capped by what is left on screen, and what was dropped is
        // said out loud. A list that silently stops at ten reads as "ten
        // devices" to the person looking at it.
        let room = f.rows.saturating_sub(f.used() + 3);
        for t in live.iter().take(room) {
            let pct = if t.total > 0 { t.done as f64 / t.total as f64 } else { 0.0 };
            f.push(&format!(
                "  {:<16}{} {:>3}%  {:>12}  {}",
                t.peer,
                bar(pct, 16),
                (pct * 100.0) as u64,
                if t.finished { "done".to_string() } else { format!("{:.1} MB/s", t.rate / 1e6) },
                t.file
            ));
        }
        if live.len() > room {
            f.push_dim(&format!("  and {} more not shown, the window is too short", live.len() - room));
        }
        self.hints(f, "  q or esc to stop handing out");
    }

    fn draw_receive(&self, f: &mut Frame) {
        self.title(f, "Get files from another computer");
        let found = self.found.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut n = 0;
        match &found {
            None => {
                f.push("  Looking for a computer handing out files...");
                f.blank();
            }
            Some(list) if list.is_empty() => {
                f.push("  Nothing found on this network.");
                f.blank();
                f.push_dim("  Check you are connected to the teacher's wifi,");
                f.push_dim("  and that they have started handing files out.");
                f.blank();
            }
            Some(list) => {
                for (ip, count) in list {
                    let line = format!("  {ip}    {count} file{}", if *count == 1 { "" } else { "s" });
                    if self.row == n {
                        f.push_selected(&line);
                    } else {
                        f.push(&line);
                    }
                    n += 1;
                }
                f.blank();
            }
        }
        let manual = if self.editing.is_some() {
            format!("  Type the address    {}\x1b[7m \x1b[0m", self.editing.clone().unwrap_or_default())
        } else {
            format!("  Type the address    {}", if self.typed_address.is_empty() { "(press enter)".into() } else { self.typed_address.clone() })
        };
        if self.row == n && self.editing.is_none() {
            f.push_selected(&manual);
        } else {
            f.push(&manual);
        }
        if self.editing.is_some() {
            self.hints(f, "  type the address    enter to use it    esc to leave it alone");
        } else {
            self.hints(f, "  up and down to move    enter to choose    r to look again    esc to go back");
        }
    }

    fn draw_files(&self, f: &mut Frame) {
        let who = match (self.server, self.server_port) {
            (Some(ip), p) if p == PORT => ip.to_string(),
            (Some(ip), p) => format!("{ip} port {p}"),
            _ => String::new(),
        };
        self.title(f, &format!("Files on {who}"));
        let settings: [(&str, String); 2] = [
            ("Save into", self.save_into.clone()),
            ("Pieces at once", self.at_once.to_string()),
        ];
        for (i, (label, value)) in settings.iter().enumerate() {
            let shown = if self.row == i && self.editing.is_some() {
                format!("{}\x1b[7m \x1b[0m", self.editing.clone().unwrap_or_default())
            } else {
                value.clone()
            };
            let line = format!("  {label:<16}{shown}");
            if self.row == i && self.editing.is_none() {
                f.push_selected(&line);
            } else {
                f.push(&line);
            }
        }
        f.blank();
        if self.files.is_empty() {
            f.push("  That computer is not handing out any files.");
        }
        let room = f.rows.saturating_sub(f.used() + 3);
        for (i, e) in self.files.iter().enumerate().take(room) {
            let line = format!("  {:<34}{:>10}", e.name, human(e.size));
            if self.row == i + settings.len() {
                f.push_selected(&line);
            } else {
                f.push(&line);
            }
        }
        if self.files.len() > room {
            f.push_dim(&format!("  and {} more not shown, the window is too short", self.files.len() - room));
        }
        if self.editing.is_some() {
            self.hints(f, "  type to change    enter to keep it    esc to leave it alone");
        } else {
            self.hints(f, "  up and down to move    enter to get the file    esc to go back");
        }
    }

    fn draw_receiving(&self, f: &mut Frame) {
        self.title(f, "Getting a file");
        let name = self.downloading.clone().unwrap_or_default();
        let (done, total) = crate::fetch::progress();
        let pct = if total > 0 { done as f64 / total as f64 } else { 0.0 };
        let secs = self.since.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0).max(0.001);
        let rate = done as f64 / secs;
        f.push(&format!("  {name}"));
        f.blank();
        f.push(&format!("  {} {:>3}%", bar(pct, 34), (pct * 100.0) as u64));
        f.blank();
        f.push(&format!("  {} of {}", human(done), human(total)));
        f.push(&format!("  {:.1} MB/s", rate / 1e6));
        if rate > 1000.0 && total > done {
            let left = (total - done) as f64 / rate;
            f.push(&format!("  about {} left", duration(left)));
        }
        f.blank();
        let (retries, damaged) = crate::fetch::counts();
        if retries > 0 {
            f.push(&format!(
                "  {retries} piece{} had to be asked for again. That is normal",
                if retries == 1 { "" } else { "s" }
            ));
            f.push("  on a weak signal and nothing is lost by it.");
        }
        if damaged > 0 {
            f.push(&format!(
                "  {damaged} piece{} arrived damaged and {} replaced.",
                if damaged == 1 { "" } else { "s" },
                if damaged == 1 { "was" } else { "were" }
            ));
        }
        if retries > 0 || damaged > 0 {
            f.blank();
        }
        let msgs = crate::fetch::messages();
        let room = f.rows.saturating_sub(f.used() + 3);
        for (m, times) in msgs.iter().rev().take(room.min(4)).rev() {
            if *times > 1 {
                f.push_dim(&format!("  {m}   ({times} times)"));
            } else {
                f.push_dim(&format!("  {m}"));
            }
        }
        self.hints(f, "  q or esc to stop    it will carry on from here next time");
    }

    fn draw_note(&self, f: &mut Frame) {
        let Screen::Note(text) = &self.screen else { return };
        self.title(f, "");
        for line in text.lines() {
            // Wrap by words at the frame width rather than letting the terminal
            // wrap mid-word, which would also push the hint line off the
            // bottom and make the screen look broken.
            for chunk in wrap(line, f.cols.saturating_sub(4)) {
                f.push(&format!("  {chunk}"));
            }
        }
        self.hints(f, "  enter or esc to go back");
    }
}

// ---------------------------------------------------------------- keys

impl App {
    /// Returns true to quit the program.
    fn key(&mut self, k: Key) -> bool {
        if let Some(buf) = self.editing.clone() {
            return self.edit_key(k, buf);
        }
        match self.screen {
            Screen::Home => self.home_key(k),
            Screen::Send => self.send_key(k),
            Screen::Sending => self.sending_key(k),
            Screen::Receive => self.receive_key(k),
            Screen::ReceiveFiles => self.files_key(k),
            Screen::Receiving => self.receiving_key(k),
            Screen::Note(_) => {
                if matches!(k, Key::Enter | Key::Esc | Key::Char('q')) {
                    self.screen = std::mem::replace(&mut self.back, Screen::Home);
                    self.row = 0;
                }
                false
            }
        }
    }

    fn edit_key(&mut self, k: Key, mut buf: String) -> bool {
        match k {
            Key::Char(c) => {
                buf.push(c);
                self.editing = Some(buf);
            }
            Key::Backspace => {
                buf.pop();
                self.editing = Some(buf);
            }
            Key::Esc => self.editing = None,
            Key::Enter => {
                self.commit(buf);
                self.editing = None;
            }
            Key::Quit => return true,
            _ => {}
        }
        false
    }

    fn commit(&mut self, buf: String) {
        match self.screen {
            Screen::Send => match self.row {
                0 => self.folder = buf,
                1 => self.ssid = buf.trim().to_string(),
                2 => self.password = buf.trim().to_string(),
                // Clamped, not rejected. Typing a letter into a number field
                // should not throw the value away, and 100,000 helpers is a
                // typo rather than a wish.
                3 => self.helpers = buf.trim().parse().unwrap_or(self.helpers).clamp(1, 512),
                _ => {}
            },
            Screen::Receive => {
                self.typed_address = buf.trim().to_string();
                self.open_typed();
            }
            Screen::ReceiveFiles => match self.row {
                0 => self.save_into = buf,
                1 => self.at_once = buf.trim().parse().unwrap_or(self.at_once).clamp(1, 32),
                _ => {}
            },
            _ => {}
        }
    }

    fn move_row(&mut self, k: Key, count: usize) {
        match k {
            Key::Up => self.row = self.row.saturating_sub(1),
            Key::Down => {
                if self.row + 1 < count {
                    self.row += 1;
                }
            }
            _ => {}
        }
    }

    fn home_key(&mut self, k: Key) -> bool {
        self.move_row(k, 2);
        match k {
            Key::Enter => {
                if self.row == 0 {
                    self.screen = Screen::Send;
                } else {
                    self.start_looking();
                    self.screen = Screen::Receive;
                }
                self.row = 0;
            }
            Key::Char('q') | Key::Esc | Key::Quit => return true,
            _ => {}
        }
        false
    }

    fn send_key(&mut self, k: Key) -> bool {
        let n = self.send_fields().len() + 1;
        self.move_row(k, n);
        match k {
            Key::Enter => {
                if self.row == n - 1 {
                    self.begin_sending();
                } else {
                    let fields = self.send_fields();
                    // Editing starts from the real value, not from the
                    // explanatory text shown when a field is empty. Otherwise
                    // the first keystroke would append to a sentence.
                    self.editing = Some(match self.row {
                        0 => self.folder.clone(),
                        1 => self.ssid.clone(),
                        2 => self.password.clone(),
                        3 => self.helpers.to_string(),
                        _ => fields[self.row].1.clone(),
                    });
                }
            }
            Key::Esc => {
                self.screen = Screen::Home;
                self.row = 0;
            }
            Key::Char('q') | Key::Quit => return true,
            _ => {}
        }
        false
    }

    fn sending_key(&mut self, k: Key) -> bool {
        match k {
            Key::Char('q') | Key::Esc => {
                if let Some(h) = self.hotspot.take() {
                    h.down();
                    h.disarm_restore();
                }
                // The server keeps its threads: there is no way to unbind a
                // listener in std without tearing the process down, and going
                // back to the menu and starting again on the same port is the
                // one case that would fail. Said plainly rather than hidden.
                self.note("Stopped handing out.\n\nThe network has been put back the way it was.\n\nTo hand out a different folder, close this and start it again.");
                self.back = Screen::Home;
            }
            Key::Quit => return true,
            _ => {}
        }
        false
    }

    fn receive_key(&mut self, k: Key) -> bool {
        let found = self.found.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let list = found.unwrap_or_default();
        self.move_row(k, list.len() + 1);
        match k {
            Key::Enter => {
                if self.row < list.len() {
                    self.open_server(list[self.row].0, PORT);
                } else {
                    self.editing = Some(self.typed_address.clone());
                }
            }
            Key::Char('r') => self.start_looking(),
            Key::Esc => {
                self.screen = Screen::Home;
                self.row = 0;
            }
            Key::Char('q') | Key::Quit => return true,
            _ => {}
        }
        false
    }

    fn files_key(&mut self, k: Key) -> bool {
        self.move_row(k, self.files.len() + 2);
        match k {
            Key::Enter => {
                if self.row < 2 {
                    self.editing = Some(match self.row {
                        0 => self.save_into.clone(),
                        _ => self.at_once.to_string(),
                    });
                } else if let Some(e) = self.files.get(self.row - 2).cloned() {
                    self.begin_download(&e);
                }
            }
            Key::Esc => {
                self.screen = Screen::Receive;
                self.row = 0;
            }
            Key::Char('q') | Key::Quit => return true,
            _ => {}
        }
        false
    }

    fn receiving_key(&mut self, k: Key) -> bool {
        // A finished or failed download moves on by itself; the keys here are
        // only for stopping one that is still going.
        // Taken out of the lock first: holding it across self.note() borrows
        // self twice.
        let finished = self.result.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(r) = finished {
            match r {
                Ok(rate) => {
                    let name = self.downloading.clone().unwrap_or_default();
                    self.note(&format!(
                        "{name} is here.\n\nIt was saved in {}\n\nThat took {:.1} megabytes a second.",
                        self.save_into, rate
                    ));
                }
                Err(e) => self.note(&e),
            }
            self.back = Screen::ReceiveFiles;
            return false;
        }
        match k {
            Key::Char('q') | Key::Esc => {
                crate::fetch::cancel();
            }
            Key::Quit => {
                crate::fetch::cancel();
                return true;
            }
            _ => {}
        }
        false
    }
}

// ---------------------------------------------------------------- actions

impl App {
    fn note(&mut self, text: &str) {
        self.back = std::mem::replace(&mut self.screen, Screen::Note(text.to_string()));
        self.row = 0;
    }

    fn begin_sending(&mut self) {
        let folder = PathBuf::from(shellexpand(&self.folder));
        if !folder.is_dir() {
            self.note(&format!(
                "There is no folder called\n\n{}\n\nCheck the name, or make the folder first.",
                self.folder
            ));
            self.back = Screen::Send;
            return;
        }
        // The network before the port. Binding a port on a network the class
        // cannot reach looks like success and is not.
        if !self.ssid.is_empty() {
            match net::hotspot_up(&self.ssid, &self.password) {
                Ok(h) => {
                    // Armed BEFORE anything else can go wrong. systemd owns
                    // the timer, so the wifi comes back even if this program is
                    // killed outright, which Drop and panic hooks cannot cover.
                    h.arm_restore(net::RESTORE_FUSE);
                    self.hotspot = Some(h);
                    net::start_heartbeat();
                }
                Err(e) => {
                    self.note(&e);
                    self.back = Screen::Send;
                    return;
                }
            }
        }
        let addr = format!("0.0.0.0:{PORT}");
        if let Err(e) = serve::start(&folder, &addr, self.helpers) {
            if let Some(h) = self.hotspot.take() {
                h.down();
                h.disarm_restore();
            }
            self.note(&format!(
                "Could not start handing files out.\n\n{e}\n\nAnother copy of this may already be running."
            ));
            self.back = Screen::Send;
            return;
        }
        self.addresses = net::local_addresses();
        self.started = Some(Instant::now());
        self.screen = Screen::Sending;
        self.row = 0;
    }

    fn start_looking(&mut self) {
        *self.found.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let found = Arc::clone(&self.found);
        std::thread::spawn(move || {
            let list = net::find_servers(PORT);
            *found.lock().unwrap_or_else(|e| e.into_inner()) = Some(list);
        });
        self.row = 0;
    }

    fn open_typed(&mut self) {
        // "10.42.0.1", "10.42.0.1:8080" and "http://10.42.0.1:8080/" are all
        // things a person will type, and all three are the same answer.
        let t = self
            .typed_address
            .trim()
            .trim_start_matches("http://")
            .trim_end_matches('/');
        let (host, port) = match t.split_once(':') {
            Some((h, p)) => (h, p.parse().unwrap_or(PORT)),
            None => (t, PORT),
        };
        match host.parse() {
            Ok(ip) => self.open_server(ip, port),
            Err(_) => {
                self.note(&format!(
                    "{} is not an address.\n\nAn address looks like 10.42.0.1",
                    self.typed_address
                ));
                self.back = Screen::Receive;
            }
        }
    }

    fn open_server(&mut self, ip: std::net::Ipv4Addr, port: u16) {
        match net::list_files(ip, port) {
            Ok(files) => {
                self.files = files;
                self.server = Some(ip);
                self.server_port = port;
                self.screen = Screen::ReceiveFiles;
                self.row = 2;
            }
            Err(e) => {
                self.note(&format!(
                    "Could not read the list of files on {ip}.\n\n{e}\n\nIs that computer still handing them out?"
                ));
                self.back = Screen::Receive;
            }
        }
    }

    fn begin_download(&mut self, entry: &net::Entry) {
        let Some(ip) = self.server else { return };
        let url = format!("http://{ip}:{}/{}", self.server_port, entry.name);
        let dest = PathBuf::from(shellexpand(&self.save_into)).join(&entry.name);
        let dest = dest.to_string_lossy().into_owned();
        let at_once = self.at_once;
        let result = Arc::clone(&self.result);
        *result.lock().unwrap_or_else(|e| e.into_inner()) = None;
        crate::fetch::set_quiet(true);
        std::thread::spawn(move || {
            // Verification on, always. It costs nothing when the other side
            // offers no fingerprints, and when it does it is the difference
            // between a damaged lesson and a known-good one.
            let r = crate::fetch::download(&url, &dest, at_once, true);
            *result.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
        });
        self.downloading = Some(entry.name.clone());
        self.since = Some(Instant::now());
        self.screen = Screen::Receiving;
        self.row = 0;
    }
}

// ---------------------------------------------------------------- formatting

fn bar(fraction: f64, width: usize) -> String {
    // ASCII, not block characters. A full block and a light shade are East
    // Asian Ambiguous width: one column here, two columns in a terminal
    // configured for Chinese, which silently doubles the length of the bar and
    // wraps the line.
    let n = ((fraction.clamp(0.0, 1.0)) * width as f64).round() as usize;
    format!("{}{}", "#".repeat(n), "-".repeat(width - n))
}

fn human(bytes: u64) -> String {
    const K: f64 = 1000.0;
    let b = bytes as f64;
    if b >= K * K * K {
        format!("{:.1} GB", b / (K * K * K))
    } else if b >= K * K {
        format!("{:.1} MB", b / (K * K))
    } else if b >= K {
        format!("{:.0} KB", b / K)
    } else {
        format!("{bytes} B")
    }
}

fn duration(secs: f64) -> String {
    let s = secs as u64;
    if s >= 3600 {
        format!("{} hours {} minutes", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{} minutes", s / 60)
    } else {
        format!("{s} seconds")
    }
}

fn wrap(line: &str, cols: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in line.split(' ') {
        if !cur.is_empty() && term::width(&cur) + 1 + term::width(word) > cols {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A teacher types `~/lessons`, because that is what is written in every set of
/// instructions they have ever seen. The shell expands it; a program started by
/// double-clicking has no shell.
fn shellexpand(p: &str) -> String {
    if let Some(rest) = p.strip_prefix('~') {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return format!("{}{}", home.to_string_lossy(), rest);
        }
    }
    p.to_string()
}
