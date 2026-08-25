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

/// 8080 unless the bench overrides it. The pty test harness has to run a
/// second copy on a machine where a real one may already be serving, and
/// killing the teacher's live instance to free the port is not a test.
fn port() -> u16 {
    std::env::var("HUB_PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(8080)
}
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
    /// The tick list: which files in the folder the network is allowed to see.
    /// `pre` is the review before anything starts; the same screen mid-lesson
    /// is how the teacher publishes or withdraws a file while kids watch.
    Tick { pre: bool },
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
    joined: Vec<net::Joined>,
    joined_at: Option<Instant>,
    names: net::NameCache,
    /// (name, size, ticked) for the tick screen.
    tick: Vec<(String, u64, bool)>,
    /// The notice editor borrows the same editing buffer as the form fields;
    /// this flag says which thing a commit belongs to.
    editing_notice: bool,

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
            joined: Vec::new(),
            joined_at: None,
            names: net::NameCache::default(),
            tick: Vec::new(),
            editing_notice: false,
            found: Arc::new(Mutex::new(None)),
            server: None,
            server_port: port(),
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
            Screen::Tick { pre } => self.draw_tick(&mut f, pre),
            Screen::Sending => self.draw_sending(&mut f),
            Screen::Receive => self.draw_receive(&mut f),
            Screen::ReceiveFiles => self.draw_files(&mut f),
            Screen::Receiving => self.draw_receiving(&mut f),
            Screen::Note(_) => self.draw_note(&mut f),
        }
        f.draw();
    }

    fn title(&self, f: &mut Frame, s: &str) {
        // Indented to match the content below it. At column 0 against
        // two-column content the heading looked like it belonged to the
        // terminal rather than to the screen.
        f.push(&format!("  {s}"));
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
        let items: Vec<String> = ["Hand out files to the class", "Get files from another computer"]
            .iter()
            .map(|it| format!("  {it}"))
            .collect();
        let w = term::group_width(&items);
        for (i, it) in items.iter().enumerate() {
            if i == self.row {
                f.push_selected_within(it, w);
            } else {
                f.push(it);
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
        // The highlight width covers the form AND the button below it, so the
        // bar does not change size as the selection moves down onto Start.
        let mut plain: Vec<String> = fields
            .iter()
            .map(|(l, v)| format!("  {l:<label_width$}{v}"))
            .collect();
        plain.push("  Start handing out".into());
        let w = term::group_width(&plain);
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
                f.push_selected_within(&line, w);
            } else {
                f.push(&line);
            }
        }
        f.blank();
        let start = "  Start handing out";
        if self.row == fields.len() {
            f.push_selected_within(start, w);
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

    fn draw_tick(&self, f: &mut Frame, pre: bool) {
        self.title(f, "What gets handed out");
        if self.tick.is_empty() {
            f.push("  The folder has no files in it.");
        }
        let lines: Vec<String> = self
            .tick
            .iter()
            .map(|(n, sz, t)| format!("  [{}] {:<34}{:>10}", if *t { "x" } else { " " }, term::truncate(n, 32), human(*sz)))
            .collect();
        let w = term::group_width(&lines);
        let room = f.rows.saturating_sub(f.used() + 5);
        for (i, line) in lines.iter().enumerate().take(room) {
            if i == self.row {
                f.push_selected_within(line, w);
            } else {
                f.push(line);
            }
        }
        if lines.len() > room {
            f.push_dim(&format!("  and {} more not shown, the window is too short", lines.len() - room));
        }
        f.blank();
        let ticked = self.tick.iter().filter(|(_, _, t)| *t).count();
        f.push(&format!("  {ticked} of {} will be visible to the class.", self.tick.len()));
        if !pre {
            f.push_dim("  Ticking a file hands it out NOW; unticking withdraws it.");
        }
        self.hints(f, "  space to tick    a all    n none    enter to continue    esc to go back");
    }

    fn draw_sending(&mut self, f: &mut Frame) {
        // A hotspot does not have an address the instant nmcli returns; the
        // interface has to come up and be given one. Asking again while the
        // list is empty costs four UDP sockets and stops the screen from
        // saying nothing at the one moment the teacher needs the address.
        if self.addresses.is_empty() {
            self.addresses = net::local_addresses();
        }
        // Once a second, not four times: two small files, but there is no
        // reason to read them at the frame rate.
        let stale = self.joined_at.map(|t| t.elapsed().as_secs() >= 1).unwrap_or(true);
        if stale {
            // Only when WE made the network. Handing files out over a network
            // somebody else provided, "who is on it" is the whole building, and
            // a teacher's screen filling with three hundred strangers is worse
            // than showing none of them.
            self.joined = match &self.hotspot {
                Some(h) => match h.address() {
                    Some(ours) => {
                        let list = net::joined_devices(ours);
                        // The names are already known: every device said what
                        // it was called when it asked for an address, and the
                        // same dnsmasq that wrote them down is the DNS server
                        // for this network. Asking it needs no privileges, so
                        // a teacher sees "Amina-Laptop" rather than a number
                        // without touching a terminal or a password prompt.
                        for j in &list {
                            if j.name.is_none() {
                                self.names.ensure(j.ip, ours);
                            }
                            // Whatever name we have, the serving side gets it
                            // too, so handed-in files and notes are labelled
                            // "Amina-phone" rather than an address.
                            if let Some(n) = j.name.clone().or_else(|| self.names.get(j.ip)) {
                                serve::set_device_name(&j.ip.to_string(), &n);
                            }
                        }
                        list
                    }
                    None => Vec::new(),
                },
                None => Vec::new(),
            };
            self.joined_at = Some(Instant::now());
        }

        self.title(f, "Handing out files");
        if let Some(h) = &self.hotspot {
            f.push(&format!("  Wifi network      {}", h.ssid));
            f.push(&format!("  Password          {}", self.password));
        }
        let port80 = serve::on_port_80();
        for a in &self.addresses {
            if port80 {
                f.push(&format!("  Address to type   http://{a}"));
            } else {
                f.push(&format!("  Address to type   http://{a}:{}", port()));
            }
        }
        // Two loud states a USB drive causes. The folder is often a flash
        // drive kept as the teacher's failsafe, and it gets unplugged, filled
        // and write-locked as a matter of course.
        let root = PathBuf::from(shellexpand(&self.folder));
        if !root.exists() {
            f.blank();
            f.push("  THE FOLDER CANNOT BE REACHED. If it lives on a USB drive,");
            f.push("  the drive may have been unplugged. Files stop until it is back.");
        } else if !serve::handin_available() {
            f.push_dim("  Hand-in is off: that folder cannot receive files (read-only?).");
        }
        let notice = crate::page::notice();
        if !notice.is_empty() {
            f.push(&format!("  Notice            {}", notice));
        }
        f.blank();

        let live = serve::transfers();
        let sent = serve::total_sent();
        let getting = live.iter().filter(|t| !t.finished && !t.handing_in).count();
        let handing = live.iter().filter(|t| !t.finished && t.handing_in).count();
        let mut rows: Vec<String> = Vec::new();

        // One row per DEVICE ON THE NETWORK, whether or not it has asked for
        // anything. A phone that joins and waits is the normal state at the
        // start of a lesson, and it used to show as nothing at all.
        for j in &self.joined {
            // Lease name if we could read it (root), then the name the network
            // answered with, then the bare address.
            let who = j
                .name
                .clone()
                .or_else(|| self.names.get(j.ip))
                .unwrap_or_else(|| j.ip.to_string());
            match live.iter().find(|t| t.peer == j.ip.to_string()) {
                Some(t) => rows.push(transfer_row(&who, t)),
                None if serve::has_seen_page(&j.ip.to_string()) => {
                    rows.push(format!("  {:<24}looking at the page", term::truncate(&who, 22)));
                }
                None => rows.push(format!("  {:<24}on the network, has not opened the page yet", term::truncate(&who, 22))),
            }
        }
        // Anything downloading from an address that is not on our subnet: the
        // case where the class is on a network somebody else provided.
        for t in &live {
            if !self.joined.iter().any(|j| j.ip.to_string() == t.peer) {
                let who = serve::device_label(&t.peer);
                rows.push(transfer_row(&who, t));
            }
        }

        // With a hotspot the joined list is authoritative. Over somebody
        // else's network the only headcount is who has opened the page.
        let on_net = self.joined.len().max(rows.len()).max(serve::pages_seen_count());
        f.push(&match (on_net, getting + handing) {
            (0, 0) => "  Nothing has connected yet.".to_string(),
            (n, 0) => format!("  {n} on the network, none moving files, {} sent", human(sent)),
            (n, _) => format!(
                "  {n} on the network, {getting} getting, {handing} handing in, {} sent",
                human(sent)
            ),
        });
        f.blank();

        if rows.is_empty() {
            f.push_dim("  On a phone or any computer: join the wifi and the sign-in");
            f.push_dim("  screen brings them here by itself. Or open a browser at the");
            f.push_dim("  address above.");
        }
        let notes = crate::page::notes(4);
        let note_rows = if notes.is_empty() { 0 } else { notes.len() + 1 };
        // Capped by what is left on screen, and what was dropped is said out
        // loud. A list that silently stops at ten reads as "ten devices".
        let room = f.rows.saturating_sub(f.used() + 3 + note_rows);
        for r in rows.iter().take(room) {
            f.push(r);
        }
        if rows.len() > room {
            f.push_dim(&format!("  and {} more not shown, the window is too short", rows.len() - room));
        }
        if !notes.is_empty() {
            f.blank();
            for (who, text) in &notes {
                f.push_dim(&format!("  {}: {}", term::truncate(who, 18), text));
            }
        }
        self.hints(f, "  f files    n notice    q or esc to stop handing out");
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
                        f.push_selected_within(&line, 40);
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
            f.push_selected_within(&manual, 40);
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
            (Some(ip), p) if p == port() => ip.to_string(),
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
                f.push_selected_within(&line, self.files_width());
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
                f.push_selected_within(&line, self.files_width());
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

    /// One width for the settings and the file list on that screen, so the
    /// highlight does not jump in size between them.
    fn files_width(&self) -> usize {
        let mut lines: Vec<String> = vec![
            format!("  {:<16}{}", "Save into", self.save_into),
            format!("  {:<16}{}", "Pieces at once", self.at_once),
        ];
        lines.extend(self.files.iter().map(|e| format!("  {:<34}{:>10}", e.name, human(e.size))));
        term::group_width(&lines)
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
            Screen::Tick { pre } => self.tick_key(k, pre),
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
        if self.editing_notice {
            self.editing_notice = false;
            crate::page::set_notice(&buf);
            return;
        }
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
                    self.open_tick(true);
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
            Key::Char('f') => {
                self.open_tick(false);
                return false;
            }
            Key::Char('n') => {
                self.editing_notice = true;
                self.editing = Some(crate::page::notice());
                return false;
            }
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

    fn tick_key(&mut self, k: Key, pre: bool) -> bool {
        self.move_row(k, self.tick.len().max(1));
        match k {
            Key::Char(' ') => {
                if let Some(e) = self.tick.get_mut(self.row) {
                    e.2 = !e.2;
                }
                if !pre {
                    self.apply_ticks();
                }
            }
            Key::Char('a') => {
                for e in &mut self.tick {
                    e.2 = true;
                }
                if !pre {
                    self.apply_ticks();
                }
            }
            Key::Char('n') => {
                for e in &mut self.tick {
                    e.2 = false;
                }
                if !pre {
                    self.apply_ticks();
                }
            }
            Key::Enter => {
                self.apply_ticks();
                if pre {
                    self.begin_sending();
                } else {
                    self.screen = Screen::Sending;
                    self.row = 0;
                }
            }
            Key::Esc => {
                if pre {
                    self.screen = Screen::Send;
                } else {
                    self.apply_ticks();
                    self.screen = Screen::Sending;
                }
                self.row = 0;
            }
            Key::Char('q') if !pre => {
                // q on the mid-lesson tick screen must not quietly quit the
                // whole program while a class is connected.
                self.apply_ticks();
                self.screen = Screen::Sending;
                self.row = 0;
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
                    let ip = list[self.row].0;
                    if net::list_files(ip, 80).is_ok() {
                        self.open_server(ip, 80);
                    } else {
                        self.open_server(ip, port());
                    }
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

    /// Read the folder and open the tick screen. Mid-lesson the list is
    /// re-read so a file copied in during the lesson appears, UNTICKED:
    /// putting a file in the folder is not publishing it, ticking it is.
    fn open_tick(&mut self, pre: bool) {
        let folder = PathBuf::from(shellexpand(&self.folder));
        if pre && !folder.is_dir() {
            self.note(&format!(
                "There is no folder called\n\n{}\n\nCheck the name, or make the folder first.",
                self.folder
            ));
            self.back = Screen::Send;
            return;
        }
        let mut fresh: Vec<(String, u64, bool)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&folder) {
            let mut items: Vec<_> = rd.flatten().collect();
            items.sort_by_key(|e| e.file_name());
            for e in items {
                let Ok(md) = e.metadata() else { continue };
                if !md.is_file() {
                    continue;
                }
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let ticked = if pre {
                    true // the review starts from "everything", the teacher unticks
                } else {
                    self.tick.iter().find(|(n, _, _)| *n == name).map(|(_, _, t)| *t).unwrap_or(false)
                };
                fresh.push((name, md.len(), ticked));
            }
        }
        self.tick = fresh;
        self.screen = Screen::Tick { pre };
        self.row = 0;
    }

    fn apply_ticks(&mut self) {
        let set: std::collections::HashSet<String> =
            self.tick.iter().filter(|(_, _, t)| *t).map(|(n, _, _)| n.clone()).collect();
        serve::set_allowed(Some(set));
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
        let addr = format!("0.0.0.0:{}", port());
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
            // 80 first (the packaged install), 8080 for an unpackaged build.
            let mut list = net::find_servers(80);
            let more = net::find_servers(port());
            for (ip, n) in more {
                if !list.iter().any(|(i, _)| *i == ip) {
                    list.push((ip, n));
                }
            }
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
        let (host, typed_port) = match t.split_once(':') {
            Some((h, p)) => (h, Some(p.parse().unwrap_or_else(|_| port()))),
            None => (t, None),
        };
        match host.parse() {
            Ok(ip) => {
                // No port typed: the packaged teacher machine answers on 80,
                // an unpackaged one on 8080. Try both rather than teach anyone
                // what a port is.
                match typed_port {
                    Some(p) => self.open_server(ip, p),
                    None => {
                        if net::list_files(ip, 80).map(|_| ()).is_ok() {
                            self.open_server(ip, 80);
                        } else {
                            self.open_server(ip, port());
                        }
                    }
                }
            }
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

fn transfer_row(who: &str, t: &serve::Transfer) -> String {
    let pct = if t.total > 0 { t.done as f64 / t.total as f64 } else { 0.0 };
    format!(
        "  {:<24}{} {:>3}%  {:>12}  {}",
        term::truncate(who, 22),
        bar(pct, 16),
        (pct * 100.0) as u64,
        if t.finished { "done".to_string() } else { format!("{:.1} MB/s", t.rate / 1e6) },
        t.file
    )
}

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
