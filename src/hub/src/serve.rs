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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;
use std::{env, thread};

/// Workers scale with the machine this is RUNNING on, not with the one it was
/// written on. 64 was hardcoded here, and it is exactly what this Sony's 8
/// threads produce, which is why the assumption stayed invisible: it was right
/// for the development machine and wrong as a constant. The fleet ranges from a
/// 4-thread 2011 MacBook Air to a 12-thread ThinkPad.
///
/// Eight per thread rather than one, because serving files is I/O-bound: a
/// worker spends most of its life blocked on a socket, not computing.
/// Override with FILESERVE_WORKERS.
fn workers() -> usize {
    if let Some(n) = std::env::var("FILESERVE_WORKERS").ok().and_then(|v| v.parse().ok()) {
        return n;
    }
    let t = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    (t * 8).max(16)
}
const BUF: usize = 256 * 1024;

static LIVE: AtomicU64 = AtomicU64::new(0);
static SERVED: AtomicU64 = AtomicU64::new(0);

pub fn run(args: Vec<String>) {
    // args, NOT env::args(). This was a mechanical refactor from main() to
    // run(args) and the body still reached for the global, so `hub serve
    // /folder 1.2.3.4:80` tried to serve a folder called "serve" and bind to
    // "/folder". It compiled and ran and did the wrong thing silently.
    let root: PathBuf = args.get(1).cloned().unwrap_or_else(|| ".".into()).into();
    let addr = args.get(2).cloned().unwrap_or_else(|| "0.0.0.0:8080".into());
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("fileserve  -  hand out the files in a folder over the network\n");
        println!("  fileserve <folder> [address:port]     default 0.0.0.0:8080\n");
        println!("  Serves byte ranges, so an interrupted download can resume.");
        return;
    }
    let root = match root.canonicalize() {
        Ok(r) => r,
        Err(e) => { eprintln!("Cannot use that folder: {} ({e})", root.display()); std::process::exit(1); }
    };
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Cannot listen on {addr}: {e}");
            eprintln!("Something else may already be using that port.");
            std::process::exit(1);
        }
    };
    let nworkers = workers();
    println!("serving {} on {} with {nworkers} workers ({} threads detected)",
             root.display(), addr, std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0));

    let (tx, rx) = mpsc::channel::<TcpStream>();
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..nworkers {
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
        None => return respond(&mut out, 403, "text/plain", b"forbidden").map(|_| true),
    };
    if !target.exists() {
        return respond(&mut out, 404, "text/plain", b"not found").map(|_| true);
    }
    if target.is_dir() {
        return respond(&mut out, 200, "text/html; charset=utf-8", listing(&target, root).as_bytes()).map(|_| true);
    }
    if method == "HEAD" {
        let len = fs::metadata(&target)?.len();
        write!(out, "HTTP/1.1 200 OK\r\nAccept-Ranges: bytes\r\nContent-Length: {len}\r\n\r\n")?;
        out.flush()?;
        return Ok(true);
    }
    send_file(&mut out, &target, range.as_deref(), peer).map(|_| true)
}

fn send_file(out: &mut BufWriter<TcpStream>, path: &Path, range: Option<&str>, peer: &str) -> std::io::Result<()> {
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
    while left > 0 {
        let want = std::cmp::min(left as usize, buf.len());
        let n = f.read(&mut buf[..want])?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        left -= n as u64;
    }
    out.flush()?;
    let secs = t0.elapsed().as_secs_f64().max(0.001);
    SERVED.fetch_add(len, Ordering::Relaxed);
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
