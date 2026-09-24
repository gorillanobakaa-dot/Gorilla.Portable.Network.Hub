//! The trusted adult's page: private help, on the adult's own phone.
//!
//! The owner's decision (2026-09-24): who receives a child's private messages
//! and "I need to talk" is chosen at the start of the lesson, and may be a
//! named trusted adult (a nurse, a protection officer) rather than the
//! teacher, because sometimes the teacher is part of the problem. That adult
//! does not sit at the laptop. They open http://<hub>/adult on their own
//! phone, type their password, and see only the private conversations. A
//! second adult, if one was named, signs in the same way with their own
//! password and sees the same.
//!
//! The password is the one that locks the record (record.rs); signing in
//! checks it the same slow way, and five wrong tries from one device in five
//! minutes stop further tries for a while. Signed in means a random token in
//! a cookie that only this path receives, forgotten when the lesson stops.
//!
//! Said out loud, as everywhere else: this is plain http. Somebody with the
//! wifi password and the right tools could read what crosses the air. It
//! keeps the class, the teacher's screen and a borrowed laptop out; it does
//! not stop a determined eavesdropper in radio range.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::page::{form_decode, html_escape, urlencode};

const SESSION_LIFE: Duration = Duration::from_secs(12 * 3600);
const TRIES: usize = 5;
const TRIES_WINDOW: Duration = Duration::from_secs(300);

/// (token, adult's name, when signed in)
static SESSIONS: Mutex<Vec<(String, String, Instant)>> = Mutex::new(Vec::new());
/// (device address, when) of wrong passwords.
static FAILS: Mutex<Vec<(String, Instant)>> = Mutex::new(Vec::new());

/// Everybody signed out: the lesson stopped.
pub fn forget() {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).clear();
    FAILS.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// The adult signed in on this request, from its Cookie header.
pub fn signed_in(headers: &str) -> Option<String> {
    let token = headers
        .lines()
        .filter(|l| l.to_ascii_lowercase().starts_with("cookie:"))
        .flat_map(|l| l.split_once(':').map(|(_, v)| v).unwrap_or("").split(';'))
        .find_map(|c| c.trim().strip_prefix("hubadult=").map(str::to_string))?;
    let mut s = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    s.retain(|(_, _, at)| at.elapsed() < SESSION_LIFE);
    s.iter().find(|(t, _, _)| *t == token).map(|(_, n, _)| n.clone())
}

/// Too many wrong passwords from this device lately.
pub fn locked_out(ip: &str) -> bool {
    let mut f = FAILS.lock().unwrap_or_else(|e| e.into_inner());
    f.retain(|(_, at)| at.elapsed() < TRIES_WINDOW);
    f.iter().filter(|(i, _)| i == ip).count() >= TRIES
}

/// Check a password; on success, a new session token for the cookie.
pub fn sign_in(ip: &str, body: &str) -> Option<String> {
    if locked_out(ip) {
        return None;
    }
    let pw = body
        .split('&')
        .find_map(|p| p.strip_prefix("password="))
        .map(form_decode)
        .unwrap_or_default();
    match crate::record::who_is(&pw) {
        Some(name) => {
            let token: String = crate::net::random_bytes(16)?.iter().map(|b| format!("{b:02x}")).collect();
            SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).push((token.clone(), name, Instant::now()));
            Some(token)
        }
        None => {
            FAILS.lock().unwrap_or_else(|e| e.into_inner()).push((ip.to_string(), Instant::now()));
            None
        }
    }
}

pub fn sign_out(headers: &str) {
    if let Some(token) = headers
        .lines()
        .filter(|l| l.to_ascii_lowercase().starts_with("cookie:"))
        .flat_map(|l| l.split_once(':').map(|(_, v)| v).unwrap_or("").split(';'))
        .find_map(|c| c.trim().strip_prefix("hubadult=").map(str::to_string))
    {
        SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).retain(|(t, _, _)| *t != token);
    }
}

const STYLE: &str = "<style>body{font-family:sans-serif;margin:0;padding:12px;background:#fff;color:#111;max-width:680px;line-height:1.4}\
h1{font-size:1.3em}.hint{color:#333}.small{font-size:.9em;color:#555}\
input[type=password],textarea{width:100%;box-sizing:border-box;font-size:1.1em;padding:8px;margin:6px 0}textarea{height:5em}\
button,a.btn{display:inline-block;background:#1a6b1a;color:#fff;border:0;border-radius:6px;padding:12px 18px;font-size:1.05em;text-decoration:none;margin:4px 8px 4px 0}\
button.ask{background:#28527a}button.out{background:#555}\
.row{display:block;border:1px solid #ccc;border-radius:6px;padding:10px;margin:8px 0;color:#111;text-decoration:none}\
.new{background:#fff8d6;border-color:#d9c65a}.need{font-weight:bold;color:#a11}\
.bad{background:#fdecea;border:2px solid #c0392b;padding:10px;margin:10px 0}\
iframe{width:100%;height:50vh;border:1px solid #ddd;border-radius:6px}\
.msg{margin:6px 0;padding:8px;border-radius:8px;white-space:pre-wrap;word-break:break-word}\
.child{background:#eef7ee;margin-right:15%}.adult{background:#e8f0fe;margin-left:15%}.who{font-size:.8em;color:#555}</style>";

fn head(title: &str, refresh: Option<u32>) -> String {
    let r = refresh.map(|s| format!("<meta http-equiv=\"refresh\" content=\"{s}\">")).unwrap_or_default();
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <meta name=\"referrer\" content=\"no-referrer\">{r}<title>{title}</title>{STYLE}</head><body>\n"
    )
}

pub fn login_page(bad: bool, ip: &str) -> String {
    let mut s = head("Adult page", None);
    s.push_str("<h1>Private help: trusted adults only</h1>\n");
    if !crate::chat::private_on() {
        s.push_str("<p>Private help is not switched on in this lesson.</p></body></html>\n");
        return s;
    }
    if locked_out(ip) {
        s.push_str("<div class=bad>Too many wrong passwords from this phone. Wait five minutes, then try again.</div>\n");
    } else if bad {
        s.push_str("<div class=bad>That password is not right. Check it with whoever set up the lesson.</div>\n");
    }
    s.push_str(
        "<p class=hint>If you were named as a trusted adult for this lesson, type the password \
         you were given. It takes a moment: that slowness is what protects the children's words.</p>\n\
         <form method=post action=\"/adult/in\"><input type=password name=password autocomplete=off \
         autocapitalize=off><br><button type=submit>SIGN IN</button></form>\n\
         <p class=small>Do not let a child or anyone else see this phone while you are signed in.</p>\n\
         </body></html>\n",
    );
    s
}

pub fn inbox_page(name: &str) -> String {
    let mut s = head("Adult page", Some(15));
    s.push_str(&format!(
        "<h1>Private help</h1>\n<p class=small>Signed in as the {}. This list updates every 15 seconds.</p>\n",
        html_escape(name)
    ));
    let convs = crate::chat::conversations(true);
    if convs.is_empty() {
        s.push_str(
            "<p>No child has asked for help yet.</p>\n<p class=hint>When a child taps HELP on their \
             class page and writes, or taps I NEED TO TALK, they appear here. Only you (and the second \
             adult, if one was named) can see this.</p>\n",
        );
    }
    for c in &convs {
        let class = if c.unread > 0 { "row new" } else { "row" };
        let need = if c.needs_talk { "<br><span class=need>ASKED TO TALK. Find a safe, private moment; do not call them out in front of others.</span>" } else { "" };
        let new = if c.unread > 0 { format!(" &middot; <b>{} new</b>", c.unread) } else { String::new() };
        s.push_str(&format!(
            "<a class=\"{class}\" href=\"/adult?c={}\"><b>{}</b>{new} &middot; {}{need}</a>\n",
            urlencode(&c.key),
            html_escape(&c.label),
            html_escape(&c.last_at)
        ));
    }
    s.push_str(
        "<form method=post action=\"/adult/out\"><button type=submit class=out>SIGN OUT</button></form>\n\
         <p class=small>Every word here is kept in a locked record on the laptop; your password, or \
         the second adult's, opens it (hub private-record). It cannot be read without one.</p>\n\
         </body></html>\n",
    );
    s
}

pub fn thread_page(key: &str) -> String {
    crate::chat::adult_opened(key, true);
    let label = crate::chat::conversations(true)
        .into_iter()
        .find(|c| c.key == key)
        .map(|c| c.label)
        .unwrap_or_else(|| "this child".into());
    let k = urlencode(key);
    let t = crate::net::random_bytes(8).map(|b| b.iter().map(|x| format!("{x:02x}")).collect::<String>()).unwrap_or_default();
    let mut s = head("Adult page", None);
    s.push_str(&format!(
        "<a class=btn href=\"/adult\">&#8592; ALL CONVERSATIONS</a>\n<h1>{}</h1>\n\
         <iframe src=\"/adult/frame?c={k}\"></iframe>\n\
         <form method=post action=\"/adult/send\"><input type=hidden name=c value=\"{k}\">\
         <input type=hidden name=token value=\"{t}\">\
         <textarea name=text autocomplete=off placeholder=\"Your answer\"></textarea><br>\
         <button type=submit>SEND</button></form>\n\
         <form method=post action=\"/adult/ask\"><input type=hidden name=c value=\"{k}\">\
         <button type=submit class=ask>ASK QUIETLY TO TALK</button></form>\n\
         <p class=small>ASK QUIETLY TO TALK puts a question on the child's HELP page: \"A trusted \
         adult would like to talk to you. Is that all right?\" with YES, LATER and NO. Nothing rings \
         or pops up on their phone. Their answer appears above.</p>\n</body></html>\n",
        html_escape(&label)
    ));
    s
}

/// The conversation itself, in a frame that refreshes, so an answer being
/// typed below is never lost.
pub fn frame(key: &str) -> String {
    crate::chat::adult_opened(key, true);
    let mut s = head("Adult page", Some(5));
    let msgs = crate::chat::thread(key, true);
    if msgs.is_empty() {
        s.push_str("<p class=small>No messages yet. You can write first, or ask quietly to talk.</p>");
    }
    for m in &msgs {
        let (class, who) = if m.from_child { ("child", "Child") } else { ("adult", "You") };
        let seen = if !m.from_child && m.seen { " &middot; seen by the child" } else { "" };
        s.push_str(&format!(
            "<div class=\"msg {class}\"><span class=who>{who} &middot; {}{seen}</span><br>{}</div>",
            html_escape(&m.at),
            html_escape(&crate::chat::describe(m))
        ));
    }
    s.push_str("<script>window.scrollTo(0,document.body.scrollHeight);</script></body></html>\n");
    s
}

/// A form field's value.
pub fn field(body: &str, name: &str) -> String {
    body.split('&')
        .find_map(|p| p.strip_prefix(&format!("{name}=")))
        .map(form_decode)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_right_password_signs_in_and_guessing_is_stopped() {
        let _c = crate::chat::tests::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _r = crate::record::tests::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        forget();
        let dir = std::env::temp_dir().join(format!("hub-adult-{}", std::process::id()));
        crate::record::start(&dir, &[("trusted adult", "nurse-pass-1")]).unwrap();
        crate::chat::set_receiver(crate::chat::Receiver::TrustedAdult);
        assert!(sign_in("10.0.0.5", "password=wrong").is_none());
        let token = sign_in("10.0.0.5", "password=nurse-pass-1").expect("the right password signs in");
        assert_eq!(signed_in(&format!("GET /adult HTTP/1.1\r\nCookie: a=b; hubadult={token}\r\n")).as_deref(), Some("trusted adult"));
        assert!(signed_in("GET /adult HTTP/1.1\r\nCookie: hubadult=forged\r\n").is_none());
        for _ in 0..TRIES {
            let _ = sign_in("10.0.0.9", "password=guess");
        }
        assert!(sign_in("10.0.0.9", "password=nurse-pass-1").is_none(), "after five wrong tries even the right one waits");
        sign_out(&format!("Cookie: hubadult={token}\r\n"));
        assert!(signed_in(&format!("Cookie: hubadult={token}\r\n")).is_none());
        crate::record::stop();
        crate::chat::set_receiver(crate::chat::Receiver::Off);
        forget();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
