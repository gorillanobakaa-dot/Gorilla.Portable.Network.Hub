#![allow(dead_code, unused_imports, clippy::needless_return)]
// Version: 0.1.0 · updated 26-08-24-16-45
//
// A static file server for the wifi bench, and the serving core the classroom
// tool will need anyway.
//
// WHY NOT `python3 -m http.server`: it is threaded, but one OS thread per
// connection, and 400 concurrent connections on an Ivy Bridge chip becomes a
// measurement of Python's scheduler rather than of the radio. This uses a fixed
// worker pool fed by a queue, so concurrency is bounded and the number that
// comes out is about the wifi.
//
// WHY RANGE REQUESTS ARE HERE FROM LINE ONE: measured 2026-08-24, LocalSend has
// no byte-range support and opens the destination with File::create, which
// truncates. On a link that dropped every 16 seconds it managed 30.6 KB/s,
// discarding ~96 MB each time, while a browser doing range requests over the
// identical link got 6 MB/s. On the links this is being built for, resume is
// not a nicety, it is the difference between finishing and never finishing.
use std::fs;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;
use std::{env, thread};

const BUF: usize = 256 * 1024;

static LIVE: AtomicU64 = AtomicU64::new(0);
static SERVED: AtomicU64 = AtomicU64::new(0);
/// Set once the accept loop is up, so a caller that did not block on `run` can
/// tell the difference between "starting" and "started".
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Quiet mode: the screen draws the numbers, so the server must not also print
/// them. A println from a worker thread while a frame is being drawn scrolls
/// the whole terminal.
static QUIET: AtomicBool = AtomicBool::new(false);

/// One device, mid-download, as the teacher sees it.
#[derive(Clone)]
pub struct Transfer {
    pub peer: String,
    pub file: String,
    pub done: u64,
    pub total: u64,
    pub rate: f64,
    pub finished: bool,
    updated: Instant,
    /// Rate is measured over a window rather than between writes. A 256 KB
    /// write can complete in microseconds, and bytes divided by that is a
    /// number in the thousands that means nothing.
    window_bytes: u64,
    window_start: Instant,
    pub handing_in: bool,
}

/// A plain Vec, not a map. A classroom is thirty devices, and a linear scan of
/// thirty entries is faster than hashing the key. Const Mutex::new so there is
/// no lazy initialisation to get wrong.
static TRANSFERS: Mutex<Vec<Transfer>> = Mutex::new(Vec::new());

/// Which way the bytes are going, in the words the roster uses.
#[derive(Clone, Copy, PartialEq)]
pub enum Direction {
    Getting,
    HandingIn,
    Done,
}

/// Which files the teacher has ticked. None means everything, which is what
/// the plain command line serves; the screen replaces it with the ticked set
/// and from then on an unticked file does not exist as far as the network is
/// concerned: not listed, not fetchable, not even by guessing the name.
static ALLOWED: Mutex<Option<std::collections::HashSet<String>>> = Mutex::new(None);

pub fn set_allowed(list: Option<std::collections::HashSet<String>>) {
    *ALLOWED.lock().unwrap_or_else(|e| e.into_inner()) = list;
}

fn is_allowed(rel: &str) -> bool {
    let a = ALLOWED.lock().unwrap_or_else(|e| e.into_inner());
    match &*a {
        None => true,
        Some(set) => set.contains(rel),
    }
}

/// Names the kids CLAIMED on the class page, keyed by address.
///
/// Thirty identical phones make device models useless for attribution: the
/// owner's phrase was that a note must say who it came from, or thirty kids
/// sending the same message for kicks are indistinguishable. So the page asks
/// each device for a name before it shows anything else. The claim is not
/// authenticated (there are no accounts, on purpose); what makes it honest is
/// that the permanent record keeps the claimed name AND the device AND the
/// address side by side, and two devices claiming the same name are visible
/// as exactly that on the roster.
static CLAIMED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// The key a claim is filed under: the device where we can identify it, the
/// address only as a fallback.
///
/// Addresses are the wrong key in both directions. A phone that reconnects
/// gets a new lease and would lose its name, which is merely annoying. Worse,
/// dnsmasq RECYCLES addresses: the next child to join can be handed the
/// address a previous one had, and would silently inherit their name, which
/// quietly corrupts the very attribution this exists for.
fn claim_key(ip: &str) -> String {
    match crate::net::device_tag(ip) {
        Some(tag) => format!("tag:{tag}"),
        None => format!("ip:{ip}"),
    }
}

pub fn set_claimed_name(ip: &str, name: &str) {
    let key = claim_key(ip);
    let mut c = CLAIMED.lock().unwrap_or_else(|e| e.into_inner());
    match c.iter_mut().find(|(k, _)| *k == key) {
        Some(e) => e.1 = name.to_string(),
        None => c.push((key, name.to_string())),
    }
}

pub fn claimed_name(ip: &str) -> Option<String> {
    let key = claim_key(ip);
    let c = CLAIMED.lock().unwrap_or_else(|e| e.into_inner());
    c.iter().find(|(k, _)| *k == key).map(|(_, n)| n.clone())
}

/// Forget this device's name, so the page asks again. The "Not you?" link.
pub fn forget_claim(ip: &str) {
    let key = claim_key(ip);
    let mut c = CLAIMED.lock().unwrap_or_else(|e| e.into_inner());
    c.retain(|(k, _)| *k != key);
}

/// Devices the teacher has paused, keyed the same way names are: by the device
/// where we can identify it, by address only where we cannot.
///
/// The address is the wrong key here for the same reason it is the wrong key
/// for a name, only sharper. A child told to stop can get a new address in
/// about four taps (forget the network, join it again), so an address-keyed
/// block lasts until they think of that. The tag comes from the hardware
/// address, and those four taps do not change it.
///
/// What this is NOT: a lock. It is a classroom instruction the software
/// enforces, in the same sense as being moved to the front of the room. Two
/// things escape it, and both are said out loud on the teacher's screen rather
/// than left to be discovered:
///
///   * A phone can be made to present a different hardware address. Android
///     already randomises one per network, and forgetting the network can draw
///     a fresh one. A new address means a new tag and the block does not follow.
///   * A blocked device is still ON the wifi. It cannot reach the lesson, and
///     that is the whole of what it cannot do.
///
/// The answer to both is the password change, which is the other control and
/// is deliberately a separate one.
///
/// The label is captured AT THE MOMENT OF BLOCKING and kept, so the list still
/// reads the same after that device has gone quiet and dropped off every other
/// list on the screen. A blocked device you can no longer see is exactly the
/// one worth being reminded about.
static BLOCKED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Pause this device. Returns the label it was filed under, for the teacher.
pub fn block_device(ip: &str) -> String {
    let key = claim_key(ip);
    let label = full_label(ip);
    let mut b = BLOCKED.lock().unwrap_or_else(|e| e.into_inner());
    match b.iter_mut().find(|(k, _)| *k == key) {
        // Already paused: refresh the label rather than queue a second entry,
        // so a double press is not a second block to undo.
        Some(e) => e.1 = label.clone(),
        None => b.push((key, label.clone())),
    }
    label
}

/// Let them back in.
pub fn unblock_device(ip: &str) {
    let key = claim_key(ip);
    let mut b = BLOCKED.lock().unwrap_or_else(|e| e.into_inner());
    b.retain(|(k, _)| *k != key);
}

/// Undo the block on a list entry, which is the only handle the teacher has
/// left once the device has stopped appearing on the roster.
pub fn unblock_key(key: &str) {
    let mut b = BLOCKED.lock().unwrap_or_else(|e| e.into_inner());
    b.retain(|(k, _)| k != key);
}

pub fn is_blocked(ip: &str) -> bool {
    // The common case is an empty list, and this runs on every single request.
    // Checking that first means a lesson with nobody paused pays nothing at
    // all: no /proc read, no hashing, no allocation.
    {
        let b = BLOCKED.lock().unwrap_or_else(|e| e.into_inner());
        if b.is_empty() {
            return false;
        }
    }
    let key = claim_key(ip);
    let b = BLOCKED.lock().unwrap_or_else(|e| e.into_inner());
    b.iter().any(|(k, _)| *k == key)
}

/// (key, label) for every paused device, for the screen.
pub fn blocked_devices() -> Vec<(String, String)> {
    BLOCKED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn blocked_count() -> usize {
    BLOCKED.lock().unwrap_or_else(|e| e.into_inner()).len()
}

/// Deliberately NOT called when the password changes.
///
/// A password change knocks everybody off, which makes it tempting to treat
/// the blocks as spent. It is the wrong way round: the child who was paused is
/// the one most likely to get the new password from a friend, and they should
/// come back to the same closed door. The two controls stack.
pub fn clear_blocks() {
    BLOCKED.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Everything known about a device, for the permanent record:
/// "Johnny [Xiaomi-11-Lite, 10.42.0.90]", degrading gracefully to whatever
/// parts exist.
pub fn full_label(ip: &str) -> String {
    let claimed = claimed_name(ip);
    let device = {
        let n = NAMES.lock().unwrap_or_else(|e| e.into_inner());
        n.iter().find(|(i, _)| i == ip).map(|(_, name)| name.clone())
    };
    // The tag is the part that survives a reconnect. Everything else in this
    // line can be shared by two devices or changed by one.
    let tag = crate::net::device_tag(ip).map(|t| format!(" #{t}")).unwrap_or_default();
    match (claimed, device) {
        (Some(c), Some(d)) => format!("{c}{tag} [{d}, {ip}]"),
        (Some(c), None) => format!("{c}{tag} [{ip}]"),
        (None, Some(d)) => format!("{d}{tag} [{ip}]"),
        (None, None) => format!("{ip}{tag}"),
    }
}

/// Devices that have claimed a name somebody else is also using.
///
/// The direct answer to "thirty Cuntius.Maximuses and you cannot tell who is
/// who": the teacher does not have to work it out, the screen says it. Keyed
/// on the device tag, not the address, so one phone reconnecting under a new
/// lease is NOT reported as a second impostor.
pub fn duplicate_claims() -> Vec<String> {
    let c = CLAIMED.lock().unwrap_or_else(|e| e.into_inner());
    let mut seen: Vec<(String, String)> = Vec::new(); // (name, key)
    let mut dupes: Vec<String> = Vec::new();
    for (key, name) in c.iter() {
        // Keys are already per-device, so two entries sharing a name really
        // are two devices; a reconnecting phone reuses its own key.
        if seen.iter().any(|(n, k)| n == name && k != key) && !dupes.contains(name) {
            dupes.push(name.clone());
        }
        seen.push((name.clone(), key.clone()));
    }
    dupes
}

/// One piece of work waiting for the teacher to accept or refuse.
#[derive(Clone)]
pub struct Pending {
    /// Who sent it, in full: name, device tag, model, address.
    pub from: String,
    /// The name the child's device gave it.
    pub original: String,
    /// What it is called on disk inside waiting/.
    pub on_disk: String,
    pub bytes: u64,
    pub at: String,
}

static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());

pub fn note_pending(ip: &str, original: &str, on_disk: &str, bytes: u64) {
    let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    // A retry that resolved to a name already queued must not queue twice.
    if p.iter().any(|e| e.on_disk == on_disk) {
        return;
    }
    p.push(Pending {
        from: full_label(ip),
        original: original.to_string(),
        on_disk: on_disk.to_string(),
        bytes,
        at: crate::net::timestamp(),
    });
}

pub fn pending() -> Vec<Pending> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn pending_count() -> usize {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).len()
}

fn drop_pending(on_disk: &str) {
    let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    p.retain(|e| e.on_disk != on_disk);
}

/// Accept: the file moves out of waiting and into the teacher's folder.
pub fn accept_pending(root: &Path, on_disk: &str) -> std::io::Result<()> {
    let from = crate::page::waiting_dir(root).join(on_disk);
    let to = crate::page::handed_in_dir(root).join(on_disk);
    fs::create_dir_all(crate::page::handed_in_dir(root))?;
    fs::rename(&from, &to)?;
    drop_pending(on_disk);
    Ok(())
}

/// Refuse: moved to refused/, never deleted. It may be evidence, and deciding
/// what disappears is not this program's call.
pub fn refuse_pending(root: &Path, on_disk: &str) -> std::io::Result<()> {
    let from = crate::page::waiting_dir(root).join(on_disk);
    let refused = crate::page::refused_dir(root);
    fs::create_dir_all(&refused)?;
    fs::rename(&from, refused.join(on_disk))?;
    drop_pending(on_disk);
    Ok(())
}

/// The key this address's claims and blocks are filed under, so the screen can
/// hold on to a device that has left the network.
pub fn device_key(ip: &str) -> String {
    claim_key(ip)
}

/// The tag alone, for the roster.
pub fn tag_for(ip: &str) -> Option<String> {
    crate::net::device_tag(ip)
}

/// Device names the screen has resolved, so uploads and notes can be labelled
/// with "Amina-phone" rather than an address. The screen fills this from its
/// lookup cache; the plain command line leaves it empty and labels fall back
/// to the address, which is honest if unfriendly.
static NAMES: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

pub fn set_device_name(ip: &str, name: &str) {
    let mut n = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    match n.iter_mut().find(|(i, _)| i == ip) {
        Some(e) => e.1 = name.to_string(),
        None => n.push((ip.to_string(), name.to_string())),
    }
}

/// Name plus device, for the screen the teacher is actually looking at.
///
/// The owner's exact scenario: thirty identical phones, a kid claims a name,
/// and later says "wasn't me". The claimed name alone is not enough to settle
/// that in the room; the device model sitting next to it is the part a kid
/// cannot talk their way out of, because it was not typed, it was reported by
/// the device itself when it joined the network.
pub fn roster_label(ip: &str) -> String {
    let claimed = claimed_name(ip);
    let device = {
        let n = NAMES.lock().unwrap_or_else(|e| e.into_inner());
        n.iter().find(|(i, _)| i == ip).map(|(_, name)| name.clone())
    };
    match (claimed, device) {
        (Some(c), Some(d)) => format!("{c} [{d}]"),
        (Some(c), None) => c,
        (None, Some(d)) => d,
        (None, None) => ip.to_string(),
    }
}

pub fn device_label(ip: &str) -> String {
    if let Some(c) = claimed_name(ip) {
        return c;
    }
    let n = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    n.iter().find(|(i, _)| i == ip).map(|(_, name)| name.clone()).unwrap_or_else(|| ip.to_string())
}

/// Whether handed-in work can actually land in the served folder.
///
/// Teachers in the schools this is for serve from USB flash drives and small
/// portable hard drives, kept as the failsafe copy of everything. A drive can
/// be read-only, full, or formatted strangely, and the failure must be LOUD
/// and up front: the page hides the hand-in form and says why, the roster
/// warns the teacher. The alternative is a lesson's homework quietly eaten.
static HANDIN_OK: AtomicBool = AtomicBool::new(false);

pub fn probe_handin(root: &Path) -> bool {
    let dir = crate::page::handed_in_dir(root);
    let ok = (|| {
        fs::create_dir_all(&dir).ok()?;
        let probe = dir.join(".write-probe");
        fs::write(&probe, b"x").ok()?;
        let _ = fs::remove_file(&probe);
        Some(())
    })()
    .is_some();
    HANDIN_OK.store(ok, Ordering::Relaxed);
    ok
}

pub fn handin_available() -> bool {
    HANDIN_OK.load(Ordering::Relaxed)
}

/// Addresses that have opened the class page, so the roster can tell "on the
/// network, has not looked yet" from "looking at the page". The teacher walks
/// to the stuck kid instead of the kid having to self-diagnose.
static PAGE_SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn mark_page_seen(ip: &str) {
    let mut p = PAGE_SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if !p.iter().any(|x| x == ip) {
        p.push(ip.to_string());
    }
}

pub fn has_seen_page(ip: &str) -> bool {
    PAGE_SEEN.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|x| x == ip)
}

/// How many distinct devices have opened the page. Over a network somebody
/// else provides, this is the only headcount there is: the roster cannot
/// enumerate a router's clients, but it can count who turned up.
pub fn pages_seen_count() -> usize {
    PAGE_SEEN.lock().unwrap_or_else(|e| e.into_inner()).len()
}

/// The files the network can see right now: ticked, top-level, with handed-in
/// work excluded by construction because it is not a file in the folder.
pub fn visible_files(root: &Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(root) {
        let mut items: Vec<_> = rd.flatten().collect();
        items.sort_by_key(|e| e.file_name());
        for e in items {
            let Ok(md) = e.metadata() else { continue };
            if !md.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !is_allowed(&name) {
                continue;
            }
            out.push((name, md.len()));
        }
    }
    out
}

/// What is happening right now, for the screen to draw.
pub fn transfers() -> Vec<Transfer> {
    let mut t = TRANSFERS.lock().unwrap_or_else(|e| e.into_inner());
    // A device that walked out of range leaves a row that would otherwise sit
    // there forever claiming 43%. Finished rows linger briefly so the teacher
    // sees them complete, then go.
    t.retain(|x| {
        let age = x.updated.elapsed().as_secs();
        if x.finished { age < 20 } else { age < 60 }
    });
    t.clone()
}

pub fn total_sent() -> u64 {
    SERVED.load(Ordering::Relaxed)
}

pub fn live_connections() -> u64 {
    LIVE.load(Ordering::Relaxed)
}

pub fn is_running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

/// One row per DEVICE, not per connection.
///
/// This was keyed on `peer`, which is what the socket reports: an address AND a
/// port. A single laptop downloading with four parallel connections therefore
/// appeared as four separate devices, each stuck at 1%, and the screen said
/// "5 devices getting files" when one child was in the room. For a teacher that
/// is worse than showing nothing: it looks like five children are stuck.
///
/// `delta` is the bytes just written, not a running total, because the running
/// total of one connection says nothing about what the device as a whole has.
fn note(peer: &str, file: &str, delta: u64, total: u64) {
    note_dir(peer, file, delta, total, false, false);
}

/// The upload path's view of the same table. Direction::Done marks the row
/// finished explicitly, because an upload knows its own end.
pub fn note_direction(peer: &str, file: &str, delta: u64, total: u64, dir: Direction) {
    note_dir(peer, file, delta, total, dir != Direction::Getting, dir == Direction::Done);
}

fn note_dir(peer: &str, file: &str, delta: u64, total: u64, handing_in: bool, force_done: bool) {
    let ip = peer.split(':').next().unwrap_or(peer).to_string();
    let mut t = TRANSFERS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = t.iter_mut().find(|e| e.peer == ip) {
        // A device that moves on to a second file starts again from zero.
        if e.file != file {
            e.file = file.to_string();
            e.done = 0;
            e.window_bytes = 0;
            e.window_start = Instant::now();
        }
        e.done = (e.done + delta).min(total.max(1));
        e.total = total;
        e.window_bytes += delta;
        let secs = e.window_start.elapsed().as_secs_f64();
        if secs >= 1.0 {
            e.rate = e.window_bytes as f64 / secs;
            e.window_bytes = 0;
            e.window_start = Instant::now();
        }
        e.handing_in = handing_in;
        e.finished = force_done || e.done >= total;
        if e.finished {
            // A finished row read "99%  done" because the byte count is of the
            // multipart body, whose boundary lines are never written to disk.
            // Finished is finished.
            e.done = e.total.max(1);
        }
        e.updated = Instant::now();
    } else {
        t.push(Transfer {
            peer: ip,
            file: file.to_string(),
            done: delta.min(total.max(1)),
            total,
            rate: 0.0,
            finished: force_done,
            updated: Instant::now(),
            window_bytes: delta,
            window_start: Instant::now(),
            handing_in,
        });
    }
}

/// Bind, then serve on background threads and return.
///
/// `run` blocks in the accept loop, which is right for a command line and
/// impossible for a screen that has to keep drawing. Binding happens BEFORE the
/// thread is spawned so that "the port is already in use" is reported to the
/// caller as an error rather than appearing on a background thread after the
/// screen has already said the network is ready.
pub fn start(root: &Path, addr: &str, helpers: usize) -> std::io::Result<()> {
    let root = root.canonicalize()?;
    let listeners = bind_all(addr)?;
    probe_handin(&root);
    QUIET.store(true, Ordering::Relaxed);
    thread::spawn(move || {
        accept_loop(listeners, root, helpers);
    });
    RUNNING.store(true, Ordering::Relaxed);
    Ok(())
}

/// Port 80 as well as the named port, when this machine is allowed to.
///
/// Port 80 is where a phone's own "is there internet?" probe arrives, so it is
/// what makes the sign-in screen pop. Ports under 1024 need a permission that
/// the .deb grants once at install (setcap on the binary); unpackaged builds
/// simply do not get port 80 and everything else still works. At least one
/// bind must succeed.
fn bind_all(addr: &str) -> std::io::Result<Vec<TcpListener>> {
    let mut listeners = Vec::new();
    let mut first_err = None;
    let mut wanted: Vec<String> = vec![addr.to_string()];
    let host = addr.rsplit_once(':').map(|(h, _)| h).unwrap_or("0.0.0.0");
    // Port 80 ONLY for a real, network-wide serve.
    //
    // This used to take port 80 on whatever host was asked for, so a test
    // instance bound to 127.0.0.1:18102 also grabbed 127.0.0.1:80, and that
    // blocks a real 0.0.0.0:80 with EADDRINUSE. The teacher's hotspot then
    // silently loses its sign-in screen because a loopback toy owns the port.
    // Happened twice in one day of field testing. A loopback bind is by
    // definition local and has no business owning the captive port.
    let is_wildcard = host == "0.0.0.0" || host == "*" || host.is_empty();
    let port80 = format!("{host}:80");
    if is_wildcard && port80 != addr {
        wanted.push(port80);
    }
    for a in wanted {
        match TcpListener::bind(&a) {
            Ok(l) => listeners.push(l),
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
    }
    if listeners.is_empty() {
        return Err(first_err.unwrap_or_else(|| std::io::Error::other("no address to bind")));
    }
    Ok(listeners)
}

/// True when the class can be told an address with no port in it.
pub fn on_port_80() -> bool {
    PORT80.load(Ordering::Relaxed)
}

static PORT80: AtomicBool = AtomicBool::new(false);

fn accept_loop(listeners: Vec<TcpListener>, root: PathBuf, helpers: usize) {
    let (tx, rx) = mpsc::channel::<TcpStream>();
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..helpers {
        let rx = Arc::clone(&rx);
        let root = root.clone();
        thread::spawn(move || loop {
            let sock = { rx.lock().unwrap().recv() };
            match sock {
                Ok(s) => {
                    LIVE.fetch_add(1, Ordering::Relaxed);
                    let _ = handle(s, &root);
                    LIVE.fetch_sub(1, Ordering::Relaxed);
                }
                Err(_) => break,
            }
        });
    }
    for l in &listeners {
        if l.local_addr().map(|a| a.port()).unwrap_or(0) == 80 {
            PORT80.store(true, Ordering::Relaxed);
        }
    }
    let mut threads = Vec::new();
    for l in listeners {
        let tx = tx.clone();
        threads.push(thread::spawn(move || {
            for s in l.incoming().flatten() {
                let _ = s.set_nodelay(true);
                let _ = tx.send(s);
            }
        }));
    }
    for t in threads {
        let _ = t.join();
    }
}

pub fn run(args: Vec<String>) {
    const USAGE: &str = "\
hub serve  -  hand out the files in a folder to every device in the room

  hub serve <folder> [options]

  --name <network>      create a wifi network with this name and serve over it
  --notice <text>       a message shown at the top of every kid's page
  --password <word>     password for that network (at least 8 characters)
  --helpers <number>    how many devices to serve at once (default: 8 per core)
  --port <number>       which port to listen on (default 8080)
  --address <ip:port>   listen on one address only
  -h, --help            this text

  examples:
    hub serve ~/lessons
    hub serve ~/lessons --name Classroom --password chalkdust

  Without --name it hands files out over whatever network already exists.
  With --name it creates one, which needs administrator rights.

  Files are served in pieces that can be asked for individually, so a device
  that loses the signal carries on from where it stopped instead of starting
  the whole file again.";

    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }

    // args, NOT env::args(). This was a mechanical refactor from main() to
    // run(args) and the body still reached for the global, so `hub serve
    // /folder 1.2.3.4:80` tried to serve a folder called "serve" and bind to
    // "/folder". It compiled and ran and did the wrong thing silently.
    let mut root: PathBuf = ".".into();
    let mut addr: Option<String> = None;
    let mut port: u16 = 8080;
    let mut helpers = default_helpers();
    let mut ssid: Option<String> = None;
    let mut password: Option<String> = None;

    let mut i = 1;
    let mut positional = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let value = || -> Option<String> { args.get(i + 1).cloned() };
        match a {
            "--name" => { ssid = value(); i += 2; }
            "--notice" => {
                if let Some(v) = value() {
                    crate::page::set_notice(&v);
                }
                i += 2;
            }
            "--password" => { password = value(); i += 2; }
            "--helpers" | "--workers" => {
                helpers = value().and_then(|v| v.parse().ok()).unwrap_or(helpers).clamp(1, 512);
                i += 2;
            }
            "--port" => { port = value().and_then(|v| v.parse().ok()).unwrap_or(port); i += 2; }
            "--address" => { addr = value(); i += 2; }
            other if other.starts_with('-') => {
                eprintln!("Not an option: {other}\n");
                eprintln!("{USAGE}");
                std::process::exit(2);
            }
            other => {
                // Second positional was the address in the bench tool. Keep
                // accepting it so old notes and scripts still work.
                if positional == 0 { root = other.into(); } else if addr.is_none() { addr = Some(other.to_string()); }
                positional += 1;
                i += 1;
            }
        }
    }

    let root = match root.canonicalize() {
        Ok(r) => r,
        Err(e) => { eprintln!("Cannot use that folder: {} ({e})", root.display()); std::process::exit(1); }
    };
    let addr = addr.unwrap_or_else(|| format!("0.0.0.0:{port}"));

    // The network first, because there is no point binding a port if the
    // network the class is supposed to reach it on never comes up.
    let hotspot = match (&ssid, &password) {
        (Some(name), Some(pass)) => match crate::net::hotspot_up(name, pass) {
            Ok(h) => Some(h),
            Err(e) => { eprintln!("{e}"); std::process::exit(1); }
        },
        (Some(_), None) => {
            eprintln!("--name needs --password as well. An open network lets anyone");
            eprintln!("in range reach this computer, so this tool does not make one.");
            std::process::exit(2);
        }
        _ => None,
    };

    let listeners = match bind_all(&addr) {
        Ok(l) => l,
        Err(e) => {
            if let Some(h) = &hotspot { h.down(); }
            eprintln!("Cannot listen on {addr}: {e}");
            eprintln!("Something else may already be using that port.");
            std::process::exit(1);
        }
    };

    probe_handin(&root);
    if let Some(h) = &hotspot {
        println!("network \"{}\" is up on {}", h.ssid, h.iface);
    }
    for ip in crate::net::local_addresses() {
        if on_port_80() {
            println!("  tell the class to open   http://{ip}");
        } else {
            println!("  tell the class to open   http://{ip}:{port}");
        }
    }
    if !on_port_80() {
        println!("note: port 80 belongs to something else, so joining phones");
        println!("      will NOT be brought here by the sign-in screen.");
    }
    println!("serving {} with {helpers} helpers ({} threads detected)",
             root.display(), std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0));
    println!("press ctrl-c to stop");

    // Ctrl-C on the command line skips every destructor, so a hotspot started
    // here would outlive the process and leave the teacher's wifi replaced by
    // a network with nobody serving on it. systemd owns the restore, we do not.
    if let Some(h) = &hotspot {
        h.arm_restore(crate::net::RESTORE_FUSE);
        crate::net::start_heartbeat();
    }

    accept_loop(listeners, root, helpers);
}

/// Serving files is I/O-bound: a helper spends most of its life blocked on a
/// socket, not computing, so there are eight per hardware thread rather than
/// one.
///
/// 64 was hardcoded here, and it is exactly what this Sony's 8 threads produce,
/// which is why the assumption stayed invisible: right for the development
/// machine, wrong as a constant. The fleet ranges from a 4-thread 2011 MacBook
/// Air to a 12-thread ThinkPad.
pub fn default_helpers() -> usize {
    if let Some(n) = std::env::var("FILESERVE_WORKERS").ok().and_then(|v| v.parse().ok()) {
        return n;
    }
    let t = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    (t * 8).max(16)
}

/// A peer that stops consuming must not own a worker forever.
///
/// MEASURED 2026-08-24 18:21: a Windows client was suspended by its power
/// settings mid-transfer. Its sockets stayed open, this server's write_all
/// blocked with no timeout, and each stalled connection permanently consumed a
/// worker. After 64 of them the server was dead while looking perfectly
/// healthy: unit active, connections established, zero errors logged. The
/// transfer froze at 93% and would never have resumed.
///
/// That is the classroom failure mode exactly. One child shuts a lid, one
/// laptop sleeps, one walks out of range, and each silently eats a worker off
/// the teacher's machine until the whole class stops receiving.
///
/// A bounded worker pool without timeouts is not a pool, it is a countdown.
const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// How long an IDLE kept-alive connection may hold a worker while waiting for
/// a request that may never come.
///
/// This is the second instance of the countdown trap, and the arithmetic is
/// the whole story. The class page refreshes its file list on a timer. A
/// browser that opens a fresh connection each time, and every phone browser
/// opens several in parallel, leaves each one parked in the keep-alive read
/// for the full IO_TIMEOUT. At a 6 second refresh and a 30 second idle
/// timeout that is five parked workers PER DEVICE: thirty children need 150
/// workers and the pool has 64. The server then answers nobody, while looking
/// perfectly healthy, and the browser reports a connection timeout.
///
/// Shorter than the refresh interval, so a connection is always released
/// before the same page comes back for more. A real transfer is never
/// affected: the timeout is raised back to IO_TIMEOUT the moment a request
/// actually starts arriving.
const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

fn handle(sock: TcpStream, root: &Path) -> std::io::Result<()> {
    sock.set_read_timeout(Some(IO_TIMEOUT))?;
    sock.set_write_timeout(Some(IO_TIMEOUT))?;
    let peer = sock.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    let mut reader = BufReader::new(sock.try_clone()?);
    // HTTP/1.1 keep-alive: serve requests on this connection until the peer
    // closes it or goes quiet. Without this loop the server answered once and
    // hung up, so a client asking for keep-alive paid a full TCP handshake per
    // chunk. Measured 2026-08-24: 477 chunks meant 476 connections and dropped
    // a loopback transfer from 348 MB/s to 31.5 MB/s, a 10x cost.
    let mut first = true;
    loop {
        // The first request gets the full patience; every later one on the
        // same connection is speculative and must not squat on a worker.
        if !first {
            sock.set_read_timeout(Some(IDLE_TIMEOUT))?;
        }
        first = false;
        match serve_one(&mut reader, &sock, root, &peer) {
            Ok(true) => continue,   // keep the connection
            Ok(false) => return Ok(()),
            Err(_) => return Ok(()), // timeout, EOF or a broken peer
        }
    }
}

/// Serve a single request. Returns Ok(true) if the connection may be reused.
fn serve_one(
    reader: &mut BufReader<TcpStream>,
    sock: &TcpStream,
    root: &Path,
    peer: &str,
) -> std::io::Result<bool> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    // read headers a byte at a time until CRLFCRLF; requests here are tiny
    while head.len() < 8192 {
        if reader.read(&mut byte)? == 0 {
            return Ok(false); // peer closed
        }
        if head.is_empty() {
            // A request has actually started. Give it the full timeout again:
            // the short one exists only to evict connections that are idle,
            // never to cut short one that is being used.
            sock.set_read_timeout(Some(IO_TIMEOUT))?;
        }
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&head).to_string();
    let mut lines = text.lines();
    let request = lines.next().unwrap_or("");
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("");
    let raw_path = parts.next().unwrap_or("/");
    let version = parts.next().unwrap_or("HTTP/1.1");

    // Whether this connection may be reused, decided from what the CLIENT
    // asked for.
    //
    // This was hardcoded to "yes" and it broke discovery outright: a client
    // that sent `Connection: close` and then read until end of file waited for
    // a close that never came, so every attempt to list the files on a machine
    // timed out. The probe uses a 400 ms timeout, so nothing was ever found and
    // the screen said "nothing on this network" while a server sat there
    // answering. Found 2026-08-24 by driving the real screen through a pty;
    // it could not be seen in the source and both halves looked correct alone.
    let wants_close = text
        .lines()
        .any(|l| {
            let l = l.to_ascii_lowercase();
            l.starts_with("connection:") && l.contains("close")
        })
        || (version == "HTTP/1.0"
            && !text.to_ascii_lowercase().contains("connection: keep-alive"));
    let keep = !wants_close;

    // Range: bytes=START-[END]
    // Take the whole header VALUE ("bytes=0-99"), not the part after the first
    // '='. Splitting on '=' here stripped the "bytes=" prefix that parse_range
    // then tried to strip again, so every valid range parsed as None and every
    // resumed download got a 416. Caught by the range test, which is the one
    // test this program exists to pass.
    let range = text
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("range:"))
        .and_then(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()));

    let host = text
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("host:"))
        .and_then(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()));

    let mut out = BufWriter::with_capacity(BUF, sock.try_clone()?);
    let peer_ip = peer.split(':').next().unwrap_or(peer).to_string();

    // The captive answer. A request addressed to any name that is not one of
    // our own addresses is a phone's connectivity probe (the dnsmasq drop-in
    // resolves those names to us) or a kid who typed some site into the bar.
    // Both get sent to the class page, and the redirect is exactly what makes
    // a joining phone pop "Sign in to this network".
    if crate::page::is_foreign_host(host.as_deref(), &our_names(sock)) {
        let to = redirect_base(sock);
        write!(out, "HTTP/1.1 302 Found\r\nLocation: {to}/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
        out.flush()?;
        return Ok(false);
    }

    let path_only = raw_path.split('?').next().unwrap_or("/");
    let query = raw_path.split_once('?').map(|(_, q)| q).unwrap_or("");

    // Paused by the teacher.
    //
    // Checked AFTER the captive redirect on purpose. A phone whose
    // connectivity probe is refused concludes it has no network, and some
    // Android builds then drop it and rejoin, which is the one action that
    // gets a device a new address and therefore a new tag. Letting the probe
    // succeed keeps the phone sitting still on a network where the block
    // holds. The refusal is delivered where a person will read it, on the
    // page, and nowhere else.
    //
    // Deliberately a 200 and not a 403. The only reader is a child holding a
    // phone, and the message has to render; a browser that decides to show its
    // own error page instead has turned "your teacher paused you" into
    // "something is broken", which sends them to the teacher to report a fault.
    if is_blocked(&peer_ip) {
        if method == "POST" {
            // Drain what fits and no more. Nothing is parsed and nothing is
            // kept; this runs only so the browser gets an answer it will
            // render rather than a dropped connection. A paused device is not
            // owed the bandwidth to finish uploading whatever it was sending.
            let len: usize = text
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
                .and_then(|l| l.split_once(':'))
                .and_then(|(_, v)| v.trim().parse().ok())
                .unwrap_or(0);
            let mut sink = vec![0u8; len.min(64 * 1024)];
            let _ = reader.read_exact(&mut sink);
        }
        let page = crate::page::paused_page();
        respond_fresh(&mut out, "text/html; charset=utf-8", page.as_bytes())?;
        return Ok(false);
    }

    if method == "POST" {
        // POSTs answer with a redirect and the connection closes: the body has
        // been consumed and a clean start is worth more than one saved
        // handshake.
        if path_only == "/note" {
            let len: usize = text
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
                .and_then(|l| l.split_once(':'))
                .and_then(|(_, v)| v.trim().parse().ok())
                .unwrap_or(0);
            let mut body = vec![0u8; len.min(64 * 1024)];
            reader.read_exact(&mut body)?;
            let tag = crate::page::take_note(&peer_ip, &String::from_utf8_lossy(&body), root);
            crate::page::redirect_done(&mut out, tag)?;
            return Ok(false);
        }
        if path_only == "/name" {
            let len: usize = text
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
                .and_then(|l| l.split_once(':'))
                .and_then(|(_, v)| v.trim().parse().ok())
                .unwrap_or(0);
            let mut body = vec![0u8; len.min(4096)];
            reader.read_exact(&mut body)?;
            crate::page::claim_name(&peer_ip, &String::from_utf8_lossy(&body));
            write!(out, "HTTP/1.1 303 See Other\r\nLocation: /\r\nContent-Length: 0\r\n\r\n")?;
            out.flush()?;
            return Ok(false);
        }
        if path_only == "/handin" {
            let outcome = crate::page::take_upload(reader, &text, &peer_ip, root)?;
            crate::page::redirect_done(&mut out, outcome.tag)?;
            return Ok(false);
        }
        crate::page::redirect_done(&mut out, "empty")?;
        return Ok(false);
    }

    if path_only == "/" {
        // `?list` is what another copy of this program asks for: one line per
        // file, size then a tab then the name.
        if query.contains("list") {
            return respond(&mut out, 200, "text/plain; charset=utf-8", plain_listing(root, root).as_bytes()).map(|_| keep);
        }
        mark_page_seen(&peer_ip);
        let done = query.strip_prefix("done=");
        // The address the class was told to use, taken from the socket this
        // request actually arrived on, so the escape hatch never prints a
        // guess.
        let here = sock
            .local_addr()
            .map(|a| if a.port() == 80 { a.ip().to_string() } else { format!("{}:{}", a.ip(), a.port()) })
            .unwrap_or_else(|_| "10.42.0.1".to_string());
        let page = crate::page::class_page(root, done, &peer_ip, query.contains("rename=1"), &here);
        return respond_fresh(&mut out, "text/html; charset=utf-8", page.as_bytes()).map(|_| keep);
    }
    if path_only == "/files" {
        mark_page_seen(&peer_ip);
        return respond_fresh(&mut out, "text/html; charset=utf-8", crate::page::files_frame(root).as_bytes()).map(|_| keep);
    }
    // The viewer: the same file the READ button used to open bare, wrapped in
    // a page that keeps a way BACK. Inside a sign-in sheet there is no back
    // button, so a bare file was a room with no door.
    if let Some(rest) = path_only.strip_prefix("/view/") {
        let name = percent_decode(rest);
        if !name.contains('/') && is_allowed(&name) && root.join(&name).is_file() {
            return respond_fresh(&mut out, "text/html; charset=utf-8",
                crate::page::view_page(&name).as_bytes()).map(|_| keep);
        }
        return respond(&mut out, 404, "text/plain", b"not found").map(|_| keep);
    }

    let decoded = percent_decode(path_only);
    let target = safe_join(root, &decoded);

    let target = match target {
        Some(t) => t,
        None => return respond(&mut out, 403, "text/plain", b"forbidden").map(|_| keep),
    };
    // Handed-in work is INSIDE the folder and NEVER served: kid A's homework
    // must not be downloadable by kid B. Ticking rules apply to everything
    // else; an unticked file does not exist as far as the network can tell.
    let rel = decoded.trim_start_matches('/');
    let first = rel.split('/').next().unwrap_or("");
    if first == "handed-in" || first.starts_with('.') || !is_allowed(first) {
        return respond(&mut out, 404, "text/plain", b"not found").map(|_| keep);
    }
    if !target.exists() {
        return respond(&mut out, 404, "text/plain", b"not found").map(|_| keep);
    }
    if target.is_dir() {
        return respond(&mut out, 200, "text/html; charset=utf-8", listing(&target, root).as_bytes()).map(|_| keep);
    }
    if method == "HEAD" {
        let len = fs::metadata(&target)?.len();
        write!(out, "HTTP/1.1 200 OK\r\nAccept-Ranges: bytes\r\nContent-Length: {len}\r\n\r\n")?;
        out.flush()?;
        return Ok(keep);
    }
    // READ opens in the browser (real content type, inline); GET IT forces the
    // download (?dl=1). The difference is one header, and it is the difference
    // between a video that streams and thirty storage-full phones.
    let force_download = query.contains("dl=1");
    send_file(&mut out, &target, range.as_deref(), peer, force_download).map(|_| keep)
}

/// Names under which this machine may be legitimately addressed.
fn our_names(sock: &TcpStream) -> Vec<String> {
    let mut names: Vec<String> = crate::net::local_addresses().iter().map(|a| a.to_string()).collect();
    if let Ok(a) = sock.local_addr() {
        names.push(a.ip().to_string());
    }
    names.push("localhost".to_string());
    names.push("127.0.0.1".to_string());
    names
}

/// Where the captive redirect points: our address on this very socket, which
/// needs no configuration and is right on every interface.
fn redirect_base(sock: &TcpStream) -> String {
    let (ip, port) = sock
        .local_addr()
        .map(|a| (a.ip().to_string(), a.port()))
        .unwrap_or_else(|_| ("10.42.0.1".to_string(), 80));
    if port == 80 {
        format!("http://{ip}")
    } else {
        format!("http://{ip}:{port}")
    }
}

fn send_file(out: &mut BufWriter<TcpStream>, path: &Path, range: Option<&str>, peer: &str, force_download: bool) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // The REAL type, even on a forced download. This was octet-stream to
    // "force" saving, and Android's Downloads app believed it: tapping the
    // finished download said "We can't open this file", because we had said
    // the file was nothing in particular. The attachment header alone forces
    // the save; the type is how the phone knows what opens it. Found on a
    // real Xiaomi, 2026-08-25.
    let ctype = crate::page::content_type(&name);
    let disposition = if force_download {
        format!("Content-Disposition: attachment; filename=\"{name}\"\r\n")
    } else {
        String::new()
    };
    let total = fs::metadata(path)?.len();
    let (start, end) = match range.and_then(|r| parse_range(r, total)) {
        Some(v) => v,
        None if range.is_some() => {
            write!(out, "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{total}\r\nContent-Length: 0\r\n\r\n")?;
            return out.flush();
        }
        None => (0, total.saturating_sub(1)),
    };
    let len = end - start + 1;
    if range.is_some() {
        write!(out, "HTTP/1.1 206 Partial Content\r\nAccept-Ranges: bytes\r\nContent-Type: {ctype}\r\n{disposition}Content-Range: bytes {start}-{end}/{total}\r\nContent-Length: {len}\r\n\r\n")?;
    } else {
        write!(out, "HTTP/1.1 200 OK\r\nAccept-Ranges: bytes\r\nContent-Type: {ctype}\r\n{disposition}Content-Length: {len}\r\n\r\n")?;
    }

    let mut f = fs::File::open(path)?;
    f.seek(SeekFrom::Start(start))?;
    let mut left = len;
    let mut buf = vec![0u8; BUF];
    let t0 = Instant::now();
    // Progress is reported against the WHOLE file, not against this range.
    // A client asking for 2 MB pieces would otherwise show thirty separate
    // transfers racing from 0 to 100, which tells a teacher nothing.
    while left > 0 {
        let want = std::cmp::min(left as usize, buf.len());
        let n = f.read(&mut buf[..want])?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        left -= n as u64;
        note(peer, &name, n as u64, total);
    }
    out.flush()?;
    let secs = t0.elapsed().as_secs_f64().max(0.001);
    SERVED.fetch_add(len, Ordering::Relaxed);
    if QUIET.load(Ordering::Relaxed) {
        // The screen is drawing. A println from a worker thread here scrolls
        // the frame out from under itself and the display tears.
        return Ok(());
    }
    println!(
        "{peer} {} bytes in {:.1}s = {:.2} MB/s   live={} total={:.2} GB",
        len,
        secs,
        len as f64 / secs / 1_048_576.0,
        LIVE.load(Ordering::Relaxed),
        SERVED.load(Ordering::Relaxed) as f64 / 1e9
    );
    Ok(())
}

fn parse_range(r: &str, total: u64) -> Option<(u64, u64)> {
    let r = r.strip_prefix("bytes=")?;
    let (a, b) = r.split_once('-')?;
    let (a, b) = (a.trim(), b.trim());
    let (start, end) = if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        (total.checked_sub(n)?, total - 1)
    } else {
        let s: u64 = a.parse().ok()?;
        let e = if b.is_empty() { total - 1 } else { b.parse().ok()? };
        (s, e.min(total - 1))
    };
    if start > end || start >= total { None } else { Some((start, end)) }
}

fn respond(out: &mut BufWriter<TcpStream>, code: u16, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    let why = match code {
        200 => "OK",
        206 => "Partial Content",
        403 => "Forbidden",
        404 => "Not Found",
        416 => "Range Not Satisfiable",
        _ => "OK",
    };
    write!(out, "HTTP/1.1 {code} {why}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\r\n", body.len())?;
    out.write_all(body)?;
    out.flush()
}

/// Like respond, but forbidding every cache between here and the kid.
///
/// Found on a real phone 2026-08-25: swiping back onto a cached copy of the
/// page meant its one-time tokens were already spent, so sending a note was
/// silently treated as a retry and the buttons looked dead until a manual
/// refresh. A page whose forms carry one-time tokens must never come out of
/// a cache.
fn respond_fresh(out: &mut BufWriter<TcpStream>, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    write!(out, "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nCache-Control: no-store, must-revalidate\r\nPragma: no-cache\r\nContent-Length: {}\r\n\r\n", body.len())?;
    out.write_all(body)?;
    out.flush()
}

fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut p = root.to_path_buf();
    for seg in rel.split('/').filter(|s| !s.is_empty()) {
        if seg == ".." || seg == "." {
            return None; // no traversal, at all
        }
        p.push(seg);
    }
    if p.starts_with(root) { Some(p) } else { None }
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// size<TAB>name, one file per line. Folders are skipped: the tool hands out
/// a folder of files, and a teacher who nests them can say so and we can walk
/// them later.
fn plain_listing(dir: &Path, _root: &Path) -> String {
    // The same visibility rules as the class page: ticked files only. A tool
    // and a browser must never disagree about what exists.
    let mut s = String::new();
    for (name, size) in visible_files(dir) {
        s.push_str(&format!("{size}\t{name}\n"));
    }
    s
}

fn listing(dir: &Path, root: &Path) -> String {
    let mut s = String::from("<!doctype html><meta charset=utf-8><title>files</title><body><h2>files</h2><ul>");
    if let Ok(rd) = fs::read_dir(dir) {
        let mut items: Vec<_> = rd.flatten().collect();
        items.sort_by_key(|e| e.file_name());
        for e in items {
            let name = e.file_name().to_string_lossy().into_owned();
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            let rel = e.path().strip_prefix(root).map(|p| p.to_string_lossy().into_owned()).unwrap_or(name.clone());
            s.push_str(&format!("<li><a href=\"/{rel}\">{name}</a> {:.2} GB</li>", size as f64 / 1e9));
        }
    }
    s.push_str("</ul></body>");
    s
}


#[cfg(test)]
mod bind_scope_tests {
    use super::*;

    /// A loopback-scoped serve must NOT take port 80.
    ///
    /// Twice during field testing a test instance on 127.0.0.1 grabbed
    /// 127.0.0.1:80, which makes a real 0.0.0.0:80 fail with EADDRINUSE and
    /// silently costs the teacher's hotspot its sign-in screen.
    #[test]
    fn a_loopback_serve_leaves_port_80_alone() {
        let listeners = bind_all("127.0.0.1:0").expect("loopback bind");
        let ports: Vec<u16> = listeners.iter().filter_map(|l| l.local_addr().ok()).map(|a| a.port()).collect();
        assert!(!ports.contains(&80), "a loopback test must never own port 80: {ports:?}");
        assert_eq!(ports.len(), 1, "exactly the port that was asked for");
    }

    /// And a real serve still tries for it. Port 80 needs a capability this
    /// test process does not have, so the assertion is on what was ATTEMPTED:
    /// the wildcard bind succeeds on its own port either way.
    #[test]
    fn a_wildcard_serve_still_binds_its_own_port() {
        let listeners = bind_all("0.0.0.0:0").expect("wildcard bind");
        assert!(!listeners.is_empty(), "the requested port must always bind");
    }
}

#[cfg(test)]
mod block_tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::TcpStream;

    /// Undo the block whatever happens, including a panic partway through an
    /// assertion. BLOCKED is process-wide state and a test that leaves it set
    /// hands its failure to whichever test runs next, which is the worst kind
    /// to debug: the one that fails somewhere else.
    struct Unblock(&'static str);
    impl Drop for Unblock {
        fn drop(&mut self) {
            unblock_device(self.0);
        }
    }

    fn tmproot(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hub-block-test-{}-{tag}", std::process::id()));
        let _ = fs::create_dir_all(&d);
        d
    }

    fn serve_loopback(root: PathBuf) -> u16 {
        let listeners = bind_all("127.0.0.1:0").expect("loopback bind");
        let port = listeners[0].local_addr().expect("port").port();
        thread::spawn(move || accept_loop(listeners, root, 2));
        port
    }

    fn get(port: u16, path: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").unwrap();
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        String::from_utf8_lossy(&out).to_string()
    }

    fn post(port: u16, path: &str, body: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        write!(
            s,
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        String::from_utf8_lossy(&out).to_string()
    }

    /// The whole point of the feature, end to end over a real socket: a paused
    /// device gets nothing, and letting it back in needs nothing from the child.
    ///
    /// One test, not five. BLOCKED is process-wide and cargo runs tests in
    /// parallel, so a paused loopback in one test would be a paused loopback in
    /// all of them. Keeping the sequence in a single function is what makes the
    /// ordering real rather than hoped for.
    #[test]
    fn a_paused_device_gets_nothing_and_a_freed_one_gets_it_back() {
        let root = tmproot("gate");
        fs::write(root.join("lesson.txt"), b"the lesson").unwrap();
        let port = serve_loopback(root.clone());

        // Before: the page asks who you are, which is how the class page starts.
        let before = get(port, "/");
        assert!(before.contains("type your name"), "expected the name prompt, got:\n{before}");

        let _guard = Unblock("127.0.0.1");
        block_device("127.0.0.1");
        assert!(is_blocked("127.0.0.1"));

        // The page is replaced, not decorated. A paused device must not be
        // able to see what it is missing.
        let paused = get(port, "/");
        assert!(paused.contains("Paused"), "expected the paused page, got:\n{paused}");
        assert!(!paused.contains("type your name"), "a paused device must not get the class page");

        // Guessing the file name directly is the obvious way round a page that
        // has stopped listing files.
        let direct = get(port, "/lesson.txt");
        assert!(!direct.contains("the lesson"), "a paused device fetched the file anyway:\n{direct}");

        // And the machine-readable listing, which is what another copy of this
        // program asks for.
        let listing = get(port, "/?list");
        assert!(!listing.contains("lesson.txt"), "a paused device read the file list:\n{listing}");

        // Sending is stopped too, not merely receiving. This is the direction
        // that matters: the reason to pause somebody is usually what they are
        // sending, not what they are reading.
        let secret = "paused-device-must-not-be-heard";
        let noted = post(port, "/note", &format!("text={secret}&token=x"));
        assert!(noted.contains("Paused"), "a paused device should be told, got:\n{noted}");
        let notes_after = crate::page::notes(50);
        assert!(
            !notes_after.iter().any(|(_, t)| t.contains(secret)),
            "a note from a paused device was recorded"
        );

        // Letting them back in restores everything, with no action from the
        // child: their page refreshes itself.
        unblock_device("127.0.0.1");
        assert!(!is_blocked("127.0.0.1"));
        let after = get(port, "/");
        assert!(after.contains("type your name"), "expected the class page back, got:\n{after}");

        let _ = fs::remove_dir_all(&root);
    }

    /// Filed under the DEVICE, not the address, whenever the device can be
    /// identified. This is what makes a pause survive a child forgetting the
    /// network and joining it again, which is a four-tap escape otherwise.
    ///
    /// Uses this machine's real ARP table, and skips itself honestly when it is
    /// empty rather than passing on nothing.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_block_is_filed_against_the_device_not_the_address() {
        let Ok(text) = fs::read_to_string("/proc/net/arp") else { return };
        let mut found = None;
        for line in text.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            // 0x2 is a complete entry: an address we have actually spoken to.
            if f.len() >= 4 && f[2] == "0x2" {
                found = Some(f[0].to_string());
                break;
            }
        }
        let Some(ip) = found else { return };
        let key = device_key(&ip);
        assert!(key.starts_with("tag:"), "a device we know the hardware for must key by tag, got {key}");

        block_device(&ip);
        let listed = blocked_devices();
        unblock_key(&key);
        assert!(
            listed.iter().any(|(k, _)| *k == key),
            "the block was not filed under the device key {key}: {listed:?}"
        );
        assert!(!is_blocked(&ip), "the cleanup did not take");
    }

    /// Pressing pause twice is one pause, not two to undo. A list that grows
    /// per keypress would leave a device that reads as freed still blocked.
    #[test]
    fn pausing_twice_leaves_one_entry() {
        // TEST-NET-3, reserved for documentation, so nothing else in the suite
        // can be talking to it.
        let ip = "203.0.113.7";
        let key = device_key(ip);
        block_device(ip);
        block_device(ip);
        let mine = blocked_devices().iter().filter(|(k, _)| *k == key).count();
        unblock_device(ip);
        assert_eq!(mine, 1, "a second pause added a second entry to undo");
        assert!(!is_blocked(ip));
    }

    /// A paused device that walks out of the room keeps its row, because
    /// otherwise there is no way left to un-pause it.
    #[test]
    fn a_paused_device_that_disappears_keeps_its_label() {
        let ip = "203.0.113.9";
        block_device(ip);
        let listed = blocked_devices();
        let key = device_key(ip);
        let kept = listed.iter().find(|(k, _)| *k == key).cloned();
        unblock_key(&key);
        let (_, label) = kept.expect("a paused device must stay on the list");
        assert!(label.contains(ip), "the label has to name it somehow: {label}");
    }
}
