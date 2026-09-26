//! Joining another hub's class net from this window: a pupil with a laptop,
//! or a new team member who has just arrived at the site.
//!
//! WHY. The owner, 2026-09-26: the hub is "the ONLY infrastructure in the
//! area", the central point for coordinating a team or a class, and a person
//! arriving with the program on their own laptop had no way in without
//! jumping through hoops. So the start screen has a button: find the hubs
//! nearby, give a name, and be in the chat, with the history, so the morning
//! briefing is there to read.
//!
//! HOW. As a phone does it, over the same doors (serve.rs, /name and /net/),
//! so the person in charge sees this laptop like any other member, and every
//! rule (quiet, mute, pause, the record) applies to it the same way. One
//! background thread keeps a request waiting for news, exactly as the page's
//! script does; sending is a short request of its own.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::json::J;

#[derive(Clone, Debug, Default)]
pub struct Line {
    pub id: u64,
    pub at: String,
    pub key: String,
    pub who: String,
    pub kind: String,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Person {
    pub key: String,
    pub nick: String,
    pub op: bool,
    pub online: bool,
    pub answered: String,
    pub muted: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Msg {
    pub id: u64,
    pub at: String,
    /// Written by this member (false: by the person in charge).
    pub mine: bool,
    pub help: bool,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Link {
    #[default]
    Connecting,
    Online,
    /// Tries so far.
    Lost(u32),
    /// The person in charge paused this computer.
    Paused,
}

/// Everything the window shows, filled in by the waiting thread.
#[derive(Clone, Debug, Default)]
pub struct State {
    pub link: Link,
    pub v: u64,
    pub line: u64,
    pub my_key: String,
    pub my_nick: String,
    pub op_nick: String,
    pub topic: String,
    pub quiet: bool,
    pub muted: bool,
    /// (number, "HH:MM", answered by me)
    pub check: Option<(u64, String, bool)>,
    pub lines: Vec<Line>,
    pub people: Vec<Person>,
    pub thread: Vec<Msg>,
    /// Private messages from the person in charge not yet looked at.
    pub unread: u32,
    /// Something the window should ring for, taken by it.
    pub ring: Option<String>,
    /// What the last send said went wrong, in words.
    pub trouble: Option<String>,
}

pub struct Member {
    pub addr: SocketAddr,
    pub hub: String,
    pub name: String,
    pub state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Member {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// One HTTP request with its own connection. (status, body)
fn http(addr: SocketAddr, method: &str, path: &str, body: &str, wait: Duration) -> std::io::Result<(u16, String)> {
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    s.set_read_timeout(Some(wait))?;
    s.set_write_timeout(Some(Duration::from_secs(10)))?;
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nUser-Agent: GorillaHub/{} (the hub's own window)\r\nConnection: close\r\n",
        env!("CARGO_PKG_VERSION")
    );
    if method == "POST" {
        req.push_str(&format!("Content-Type: application/x-www-form-urlencoded\r\nX-Hub: 1\r\nContent-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw)?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let code = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    Ok((code, rest.to_string()))
}

fn enc(s: &str) -> String {
    crate::page::urlencode(s)
}

fn token() -> String {
    crate::net::random_bytes(6).map(|b| b.iter().map(|x| format!("{x:02x}")).collect()).unwrap_or_else(|| "t".into())
}

/// Where this computer's name is kept between times, so it is asked once.
fn name_file() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("HOME"))?;
    Some(std::path::PathBuf::from(base).join("PortableNetworkHub").join("my-name.txt"))
}

pub fn remembered_name() -> String {
    name_file().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn remember_name(n: &str) {
    if let Some(p) = name_file() {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, n);
    }
}

impl Member {
    /// Join: say the name, then keep a request waiting for news. `repaint`
    /// wakes the window whenever something arrives.
    pub fn join(addr: SocketAddr, hub: &str, name: &str, repaint: impl Fn() + Send + 'static) -> Member {
        let name = crate::page::sanitize_display_name(name).unwrap_or_else(|| "Guest".into());
        remember_name(&name);
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp, who) = (Arc::clone(&state), Arc::clone(&stop), name.clone());
        std::thread::spawn(move || listen(addr, &who, st, sp, repaint));
        Member { addr, hub: hub.to_string(), name, state, stop }
    }

    pub fn snapshot(&self) -> State {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn take_ring(&self) -> Option<String> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).ring.take()
    }

    pub fn seen_private(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).unread = 0;
    }

    fn post(&self, path: &'static str, body: String) {
        let (addr, state) = (self.addr, Arc::clone(&self.state));
        std::thread::spawn(move || {
            let said = match http(addr, "POST", path, &body, Duration::from_secs(10)) {
                Ok((200, w)) => w,
                Ok((403, _)) => "paused".into(),
                Ok(_) | Err(_) => "lost".into(),
            };
            let why = match said.trim() {
                "toofast" => Some("Slow down a little, then send again."),
                "quiet" => Some("The room is quiet: only the person in charge can write in # main just now. HELP and your private chat still work."),
                "muted" => Some("The person in charge muted you in # main. HELP and your private chat still work."),
                "paused" => Some("The person in charge has paused this computer."),
                "lost" => Some("Not sent: the hub could not be reached. Check the wifi and try again."),
                _ => None,
            };
            state.lock().unwrap_or_else(|e| e.into_inner()).trouble = why.map(str::to_string);
        });
    }

    pub fn say_to_room(&self, text: &str) {
        self.post("/net/say", format!("to=%23main&text={}&token={}", enc(text), token()));
    }

    pub fn say_to_charge(&self, text: &str) {
        let key = self.snapshot().my_key;
        self.post("/net/say", format!("to=%40{}&text={}&token={}", enc(&key), enc(text), token()));
    }

    pub fn help(&self) {
        self.post("/net/help", format!("token={}", token()));
    }

    pub fn answer(&self) {
        if let Some(c) = self.state.lock().unwrap_or_else(|e| e.into_inner()).check.as_mut() {
            c.2 = true;
        }
        self.post("/net/answer", String::new());
    }
}

/// The waiting request, over and over, as the page does it.
fn listen(addr: SocketAddr, name: &str, state: Arc<Mutex<State>>, stop: Arc<AtomicBool>, repaint: impl Fn()) {
    let mut named = false;
    let mut fails = 0u32;
    let (mut since, mut line) = (0u64, 0u64);
    while !stop.load(Ordering::Relaxed) {
        if !named {
            match http(addr, "POST", "/name", &format!("who={}", enc(name)), Duration::from_secs(10)) {
                Ok(_) => named = true,
                Err(_) => {
                    fails += 1;
                    state.lock().unwrap_or_else(|e| e.into_inner()).link = Link::Lost(fails);
                    repaint();
                    std::thread::sleep(Duration::from_millis((1500 * fails as u64).min(10_000)));
                    continue;
                }
            }
        }
        let path = format!("/net/wait?since={since}&line={line}");
        match http(addr, "GET", &path, "", Duration::from_secs(40)) {
            Ok((200, body)) => {
                fails = 0;
                if let Some(j) = crate::json::parse(&body) {
                    let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                    apply(&mut s, &j, since == 0);
                    since = s.v;
                    line = s.line;
                }
                repaint();
            }
            Ok((401, _)) => named = false,
            Ok((403, _)) => {
                state.lock().unwrap_or_else(|e| e.into_inner()).link = Link::Paused;
                repaint();
                std::thread::sleep(Duration::from_secs(5));
            }
            _ => {
                fails += 1;
                state.lock().unwrap_or_else(|e| e.into_inner()).link = Link::Lost(fails);
                repaint();
                std::thread::sleep(Duration::from_millis((1500 * fails as u64).min(10_000)));
            }
        }
    }
}

fn apply(s: &mut State, j: &J, first: bool) {
    s.link = Link::Online;
    s.v = j.get("v").num();
    s.my_key = j.get("me").get("key").str().to_string();
    s.my_nick = j.get("me").get("nick").str().to_string();
    s.op_nick = j.get("opnick").str().to_string();
    let topic = j.get("topic").str().to_string();
    s.quiet = j.get("quiet").bool();
    s.muted = j.get("muted").bool();
    let check = match j.get("check") {
        J::Null => None,
        c => Some((c.get("id").num(), c.get("at").str().to_string(), c.get("me").bool())),
    };
    if !first {
        if let Some((id, _, false)) = &check {
            if s.check.as_ref().map(|c| c.0) != Some(*id) {
                s.ring = Some("comms check".into());
            }
        }
        if topic != s.topic && !topic.is_empty() {
            s.ring.get_or_insert_with(|| "topic".into());
        }
    }
    s.topic = topic;
    s.check = check;
    for l in j.get("lines").arr() {
        let id = l.get("id").num();
        if id <= s.line {
            continue;
        }
        s.line = id;
        s.lines.push(Line {
            id,
            at: l.get("at").str().into(),
            key: l.get("k").str().into(),
            who: l.get("who").str().into(),
            kind: l.get("kind").str().into(),
            text: l.get("text").str().into(),
        });
    }
    if s.lines.len() > 600 {
        s.lines.drain(..s.lines.len() - 500);
    }
    s.people = j
        .get("people")
        .arr()
        .iter()
        .map(|p| Person {
            key: p.get("k").str().into(),
            nick: p.get("n").str().into(),
            op: p.get("op").bool(),
            online: p.get("on").bool(),
            answered: p.get("ans").str().into(),
            muted: p.get("muted").bool(),
        })
        .collect();
    for (_, t) in j.get("threads").entries() {
        let before = s.thread.iter().filter(|m| !m.mine).count();
        s.thread = t
            .arr()
            .iter()
            .map(|m| Msg {
                id: m.get("id").num(),
                at: m.get("at").str().into(),
                mine: m.get("child").bool(),
                help: m.get("kind").str() == "help",
                text: m.get("text").str().into(),
            })
            .collect();
        let after = s.thread.iter().filter(|m| !m.mine).count();
        if !first && after > before {
            s.unread += (after - before) as u32;
            s.ring = Some("private".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A laptop joins a real hub over a socket, says a line in # main, and
    /// reads it back, with the room's own rules applied to it.
    #[test]
    fn a_laptop_joins_a_hub_and_talks_in_the_room() {
        let _serial = crate::serve::SESSION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _r = crate::room::tests::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::room::clear();
        let root = crate::scratchdir::scratch("member-join");
        std::fs::write(root.join("briefing.txt"), b"morning briefing").unwrap();
        let listener = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        crate::serve::start(&root, &format!("0.0.0.0:{port}"), 4).expect("serve");
        // Not 127.0.0.1: that is the person in charge. Any other address of
        // this machine is a member, as a laptop on the wifi would be.
        let Some(me) = crate::net::local_addresses().into_iter().find(|a| !a.is_loopback()) else {
            crate::serve::halt();
            return; // no network here at all: nothing to join over
        };
        crate::room::say("op", "Teacher", "Morning briefing: water point at gate B.", "", true);
        let m = Member::join(SocketAddr::from((me, port)), "test hub", "Tango Lima", || {});
        let mut ok = false;
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(100));
            let s = m.snapshot();
            if s.link == Link::Online && s.lines.iter().any(|l| l.text.contains("water point")) {
                ok = true;
                break;
            }
        }
        assert!(ok, "the briefing arrives on joining: {:?}", m.snapshot());
        m.say_to_room("Tango Lima reads you 5 by 5");
        let mut said = false;
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(100));
            if crate::room::snapshot().lines.iter().any(|l| l.nick == "Tango Lima" && l.text.contains("5 by 5")) {
                said = true;
                break;
            }
        }
        assert!(said, "the line reached the room");
        drop(m);
        crate::serve::halt();
        crate::room::clear();
    }
}
