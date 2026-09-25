//! The class net: one room everybody talks in, like an IRC channel.
//!
//! WHY THIS EXISTS. The 0.10.0 messages were one thread per child, read on
//! the teacher's terminal, and the owner's two-phone test (2026-09-25) found
//! it "a joke": no room the whole class shares, no list of who is there, no
//! way to see two children at once. What was asked for is what mIRC and irssi
//! did for millions of people on 1990s machines, and what a radio net does
//! with voices: one channel everybody hears, where the operator opens the net
//! and calls a comms check and the stations answer, plus a private line to
//! any one station.
//!
//! THE SHAPE.
//!   * #main: everybody signed in can write, everybody reads. Kept here.
//!   * The operator (teacher, team leader): the laptop's own browser, and only
//!     that (see `serve::is_operator`). Sets the topic, calls comms checks,
//!     can quiet the room (IRC's +m), mute one person, remove a device.
//!   * Private chats, operator and one person: the existing per-child
//!     conversations in chat.rs, HELP included, so the terminal's message
//!     screens and alarms keep working unchanged.
//!
//! HOW PAGES LEARN SOMETHING HAPPENED. One counter, bumped on every change
//! anywhere (here or in chat.rs), with a condition variable. A page asks
//! "anything after N?" and the server answers the moment there is, or after
//! WAIT_FOR with nothing. The waiting happens on its own small thread, never
//! on one of the server's fixed pool of workers: serve.rs explains the
//! countdown a held worker starts (IO_TIMEOUT, IDLE_TIMEOUT).
//!
//! WHAT IS KEPT. Every line of #main goes to messages.txt beside the received
//! work, with the private chats, in plain words. Comms checks, answers and
//! removed devices go to sign-ins.txt. In memory everything is forgotten when
//! the lesson stops.

use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// A line is a line. Longer thoughts go in two.
pub const TEXT_CAP: usize = 500;
/// Lines one device may write per minute. The operator is not limited.
const PER_MINUTE: usize = 12;
/// Lines #main keeps in memory; the record keeps everything.
const KEPT: usize = 500;
/// Lines a page gets when it first opens.
const FIRST_LOAD: usize = 150;
/// How long a page waits for news before it is answered anyway. Shorter than
/// the 30 s a phone browser or a proxy is likely to give up on a quiet
/// request, long enough that an idle class costs almost nothing.
pub const WAIT_FOR: Duration = Duration::from_secs(25);
/// Somebody whose page has not asked for this long has gone.
const ONLINE_FOR: Duration = Duration::from_secs(45);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Text,
    /// The operator's comms check.
    Call,
    /// "I read you".
    Answer,
    /// Joins, topic changes, quiet, mute: said by nobody.
    Event,
}

impl LineKind {
    fn word(self) -> &'static str {
        match self {
            LineKind::Text => "text",
            LineKind::Call => "call",
            LineKind::Answer => "ans",
            LineKind::Event => "ev",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Line {
    pub id: u64,
    pub at: String,
    /// The writer's device key, "op" for the operator, empty for events.
    pub key: String,
    pub nick: String,
    pub kind: LineKind,
    pub text: String,
}

#[derive(Clone, Debug)]
struct Person {
    key: String,
    nick: String,
    ip: String,
    last_seen: Instant,
}

#[derive(Clone, Debug)]
struct Check {
    id: u64,
    at: String,
    /// (key, "HH:MM")
    answered: Vec<(String, String)>,
}

#[derive(Default)]
struct Room {
    lines: Vec<Line>,
    next_id: u64,
    topic: String,
    quiet: bool,
    muted: Vec<String>,
    check: Option<Check>,
    checks: u64,
    people: Vec<Person>,
    recent: Vec<(String, Instant)>,
    tokens: Vec<(String, String)>,
    op_nick: String,
}

static ROOM: Mutex<Option<Room>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Room) -> R) -> R {
    let mut g = ROOM.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Room::default))
}

// ---------------------------------------------------------------- the pulse
//
// A leaf lock: nothing else is ever taken while it is held, so chat.rs can
// pulse from inside its own lock and this module from inside its own.

struct Pulse {
    v: u64,
    /// (conversation key, version it last changed at), for chat.rs threads.
    convs: Vec<(String, u64)>,
}

static PULSE: Mutex<Pulse> = Mutex::new(Pulse { v: 1, convs: Vec::new() });
static BELL: Condvar = Condvar::new();

/// Something changed. `conv` names a private conversation that changed.
pub fn pulse(conv: Option<&str>) {
    let mut p = PULSE.lock().unwrap_or_else(|e| e.into_inner());
    p.v += 1;
    let v = p.v;
    if let Some(k) = conv {
        match p.convs.iter_mut().find(|(c, _)| c == k) {
            Some(e) => e.1 = v,
            None => p.convs.push((k.to_string(), v)),
        }
    }
    drop(p);
    BELL.notify_all();
}

pub fn version() -> u64 {
    PULSE.lock().unwrap_or_else(|e| e.into_inner()).v
}

/// Block until something changes after `since`, or `max` passes. Returns the
/// version now.
pub fn wait(since: u64, max: Duration) -> u64 {
    let end = Instant::now() + max;
    let mut p = PULSE.lock().unwrap_or_else(|e| e.into_inner());
    while p.v <= since {
        let now = Instant::now();
        if now >= end {
            break;
        }
        p = BELL.wait_timeout(p, end - now).unwrap_or_else(|e| e.into_inner()).0;
    }
    p.v
}

/// Private conversations that changed after `since`.
fn convs_changed(since: u64) -> Vec<String> {
    let p = PULSE.lock().unwrap_or_else(|e| e.into_inner());
    p.convs.iter().filter(|(_, v)| *v > since).map(|(k, _)| k.clone()).collect()
}

// ---------------------------------------------------------------- the room

fn clock() -> String {
    let t = crate::net::timestamp();
    t.rsplit(' ').next().unwrap_or("").to_string()
}

fn push(r: &mut Room, key: &str, nick: &str, kind: LineKind, text: &str) -> Line {
    r.next_id += 1;
    let l = Line { id: r.next_id, at: clock(), key: key.into(), nick: nick.into(), kind, text: text.into() };
    r.lines.push(l.clone());
    if r.lines.len() > KEPT {
        r.lines.drain(..r.lines.len() - KEPT);
    }
    l
}

/// The lesson stopped: forget the room.
pub fn clear() {
    *ROOM.lock().unwrap_or_else(|e| e.into_inner()) = None;
    pulse(None);
}

/// What the operator is called on the net.
pub fn op_nick() -> String {
    with(|r| if r.op_nick.is_empty() { "Teacher".to_string() } else { r.op_nick.clone() })
}

pub fn set_op_nick(nick: &str) -> bool {
    let Some(n) = crate::page::sanitize_display_name(nick) else { return false };
    let old = op_nick();
    if n == old {
        return true;
    }
    with(|r| {
        r.op_nick = n.clone();
        push(r, "", "", LineKind::Event, &format!("{old} is now called {n}"));
    });
    pulse(None);
    true
}

/// A page asked: this device is here, with this name. The first time a name
/// is seen, the room is told, as IRC says who joined.
pub fn here(key: &str, nick: &str, ip: &str) {
    let changed = with(|r| {
        match r.people.iter_mut().find(|p| p.key == key) {
            Some(p) => {
                let back = p.last_seen.elapsed() > ONLINE_FOR;
                p.last_seen = Instant::now();
                p.ip = ip.to_string();
                if p.nick != nick {
                    let old = std::mem::replace(&mut p.nick, nick.to_string());
                    push(r, "", "", LineKind::Event, &format!("{old} is now called {nick}"));
                    true
                } else {
                    back
                }
            }
            None => {
                r.people.push(Person { key: key.into(), nick: nick.into(), ip: ip.into(), last_seen: Instant::now() });
                push(r, "", "", LineKind::Event, &format!("\u{2192} {nick} joined"));
                true
            }
        }
    });
    if changed {
        pulse(None);
    }
}

/// Where a line from somebody lands. The words the page shows for each.
pub fn say(key: &str, nick: &str, text: &str, token: &str, operator: bool) -> (&'static str, Option<Line>) {
    let text: String = text.trim().chars().take(TEXT_CAP).collect();
    if text.is_empty() {
        return ("empty", None);
    }
    let out = with(|r| {
        if !token.is_empty() {
            if r.tokens.iter().any(|(k, t)| k == key && t == token) {
                return ("sent", None);
            }
            r.tokens.push((key.into(), token.into()));
            if r.tokens.len() > 2000 {
                r.tokens.drain(..1000);
            }
        }
        if !operator {
            if r.muted.iter().any(|k| k == key) {
                return ("muted", None);
            }
            if r.quiet {
                return ("quiet", None);
            }
            r.recent.retain(|(_, t)| t.elapsed().as_secs() < 60);
            if r.recent.iter().filter(|(k, _)| k == key).count() >= PER_MINUTE {
                return ("toofast", None);
            }
            r.recent.push((key.into(), Instant::now()));
        }
        ("sent", Some(push(r, key, nick, LineKind::Text, &text)))
    });
    if out.1.is_some() {
        pulse(None);
    }
    out
}

/// The operator calls a comms check. With one running and people still to
/// answer, this is the second call, to those only, as a net controller does.
pub fn start_check() -> Line {
    let l = with(|r| {
        let again = r.check.as_ref().map(|c| {
            r.people
                .iter()
                .filter(|p| p.last_seen.elapsed() <= ONLINE_FOR && !c.answered.iter().any(|(k, _)| *k == p.key))
                .map(|p| p.nick.clone())
                .collect::<Vec<_>>()
        });
        let nick = if r.op_nick.is_empty() { "Teacher".to_string() } else { r.op_nick.clone() };
        match again {
            Some(left) if !left.is_empty() => {
                let text = format!("COMMS CHECK again. Not answered yet: {}", left.join(", "));
                push(r, "op", &nick, LineKind::Call, &text)
            }
            _ => {
                r.checks += 1;
                let at = clock();
                r.check = Some(Check { id: r.checks, at, answered: Vec::new() });
                push(r, "op", &nick, LineKind::Call, "COMMS CHECK. Everybody answer.")
            }
        }
    });
    pulse(None);
    l
}

/// Somebody answered the comms check. False when there is none, or they had.
pub fn answer(key: &str, nick: &str) -> bool {
    let done = with(|r| {
        let at = clock();
        let Some(c) = r.check.as_mut() else { return false };
        if c.answered.iter().any(|(k, _)| k == key) {
            return false;
        }
        c.answered.push((key.into(), at));
        push(r, key, nick, LineKind::Answer, "\u{2714} I read you");
        true
    });
    if done {
        pulse(None);
    }
    done
}

/// (check number, answered, online now, names still to answer)
pub fn check_summary() -> Option<(u64, usize, usize, Vec<String>)> {
    with(|r| {
        let c = r.check.as_ref()?;
        let online: Vec<&Person> = r.people.iter().filter(|p| p.last_seen.elapsed() <= ONLINE_FOR).collect();
        let left = online.iter().filter(|p| !c.answered.iter().any(|(k, _)| *k == p.key)).map(|p| p.nick.clone()).collect();
        // Answers from people here NOW. Counting every answer ever given read
        // "1 of 0 answered" on the owner's screen (2026-09-25) once the one
        // phone that had answered went quiet.
        let got = online.iter().filter(|p| c.answered.iter().any(|(k, _)| *k == p.key)).count();
        Some((c.id, got, online.len(), left))
    })
}

pub fn set_topic(text: &str) {
    let text: String = text.trim().chars().take(200).collect();
    with(|r| {
        r.topic = text.clone();
        let who = if r.op_nick.is_empty() { "Teacher".to_string() } else { r.op_nick.clone() };
        push(r, "", "", LineKind::Event, &format!("{who} set the topic: {text}"));
    });
    pulse(None);
}

pub fn set_quiet(on: bool) {
    let changed = with(|r| {
        if r.quiet == on {
            return false;
        }
        r.quiet = on;
        let who = if r.op_nick.is_empty() { "Teacher".to_string() } else { r.op_nick.clone() };
        let text = if on {
            format!("{who} quieted the room. Only {who} can write here. HELP and private messages still work.")
        } else {
            format!("{who} opened the room. Everybody can write.")
        };
        push(r, "", "", LineKind::Event, &text);
        true
    });
    if changed {
        pulse(None);
    }
}

pub fn quiet() -> bool {
    with(|r| r.quiet)
}

pub fn set_muted(key: &str, on: bool) -> bool {
    let changed = with(|r| {
        let Some(nick) = r.people.iter().find(|p| p.key == key).map(|p| p.nick.clone()) else { return false };
        let was = r.muted.iter().any(|k| k == key);
        if was == on {
            return false;
        }
        if on {
            r.muted.push(key.into());
        } else {
            r.muted.retain(|k| k != key);
        }
        push(r, "", "", LineKind::Event, &format!("{nick} {}", if on { "was muted in this room" } else { "can write again" }));
        true
    });
    if changed {
        pulse(None);
    }
    changed
}

/// The operator removes a device from the lesson. Returns (address, name) so
/// the caller can pause it (serve::block_device) and write the record.
pub fn remove(key: &str) -> Option<(String, String)> {
    let out = with(|r| {
        let i = r.people.iter().position(|p| p.key == key)?;
        let p = r.people.remove(i);
        push(r, "", "", LineKind::Event, &format!("{} was removed from the lesson", p.nick));
        Some((p.ip, p.nick))
    });
    if out.is_some() {
        pulse(None);
    }
    out
}

/// The name a device goes by in the room.
pub fn nick_of(key: &str) -> Option<String> {
    with(|r| r.people.iter().find(|p| p.key == key).map(|p| p.nick.clone()))
}

/// People online now, for the terminal's headline.
pub fn online() -> usize {
    with(|r| r.people.iter().filter(|p| p.last_seen.elapsed() <= ONLINE_FOR).count())
}

// ---------------------------------------------------------------- the teacher's window

/// One person as the teacher's window shows them.
#[derive(Clone, Debug)]
pub struct PersonView {
    pub key: String,
    pub nick: String,
    pub online: bool,
    /// "HH:MM" when they answered the current comms check, empty if not.
    pub answered: String,
    pub muted: bool,
}

/// The room as it is now, for the teacher's window (gui.rs), which runs in
/// this same program and so needs no page and no waiting.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub lines: Vec<Line>,
    pub topic: String,
    pub quiet: bool,
    /// (number, "HH:MM") of the comms check running now.
    pub check: Option<(u64, String)>,
    pub people: Vec<PersonView>,
}

pub fn snapshot() -> Snapshot {
    with(|r| Snapshot {
        lines: r.lines.clone(),
        topic: r.topic.clone(),
        quiet: r.quiet,
        check: r.check.as_ref().map(|c| (c.id, c.at.clone())),
        people: r
            .people
            .iter()
            .map(|p| PersonView {
                key: p.key.clone(),
                nick: p.nick.clone(),
                online: p.last_seen.elapsed() <= ONLINE_FOR,
                answered: r
                    .check
                    .as_ref()
                    .and_then(|c| c.answered.iter().find(|(k, _)| *k == p.key))
                    .map(|(_, a)| a.clone())
                    .unwrap_or_default(),
                muted: r.muted.iter().any(|k| *k == p.key),
            })
            .collect(),
    })
}

// ---------------------------------------------------------------- the answer a page gets

pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            // Never let a message close the script tag it may be printed into.
            '<' => o.push_str("\\u003c"),
            '>' => o.push_str("\\u003e"),
            '&' => o.push_str("\\u0026"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Who is asking.
pub struct Viewer {
    pub key: String,
    pub nick: String,
    pub operator: bool,
}

/// Everything a page needs to draw itself after `since` (the pulse version it
/// last had) and `line` (the last #main line it has). One small JSON object.
pub fn state(v: &Viewer, since: u64, line: u64) -> String {
    let now = version();
    let (lines, topic, quiet, muted, check, people, op) = with(|r| {
        let fresh: Vec<Line> = if line == 0 {
            r.lines.iter().rev().take(FIRST_LOAD).rev().cloned().collect()
        } else {
            r.lines.iter().filter(|l| l.id > line).cloned().collect()
        };
        let check = r.check.clone();
        let people = r.people.clone();
        let op = if r.op_nick.is_empty() { "Teacher".to_string() } else { r.op_nick.clone() };
        (fresh, r.topic.clone(), r.quiet, r.muted.clone(), check, people, op)
    });
    // chat.rs is read after the room's lock is let go: never both at once.
    let convs = if v.operator { crate::chat::conversations(false) } else { Vec::new() };
    let mut s = String::with_capacity(4096);
    s.push_str(&format!("{{\"v\":{now},\"me\":{{\"key\":{},\"nick\":{},\"op\":{}}}", json_str(&v.key), json_str(&v.nick), v.operator));
    s.push_str(&format!(",\"opnick\":{},\"topic\":{},\"quiet\":{quiet},\"muted\":{}", json_str(&op), json_str(&topic), muted.iter().any(|k| *k == v.key)));
    match &check {
        Some(c) => {
            let mine = c.answered.iter().any(|(k, _)| *k == v.key);
            s.push_str(&format!(",\"check\":{{\"id\":{},\"at\":{},\"me\":{mine}}}", c.id, json_str(&c.at)));
        }
        None => s.push_str(",\"check\":null"),
    }
    // #main
    s.push_str(",\"lines\":[");
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"id\":{},\"at\":{},\"k\":{},\"who\":{},\"kind\":\"{}\",\"text\":{}}}",
            l.id,
            json_str(&l.at),
            json_str(&l.key),
            json_str(&l.nick),
            l.kind.word(),
            json_str(&l.text)
        ));
    }
    s.push(']');
    // the people, the operator first
    s.push_str(&format!(",\"people\":[{{\"k\":\"op\",\"n\":{},\"op\":true,\"on\":true,\"ans\":\"\",\"muted\":false,\"help\":false,\"unread\":0}}", json_str(&op)));
    for p in &people {
        let on = p.last_seen.elapsed() <= ONLINE_FOR;
        let ans = check.as_ref().and_then(|c| c.answered.iter().find(|(k, _)| *k == p.key)).map(|(_, a)| a.clone()).unwrap_or_default();
        let conv = convs.iter().find(|c| c.key == p.key);
        s.push_str(&format!(
            ",{{\"k\":{},\"n\":{},\"op\":false,\"on\":{on},\"ans\":{},\"muted\":{},\"help\":{},\"unread\":{}}}",
            json_str(&p.key),
            json_str(&p.nick),
            json_str(&ans),
            muted.iter().any(|k| *k == p.key),
            conv.map(|c| c.needs_talk).unwrap_or(false),
            conv.map(|c| c.unread).unwrap_or(0)
        ));
    }
    s.push(']');
    // private chats: which exist (operator), and the full thread of each that
    // changed since the page last asked
    if v.operator {
        s.push_str(",\"convs\":[");
        for (i, c) in convs.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let nick = people.iter().find(|p| p.key == c.key).map(|p| p.nick.clone()).unwrap_or_else(|| c.label.clone());
            s.push_str(&format!(
                "{{\"k\":{},\"n\":{},\"unread\":{},\"help\":{}}}",
                json_str(&c.key),
                json_str(&nick),
                c.unread,
                c.needs_talk
            ));
        }
        s.push(']');
    }
    let wanted: Vec<String> = if v.operator {
        if since == 0 { convs.iter().map(|c| c.key.clone()).collect() } else { convs_changed(since) }
    } else if since == 0 || convs_changed(since).iter().any(|k| *k == v.key) {
        vec![v.key.clone()]
    } else {
        Vec::new()
    };
    s.push_str(",\"threads\":{");
    let mut first = true;
    for k in wanted {
        let t = crate::chat::thread(&k, false);
        if t.is_empty() && !(!v.operator && since == 0) {
            continue;
        }
        if !first {
            s.push(',');
        }
        first = false;
        s.push_str(&json_str(&k));
        s.push_str(":[");
        for (i, m) in t.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let kind = match m.kind {
                crate::chat::Kind::NeedToTalk => "help",
                crate::chat::Kind::Text => "text",
                _ => "other",
            };
            s.push_str(&format!(
                "{{\"id\":{},\"at\":{},\"child\":{},\"kind\":\"{kind}\",\"text\":{},\"seen\":{}}}",
                m.id,
                json_str(&m.at),
                m.from_child,
                json_str(&crate::chat::describe(m)),
                m.seen
            ));
        }
        s.push(']');
    }
    s.push_str("}}");
    s
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// ROOM is process-wide; tests that use it take this first.
    pub static LOCK: Mutex<()> = Mutex::new(());

    fn kid(key: &str, nick: &str) -> Viewer {
        Viewer { key: key.into(), nick: nick.into(), operator: false }
    }

    /// The radio net, end to end: the operator opens, everybody talks in the
    /// one room, a comms check is called and answered, and the count is right.
    #[test]
    fn everybody_talks_in_the_room_and_the_comms_check_counts_answers() {
        let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        here("kA", "Amina", "10.0.0.2");
        here("kB", "Kofi", "10.0.0.3");
        assert_eq!(say("op", "Teacher", "Net open", "", true).0, "sent");
        assert_eq!(say("kA", "Amina", "hello", "t1", false).0, "sent");
        let again = say("kA", "Amina", "hello", "t1", false);
        assert!(again.0 == "sent" && again.1.is_none(), "a double tap lands once");
        start_check();
        assert!(answer("kB", "Kofi"));
        assert!(!answer("kB", "Kofi"), "one answer each");
        let (_, got, on, left) = check_summary().unwrap();
        assert_eq!((got, on), (1, 2));
        assert_eq!(left, vec!["Amina".to_string()]);
        let again = start_check();
        assert!(again.text.contains("Not answered yet: Amina"), "{}", again.text);
        let s = state(&kid("kA", "Amina"), 0, 0);
        assert!(s.contains("\"text\":\"hello\"") && s.contains("\"kind\":\"call\"") && s.contains("\"me\":false"), "{s}");
        clear();
    }

    /// Quiet is the room, never HELP; mute is one person; the operator is
    /// never stopped.
    #[test]
    fn quiet_and_mute_stop_the_room_but_never_the_operator() {
        let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        here("kA", "Amina", "10.0.0.2");
        here("kB", "Kofi", "10.0.0.3");
        set_quiet(true);
        assert_eq!(say("kA", "Amina", "hi", "", false).0, "quiet");
        assert_eq!(say("op", "Teacher", "listen", "", true).0, "sent");
        set_quiet(false);
        assert!(set_muted("kB", true));
        assert_eq!(say("kB", "Kofi", "spam", "", false).0, "muted");
        assert_eq!(say("kA", "Amina", "fine", "", false).0, "sent");
        assert!(state(&kid("kB", "Kofi"), 0, 0).contains("\"muted\":true"));
        assert_eq!(remove("kB").map(|x| x.1), Some("Kofi".to_string()));
        clear();
    }

    /// A child sees only their own private chat; the operator sees all of
    /// them, HELP marked; and a page is told only what changed.
    #[test]
    fn a_child_sees_only_their_own_private_chat() {
        let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _c = crate::chat::tests::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        crate::chat::clear();
        here("kA", "Amina", "10.0.0.2");
        here("kB", "Kofi", "10.0.0.3");
        crate::chat::from_child("kA", "Amina", "secret of Amina", crate::chat::Kind::Text, false, "a");
        crate::chat::from_child("kB", "Kofi", "", crate::chat::Kind::NeedToTalk, false, "b");
        let amina = state(&kid("kA", "Amina"), 0, 0);
        assert!(amina.contains("secret of Amina") && !amina.contains("tapped HELP"), "{amina}");
        let kofi = state(&kid("kB", "Kofi"), 0, 0);
        assert!(!kofi.contains("secret of Amina"), "{kofi}");
        let op = state(&Viewer { key: "op".into(), nick: "Teacher".into(), operator: true }, 0, 0);
        assert!(op.contains("secret of Amina") && op.contains("\"help\":true"), "{op}");
        let v = version();
        crate::chat::from_adult("kA", "answer for Amina", crate::chat::Kind::Text, false);
        let later = state(&kid("kB", "Kofi"), v, 1_000_000);
        assert!(!later.contains("answer for Amina"), "{later}");
        let later = state(&kid("kA", "Amina"), v, 1_000_000);
        assert!(later.contains("answer for Amina"), "{later}");
        crate::chat::clear();
        clear();
    }

    /// A waiting page is woken by news, not left for the whole wait.
    #[test]
    fn a_waiting_page_wakes_when_something_happens() {
        let v = version();
        let t = std::thread::spawn(move || {
            let t0 = Instant::now();
            let now = wait(v, Duration::from_secs(10));
            (now, t0.elapsed())
        });
        std::thread::sleep(Duration::from_millis(200));
        pulse(None);
        let (now, took) = t.join().unwrap();
        assert!(now > v);
        assert!(took < Duration::from_secs(5), "{took:?}");
    }

    #[test]
    fn a_message_cannot_close_the_script_it_is_printed_in() {
        assert_eq!(json_str("</script>\"x\""), "\"\\u003c/script\\u003e\\\"x\\\"\"");
    }
}
