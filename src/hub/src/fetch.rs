#![allow(dead_code, unused_imports, clippy::needless_return)]
// Version: 0.1.0 · updated 26-08-24-17-10
//
// Parallel, RESUMABLE HTTP downloader. The client half of the bench, and the
// client the classroom tool needs.
//
// WHY IT EXISTS: measured 2026-08-24, LocalSend has no byte-range support and
// opens the destination with File::create, which truncates. On a link that
// dropped every 16 seconds it managed 30.6 KB/s, discarding ~96 MB on each
// drop; a browser doing range requests over the identical link got 6 MB/s.
// Resume is not a nicety on these links, it is the difference between finishing
// and never finishing. So it is here in the first version, not deferred.
//
// STATE: a `.parts` sidecar records which chunks are complete. Killing this
// mid-download and restarting it re-fetches only what is missing. There is no
// negotiation and no server-side state; the server only has to answer Range.
//
// NO TLS, deliberately. std has no TLS and pulling one in would multiply the
// binary size for a local-network transfer. The payload's confidentiality is
// the fountain layer's job, with a key handed out physically.
use crate::sha256;

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use std::{env, thread};

// 2 MB, not 8. The cost of a lost chunk is the whole chunk, and on a marginal
// classroom link (0.1 MB/s at the back of a room) 8 MB is 80 seconds thrown
// away where 2 MB is 20. On our measured 6 MB/s AP it is 0.3 seconds either way,
// so the small chunk costs nothing where the link is good and saves real time
// where it is not.
const CHUNK: u64 = 2 * 1024 * 1024;

// wget separates these for good reason and we should too: failing to CONNECT is
// a different problem from a transfer going quiet, and they deserve different
// patience. A dead peer refuses quickly; a slow one just needs time.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

// wget's --waitretry grows the wait linearly rather than hammering. A link that
// just dropped is not helped by 400 threads retrying instantly.
const RETRY_BASE_MS: u64 = 500;
const RETRY_MAX_MS: u64 = 15_000;

static DONE_BYTES: AtomicU64 = AtomicU64::new(0);
static TOTAL: AtomicU64 = AtomicU64::new(0);
static CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The screen is drawing; a println from a worker thread scrolls the frame out
/// from under itself and the display tears.
static QUIET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The last few things that happened, for the screen to show. Retries and
/// damaged pieces are exactly what a teacher needs to see and exactly what
/// scrolls past unread on a command line.
static MESSAGES: Mutex<Vec<(String, u32)>> = Mutex::new(Vec::new());
/// Counted rather than listed. Twelve identical retry lines fill a screen and
/// say exactly as much as one line saying twelve.
static RETRIES: AtomicU64 = AtomicU64::new(0);
static DAMAGED: AtomicU64 = AtomicU64::new(0);

/// (pieces asked for again, pieces that arrived damaged).
pub fn counts() -> (u64, u64) {
    (RETRIES.load(Ordering::Relaxed), DAMAGED.load(Ordering::Relaxed))
}

pub fn set_quiet(q: bool) {
    QUIET.store(q, Ordering::Relaxed);
    if q {
        MESSAGES.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

/// The recent messages, each with how many times it has just happened.
pub fn messages() -> Vec<(String, u32)> {
    MESSAGES.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

struct Url {
    host: String,
    port: u16,
    path: String,
}

fn parse_url(s: &str) -> Option<Url> {
    let rest = s.strip_prefix("http://")?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().ok()?),
        None => (hostport.to_string(), 80u16),
    };
    Some(Url { host, port, path: path.to_string() })
}

/// A connection held open across many requests.
///
/// Before this, every chunk opened its own TCP connection and sent
/// `Connection: close`. Measured 2026-08-24: an 8 GB transfer meant 954
/// connections and 954 handshakes, and dropping to 2 MB chunks would have made
/// it 4,000. Keep-alive makes fine-grained chunks nearly free, which is what
/// lets the chunk size be chosen for resume granularity instead of for
/// connection overhead.
struct Conn {
    r: BufReader<TcpStream>,
    w: TcpStream,
}

impl Conn {
    fn open(u: &Url) -> std::io::Result<Conn> {
        let addr = format!("{}:{}", u.host, u.port);
        let addrs: Vec<_> = std::net::ToSocketAddrs::to_socket_addrs(&addr)?.collect();
        let first = addrs.first().ok_or_else(|| std::io::Error::other("no address"))?;
        let sock = TcpStream::connect_timeout(first, CONNECT_TIMEOUT)?;
        sock.set_nodelay(true)?;
        sock.set_read_timeout(Some(READ_TIMEOUT))?;
        sock.set_write_timeout(Some(READ_TIMEOUT))?;
        Ok(Conn { r: BufReader::new(sock.try_clone()?), w: sock })
    }
}

/// One request on an existing connection. Returns (status, content_length).
/// The body is left in `c.r` for the caller to read exactly `len` bytes from.
fn request_on(c: &mut Conn, u: &Url, range: Option<(u64, u64)>) -> std::io::Result<(u16, u64)> {
    let mut req = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: keep-alive\r\nAccept-Encoding: identity\r\n",
        u.path, u.host
    );
    if let Some((a, b)) = range {
        req.push_str(&format!("Range: bytes={a}-{b}\r\n"));
    }
    req.push_str("\r\n");
    c.w.write_all(req.as_bytes())?;
    c.w.flush()?;

    let r = &mut c.r;
    let mut line = String::new();
    if r.read_line(&mut line)? == 0 {
        return Err(std::io::Error::other("peer closed the connection"));
    }
    let status: u16 = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut len = 0u64;
    let mut total_from_range = 0u64;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        let low = h.to_ascii_lowercase();
        if let Some(v) = low.strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
        if let Some(v) = low.strip_prefix("content-range:") {
            // bytes START-END/TOTAL
            if let Some(t) = v.rsplit('/').next() {
                total_from_range = t.trim().parse().unwrap_or(0);
                TOTAL.store(total_from_range, Ordering::Relaxed);
            }
        }
    }
    if total_from_range > 0 && range.is_none() {
        len = total_from_range;
    }
    Ok((status, len))
}

fn probe_total(u: &Url) -> std::io::Result<u64> {
    let mut c = Conn::open(u)?;
    let (status, len) = request_on(&mut c, u, Some((0, 0)))?;
    if status != 206 {
        eprintln!("warning: server answered {status} to a range request. Resume is NOT available.");
        return Ok(0);
    }
    // drain the one body byte so the connection stays usable
    let mut b = [0u8; 1];
    let _ = c.r.read_exact(&mut b[..len.min(1) as usize]);
    Ok(TOTAL.load(Ordering::Relaxed))
}

/// Keep the machine awake for the duration of a transfer.
///
/// MEASURED 2026-08-24 18:21: Windows suspended a client 27 minutes into an
/// 8 GB transfer because nothing told it not to. Nobody sits watching a
/// progress bar in a classroom, so a twenty-minute transfer with an untouched
/// mouse is the NORMAL case, not the edge case.
///
/// Windows: SetThreadExecutionState with ES_CONTINUOUS keeps the request
/// standing until it is cleared, which is why the Drop matters.
/// Linux: systemd-inhibit held by a child process, killed on Drop. Not elegant,
/// but it needs no dependencies and systemd is present on every target here.
struct StayAwake {
    #[cfg(unix)]
    child: Option<std::process::Child>,
    /// Held, never used. Dropping it, or the process dying for ANY reason,
    /// closes the pipe and lets the child exit. See the comment on new().
    #[cfg(unix)]
    _pipe: Option<std::process::ChildStdin>,
}

#[cfg(windows)]
mod winapi {
    extern "system" {
        pub fn SetThreadExecutionState(flags: u32) -> u32;
    }
    pub const ES_CONTINUOUS: u32 = 0x8000_0000;
    pub const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
    pub const ES_AWAYMODE_REQUIRED: u32 = 0x0000_0040;
}

impl StayAwake {
    #[cfg(windows)]
    fn new() -> StayAwake {
        unsafe {
            let r = winapi::SetThreadExecutionState(
                winapi::ES_CONTINUOUS | winapi::ES_SYSTEM_REQUIRED | winapi::ES_AWAYMODE_REQUIRED,
            );
            if r == 0 {
                eprintln!("warning: could not hold a wake lock; the machine may sleep mid-transfer");
            }
        }
        StayAwake {}
    }

    #[cfg(unix)]
    fn new() -> StayAwake {
        // The child runs `cat` with its stdin on a pipe we hold open, NOT
        // `sleep infinity`.
        //
        // Rust does not run Drop on SIGKILL, and a killed fetch would otherwise
        // orphan the inhibitor and keep the machine awake forever, quietly
        // draining a battery nobody can charge. A pipe needs no cooperation
        // from anybody: when this process dies for any reason at all, the
        // kernel closes the write end, `cat` sees EOF and exits, and
        // systemd-inhibit releases the lock as it goes.
        let mut child = match std::process::Command::new("systemd-inhibit")
            .args([
                "--what=sleep:idle",
                "--who=fetch",
                "--why=file transfer in progress",
                "--mode=block",
                "cat",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => {
                eprintln!("warning: systemd-inhibit unavailable; the machine may sleep mid-transfer");
                return StayAwake { child: None, _pipe: None };
            }
        };
        let pipe = child.stdin.take();
        StayAwake { child: Some(child), _pipe: pipe }
    }
}

impl Drop for StayAwake {
    #[cfg(windows)]
    fn drop(&mut self) {
        // Clearing the request is not optional. ES_CONTINUOUS persists for the
        // life of the process, so leaving it set would keep a laptop awake long
        // after the transfer ended, quietly draining a battery nobody can charge.
        unsafe {
            winapi::SetThreadExecutionState(winapi::ES_CONTINUOUS);
        }
    }

    #[cfg(unix)]
    fn drop(&mut self) {
        // Belt and braces. The pipe closing is what actually guarantees it;
        // this just makes a clean exit tidy up immediately rather than waiting
        // for the child to notice.
        self._pipe.take();
        if let Some(c) = self.child.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Read a chunk back off disk and digest it. Reading back rather than hashing
/// the buffer in flight is deliberate: it verifies what actually LANDED, which
/// is what a resumed download will later trust.
/// Re-hash every chunk the sidecar claims we already have, across all cores.
fn verify_existing(
    path: &str,
    done: &BTreeSet<u64>,
    total: u64,
    sums: &HashMap<u64, String>,
) -> Vec<u64> {
    let list: Vec<u64> = done.iter().copied().collect();
    let cores = thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let next = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let bad = Arc::new(Mutex::new(Vec::new()));
    let t0 = Instant::now();
    let mut hs = Vec::new();
    for _ in 0..cores {
        let (next, bad, list) = (Arc::clone(&next), Arc::clone(&bad), list.clone());
        let (path, sums) = (path.to_string(), sums.clone());
        hs.push(thread::spawn(move || {
            let Ok(mut f) = File::open(&path) else { return };
            loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= list.len() { break }
                let c = list[i];
                let start = c * CHUNK;
                let len = std::cmp::min(CHUNK, total.saturating_sub(start));
                if len == 0 { continue }
                match chunk_digest(&mut f, start, len) {
                    Ok(got) => {
                        if sums.get(&c).map(|e| e != &got).unwrap_or(false) {
                            bad.lock().unwrap().push(c);
                        }
                    }
                    Err(_) => bad.lock().unwrap().push(c),
                }
            }
        }));
    }
    for h in hs { let _ = h.join(); }
    let v = Arc::try_unwrap(bad).unwrap().into_inner().unwrap();
    println!("  verified {} existing chunks in {:.1}s across {cores} cores",
             list.len(), t0.elapsed().as_secs_f64());
    v
}

fn chunk_digest(fh: &mut File, start: u64, len: u64) -> std::io::Result<String> {
    // Stream it in 256 KB pieces rather than allocating the whole chunk.
    // Allocating `len` per verifying thread meant 2 MB each, and the weakest
    // machine in the fleet is a 2011 MacBook Air with 4 GB: at the old default
    // that is hundreds of megabytes of transient buffers for no reason.
    let mut f = fh.try_clone()?;
    f.seek(SeekFrom::Start(start))?;
    let mut h = sha256::Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut left = len;
    while left > 0 {
        let want = std::cmp::min(left as usize, buf.len());
        f.read_exact(&mut buf[..want])?;
        h.update(&buf[..want]);
        left -= want as u64;
    }
    Ok(sha256::hex(&h.finish()))
}

fn fetch_sums(u: &Url) -> std::io::Result<HashMap<u64, String>> {
    let su = Url { host: u.host.clone(), port: u.port, path: format!("{}.sums", u.path) };
    let mut c = Conn::open(&su)?;
    let (status, len) = request_on(&mut c, &su, None)?;
    let mut m = HashMap::new();
    if status != 200 { return Ok(m); }
    // The length comes from the other machine, and this allocates it. A
    // fingerprint file holds one short line per 2 MB piece, so 8 MB covers a
    // download of about 230 GB; anything larger is a wrong answer or a server
    // that means harm, and on a classroom laptop with 2 GB of memory an
    // unbounded allocation here is the whole machine.
    //
    // Found 2026-08-24: a test server that answered every path with the file
    // itself made this try to read 400 MB as a fingerprint list, and the
    // download sat at 0 bytes with no message for as long as anyone watched.
    const SUMS_LIMIT: u64 = 8 * 1024 * 1024;
    if len > SUMS_LIMIT {
        log(&format!("ignoring a fingerprint list of {len} bytes, which is not a fingerprint list"));
        return Ok(m);
    }
    let mut body = vec![0u8; len as usize];
    c.r.read_exact(&mut body)?;
    for line in String::from_utf8_lossy(&body).lines() {
        if line.starts_with('#') { continue }
        if let Some((i, h)) = line.split_once(' ') {
            if let Ok(i) = i.parse::<u64>() { m.insert(i, h.trim().to_string()); }
        }
    }
    Ok(m)
}

/// Sweep the worker count and print the result.
///
/// Built INTO the binary rather than shipped as a PowerShell script, because a
/// .ps1 is blocked by Windows' execution policy and the answer to that is not
/// to teach a teacher about `-ExecutionPolicy Bypass`. An .exe they already
/// downloaded just runs.
///
/// It re-invokes itself per worker count rather than looping internally, so
/// each run starts from a genuinely clean state: fresh sockets, fresh threads,
/// no leftover .parts, nothing carried over that could flatter a later run.
fn run_bench(url: &str, out: &str) {
    let exe = match env::current_exe() {
        Ok(e) => e,
        Err(e) => { eprintln!("cannot find own path: {e}"); return }
    };
    let counts = [1usize, 2, 4, 8, 16, 32];
    println!("worker sweep against {url}");
    println!("  each run is a separate process with a clean state\n");
    let mut rows: Vec<(usize, f64, f64)> = Vec::new();
    for n in counts {
        let _ = fs::remove_file(out);
        let _ = fs::remove_file(format!("{out}.parts"));
        let t0 = Instant::now();
        let st = std::process::Command::new(&exe)
            .args([url, "-n", &n.to_string(), "-o", out])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let secs = t0.elapsed().as_secs_f64();
        let bytes = fs::metadata(out).map(|m| m.len()).unwrap_or(0);
        match st {
            Ok(s) if s.success() && bytes > 0 => {
                let mbps = bytes as f64 / secs / 1_048_576.0;
                println!("  {n:>3} workers : {secs:>6.1}s = {mbps:>6.2} MB/s");
                rows.push((n, secs, mbps));
            }
            _ => println!("  {n:>3} workers : FAILED"),
        }
        thread::sleep(std::time::Duration::from_secs(3));
    }
    let _ = fs::remove_file(out);
    let _ = fs::remove_file(format!("{out}.parts"));
    if let Some(best) = rows.iter().max_by(|a, b| a.2.total_cmp(&b.2)) {
        println!("\n  fastest: {} workers at {:.2} MB/s", best.0, best.2);
        if let Some(one) = rows.iter().find(|r| r.0 == 1) {
            println!("  against 1 worker at {:.2} MB/s -> {:+.1}%",
                     one.2, (best.2 - one.2) / one.2 * 100.0);
        }
    }
    if let Ok(mut f) = fs::File::create("bench-workers.csv") {
        let _ = writeln!(f, "workers,seconds,MB_per_s");
        for (n, s, m) in &rows { let _ = writeln!(f, "{n},{s:.2},{m:.3}"); }
        println!("  written: bench-workers.csv");
    }
}

fn backoff(fails: u32) -> std::time::Duration {
    let ms = (RETRY_BASE_MS * fails as u64).min(RETRY_MAX_MS);
    std::time::Duration::from_millis(ms)
}

/// Everything goes to stdout AND to `fetch.log`, always, unasked.
///
/// Tonight a run was captured with `fetch.exe > test.txt`, which takes stdout
/// and silently discards stderr, where every retry message was going. A person
/// handing out files should not have to know what a stream is to end up with a
/// usable record of what happened.
/// Same sentence, different numbers.
fn same_shape(a: &str, b: &str) -> bool {
    let strip = |s: &str| s.chars().filter(|c| !c.is_ascii_digit()).collect::<String>();
    strip(a) == strip(b)
}

fn log(msg: &str) {
    if QUIET.load(Ordering::Relaxed) {
        let mut m = MESSAGES.lock().unwrap_or_else(|e| e.into_inner());
        // Two messages that differ only in a piece number are the same event
        // happening again, so they collapse to one line and a count. Without
        // this, a link that is dropping fills the whole screen with the same
        // sentence and hides everything else on it.
        if let Some(last) = m.last_mut() {
            if same_shape(&last.0, msg) {
                last.1 += 1;
                last.0 = msg.to_string();
                return;
            }
        }
        // Bounded: a transfer that retries for an hour must not grow a log in
        // memory on a machine that has 2 GB of it.
        if m.len() >= 40 {
            m.remove(0);
        }
        m.push((msg.to_string(), 1));
        return;
    } else {
        println!("{msg}");
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("fetch.log") {
        let _ = writeln!(f, "{msg}");
    }
}

pub fn run(args: Vec<String>) {
    
    // NEVER panic on something a person typed.
    //
    // `fetch --help` used to end in a Rust stack trace, because the URL was
    // parsed with .expect(). The audience for this is a teacher, and a panic
    // message is not an error message: it says nothing about what to do, and it
    // looks like the program is broken rather than the input.
    const USAGE: &str = "\
fetch  -  download a file, resuming if it was interrupted

  fetch <url> [options]

  -n <number>     parallel connections (default 4, measured best on wifi)
  -o <file>       where to save it (default: the name from the url)
  --verify on     check every piece against the server's .sums file
  --bench on      try 1/2/4/8/16/32 connections and report which is fastest
  -h, --help      this text

  example:
    fetch http://10.42.0.1:8080/lessons.zip -o lessons.zip

  If it is interrupted, run the SAME command again. It continues from where
  it stopped rather than starting over.";

    if args.len() < 2 || args[1] == "-h" || args[1] == "--help" {
        println!("{USAGE}");
        std::process::exit(if args.len() < 2 { 2 } else { 0 });
    }
    let url = match parse_url(&args[1]) {
        Some(u) => u,
        None => {
            eprintln!("That does not look like a web address: {}", args[1]);
            eprintln!("It needs to start with http://  for example:");
            eprintln!("  fetch http://10.42.0.1:8080/lessons.zip");
            std::process::exit(1);
        }
    };
    // FOUR, and it is measured rather than guessed.
    //
    // Swept 1/2/4/8/16/32 back to back over a real 802.11n link on 2026-08-24,
    // same file, same conditions, each run a separate process:
    //
    //     conns   mean   median   peak    sd
    //         1   6.51    6.65    7.00   0.61
    //         2   6.55    6.66    7.04   0.50
    //         4   6.57    6.64    7.04   0.45   <- best mean AND steadiest
    //         8   6.36    6.44    7.02   0.58
    //        16   6.14    6.37    6.81   0.78
    //        32   6.20    6.46    6.91   0.76
    //
    // The peak is 7.0 in EVERY row: the ceiling is airtime and no amount of
    // threading creates more of it. Above four it gets slower and noticeably
    // noisier. Four exists for resilience and 2 MB granularity, not speed: a
    // stalled chunk must not halt everything.
    //
    // Do NOT scale this with core count. That is a wide-area-network habit, for
    // links where one stream cannot fill the pipe. A classroom is the opposite,
    // and thirty children at 32 workers each would put 960 sockets on the
    // teacher's laptop to do a job that 120 does better.
    //
    // Contrast the hashing case in `sums`, where threads SHOULD track cores,
    // because that work is CPU-bound. Same program, opposite answers.
    let mut workers = 4usize;
    let mut out = url.path.rsplit('/').next().unwrap_or("download").to_string();
    let mut verify = false;
    let mut bench = false;
    let mut i = 2;
    while i + 1 < args.len() {
        match args[i].as_str() {
            "-n" => workers = args[i + 1].parse().unwrap_or(8),
            "-o" => out = args[i + 1].clone(),
            "--verify" | "-v" => { verify = args[i+1] != "off"; }
            "--bench" => { bench = args[i+1] != "off"; }
            _ => {}
        }
        i += 2;
    }

    if bench {
        run_bench(&args[1], &out);
        return;
    }

    match download(&args[1], &out, workers, verify) {
        Ok(rate) => println!("done: {:.2} MB/s", rate),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

/// Fetch a file, resuming whatever is already on disk.
///
/// Returns megabytes per second, or a sentence a teacher can act on. It does
/// NOT exit the process and it does NOT print progress: both of those belong to
/// the caller, because the same function is driven from a command line and from
/// a screen that is redrawing four times a second.
pub fn download(url_str: &str, out: &str, workers: usize, verify: bool) -> Result<f64, String> {
    let url = parse_url(url_str).ok_or_else(|| format!("That is not a web address: {url_str}"))?;

    // Held until main returns, so it covers the entire transfer and is
    // released automatically however we exit.
    let _awake = StayAwake::new();

    let total = probe_total(&url).map_err(|e| describe(&e))?;
    if total == 0 {
        return Err("That computer answered, but not with a file. \
                    Check the name is right.".into());
    }
    TOTAL.store(total, Ordering::Relaxed);
    DONE_BYTES.store(0, Ordering::Relaxed);
    CANCEL.store(false, Ordering::Relaxed);
    RETRIES.store(0, Ordering::Relaxed);
    DAMAGED.store(0, Ordering::Relaxed);
    // Per-chunk digests, if the server offers them. A mismatch requeues that
    // chunk alone; there is no whole-file rehash and no all-or-nothing verdict.
    let sums: Arc<HashMap<u64, String>> = Arc::new(if verify {
        match fetch_sums(&url) {
            Ok(m) if !m.is_empty() => { log(&format!("checking every piece against {} fingerprints", m.len())); m }
            _ => { log("no fingerprints offered, so the pieces cannot be checked"); HashMap::new() }
        }
    } else { HashMap::new() });

    let chunks: u64 = total.div_ceil(CHUNK);
    log(&format!("{:.1} MB in {} pieces, {} at a time",
                 total as f64 / 1e6, chunks, workers));

    // Preallocate, then load whatever a previous run finished.
    // How much is actually on disk, read BEFORE the file is grown to full size.
    // After set_len the file is `total` bytes of mostly zeroes and there is no
    // way left to tell what really arrived.
    let existing_len = fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    let f = OpenOptions::new().create(true).write(true).read(true).open(out)
        .map_err(|e| format!("Cannot write to {out}: {e}"))?;
    f.set_len(total).map_err(|e| format!("Not enough room on the disk for {out}: {e}"))?;
    let parts_path = format!("{out}.parts");
    let mut done: BTreeSet<u64> = fs::read_to_string(&parts_path)
        .map(|s| s.split_whitespace().filter_map(|t| t.parse().ok()).collect())
        .unwrap_or_default();
    if !done.is_empty() {
        // A piece cannot be complete if the file was never long enough to hold
        // it. This costs one stat and needs nothing from the server, so unlike
        // the digest check below it always runs.
        //
        // Found 2026-08-24 by a deliberately wrong test: a file truncated to
        // 1,000,000 bytes with a sidecar claiming both 2 MB pieces were done
        // was reported complete, at 2,861 MB/s, having fetched nothing. A
        // sidecar is a CLAIM. Anything checkable about it should be checked.
        let before = done.len();
        done.retain(|c| existing_len >= std::cmp::min((c + 1) * CHUNK, total));
        if done.len() != before {
            log(&format!("{} pieces were recorded but not actually here, fetching them again",
                         before - done.len()));
        }
    }
    if !done.is_empty() {
        log(&format!("carrying on: {} of {} pieces are already here", done.len(), chunks));
        // VERIFY WHAT WE ALREADY HAVE, do not merely trust the sidecar.
        //
        // Found 2026-08-24 by a test that corrupted a chunk on disk, marked it
        // complete in .parts, and watched the corruption survive untouched: a
        // resumed download re-verified nothing, because verification only ran
        // on chunks it had just fetched. The whole reason for per-chunk digests
        // is that a partial file on disk is UNTRUSTED, and a .parts file is a
        // claim rather than proof.
        //
        // Parallel because it is pure CPU work over independent chunks, which
        // is the same reason `sums` exists.
        if !sums.is_empty() {
            let bad = verify_existing(&out, &done, total, &sums);
            if !bad.is_empty() {
                log(&format!("{} of those were damaged and will be fetched again", bad.len()));
                for c in &bad { done.remove(c); }
            }
        }
        DONE_BYTES.fetch_add(done.len() as u64 * CHUNK, Ordering::Relaxed);
    }

    // The sidecar is rewritten from the VERIFIED set, so a chunk that failed
    // is not silently trusted again by the next run.
    {
        let mut p = File::create(&parts_path).map_err(|e| format!("Cannot write {parts_path}: {e}"))?;
        for c in &done { let _ = writeln!(p, "{c}"); }
    }

    let queue: Vec<u64> = (0..chunks).filter(|c| !done.contains(c)).collect();
    let queue = Arc::new(Mutex::new(queue));
    let done = Arc::new(Mutex::new(done));
    let parts_file = Arc::new(Mutex::new(
        OpenOptions::new().create(true).append(true).open(&parts_path)
            .map_err(|e| format!("Cannot write {parts_path}: {e}"))?,
    ));

    let t0 = Instant::now();
    let mut handles = Vec::new();
    for _ in 0..workers {
        let (q, d, pf) = (Arc::clone(&queue), Arc::clone(&done), Arc::clone(&parts_file));
        let sums = Arc::clone(&sums);
        let (host, port, path) = (url.host.clone(), url.port, url.path.clone());
        let outfile = out.to_string();
        handles.push(thread::spawn(move || {
            let u = Url { host, port, path };
            // read(true) as well as write(true): chunk_digest reads the bytes
            // back off disk to verify what actually landed, and a write-only
            // handle fails that with EBADF. It did, silently, on every chunk,
            // which meant download-path verification never ran at all.
            let mut fh = OpenOptions::new().read(true).write(true).open(&outfile)
                .expect("open for read+write");
            // One connection per worker, held across chunks and reopened only
            // when it breaks. Before this, every chunk cost a fresh TCP
            // handshake: 954 of them for an 8 GB transfer at the old chunk size,
            // and 4,000 at the new one.
            let mut conn: Option<Conn> = None;
            // wget's --waitretry shape: the wait grows with consecutive
            // failures and resets on success, so a link that has just dropped
            // is not hammered by every worker at once.
            let mut fails: u32 = 0;
            loop {
                // Stopping is "stop taking work", not "kill the thread". A
                // half-written chunk left behind by a killed thread would be
                // recorded as done, and the damage would only show up the next
                // time someone opened the file.
                if CANCEL.load(Ordering::Relaxed) {
                    break;
                }
                let chunk = { q.lock().unwrap().pop() };
                let Some(c) = chunk else { break };
                let start = c * CHUNK;
                let end = std::cmp::min(start + CHUNK, total) - 1;

                if conn.is_none() {
                    match Conn::open(&u) {
                        Ok(k) => conn = Some(k),
                        Err(e) => {
                            RETRIES.fetch_add(1, Ordering::Relaxed);
                            log(&format!("lost contact ({}), waiting to try again", plain_error(&e)));
                            q.lock().unwrap().push(c);
                            fails += 1;
                            thread::sleep(backoff(fails));
                            continue;
                        }
                    }
                }

                let res = fetch_chunk(conn.as_mut().unwrap(), &u, start, end, &mut fh);
                match res {
                    Ok(n) => {
                        if let Some(expect) = sums.get(&c) {
                            match chunk_digest(&mut fh, start, n) {
                                Ok(got) if &got == expect => {}
                                Ok(got) => {
                                    fails += 1;
                                    DAMAGED.fetch_add(1, Ordering::Relaxed);
                                    log(&format!("piece {c} arrived damaged (fingerprint {} not {}), asking again",
                                                 &got[..8], &expect[..8]));
                                    q.lock().unwrap().push(c);
                                    thread::sleep(backoff(fails));
                                    continue;
                                }
                                Err(e) => {
                                    // Not a shrug. If verification was asked
                                    // for and cannot run, the user must be told
                                    // plainly rather than handed a file we did
                                    // not actually check.
                                    log(&format!("chunk {c} COULD NOT BE VERIFIED ({e}) \
                                                  -- verification is not working, do not trust this file"));
                                }
                            }
                        }
                        fails = 0;
                        DONE_BYTES.fetch_add(n, Ordering::Relaxed);
                        d.lock().unwrap().insert(c);
                        let mut p = pf.lock().unwrap();
                        let _ = writeln!(p, "{c}");
                        let _ = p.flush();
                    }
                    Err(e) => {
                        // The connection is of unknown state now; drop it and
                        // reconnect rather than trying to reuse a broken one.
                        conn = None;
                        fails += 1;
                        RETRIES.fetch_add(1, Ordering::Relaxed);
                        log(&format!("piece {c} did not arrive ({}), asking again",
                                     plain_error(&e)));
                        q.lock().unwrap().push(c);
                        thread::sleep(backoff(fails));
                    }
                }
            }
        }));
    }

    // No progress thread here. On the command line `run` prints; on the screen
    // the draw loop reads DONE_BYTES itself. A library that prints is a library
    // that fights whatever is drawing.
    for h in handles {
        let _ = h.join();
    }
    let secs = t0.elapsed().as_secs_f64().max(0.001);
    if CANCEL.load(Ordering::Relaxed) {
        // The .parts file stays. That is the entire point: the next run reads
        // it and continues instead of starting the file again.
        return Err("Stopped. Run the same thing again to carry on from here.".into());
    }
    let got = DONE_BYTES.load(Ordering::Relaxed);
    if got < total {
        return Err(format!(
            "Only {} of {} bytes arrived. The signal was lost. \
             Ask for it again and it will carry on from here.",
            got, total));
    }
    let _ = fs::remove_file(&parts_path);
    Ok(total as f64 / secs / 1_048_576.0)
}

fn fetch_chunk(c: &mut Conn, u: &Url, start: u64, end: u64, fh: &mut File) -> std::io::Result<u64> {
    let (status, len) = request_on(c, u, Some((start, end)))?;
    if status != 206 {
        return Err(std::io::Error::other(format!("expected 206, got {status}")));
    }
    let r = &mut c.r;
    let mut buf = vec![0u8; 256 * 1024];
    let mut written = 0u64;
    let mut pos = start;
    while written < len {
        let want = std::cmp::min((len - written) as usize, buf.len());
        let n = r.read(&mut buf[..want])?;
        if n == 0 {
            return Err(std::io::Error::other("short read"));
        }
        fh.seek(SeekFrom::Start(pos))?;
        fh.write_all(&buf[..n])?;
        pos += n as u64;
        written += n as u64;
    }
    Ok(written)
}

/// Ask the transfer to stop. Checked between pieces, so it takes effect within
/// one piece rather than instantly, and nothing half-written is ever recorded
/// as complete.
pub fn cancel() {
    CANCEL.store(true, Ordering::Relaxed);
}

/// (bytes so far, bytes expected) for whatever is downloading.
pub fn progress() -> (u64, u64) {
    (DONE_BYTES.load(Ordering::Relaxed), TOTAL.load(Ordering::Relaxed))
}

/// "Connection reset by peer (os error 104)" tells a teacher nothing, and the
/// number at the end is the part that looks most alarming.
fn plain_error(e: &std::io::Error) -> String {
    let s = e.to_string();
    match s.split_once(" (os error") {
        Some((head, _)) => head.to_string(),
        None => s,
    }
}

/// Network errors say "Connection refused (os error 111)". A teacher needs to
/// know which of the three things to try.
fn describe(e: &std::io::Error) -> String {
    use std::io::ErrorKind::*;
    match e.kind() {
        ConnectionRefused => "Nothing is handing out files at that address. \
                              Is the teacher's computer still running it?".into(),
        TimedOut | WouldBlock => "No answer. The signal may be too weak, \
                                  or you may be on a different network.".into(),
        NotFound => "That computer is handing files out, but not that one.".into(),
        _ => format!("Could not reach it: {e}"),
    }
}
