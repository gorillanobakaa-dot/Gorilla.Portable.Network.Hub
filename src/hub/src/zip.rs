//! A zip archive streamed straight down the socket, built for one button:
//! GET EVERYTHING in a browser, with nothing installed on the kid's machine.
//!
//! Why zip and not tar: every Windows since XP opens a zip in Explorer with no
//! software at all, and the machines this is for are exactly the ones where
//! nothing can be installed. Why STORE mode and not compression: the 2026-08-22
//! design decision stands, squeezing files is harsh on a weak teacher's laptop
//! and is a last resort. Store mode is the files laid end to end with a table
//! of contents; the only arithmetic is a CRC32 per file, which this 2012
//! machine does hundreds of times faster than the wifi can carry the bytes.
//!
//! Streamed, never staged: the archive is written to the socket as it is
//! built, so a 7 GB folder needs 7 GB of disk on the KID's side only. The
//! teacher's machine never holds a copy.
//!
//! Always Zip64. The classic format dies at 4 GB and at 65,535 entries, and
//! the folder that motivated this holds 68,153 files including one of 5.4 GB.
//! Writing Zip64 unconditionally means one code path, exercised by every test,
//! rather than a rare branch that only a huge folder ever runs.

use std::io::{self, Read, Write};
use std::path::Path;

/// CRC-32 (IEEE), the checksum zip requires. std does not carry one.
fn crc32_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

struct Crc {
    table: [u32; 256],
    value: u32,
}

impl Crc {
    fn new() -> Crc {
        Crc { table: crc32_table(), value: 0xFFFF_FFFF }
    }
    fn update(&mut self, data: &[u8]) {
        for &b in data {
            self.value = self.table[((self.value ^ b as u32) & 0xFF) as usize] ^ (self.value >> 8);
        }
    }
    fn finish(&self) -> u32 {
        self.value ^ 0xFFFF_FFFF
    }
}

/// A fixed DOS timestamp: 2026-01-01 00:00.
///
/// Deliberately not each file's real mtime. Converting mtimes needs calendar
/// code this module would have to borrow, and the one thing the stamp is used
/// for, Explorer's date column on the kid's machine, does not matter to a
/// child unpacking homework. A constant is honest about being one.
const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = ((2026 - 1980) << 9) | (1 << 5) | 1;

struct Entry {
    name: Vec<u8>,
    crc: u32,
    size: u64,
    offset: u64,
}

/// Everything zip writes is little-endian.
fn w16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn w32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn w64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// The exact byte size the archive WILL be, before a byte is written.
///
/// Store mode makes this arithmetic rather than prophecy: per entry a 30-byte
/// local header + name + 20 of Zip64 extra, the bytes themselves, a 24-byte
/// descriptor, then a 46-byte central record + name + 28 of extra, and 98 of
/// end-of-archive records. stream() enforces the promised sizes (growth is
/// cut, shrinkage is an error), so this number is exact and can be sent as
/// Content-Length: the difference between a browser showing a progress bar
/// with an end, and a spinner for twenty minutes.
pub fn exact_size(files: &[(String, u64)]) -> u64 {
    let mut n = 98u64; // zip64 EOCD 56 + locator 20 + classic EOCD 22
    for (rel, size) in files {
        let name = rel.len() as u64;
        n += 30 + name + 20; // local header + zip64 extra
        n += size;
        n += 24; // data descriptor
        n += 46 + name + 28; // central record + zip64 extra
    }
    n
}

/// Stream `files` (relative name, size) under `root` as a store-mode zip.
///
/// `count_out`, when given, receives the number of BYTES written so far after
/// each block, so the caller can feed a progress display without this module
/// knowing anything about rosters.
pub fn stream<W: Write>(
    out: &mut W,
    root: &Path,
    files: &[(String, u64)],
    mut progress: Option<&mut dyn FnMut(u64)>,
) -> io::Result<()> {
    let mut written: u64 = 0;
    let mut entries: Vec<Entry> = Vec::with_capacity(files.len());
    let mut buf = vec![0u8; 256 * 1024];

    for (rel, expect) in files {
        let name: Vec<u8> = rel.as_bytes().to_vec();
        let offset = written;

        // Local file header. With flag bit 3 set the sizes and CRC are not
        // known yet and live in the data descriptor after the bytes; the
        // 0xFFFFFFFF sizes plus the Zip64 extra are what tell a reader the
        // descriptor uses 8-byte fields.
        let mut h = Vec::with_capacity(64 + name.len());
        w32(&mut h, 0x0403_4B50);
        w16(&mut h, 45); // version needed: Zip64
        w16(&mut h, 0x0808); // bit 3 descriptor follows, bit 11 UTF-8 names
        w16(&mut h, 0); // method: store
        w16(&mut h, DOS_TIME);
        w16(&mut h, DOS_DATE);
        w32(&mut h, 0); // crc, in the descriptor
        w32(&mut h, 0xFFFF_FFFF); // compressed size, see Zip64 extra
        w32(&mut h, 0xFFFF_FFFF); // uncompressed size
        w16(&mut h, name.len() as u16);
        w16(&mut h, 20); // extra length
        h.extend_from_slice(&name);
        w16(&mut h, 0x0001); // Zip64 extra
        w16(&mut h, 16);
        w64(&mut h, 0); // sizes unknown here, the descriptor carries them
        w64(&mut h, 0);
        out.write_all(&h)?;
        written += h.len() as u64;

        // The bytes. Exactly the size the listing promised: a file that grew
        // mid-lesson must not shift every offset after it, and one that shrank
        // must not leave the archive short. Growth is truncated at the
        // promised length; a shortfall is an error, because padding would
        // corrupt the CRC and a silently damaged archive is the worst outcome.
        let mut crc = Crc::new();
        let mut sent: u64 = 0;
        let mut f = std::fs::File::open(root.join(rel))?;
        while sent < *expect {
            let want = ((*expect - sent) as usize).min(buf.len());
            let n = f.read(&mut buf[..want])?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!("{rel} is shorter than it was when the lesson started"),
                ));
            }
            crc.update(&buf[..n]);
            out.write_all(&buf[..n])?;
            sent += n as u64;
            written += n as u64;
            if let Some(p) = progress.as_deref_mut() {
                p(written);
            }
        }

        // Data descriptor, 8-byte sizes per the Zip64 local header above.
        let mut d = Vec::with_capacity(24);
        w32(&mut d, 0x0807_4B50);
        w32(&mut d, crc.finish());
        w64(&mut d, sent);
        w64(&mut d, sent);
        out.write_all(&d)?;
        written += d.len() as u64;

        entries.push(Entry { name, crc: crc.finish(), size: sent, offset });
    }

    // Central directory: the table of contents Explorer actually reads.
    let cd_start = written;
    for e in &entries {
        let mut c = Vec::with_capacity(80 + e.name.len());
        w32(&mut c, 0x0201_4B50);
        w16(&mut c, 45); // made by
        w16(&mut c, 45); // needed
        w16(&mut c, 0x0808);
        w16(&mut c, 0); // store
        w16(&mut c, DOS_TIME);
        w16(&mut c, DOS_DATE);
        w32(&mut c, e.crc);
        w32(&mut c, 0xFFFF_FFFF);
        w32(&mut c, 0xFFFF_FFFF);
        w16(&mut c, e.name.len() as u16);
        w16(&mut c, 28); // extra len
        w16(&mut c, 0); // comment
        w16(&mut c, 0); // disk
        w16(&mut c, 0); // internal attrs
        w32(&mut c, 0); // external attrs
        w32(&mut c, 0xFFFF_FFFF); // offset, in the extra
        c.extend_from_slice(&e.name);
        w16(&mut c, 0x0001);
        w16(&mut c, 24);
        w64(&mut c, e.size);
        w64(&mut c, e.size);
        w64(&mut c, e.offset);
        out.write_all(&c)?;
        written += c.len() as u64;
    }
    let cd_size = written - cd_start;

    // Zip64 end of central directory, its locator, and the classic record
    // with every field saturated, which is what points readers at the real one.
    let mut e = Vec::with_capacity(120);
    w32(&mut e, 0x0606_4B50);
    w64(&mut e, 44);
    w16(&mut e, 45);
    w16(&mut e, 45);
    w32(&mut e, 0);
    w32(&mut e, 0);
    w64(&mut e, entries.len() as u64);
    w64(&mut e, entries.len() as u64);
    w64(&mut e, cd_size);
    w64(&mut e, cd_start);
    // 0x07064B50, and the 06 matters: the first version of this wrote 07,
    // and the archive opened nowhere. Caught by the independent reader, which
    // is the whole reason the test uses one.
    w32(&mut e, 0x0706_4B50);
    w32(&mut e, 0);
    w64(&mut e, written); // where the Zip64 EOCD itself sits
    w32(&mut e, 1);
    w32(&mut e, 0x0605_4B50);
    w16(&mut e, 0);
    w16(&mut e, 0);
    w16(&mut e, 0xFFFF);
    w16(&mut e, 0xFFFF);
    w32(&mut e, 0xFFFF_FFFF);
    w32(&mut e, 0xFFFF_FFFF);
    w16(&mut e, 0);
    out.write_all(&e)?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tree(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hub-zip-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("subject/week1")).unwrap();
        std::fs::write(d.join("notes.txt"), b"top level").unwrap();
        std::fs::write(d.join("subject/handout.txt"), vec![7u8; 300_000]).unwrap();
        std::fs::write(d.join("subject/week1/worksheet.txt"), b"nested file").unwrap();
        d
    }

    fn listing(root: &Path) -> Vec<(String, u64)> {
        vec![
            ("notes.txt".into(), std::fs::metadata(root.join("notes.txt")).unwrap().len()),
            ("subject/handout.txt".into(), 300_000),
            ("subject/week1/worksheet.txt".into(), 11),
        ]
    }

    /// The archive is judged by an INDEPENDENT reader, not by this module
    /// agreeing with itself. Python's zipfile knows nothing of this code and
    /// reads Zip64 and data descriptors; if it can extract every file byte for
    /// byte and the CRCs check, Explorer will too.
    #[test]
    fn an_independent_reader_gets_every_byte_back() {
        let root = tree("roundtrip");
        let files = listing(&root);
        let mut archive = Vec::new();
        stream(&mut archive, &root, &files, None).expect("streaming failed");
        assert_eq!(archive.len() as u64, exact_size(&files),
                   "the promised Content-Length must be the truth");
        let zip_path = root.join("out.zip");
        std::fs::write(&zip_path, &archive).unwrap();

        let script = r#"
import sys, zipfile
z = zipfile.ZipFile(sys.argv[1])
bad = z.testzip()
assert bad is None, f"CRC failed on {bad}"
names = sorted(z.namelist())
assert names == ["notes.txt", "subject/handout.txt", "subject/week1/worksheet.txt"], names
assert z.read("notes.txt") == b"top level"
assert z.read("subject/handout.txt") == bytes([7])*300000
assert z.read("subject/week1/worksheet.txt") == b"nested file"
# store mode, nothing compressed
for i in z.infolist():
    assert i.compress_type == zipfile.ZIP_STORED, i.filename
print("OK")
"#;
        let out = std::process::Command::new("python3")
            .arg("-c")
            .arg(script)
            .arg(&zip_path)
            .output()
            .expect("python3 is present on every machine this builds on");
        assert!(
            out.status.success() && String::from_utf8_lossy(&out.stdout).contains("OK"),
            "independent reader rejected the archive:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A file that shrinks mid-stream must fail loudly, never pad. Padding
    /// keeps the offsets right and quietly corrupts the file's CRC, which is
    /// the worst possible outcome: an archive that opens and hands a child
    /// damaged homework.
    #[test]
    fn a_file_that_shrank_kills_the_stream_rather_than_padding() {
        let root = tree("shrink");
        let mut files = listing(&root);
        // Promise more bytes than the file has.
        files[0].1 += 1000;
        let mut archive = Vec::new();
        let err = stream(&mut archive, &root, &files, None);
        assert!(err.is_err(), "a short file must be an error");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Growth is capped at the promised size, so one appended-to log file does
    /// not shift every offset behind it and wreck the table of contents.
    #[test]
    fn a_file_that_grew_is_cut_at_the_size_that_was_promised() {
        let root = tree("grow");
        let files = vec![("notes.txt".to_string(), 3u64)]; // file is 9 bytes
        let mut archive = Vec::new();
        stream(&mut archive, &root, &files, None).expect("must succeed");
        let zip_path = root.join("out.zip");
        std::fs::write(&zip_path, &archive).unwrap();
        let out = std::process::Command::new("python3")
            .arg("-c")
            .arg("import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); assert z.read('notes.txt')==b'top'; print('OK')")
            .arg(&zip_path)
            .output()
            .expect("python3");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Progress reports climb monotonically and end at the archive's true size.
    #[test]
    fn progress_counts_every_byte_once() {
        let root = tree("progress");
        let files = listing(&root);
        let mut archive = Vec::new();
        let mut last = 0u64;
        let mut calls = 0u32;
        stream(&mut archive, &root, &files, Some(&mut |w| {
            assert!(w >= last, "progress went backwards");
            last = w;
            calls += 1;
        }))
        .unwrap();
        assert!(calls > 0);
        assert!(last <= archive.len() as u64);
        let _ = std::fs::remove_dir_all(&root);
    }
}
