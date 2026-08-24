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
}

/// A plain Vec, not a map. A classroom is thirty devices, and a linear scan of
/// thirty entries is faster than hashing the key. Const Mutex::new so there is
/// no lazy initialisation to get wrong.
static TRANSFERS: Mutex<Vec<Transfer>> = Mutex::new(Vec::new());

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
        e.finished = e.done >= total;
        e.updated = Instant::now();
    } else {
        t.push(Transfer {
            peer: ip,
            file: file.to_string(),
            done: delta.min(total.max(1)),
            total,
            rate: 0.0,
            finished: false,
            updated: Instant::now(),
            window_bytes: delta,
            window_start: Instant::now(),
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
    let listener = TcpListener::bind(addr)?;
    QUIET.store(true, Ordering::Relaxed);
    thread::spawn(move || {
        accept_loop(listener, root, helpers);
    });
    RUNNING.store(true, Ordering::Relaxed);
    Ok(())
}

fn accept_loop(listener: TcpListener, root: PathBuf, helpers: usize) {
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
    for s in listener.incoming().flatten() {
        let _ = s.set_nodelay(true);
        let _ = tx.send(s);
    }
}

pub fn run(args: Vec<String>) {
    const USAGE: &str = "\
hub serve  -  hand out the files in a folder to every device in the room

  hub serve <folder> [options]

  --name <network>      create a wifi network with this name and serve over it
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

    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            if let Some(h) = &hotspot { h.down(); }
            eprintln!("Cannot listen on {addr}: {e}");
            eprintln!("Something else may already be using that port.");
            std::process::exit(1);
        }
    };

    if let Some(h) = &hotspot {
        println!("network \"{}\" is up on {}", h.ssid, h.iface);
    }
    for ip in crate::net::local_addresses() {
        println!("  tell the class to open   http://{ip}:{port}");
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

    accept_loop(listener, root, helpers);
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
    loop {
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
    let range = lines
        .find(|l| l.to_ascii_lowercase().starts_with("range:"))
        .and_then(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()));

    let mut out = BufWriter::with_capacity(BUF, sock.try_clone()?);
    let decoded = percent_decode(raw_path.split('?').next().unwrap_or("/"));
    let target = safe_join(root, &decoded);

    let target = match target {
        Some(t) => t,
        None => return respond(&mut out, 403, "text/plain", b"forbidden").map(|_| keep),
    };
    if !target.exists() {
        return respond(&mut out, 404, "text/plain", b"not found").map(|_| keep);
    }
    if target.is_dir() {
        // `?list` is what another copy of this program asks for: one line per
        // file, size then a tab then the name. The HTML index is for a phone
        // with only a browser, which is most of the room.
        if raw_path.contains("?list") {
            return respond(&mut out, 200, "text/plain; charset=utf-8", plain_listing(&target, root).as_bytes()).map(|_| keep);
        }
        return respond(&mut out, 200, "text/html; charset=utf-8", listing(&target, root).as_bytes()).map(|_| keep);
    }
    if method == "HEAD" {
        let len = fs::metadata(&target)?.len();
        write!(out, "HTTP/1.1 200 OK\r\nAccept-Ranges: bytes\r\nContent-Length: {len}\r\n\r\n")?;
        out.flush()?;
        return Ok(keep);
    }
    send_file(&mut out, &target, range.as_deref(), peer).map(|_| keep)
}

fn send_file(out: &mut BufWriter<TcpStream>, path: &Path, range: Option<&str>, peer: &str) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
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
        write!(out, "HTTP/1.1 206 Partial Content\r\nAccept-Ranges: bytes\r\nContent-Type: application/octet-stream\r\nContent-Range: bytes {start}-{end}/{total}\r\nContent-Length: {len}\r\n\r\n")?;
    } else {
        write!(out, "HTTP/1.1 200 OK\r\nAccept-Ranges: bytes\r\nContent-Type: application/octet-stream\r\nContent-Length: {len}\r\n\r\n")?;
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
    write!(out, "HTTP/1.1 {code} OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\r\n", body.len())?;
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

fn percent_decode(s: &str) -> String {
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
fn plain_listing(dir: &Path, root: &Path) -> String {
    let mut s = String::new();
    if let Ok(rd) = fs::read_dir(dir) {
        let mut items: Vec<_> = rd.flatten().collect();
        items.sort_by_key(|e| e.file_name());
        for e in items {
            let Ok(md) = e.metadata() else { continue };
            if !md.is_file() {
                continue;
            }
            let rel = e.path().strip_prefix(root).map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| e.file_name().to_string_lossy().into_owned());
            s.push_str(&format!("{}\t{}\n", md.len(), rel));
        }
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
