//! Two-way messages between the teacher and each child, and the private help
//! channel.
//!
//! WHY THIS EXISTS. Until 0.9.10 a child could send the teacher a note and the
//! teacher could put one line at the top of every page, and that was all: no
//! reply to one child, no history on either side, no sign that anything was
//! read. The owner, 2026-09-24: "not fit for the purpose". And a second need
//! behind the first: in the places this is for, a child may carry something
//! they cannot say in front of the class, their family or visiting officials,
//! and needs a quiet way to reach one trusted adult. Either side can start.
//!
//! THE SHAPE. One list of messages, each belonging to one child's
//! conversation (keyed like names and blocks, by the device's tag where there
//! is one, so a new address does not split a conversation). A message is
//! either ordinary (child and teacher) or PRIVATE (child and the trusted adult,
//! who may be the teacher or somebody else chosen at the start of the lesson).
//! Private messages never appear in the ordinary conversation, on either side.
//!
//! HOW PHONES GET NEW MESSAGES. They ask. Every few seconds a phone sends a
//! tiny request carrying the version of its conversation it last saw; the
//! answer is either "nothing new" or the whole conversation, rendered. A held
//! connection per phone (websockets, long polling) would each occupy one of
//! the server's fixed pool of workers, which is 16 on a two-core laptop: the
//! same countdown the project already measured once (serve.rs, IO_TIMEOUT).
//! A request that finishes at once costs the pool nothing.
//!
//! WHAT IS KEPT. Ordinary messages go to messages.txt beside the received
//! work, like the old notes. Private messages are never written in the clear:
//! see record.rs. In memory everything is forgotten when the lesson stops.

use std::sync::Mutex;

/// A message is a message; a disclosure longer than this can come in parts.
pub const TEXT_CAP: usize = 2000;
/// Messages a device may send per minute. Tapping is what children do best.
const PER_MINUTE: usize = 8;
/// The most messages one conversation keeps in memory and sends to a phone.
/// The permanent record keeps everything.
const KEPT_PER_CONVERSATION: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Words.
    Text,
    /// A child's one tap on HELP, in the ordinary conversation. No typing
    /// needed, nothing to switch on first.
    NeedToTalk,
    /// The adult asks one child, quietly: "I would like to talk to you".
    AdultAsks,
    /// The child's answer to that: yes, later, no.
    AnswerYes,
    AnswerLater,
    AnswerNo,
}

#[derive(Clone, Debug)]
pub struct Msg {
    pub id: u64,
    /// "HH:MM", local time.
    pub at: String,
    /// The conversation: the child's device key.
    pub key: String,
    /// What the child was called when this was sent, for the screen.
    pub label: String,
    pub from_child: bool,
    pub private: bool,
    pub kind: Kind,
    pub text: String,
    /// From the child: the adult has opened the conversation since.
    /// From the adult: the child's page has shown it.
    pub seen: bool,
}

/// Who receives private messages and "I need to talk", chosen at the start of
/// the lesson (the owner's decision, 2026-09-24). Off until somebody sets a
/// password to lock the record with: an unlocked record of a child's
/// disclosure on a shared laptop is a danger in itself, so without a lock
/// there is no private channel at all, and the phones do not offer one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Receiver {
    Off,
    /// On this laptop's screen.
    Teacher,
    /// A named trusted adult, on their own phone, behind a password.
    TrustedAdult,
}

static RECEIVER: Mutex<Receiver> = Mutex::new(Receiver::Off);

pub fn set_receiver(r: Receiver) {
    *RECEIVER.lock().unwrap_or_else(|e| e.into_inner()) = r;
}

pub fn receiver() -> Receiver {
    *RECEIVER.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn private_on() -> bool {
    receiver() != Receiver::Off
}

#[derive(Default)]
struct State {
    msgs: Vec<Msg>,
    next_id: u64,
    /// (key, private, version): bumped on every change a phone should see.
    versions: Vec<(String, bool, u64)>,
    /// (key, when) of recent sends, for the per-minute budget.
    recent: Vec<(String, std::time::Instant)>,
    /// (key, token) already honoured, so a retry or double tap lands once.
    tokens: Vec<(String, String)>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(State::default))
}

fn bump(st: &mut State, key: &str, private: bool) {
    match st.versions.iter_mut().find(|(k, p, _)| k == key && *p == private) {
        Some(v) => v.2 += 1,
        None => st.versions.push((key.to_string(), private, 1)),
    }    // Pages waiting on the class net (room.rs) hear about it at once.
    crate::room::pulse(Some(key));
}

/// The version of one conversation a phone should compare against.
pub fn version(key: &str, private: bool) -> u64 {
    with(|st| st.versions.iter().find(|(k, p, _)| k == key && *p == private).map(|v| v.2).unwrap_or(0))
}

fn clock() -> String {
    let t = crate::net::timestamp();
    t.rsplit(' ').next().unwrap_or("").to_string()
}

/// Forget every message: the lesson stopped. The permanent record stays.
pub fn clear() {
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    crate::room::pulse(None);
}

fn push(st: &mut State, key: &str, label: &str, from_child: bool, private: bool, kind: Kind, text: &str) -> Msg {
    st.next_id += 1;
    let m = Msg {
        id: st.next_id,
        at: clock(),
        key: key.to_string(),
        label: label.to_string(),
        from_child,
        private,
        kind,
        text: text.to_string(),
        seen: false,
    };
    st.msgs.push(m.clone());
    // Bounded per conversation; the oldest go first.
    let count = st.msgs.iter().filter(|x| x.key == key && x.private == private).count();
    if count > KEPT_PER_CONVERSATION {
        if let Some(i) = st.msgs.iter().position(|x| x.key == key && x.private == private) {
            st.msgs.remove(i);
        }
    }
    bump(st, key, private);
    m
}

/// Cut to the cap by characters, never in the middle of one.
fn cap(text: &str) -> String {
    text.trim().chars().take(TEXT_CAP).collect()
}

/// A child sends something. Returns what the page should say:
/// "sent", "empty", "toofast".
pub fn from_child(key: &str, label: &str, text: &str, kind: Kind, private: bool, token: &str) -> (&'static str, Option<Msg>) {
    let text = cap(text);
    if kind == Kind::Text && text.is_empty() {
        return ("empty", None);
    }
    with(|st| {
        // The same submission again (a retry, a double tap): "sent", once.
        if !token.is_empty() {
            if st.tokens.iter().any(|(k, t)| k == key && t == token) {
                return ("sent", None);
            }
            st.tokens.push((key.to_string(), token.to_string()));
            if st.tokens.len() > 2000 {
                st.tokens.drain(..1000);
            }
        }
        st.recent.retain(|(_, t)| t.elapsed().as_secs() < 60);
        // Asking for help is never refused for going too fast.
        let urgent = kind != Kind::Text;
        if !urgent && st.recent.iter().filter(|(k, _)| k == key).count() >= PER_MINUTE {
            return ("toofast", None);
        }
        st.recent.push((key.to_string(), std::time::Instant::now()));
        let m = push(st, key, label, true, private, kind, &text);
        ("sent", Some(m))
    })
}

/// The adult (the teacher, or the trusted adult for private ones) sends.
pub fn from_adult(key: &str, text: &str, kind: Kind, private: bool) -> Option<Msg> {
    let text = cap(text);
    if kind == Kind::Text && text.is_empty() {
        return None;
    }
    Some(with(|st| {
        let label = st
            .msgs
            .iter()
            .rev()
            .find(|m| m.key == key)
            .map(|m| m.label.clone())
            .unwrap_or_default();
        push(st, key, &label, false, private, kind, &text)
    }))
}

/// One conversation, oldest first.
pub fn thread(key: &str, private: bool) -> Vec<Msg> {
    with(|st| st.msgs.iter().filter(|m| m.key == key && m.private == private).cloned().collect())
}

/// The child's page has shown the adult's messages: mark them seen, and tell
/// the adult's side. Returns whether anything changed.
pub fn child_saw(key: &str, private: bool) -> bool {
    with(|st| {
        let mut changed = false;
        for m in st.msgs.iter_mut().filter(|m| m.key == key && m.private == private && !m.from_child && !m.seen) {
            m.seen = true;
            changed = true;
        }
        changed
    })
}

/// The adult opened the conversation: the child's messages are seen, and the
/// child's page shows it.
pub fn adult_opened(key: &str, private: bool) {
    with(|st| {
        let mut changed = false;
        for m in st.msgs.iter_mut().filter(|m| m.key == key && m.private == private && m.from_child && !m.seen) {
            m.seen = true;
            changed = true;
        }
        if changed {
            bump(st, key, private);
        }
    })
}

/// One line per conversation for the adult's list.
#[derive(Clone, Debug)]
pub struct Conversation {
    pub key: String,
    pub label: String,
    pub unread: usize,
    pub last_id: u64,
    pub last_at: String,
    /// The last message, for the list. For private conversations the caller
    /// must not show it until the conversation is deliberately opened.
    pub last_text: String,
    pub needs_talk: bool,
}

/// Every conversation with something in it, most recent first.
pub fn conversations(private: bool) -> Vec<Conversation> {
    with(|st| {
        let mut out: Vec<Conversation> = Vec::new();
        for m in st.msgs.iter().filter(|m| m.private == private) {
            let c = match out.iter_mut().find(|c| c.key == m.key) {
                Some(c) => c,
                None => {
                    out.push(Conversation {
                        key: m.key.clone(),
                        label: String::new(),
                        unread: 0,
                        last_id: 0,
                        last_at: String::new(),
                        last_text: String::new(),
                        needs_talk: false,
                    });
                    out.last_mut().unwrap()
                }
            };
            if m.from_child {
                c.label = m.label.clone();
                if !m.seen {
                    c.unread += 1;
                    if m.kind == Kind::NeedToTalk {
                        c.needs_talk = true;
                    }
                }
            }
            if c.label.is_empty() {
                c.label = m.label.clone();
            }
            c.last_id = m.id;
            c.last_at = m.at.clone();
            c.last_text = describe(m);
        }
        out.sort_by(|a, b| b.last_id.cmp(&a.last_id));
        out
    })
}

/// Messages from children nobody has opened yet.
pub fn unread(private: bool) -> usize {
    with(|st| st.msgs.iter().filter(|m| m.private == private && m.from_child && !m.seen).count())
}

/// Children who tapped HELP and whose conversation nobody has opened since.
pub fn help_waiting() -> usize {
    conversations(false).iter().filter(|c| c.needs_talk).count()
}

/// Adult messages or requests a child's page has not shown yet.
pub fn waiting_for_child(key: &str, private: bool) -> usize {
    with(|st| {
        st.msgs
            .iter()
            .filter(|m| m.key == key && m.private == private && !m.from_child && !m.seen)
            .count()
    })
}

/// What a message says, in words, including the one-tap kinds.
pub fn describe(m: &Msg) -> String {
    match m.kind {
        Kind::Text => m.text.clone(),
        Kind::NeedToTalk => "HELP: this child tapped HELP".into(),
        Kind::AdultAsks => "I would like to talk to you. Is that all right?".into(),
        Kind::AnswerYes => "Yes, I want to talk.".into(),
        Kind::AnswerLater => "Later, not now.".into(),
        Kind::AnswerNo => "No, thank you.".into(),
    }
}

/// Whether the adult's latest request to talk is still unanswered, so the
/// child's page offers the three answers.
pub fn open_request(key: &str) -> bool {
    with(|st| {
        let mine: Vec<&Msg> = st.msgs.iter().filter(|m| m.key == key && m.private).collect();
        let last_ask = mine.iter().rposition(|m| m.kind == Kind::AdultAsks);
        match last_ask {
            None => false,
            Some(i) => !mine[i + 1..].iter().any(|m| {
                matches!(m.kind, Kind::AnswerYes | Kind::AnswerLater | Kind::AnswerNo)
            }),
        }
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // One process-wide store: tests take this lock so they do not see each
    // other's messages.
    pub(crate) static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn a_child_and_the_teacher_can_talk_both_ways() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        let (said, _) = from_child("k1", "Amina", "I can't open lesson 2", Kind::Text, false, "t1");
        assert_eq!(said, "sent");
        from_adult("k1", "Try the READ button", Kind::Text, false).unwrap();
        let t = thread("k1", false);
        assert_eq!(t.len(), 2);
        assert!(t[0].from_child && !t[1].from_child);
        assert_eq!(t[1].label, "Amina", "the reply is filed under the same child");
        assert_eq!(unread(false), 1);
        adult_opened("k1", false);
        assert_eq!(unread(false), 0, "opening the conversation marks it seen");
        assert!(thread("k1", false)[0].seen);
        assert!(child_saw("k1", false), "the child's page showing the reply marks it seen");
        assert!(thread("k1", false)[1].seen);
    }

    #[test]
    fn private_messages_never_appear_in_the_ordinary_conversation() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        from_child("k2", "B", "hello", Kind::Text, false, "a");
        from_child("k2", "B", "something private", Kind::Text, true, "b");
        assert!(thread("k2", false).iter().all(|m| !m.private));
        assert!(thread("k2", false).iter().all(|m| m.text != "something private"));
        assert_eq!(thread("k2", true).len(), 1);
        assert_eq!(conversations(false).len(), 1);
        assert_eq!(conversations(true).len(), 1);
        assert_eq!(unread(false), 1);
        assert_eq!(unread(true), 1);
    }

    #[test]
    fn a_retry_lands_once_and_a_flood_is_slowed_but_help_is_not() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        from_child("k3", "C", "hi", Kind::Text, false, "same");
        from_child("k3", "C", "hi", Kind::Text, false, "same");
        assert_eq!(thread("k3", false).len(), 1, "the same token twice is one message");
        for i in 0..20 {
            from_child("k3", "C", &format!("spam {i}"), Kind::Text, false, &format!("s{i}"));
        }
        assert!(thread("k3", false).len() <= PER_MINUTE, "a flood is capped per minute");
        let (said, _) = from_child("k3", "C", "", Kind::NeedToTalk, true, "help");
        assert_eq!(said, "sent", "asking for help is never refused for going too fast");
    }

    #[test]
    fn a_request_to_talk_stays_open_until_the_child_answers() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        assert!(!open_request("k4"));
        from_adult("k4", "", Kind::AdultAsks, true).unwrap();
        assert!(open_request("k4"));
        from_child("k4", "D", "", Kind::AnswerLater, true, "x");
        assert!(!open_request("k4"));
        assert!(version("k4", true) > 0, "a change bumps the version phones poll with");
    }

    #[test]
    fn empty_text_is_not_a_message_and_long_text_is_capped() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        assert_eq!(from_child("k5", "E", "   ", Kind::Text, false, "e").0, "empty");
        let long = "x".repeat(TEXT_CAP + 50);
        from_child("k5", "E", &long, Kind::Text, false, "f");
        assert_eq!(thread("k5", false)[0].text.chars().count(), TEXT_CAP);
    }
}
