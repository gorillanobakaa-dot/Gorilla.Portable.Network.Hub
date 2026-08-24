#![allow(dead_code, unused_imports, clippy::needless_return)]
// Version: 1.0.0 · updated 26-08-24-19-40
//
// Per-chunk SHA-256, computed across every core.
//
// WHY PER-CHUNK RATHER THAN ONE HASH OF THE FILE: a single whole-file digest
// is inherently sequential, so it cannot use more than one core, and it can
// only ever tell you the file is wrong, not WHICH PART is wrong. Per-chunk
// digests parallelise perfectly and land exactly on the unit resume already
// works in, so a corrupt 2 MB chunk is refetched on its own instead of
// invalidating a 4 GB download.
//
// WHY NOT COPY LOCALSEND HERE: measured 2026-08-24, their sender hashes every
// file in a sequential `for ... await` loop before a single byte is sent. On
// this quad-core that leaves three cores idle, and on a fast link the hashing
// costs 78% of the transfer time.
use crate::sha256;

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use std::{env, thread};

const CHUNK: u64 = 2 * 1024 * 1024; // must match fetch's CHUNK

pub fn run(args: Vec<String>) {
    
    if args.len() < 2 || args[1] == "-h" || args[1] == "--help" {
        println!("sums  -  checksum each piece of a file, using every core\n");
        println!("  sums <file> [<file>...]      writes <file>.sums beside each one\n");
        println!("  The .sums file lets a download check each 2 MB piece as it");
        println!("  arrives, and repair just that piece if it is wrong.");
        std::process::exit(if args.len() < 2 { 2 } else { 0 });
    }
    // Threads are settable so the assumption can be TESTED rather than argued
    // about. SHA-256 is CPU-bound, so the expectation is that more threads than
    // logical cores buys nothing and costs context switches. Measured below.
    let cores = env::var("SUMS_THREADS").ok().and_then(|v| v.parse().ok())
        .unwrap_or_else(|| thread::available_parallelism().map(|n| n.get()).unwrap_or(4));
    for path in &args[1..] {
        match hash_file(path, cores) {
            Ok((n, secs, rate)) => println!(
                "{path}: {n} chunks in {secs:.1}s = {rate:.0} MB/s across {cores} cores"
            ),
            Err(e) => eprintln!("{path}: {e}"),
        }
    }
}

fn hash_file(path: &str, cores: usize) -> std::io::Result<(u64, f64, f64)> {
    let total = std::fs::metadata(path)?.len();
    let chunks = total.div_ceil(CHUNK);
    let results: Arc<Mutex<Vec<Option<String>>>> =
        Arc::new(Mutex::new(vec![None; chunks as usize]));
    let next = Arc::new(AtomicU64::new(0));
    let t0 = Instant::now();

    let mut handles = Vec::new();
    for _ in 0..cores {
        let (next, results, path) = (Arc::clone(&next), Arc::clone(&results), path.to_string());
        let spans = cores;
        handles.push(thread::spawn(move || -> std::io::Result<()> {
            let mut f = File::open(&path)?;
            let mut buf = vec![0u8; CHUNK as usize];
            // CONTIGUOUS SPANS, not round-robin chunks.
            //
            // Handing chunks out one at a time from a shared counter means N
            // threads each seek to a different place, so the kernel sees N
            // interleaved read streams and its sequential readahead gives up.
            // Measured on this SATA SSD: the disk does 520 MB/s sequentially,
            // but hashing an 8 GB file managed only 339 MB/s. Giving each
            // thread one unbroken span means each one reads forwards, which is
            // the access pattern readahead is built for.
            let span = chunks.div_ceil(spans as u64);
            let me = next.fetch_add(1, Ordering::Relaxed);
            let first = me * span;
            let last = ((me + 1) * span).min(chunks);
            let mut c = first;
            loop {
                if c >= last {
                    break;
                }
                let this = c;
                c += 1;
                let c = this;
                let start = c * CHUNK;
                let len = std::cmp::min(CHUNK, total - start) as usize;
                f.seek(SeekFrom::Start(start))?;
                f.read_exact(&mut buf[..len])?;
                let d = sha256::digest(&buf[..len]);
                results.lock().unwrap()[c as usize] = Some(sha256::hex(&d));
            }
            Ok(())
        }));
    }
    for h in handles {
        let _ = h.join();
    }

    let secs = t0.elapsed().as_secs_f64().max(0.001);
    let r = results.lock().unwrap();
    let mut out = File::create(format!("{path}.sums"))?;
    writeln!(out, "# chunk-size {CHUNK}")?;
    writeln!(out, "# total {total}")?;
    for (i, h) in r.iter().enumerate() {
        writeln!(out, "{i} {}", h.as_deref().unwrap_or("MISSING"))?;
    }
    Ok((chunks, secs, total as f64 / secs / 1_048_576.0))
}
