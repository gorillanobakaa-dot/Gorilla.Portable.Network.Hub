// Version: 0.2.0 · updated 26-09-07-11-00
//
// 0.9.0 rebuilt the picker so nothing is ever typed. Space ticks files and
// folders at any depth, ticks are held as absolute paths and survive moving
// between folders, and the served root is the deepest folder containing all of
// them. Enter only adds, because it is the reflex key and making the reflex
// key the destructive one cost a real transfer. Also the cable screens, the
// checkup screen, and a live read of the address supervisor every frame.
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

use crate::cable;
use crate::dhcp;
use crate::net;
use crate::serve;
use crate::tune;
use crate::term::{self, Frame, Key, Keys};
use std::path::{Path, PathBuf};
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
    // Looked at straight away, in the background, so the first screen can say
    // whether this computer is ready without making anyone wait for it.
    app.refresh_services();
    std::thread::spawn(|| {
        let _ = net::wifi_card_summary();
    });
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
    /// Choosing a folder by looking, not by typing a path.
    Pick,
    /// What this computer can and cannot do, and the one button that fixes the
    /// commonest cause of "it does not work": a firewall quietly dropping
    /// every incoming connection while everything else looks healthy.
    Checkup,
    /// Parts of Windows the hub needs are switched off, found on the way into
    /// wifi or cable: say so there and offer the fix, rather than let the
    /// person meet the failure later in front of the class.
    FixOffer,
    Note(String),
}

/// Getting a whole folder rather than one file.
///
/// A folder is what a person actually points at. Before this existed the
/// receive screen could fetch exactly one selected file, so a resource pack of
/// two hundred files meant two hundred selections, and a folder of 68,130 was
/// simply not gettable at all.
#[derive(Clone)]
struct Batch {
    done: usize,
    total: usize,
    /// What is being fetched right now. More than one, when several files are
    /// in the air at the same time.
    in_flight: Vec<String>,
    bytes: u64,
    failed: Vec<String>,
    finished: bool,
    /// How many files at a time this run is using, for the screen to state.
    lanes: usize,
}

/// What a probe thread has found so far. `None` means still looking, which is
/// a different thing from "found nothing" and has to look different on screen.
type Found = Arc<Mutex<Option<Vec<(std::net::Ipv4Addr, usize, String)>>>>;

struct App {
    screen: Screen,
    back: Screen,
    row: usize,
    editing: Option<String>,

    // the send form
    folder: String,
    /// What `services::check` found, filled in by a background thread at start
    /// and after every fix. None while still looking, or off Windows.
    services: Arc<Mutex<Option<Vec<crate::services::Found>>>>,
    ssid: String,
    password: String,
    /// The password field was typed into, so it is theirs and is never
    /// swapped for the one saved with the network name.
    password_typed: bool,
    /// Which of the thirteen 2.4 GHz lanes to broadcast on. Empty means the
    /// system chooses, which is usually fine and occasionally costs the whole
    /// room 30%: see hotspot_up for the measured case.
    channel: String,
    helpers: usize,
    hotspot: Option<net::Hotspot>,
    /// Cable mode rather than wifi mode.
    ///
    /// The same form and the same serving machinery, with the wifi rows hidden
    /// (there is no network to name) and two extra things running beside it:
    /// the beacon that says where this computer is, and the address server the
    /// far end needs if it runs Linux.
    cable: bool,
    /// Stops the beacon and the address server when serving stops.
    cable_stop: Arc<std::sync::atomic::AtomicBool>,
    /// What the address server decided, in words, for the screen to show. A
    /// refusal here is normal and correct on a network that has a router.
    cable_note: String,
    /// The supervisor's live view of that, re-read on every frame so the
    /// screen can change while somebody is looking at it.
    cable_watch: Option<Arc<std::sync::Mutex<String>>>,
    /// The folder the picker is looking inside.
    pick_dir: PathBuf,
    /// Its subfolders, sorted, hidden ones left out.
    pick_kids: Vec<String>,
    /// Its files, sorted, with their sizes. Listed after the folders because
    /// the usual reason to be in a folder is to go deeper, and ten thousand
    /// files should not bury the one subfolder somebody is looking for.
    pick_files: Vec<(String, u64)>,
    /// What has been ticked, as absolute paths, so a tick survives moving to
    /// another folder. A person can take two files from here and a whole
    /// folder from three levels down in one pass.
    picked: Vec<PathBuf>,
    /// Another machine on this link already answers to our .local name.
    name_taken: bool,
    /// A ticked folder held more files than the walk will return.
    ///
    /// serve::all_files() stops at MAX_FILES and records it, and the serving
    /// screen already says so. The picker threw that away, so ticking a folder
    /// of 150,000 files produced a confident "100,000 files chosen" and 50,000
    /// files that would never be sent, with nothing anywhere saying a word.
    pick_truncated: bool,
    /// First subfolder shown, so a folder with two hundred children scrolls
    /// instead of drawing off the bottom of the window.
    pick_top: usize,
    /// The folder could not be read. A different thing from having no
    /// subfolders, and it has to say so differently.
    pick_unreadable: bool,
    /// Give out addresses even on a machine that is on another network.
    ///
    /// Off, and it takes a deliberate act to turn on, because the thing it
    /// switches off is the guard that stops this taking down a school network.
    /// It exists because the guard is also in the way of somebody testing on a
    /// laptop that has to stay online for other reasons, and a guard with no
    /// override gets worked around by worse means.
    anyway: bool,
    /// Exactly what may be handed out, when the picker was used to choose
    /// rather than to point at a folder. None means the whole folder.
    chosen: Option<std::collections::HashSet<String>>,
    /// Whether .local is being announced. Independent of the guard.
    mdns: bool,
    /// The same, for a hotspot whose address arrives after the screen has
    /// started: set by the waiting thread in dns::start_mdns_when_ready.
    mdns_live: Arc<std::sync::atomic::AtomicBool>,
    taken_live: Arc<std::sync::atomic::AtomicBool>,
    /// Whether we are answering names as well as handing out addresses. When
    /// true the other end can type a word instead of an address, and its own
    /// operating system should offer to open the page. When false, port 53 was
    /// refused, which on Linux and macOS means no root.
    naming: bool,
    started: Option<Instant>,
    addresses: Vec<std::net::Ipv4Addr>,
    joined: Vec<net::Joined>,
    joined_at: Option<Instant>,
    names: net::NameCache,
    /// (name, size, ticked) for the tick screen.
    tick: Vec<(String, u64, bool)>,
    /// Where received work goes. Shown on the start and sending screens.
    receive_dir: PathBuf,
    /// The picker is choosing where received work goes, not what to send.
    picking_receive: bool,
    /// Windows: make the network on 5 GHz instead of 2.4 GHz.
    band5: bool,
    /// The folder being looked at on the tick screen, relative to what is
    /// handed out, "" for the top. The list shows one folder at a time.
    tick_dir: String,
    /// First column on screen in the tick list.
    tick_first: usize,
    /// A big folder (or "all here") waiting for space to be pressed again.
    tick_confirm: Option<String>,
    /// What the last tick did, said in words under the list.
    tick_said: String,
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
    /// How many FILES are fetched at the same time.
    ///
    /// Separate from at_once, which is connections WITHIN one file, because
    /// the two solve different problems. Connections within a file share out
    /// airtime on a big file. Files in parallel hide round-trip latency, which
    /// is what actually dominates when the average file is five kilobytes:
    /// while one file's request is in flight the radio would otherwise sit
    /// idle, and 68,130 files at 4 ms each is minutes of nothing happening.
    files_at_once: usize,
    downloading: Option<String>,
    /// Progress across a whole folder, when the teacher asked for everything.
    batch: Arc<Mutex<Option<Batch>>>,
    result: Arc<Mutex<Option<Result<f64, String>>>>,
    since: Option<Instant>,
}

/// Where the folder field starts.
///
/// The working directory is the right answer when somebody typed `hub` in a
/// terminal, and the wrong one in the case this tool actually ships for.
/// Double-clicking the desktop shortcut sets the working directory to wherever
/// the program was installed, so the first thing a teacher saw offered as the
/// folder to hand out was the program's own install directory: never what
/// anybody wants to send, and sixty characters of path to edit down by hand.
///
/// So: the working directory when it looks like a place a person chose, and
/// the desktop when it looks like the program was double-clicked.
/// The deepest folder that contains every one of these paths.
///
/// The served root has to contain everything ticked. Tick two files in one
/// folder and that folder is the root; tick one here and one three levels
/// down and the root rises far enough to cover both.
fn common_ancestor(paths: &[PathBuf]) -> Option<PathBuf> {
    let mut it = paths.iter();
    let first = it.next()?;
    // A file contributes its folder, not itself: a file cannot be the root.
    let mut acc: Vec<std::ffi::OsString> = dir_of(first)
        .components()
        .map(|c| c.as_os_str().to_os_string())
        .collect();
    for p in it {
        let parts: Vec<std::ffi::OsString> = dir_of(p)
            .components()
            .map(|c| c.as_os_str().to_os_string())
            .collect();
        let keep = acc
            .iter()
            .zip(parts.iter())
            .take_while(|(a, b)| a == b)
            .count();
        acc.truncate(keep);
        if acc.is_empty() {
            // Different drives on Windows. There is no folder above both, so
            // there is nothing sensible to serve.
            return None;
        }
    }
    let mut out = PathBuf::new();
    for c in acc {
        out.push(c);
    }
    Some(out)
}

fn dir_of(p: &Path) -> PathBuf {
    if p.is_dir() {
        p.to_path_buf()
    } else {
        p.parent().map(|q| q.to_path_buf()).unwrap_or_else(|| p.to_path_buf())
    }
}

/// `full` written as a path relative to `root`, using forward slashes.
///
/// Forward slashes because that is what the server matches against and what a
/// URL carries. A backslash here would mean a file ticked on Windows is
/// invisible to the very page offering it.
fn relative_to(root: &Path, full: &Path) -> Option<String> {
    let rel = full.strip_prefix(root).ok()?;
    let s: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if s.is_empty() {
        None
    } else {
        Some(s.join("/"))
    }
}

fn starting_folder() -> PathBuf {
    let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let launched_from_install = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
        .map(|dir| dir == here)
        .unwrap_or(false);
    if !launched_from_install {
        return here;
    }
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from);
    match home {
        Some(h) => {
            let desktop = h.join("Desktop");
            if desktop.is_dir() {
                desktop
            } else {
                h
            }
        }
        None => here,
    }
}

impl App {
    fn new() -> App {
        let here = starting_folder();
        App {
            screen: Screen::Home,
            back: Screen::Home,
            row: 0,
            editing: None,
            folder: here.to_string_lossy().into_owned(),
            // On Windows the hub now makes the network itself, and in the places
            // this is for there is no other network to use. So a name is ready
            // and nobody has to invent one. Cleared, it serves over whatever
            // network the computer is already on, as before.
            ssid: if cfg!(windows) { "Gorilla Hub".to_string() } else { String::new() },
            channel: String::new(),
            // Offered, not imposed. A suggested password is the difference
            // between a teacher setting one and a teacher leaving the network
            // open because inventing a password is one more thing to do.
            password: net::suggest_password(),
            password_typed: false,
            services: Arc::new(Mutex::new(None)),
            mdns_live: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            taken_live: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            helpers: serve::default_helpers(),
            hotspot: None,
            cable: false,
            cable_stop: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cable_note: String::new(),
            cable_watch: None,
            pick_dir: PathBuf::from("."),
            pick_kids: Vec::new(),
            pick_files: Vec::new(),
            picked: Vec::new(),
            name_taken: false,
            pick_truncated: false,
            pick_top: 0,
            pick_unreadable: false,
            anyway: false,
            chosen: None,
            mdns: false,
            naming: false,
            started: None,
            addresses: Vec::new(),
            joined: Vec::new(),
            joined_at: None,
            names: net::NameCache::default(),
            tick: Vec::new(),
            receive_dir: crate::page::default_receive_dir(),
            picking_receive: false,
            band5: false,
            tick_dir: String::new(),
            tick_first: 0,
            tick_confirm: None,
            tick_said: String::new(),
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
            files_at_once: 4,
            downloading: None,
            batch: Arc::new(Mutex::new(None)),
            result: Arc::new(Mutex::new(None)),
            since: None,
        }
    }

    /// Forget the choice, so the next transfer cannot inherit it.
    ///
    /// A tick that survived from an earlier transfer became the whole of a
    /// later one, and sent a photograph of a driving licence to a laptop that
    /// was meant to be getting a folder of lessons. Ticks surviving while a
    /// person walks around the picker is the behaviour that was asked for;
    /// surviving a completed transfer is not, and the difference is the whole
    /// bug.
    fn forget_session(&mut self) {
        self.picked.clear();
        self.chosen = None;
        self.tick.clear();
        self.cable_note.clear();
        // Actually stop them. Dropping cable_watch only dropped the screen's
        // copy of a status line: the flag the beacon, the .local answerer, the
        // address handout and the name server all wait on was never raised,
        // so every one of them ran on after Stop until the program closed,
        // answering gorilla.local with the old address. A second start then
        // added a second set beside the first.
        self.cable_stop.store(true, std::sync::atomic::Ordering::Relaxed);
        serve::halt();
        self.cable_watch = None;
        self.naming = false;
        self.mdns = false;
        self.name_taken = false;
        self.mdns_live = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.taken_live = Arc::new(std::sync::atomic::AtomicBool::new(false));
        serve::forget_session();
    }

    fn shutdown(&mut self) {
        if let Some(h) = self.hotspot.take() {
            h.down();
            h.disarm_restore();
            println!("The wifi has been put back the way it was.");
        }
        crate::fetch::cancel();
        // On the way out too: names, notes and choices should not outlive the
        // program in a crash dump or a core file.
        self.forget_session();
    }
}

// ---------------------------------------------------------------- drawing

impl App {
    fn draw(&mut self, rows: usize, cols: usize) {
        // The supervisor's answer can change between frames: that is the whole
        // point of it. Read it here rather than remembering what it said when
        // serving began.
        if let Some(w) = &self.cable_watch {
            let now = w.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if now != self.cable_note {
                self.cable_note = now;
            }
        }
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
        // A message's scroll position, kept inside the text for this window.
        if let Screen::Note(text) = &self.screen {
            let max = note_lines(text, cols).len().saturating_sub(Self::note_room(rows));
            self.row = self.row.min(max);
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
            Screen::Pick => self.draw_pick(&mut f),
            Screen::Checkup => self.draw_checkup(&mut f),
            Screen::FixOffer => self.draw_fixoffer(&mut f),
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

    /// Look at the Windows services again, off the drawing thread. Half a
    /// second on the development laptop; an old one may take a few.
    fn refresh_services(&self) {
        if !cfg!(windows) {
            return;
        }
        let slot = Arc::clone(&self.services);
        *slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
        std::thread::spawn(move || {
            let found = crate::services::check();
            *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(found);
        });
    }

    /// Names of what is switched off. None: still looking, or not Windows.
    fn services_off(&self) -> Option<Vec<String>> {
        let guard = self.services.lock().unwrap_or_else(|e| e.into_inner());
        let found = guard.as_ref()?;
        Some(found.iter().filter(|f| f.switched_off()).map(|f| f.label.clone()).collect())
    }

    fn draw_home(&self, f: &mut Frame) {
        // The version belongs on this screen, not only behind --version.
        //
        // The menu is the way in for most people, and it was the one screen
        // that never said which build it was. A teacher on the phone, or
        // anyone reporting that something does not work, has the program open
        // in front of them and no way to answer "which version". Telling them
        // to quit and run `hub --version` is asking them to leave the only
        // thing they know how to use.
        //
        // It goes on the title line rather than a line of its own: rows here
        // are a fixed budget shared with the content, and a heading that
        // already exists costs nothing to extend.
        self.title(
            f,
            &format!("Gorilla Portable Network Hub {}{}", env!("CARGO_PKG_VERSION"), crate::built()),
        );
        let items: Vec<String> = [
            "Hand out files to the class over wifi",
            "Send files down a cable to one other computer",
            "Get files from another computer",
            "Fix problems with this computer (wifi, cable, firewall)",
        ]
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
        // What the highlighted choice does, before anyone presses enter on it.
        // The four lines name the choices; they cannot also say what each one
        // is FOR, or what it needs (a cable, another computer running the
        // hub). Two lines, for the one under the bar only, so the screen does
        // not become a manual.
        let what: [&str; 2] = match self.row {
            0 => [
                "  Makes a wifi network from this laptop. Phones and laptops join it,",
                "  get the files you choose, and can send work back. No internet needed.",
            ],
            1 => [
                "  For one other computer joined to this one by a network cable.",
                "  The fastest way to move a lot at once, like a folder of videos.",
            ],
            2 => [
                "  Takes files from another computer that is running this hub,",
                "  on the same wifi or over a cable.",
            ],
            _ if cfg!(windows) => [
                "  Checks the parts of Windows the hub needs, the firewall and the wifi",
                "  card, and switches back on whatever is off. One permission prompt.",
            ],
            _ => [
                "  Checks this computer's wifi, cable and firewall, and says what",
                "  to change if something would stop the hub working.",
            ],
        };
        for line in what {
            f.push(line);
        }
        f.blank();
        // Said on the FIRST screen, and not as a one-liner. A laptop that
        // has been "sped up" looks perfectly fine: nothing tells its owner
        // that the hotspot or the cable will fail, and the person who ran
        // the tweak script is the least likely to suspect it. So the check
        // is asked for plainly, and when it has found something, louder.
        match self.services_off() {
            Some(off) if !off.is_empty() => {
                f.push("  !! THIS COMPUTER IS NOT READY YET.");
                f.push("     Parts of Windows the hub needs are switched off here:");
                f.push(&format!("     {}.", off.join(", ")));
                f.push("     Choose \"Fix problems with this computer\" before anything else.");
                f.push("     It takes a few seconds. Windows will ask permission: say Yes.");
            }
            Some(_) => {
                f.push("  This computer is ready: everything the hub needs is switched on.");
                f.push_dim("  This works with no internet and no router.");
            }
            None if cfg!(windows) => {
                f.push_dim("  Checking whether this computer is ready...");
            }
            None => {
                f.push_dim("  This works with no internet and no router.");
                f.push_dim("  A cable is the fastest way to move a lot at once.");
            }
        }
        f.blank();
        // Full brightness unless this computer is already known to be fine:
        // grey text is text people skip, and this is the one they must not.
        let ready = self.services_off().is_some_and(|off| off.is_empty());
        for line in [
            "  On a new computer, run \"Fix problems with this computer\" once,",
            "  even if everything seems fine. Many laptops have had parts of",
            "  Windows switched off to \"speed them up\", by a tweak list, a script",
            "  or a friend. Nothing looks wrong until the wifi network or the cable",
            "  fails in front of the people waiting for the files.",
        ] {
            if ready {
                f.push_dim(line);
            } else {
                f.push(line);
            }
        }
        self.hints(f, "  up and down to choose    enter to open    q to quit");
    }

    /// The folder, and how much of it, in one line.
    ///
    /// Choosing four files out of a folder used to leave this row reading
    /// exactly as it read before: the same path, no count, nothing changed.
    /// Pressing the button and being returned to an identical screen is
    /// indistinguishable from pressing a button that does not work, and was
    /// reported as exactly that. The screen has to show what was decided or
    /// the deciding did not happen as far as anybody can tell.
    fn what_to_send(&self) -> String {
        match &self.chosen {
            Some(only) if only.len() == 1 => {
                // One file: name it. A count of one tells a person nothing
                // they did not already know, and the name confirms they
                // ticked the thing they meant to.
                let name = only.iter().next().map(String::as_str).unwrap_or("");
                let short = name.rsplit('/').next().unwrap_or(name);
                format!("{short}   (1 file, from {})", self.folder)
            }
            Some(only) if self.pick_truncated => format!(
                "more than {} files from {}, and only the first {} will be sent",
                only.len(),
                self.folder,
                only.len()
            ),
            Some(only) => format!("{} files chosen from {}", only.len(), self.folder),
            None => self.folder.clone(),
        }
    }

    fn send_fields(&self) -> Vec<(String, String)> {
        // Down a cable there is no network to name, no password to set and no
        // channel to pick: the cable IS the network. Showing those three rows
        // greyed out would be three more things to read and understand before
        // getting to the one row that matters.
        if self.cable {
            return vec![
                ("What to send".into(), self.what_to_send()),
                (
                    // Named for what it does for the person, not for the
                    // protocol it does it with. "Give out addresses" is a
                    // true description of the mechanism and tells somebody
                    // nothing about whether they want it.
                    "Set up the other computer".into(),
                    if self.anyway {
                        "yes, even on this network. CAN BREAK IT.".into()
                    } else {
                        "yes, when this computer is on nothing else".into()
                    },
                ),
                ("Connections to serve at once".into(), self.helpers.to_string()),
                ("Received files go to".into(), self.receive_dir.display().to_string()),
            ];
        }
        vec![
            ("Folder to hand out".into(), self.what_to_send()),
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
            // NOT "devices at once", which is what this said until 0.8.0 and
            // was wrong in a way that mattered. These are worker threads, and
            // ONE device holds several at a time: a browser opens about six,
            // and this tool's own downloads use four. Thirty phones is nearer
            // 180 connections than 30, so a teacher reading "64 devices" and
            // counting heads was reassured by the wrong number.
            if cfg!(windows) {
                (
                    "Wifi band".into(),
                    if self.band5 {
                        "5 GHz (faster; some phones will not see it)".into()
                    } else {
                        "2.4 GHz (every phone sees it)".into()
                    },
                )
            } else { (
                "Wifi channel".into(),
                if self.ssid.is_empty() {
                    // Not a verdict, an instruction. This first read "not ours
                    // to choose on somebody else's network", which the owner
                    // read as the tool refusing channel control outright,
                    // while standing one row away from creating his own
                    // network. Say the one thing to do, not the reason the
                    // row is asleep.
                    "name a network above first, then a lane can be chosen".into()
                } else if self.channel.is_empty() {
                    "picked automatically".into()
                } else {
                    self.channel.clone()
                },
            ) },
            ("Connections to serve at once".into(), self.helpers.to_string()),
            ("Received files go to".into(), self.receive_dir.display().to_string())
        ]
    }

    fn draw_send(&self, f: &mut Frame) {
        self.title(
            f,
            if self.cable { "Send files down a cable" } else { "Hand out files" },
        );
        let fields = self.send_fields();
        // Wide enough for the longest label. It was 22, which "Connections to
        // serve at once" overflows, and an overflowing label pushes its value
        // out of the column so the form stops reading as a form.
        let label_width = fields.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(22) + 2;
        // The highlight width covers the form AND the button below it, so the
        // bar does not change size as the selection moves down onto Start.
        let mut plain: Vec<String> = fields
            .iter()
            .map(|(l, v)| format!("  {l:<label_width$}{v}"))
            .collect();
        plain.push("  Start handing out".into());
        let w = term::group_width(&plain);
        for (i, (label, value)) in fields.iter().enumerate() {
            let editing_here = self.row == i && self.editing.is_some();
            let shown = if let (true, Some(buf)) = (self.row == i, &self.editing) {
                // A reverse-video space is the cursor. The real cursor is
                // hidden because it flickers across a full redraw.
                format!("{buf}\x1b[7m \x1b[0m")
            } else {
                value.clone()
            };
            let line = format!("  {label:<label_width$}{shown}");
            if self.row == i && self.editing.is_none() {
                f.push_selected_within(&line, w);
            } else {
                f.push(&line);
            }
            // Say, on the row itself, that this field is now taking typing.
            //
            // Pressing enter used to REMOVE the selection bar and add a
            // one-character block at the end of a path sixty characters long.
            // The only other sign was a hint at the very bottom of the screen.
            // On a full-screen window that is far from where the eye is, so
            // the field looked deselected and the reasonable conclusion was
            // that it could not be edited at all. Reported as exactly that.
            if editing_here {
                f.push_dim("      now typing. enter keeps it, esc leaves it alone");
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
        // What the middle row is for, while somebody is standing on it.
        //
        // The screen used to state the danger and never the purpose, so the
        // row read as a warning with no reason to exist. Asked directly:
        // "what's this supposed to do... the giveout adrrsses".
        if self.cable && self.row == 1 && self.editing.is_none() {
            f.push_dim("  Before two computers can talk, each needs an address. Windows and");
            f.push_dim("  Mac give themselves one after about half a minute. Most Linux");
            f.push_dim("  computers wait for one forever and never come up at all.");
            f.blank();
            f.push_dim("  Left on, this hands the other computer an address so nobody has to");
            f.push_dim("  touch its network settings. That is the normal setting and it only");
            f.push_dim("  acts when this computer is on nothing but the cable.");
            f.blank();
        }
        if cfg!(windows) && !self.cable && self.row == 3 && self.editing.is_none() {
            f.push_dim("  Enter switches between 2.4 GHz and 5 GHz. 2.4 GHz reaches every");
            f.push_dim("  phone and goes through walls better; 5 GHz is faster, but some");
            f.push_dim("  phones cannot see it. If phones cannot find the network, use 2.4.");
            f.push_dim("  Windows picks the channel itself; the screen shows which one.");
        }
        // The rows that had no explanation at all: the folder, the name, the
        // password and the button. A first-time teacher stands on each of
        // them wondering what it wants.
        if self.editing.is_none() {
            let why: &[&str] = match (self.cable, self.row) {
                (_, 0) => &[
                    "  The folder with the lesson's files; a USB drive is fine. Press",
                    "  enter to find it. After Start you tick which files the class sees.",
                ],
                (false, 1) => &[
                    "  The name phones see in their wifi list. Anything you like; the",
                    "  class looks for this name.",
                ],
                (false, 2) => &[
                    "  At least 8 letters or numbers. The class does not have to type it:",
                    "  they scan a code. Write it on the board for phones that cannot scan.",
                ],
                (false, r) if r == fields.len() => &[
                    "  Next you tick which files the class may see. Then the wifi network",
                    "  switches on and two codes appear for the class to scan.",
                ],
                (true, r) if r == fields.len() => &[
                    "  Next you tick which files to send. Plug the cable in first.",
                ],
                _ => &[],
            };
            for line in why {
                f.push_dim(line);
            }
        }
        if self.row == fields.len() - 1 && self.editing.is_none() {
            f.push_dim("  Everything people send you lands in this one folder. Press");
            f.push_dim("  enter to choose another. On the next screen, o opens it.");
        }
        if self.row == fields.len() - 2 && self.editing.is_none() {
            // Only while the teacher is on that row, so the screen is not
            // carrying an explanation nobody is reading.
            f.push_dim("  One device holds several connections at a time: a phone's browser");
            f.push_dim("  opens about six, and this tool's own downloads use four. Thirty");
            f.push_dim("  phones is nearer 180 connections than 30.");
            f.blank();
        }
        // Linux only: there the hub sets the channel. On Windows this row is
        // the band, and Windows picks the channel.
        if !cfg!(windows) && !self.cable && self.row == 3 && !self.ssid.is_empty() {
            let allowed = net::allowed_channels();
            f.push_dim("  Channels are lanes on the same road. If the room is slow, another");
            if allowed.is_empty() {
                f.push_dim("  lane often fixes it: try 11 or 13.");
            } else {
                f.push_dim(&format!(
                    "  lane often fixes it. This radio, here, may use: {}.",
                    net::channel_ranges(allowed)
                ));
            }
            f.push_dim("  Some phones with American radios cannot see 12 or 13 at all; if a");
            f.push_dim("  device cannot find the network, use 11 or lower.");
            f.blank();
        }
        if self.cable {
            // The wifi advice below is not merely unhelpful here, it is about
            // a field this mode does not have, so it sends a person looking
            // for a row that is not on the screen.
            if self.anyway {
                f.push_dim("  WARNING. Addresses will be given out on every network this");
                f.push_dim("  computer is connected to, not only the cable. On a network");
                f.push_dim("  that has a router this can take it down for everyone on it.");
                f.push_dim("  Turn this back off unless you know why you turned it on.");
            } else if self.row != 1 {
                f.push_dim("  Plug a network cable between the two computers, then wait");
                f.push_dim("  half a minute before starting. No wifi and no router needed.");
            }
        } else if self.ssid.is_empty() {
            f.push_dim("  Leave the network name empty if the class is already");
            f.push_dim("  on the same wifi as this computer.");
        } else if cfg!(windows) {
            f.push_dim("  The hub switches this laptop's wifi network on by itself,");
            f.push_dim("  and off again when you stop. Nothing to set up in Windows.");
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
        // What the marks mean, said once at the top: [~] is not a symbol
        // anybody outside a file manager has been taught.
        f.push("  Tick what the class may see; only ticked files reach the phones.");
        f.push_dim("  [x] handed out    [ ] not handed out    [~] some of the files inside");
        if self.tick_dir.is_empty() {
            f.push_dim("  In: the folder you chose (top level)");
        } else {
            f.push_dim(&format!("  In: {}/", self.tick_dir));
        }
        f.blank();
        if self.tick.is_empty() {
            f.push("  The folder has no files in it.");
        }

        let fixed = self.tick_fixed();
        let ticked = self.tick.iter().filter(|(_, _, t)| *t).count();
        let action = if !pre {
            "  BACK TO THE LESSON".to_string()
        } else {
            format!("  CONTINUE: HAND OUT THE {} TICKED", count(ticked))
        };
        let mut head: Vec<String> = vec![action];
        if fixed == 2 {
            head.push("  ..  go up one folder".to_string());
        }
        let hw = term::group_width(&head);
        for (i, h) in head.iter().enumerate() {
            if self.row == i {
                f.push_selected_within(h, hw);
            } else {
                f.push(h);
            }
        }

        let entries = self.tick_entries();
        let cells: Vec<String> = entries.iter().map(|e| self.tick_cell(e)).collect();
        let (rows, cols) = (f.rows, f.cols);
        let g = self.tick_grid(&cells, rows, cols);
        let sel = self.row.checked_sub(fixed);
        for line in g.lines(&cells, sel) {
            f.push_raw(&line);
        }
        f.blank();

        if let Some(what) = &self.tick_confirm {
            let (files, bytes, name) = self.tick_confirm_size(what);
            f.push(&format!(
                "  !! Space again ticks ALL {} files {} ({}),",
                count(files), name, human(bytes)
            ));
            f.push("     every folder inside included. Some may be files you did not know");
            f.push("     were there, and the class will be able to see every one of them.");
            f.push("     Space again: tick them all.   Enter: go inside and choose instead.");
        } else if !self.tick_said.is_empty() {
            f.push_dim(&format!("  {}", self.tick_said));
        }
        if let Some(s) = g.status() {
            f.push_dim(&format!("  {s}"));
        }
        let bytes: u64 = self.tick.iter().filter(|(_, _, t)| *t).map(|(_, s, _)| *s).sum();
        f.push(&format!(
            "  {} of {} files will be visible to the class ({}).",
            count(ticked), count(self.tick.len()), human(bytes)
        ));
        if !pre {
            f.push_dim("  Ticking a file hands it out NOW; unticking withdraws it.");
        }
        self.hints(f, "  space ticks   enter opens   a all here   n none here   esc back");
    }

    /// Rows above the list: the action, and ".." inside a folder.
    fn tick_fixed(&self) -> usize {
        if self.tick_dir.is_empty() { 1 } else { 2 }
    }

    /// What is in the folder being looked at: its subfolders, each with how
    /// many files are in it at any depth and how many of those are ticked,
    /// then its own files. Built from the flat list, which stays the one
    /// record of what is ticked.
    fn tick_entries(&self) -> Vec<TickEntry> {
        let prefix = if self.tick_dir.is_empty() { String::new() } else { format!("{}/", self.tick_dir) };
        let mut folders: std::collections::BTreeMap<String, (String, usize, usize, u64)> =
            std::collections::BTreeMap::new();
        let mut files: Vec<usize> = Vec::new();
        for (i, (rel, size, t)) in self.tick.iter().enumerate() {
            let Some(rest) = rel.strip_prefix(&prefix) else { continue };
            match rest.split_once('/') {
                Some((dir, _)) => {
                    let e = folders.entry(dir.to_lowercase()).or_insert((dir.to_string(), 0, 0, 0));
                    e.1 += 1;
                    e.2 += usize::from(*t);
                    e.3 += *size;
                }
                None => files.push(i),
            }
        }
        files.sort_by_key(|i| self.tick[*i].0.to_lowercase());
        let mut out: Vec<TickEntry> = folders
            .into_values()
            .map(|(name, files, ticked, bytes)| TickEntry::Folder { name, files, ticked, bytes })
            .collect();
        out.extend(files.into_iter().map(TickEntry::File));
        out
    }

    fn tick_cell(&self, e: &TickEntry) -> String {
        match e {
            TickEntry::Folder { name, files, ticked, .. } => {
                let m = if *ticked == 0 { " " } else if ticked == files { "x" } else { "~" };
                let word = if *files == 1 { "file" } else { "files" };
                format!("[{m}] {name}/  ({} {word})", count(*files))
            }
            TickEntry::File(i) => {
                let (rel, size, t) = &self.tick[*i];
                let name = rel.rsplit('/').next().unwrap_or(rel);
                format!("[{}] {name}  {}", if *t { "x" } else { " " }, human(*size))
            }
        }
    }

    /// The list's layout for a window `rows` by `cols`, following the cursor.
    /// The same numbers for drawing and for moving, so a key never moves to a
    /// place the screen did not show.
    fn tick_grid(&self, cells: &[String], rows: usize, cols: usize) -> crate::grid::Grid {
        // Title 2, what the marks mean 2, "In:" 2, fixed rows, and below:
        // blank, up to 4 warning lines, status, count, the mid-lesson line and
        // the hint row.
        let room = rows.saturating_sub(6 + self.tick_fixed() + 10).max(3);
        let widest = cells.iter().map(|c| term::width(c)).max().unwrap_or(10);
        let sel = self.row.saturating_sub(self.tick_fixed());
        crate::grid::Grid::lay(cells.len(), widest, cols, room, sel, self.tick_first)
    }

    /// Files and bytes a pending confirmation would tick, and how to name it.
    fn tick_confirm_size(&self, what: &str) -> (usize, u64, String) {
        let (prefix, name) = match what.strip_prefix('\u{0}') {
            // "all here": everything under the folder being looked at.
            Some(dir) => (
                if dir.is_empty() { String::new() } else { format!("{dir}/") },
                if dir.is_empty() { "in everything you chose".to_string() } else { format!("inside {dir}/") },
            ),
            None => (format!("{what}/"), format!("inside {}/", what.rsplit('/').next().unwrap_or(what))),
        };
        let mut files = 0;
        let mut bytes = 0;
        for (rel, size, _) in &self.tick {
            if rel.starts_with(&prefix) {
                files += 1;
                bytes += *size;
            }
        }
        (files, bytes, name)
    }

    /// Tick or untick everything whose path starts with `prefix`.
    fn tick_under(&mut self, prefix: &str, on: bool) -> usize {
        let mut n = 0;
        for e in &mut self.tick {
            if e.0.starts_with(prefix) {
                e.2 = on;
                n += 1;
            }
        }
        n
    }

    fn draw_waiting(&self, f: &mut Frame) {
        self.title(f, "Work waiting for you");
        f.push_dim(&format!(
            "  Accepted work goes to   {}",
            crate::page::handed_in_dir(&PathBuf::from(shellexpand(&self.folder))).display()
        ));
        f.blank();
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
            .map(|p| {
                // The whole window, not 28 and 24 characters: the name of the
                // work was cut to "Screenshot_2026-09-23-1~" five times over.
                let room = f.cols.saturating_sub(16).max(40);
                let who = room * 2 / 5;
                let what = room - who;
                format!(
                    "  {:<who$}{:<what$}{:>9}",
                    term::truncate(&p.from, who.saturating_sub(2)),
                    term::truncate_middle(&p.original, what.saturating_sub(2)),
                    human(p.bytes),
                )
            })
            .collect();
        let w = term::group_width(&lines);
        let room = f.rows.saturating_sub(f.used() + 7).max(1);
        // The page follows the cursor. This used to draw the first rows only,
        // so work beyond the bottom of the window could be selected and
        // accepted or refused without ever being seen.
        let top = self.row.saturating_sub(room - 1).min(lines.len().saturating_sub(room));
        for (i, line) in lines.iter().enumerate().skip(top).take(room) {
            if i == self.row {
                f.push_selected_within(line, w);
            } else {
                f.push(line);
            }
        }
        if lines.len() > room {
            f.push_dim(&format!(
                "  Showing {} to {} of {}. Press the down arrow to see more.",
                top + 1,
                (top + room).min(lines.len()),
                lines.len()
            ));
        }
        f.blank();
        if let Some(p) = items.get(self.row) {
            f.push_dim(&format!("  sent {}", p.at));
        }
        f.blank();
        // Every key said with what it does to the work, because "refuse"
        // sounds like "delete" and nobody would press it if it did.
        f.push(&format!(
            "  {} waiting. Nothing is kept until you accept it.",
            items.len()
        ));
        f.push("  o looks at it first   a accepts it   e accepts ALL   p all from them");
        f.push("  r refuses it: moved aside, never deleted. Accepted work goes above.");
        self.hints(f, "  o look  a accept  e ALL  p all from them  r refuse  esc back");
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
    /// The address a phone should open, chosen the same way the screen chooses
    /// which one to print. On a cable that is the link-local one, because the
    /// wifi address is the one the other machine cannot reach.
    /// The addresses to give people. Our own network's address when we made
    /// one: this laptop is often on another network as well (a phone's, on
    /// the test day), and that address is one nobody on the hotspot can reach.
    fn addresses_to_give(&self) -> Vec<std::net::Ipv4Addr> {
        // Our network, or nothing. Falling back to every address when the
        // hotspot was down put the phone network's address on the screen
        // beside the real one, on the test day, with no way to tell which.
        if let Some(h) = &self.hotspot {
            return h.address().into_iter().collect();
        }
        if self.cable {
            let ll: Vec<_> = self.addresses.iter().copied().filter(|a| a.is_link_local()).collect();
            if ll.is_empty() { self.addresses.clone() } else { ll }
        } else {
            self.addresses.clone()
        }
    }

    fn page_url(&self) -> Option<String> {
        if let Some(a) = self.addresses_to_give().first() {
            return Some(if crate::serve::on_port_80() {
                format!("http://{a}")
            } else {
                format!("http://{a}:{}", port())
            });
        }
        let show: Vec<std::net::Ipv4Addr> = if self.cable {
            let ll: Vec<_> = self.addresses.iter().copied().filter(|a| a.is_link_local()).collect();
            if ll.is_empty() { self.addresses.clone() } else { ll }
        } else {
            self.addresses.clone()
        };
        let a = show.first()?;
        Some(if crate::serve::on_port_80() {
            format!("http://{a}")
        } else {
            format!("http://{a}:{}", port())
        })
    }

    /// "1. Scan to join the wifi" and "2. Then scan to open the page", drawn
    /// side by side if they fit in what is left of the window.
    fn draw_scan_codes(&self, f: &mut Frame, ssid: &str) {
        // Two modules of border, not the standard's four: on the test day the
        // codes were too big to fit a phone's camera frame without stepping
        // well back. The terminal around them is dark, and phone cameras read
        // a two-module border off a screen without trouble.
        const QUIET: usize = 2;
        let join = crate::qr::wifi_join(ssid, &self.password);
        let page = self.page_url().and_then(|u| crate::qr::encode(u.as_bytes()));
        let (Some(join), Some(page)) = (join, page) else { return };
        let (jw, jh) = crate::qr::rendered_size(&join, QUIET);
        let (pw, ph) = crate::qr::rendered_size(&page, QUIET);
        let gap = 6;
        let need_cols = 2 + jw + gap + pw;
        // Headings, the codes, and a line under them; the hint row below.
        let room = f.rows.saturating_sub(f.used() + 5);
        f.blank();
        if need_cols > f.cols || jh.max(ph) > room {
            f.push("  PHONES: press j to show a code they can scan to join and open the page.");
            f.push_dim("  (The window is too small to show it here. Making it bigger shows it.)");
            return;
        }
        let head_a = "1. Scan to join the wifi";
        let head_b = "2. Then scan to open the page";
        f.push(&format!("  {head_a:<w$}{}{head_b}", " ".repeat(gap), w = jw));
        let a = crate::qr::render(&join, QUIET);
        let bl = crate::qr::render(&page, QUIET);
        for i in 0..a.len().max(bl.len()) {
            let left = a.get(i).cloned().unwrap_or_else(|| " ".repeat(jw));
            let right = bl.get(i).cloned().unwrap_or_default();
            f.push_raw(&format!("  {left}{}{right}", " ".repeat(gap)));
        }
        f.push_dim("  Point the phone's camera at the code and tap what appears.");
    }

    fn draw_joincode(&self, f: &mut Frame) {
        let Some(h) = &self.hotspot else {
            // No hotspot means a cable, or a network somebody else set up. In
            // both cases there is no password to encode, and this screen used
            // to stop there and say so, which left the one group who most need
            // a camera, phones and tablets, with an address to read off a
            // terminal and type in by hand.
            //
            // There is still something worth putting in a code: the page
            // itself. A phone points at it and the page opens, which is the
            // whole ask, and it needs no password because there is no network
            // to join.
            self.title(f, "Open by camera");
            const QUIET: usize = 4;
            let Some(url) = self.page_url() else {
                f.push("  This computer has no address yet, so there is nothing");
                f.push("  to put in a code. Wait half a minute and look again.");
                self.hints(f, "  esc to go back");
                return;
            };
            f.push(&format!("  Point a camera at this to open   {url}"));
            f.blank();
            let Some(code) = crate::qr::encode(url.as_bytes()) else {
                f.push("  That address is too long to fit in a code.");
                f.push(&format!("  Everything still works; open {url} by hand."));
                self.hints(f, "  esc to go back");
                return;
            };
            let (cols, rows) = crate::qr::rendered_size(&code, QUIET);
            let room = f.rows.saturating_sub(f.used() + 1);
            if cols + 2 > f.cols || rows > room {
                f.push("  The window is too small to draw the code.");
                f.push(&format!("  It needs {} rows and {} columns; this window has {} by {}.", rows + 3, cols + 2, f.rows, f.cols));
                f.push(&format!("  Make it bigger, or open {url} by hand."));
                self.hints(f, "  esc to go back");
                return;
            }
            for line in crate::qr::render(&code, QUIET) {
                f.push_raw(&format!("  {line}"));
            }
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
        self.title(
            f,
            if self.cable { "Sending down the cable" } else { "Handing out files" },
        );
        if let Some(h) = &self.hotspot {
            f.push(&format!("  Wifi network      {}", h.ssid));
            f.push(&format!("  Password          {}", self.password));
            if let Some(ch) = net::hotspot_channel() {
                f.push(&format!("  Broadcasting on   {}", net::describe_channel(ch)));
            } else if cfg!(windows) {
                f.push_dim("  Broadcasting on   (measuring...)");
            }
            if let Some(problem) = net::hotspot_problem() {
                f.blank();
                let measure = f.cols.saturating_sub(6).min(72).max(24);
                for line in wrap(&format!("!! {problem}"), measure) {
                    f.push(&format!("  {line}"));
                }
                f.blank();
            }
        }
        let port80 = serve::on_port_80();
        // On a cable, the cable's address is the one that matters and any
        // other is a distraction. This screen listed the wifi address first,
        // on a cable transfer, which is the address the other laptop cannot
        // reach: the one thing on the screen a person is meant to read out,
        // and it was the wrong one.
        let show = self.addresses_to_give();
        if show.is_empty() && self.hotspot.is_some() {
            f.push("  Address to type   (waiting for the wifi network to come up)");
        }
        for a in &show {
            if port80 {
                f.push(&format!("  Address to type   http://{a}"));
            } else {
                f.push(&format!("  Address to type   http://{a}:{}", port()));
            }
        }
        // Whether the things that let the other end find this without being
        // told anything are actually running. Said here because this is the
        // screen a person is looking at while wondering why nothing has
        // appeared on the other laptop. The .local line is on the hotspot too
        // since 2026-09-23: before that nothing answered the name over wifi,
        // so the only thing that could was a stale copy somewhere else.
        {
            use std::sync::atomic::Ordering::Relaxed;
            let mdns = self.mdns || self.mdns_live.load(Relaxed);
            let name_taken = self.name_taken || self.taken_live.load(Relaxed);
            if mdns && name_taken {
                // Say nothing encouraging about a name that will not reach us.
                //
                // Two machines both running this both answer to gorilla.local
                // and each resolves it to ITSELF, so a person following the
                // old line landed on their own computer and saw a working page
                // with the wrong files on it. Nothing failed, which is what
                // made it hard to notice.
                f.push_dim("  gorilla.local will NOT reach this computer: another");
                f.push_dim("  computer here already answers to it. Use the address,");
                f.push_dim("  or pick this machine by name on the other one.");
            } else if mdns {
                if port80 {
                    f.push("  Or they can type   gorilla.local");
                } else {
                    f.push(&format!("  Or they can type   gorilla.local:{}", port()));
                }
            }
            if self.naming {
                f.push("  Or just            gorilla/");
            }
            // Wrapped at a readable measure, not at the window width.
            //
            // push() truncates at the terminal's own width, so on a window
            // stretched across a wide screen this sentence ran the whole way
            // across in one line and read as broken. Prose needs a measure a
            // person can follow back to the start of the next line, and that
            // is not "however wide somebody dragged the window".
            let measure = f.cols.saturating_sub(22).min(64).max(24);
            let note = if self.cable { self.cable_note.as_str() } else { "" };
            // wrap("") still gives one empty line, which drew a label with
            // nothing after it on the wifi screen.
            let chunks = if note.is_empty() { Vec::new() } else { wrap(note, measure) };
            for (i, chunk) in chunks.into_iter().enumerate() {
                if i == 0 {
                    f.push_dim(&format!("  addresses         {chunk}"));
                } else {
                    f.push_dim(&format!("                    {chunk}"));
                }
            }
        }
        if self.hotspot.is_some() && port80 && cfg!(target_os = "linux") {
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
        // Scan, do not type. The address is a string nobody in the room can
        // type, on phones whose owners have never looked for a slash. So when
        // this computer made the network, the two codes that do everything go
        // right here, not behind a key: one joins the wifi (password and all),
        // one opens the page. Side by side when the window is wide enough,
        // otherwise one loud line saying which key shows them.
        if let Some(h) = &self.hotspot {
            self.draw_scan_codes(f, &h.ssid.clone());
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
        if serve::files_truncated() {
            f.push("  THAT FOLDER HAS MORE FILES THAN THIS CAN HAND OUT.");
            f.push("  Some of them are not being offered. Point it at a smaller folder.");
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
            if self.cable {
                f.push_dim("  On the other computer: its own screen should offer to open");
                f.push_dim("  this page. If it does not, open any browser there and type");
                f.push_dim("  the address above.");
                f.push_dim("  Give it half a minute after the cable goes in. Nothing can");
                f.push_dim("  happen until both ends have settled on an address.");
            } else if cfg!(windows) && self.hotspot.is_some() {
                // Windows has no sign-in page to pop, so do not promise one.
                f.push_dim("  Phones: switch WIFI ON (mobile data can stay off) and scan code 1.");
                f.push_dim("  The page opens by itself; if it does not, scan code 2.");
                f.push_dim("  Laptops: join the wifi network above; the page opens by itself,");
                f.push_dim("  or type gorilla.local. This laptop's own wifi must stay on.");
            } else {
                f.push_dim("  On a phone or any computer: join the wifi and the sign-in");
                f.push_dim("  screen brings them here by itself. Or open a browser at the");
                f.push_dim("  address above.");
            }
        }
        let waiting = serve::pending_count();
        if waiting > 0 {
            f.push(&format!(
                "  {waiting} PIECE{} OF WORK WAITING FOR YOU. Press w to look.",
                if waiting == 1 { "" } else { "S" }
            ));
            f.push_dim("  Accepted work goes into the folder below.");
            f.blank();
        }
        if !self.cable {
            f.push(&format!("  Received files go to   {}", self.receive_dir.display()));
            f.push_dim("  Press o to open that folder.");
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
        // h first: the one key that explains all the others. The rest are
        // named by what they are for ("message", "work"), not by the
        // program's own words for them ("notice", "waiting").
        self.hints(f, "  h HELP  f files  n message  w work  o received  c who  j code  q stop");
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
                for (ip, count, who) in list {
                    // A NAME, and the address only when there is no name.
                    //
                    // This listed "169.254.87.1    4 files". Nothing has to be
                    // typed to choose it, but a teacher still has to recognise
                    // which machine four numbers and three dots refer to, and
                    // in a room with two of them there is no way to tell. The
                    // name already travels: the beacon carries it and the page
                    // says it, so this was four numbers standing in front of
                    // an answer the program already had.
                    //
                    // An older copy on the far end answers with no name, and
                    // then the address is shown exactly as before.
                    let label = if who.is_empty() { ip.to_string() } else { who.clone() };
                    let line = format!("  {label}    {count} file{}", if *count == 1 { "" } else { "s" });
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
        let settings: [(&str, String); 3] = [
            ("Save into", self.save_into.clone()),
            ("Connections per file", self.at_once.to_string()),
            ("Files at the same time", self.files_at_once.to_string()),
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
        let room = f.rows.saturating_sub(f.used() + 6);
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
        // The three settings, said in words, only while standing on one.
        // "Connections per file" is the mechanism's name; what a person needs
        // to know is that leaving it alone is the right answer.
        if self.editing.is_none() && self.tab_hint.is_none() {
            f.blank();
            if self.row < settings.len() {
                f.push_dim(match self.row {
                    0 => "  The folder on THIS computer the files are saved into. Enter to change it.",
                    1 => "  How many pieces of one file come at once. Leave it unless told otherwise.",
                    _ => "  How many files come at once. Leave it unless told otherwise.",
                });
            } else {
                f.push_dim("  Enter gets the file under the bar; a gets them all. A download that");
                f.push_dim("  breaks off carries on from where it stopped the next time.");
            }
        }
        if self.editing.is_some() {
            self.hints(f, "  type to change    tab completes a path    enter to keep it    esc to leave it");
        } else {
            self.hints(f, "  enter to get one file    a to get ALL of them    esc to go back");
        }
    }

    /// One width for the settings and the file list on that screen, so the
    /// highlight does not jump in size between them.
    fn files_width(&self) -> usize {
        let mut lines: Vec<String> = vec![
            format!("  {:<16}{}", "Save into", self.save_into),
            format!("  {:<22}{}", "Connections per file", self.at_once),
            format!("  {:<22}{}", "Files at the same time", self.files_at_once),
        ];
        lines.extend(self.files.iter().map(|e| format!("  {:<34}{:>10}", e.name, human(e.size))));
        term::group_width(&lines)
    }

    fn draw_receiving(&self, f: &mut Frame) {
        let batch = self.batch.lock().unwrap_or_else(|e| e.into_inner()).clone();
        self.title(f, if batch.is_some() { "Getting a folder" } else { "Getting a file" });
        if let Some(b) = &batch {
            // Two numbers, because they answer different questions: how far
            // through the folder, and how far through the file on screen now.
            let pct = if b.total > 0 { b.done as f64 / b.total as f64 } else { 0.0 };
            f.push(&format!("  file {} of {}", b.done.min(b.total), b.total));
            f.push(&format!("  {} {:>3}%", bar(pct, 34), (pct * 100.0) as u64));
            f.blank();
            for name in b.in_flight.iter().take(6) {
                f.push(&format!("  {}", term::truncate(name, f.cols.saturating_sub(4))));
            }
            let secs = self.since.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0).max(0.001);
            // Only bytes from FINISHED files. fetch's progress counters are
            // global to the process, so with several files in the air at once
            // they describe whichever one wrote to them last, which is not a
            // number worth showing anybody. Undercounting by whatever is
            // currently in flight is the honest error to make.
            let moved = b.bytes;
            f.push(&format!("  {} of this folder, {:.1} MB/s overall",
                            human(moved), moved as f64 / secs / 1e6));
            f.push_dim(&format!("  {} file{} at a time, {} connection{} inside each",
                                b.lanes, if b.lanes == 1 { "" } else { "s" },
                                self.at_once, if self.at_once == 1 { "" } else { "s" }));
            f.blank();
            if !b.failed.is_empty() {
                f.push(&format!("  {} file{} did not arrive:", b.failed.len(),
                                if b.failed.len() == 1 { "" } else { "s" }));
                for line in b.failed.iter().rev().take(3) {
                    f.push_dim(&format!("  {}", term::truncate(line, f.cols.saturating_sub(4))));
                }
                f.blank();
            }
            if b.finished {
                f.push(if b.failed.is_empty() {
                    "  Everything arrived. Press esc."
                } else {
                    "  Finished, with the failures listed above. Press esc."
                });
            }
            self.hints(f, "  q or esc to stop    it will carry on from here next time");
            return;
        }
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

    /// A message screen scrolls. It did not: a message taller than the window
    /// was cut off at the bottom, and the line saying how to leave went with
    /// it. The help page behind h (0.9.10) is about thirty lines, which is
    /// more than a small window has. self.row is the first line shown,
    /// clamped in draw() where the window's size is known.
    fn note_room(rows: usize) -> usize {
        // Title and its blank (2), the "more below" line and the hint row.
        rows.saturating_sub(4).max(1)
    }

    fn draw_note(&self, f: &mut Frame) {
        let Screen::Note(text) = &self.screen else { return };
        self.title(f, "");
        let lines = note_lines(text, f.cols);
        let room = Self::note_room(f.rows);
        let start = self.row.min(lines.len().saturating_sub(room));
        for line in lines.iter().skip(start).take(room) {
            f.push(line);
        }
        let long = lines.len() > room;
        if long && start + room < lines.len() {
            f.push_dim(&format!("  ...{} more lines below: press the down arrow", lines.len() - start - room));
        }
        self.hints(
            f,
            if long { "  up and down to read    enter or esc to go back" } else { "  enter or esc to go back" },
        );
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
            Screen::Pick => self.pick_key(k),
            Screen::Checkup => self.checkup_key(k),
            Screen::FixOffer => self.fixoffer_key(k),
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
                // Scrolling; draw() keeps the row inside the text.
                match k {
                    Key::Up => self.row = self.row.saturating_sub(1),
                    Key::Down => self.row += 1,
                    Key::PageUp => self.row = self.row.saturating_sub(10),
                    Key::PageDown => self.row += 10,
                    Key::Home => self.row = 0,
                    Key::End => self.row = usize::MAX / 2,
                    _ => {}
                }
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
            // Cable mode's form has two rows where wifi's has five, so the row
            // number means a different field and has to be read separately.
            Screen::Send if self.cable => match self.row {
                0 => self.folder = buf,
                // Row 1 is the addresses toggle and never opens an editor.
                2 => self.helpers = buf.trim().parse().unwrap_or(self.helpers).clamp(1, 512),
                _ => {}
            },
            Screen::Send => match self.row {
                0 => self.folder = buf,
                1 => {
                    self.ssid = buf.trim().to_string();
                    // The same name keeps the same password. A new random one
                    // every launch meant every laptop that had joined before
                    // tried its saved key first, failed, waited and retried:
                    // the "takes ages to connect" testers reported. A
                    // password somebody typed themselves is never replaced.
                    if !self.password_typed {
                        if let Some(saved) = net::saved_hotspot_password(&self.ssid) {
                            self.password = saved;
                        }
                    }
                }
                2 => {
                    self.password = buf.trim().to_string();
                    self.password_typed = true;
                }
                // Kept as text, validated at start: an empty field means "the
                // system chooses" and has to stay expressible.
                3 => self.channel = buf.trim().to_string(),
                // Clamped, not rejected. Typing a letter into a number field
                // should not throw the value away, and 100,000 helpers is a
                // typo rather than a wish.
                4 => self.helpers = buf.trim().parse().unwrap_or(self.helpers).clamp(1, 512),
                _ => {}
            },
            Screen::Receive => {
                self.typed_address = buf.trim().to_string();
                self.open_typed();
            }
            Screen::ReceiveFiles => match self.row {
                0 => self.save_into = buf,
                1 => self.at_once = buf.trim().parse().unwrap_or(self.at_once).clamp(1, 32),
                2 => self.files_at_once = buf.trim().parse().unwrap_or(self.files_at_once).clamp(1, 16),
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
        self.move_row(k, 4);
        match k {
            Key::Enter => {
                match self.row {
                    0 => {
                        self.cable = false;
                        self.screen = Screen::Send;
                    }
                    1 => {
                        // Cable mode clears the wifi rows rather than hiding
                        // them with values still set. A password left over
                        // from an earlier wifi session would otherwise be
                        // carried into a hotspot nobody asked for.
                        self.cable = true;
                        self.ssid.clear();
                        self.password.clear();
                        self.channel.clear();
                        self.screen = Screen::Send;
                    }
                    2 => {
                        self.start_looking();
                        self.screen = Screen::Receive;
                    }
                    _ => self.screen = Screen::Checkup,
                }
                // On the way into wifi or cable, with something switched off:
                // stop here and say so, with the fix one key away.
                if matches!(self.screen, Screen::Send)
                    && self.services_off().is_some_and(|off| !off.is_empty())
                {
                    self.screen = Screen::FixOffer;
                }
                self.row = 0;
            }
            Key::Char('q') | Key::Esc | Key::Quit => return true,
            _ => {}
        }
        false
    }

    /// Choosing what to send by looking at it, instead of typing a path.
    ///
    /// WHY THIS EXISTS, AND WHY IT TICKS.
    ///
    /// The folder row was a text field, so choosing what to send meant
    /// producing `C:\Users\someone\Desktop\Year 7` from memory. Backslashes and
    /// colons are not punctuation most people can generate on demand.
    ///
    /// Picking a whole folder was the first version of this and was not enough.
    /// A folder can hold a terabyte, and the review screen that follows lists
    /// every file in it flat, to a hundred thousand of them, all ticked. To
    /// send three files out of that a person unticks ninety-nine thousand nine
    /// hundred and ninety-seven. The default was exactly backwards for the case
    /// where somebody wants a few things.
    ///
    /// So this walks the tree at any depth and space ticks whatever is under
    /// the cursor: a file, or a folder meaning everything inside it. Ticks are
    /// held as absolute paths and survive moving between folders, so a person
    /// can take two files from here and a folder from three levels down.
    fn open_picker(&mut self) {
        let start = PathBuf::from(shellexpand(&self.folder));
        // Start somewhere real. A path typed earlier and since deleted, or a
        // drive no longer plugged in, must not open an empty screen with no
        // way out.
        let start = if start.is_dir() {
            start
        } else {
            start
                .ancestors()
                .find(|a| a.is_dir())
                .map(|a| a.to_path_buf())
                .unwrap_or_else(starting_folder)
        };
        // Ticks are NOT cleared on the way in.
        //
        // Choosing four files, going back to the form to check something, and
        // returning to change one of them used to throw all four away. The
        // picker is the place where a choice is made and unmade; leaving it is
        // not a decision to discard the choice. Only "SEND EVERYTHING IN THIS
        // FOLDER" clears it, because that is a person saying they want the
        // whole folder instead.
        self.pick_at(start);
        self.screen = Screen::Pick;
    }

    /// Read one folder: subfolders first, then files, each sorted.
    ///
    /// Folders first because the reason to be here is usually to go deeper,
    /// and a folder of ten thousand files should not bury the one subfolder
    /// somebody is looking for.
    fn pick_at(&mut self, dir: PathBuf) {
        let mut dirs: Vec<String> = Vec::new();
        let mut files: Vec<(String, u64)> = Vec::new();
        let mut unreadable = false;
        match std::fs::read_dir(&dir) {
            Ok(entries) => {
                for e in entries.flatten() {
                    let n = e.file_name().to_string_lossy().to_string();
                    // Hidden and system entries are noise here. .hub_holding
                    // is ours, and handing it out would hand the class back
                    // the work it just handed in.
                    if n.starts_with('.') || n.starts_with('$') {
                        continue;
                    }
                    match e.file_type() {
                        Ok(t) if t.is_dir() => dirs.push(n),
                        Ok(t) if t.is_file() => {
                            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                            files.push((n, size));
                        }
                        _ => {}
                    }
                }
            }
            Err(_) => unreadable = true,
        }
        dirs.sort_by_key(|n| n.to_lowercase());
        files.sort_by_key(|(n, _)| n.to_lowercase());
        self.pick_dir = dir;
        self.pick_kids = dirs;
        self.pick_files = files;
        self.pick_unreadable = unreadable;
        self.pick_top = 0;
        self.row = 0;
    }

    /// How many list rows fit, given the fixed furniture above and below.
    fn pick_page(&self, rows: usize) -> usize {
        // Three more lines of furniture while a folder is ticked: the note
        // that says a folder means everything inside it.
        let note = if self.picked.iter().any(|p| p.is_dir()) { 3 } else { 0 };
        rows.saturating_sub(11 + note).max(3)
    }

    /// The rows above the scrolling list: the action, and ".. go up one".
    fn pick_fixed(&self) -> usize {
        if self.pick_dir.parent().is_some() {
            2
        } else {
            1
        }
    }

    fn pick_is_ticked(&self, p: &Path) -> bool {
        self.picked.iter().any(|q| q == p)
    }

    /// Tick, idempotently. Pressing this twice is the same as pressing it
    /// once, which is what makes it safe to bind to the key people press by
    /// reflex.
    fn pick_add(&mut self, p: PathBuf) {
        if !self.picked.iter().any(|q| *q == p) {
            self.picked.push(p);
        }
    }

    fn pick_toggle(&mut self, p: PathBuf) {
        match self.picked.iter().position(|q| *q == p) {
            Some(i) => {
                self.picked.remove(i);
            }
            None => self.picked.push(p),
        }
    }

    /// The picker's list cells: subfolders, then files.
    fn pick_cells(&self) -> Vec<String> {
        // Choosing a place: folders only, and no boxes to tick.
        if self.picking_receive {
            return self.pick_kids.iter().map(|n| format!("{n}/")).collect();
        }
        let mut cells = Vec::with_capacity(self.pick_kids.len() + self.pick_files.len());
        for n in &self.pick_kids {
            let t = if self.pick_is_ticked(&self.pick_dir.join(n)) { "[x]" } else { "[ ]" };
            cells.push(format!("{t} {n}/"));
        }
        for (n, size) in &self.pick_files {
            let t = if self.pick_is_ticked(&self.pick_dir.join(n)) { "[x]" } else { "[ ]" };
            cells.push(format!("{t} {n}  {}", human(*size)));
        }
        cells
    }

    /// Layout for drawing and moving alike, so they always agree.
    fn pick_grid(&self, cells: &[String], rows: usize, cols: usize) -> crate::grid::Grid {
        let page = self.pick_page(rows);
        let widest = cells.iter().map(|c| term::width(c)).max().unwrap_or(10);
        let sel = self.row.saturating_sub(self.pick_fixed());
        crate::grid::Grid::lay(cells.len(), widest, cols, page, sel, self.pick_top)
    }

    fn draw_pick(&self, f: &mut Frame) {
        self.title(
            f,
            if self.picking_receive { "Where should received files go?" } else { "What do you want to send?" },
        );
        // The path is shown but never typed. A person recognises where they
        // are from it even when they could not have written it down.
        f.push(&format!("  {}", self.pick_dir.display()));
        f.blank();

        let has_parent = self.pick_dir.parent().is_some();
        let action = if self.picking_receive {
            "  PUT RECEIVED FILES IN THIS FOLDER".to_string()
        } else if self.picked.is_empty() {
            "  SEND EVERYTHING IN THIS FOLDER".to_string()
        } else {
            format!("  SEND THE {} TICKED", self.picked.len())
        };
        let mut head: Vec<String> = vec![action];
        if has_parent {
            head.push("  ..  go up one".to_string());
        }
        let hw = term::group_width(&head);
        for (i, h) in head.iter().enumerate() {
            if i == self.row {
                f.push_selected_within(h, hw);
            } else {
                f.push(h);
            }
        }
        // The folder's contents in columns across the window: see grid.rs.
        let cells = self.pick_cells();
        let fixed = self.pick_fixed();
        let g = self.pick_grid(&cells, f.rows, f.cols);
        for line in g.lines(&cells, self.row.checked_sub(fixed)) {
            f.push_raw(&line);
        }

        f.blank();
        if self.pick_unreadable {
            f.push_dim("  This folder cannot be read on this computer. Go up one and");
            f.push_dim("  choose another, or ask whoever set the machine up.");
        } else if self.pick_kids.is_empty() && self.pick_files.is_empty() {
            f.push_dim("  This folder is empty.");
        }
        if let Some(s) = g.status() {
            f.push_dim(&format!("  {s}"));
        }
        // A ticked folder is everything inside it, at any depth. Say so while
        // it can still be undone, and say where the count will be shown.
        if !self.picking_receive && self.picked.iter().any(|p| p.is_dir()) {
            f.push("  A ticked folder sends EVERYTHING inside it, every folder within");
            f.push("  included. The next screen shows how many files that is, and lets");
            f.push("  you open it and untick any you do not want.");
        }
        if self.picking_receive {
            f.push("  Go into the folder where received files should go, then");
            f.push("  press enter on the line at the top.");
        } else if self.picked.is_empty() {
            f.push_dim("  Tick things with space to send only those. Tick nothing and");
            f.push_dim("  the whole folder goes.");
        } else {
            // NAME what is ticked somewhere else, do not just say ticks are kept.
            //
            // Reported by the owner: he ticked one font, and the button said
            // SEND THE 2 TICKED while one box was ticked on screen. The other
            // tick was a folder in a branch he had walked through earlier, and
            // nothing on this screen could tell him what it was or take it off.
            // The folder held over 100,000 files, so the next screen announced
            // "100001 files chosen" for what he believed was a single file.
            //
            // "Ticks are kept while you move around" was true and useless: it
            // explains the mechanism and names none of the consequences. A
            // count that disagrees with what is on screen has to be accounted
            // for on the screen it disagrees with.
            let elsewhere: Vec<&PathBuf> = self
                .picked
                .iter()
                .filter(|p| p.parent() != Some(self.pick_dir.as_path()))
                .collect();
            if elsewhere.is_empty() {
                f.push_dim("  Ticks are kept while you move around, so you can take a file");
                f.push_dim("  from here and a folder from somewhere else.");
            } else {
                let word = if elsewhere.len() == 1 { "tick" } else { "ticks" };
                f.push_dim(&format!(
                    "  {} more {word} in other folders, still going with this:",
                    elsewhere.len()
                ));
                for p in elsewhere.iter().take(2) {
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| p.display().to_string());
                    let mark = if p.is_dir() { "/  (a whole folder)" } else { "" };
                    f.push_dim(&format!("    {name}{mark}"));
                }
                if elsewhere.len() > 2 {
                    f.push_dim(&format!("    and {} more", elsewhere.len() - 2));
                }
            }
        }
        // Naming both keys, because they do different things and the
        // difference is the one that cost somebody their selection: space
        // takes a tick off, enter never does.
        if self.picking_receive {
            self.hints(f, "  enter opens a folder    esc goes back without changing it");
        } else if self.picked.is_empty() {
            self.hints(
                f,
                "  space ticks and unticks    enter opens a folder    esc goes back",
            );
        } else {
            self.hints(
                f,
                "  space ticks   c clears all   enter opens a folder   esc goes back",
            );
        }
    }

    fn pick_key(&mut self, k: Key) -> bool {
        let has_parent = self.pick_dir.parent().is_some();
        let fixed = self.pick_fixed();
        let listed = if self.picking_receive {
            self.pick_kids.len()
        } else {
            self.pick_kids.len() + self.pick_files.len()
        };

        // Moving. pick_top is the grid's first column on screen.
        let cells = self.pick_cells();
        let (rows, cols) = term::size();
        let g = self.pick_grid(&cells, rows, cols);
        let moved = if self.row >= fixed {
            let sel = self.row - fixed;
            match (k, g.step(k, sel)) {
                (Key::Up, _) if sel == 0 => Some(fixed - 1),
                (_, Some(to)) => Some(fixed + to),
                _ => None,
            }
        } else {
            match k {
                Key::Up => Some(self.row.saturating_sub(1)),
                Key::Down | Key::PageDown if self.row + 1 < fixed + listed => Some(self.row + 1),
                _ => None,
            }
        };
        if let Some(r) = moved {
            self.row = r;
            self.pick_top = self.pick_grid(&cells, rows, cols).first_col;
            return false;
        }

        // What is under the cursor, if it is not one of the fixed rows.
        let under: Option<PathBuf> = if self.row < fixed {
            None
        } else {
            let i = self.row - fixed;
            if i < self.pick_kids.len() {
                Some(self.pick_dir.join(&self.pick_kids[i]))
            } else {
                self.pick_files
                    .get(i - self.pick_kids.len())
                    .map(|(n, _)| self.pick_dir.join(n))
            }
        };

        match k {
            // Choosing a place, not things: nothing to tick.
            Key::Char(' ') if self.picking_receive => {}
            Key::Char(' ') => {
                if let Some(p) = under {
                    self.pick_toggle(p);
                }
            }
            // One key that takes every tick off, wherever it was made.
            //
            // Space only reaches what is under the cursor, so a tick left in a
            // folder three branches away could be removed only by walking back
            // to it, and the screen did not say where it was. Starting again
            // meant leaving the picker entirely.
            Key::Char('c') => {
                self.picked.clear();
            }
            Key::Enter => {
                if self.row == 0 && self.picking_receive {
                    self.receive_dir = self.pick_dir.clone();
                    self.picking_receive = false;
                    self.screen = Screen::Send;
                    self.row = self.send_fields().len() - 1;
                } else if self.row == 0 {
                    self.finish_picking();
                } else if has_parent && self.row == 1 {
                    if let Some(p) = self.pick_dir.parent().map(|p| p.to_path_buf()) {
                        self.pick_at(p);
                    }
                } else if let Some(p) = under {
                    if p.is_dir() {
                        self.pick_at(p);
                    } else {
                        // Enter on a file ADDS it, and can never take it away.
                        //
                        // This used to toggle, on the reasoning that somebody
                        // who has not read the hint will press enter before
                        // they press space. True, and it made enter destructive:
                        // press it on something already ticked and the tick
                        // silently went. Reported as "pressing enter actually
                        // unselects the previously selected things", which is
                        // exactly what it did.
                        //
                        // Only space takes a tick off now. One key adds, one
                        // key removes, and the one people press by reflex is
                        // the one that cannot lose work.
                        self.pick_add(p);
                    }
                }
            }
            Key::Left | Key::Backspace => {
                if let Some(p) = self.pick_dir.parent().map(|p| p.to_path_buf()) {
                    self.pick_at(p);
                }
            }
            Key::Right => {
                if let Some(p) = under {
                    if p.is_dir() {
                        self.pick_at(p);
                    }
                }
            }
            Key::Esc => {
                self.picking_receive = false;
                self.screen = Screen::Send;
                self.row = 0;
            }
            Key::Char('q') | Key::Quit => return true,
            _ => {}
        }
        false
    }

    /// Turn the ticks into a folder to serve and a list of what may be seen.
    ///
    /// The served root has to contain everything ticked, so it is the deepest
    /// folder that is an ancestor of all of them. Tick two files in the same
    /// folder and the root is that folder; tick one here and one three levels
    /// down and the root rises to cover both. Nothing ticked means the folder
    /// being looked at, whole.
    fn finish_picking(&mut self) {
        if self.picked.is_empty() {
            self.folder = self.pick_dir.to_string_lossy().into_owned();
            self.picked.clear();
            self.chosen = None;
            self.screen = Screen::Send;
            self.row = self.send_fields().len();
            return;
        }

        let root = common_ancestor(&self.picked).unwrap_or_else(|| self.pick_dir.clone());

        // Expand ticked folders into the files under them, so the allow list
        // is files and only files: that is what the server checks against.
        let mut allow: std::collections::HashSet<String> = std::collections::HashSet::new();
        self.pick_truncated = false;
        for p in &self.picked {
            if p.is_dir() {
                for (rel, _) in serve::all_files(p) {
                    let full = p.join(&rel);
                    if let Some(r) = relative_to(&root, &full) {
                        allow.insert(r);
                    }
                }
                // Asked per folder, not once at the end: the flag describes the
                // most recent walk, so checking after the loop would only ever
                // report on whichever folder happened to be last.
                if serve::files_truncated() {
                    self.pick_truncated = true;
                }
            } else if let Some(r) = relative_to(&root, p) {
                allow.insert(r);
            }
        }

        self.folder = root.to_string_lossy().into_owned();
        self.chosen = Some(allow);
        self.screen = Screen::Send;
        // Land on the button that starts it, not back on the row that was just
        // answered. The next thing a person wants after choosing what to send
        // is to send it, and leaving the cursor on "What to send" invites
        // pressing enter again and reopening the picker they just left.
        self.row = self.send_fields().len();
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
                    // The folder row opens the picker instead of a text
                    // field. Typing a path is the thing this replaces.
                    if self.row == 0 {
                        self.open_picker();
                        return false;
                    }
                    // The last row: where received files go. Chosen by looking,
                    // like the folder to send, never by typing a path.
                    if self.row == fields.len() - 1 {
                        self.picking_receive = true;
                        let start = self
                            .receive_dir
                            .ancestors()
                            .find(|a| a.is_dir())
                            .map(|a| a.to_path_buf())
                            .unwrap_or_else(starting_folder);
                        self.pick_at(start);
                        self.screen = Screen::Pick;
                        return false;
                    }
                    // Row 1 in cable mode is the addresses toggle, handled
                    // below; nothing else here may read the wifi row numbers.
                    // Cable mode's middle row is a yes/no, not text, so
                    // enter flips it instead of opening an editor.
                    if self.cable && self.row == 1 {
                        self.anyway = !self.anyway;
                        return false;
                    }
                    // Windows: the band row is a choice of two, not text.
                    if cfg!(windows) && !self.cable && self.row == 3 {
                        self.band5 = !self.band5;
                        return false;
                    }
                    self.editing = Some(if self.cable {
                        // Cable mode's rows are not the wifi rows. Reading
                        // them off the wifi list would file a folder name as
                        // a password.
                        match self.row {
                            0 => self.folder.clone(),
                            _ => self.helpers.to_string(),
                        }
                    } else {
                        match self.row {
                            0 => self.folder.clone(),
                            1 => self.ssid.clone(),
                            2 => self.password.clone(),
                            3 => self.channel.clone(),
                            4 => self.helpers.to_string(),
                            _ => fields[self.row].1.clone(),
                        }
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
            Key::Char('h') | Key::Char('?') => {
                self.note(SENDING_HELP);
                return false;
            }
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
            // The folder received work goes to, opened like any folder.
            Key::Char('o') => open_with_system(&self.receive_dir),
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
                self.forget_session();
                // "Close this and start it again" was true before 0.9.9, when
                // Stop could not stop the server. It can now, so the way on is
                // the menu, not a restart.
                self.note("Stopped handing out.\n\nThe wifi network is off, and the phones can no longer reach the class page.\n\nWhat you chose to send has been forgotten, so the next lesson starts from nothing.\n\nTo hand out again, choose \"Hand out files to the class over wifi\" on the first screen.");
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
        let fixed = self.tick_fixed();
        let entries = self.tick_entries();
        let cells: Vec<String> = entries.iter().map(|e| self.tick_cell(e)).collect();
        let (rows, cols) = term::size();
        let g = self.tick_grid(&cells, rows, cols);
        // Anything but a second space cancels a pending "tick all of this".
        let confirming = self.tick_confirm.take();
        if k != Key::Char(' ') && k != Key::Char('a') {
            self.tick_said.clear();
        }

        // Moving. The fixed rows are a short list above the grid; up from the
        // top of the grid goes back into them.
        let in_grid = self.row >= fixed;
        let moved = if in_grid {
            let sel = self.row - fixed;
            match (k, g.step(k, sel)) {
                (Key::Up, _) if sel == 0 => Some(fixed - 1),
                (_, Some(to)) => Some(fixed + to),
                _ => None,
            }
        } else {
            match k {
                Key::Up => Some(self.row.saturating_sub(1)),
                Key::Down | Key::PageDown if !cells.is_empty() || self.row + 1 < fixed => {
                    Some((self.row + 1).min(fixed + cells.len().saturating_sub(1)))
                }
                _ => None,
            }
        };
        if let Some(r) = moved {
            self.row = r;
            let g = self.tick_grid(&cells, rows, cols);
            self.tick_first = g.first_col;
            return false;
        }

        let under = if in_grid { entries.get(self.row - fixed).cloned() } else { None };
        let mut changed = false;
        match k {
            Key::Char(' ') => match under {
                Some(TickEntry::Folder { name, files, ticked, bytes }) => {
                    let path = if self.tick_dir.is_empty() { name.clone() } else { format!("{}/{name}", self.tick_dir) };
                    if ticked == files {
                        self.tick_under(&format!("{path}/"), false);
                        self.tick_said = format!("Unticked everything inside {name}/.");
                        changed = true;
                    } else if big(files, bytes) && confirming.as_deref() != Some(path.as_str()) {
                        // One key can reach a hundred thousand files in a
                        // source tree. Say so, with the number, before it
                        // happens, and offer the way to choose instead.
                        self.tick_confirm = Some(path);
                    } else {
                        let n = self.tick_under(&format!("{path}/"), true);
                        self.tick_said = format!(
                            "Ticked all {} files inside {name}/. Press enter on it to look inside and untick any.",
                            count(n)
                        );
                        changed = true;
                    }
                }
                Some(TickEntry::File(i)) => {
                    self.tick[i].2 = !self.tick[i].2;
                    changed = true;
                }
                None => {}
            },
            Key::Char('a') => {
                let key = format!("\u{0}{}", self.tick_dir);
                let prefix = if self.tick_dir.is_empty() { String::new() } else { format!("{}/", self.tick_dir) };
                let (files, bytes, _) = self.tick_confirm_size(&key);
                if big(files, bytes) && confirming.as_deref() != Some(key.as_str()) {
                    self.tick_confirm = Some(key);
                } else {
                    let n = self.tick_under(&prefix, true);
                    self.tick_said = format!("Ticked all {} files here, folders inside included.", count(n));
                    changed = true;
                }
            }
            Key::Char('n') => {
                let prefix = if self.tick_dir.is_empty() { String::new() } else { format!("{}/", self.tick_dir) };
                let n = self.tick_under(&prefix, false);
                self.tick_said = format!("Unticked all {} files here.", count(n));
                changed = true;
            }
            Key::Enter => {
                if self.row == 0 {
                    self.apply_ticks();
                    if pre {
                        self.begin_sending();
                    } else {
                        self.screen = Screen::Sending;
                        self.row = 0;
                    }
                    return false;
                } else if fixed == 2 && self.row == 1 {
                    self.tick_up();
                } else {
                    match under {
                        Some(TickEntry::Folder { name, .. }) => self.tick_into(&name),
                        // Enter adds and never removes, as in the picker:
                        // the key pressed by reflex must not lose a choice.
                        Some(TickEntry::File(i)) => {
                            self.tick[i].2 = true;
                            changed = true;
                        }
                        None => {}
                    }
                }
            }
            Key::Backspace => self.tick_up(),
            // In a single column left and right are free: out of and into
            // folders, as in the picker.
            Key::Left => self.tick_up(),
            Key::Right => {
                if let Some(TickEntry::Folder { name, .. }) = under {
                    self.tick_into(&name);
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
        if changed && !pre {
            self.apply_ticks();
        }
        false
    }

    fn tick_into(&mut self, name: &str) {
        self.tick_dir = if self.tick_dir.is_empty() { name.to_string() } else { format!("{}/{name}", self.tick_dir) };
        self.tick_first = 0;
        self.row = self.tick_fixed();
    }

    /// Up one folder, landing on the folder just left.
    fn tick_up(&mut self) {
        if self.tick_dir.is_empty() {
            return;
        }
        let left = self.tick_dir.rsplit('/').next().unwrap_or("").to_string();
        self.tick_dir = match self.tick_dir.rsplit_once('/') {
            Some((up, _)) => up.to_string(),
            None => String::new(),
        };
        self.tick_first = 0;
        let fixed = self.tick_fixed();
        let at = self
            .tick_entries()
            .iter()
            .position(|e| matches!(e, TickEntry::Folder { name, .. } if *name == left))
            .unwrap_or(0);
        self.row = fixed + at;
        let (rows, cols) = term::size();
        let entries = self.tick_entries();
        let cells: Vec<String> = entries.iter().map(|e| self.tick_cell(e)).collect();
        self.tick_first = self.tick_grid(&cells, rows, cols).first_col;
    }

    fn waiting_key(&mut self, k: Key) -> bool {
        let items = serve::pending();
        self.move_row(k, items.len().max(1));
        let root = PathBuf::from(shellexpand(&self.folder));
        match k {
            // Everything at once, or everything from one person.
            //
            // One key per file was the only way, and the owner did the sum:
            // forty children sending six pieces each is two hundred and forty
            // presses. Accepting only moves files into the received folder,
            // nothing is deleted or shown, so doing many at once is safe; o on
            // any one still opens it first for a teacher who wants to look.
            Key::Char('e') | Key::Char('p') => {
                let person = items.get(self.row).map(|p| p.from.clone());
                let chosen: Vec<_> = items
                    .iter()
                    .filter(|p| k == Key::Char('e') || Some(&p.from) == person.as_ref())
                    .collect();
                let mut done = 0;
                let mut failed: Vec<String> = Vec::new();
                for p in &chosen {
                    match serve::accept_pending(&root, &p.on_disk) {
                        Ok(()) => done += 1,
                        Err(e) => failed.push(format!("{}: {e}", p.original)),
                    }
                }
                let word = if done == 1 { "piece" } else { "pieces" };
                let mut msg = format!(
                    "Accepted {done} {word} of work.\n\nThey are in\n{}\n\nPress o on the sending screen to open that folder.",
                    crate::page::handed_in_dir(&root).display()
                );
                if !failed.is_empty() {
                    msg.push_str(&format!("\n\nThese could not be moved:\n{}", failed.join("\n")));
                }
                self.row = 0;
                self.note(&msg);
                self.back = Screen::Waiting;
            }
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
                    open_with_system(&crate::page::waiting_dir(&root).join(&p.on_disk));
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
        self.move_row(k, self.files.len() + 3);
        match k {
            Key::Enter => {
                if self.row < 3 {
                    self.editing = Some(match self.row {
                        0 => self.save_into.clone(),
                        1 => self.at_once.to_string(),
                        _ => self.files_at_once.to_string(),
                    });
                } else if let Some(e) = self.files.get(self.row - 3).cloned() {
                    self.begin_download(&e);
                }
            }
            Key::Char('a') => {
                self.begin_download_all();
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

        // An explicit choice is listed as itself, without walking the tree.
        //
        // The root of a ticked set is the deepest folder containing all of it,
        // which can be very high up: tick one file on the desktop and one in
        // Documents and the root is the whole user profile. Walking that to
        // show four files would read a hundred thousand paths off the disk to
        // throw almost all of them away, and on the machines this is for that
        // is a long freeze in front of a class.
        if pre {
            if let Some(only) = self.chosen.clone() {
                let mut rows: Vec<(String, u64, bool)> = only
                    .iter()
                    .map(|rel| {
                        let size = folder.join(rel).metadata().map(|m| m.len()).unwrap_or(0);
                        (rel.clone(), size, true)
                    })
                    .collect();
                rows.sort_by(|a, b| a.0.cmp(&b.0));
                self.tick = rows;
                self.tick_dir.clear();
                self.tick_first = 0;
                self.tick_confirm = None;
                self.tick_said.clear();
                self.screen = Screen::Tick { pre };
                self.row = 0;
                return;
            }
        }

        // The same walk the network uses, so the tick list and the class page
        // can never disagree about what exists. Reading the folder separately
        // here is what let the two drift: this screen listed the top level
        // while the server also served everything nested under it.
        //
        // all_files, NOT visible_files: this screen has to show what is there,
        // including what is currently unticked, and it must not change what the
        // class can reach just by being opened.
        for (rel, size) in serve::all_files(&folder) {
            let ticked = if pre {
                // "Everything" is the right default when a folder was pointed
                // at, and the wrong one when a person has just ticked four
                // files out of a terabyte: re-ticking all of them here would
                // throw that away silently and hand out the lot.
                match &self.chosen {
                    Some(only) => only.contains(&rel),
                    None => true,
                }
            } else {
                self.tick.iter().find(|(n, _, _)| *n == rel).map(|(_, _, t)| *t).unwrap_or(false)
            };
            fresh.push((rel, size, ticked));
        }
        self.tick = fresh;
        self.tick_dir.clear();
        self.tick_first = 0;
        self.tick_confirm = None;
        self.tick_said.clear();
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
        // A fresh flag for everything this session starts, so Stop can end
        // exactly this session's helpers and nothing else.
        self.cable_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        // The network before the port. Binding a port on a network the class
        // cannot reach looks like success and is not.
        if !self.ssid.is_empty() {
            // Only "not a number" is refused here. Whether the number is a
            // channel this radio may broadcast on is hotspot_up's judgement,
            // made against the kernel's own list, and its refusal names the
            // channels that ARE allowed.
            let channel = match self.channel.trim() {
                "" => None,
                t => match t.parse::<u16>() {
                    Ok(ch) => Some(ch),
                    Err(_) => {
                        self.note(
                            "The wifi channel has to be a number, \
                             or empty to let the computer choose.",
                        );
                        self.back = Screen::Send;
                        return;
                    }
                },
            };
            // The same name keeps the same password, so laptops that joined
            // last time join again without being asked. Only when the person
            // did not type one themselves.
            if !self.password_typed {
                if let Some(saved) = net::saved_hotspot_password(&self.ssid) {
                    self.password = saved;
                }
            }
            net::set_wifi_band_5(self.band5);
            match net::hotspot_up(&self.ssid, &self.password, channel) {
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
        // Received work goes to the named folder, made now so it is there to
        // be opened from the first minute.
        let _ = std::fs::create_dir_all(&self.receive_dir);
        crate::page::set_receive_dir(Some(self.receive_dir.clone()));
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

        // A cable needs two things a wifi network gets from its router: a way
        // for the other end to find this computer, and an address for it to
        // use. Both are started only in cable mode, and both stop when the
        // serving stops.
        // gorilla.local on a hotspot this program made, or on Windows' own
        // Mobile Hotspot (always 192.168.137.1). Before 2026-09-23 the name
        // was answered only down a cable, so on wifi the only thing that
        // could answer it was a stale copy somewhere else, and testers typing
        // it landed on old lessons.
        if !self.cable {
            self.mdns_live = Arc::new(std::sync::atomic::AtomicBool::new(false));
            self.taken_live = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stop = Arc::clone(&self.cable_stop);
            let (live, taken) = (Arc::clone(&self.mdns_live), Arc::clone(&self.taken_live));
            match &self.hotspot {
                // Ours: its address may still be on its way, so wait for it.
                Some(h) => {
                    // On Windows, also the names the phones ask, so the sign-in
                    // page opens by itself: see dns::start_hotspot_names.
                    if cfg!(windows) {
                        let iface = h.iface.clone();
                        crate::dns::start_hotspot_names(
                            move || net::hotspot_address_of(&iface),
                            Arc::clone(&self.cable_stop),
                        );
                    }
                    let iface = h.iface.clone();
                    crate::dns::start_mdns_when_ready(
                        move || net::hotspot_address_of(&iface),
                        stop,
                        live,
                        taken,
                    );
                }
                // Windows' own hotspot, switched on by hand: only if it is up.
                None if cfg!(windows) && net::hotspot_address_of("").is_some() => {
                    crate::dns::start_mdns_when_ready(move || net::hotspot_address_of(""), stop, live, taken);
                }
                None => {}
            }
        }
        if self.cable {
            let short = PathBuf::from(shellexpand(&self.folder))
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "files".into());
            let me = std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .unwrap_or_else(|_| "this computer".into());
            // Switches the served page from the class page to the accept page.
            crate::page::set_sender(&me);
            let _ = cable::start_beacon(port(), me, short, Arc::clone(&self.cable_stop));

            let ours = self
                .addresses
                .iter()
                .copied()
                .find(|a| a.is_link_local())
                .unwrap_or(std::net::Ipv4Addr::new(169, 254, 1, 1));
            // Names first: the address lease has to say whether a resolver is
            // running, and that is only known once one has tried to start.
            // Behind the same guard as the address server: this resolver
            // answers every name with our address, which is correct on a bare
            // cable and is claiming to be the whole internet anywhere else.
            // .local is announced whatever the guard decides: it is
            // link-scoped and needs nothing configured on the far end, so it
            // is the naming that still works when the guard refuses.
            // Asked before we start answering, so the probe cannot hear us.
            self.name_taken = crate::dns::name_is_taken(ours, std::time::Duration::from_millis(700));
            self.mdns = crate::dns::start_mdns(ours, Arc::clone(&self.cable_stop)).is_ok();
            // Watched, not decided once.
            //
            // Deciding at this moment and never looking again is what made
            // "turn the wifi off" unactionable: the answer was reached before
            // the person did the thing it asked for, and the screen then said
            // the same sentence forever. The supervisor re-reads the network
            // every few seconds and starts on its own when the way is clear.
            self.cable_watch = Some(dhcp::supervise(self.anyway, Arc::clone(&self.cable_stop)));
        }

        self.started = Some(Instant::now());
        self.screen = Screen::Sending;
        self.row = 0;
    }

    /// What this computer can and cannot do.
    ///
    /// Everything on this screen is something that has actually gone wrong in
    /// a room with nobody to ask. The firewall line is the important one: when
    /// it is blocking, every other line still reads perfectly and no phone in
    /// the room can reach the address on the board.
    fn draw_checkup(&self, f: &mut Frame) {
        self.title(f, "Fix problems with this computer");

        // One line per thing that can stop the hub, each saying plainly
        // whether it is fine. Labels are never repeated: this screen once
        // said "cable" twice about two different things, and called a wifi
        // link a cable.
        match self.services_off() {
            Some(off) if !off.is_empty() => {
                f.push(&format!("  Windows parts    {} SWITCHED OFF:", off.len()));
                for name in &off {
                    f.push(&format!("                     {name}"));
                }
            }
            Some(_) => f.push("  Windows parts    all switched on"),
            None if cfg!(windows) => f.push("  Windows parts    checking..."),
            None => {}
        }
        match tune::reachable() {
            Some(true) => f.push("  Firewall         open for the hub"),
            // Only that OUR rule is missing, not that nothing can get in:
            // Windows may still allow the program by name.
            Some(false) => f.push("  Firewall         not opened for the hub yet: others may be blocked"),
            None => {}
        }
        if let Some(card) = net::wifi_card_summary() {
            let measure = f.cols.saturating_sub(21).min(70).max(24);
            for (i, chunk) in wrap(&card, measure).into_iter().enumerate() {
                if i == 0 {
                    f.push(&format!("  Wifi card        {chunk}"));
                } else {
                    f.push(&format!("                   {chunk}"));
                }
            }
        }
        let w = tune::wire();
        let name = w.adapter.to_ascii_lowercase();
        let wifi = ["wi-fi", "wifi", "wlan", "wireless"].iter().any(|k| name.contains(k));
        if !w.measured {
            f.push("  Connection       none, or its speed cannot be read");
        } else if wifi {
            f.push(&format!("  Connection       wifi ({}) at {} Mbps", w.adapter, w.megabits));
        } else {
            f.push(&format!("  Connection       cable ({}) at {} Mbps", w.adapter, w.megabits));
        }
        if dhcp::safe_to_offer(&net::local_addresses(), net::default_gateway()) {
            f.push("  Cable            plugged in, straight to another computer");
        } else {
            f.push_dim("  Cable            none plugged in (only needed to send down a cable)");
        }
        let addrs = net::local_addresses();
        if addrs.is_empty() {
            f.push("  Address          none, so nobody can reach this computer");
        }
        for a in &addrs {
            let what = if a.is_link_local() { "  (the cable)" } else { "" };
            f.push(&format!("  Address          {a}{what}"));
        }
        f.blank();

        let items = self.checkup_items();
        let gw = term::group_width(&items.iter().map(|i| format!("  {i}")).collect::<Vec<_>>());
        for (i, item) in items.iter().enumerate() {
            let line = format!("  {item}");
            if self.row == i {
                f.push_selected_within(&line, gw);
            } else {
                f.push(&line);
            }
        }
        f.blank();
        f.push_dim("  Do both once on every computer, even if it seems to work fine.");
        f.push_dim("  Laptops that have been \"sped up\" by a tweak list or a script look");
        f.push_dim("  normal until the wifi network or the cable fails in front of people.");
        f.push_dim("  Windows will ask for permission. That is expected: say Yes.");
        self.hints(f, "  enter to do it    esc to go back");
    }

    /// The buttons on the fix screen, most important first.
    fn checkup_items(&self) -> Vec<&'static str> {
        if cfg!(windows) {
            vec![
                "Turn on the parts of Windows the hub needs",
                "Let other devices reach this computer (firewall)",
            ]
        } else {
            vec!["Let other devices reach this computer (firewall)"]
        }
    }

    fn draw_fixoffer(&self, f: &mut Frame) {
        self.title(f, "This computer is not ready yet");
        f.push("  Parts of Windows the hub needs are switched off here:");
        f.blank();
        for name in self.services_off().unwrap_or_default() {
            f.push(&format!("    {name}"));
        }
        f.blank();
        f.push("  Without them the wifi network or the cable will not work, and");
        f.push("  Windows will not say why. This is common on laptops that have been");
        f.push("  \"sped up\" by a tweak list or a script. Nothing is broken.");
        f.blank();
        f.push("  Press enter to switch them back on. Windows will ask for");
        f.push("  permission: say Yes. It takes a few seconds.");
        self.hints(f, "  enter to switch them on    esc to carry on without");
    }

    fn fixoffer_key(&mut self, k: Key) -> bool {
        match k {
            Key::Enter => {
                let msg = crate::services::fix();
                self.refresh_services();
                self.screen = Screen::Send;
                self.note(&msg);
                self.back = Screen::Send;
            }
            Key::Esc | Key::Char('q') => {
                self.screen = Screen::Send;
                self.row = 0;
            }
            Key::Quit => return true,
            _ => {}
        }
        false
    }

    fn checkup_key(&mut self, k: Key) -> bool {
        let count = self.checkup_items().len();
        self.move_row(k, count);
        match k {
            Key::Enter => {
                // The answer goes in a box on this screen. It must not be
                // printed: this program owns the whole terminal, so anything
                // written straight to stdout is drawn over by the next frame
                // a quarter of a second later.
                //
                // Row 1 waits on Windows' permission prompt and on the
                // services starting, a few seconds in all. The screen stands
                // still meanwhile, which is right: the prompt is in front.
                let services_row = cfg!(windows) && self.row == 0;
                let msg = if services_row {
                    let m = crate::services::fix();
                    self.refresh_services();
                    m
                } else {
                    match tune::open_ports() {
                        Ok(m) => m,
                        Err(m) => m,
                    }
                };
                self.note(&msg);
                self.back = Screen::Home;
            }
            Key::Esc | Key::Char('q') => {
                self.screen = Screen::Home;
                self.row = 0;
            }
            _ => {}
        }
        false
    }

    fn start_looking(&mut self) {
        *self.found.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let found = Arc::clone(&self.found);
        std::thread::spawn(move || {
            // 80 first (the packaged install), 8080 for an unpackaged build.
            let mut list = net::find_servers(80);
            let more = net::find_servers(port());
            for (ip, n, who) in more {
                if !list.iter().any(|(i, _, _)| *i == ip) {
                    list.push((ip, n, who));
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

    /// Get every file on the list, folders and all.
    ///
    /// Sequential, with the usual four connections INSIDE each file. That is
    /// deliberate: the connection sweep measured the ceiling as airtime, not
    /// threads, and the peak was 7.0 MB/s at every worker count from 1 to 32.
    /// Fetching six files at once would not create more air; it would only make
    /// six files half-finished when the signal drops instead of five finished
    /// and one to resume.
    fn begin_download_all(&mut self) {
        let Some(ip) = self.server else { return };
        let files = self.files.clone();
        if files.is_empty() {
            return;
        }
        let count = files.len();
        let lanes = self.files_at_once.max(1).min(count);
        let root = PathBuf::from(shellexpand(&self.save_into));
        let port = self.server_port;
        let at_once = self.at_once;
        let batch = Arc::clone(&self.batch);
        let result = Arc::clone(&self.result);
        *result.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *batch.lock().unwrap_or_else(|e| e.into_inner()) = Some(Batch {
            done: 0,
            total: count,
            in_flight: Vec::new(),
            bytes: 0,
            failed: Vec::new(),
            finished: false,
            lanes,
        });
        crate::fetch::set_quiet(true);

        // A queue the lanes share, rather than a slice each. Files vary from
        // five kilobytes to five gigabytes, so handing each lane a fixed share
        // would leave one lane carrying the database while the rest finished
        // and sat idle.
        let queue = Arc::new(Mutex::new(files));
        let mut handles = Vec::new();
        for _ in 0..lanes {
            let queue = Arc::clone(&queue);
            let batch = Arc::clone(&batch);
            let root = root.clone();
            handles.push(std::thread::spawn(move || loop {
                let next = {
                    let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
                    if q.is_empty() { None } else { Some(q.remove(0)) }
                };
                let Some(e) = next else { return };
                {
                    let mut b = batch.lock().unwrap_or_else(|x| x.into_inner());
                    match b.as_mut() {
                        Some(st) => st.in_flight.push(e.name.clone()),
                        None => return, // the teacher pressed escape
                    }
                }
                let dest = root.join(&e.name);
                let mut failure = None;
                if let Some(parent) = dest.parent() {
                    if let Err(err) = std::fs::create_dir_all(parent) {
                        failure = Some(format!("{}: {err}", e.name));
                    }
                }
                if failure.is_none() {
                    let url = format!("http://{ip}:{port}/{}", crate::fetch::url_path(&e.name));
                    if let Err(err) = crate::fetch::download_known(
                        &url, &dest.to_string_lossy(), at_once, true, e.size)
                    {
                        failure = Some(format!("{}: {err}", e.name));
                    }
                }
                let mut b = batch.lock().unwrap_or_else(|x| x.into_inner());
                let Some(st) = b.as_mut() else { return };
                st.in_flight.retain(|n| n != &e.name);
                st.done += 1;
                match failure {
                    Some(f) => st.failed.push(f),
                    None => st.bytes += e.size,
                }
            }));
        }
        // One thread waits for the lanes, so the screen learns it has finished
        // without polling every handle from the draw loop.
        let batch_done = Arc::clone(&batch);
        std::thread::spawn(move || {
            for h in handles {
                let _ = h.join();
            }
            let mut b = batch_done.lock().unwrap_or_else(|x| x.into_inner());
            if let Some(st) = b.as_mut() {
                st.finished = true;
                st.in_flight.clear();
            }
        });

        self.downloading = Some(format!("{count} files"));
        self.since = Some(Instant::now());
        self.screen = Screen::Receiving;
        self.row = 0;
    }

    fn begin_download(&mut self, entry: &net::Entry) {
        let Some(ip) = self.server else { return };
        let url = format!("http://{ip}:{}/{}", self.server_port, entry.name);
        let dest = PathBuf::from(shellexpand(&self.save_into)).join(&entry.name);
        let dest = dest.to_string_lossy().into_owned();
        let at_once = self.at_once;
        let result = Arc::clone(&self.result);
        *result.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *self.batch.lock().unwrap_or_else(|e| e.into_inner()) = None;
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

/// The h key on the handing-out screen: every key there, and what the class
/// does, in words for somebody who has never seen the program.
///
/// Asked for by the owner, 2026-09-24: the tool "is getting quite complex to
/// use", and the bottom line of this screen was seven letters and seven words
/// ("f files  n notice  w waiting...") that only make sense once you know them.
/// One page, reachable from the screen where the question comes up, rather
/// than a manual nobody has with them in the bush.
const SENDING_HELP: &str = "HELP: HANDING OUT FILES

WHAT THE CLASS DOES
1. Switch wifi ON. Mobile data can stay off.
2. Scan code 1 with the phone's camera and tap what appears: the phone joins the class wifi. A phone that cannot scan: join the network named at the top by hand, and type the password shown there.
3. The class page opens by itself. If it does not, scan code 2, or type the address shown at the top.
4. On the page they type their name once. Then READ or GET IT for your files, send their work to you, or send you a note.

THE KEYS ON THIS SCREEN
f   Files: tick or untick what the class can see. It changes on the phones at once.
n   Message: a line shown at the top of every phone's page, for example \"Open lesson 2\".
w   Work: what the class has sent you. Accept it into your received folder, or refuse it.
o   Opens your received folder, where accepted work is kept.
c   Who is connected: pause a device that misbehaves, or change the wifi password.
j   One big code to join the wifi, easier to scan from the back of the room.
q   Stop: switches the wifi network off. The phones lose the class page.

IF SOMETHING GOES WRONG
A phone cannot see the network: stand closer; walls and metal block wifi. On Windows, stop and check that the band on the start screen says 2.4 GHz.
The page does not open by itself: scan code 2, or type the address.
Choose files does nothing on a phone: that phone opened the page in its small sign-in window. The page tells them how to open it in the normal browser.
The network went off: the hub switches it back on by itself within seconds, and says so on this screen.";

/// A message, wrapped by words at the window's width and indented. Wrapping
/// here rather than letting the terminal do it keeps words whole and keeps
/// the hint line on the screen.
fn note_lines(text: &str, cols: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        for chunk in wrap(line, cols.saturating_sub(4)) {
            out.push(format!("  {chunk}"));
        }
    }
    out
}

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

/// Open a file or folder the way double-clicking it would.
///
/// This was `xdg-open` everywhere, which exists only on Linux, so on Windows
/// "o open and look" did nothing at all.
fn open_with_system(path: &Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// One entry in the folder being looked at on the tick screen.
#[derive(Clone, Debug)]
enum TickEntry {
    Folder { name: String, files: usize, ticked: usize, bytes: u64 },
    /// An index into the flat tick list.
    File(usize),
}

/// Big enough that ticking it with one key deserves a warning with the
/// number in it first. Twenty files is past what a person holds in their
/// head; 100 MB is past what goes unnoticed on a classroom network.
fn big(files: usize, bytes: u64) -> bool {
    files >= 20 || bytes >= 100_000_000
}

/// 64210 as "64,210": a count people read in a warning has to be readable.
fn count(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
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
        let d = crate::scratchdir::scratch("tab");
        for sub in ["Pictures", "Public", "Music", "Pictures/Screenshots"] {
            std::fs::create_dir_all(d.join(sub)).unwrap();
        }
        std::fs::write(d.join("Pictures/a-file.txt"), b"x").unwrap();
        d
    }

    /// The first screen has to say which build it is.
    ///
    /// It did not, for every release up to 0.9.2. The menu is the way in for
    /// most people, so somebody reporting that something does not work has the
    /// program open in front of them and no way to answer "which version".
    /// Sending them to `hub --version` means telling them to quit the only
    /// part of it they know how to use.
    ///
    /// Guarded because a heading is exactly the kind of string that gets
    /// rewritten for tone and quietly loses a detail on the way.
    #[test]
    fn the_first_screen_says_which_version_it_is() {
        let app = App::new();
        let mut f = crate::term::Frame::new(24, 80);
        app.draw_home(&mut f);
        let shown = f.text();
        assert!(
            shown.contains(env!("CARGO_PKG_VERSION")),
            "the home screen does not name the version:\n{shown}"
        );
    }

    /// The help page is longer than a small window. Before 0.9.10 a message
    /// that did not fit was cut off, and the line saying how to leave went
    /// with it.
    #[test]
    fn a_long_message_scrolls_and_keeps_the_way_out() {
        let mut app = App::new();
        app.note(SENDING_HELP);
        let mut f = crate::term::Frame::new(24, 80);
        app.draw_note(&mut f);
        let top = f.text();
        assert!(top.contains("more lines below"), "must say there is more:\n{top}");
        assert!(top.contains("enter or esc to go back"), "the way out must stay:\n{top}");
        assert!(top.contains("HELP: HANDING OUT FILES"), "starts at the top:\n{top}");
        app.row = 10_000;
        let mut f = crate::term::Frame::new(24, 80);
        app.draw_note(&mut f);
        let end = f.text();
        assert!(end.contains("says so on this screen"), "scrolled to the end shows the last line:\n{end}");
        assert!(!end.contains("more lines below"), "nothing below the end:\n{end}");
        assert!(end.contains("enter or esc to go back"));
    }

    /// 0.9.10 added explanation lines to the first screen and the start
    /// screen. At 24 by 80, the smallest window this is meant for, every one
    /// of them must still leave the key line at the bottom.
    #[test]
    fn the_new_explanations_fit_a_24_by_80_window() {
        let mut app = App::new();
        for row in 0..4 {
            app.row = row;
            let mut f = crate::term::Frame::new(24, 80);
            app.draw_home(&mut f);
            assert!(f.text().contains("enter to open"), "home row {row} lost its key line:\n{}", f.text());
        }
        let rows = app.send_fields().len() + 1;
        for row in 0..rows {
            app.row = row;
            let mut f = crate::term::Frame::new(24, 80);
            app.draw_send(&mut f);
            assert!(f.text().contains("esc to go back"), "start screen row {row} lost its key line:\n{}", f.text());
        }
    }

    fn switched_off_here() -> Vec<crate::services::Found> {
        vec![crate::services::Found {
            name: "icssvc".into(),
            label: "Windows Mobile Hotspot Service".into(),
            start: Some(4),
            running: false,
            want: 3,
            named: true,
        }]
    }

    /// The owner, a long-time Debian user, could not find the fix when it
    /// sat one level down under "Check this computer". So the first screen
    /// says the computer is not ready, names what is off and where to go,
    /// in full at 72 columns, the narrowest window seen in use.
    #[test]
    fn the_first_screen_says_plainly_when_this_computer_is_not_ready() {
        let app = App::new();
        *app.services.lock().unwrap() = Some(switched_off_here());
        let mut f = crate::term::Frame::new(24, 72);
        app.draw_home(&mut f);
        let shown = f.text();
        for must in [
            "NOT READY",
            "Windows Mobile Hotspot Service",
            "Fix problems with this computer (wifi, cable, firewall)",
            "before anything else.",
            "Windows will ask permission: say Yes.",
            "fails in front of the people waiting for the files.",
        ] {
            assert!(shown.contains(must), "missing {must:?}:
{shown}");
        }
    }

    /// Choosing wifi or cable with something switched off stops at the
    /// offer to fix it, instead of letting the failure happen later.
    #[test]
    fn going_into_wifi_or_cable_offers_the_fix_first() {
        for row in [0, 1] {
            let mut app = App::new();
            *app.services.lock().unwrap() = Some(switched_off_here());
            app.row = row;
            app.home_key(Key::Enter);
            assert!(matches!(app.screen, Screen::FixOffer), "row {row} went past the offer");
            app.fixoffer_key(Key::Esc);
            assert!(matches!(app.screen, Screen::Send), "esc must carry on to the form");
        }
        let mut app = App::new();
        *app.services.lock().unwrap() = Some(Vec::new());
        app.home_key(Key::Enter);
        assert!(matches!(app.screen, Screen::Send), "nothing off, nothing in the way");
    }

    fn a_tree(app: &mut App, big_folder: usize) {
        let mut t: Vec<(String, u64, bool)> = vec![
            ("readme.txt".into(), 1000, false),
            ("photos/beach.jpg".into(), 3_000_000, false),
            ("photos/note.txt".into(), 29, false),
        ];
        for i in 0..big_folder {
            t.push((format!("kernel/drivers/net/file-{i:05}.c"), 4000, false));
        }
        app.tick = t;
        app.tick_dir.clear();
        app.row = 0;
    }

    /// One folder at a time, each folder saying how many files are in it.
    #[test]
    fn the_tick_list_shows_folders_with_their_counts() {
        let mut app = App::new();
        a_tree(&mut app, 30);
        let cells: Vec<String> = app.tick_entries().iter().map(|e| app.tick_cell(e)).collect();
        assert_eq!(cells, ["[ ] kernel/  (30 files)", "[ ] photos/  (2 files)", "[ ] readme.txt  1 KB"]);
    }

    /// The owner's point: one key on a source tree can reach a hundred
    /// thousand files. The first space says how many and waits; the second
    /// ticks them; a small folder ticks at once.
    #[test]
    fn ticking_a_big_folder_says_how_many_first() {
        let mut app = App::new();
        a_tree(&mut app, 30);
        app.row = 1; // kernel/
        app.tick_key(Key::Char(' '), true);
        assert!(app.tick_confirm.is_some(), "a 30-file folder must ask first");
        assert_eq!(app.tick.iter().filter(|t| t.2).count(), 0, "nothing ticked yet");
        let mut f = crate::term::Frame::new(40, 100);
        app.draw_tick(&mut f, true);
        assert!(f.text().contains("ALL 30 files inside kernel/"), "{}", f.text());
        app.tick_key(Key::Char(' '), true);
        assert_eq!(app.tick.iter().filter(|t| t.2).count(), 30);

        app.row = 2; // photos/, two files: no question
        app.tick_key(Key::Char(' '), true);
        assert!(app.tick_confirm.is_none());
        assert_eq!(app.tick.iter().filter(|t| t.2).count(), 32);
    }

    /// Any other key cancels the question, so a stray space later does not
    /// tick thirty thousand files.
    #[test]
    fn the_question_is_forgotten_on_any_other_key() {
        let mut app = App::new();
        a_tree(&mut app, 30);
        app.row = 1;
        app.tick_key(Key::Char(' '), true);
        app.tick_key(Key::Down, true);
        app.tick_key(Key::Up, true);
        app.tick_key(Key::Char(' '), true);
        assert_eq!(app.tick.iter().filter(|t| t.2).count(), 0, "must ask again");
    }

    /// Tick a whole folder, go inside, take one file out: the folder then
    /// shows as partly ticked from the level above.
    #[test]
    fn inside_a_ticked_folder_single_files_can_be_taken_out() {
        let mut app = App::new();
        a_tree(&mut app, 0);
        app.row = 1; // photos/
        app.tick_key(Key::Char(' '), true);
        app.tick_key(Key::Enter, true);
        assert_eq!(app.tick_dir, "photos");
        assert_eq!(app.row, 2, "lands on the first file, below the two fixed rows");
        app.tick_key(Key::Char(' '), true);
        assert_eq!(app.tick.iter().filter(|t| t.2).count(), 1);
        app.tick_key(Key::Backspace, true);
        assert_eq!(app.tick_dir, "");
        let cells: Vec<String> = app.tick_entries().iter().map(|e| app.tick_cell(e)).collect();
        assert!(cells[0].starts_with("[~] photos/"), "{cells:?}");
        assert_eq!(app.row, 1, "back on the folder just left");
    }

    /// The reported window: 78 files, 240 wide. Everything on one screen.
    #[test]
    fn a_wide_window_shows_every_entry() {
        let mut app = App::new();
        app.tick = (0..78).map(|i| (format!("some-longish-file-name-{i:03}.txt"), 1000, false)).collect();
        let mut f = crate::term::Frame::new(45, 240);
        app.draw_tick(&mut f, true);
        let shown = f.text();
        assert!(shown.contains("some-longish-file-name-000.txt"));
        assert!(shown.contains("some-longish-file-name-077.txt"), "{shown}");
        assert!(!shown.contains("Showing"), "nothing should be hidden:
{shown}");
    }

    /// Forty children sending six pieces each was 240 presses. e takes all
    /// of it, p takes everything from the person under the cursor, and the
    /// files really move into the received folder.
    #[test]
    fn work_can_be_accepted_all_at_once_or_per_person() {
        let _g = crate::serve::SESSION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = crate::scratchdir::scratch("accept-all");
        let recv = root.join("received");
        crate::page::set_receive_dir(Some(recv.clone()));
        let waiting = crate::page::waiting_dir(&root);
        std::fs::create_dir_all(&waiting).unwrap();
        for (ip, name) in [("10.9.0.1", "a1.txt"), ("10.9.0.1", "a2.txt"), ("10.9.0.2", "b1.txt")] {
            std::fs::write(waiting.join(name), name).unwrap();
            crate::serve::note_pending(ip, name, name, 5);
        }
        let mut app = App::new();
        app.folder = root.to_string_lossy().into_owned();
        app.screen = Screen::Waiting;
        app.row = 0;
        app.waiting_key(Key::Char('p'));
        assert!(recv.join("a1.txt").exists() && recv.join("a2.txt").exists(), "p took this person's work");
        assert!(waiting.join("b1.txt").exists(), "p left somebody else's alone");
        assert_eq!(crate::serve::pending_count(), 1);
        app.screen = Screen::Waiting;
        app.waiting_key(Key::Char('e'));
        assert!(recv.join("b1.txt").exists(), "e took everything left");
        assert_eq!(crate::serve::pending_count(), 0);
        crate::page::set_receive_dir(None);
    }

    /// A hint line that runs off the edge of the window is a truncated one.
    ///
    /// Adding "c clears every tick" pushed the picker's hint to 77 columns.
    /// The default terminal is 80 and the window in use was narrower, so the
    /// line ended "esc goes" and the key for going back was simply not there.
    /// Caught in a screenshot, not by anything here, which is why this exists.
    ///
    /// Checked at 72 columns and not only at 80. The first version of this
    /// guard used 80, passed, and would have let the same fault through: 77
    /// fits in 80. 72 is not a guess, it is the width of the window in the
    /// screenshot, counted from the line that was cut: "  space ticks    c
    /// clears every tick    enter opens a folder    esc goes" is 72 characters.
    /// A terminal nobody has maximised is the one to design for.
    #[test]
    fn the_hint_line_fits_the_window_it_is_drawn_in() {
        let d = crate::scratchdir::scratch("hint-width");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a.txt"), b"x").unwrap();

        for ticked in [false, true] {
            let mut app = App::new();
            app.pick_dir = d.clone();
            app.picked = if ticked { vec![d.join("a.txt")] } else { Vec::new() };

            for cols in [72, 80] {
                let mut f = crate::term::Frame::new(24, cols);
                app.draw_pick(&mut f);
                let shown = f.text();
                assert!(
                    shown.contains("esc goes back"),
                    "the hint is cut off at {cols} columns (ticked={ticked}):\n{shown}"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A tick made in another folder has to be visible from wherever you are.
    ///
    /// The owner ticked one font and the button read SEND THE 2 TICKED with a
    /// single box ticked on screen. The other tick was a folder he had walked
    /// past earlier, and nothing on the screen named it or could take it off.
    /// That folder held over 100,000 files, so the next screen announced
    /// "100001 files chosen" for what he believed was one file.
    /// On a cable there is no password, and this screen used to stop there.
    ///
    /// That left phones and tablets, the devices that most need a camera, with
    /// an address to read off a terminal and type by hand. The code now
    /// carries the page itself, which needs no password because there is no
    /// network to join.
    #[test]
    fn the_camera_screen_offers_the_page_when_there_is_no_password() {
        let mut app = App::new();
        app.hotspot = None;
        app.cable = true;
        app.addresses = vec![std::net::Ipv4Addr::new(169, 254, 87, 1)];

        let mut f = crate::term::Frame::new(30, 90);
        app.draw_joincode(&mut f);
        let shown = f.text();

        assert!(shown.contains("Open by camera"), "{shown}");
        assert!(
            shown.contains("169.254.87.1"),
            "the address is not offered:\n{shown}"
        );
        assert!(
            !shown.contains("no password for it to put in a code"),
            "still the old dead end:\n{shown}"
        );
        // A drawn code, not just a promise of one. The renderer uses block
        // characters, so their presence is the evidence it got that far.
        assert!(
            shown.contains('\u{2588}') || shown.contains('\u{2584}') || shown.contains('\u{2580}'),
            "no code was drawn:\n{shown}"
        );
    }

    #[test]
    fn a_tick_in_another_folder_is_named_on_screen() {
        let d = crate::scratchdir::scratch("pick-elsewhere");
        std::fs::create_dir_all(d.join("here")).unwrap();
        std::fs::create_dir_all(d.join("away")).unwrap();
        std::fs::write(d.join("here/a-font.ttf"), b"x").unwrap();

        let mut app = App::new();
        app.pick_dir = d.join("here");
        app.picked = vec![d.join("here/a-font.ttf"), d.join("away")];

        let mut f = crate::term::Frame::new(40, 100);
        app.draw_pick(&mut f);
        let shown = f.text();

        assert!(shown.contains("SEND THE 2 TICKED"), "{shown}");
        assert!(
            shown.contains("in other folders"),
            "the screen does not account for the tick that is not on it:\n{shown}"
        );
        assert!(
            shown.contains("away"),
            "the tick held elsewhere is not named:\n{shown}"
        );
        // The KEY, not the sentence around it. This asserted the exact wording
        // and broke the moment that wording was shortened to fit the window,
        // which is a test failing for a reason that has nothing to do with
        // what it is guarding.
        assert!(
            shown.contains("c clears"),
            "no key is offered to take it off:\n{shown}"
        );
    }

    /// And that key has to work from anywhere, not only where the tick was made.
    #[test]
    fn c_clears_every_tick_including_ones_made_elsewhere() {
        let d = crate::scratchdir::scratch("pick-clear");
        std::fs::create_dir_all(d.join("here")).unwrap();
        std::fs::create_dir_all(d.join("away")).unwrap();
        let mut app = App::new();
        app.pick_dir = d.join("here");
        app.picked = vec![d.join("away"), d.join("here/x")];
        app.screen = Screen::Pick;

        app.pick_key(Key::Char('c'));
        assert!(app.picked.is_empty(), "c left ticks behind: {:?}", app.picked);
    }

    /// A capped walk must not be reported as the whole truth.
    ///
    /// serve::all_files() stops at MAX_FILES and says so, and the picker threw
    /// that away, so a folder of 150,000 files produced a confident
    /// "100000 files chosen" and 50,000 files that would never be sent.
    #[test]
    fn a_capped_folder_does_not_claim_to_be_the_whole_count() {
        let mut app = App::new();
        app.folder = "/somewhere".into();
        app.chosen = Some(["a".to_string(), "b".to_string()].into_iter().collect());

        app.pick_truncated = false;
        let honest = app.what_to_send();
        assert!(honest.contains("2 files chosen"), "{honest}");

        app.pick_truncated = true;
        let capped = app.what_to_send();
        assert!(
            capped.contains("more than") && capped.contains("only the first"),
            "a capped count is still presented as exact: {capped}"
        );
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

    // ---------------------------------------------------------- the picker
    //
    // These decide what a tick actually means, which is the difference between
    // sending four files and sending somebody's whole home folder.

    /// Two files in the same folder: that folder is the root.
    #[test]
    fn ticks_in_one_folder_serve_that_folder() {
        let ps = vec![
            PathBuf::from("/home/t/lessons/a.pdf"),
            PathBuf::from("/home/t/lessons/b.pdf"),
        ];
        assert_eq!(common_ancestor(&ps), Some(PathBuf::from("/home/t/lessons")));
    }

    /// Ticks in different branches raise the root far enough to cover both,
    /// and no further.
    #[test]
    fn ticks_in_two_branches_rise_to_the_fork_and_stop() {
        let ps = vec![
            PathBuf::from("/home/t/lessons/maths/a.pdf"),
            PathBuf::from("/home/t/lessons/science/deep/b.pdf"),
        ];
        assert_eq!(common_ancestor(&ps), Some(PathBuf::from("/home/t/lessons")));
    }

    /// Depth is not a limit. A file fifteen levels down is reachable and
    /// contributes its whole chain.
    #[test]
    fn depth_does_not_matter() {
        let deep = PathBuf::from("/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/file.txt");
        let ps = vec![deep.clone(), PathBuf::from("/a/b/other.txt")];
        assert_eq!(common_ancestor(&ps), Some(PathBuf::from("/a/b")));
    }

    /// One file alone serves its folder, never the file's own path: a root has
    /// to be a folder or there is nothing to walk.
    #[test]
    fn a_single_file_serves_the_folder_it_is_in() {
        let ps = vec![PathBuf::from("/home/t/lessons/only.pdf")];
        assert_eq!(common_ancestor(&ps), Some(PathBuf::from("/home/t/lessons")));
    }

    #[test]
    fn nothing_ticked_has_no_root() {
        assert_eq!(common_ancestor(&[]), None);
    }

    /// Relative paths use forward slashes whatever the platform, because that
    /// is what the server matches and what a URL carries. A backslash here
    /// means a file ticked on Windows is invisible on the page offering it.
    #[test]
    fn relative_paths_use_forward_slashes() {
        let root = PathBuf::from("/home/t/lessons");
        let full = PathBuf::from("/home/t/lessons/maths/week1/a.pdf");
        assert_eq!(relative_to(&root, &full), Some("maths/week1/a.pdf".to_string()));
    }

    #[test]
    fn a_path_outside_the_root_is_not_relative_to_it() {
        let root = PathBuf::from("/home/t/lessons");
        assert_eq!(relative_to(&root, &PathBuf::from("/etc/passwd")), None);
        // The root itself is not a file under the root.
        assert_eq!(relative_to(&root, &root), None);
    }
}
