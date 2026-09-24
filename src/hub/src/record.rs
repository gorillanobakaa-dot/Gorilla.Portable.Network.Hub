//! The locked record of private conversations.
//!
//! The owner's decision, 2026-09-24: every word of a private conversation is
//! kept, locked with a password only the trusted adult knows, and optionally
//! a second adult's. Nothing private is ever written in the clear.
//!
//! HOW. Each lesson with private help switched on gets its own file,
//! `private-record-<date>-<time>.gpr`, beside the received work. A fresh
//! random key locks every entry (crypto::seal). That key is itself locked
//! once per adult, under a key made from that adult's password with PBKDF2
//! (a "slot"), so either adult's password opens the record and neither has to
//! know the other's. The file is text, one line each:
//!
//!   GORILLA PRIVATE RECORD 1
//!   SLOT <name, base64> <salt, base64> <iterations> <locked key, base64>
//!   E <locked entry, base64>
//!
//! An entry, once opened, is: time, TAB, the child's label, TAB, who spoke,
//! TAB, what kind, TAB, the words.
//!
//! WHAT IT DOES NOT DO, said out loud: anyone with access to the laptop can
//! delete the file (the program offers no way to, and the adult who took part
//! cannot remove single entries through it), and the words are not protected
//! while they cross the wifi. The lesson's key lives in memory while the
//! lesson runs, and is dropped when it stops.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Slow on purpose: each password guess costs this many HMACs. Measured in
/// `unlocking_takes_long_enough_to_slow_guessing`: a person does not notice
/// it once, and a guessing program pays it on every guess.
pub const ITERATIONS: u32 = 200_000;
const HEADER: &str = "GORILLA PRIVATE RECORD 1";

struct Slot {
    name: String,
    key: [u8; 32],
}

struct Open {
    path: PathBuf,
    data_key: [u8; 32],
    slots: Vec<Slot>,
}

static OPEN: Mutex<Option<Open>> = Mutex::new(None);

/// Start this lesson's record. `adults` is (name, password) for each adult
/// who may open it; the first is the one who receives the messages.
pub fn start(dir: &Path, adults: &[(&str, &str)]) -> Result<PathBuf, String> {
    if adults.is_empty() {
        return Err("no adult to lock the record for".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    let data_key: [u8; 32] = crate::net::random_bytes(32)
        .and_then(|b| b.try_into().ok())
        .ok_or("this computer gave no randomness, so the record cannot be locked safely")?;
    let stamp = crate::net::timestamp().replace([' ', ':'], "-");
    let mut path = dir.join(format!("private-record-{stamp}.gpr"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("private-record-{stamp}-{n}.gpr"));
        n += 1;
    }
    let mut text = format!("{HEADER}\n");
    let mut slots = Vec::new();
    for (name, password) in adults {
        let salt = crate::net::random_bytes(16).ok_or("no randomness for the salt")?;
        let key = crate::crypto::pbkdf2(password.as_bytes(), &salt, ITERATIONS);
        let locked = crate::crypto::seal(&key, &data_key).ok_or("no randomness for the lock")?;
        text.push_str(&format!(
            "SLOT {} {} {ITERATIONS} {}\n",
            crate::crypto::b64(name.as_bytes()),
            crate::crypto::b64(&salt),
            crate::crypto::b64(&locked)
        ));
        slots.push(Slot { name: name.to_string(), key });
    }
    std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    *OPEN.lock().unwrap_or_else(|e| e.into_inner()) = Some(Open { path: path.clone(), data_key, slots });
    Ok(path)
}

/// The lesson ended: forget the key.
pub fn stop() {
    *OPEN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub fn is_open() -> bool {
    OPEN.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// Which adult this password belongs to, for the adult's page. Takes as long
/// as unlocking does, which is the point.
pub fn who_is(password: &str) -> Option<String> {
    let (path, known): (PathBuf, Vec<(String, [u8; 32])>) = {
        let g = OPEN.lock().unwrap_or_else(|e| e.into_inner());
        let o = g.as_ref()?;
        (o.path.clone(), o.slots.iter().map(|s| (s.name.clone(), s.key)).collect())
    };
    // The salts are in the file; the derived keys are in memory. Derive from
    // the typed password with each salt and compare.
    let file = std::fs::read_to_string(&path).ok()?;
    for line in file.lines().filter(|l| l.starts_with("SLOT ")) {
        let f: Vec<&str> = line.split(' ').collect();
        if f.len() != 5 {
            continue;
        }
        let (Some(name), Some(salt)) = (crate::crypto::unb64(f[1]), crate::crypto::unb64(f[2])) else { continue };
        let iters: u32 = f[3].parse().unwrap_or(ITERATIONS);
        let key = crate::crypto::pbkdf2(password.as_bytes(), &salt, iters);
        let name = String::from_utf8_lossy(&name).into_owned();
        if known.iter().any(|(n, k)| *n == name && *k == key) {
            return Some(name);
        }
    }
    None
}

/// Keep one private message, locked. Nothing is written if no record is open,
/// and then the private channel is off anyway.
pub fn keep(m: &crate::chat::Msg) {
    let g = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    let Some(o) = g.as_ref() else { return };
    let who = if m.from_child { "child" } else { "adult" };
    let line = format!(
        "{}\t{}\t{who}\t{:?}\t{}",
        crate::net::timestamp(),
        m.label.replace(['\t', '\n'], " "),
        m.kind,
        crate::chat::describe(m).replace('\t', " ")
    );
    let Some(sealed) = crate::crypto::seal(&o.data_key, line.as_bytes()) else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&o.path) {
        let _ = writeln!(f, "E {}", crate::crypto::b64(&sealed));
    }
}

/// Open a record with a password. Every entry, in order, as its text line.
pub fn read(path: &Path, password: &str) -> Result<Vec<String>, String> {
    let file = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if file.lines().next() != Some(HEADER) {
        return Err(format!("{} is not a private record", path.display()));
    }
    let mut data_key: Option<[u8; 32]> = None;
    for line in file.lines().filter(|l| l.starts_with("SLOT ")) {
        let f: Vec<&str> = line.split(' ').collect();
        if f.len() != 5 {
            continue;
        }
        let (Some(salt), Some(locked)) = (crate::crypto::unb64(f[2]), crate::crypto::unb64(f[4])) else { continue };
        let iters: u32 = f[3].parse().unwrap_or(ITERATIONS);
        let key = crate::crypto::pbkdf2(password.as_bytes(), &salt, iters);
        if let Some(k) = crate::crypto::open(&key, &locked).and_then(|k| <[u8; 32]>::try_from(k).ok()) {
            data_key = Some(k);
            break;
        }
    }
    let Some(dk) = data_key else { return Err("that password does not open this record".into()) };
    let mut out = Vec::new();
    for line in file.lines().filter_map(|l| l.strip_prefix("E ")) {
        match crate::crypto::unb64(line).and_then(|s| crate::crypto::open(&dk, &s)) {
            Some(p) => out.push(String::from_utf8_lossy(&p).into_owned()),
            None => out.push("(this entry has been changed or damaged and cannot be read)".into()),
        }
    }
    Ok(out)
}

/// Every record in a folder, oldest first.
pub fn records_in(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "gpr"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// `hub private-record [folder]`: ask for a password and print every record in
/// the folder that it opens. The words appear on this screen, so it says so
/// before asking.
pub fn run(args: Vec<String>) {
    let dir = args.get(1).map(PathBuf::from).unwrap_or_else(crate::page::default_receive_dir);
    let files = records_in(&dir);
    if files.is_empty() {
        println!("No private records in {}.", dir.display());
        return;
    }
    println!("{} private record(s) in {}.", files.len(), dir.display());
    println!("What they contain will be shown on this screen. Make sure nobody else can see it.");
    print!("Password: ");
    let _ = std::io::stdout().flush();
    let mut pw = String::new();
    if std::io::stdin().read_line(&mut pw).is_err() {
        return;
    }
    let pw = pw.trim_end_matches(['\r', '\n']);
    let mut opened = 0;
    for f in &files {
        if let Ok(lines) = read(f, pw) {
            opened += 1;
            println!("\n== {}", f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            for l in lines {
                let parts: Vec<&str> = l.splitn(5, '\t').collect();
                if parts.len() == 5 {
                    println!("{}  {} ({}): {}", parts[0], parts[1], parts[2], parts[4]);
                } else {
                    println!("{l}");
                }
            }
        }
    }
    if opened == 0 {
        println!("That password opens none of them.");
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hub-record-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn msg(text: &str) -> crate::chat::Msg {
        crate::chat::Msg {
            id: 1,
            at: "10:00".into(),
            key: "k".into(),
            label: "Amina".into(),
            from_child: true,
            private: true,
            kind: crate::chat::Kind::Text,
            text: text.into(),
            seen: false,
        }
    }

    #[test]
    fn either_adult_opens_the_record_and_nobody_else_does() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tmp("two");
        let path = start(&dir, &[("trusted adult", "first-secret"), ("second adult", "other-secret")]).unwrap();
        keep(&msg("something that must stay private"));
        keep(&msg("a second message"));
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("must stay") && !raw.contains("Amina"), "no words in the clear:\n{raw}");
        for pw in ["first-secret", "other-secret"] {
            let lines = read(&path, pw).unwrap();
            assert_eq!(lines.len(), 2);
            assert!(lines[0].contains("Amina") && lines[0].ends_with("something that must stay private"), "{lines:?}");
        }
        assert!(read(&path, "wrong-guess").is_err());
        assert_eq!(who_is("other-secret").as_deref(), Some("second adult"));
        assert_eq!(who_is("nope"), None);
        stop();
        assert!(!is_open());
        keep(&msg("after stop"));
        assert_eq!(read(&path, "first-secret").unwrap().len(), 2, "nothing is written once the lesson stopped");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_changed_entry_is_reported_not_trusted() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tmp("tamper");
        let path = start(&dir, &[("trusted adult", "pw-123456")]).unwrap();
        keep(&msg("original words"));
        stop();
        let raw = std::fs::read_to_string(&path).unwrap();
        let e = raw.lines().find(|l| l.starts_with("E ")).unwrap().to_string();
        let mut bytes = crate::crypto::unb64(&e[2..]).unwrap();
        bytes[20] ^= 1;
        let changed = raw.replace(&e, &format!("E {}", crate::crypto::b64(&bytes)));
        std::fs::write(&path, changed).unwrap();
        let lines = read(&path, "pw-123456").unwrap();
        assert!(lines[0].contains("changed or damaged"), "{lines:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Measured, not assumed: one unlock must cost real time.
    #[test]
    fn unlocking_takes_long_enough_to_slow_guessing() {
        let t = std::time::Instant::now();
        let _ = crate::crypto::pbkdf2(b"guess", b"0123456789abcdef", ITERATIONS);
        let ms = t.elapsed().as_millis();
        println!("one password guess: {ms} ms");
        assert!(ms >= 20, "a guess this cheap ({ms} ms) makes a short password easy to find");
    }
}
