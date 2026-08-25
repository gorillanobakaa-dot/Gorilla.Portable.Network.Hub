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
    /// Who is on the network, and the two ways to remove somebody.
    Class,
    /// Confirming a password change, which knocks the whole room off.
    NewPassword,
    /// The join code, for the phones in the room whose cameras work.
    JoinCode,
    /// Work children have sent, waiting for the teacher to accept or refuse.
    Waiting,
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
    /// The replacement password being typed on the change-password screen.
    ///
    /// Deliberately NOT the shared `editing` buffer. Anything in `editing` is
    /// intercepted before the per-screen keys and committed through commit(),
    /// so a password typed there would be filed as a folder name and the
    /// change would never run.
    new_password: String,
    /// What Tab found: the sibling names when a completion was ambiguous,
    /// drawn under the field being edited and cleared by the next keystroke.
    tab_hint: Option<String>,

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
            new_password: String::new(),
            tab_hint: None,
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
        // Before anything is drawn, and for every screen. See refresh_joined:
        // this being a side effect of one screen's drawing cost a laptop its
        // identity in the permanent record.
        if self.hotspot.is_some() {
            self.refresh_joined();
        }
        let mut f = Frame::new(rows, cols);
        match self.screen {
            Screen::Home => self.draw_home(&mut f),
            Screen::Send => self.draw_send(&mut f),
            Screen::Tick { pre } => self.draw_tick(&mut f, pre),
            Screen::Waiting => self.draw_waiting(&mut f),
            Screen::Sending => self.draw_sending(&mut f),
            Screen::Class => self.draw_class(&mut f),
            Screen::NewPassword => self.draw_newpassword(&mut f),
            Screen::JoinCode => self.draw_joincode(&mut f),
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

    fn draw_waiting(&self, f: &mut Frame) {
        self.title(f, "Work waiting for you");
        let items = serve::pending();
        if items.is_empty() {
            f.push("  Nothing is waiting.");
            f.blank();
            f.push_dim("  Work a child sends arrives here first. Nothing lands in");
            f.push_dim("  your folder until you accept it, and nothing sent to you");
            f.push_dim("  is ever handed back out to the rest of the class.");
            self.hints(f, "  esc to go back");
            return;
        }
        let lines: Vec<String> = items
            .iter()
            .map(|p| format!("  {:<30}{:<26}{:>9}", term::truncate(&p.from, 28), term::truncate(&p.original, 24), human(p.bytes)))
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
        if let Some(p) = items.get(self.row) {
            f.push_dim(&format!("  sent {}", p.at));
        }
        self.hints(f, "  a accept    r refuse    o open and look    esc to go back");
    }

}

/// One line on the class screen.
struct ClassRow {
    /// The address, while the device is still on the network.
    ip: Option<String>,
    /// What a block on this device is filed under. Present for a paused device
    /// that has since disappeared, which is the only handle left on it.
    key: Option<String>,
    label: String,
    state: String,
    blocked: bool,
}

impl App {
    /// Who is on the network, built fresh from live state.
    ///
    /// Built by ONE function used by both the drawing and the keys, because
    /// the cursor and the thing the cursor acts on have to be the same list.
    /// Two lists built the same way from the same data is how a teacher ends
    /// up pausing the child below the one they highlighted.
    fn class_rows(&self) -> Vec<ClassRow> {
        let live = serve::transfers();
        let mut rows: Vec<ClassRow> = Vec::new();
        for j in &self.joined {
            let ip = j.ip.to_string();
            // Same precedence as the roster: the name the child picked, then
            // the lease name, then the reverse lookup, then the address.
            let who = serve::claimed_name(&ip)
                .or_else(|| j.name.clone())
                .or_else(|| self.names.get(j.ip))
                .unwrap_or_else(|| ip.clone());
            // The tag is what a name cannot be argued out of, so the screen
            // that hands out punishment shows it.
            let label = match serve::tag_for(&ip) {
                Some(t) => format!("{who} #{t}"),
                None => who,
            };
            let blocked = serve::is_blocked(&ip);
            let state = if blocked {
                "PAUSED BY YOU".to_string()
            } else if let Some(t) = live.iter().find(|t| t.peer == ip && !t.finished) {
                if t.handing_in { "handing work in".to_string() } else { "getting a file".to_string() }
            } else if serve::has_seen_page(&ip) {
                "looking at the page".to_string()
            } else {
                "on the network".to_string()
            };
            rows.push(ClassRow {
                ip: Some(ip.clone()),
                key: Some(serve::device_key(&ip)),
                label,
                state,
                blocked,
            });
        }
        // Paused devices that are no longer on the network still need a row.
        // A phone that is switched off, or has wandered out of range, drops
        // off every other list on this screen; if its block dropped off with
        // it there would be no way to undo one, and the teacher would find out
        // at the start of the next lesson.
        for (key, label) in serve::blocked_devices() {
            if rows.iter().any(|r| r.key.as_deref() == Some(key.as_str())) {
                continue;
            }
            rows.push(ClassRow {
                ip: None,
                key: Some(key),
                label,
                state: "PAUSED, and no longer on the network".to_string(),
                blocked: true,
            });
        }
        rows
    }

    fn draw_class(&self, f: &mut Frame) {
        self.title(f, "Who is on the network");
        let rows = self.class_rows();
        if rows.is_empty() {
            f.push("  Nobody has joined yet.");
            self.hints(f, "  p change the wifi password    esc to go back");
            return;
        }
        let lines: Vec<String> = rows
            .iter()
            .map(|r| format!("  {:<34}{}", term::truncate(&r.label, 32), r.state))
            .collect();
        let w = term::group_width(&lines);
        // Six rows kept back for the two explanations and the hint line. They
        // are not decoration: a teacher who believes a pause is a lock will
        // find out otherwise from a child, not from the screen.
        let room = f.rows.saturating_sub(f.used() + 7);
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
        f.push_dim("  A paused device still has the wifi. It just cannot reach this");
        f.push_dim("  lesson: no files, no handing in, no notes.");
        f.push_dim("  A phone can come back wearing a different name. The #tag is the");
        f.push_dim("  part that does not change. If one keeps coming back, change the");
        f.push_dim("  password: that is the one they cannot walk around.");
        let hint = match rows.get(self.row) {
            Some(r) if r.blocked => "  space let them back in    p change the wifi password    esc to go back",
            _ => "  space pause this device    p change the wifi password    esc to go back",
        };
        self.hints(f, hint);
    }

    /// The join code, and the words that make it optional.
    ///
    /// The credentials are printed alongside, always. That is not belt and
    /// braces, it is the main lane: a good share of the phones in these rooms
    /// have a cracked camera, a camera app that wants an account first, or an
    /// Android old enough to have no scanner in the camera at all. A screen
    /// that shows only a code has locked those children out in a way a teacher
    /// cannot see from the front of the room.
    ///
    /// Laid out tight on purpose. A version 3 code with the quiet zone the
    /// standard requires is 19 rows, and the default terminal is 24, so every
    /// other line on this screen has to earn its place. Anything optional is
    /// added only once the code is known to fit.
    fn draw_joincode(&self, f: &mut Frame) {
        let Some(h) = &self.hotspot else {
            self.title(f, "Join by camera");
            f.push("  This computer did not make the network, so there is no");
            f.push("  password for it to put in a code.");
            f.blank();
            f.push_dim("  The class is on a network somebody else set up. They join it");
            f.push_dim("  the way they always do, then open the address on the roster.");
            self.hints(f, "  esc to go back");
            return;
        };
        let code = crate::qr::wifi_join(&h.ssid, &self.password);
        // Four modules of quiet zone on every side, which the standard requires
        // and cameras genuinely rely on. Not negotiable for a shorter window:
        // a code with a clipped margin is one that fails in the room.
        const QUIET: usize = 4;

        f.push(&format!("  Wifi network {}      Password {}", h.ssid, self.password));
        f.blank();

        let Some(code) = code else {
            f.push("  That network name and password are too long to fit in a code.");
            f.push("  Everything still works; the class types them in as usual.");
            self.hints(f, "  esc to go back");
            return;
        };
        let (cols, rows) = crate::qr::rendered_size(&code, QUIET);
        // Measured BEFORE anything is drawn. A code that runs off the edge of
        // the window is not a smaller code, it is an unreadable one, and half a
        // code is worse than a line saying there is no room for one.
        let room = f.rows.saturating_sub(f.used() + 1);
        if cols + 2 > f.cols || rows > room {
            f.push("  The window is too small to draw the code.");
            f.push(&format!("  It needs {} rows and {} columns; this window has {} by {}.", rows + 3, cols + 2, f.rows, f.cols));
            f.push("  Make it bigger, or just read the password out.");
            self.hints(f, "  esc to go back");
            return;
        }
        for line in crate::qr::render(&code, QUIET) {
            f.push_raw(&format!("  {line}"));
        }
        // Only if the window can spare them.
        if f.rows.saturating_sub(f.used()) > 3 {
            f.blank();
            f.push_dim("  Point a phone camera at this. If nothing happens, that phone");
            f.push_dim("  cannot scan; type the name and password in above instead.");
        }
        self.hints(f, "  esc to go back");
    }

    fn draw_newpassword(&self, f: &mut Frame) {
        self.title(f, "Change the wifi password");
        f.push("  This knocks EVERY device off the network, not just one.");
        f.blank();
        f.push("  Everybody has to type the new password to come back, so plan");
        f.push("  on writing it on the board before you press enter.");
        f.blank();
        f.push(&format!("  New password      {}", self.new_password));
        f.blank();
        if serve::blocked_count() > 0 {
            f.push_dim(&format!(
                "  The {} paused device{} stay paused. Changing the password does",
                serve::blocked_count(),
                if serve::blocked_count() == 1 { "" } else { "s" }
            ));
            f.push_dim("  not let anybody back in.");
        }
        self.hints(f, "  type to edit    enter to change it    esc to leave it alone");
    }

    /// Who is on the network, and what each device is called. Runs on a tick
    /// whatever screen is showing.
    ///
    /// This USED TO live inside the roster's draw function, and that was a real
    /// bug rather than untidiness. A teacher sitting on the class screen or
    /// looking through waiting work froze the whole thing: no new device was
    /// noticed, and no name lookup was ever STARTED for one. Seen in the record
    /// on 2026-08-25, a laptop that joined while the teacher was on another
    /// screen was filed as `biggus.dickus #nzrm [10.42.0.251]`, with the device
    /// column empty, while a phone that joined during the roster got its model.
    /// The device column is the part a child cannot argue with, so losing it
    /// because of which screen somebody happened to be looking at is the worst
    /// place to lose it.
    ///
    /// The lesson generalises past this program: work everything depends on
    /// must not be a side effect of drawing one screen.
    fn refresh_joined(&mut self) {
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
        if !stale {
            return;
        }
        // Only when WE made the network. Handing files out over a network
        // somebody else provided, "who is on it" is the whole building, and
        // a teacher's screen filling with three hundred strangers is worse
        // than showing none of them.
        self.joined = match &self.hotspot {
            Some(h) => match h.address() {
                Some(ours) => {
                    let list = net::joined_devices(ours);
                    // Most names are already known: a device says what it is
                    // called when it asks for an address, and the same dnsmasq
                    // that wrote that down is the DNS server for this network.
                    // Asking it needs no privileges, so a teacher sees
                    // "Amina-Laptop" rather than a number without touching a
                    // terminal or a password prompt.
                    for j in &list {
                        if j.name.is_none() {
                            self.names.ensure(j.ip, ours);
                        }
                        // Whatever name we have, the serving side gets it too,
                        // so handed-in files and notes are labelled
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

    fn draw_sending(&mut self, f: &mut Frame) {
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
        if self.hotspot.is_some() && port80 {
            // The dnsmasq drop-in answers these names on OUR hotspot only.
            // The slash is what stops a phone's browser treating the word as
            // a search; a colon is three keyboard layers deep, a slash is on
            // the first.
            f.push_dim("  or type           classroom/   (the slash matters)");
        }
        if self.hotspot.is_some() && !port80 {
            // The sign-in pop points at port 80. If something else owns it,
            // every joining phone gets THAT program's page and this lesson is
            // invisible, which is exactly what happened on 2026-08-25: a
            // leftover test copy served its junk to a real phone while the
            // real lesson sat unreachable on 8080. Silent was the failure;
            // loud is the fix.
            f.blank();
            f.push("  ANOTHER PROGRAM owns the sign-in page on this computer.");
            f.push("  Phones that join will see that program, not this lesson.");
            f.push(&format!("  Close it, or tell the class to type the address WITH :{}", port()));
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
        let dupes = serve::duplicate_claims();
        let getting = live.iter().filter(|t| !t.finished && !t.handing_in).count();
        let handing = live.iter().filter(|t| !t.finished && t.handing_in).count();
        let mut rows: Vec<String> = Vec::new();

        // One row per DEVICE ON THE NETWORK, whether or not it has asked for
        // anything. A phone that joins and waits is the normal state at the
        // start of a lesson, and it used to show as nothing at all.
        for j in &self.joined {
            // Lease name if we could read it (root), then the name the network
            // answered with, then the bare address.
            // The name the kid picked outranks everything: that is the whole
            // point of asking. Then the lease name, then the reverse lookup,
            // then the bare address.
            let who = serve::claimed_name(&j.ip.to_string())
                .or_else(|| j.name.clone())
                .or_else(|| self.names.get(j.ip))
                .unwrap_or_else(|| j.ip.to_string());
            if serve::is_blocked(&j.ip.to_string()) {
                rows.push(format!("  {:<34}PAUSED BY YOU", term::truncate(&who, 32)));
                continue;
            }
            match live.iter().find(|t| t.peer == j.ip.to_string()) {
                Some(t) => rows.push(transfer_row(&who, t)),
                None if serve::has_seen_page(&j.ip.to_string()) => {
                    rows.push(format!("  {:<34}looking at the page", term::truncate(&who, 32)));
                }
                None => rows.push(format!("  {:<34}on the network, has not opened the page yet", term::truncate(&who, 32))),
            }
        }
        // Anything downloading from an address that is not on our subnet: the
        // case where the class is on a network somebody else provided.
        for t in &live {
            if !self.joined.iter().any(|j| j.ip.to_string() == t.peer) {
                let who = serve::roster_label(&t.peer);
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
        let waiting = serve::pending_count();
        if waiting > 0 {
            f.push(&format!(
                "  {waiting} PIECE{} OF WORK WAITING FOR YOU. Press w to look.",
                if waiting == 1 { "" } else { "S" }
            ));
            f.push_dim("  Nothing lands in your folder until you accept it.");
            f.blank();
        }
        let paused = serve::blocked_count();
        if paused > 0 {
            f.push(&format!(
                "  {paused} device{} paused. Press c to let {} back in.",
                if paused == 1 { "" } else { "s" },
                if paused == 1 { "it" } else { "them" }
            ));
            f.blank();
        }
        if !dupes.is_empty() {
            f.push(&format!(
                "  MORE THAN ONE DEVICE IS CALLING ITSELF: {}",
                dupes.join(", ")
            ));
            f.push_dim("  The #tag after each name tells those devices apart.");
            f.blank();
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
        self.hints(f, "  f files  n notice  w waiting  c who is on  j join code  q to stop");
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
        if let Some(h) = &self.tab_hint {
            f.blank();
            f.push_dim(&format!("  {h}"));
        }
        if self.editing.is_some() {
            self.hints(f, "  type to change    tab completes a path    enter to keep it    esc to leave it");
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
            Screen::Waiting => self.waiting_key(k),
            Screen::Sending => self.sending_key(k),
            Screen::Class => self.class_key(k),
            Screen::NewPassword => self.newpassword_key(k),
            Screen::JoinCode => self.joincode_key(k),
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
        if !matches!(k, Key::Tab) {
            self.tab_hint = None;
        }
        match k {
            Key::Char(c) => {
                buf.push(c);
                self.editing = Some(buf);
            }
            Key::Backspace => {
                buf.pop();
                self.editing = Some(buf);
            }
            Key::Tab => {
                // Shell muscle memory, honoured. Decades of fingers know that
                // /home/gorilla/P and Tab either finishes the word or shows
                // the choices; a field that ignores it is a field that makes
                // somebody type a path out letter by letter like it is 1985.
                let is_path_field = (matches!(self.screen, Screen::Send) && self.row == 0)
                    || (matches!(self.screen, Screen::ReceiveFiles) && self.row == 0);
                if is_path_field && !self.editing_notice {
                    let (done, hint) = complete_path(&buf);
                    self.editing = Some(done);
                    self.tab_hint = hint;
                }
            }
            Key::Esc => {
                self.editing = None;
                self.editing_notice = false;
            }
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
            Key::Char('w') => {
                self.screen = Screen::Waiting;
                self.row = 0;
                return false;
            }
            Key::Char('c') => {
                self.screen = Screen::Class;
                self.row = 0;
                return false;
            }
            Key::Char('j') => {
                self.screen = Screen::JoinCode;
                self.row = 0;
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

    fn class_key(&mut self, k: Key) -> bool {
        let rows = self.class_rows();
        self.move_row(k, rows.len().max(1));
        match k {
            Key::Char(' ') => {
                let Some(r) = rows.get(self.row) else { return false };
                if r.blocked {
                    // By key, not by address. A paused device that has gone
                    // quiet has no address left to name it by, and that is the
                    // one most likely to need letting back in.
                    match &r.key {
                        Some(key) => serve::unblock_key(key),
                        None => {}
                    }
                } else if let Some(ip) = &r.ip {
                    let who = serve::block_device(ip);
                    self.note(&format!(
                        "Paused {who}.\n\n\
                         That device can still see the wifi. It cannot get the class \
                         files, hand anything in, or send you a note.\n\n\
                         It sees a page saying you paused it. When you let it back in \
                         the page comes back on its own; the child does not have to \
                         do anything.\n\n\
                         If they reappear under a different name, the password is the \
                         way to remove them for real."
                    ));
                    self.back = Screen::Class;
                }
            }
            Key::Char('p') => {
                if self.hotspot.is_none() {
                    self.note(
                        "This computer did not make the network, so it cannot change \
                         its password.\n\nThe network belongs to whoever set up the \
                         router or the phone hotspot the class is using.",
                    );
                    self.back = Screen::Class;
                    return false;
                }
                // Offered filled in, so the common case is one keypress. A
                // teacher inventing a password under thirty pairs of eyes is
                // how a network ends up called 12345678.
                self.new_password = net::suggest_password();
                self.screen = Screen::NewPassword;
            }
            Key::Esc => {
                self.screen = Screen::Sending;
                self.row = 0;
            }
            Key::Quit => return true,
            _ => {}
        }
        false
    }

    fn joincode_key(&mut self, k: Key) -> bool {
        match k {
            Key::Esc | Key::Enter => {
                self.screen = Screen::Sending;
                self.row = 0;
            }
            Key::Char('q') | Key::Quit => return true,
            _ => {}
        }
        false
    }

    fn newpassword_key(&mut self, k: Key) -> bool {
        match k {
            Key::Enter => {
                let new = std::mem::take(&mut self.new_password);
                let Some(h) = self.hotspot.as_mut() else {
                    self.screen = Screen::Class;
                    return false;
                };
                match h.change_password(&new) {
                    Ok(()) => {
                        self.password = new.clone();
                        // The roster is now a list of devices that were on a
                        // network which no longer exists under that key. Clear
                        // it rather than let it decay, so the screen does not
                        // show a room that has already emptied.
                        self.joined.clear();
                        self.joined_at = None;
                        self.note(&format!(
                            "The network is back with a new password.\n\n\
                             {new}\n\n\
                             Everybody has been knocked off. Write that on the board; \
                             nobody can rejoin without it.\n\n\
                             The wifi name has not changed, so devices will still see \
                             it in their list."
                        ));
                        self.back = Screen::Class;
                    }
                    Err(e) => {
                        self.note(&format!("The password was not changed.\n\n{e}"));
                        self.back = Screen::Class;
                    }
                }
            }
            Key::Esc => {
                self.new_password.clear();
                self.screen = Screen::Class;
            }
            Key::Backspace => {
                self.new_password.pop();
            }
            Key::Char(c) => {
                // 63 is the WPA2 passphrase ceiling. Stopping at it here means
                // a teacher who leans on a key gets a full field rather than a
                // refusal from nmcli after the fact.
                if self.new_password.chars().count() < 63 {
                    self.new_password.push(c);
                }
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

    fn waiting_key(&mut self, k: Key) -> bool {
        let items = serve::pending();
        self.move_row(k, items.len().max(1));
        let root = PathBuf::from(shellexpand(&self.folder));
        match k {
            Key::Char('a') => {
                if let Some(p) = items.get(self.row) {
                    match serve::accept_pending(&root, &p.on_disk) {
                        Ok(()) => {}
                        Err(e) => {
                            self.note(&format!("Could not accept that file.\n\n{e}"));
                            self.back = Screen::Waiting;
                        }
                    }
                    self.row = self.row.min(serve::pending_count().saturating_sub(1));
                }
            }
            Key::Char('r') => {
                if let Some(p) = items.get(self.row) {
                    // Refused work is MOVED, never deleted: it may be
                    // evidence, and what disappears is not this program's call.
                    match serve::refuse_pending(&root, &p.on_disk) {
                        Ok(()) => {}
                        Err(e) => {
                            self.note(&format!("Could not refuse that file.\n\n{e}"));
                            self.back = Screen::Waiting;
                        }
                    }
                    self.row = self.row.min(serve::pending_count().saturating_sub(1));
                }
            }
            Key::Char('o') => {
                if let Some(p) = items.get(self.row) {
                    // Opened only when the teacher asks. Nothing a child sends
                    // is ever displayed unbidden on a screen a class can see.
                    let path = crate::page::waiting_dir(&root).join(&p.on_disk);
                    let _ = std::process::Command::new("xdg-open")
                        .arg(path)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
            }
            Key::Esc => {
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
    // How long she has to wait, so she knows whether she can shut the lid.
    // Only shown while it is moving and only when the rate is real enough to
    // divide by; a made-up estimate is worse than none.
    let right = if t.finished {
        "done".to_string()
    } else if t.rate > 1000.0 && t.total > t.done {
        format!("{:.1} MB/s {}", t.rate / 1e6, duration((t.total - t.done) as f64 / t.rate))
    } else {
        format!("{:.1} MB/s", t.rate / 1e6)
    };
    let way = if t.handing_in { "<- sending" } else { "-> getting" };
    format!(
        "  {:<30}{} {:>3}% {}  {:>18}  {}",
        term::truncate(who, 28),
        bar(pct, 14),
        (pct * 100.0) as u64,
        way,
        right,
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

/// Bash-shaped path completion over DIRECTORIES, for the two fields that hold
/// one. Returns the new buffer and, when the answer is ambiguous, the choices
/// to show. Only directories: both fields want a folder, and offering files
/// in them is noise.
fn complete_path(buf: &str) -> (String, Option<String>) {
    let expanded = shellexpand(buf);
    let (dir_part, prefix) = match expanded.rfind('/') {
        Some(i) => (expanded[..=i].to_string(), expanded[i + 1..].to_string()),
        None => ("./".to_string(), expanded.clone()),
    };
    let Ok(rd) = std::fs::read_dir(if dir_part.is_empty() { "/" } else { &dir_part }) else {
        return (buf.to_string(), None);
    };
    let mut matches: Vec<String> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&prefix) && (!n.starts_with('.') || prefix.starts_with('.')))
        .collect();
    matches.sort();
    match matches.len() {
        0 => (buf.to_string(), Some("nothing here starts with that".to_string())),
        1 => (format!("{dir_part}{}/", matches[0]), None),
        _ => {
            // Complete to the longest shared start, like a shell, and show
            // the choices so the next letter is an informed one.
            let mut common = matches[0].clone();
            for m in &matches[1..] {
                while !m.starts_with(&common) {
                    common.pop();
                }
            }
            let shown = matches
                .iter()
                .take(8)
                .map(|m| format!("{m}/"))
                .collect::<Vec<_>>()
                .join("  ");
            let more = if matches.len() > 8 {
                format!("  and {} more", matches.len() - 8)
            } else {
                String::new()
            };
            (format!("{dir_part}{common}"), Some(format!("{shown}{more}")))
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scaffold() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("hub-tab-test-{}", std::process::id()));
        for sub in ["Pictures", "Public", "Music", "Pictures/Screenshots"] {
            std::fs::create_dir_all(d.join(sub)).unwrap();
        }
        std::fs::write(d.join("Pictures/a-file.txt"), b"x").unwrap();
        d
    }

    #[test]
    fn a_unique_prefix_completes_with_a_trailing_slash() {
        let d = scaffold();
        let (done, hint) = complete_path(&format!("{}/Mu", d.display()));
        assert_eq!(done, format!("{}/Music/", d.display()));
        assert!(hint.is_none());
    }

    #[test]
    fn an_ambiguous_prefix_stops_at_the_shared_part_and_shows_the_choices() {
        let d = scaffold();
        let (done, hint) = complete_path(&format!("{}/P", d.display()));
        // P matches Pictures and Public: the shared start is "P".
        assert_eq!(done, format!("{}/P", d.display()));
        let h = hint.expect("choices must be shown");
        assert!(h.contains("Pictures/") && h.contains("Public/"), "{h}");
    }

    #[test]
    fn files_are_not_offered_because_the_field_wants_a_folder() {
        let d = scaffold();
        let (done, _) = complete_path(&format!("{}/Pictures/a-f", d.display()));
        assert_eq!(done, format!("{}/Pictures/a-f", d.display()), "a file must not complete");
    }

    #[test]
    fn completion_descends_into_the_completed_folder_on_the_next_tab() {
        let d = scaffold();
        let (done, _) = complete_path(&format!("{}/Pictures/Scr", d.display()));
        assert_eq!(done, format!("{}/Pictures/Screenshots/", d.display()));
    }
}
