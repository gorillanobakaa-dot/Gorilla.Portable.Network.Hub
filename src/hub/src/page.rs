// Version: 1.0.0 · updated 26-08-25-07-40
//
// The class page: what a kid's browser sees, and everything it can send back.
//
// DESIGN RULE. The kid's total skill set is: join a wifi, tap a notification,
// tap a big button. Everything on this page works with ZERO JavaScript, on a
// browser from 2009, because the second-hand phones this is for run anything.
// Links download, forms upload, an iframe with a meta refresh keeps the file
// list current without wiping a half-typed note. All of that is nineties
// technology, which is the point: it is the subset that works everywhere.
//
// WHY UPLOADS ARE PARSED BY HAND. The whole binary is std only. Multipart
// form encoding is a bounded amount of careful code (find the boundary, strip
// the part headers, stream the payload to disk), and streaming matters: a kid
// hands in a 200 MB video on a laptop with 2 GB of RAM, so the body must go
// to disk as it arrives, never into memory.

use crate::serve::{self};
use std::io::{BufReader, BufWriter, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

/// Everything a kid may hand in. Generous because homework videos are real,
/// bounded because a 2 GB laptop must survive a wrong answer.
const UPLOAD_CAP: u64 = 1024 * 1024 * 1024;
/// A note is a note.
const NOTE_CAP: usize = 4 * 1024;
/// Tokens remembered per device, for idempotent submits.
const TOKENS_KEPT: usize = 32;
/// Notes allowed per device per minute. Tapping is the one thing they do well.
const NOTES_PER_MINUTE: usize = 6;

// ---------------------------------------------------------------- shared state

/// The teacher's notice, shown at the top of every kid's page.
static NOTICE: Mutex<String> = Mutex::new(String::new());

/// Notes the kids sent: (address, shown-as, text). The address is what dedup
/// keys on; the label is what the roster shows. Bounded; the full record is
/// on disk.
static NOTES: Mutex<Vec<(String, String, String)>> = Mutex::new(Vec::new());

/// Tokens already honoured, per device address, so a browser retry or a
/// double-tap lands exactly once. A VecDeque would be tidier; a Vec of 32 is
/// nothing.
static TOKENS_SEEN: Mutex<Vec<(String, Vec<String>)>> = Mutex::new(Vec::new());

/// (device address, when) of recent notes, for the per-minute budget.
static NOTE_TIMES: Mutex<Vec<(String, Instant)>> = Mutex::new(Vec::new());

pub fn set_notice(text: &str) {
    *NOTICE.lock().unwrap_or_else(|e| e.into_inner()) = text.trim().to_string();
}

pub fn notice() -> String {
    NOTICE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The most recent notes, oldest first, for the teacher's roster.
pub fn notes(last: usize) -> Vec<(String, String)> {
    let n = NOTES.lock().unwrap_or_else(|e| e.into_inner());
    n.iter().rev().take(last).rev().map(|(_, who, text)| (who.clone(), text.clone())).collect()
}

/// A kid's claimed name, arriving from the /name form.
pub fn claim_name(peer_ip: &str, body: &str) {
    let mut who = String::new();
    for pair in body.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == "who" {
            who = form_decode(v);
        }
    }
    if let Some(name) = sanitize_display_name(&who) {
        serve::set_claimed_name(peer_ip, &name);
    }
}

/// A name a teacher reads off a screen: letters, digits, spaces and a few
/// joiners, capped short. Angle brackets and quotes go because the name is
/// rendered into HTML; a kid typing markup gets a name, not an element.
pub fn sanitize_display_name(raw: &str) -> Option<String> {
    let mut out = String::new();
    for c in raw.chars() {
        match c {
            c if c.is_alphanumeric() => out.push(c),
            ' ' | '-' | '_' | '.' | '\'' => out.push(c),
            _ => {}
        }
        if out.chars().count() >= 24 {
            break;
        }
    }
    let out = out.trim().to_string();
    if out.is_empty() { None } else { Some(out) }
}

// ---------------------------------------------------------------- captive

/// Is this request the phone's "is there internet?" probe, or anything else
/// that was not addressed to us by our own address?
///
/// The dnsmasq drop-in resolves the probe names to this laptop, so the probe
/// arrives here with the probe's own Host header. Anything whose Host is not
/// one of our addresses gets a redirect to the class page. That single rule is
/// what makes a joining phone pop "Sign in to this network" and open the
/// lesson: the probe expected a particular answer and got a redirect instead.
pub fn is_foreign_host(host: Option<&str>, ours: &[String]) -> bool {
    let Some(h) = host else { return false }; // HTTP/1.0, no Host: serve the page
    let h = h.trim();
    let bare = h.split(':').next().unwrap_or(h).to_ascii_lowercase();
    if bare.is_empty() {
        return false;
    }
    !ours.iter().any(|o| o.eq_ignore_ascii_case(&bare))
}

// ---------------------------------------------------------------- the page

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A fresh token for the forms. Random so it cannot be guessed, per render so
/// each submission is distinct, checked server-side so repeats collapse.
fn fresh_token() -> String {
    match crate::net::random_bytes(8) {
        Some(b) => b.iter().map(|x| format!("{x:02x}")).collect(),
        None => format!("{:x}", std::process::id()),
    }
}

/// Has this token been seen from this device before? Records it either way.
fn token_already_used(peer_ip: &str, token: &str) -> bool {
    if token.is_empty() {
        return false; // ancient browser lost the field; the content net below catches repeats
    }
    let mut all = TOKENS_SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let entry = match all.iter_mut().find(|(ip, _)| ip == peer_ip) {
        Some(e) => e,
        None => {
            all.push((peer_ip.to_string(), Vec::new()));
            all.last_mut().unwrap()
        }
    };
    if entry.1.iter().any(|t| t == token) {
        return true;
    }
    if entry.1.len() >= TOKENS_KEPT {
        entry.1.remove(0);
    }
    entry.1.push(token.to_string());
    false
}

/// The whole page. `done` names a just-finished action so the reloaded page
/// can say so (the POST answered with a redirect here; refresh never
/// resubmits).
pub fn class_page(root: &Path, done: Option<&str>, peer_ip: &str, rename: bool, here: &str) -> String {
    let notice = notice();
    let mut s = String::with_capacity(4096);
    s.push_str(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>Class files</title><style>\
         body{font-family:sans-serif;margin:0;padding:12px;background:#fff;color:#111;max-width:620px}\
         h1{font-size:1.3em}\
         .notice{background:#fff8d6;border:2px solid #d9c65a;padding:10px;font-size:1.15em;margin:10px 0;white-space:pre-wrap}\
         .done{background:#e2f7e2;border:2px solid #58a758;padding:10px;font-size:1.1em;margin:10px 0}\
         .file{margin:14px 0}\
         .name{font-size:1.1em;word-break:break-all}\
         .size{color:#666;font-size:.9em;margin-left:6px}\
         a.btn,button{display:inline-block;background:#1a6b1a;color:#fff;border:0;border-radius:6px;\
         padding:12px 20px;font-size:1.05em;text-decoration:none;margin:4px 8px 0 0}\
         a.read{background:#28527a}\
         textarea,input[type=file]{width:100%;font-size:1em;margin:6px 0}\
         textarea{height:4em}\
         form{margin:18px 0;border-top:1px solid #ddd;padding-top:12px}\
         .escape{background:#fff3cd;border:2px solid #d9a441;padding:10px;margin:8px 0}\
         .escapebtn{background:#b34700;font-size:1.15em;display:block;text-align:center;margin:10px 0}\
         .escapealt{background:#555;display:block;text-align:center;margin:6px 0}\
         iframe{width:100%;border:0;min-height:340px}\
         </style></head><body>\n<h1>Class files</h1>\n",
    );
    // WHO ARE YOU comes before everything else. Thirty identical phones make
    // device models useless to a teacher, so the first thing a device is
    // asked, once per lesson, is a name. Unauthenticated on purpose (no
    // accounts, ever); the honesty comes from the permanent record keeping
    // name, device and address side by side.
    let claimed = serve::claimed_name(peer_ip);
    if claimed.is_none() || rename {
        let current = claimed.unwrap_or_default();
        s.push_str(&format!(
            "<form method=\"post\" action=\"/name\">\
             <b>Before you start: type your name.</b><br>\
             Your teacher needs to know whose work is whose.<br>\
             <input type=\"text\" name=\"who\" value=\"{}\" maxlength=\"24\" \
             style=\"width:100%;font-size:1.2em;margin:8px 0;padding:8px\"><br>\
             <button type=\"submit\">THAT'S ME</button></form>\n",
            html_escape(&current)
        ));
        s.push_str("</body></html>\n");
        return s;
    }
    let me = claimed.unwrap_or_default();
    s.push_str(&format!(
        "<p>You are <b>{}</b>. <a href=\"/?rename=1\">Not you?</a></p>\n",
        html_escape(&me)
    ));
    match done {
        Some("handin") => s.push_str("<div class=done>Handed in. Your teacher has it.</div>\n"),
        Some("note") => s.push_str("<div class=done>Sent. Your teacher can see it.</div>\n"),
        Some("empty") => s.push_str("<div class=done>Nothing was chosen, so nothing was sent.</div>\n"),
        Some("toobig") => s.push_str("<div class=done>That file is too big to hand in this way.</div>\n"),
        Some("cantsave") => s.push_str("<div class=done>It could not be saved on the teacher's computer. Tell your teacher.</div>\n"),
        _ => {}
    }
    if !notice.is_empty() {
        s.push_str(&format!(
            "<div class=notice><b>From your teacher</b><br>{}</div>\n",
            linkify(&html_escape(&notice))
        ));
    }
    // The list lives in its own frame so IT can refresh while a half-typed
    // note on the page below survives. Frames are older than the parents of
    // the kids using this.
    s.push_str("<iframe src=\"/files\"></iframe>\n");
    if serve::handin_available() {
        let token = fresh_token();
        s.push_str(&format!(
            "<form method=\"post\" action=\"/handin\" enctype=\"multipart/form-data\">\
             <b>Hand in your work</b><br>\
             <input type=\"hidden\" name=\"token\" value=\"{token}\">\
             <input type=\"file\" name=\"work\" multiple><br>\
             <small>You can pick more than one.</small><br>\
             <button type=\"submit\">SEND IT TO YOUR TEACHER</button></form>\n"
        ));
        // The escape hatch: a BUTTON, not an address to type.
        //
        // Printing "go to 10.42.0.1" was useless in the field. Mobile
        // browsers autocomplete from history and silently rewrite what was
        // typed: the owner typed exactly those nine characters and Edge sent
        // him to a cached 10.42.0.1:8080 instead. A href is never autofilled,
        // never autocompleted, and never guessed at, because nobody types it.
        //
        // The wifi sign-in window is not a browser: on Android it is a
        // stripped WebView that in most builds has no file chooser wired up,
        // so "Choose file" does nothing at all and the page looks broken.
        // Downloads, reading and notes all work in there; handing in does not.
        //
        // Three layers, because no single one works on every phone:
        //   1. intent:// which is Android's documented way for a page to hand
        //      a URL to the real browser. Breaks out of the sheet when the
        //      WebView honours it.
        //   2. A plain http link, which every device understands. On a laptop
        //      or an iPhone this is simply the right answer; inside the sheet
        //      it at least navigates without anyone typing.
        //   3. The menu instruction, which always exists even when both
        //      links are swallowed.
        let bare = here.split(':').next().unwrap_or(here);
        s.push_str(&format!(
            "<div class=escape><b>Can't pick a file?</b><br>\
             You are in the wifi sign-in window. It can show and download \
             files, but it cannot send them.<br>\
             <a class=\"btn escapebtn\" \
             href=\"intent://{bare}/#Intent;scheme=http;action=android.intent.action.VIEW;end\">\
             OPEN THIS IN MY BROWSER</a>\
             <a class=\"btn escapealt\" href=\"http://{}/\">or tap here</a>\
             <br><small>If neither opens your browser: tap the three dots at \
             the top of this window and choose \"Open in browser\" or \"Use \
             this network as is\". You stay on the class wifi either way.</small>\
             </div>\n",
            html_escape(here),
            bare = html_escape(bare)
        ));
    }
    let token2 = fresh_token();
    s.push_str(&format!(
        "<form method=\"post\" action=\"/note\">\
         <b>Send a note to your teacher</b><br>\
         <input type=\"hidden\" name=\"token\" value=\"{token2}\">\
         <textarea name=\"text\" placeholder=\"type here\"></textarea><br>\
         <button type=\"submit\">SEND THE NOTE</button></form>\n"
    ));
    let _ = root;
    s.push_str("</body></html>\n");
    s
}

/// The refreshing file list inside the iframe. Big buttons, two verbs:
/// READ or PLAY opens the file right now in the browser (the video streams,
/// because byte ranges are already there for resume); GET IT keeps a copy in
/// the phone's Downloads.
pub fn files_frame(root: &Path) -> String {
    let mut s = String::with_capacity(2048);
    s.push_str(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <meta http-equiv=\"refresh\" content=\"10\">\
         <style>body{font-family:sans-serif;margin:0;color:#111}\
         .file{margin:0 0 16px 0}.name{font-size:1.1em;word-break:break-all}\
         .size{color:#666;font-size:.9em;margin-left:6px}\
         a.btn{display:inline-block;background:#1a6b1a;color:#fff;border-radius:6px;\
         padding:12px 20px;font-size:1.05em;text-decoration:none;margin:4px 8px 0 0}\
         a.read{background:#28527a}</style></head><body>\n",
    );
    if !root.exists() {
        s.push_str("<p><b>The folder cannot be reached right now.</b> \
                    If the files live on a USB drive, it may have been unplugged. Tell your teacher.</p>\n");
    }
    let files = serve::visible_files(root);
    if files.is_empty() && root.exists() {
        s.push_str("<p>Nothing is being handed out right now. This page checks by itself, just wait.</p>\n");
    }
    // A phone has to render this, and a child has to scroll it with a thumb.
    // A resource pack of 68,000 files would be a page of many megabytes that no
    // browser on a 2015 Android will finish laying out. What is left off is
    // said out loud: a list that stops silently reads as the whole list.
    const ON_PAGE: usize = 300;
    let total = files.len();
    let shown = total.min(ON_PAGE);
    for (name, size) in files.into_iter().take(ON_PAGE) {
        let esc = html_escape(&name);
        let url = urlencode(&name);
        s.push_str(&format!("<div class=file><span class=name>{esc}</span><span class=size>{}</span><br>", human(size)));
        if openable(&name) {
            let verb = if is_media(&name) { "PLAY" } else { "READ" };
            s.push_str(&format!("<a class=\"btn read\" href=\"/view/{url}\">{verb}</a>"));
        }
        s.push_str(&format!("<a class=btn href=\"/{url}?dl=1\">GET IT</a></div>\n"));
    }
    if total > shown {
        s.push_str(&format!(
            "<p><b>{} more files are being handed out than fit on this page.</b><br>\
             Ask your teacher for the one you need by name, or use the address \
             on their screen from a computer.</p>\n",
            total - shown
        ));
    }
    s.push_str("</body></html>\n");
    s
}

/// The viewer: the file, wrapped in a page whose first element is the way
/// back.
///
/// Found on a real phone 2026-08-25: READ opened the bare file, which is
/// correct in a browser with a back button and a trap inside a captive
/// sign-in sheet, which has none. The owner's words: "I am forever stuck on
/// that page viewing it." A viewer page costs nothing and has a door.
/// What a paused device sees, and the only thing it can reach.
///
/// A dead end on purpose: no form, no button, nothing to press. Every escape
/// hatch that exists elsewhere on the class page is a way to send something,
/// and a paused device sending something is the exact thing being stopped.
///
/// It refreshes itself every fifteen seconds so that unpausing needs nothing
/// from the child. They will have put the phone down; the phone comes back on
/// its own. The whole design assumes the person holding it does not know how
/// to help, and should not have to.
pub fn paused_page() -> String {
    String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <meta http-equiv=\"refresh\" content=\"15\">\
         <title>Paused</title><style>\
         body{font-family:sans-serif;margin:0;padding:24px;background:#fff;color:#111;max-width:620px}\
         h1{font-size:1.4em}\
         .box{background:#fdecea;border:2px solid #c0392b;padding:14px;font-size:1.15em;margin:14px 0}\
         p{font-size:1.05em;line-height:1.5}\
         </style></head><body>\n\
         <h1>Paused</h1>\n\
         <div class=box>Your teacher has paused this device.</div>\n\
         <p>You cannot get the class files or hand anything in right now.\
         Nothing you send will arrive.</p>\n\
         <p>Speak to your teacher. When they let you back in, this page comes\
         back by itself. You do not need to do anything.</p>\n\
         </body></html>\n",
    )
}

pub fn view_page(name: &str) -> String {
    let esc = html_escape(name);
    let url = urlencode(name);
    let mut s = String::with_capacity(1024);
    s.push_str(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <style>body{font-family:sans-serif;margin:0;background:#222;color:#eee}\
         .bar{position:sticky;top:0;background:#1a6b1a;padding:10px}\
         .bar a{color:#fff;font-size:1.1em;text-decoration:none;font-weight:bold}\
         .name{padding:6px 10px;color:#bbb;font-size:.9em;word-break:break-all}\
         img,video{max-width:100%;display:block;margin:0 auto}\
         iframe{width:100%;height:82vh;border:0;background:#fff}\
         audio{width:100%;margin:20px 0}</style></head><body>\n",
    );
    s.push_str(&format!(
        "<div class=bar><a href=\"/\">&#8592; BACK TO THE FILES</a></div><div class=name>{esc}</div>\n"
    ));
    match ext(name).as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" => {
            s.push_str(&format!("<img src=\"/{url}\" alt=\"{esc}\">\n"));
        }
        "mp4" | "webm" => {
            s.push_str(&format!("<video controls autoplay src=\"/{url}\"></video>\n"));
        }
        "mp3" | "ogg" | "m4a" | "wav" => {
            s.push_str(&format!("<audio controls src=\"/{url}\"></audio>\n"));
        }
        _ => {
            s.push_str(&format!("<iframe src=\"/{url}\"></iframe>\n"));
        }
    }
    s.push_str("</body></html>\n");
    s
}

fn human(bytes: u64) -> String {
    const K: f64 = 1000.0;
    let b = bytes as f64;
    if b >= K * K * K {
        format!("{:.1} GB", b / (K * K * K))
    } else if b >= K * K {
        format!("{:.1} MB", b / (K * K))
    } else if b >= K {
        format!("{:.0} KB", b / K)
    } else {
        format!("{bytes} B")
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            // '/' is left alone: these are relative PATHS now, not bare
            // names, and %2F is normalised inconsistently between browsers
            // and proxies. Every other byte is still escaped, so a filename
            // with a space, a hash or an ampersand cannot break the link.
            b'/' => out.push('/'),
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// File types a browser can show or play by itself. Everything else only
/// gets GET IT.
fn openable(name: &str) -> bool {
    // Only what a browser can genuinely display. A .docx opened in a browser
    // is a download prompt wearing a READ button, which is worse than no
    // button at all.
    matches!(ext(name).as_str(),
        "pdf" | "txt" | "md" | "csv" | "jpg" | "jpeg" | "png" | "gif" | "webp"
        | "svg" | "mp4" | "webm" | "mp3" | "ogg" | "m4a" | "wav" | "html" | "htm")
}

fn is_media(name: &str) -> bool {
    matches!(ext(name).as_str(), "mp4" | "webm" | "mp3" | "ogg" | "m4a" | "wav")
}

fn ext(name: &str) -> String {
    name.rsplit('.').next().unwrap_or("").to_ascii_lowercase()
}

/// The type the browser is told, which decides whether READ opens a viewer
/// or falls back to a download.
pub fn content_type(name: &str) -> &'static str {
    match ext(name).as_str() {
        "pdf" => "application/pdf",
        // The formats a European curriculum actually produces. Without these
        // a browser is told "unknown binary" and the phone refuses to open
        // what it just downloaded, which was a real complaint about photos.
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "odp" => "application/vnd.oasis.opendocument.presentation",
        "rtf" => "application/rtf",
        "epub" => "application/epub+zip",
        "zip" => "application/zip",
        "7z" => "application/x-7z-compressed",
        "csv" => "text/csv; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "xml" => "text/xml; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "txt" => "text/plain; charset=utf-8",
        "html" | "htm" => "text/html; charset=utf-8",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

/// A URL inside the teacher's notice becomes tappable. Borrowed from
/// LocalSend, which detects a message that is a link and offers to open it.
fn linkify(escaped: &str) -> String {
    let mut out = String::new();
    for word in escaped.split(' ') {
        if word.starts_with("http://") || word.starts_with("https://") {
            out.push_str(&format!("<a href=\"{word}\">{word}</a>"));
        } else {
            out.push_str(word);
        }
        out.push(' ');
    }
    out.trim_end().to_string()
}

// ---------------------------------------------------------------- notes

/// One note arriving. Returns the ?done= tag for the redirect.
pub fn take_note(peer_ip: &str, body: &str, root: &Path) -> &'static str {
    let mut token = String::new();
    let mut text = String::new();
    for pair in body.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match k {
            "token" => token = form_decode(v),
            "text" => text = form_decode(v),
            _ => {}
        }
    }
    let text = text.trim().to_string();
    if text.is_empty() {
        return "empty";
    }
    let text = truncate_chars(&text, NOTE_CAP);
    // Idempotency first: a retry of the SAME submission says "sent" again and
    // stores nothing. On the links this is built for, retries happen without
    // the kid doing anything, and one honest tap must not land three times.
    if token_already_used(peer_ip, &token) {
        return "note";
    }
    // The content net underneath, for a browser too old to keep the field.
    {
        let n = NOTES.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, _, last_text)) = n.iter().rev().find(|(ip, _, _)| ip == peer_ip) {
            if *last_text == text {
                return "note";
            }
        }
    }
    // The budget, for distinct spam. Silent: the kid still sees "sent", the
    // teacher just is not flooded. Arguing with a bored kid is not a feature.
    {
        let mut times = NOTE_TIMES.lock().unwrap_or_else(|e| e.into_inner());
        times.retain(|(_, t)| t.elapsed().as_secs() < 60);
        let recent = times.iter().filter(|(ip, _)| ip == peer_ip).count();
        if recent >= NOTES_PER_MINUTE {
            return "note";
        }
        times.push((peer_ip.to_string(), Instant::now()));
    }
    let shown = serve::device_label(peer_ip);
    {
        let mut n = NOTES.lock().unwrap_or_else(|e| e.into_inner());
        if n.len() >= 200 {
            n.remove(0);
        }
        n.push((peer_ip.to_string(), shown.clone(), text.clone()));
    }
    // The permanent copy, with EVERYTHING known about the sender. Thirty kids
    // sending the same message for kicks are thirty separate lines here, each
    // carrying the claimed name, the device and the address. The roster shows
    // the friendly name; this file is for the reckoning afterwards.
    let dir = handed_in_dir(root);
    if std::fs::create_dir_all(&dir).is_ok() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("notes.txt")) {
            // Time first, so the log sorts and reads like a register.
            let _ = writeln!(f, "{}  {}: {text}", crate::net::timestamp(), serve::full_label(peer_ip));
        }
    }
    "note"
}

fn form_decode(s: &str) -> String {
    crate::serve::percent_decode(&s.replace('+', " "))
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        if out.len() + c.len_utf8() > max {
            break;
        }
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------- uploads

/// Where handed-in work lives: INSIDE the served folder, at a fixed name that
/// the serving side refuses to serve. Inside, because it is the one place we
/// know the teacher can reach and usually write; refused, because kid A's
/// homework must never be downloadable by kid B. The refusal has its own test.
pub fn handed_in_dir(root: &Path) -> PathBuf {
    root.join("handed-in")
}

/// Strip a filename to something safe: no paths, no traversal, no leading
/// dots, nothing a filesystem chokes on. The kid's phone chose the name; we
/// keep what is recognisable and drop what is dangerous.
pub fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut out = String::new();
    for c in base.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' | ' ' => out.push(c),
            _ => out.push('_'),
        }
    }
    let out = out.trim_matches([' ', '.']).to_string();
    if out.is_empty() {
        "unnamed".to_string()
    } else {
        truncate_chars(&out, 120)
    }
}

pub struct UploadOutcome {
    pub tag: &'static str,
    /// What the roster shows, e.g. "homework.jpg" or "3 files".
    pub shown: Option<String>,
}

/// Where work waits for the teacher to look at it.
///
/// Nothing a child sends lands in the teacher's folder unaccepted. A
/// classroom is a safeguarding environment and the teacher is responsible for
/// what is on her machine, so an upload is a REQUEST, not a delivery. Sitting
/// inside handed-in/ means the serving side already refuses to hand it back
/// out: whatever arrives is never downloadable by the rest of the class, not
/// even while it waits.
pub fn waiting_dir(root: &Path) -> PathBuf {
    handed_in_dir(root).join("waiting")
}

/// Work that has been refused. Moved, never deleted: it may be evidence, and
/// deciding what disappears is not this program's call.
pub fn refused_dir(root: &Path) -> PathBuf {
    handed_in_dir(root).join("refused")
}

/// Read one multipart upload off the wire and land every file in it in the
/// waiting area.
///
/// MULTIPLE FILES, because one assignment is rarely one file: a project is a
/// document and a spreadsheet and three photographs of the work. The parser
/// walks every part rather than stopping at the first one with a filename.
///
/// Streaming, with a rolling tail: the boundary can straddle any read, so the
/// last boundary-length bytes are always held back from disk until the next
/// read proves they are payload rather than the start of the boundary. The
/// tests feed the same body in 3-byte reads to prove that.
pub fn take_upload<R: Read>(
    reader: &mut BufReader<R>,
    headers: &str,
    peer_ip: &str,
    root: &Path,
) -> std::io::Result<UploadOutcome> {
    let content_length: u64 = header_value(headers, "content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let boundary = header_value(headers, "content-type")
        .and_then(|ct| ct.split("boundary=").nth(1).map(|b| b.trim().trim_matches('"').to_string()));
    let Some(boundary) = boundary else {
        return Ok(UploadOutcome { tag: "empty", shown: None });
    };
    if content_length == 0 {
        return Ok(UploadOutcome { tag: "empty", shown: None });
    }
    if content_length > UPLOAD_CAP {
        // The body still has to be drained or the connection is poisoned, but
        // a body this size is exactly what we will not read. Close instead.
        return Ok(UploadOutcome { tag: "toobig", shown: None });
    }

    let mut body = BodyReader { r: reader, left: content_length, pushback: Vec::new() };
    let full_boundary = format!("--{boundary}");
    let marker = format!("\r\n{full_boundary}");

    let dir = waiting_dir(root);
    if std::fs::create_dir_all(&dir).is_err() {
        drain(&mut body)?;
        return Ok(UploadOutcome { tag: "cantsave", shown: None });
    }

    let who = sanitize_filename(&serve::device_label(peer_ip));
    let mut landed: Vec<String> = Vec::new();
    let mut trouble: Option<&'static str> = None;
    let mut line = Vec::new();

    // First line is the opening boundary.
    read_line(&mut body, &mut line)?;
    loop {
        // Part headers.
        let mut disposition = String::new();
        let mut ended = false;
        loop {
            if read_line(&mut body, &mut line)? == 0 {
                ended = true;
                break;
            }
            let text = String::from_utf8_lossy(&line);
            let t = text.trim_end();
            if t.is_empty() {
                break;
            }
            if t.to_ascii_lowercase().starts_with("content-disposition:") {
                disposition = t.to_string();
            }
        }
        if ended {
            break;
        }

        let filename = if disposition.to_ascii_lowercase().contains("filename=") {
            disposition
                .split("filename=")
                .nth(1)
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .split('"')
                .next()
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };

        if filename.is_empty() {
            // A field, or a file input left empty by the browser, which every
            // browser still sends as an empty part. Consume to the next
            // boundary and carry on to the part that matters.
            let last = skip_to_boundary(&mut body, &full_boundary)?;
            if last {
                break;
            }
            continue;
        }

        let clean = sanitize_filename(&filename);
        let tmp = dir.join(format!(".incoming-{}-{}", std::process::id(), landed.len()));
        let mut out = match std::fs::File::create(&tmp) {
            Ok(f) => f,
            Err(_) => {
                trouble = Some("cantsave");
                break;
            }
        };
        let (written, digest, last) =
            stream_part(&mut body, &mut out, marker.as_bytes(), peer_ip, &clean, content_length)?;
        out.flush()?;
        // Synced before it can wear a finished name. The drive this lands on
        // is often the teacher's failsafe USB stick and gets unplugged the
        // moment the lesson ends.
        let _ = out.sync_all();
        drop(out);

        if written == 0 {
            let _ = std::fs::remove_file(&tmp);
        } else {
            match place(&dir, &tmp, &who, &clean, &digest) {
                Some(final_name) => {
                    serve::note_pending(peer_ip, &clean, &final_name, written);
                    landed.push(clean);
                }
                None => {
                    let _ = std::fs::remove_file(&tmp);
                    trouble = Some("cantsave");
                }
            }
        }
        if last || trouble.is_some() {
            break;
        }
    }
    drain(&mut body)?;

    if let Some(t) = trouble {
        return Ok(UploadOutcome { tag: t, shown: None });
    }
    if landed.is_empty() {
        return Ok(UploadOutcome { tag: "empty", shown: None });
    }
    serve::note_direction(peer_ip, &landed[0], 0, content_length, serve::Direction::Done);
    let shown = if landed.len() == 1 {
        landed[0].clone()
    } else {
        format!("{} files", landed.len())
    };
    Ok(UploadOutcome { tag: "handin", shown: Some(shown) })
}

/// Stream one part's payload to `out`, stopping at the boundary.
/// Returns (bytes written, digest, whether that was the CLOSING boundary).
fn stream_part<R: Read>(
    body: &mut BodyReader<R>,
    out: &mut std::fs::File,
    marker: &[u8],
    peer_ip: &str,
    name: &str,
    total: u64,
) -> std::io::Result<(u64, String, bool)> {
    let mut hasher = crate::sha256::Sha256::new();
    let mut tail: Vec<u8> = Vec::new();
    let mut written: u64 = 0;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = body.read(&mut buf)?;
        if n == 0 {
            // Ran out of body without a boundary: write whatever is held back.
            out.write_all(&tail)?;
            hasher.update(&tail);
            written += tail.len() as u64;
            return Ok((written, crate::sha256::hex(&hasher.finish()), true));
        }
        tail.extend_from_slice(&buf[..n]);
        if let Some(pos) = find(&tail, marker) {
            out.write_all(&tail[..pos])?;
            hasher.update(&tail[..pos]);
            written += pos as u64;
            // Two bytes after the boundary tell us whether more parts follow:
            // "--" closes the body, CRLF starts another part.
            let after = pos + marker.len();
            let mut rest = tail[after.min(tail.len())..].to_vec();
            while rest.len() < 2 {
                let m = body.read(&mut buf)?;
                if m == 0 {
                    break;
                }
                rest.extend_from_slice(&buf[..m]);
            }
            let last = rest.starts_with(b"--");
            // Hand back everything after those two bytes. It is the start of
            // the next part and the caller is about to read its headers.
            if rest.len() > 2 {
                body.pushback.extend_from_slice(&rest[2..]);
            }
            return Ok((written, crate::sha256::hex(&hasher.finish()), last));
        }
        if tail.len() > marker.len() {
            let keep = tail.len() - marker.len();
            out.write_all(&tail[..keep])?;
            hasher.update(&tail[..keep]);
            written += keep as u64;
            tail.drain(..keep);
        }
        serve::note_direction(peer_ip, name, n as u64, total, serve::Direction::HandingIn);
    }
}

/// Consume a non-file part. Returns true if that was the closing boundary.
fn skip_to_boundary<R: Read>(body: &mut BodyReader<R>, full_boundary: &str) -> std::io::Result<bool> {
    let mut line = Vec::new();
    loop {
        if read_line(body, &mut line)? == 0 {
            return Ok(true);
        }
        let text = String::from_utf8_lossy(&line);
        let t = text.trim_end();
        if t.starts_with(full_boundary) {
            return Ok(t.ends_with("--"));
        }
    }
}

/// Give the finished file its name, keeping a genuine resubmission and
/// dropping a retry. Returns the name it landed under.
fn place(dir: &Path, tmp: &Path, who: &str, clean: &str, digest: &str) -> Option<String> {
    let base = format!("{who}--{clean}");
    let mut final_name = base.clone();
    let mut version = 1;
    loop {
        let candidate = dir.join(&final_name);
        if !candidate.exists() {
            break;
        }
        // Same device, same name, same bytes: the retry of an upload that
        // already landed. On the links this is for, a phone re-posts without
        // the child doing anything.
        if file_digest(&candidate).as_deref() == Some(digest) {
            let _ = std::fs::remove_file(tmp);
            return Some(final_name);
        }
        version += 1;
        final_name = versioned(&base, version);
    }
    if std::fs::rename(tmp, dir.join(&final_name)).is_err() {
        return None;
    }
    Some(final_name)
}

/// "name.ext" -> "name--v2.ext", keeping the extension where the phone's
/// viewer can still find it.
fn versioned(base: &str, version: u32) -> String {
    match base.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}--v{version}.{ext}"),
        None => format!("{base}--v{version}"),
    }
}

fn ordinal(n: u32) -> &'static str {
    match n % 10 {
        2 if n % 100 != 12 => "nd",
        3 if n % 100 != 13 => "rd",
        _ => "th",
    }
}

fn file_digest(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut hasher = crate::sha256::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(crate::sha256::hex(&hasher.finish()))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn header_value(headers: &str, name: &str) -> Option<String> {
    for line in headers.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(name) {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

fn drain<R: Read>(body: &mut BodyReader<R>) -> std::io::Result<()> {
    let mut buf = [0u8; 16 * 1024];
    while body.read(&mut buf)? > 0 {}
    Ok(())
}

/// Reads exactly the request body and never a byte of the next request.
struct BodyReader<'a, R: Read> {
    r: &'a mut BufReader<R>,
    left: u64,
    /// Bytes already pulled off the socket but not yet consumed.
    ///
    /// Finding a boundary always over-reads: the bytes after it, which belong
    /// to the NEXT part, are sitting in the same buffer. Discarding them ate
    /// the following part's headers, so a three-file upload delivered two.
    /// They go back here instead, and `left` is not charged twice because it
    /// was already decremented when they came off the socket.
    pushback: Vec<u8>,
}

impl<R: Read> Read for BodyReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if !self.pushback.is_empty() {
            let n = buf.len().min(self.pushback.len());
            buf[..n].copy_from_slice(&self.pushback[..n]);
            self.pushback.drain(..n);
            return Ok(n);
        }
        if self.left == 0 {
            return Ok(0);
        }
        let want = buf.len().min(self.left as usize);
        let n = self.r.read(&mut buf[..want])?;
        self.left -= n as u64;
        Ok(n)
    }
}

fn read_line<R: Read>(body: &mut BodyReader<R>, line: &mut Vec<u8>) -> std::io::Result<usize> {
    line.clear();
    let mut b = [0u8; 1];
    while line.len() < 16 * 1024 {
        if body.read(&mut b)? == 0 {
            break;
        }
        line.push(b[0]);
        if b[0] == b'\n' {
            break;
        }
    }
    Ok(line.len())
}

/// The redirect every POST answers with, so refresh never resubmits. This is
/// the 1990s POST-redirect-GET pattern, chosen because it works on browsers
/// older than the kids.
pub fn redirect_done(out: &mut BufWriter<TcpStream>, tag: &str) -> std::io::Result<()> {
    write!(
        out,
        "HTTP/1.1 303 See Other\r\nLocation: /?done={tag}\r\nContent-Length: 0\r\n\r\n"
    )?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicU32, Ordering};

    static DIR_N: AtomicU32 = AtomicU32::new(0);

    fn tmpdir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "hub-page-test-{}-{}",
            std::process::id(),
            DIR_N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Hands the parser its bytes three at a time, so a boundary that
    /// straddles a read edge is the NORMAL case rather than the lucky one.
    struct Trickle {
        data: Vec<u8>,
        pos: usize,
    }

    impl Read for Trickle {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.pos >= self.data.len() {
                return Ok(0);
            }
            let n = buf.len().min(3).min(self.data.len() - self.pos);
            buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    fn browser_shaped_upload(filename: &str, content: &[u8]) -> (String, Vec<u8>) {
        // The exact shape Chrome and Firefox produce: a token field first
        // because the form puts it first, then the file part.
        let b = "----WebKitFormBoundaryHubTest123";
        let mut body = Vec::new();
        body.extend_from_slice(format!(
            "--{b}\r\nContent-Disposition: form-data; name=\"token\"\r\n\r\nabc123\r\n--{b}\r\nContent-Disposition: form-data; name=\"work\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        ).as_bytes());
        body.extend_from_slice(content);
        body.extend_from_slice(format!("\r\n--{b}--\r\n").as_bytes());
        let headers = format!(
            "POST /handin HTTP/1.1\r\nHost: 10.42.0.1\r\nContent-Type: multipart/form-data; boundary={b}\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        (headers, body)
    }

    fn upload(dir: &Path, peer: &str, filename: &str, content: &[u8]) -> UploadOutcome {
        let (headers, body) = browser_shaped_upload(filename, content);
        let mut r = BufReader::new(Trickle { data: body, pos: 0 });
        take_upload(&mut r, &headers, peer, dir).unwrap()
    }

    #[test]
    fn a_browser_upload_lands_byte_for_byte() {
        let dir = tmpdir();
        // Content deliberately contains CR LF and dashes, the bytes most
        // likely to be mistaken for a boundary.
        let content = b"line one\r\n--not the boundary--\r\nline two".to_vec();
        let out = upload(&dir, "10.42.0.90", "homework.txt", &content);
        assert_eq!(out.tag, "handin");
        let landed = dir.join("handed-in/waiting").join("10.42.0.90--homework.txt");
        assert_eq!(std::fs::read(&landed).unwrap(), content, "bytes must survive the trickle");
    }

    #[test]
    fn the_same_upload_twice_lands_once() {
        let dir = tmpdir();
        let content = b"same bytes".to_vec();
        assert_eq!(upload(&dir, "10.42.0.91", "hw.txt", &content).tag, "handin");
        assert_eq!(upload(&dir, "10.42.0.91", "hw.txt", &content).tag, "handin");
        let entries: Vec<_> = std::fs::read_dir(dir.join("handed-in/waiting")).unwrap().flatten().collect();
        assert_eq!(entries.len(), 1, "a retry must not multiply the file: {entries:?}");
    }

    #[test]
    fn a_changed_resubmission_becomes_a_second_version() {
        let dir = tmpdir();
        upload(&dir, "10.42.0.92", "essay.txt", b"first go");
        let out = upload(&dir, "10.42.0.92", "essay.txt", b"fixed it");
        assert_eq!(out.tag, "handin");
        // A real correction is kept beside the first attempt, not on top of it.
        let h = dir.join("handed-in/waiting");
        assert!(h.join("10.42.0.92--essay.txt").exists());
        assert!(h.join("10.42.0.92--essay--v2.txt").exists());
    }

    #[test]
    fn a_hostile_filename_cannot_leave_the_folder() {
        let dir = tmpdir();
        let out = upload(&dir, "10.42.0.93", "../../../../etc/passwd", b"nope");
        assert_eq!(out.tag, "handin");
        // The file landed INSIDE handed-in under a defanged name.
        let entries: Vec<String> = std::fs::read_dir(dir.join("handed-in/waiting"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].ends_with("passwd"), "{entries:?}");
        assert!(!entries[0].contains(".."), "{entries:?}");
        assert!(!dir.join("../../../etc/passwd-test-marker").exists());
    }

    #[test]
    fn sanitize_strips_paths_and_keeps_the_name() {
        assert_eq!(sanitize_filename("C:\\Users\\kid\\Desktop\\hw.docx"), "hw.docx");
        assert_eq!(sanitize_filename("../..//x.pdf"), "x.pdf");
        assert_eq!(sanitize_filename(".hidden"), "hidden");
        assert_eq!(sanitize_filename(""), "unnamed");
        assert_eq!(sanitize_filename("photo (1).jpg"), "photo _1_.jpg");
    }

    #[test]
    fn foreign_hosts_are_probes_and_ours_are_not() {
        let ours = vec!["10.42.0.1".to_string(), "localhost".to_string()];
        assert!(is_foreign_host(Some("connectivitycheck.gstatic.com"), &ours));
        assert!(is_foreign_host(Some("www.msftconnecttest.com"), &ours));
        assert!(!is_foreign_host(Some("10.42.0.1"), &ours));
        assert!(!is_foreign_host(Some("10.42.0.1:8080"), &ours), "a port must not make us foreign");
        assert!(!is_foreign_host(Some("LOCALHOST"), &ours));
        assert!(!is_foreign_host(None, &ours), "HTTP/1.0 with no Host is served, not bounced");
    }

    #[test]
    fn a_note_is_idempotent_and_budgeted() {
        let dir = tmpdir();
        let ip = "10.42.0.94";
        // Same token twice: one note.
        assert_eq!(take_note(ip, "token=tok1&text=hello", &dir), "note");
        assert_eq!(take_note(ip, "token=tok1&text=hello", &dir), "note");
        let mine = |all: &Vec<(String, String)>| all.iter().filter(|(w, _)| w == ip).count();
        assert_eq!(mine(&notes(200)), 1, "a retried token must not land twice");
        // Distinct notes up to the budget.
        for i in 2..=10 {
            let _ = take_note(ip, &format!("token=tok{i}&text=note number {i}"), &dir);
        }
        let n = mine(&notes(200));
        assert!(n <= NOTES_PER_MINUTE, "budget leaked: {n} notes");
        // The permanent copy exists.
        let txt = std::fs::read_to_string(dir.join("handed-in/notes.txt")).unwrap();
        assert!(txt.contains("hello"));
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_folder_answers_cantsave_instead_of_dropping() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmpdir();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let out = upload(&dir, "10.42.0.96", "hw.txt", b"bytes");
        assert_eq!(out.tag, "cantsave", "a read-only drive must be an answer, not an error");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn the_page_asks_for_a_name_first_and_then_shows_files() {
        let dir = tmpdir();
        let before = class_page(&dir, None, "10.42.0.201", false, "10.42.0.1");
        assert!(before.contains("type your name"), "an unnamed device must be asked first");
        assert!(!before.contains("Hand in your work"), "no forms before a name");
        claim_name("10.42.0.201", "who=Amina+N.");
        let after = class_page(&dir, None, "10.42.0.201", false, "10.42.0.1");
        assert!(after.contains("You are <b>Amina N.</b>"), "{after}");
        assert!(after.contains("Send a note") || after.contains("Hand in"), "the page must open up after the name");
    }

    #[test]
    fn a_name_typed_as_markup_becomes_text_not_an_element() {
        claim_name("10.42.0.202", "who=%3Cscript%3Ezap%3C%2Fscript%3E");
        let got = crate::serve::claimed_name("10.42.0.202").unwrap();
        assert!(!got.contains('<') && !got.contains('>'), "{got}");
        assert!(got.contains("script"), "letters survive, markup does not: {got}");
    }

    #[test]
    fn a_note_from_a_named_device_is_attributable_in_the_permanent_record() {
        let dir = tmpdir();
        claim_name("10.42.0.203", "who=Johnny");
        crate::serve::set_device_name("10.42.0.203", "Xiaomi-11-Lite-5G-NE");
        let _ = take_note("10.42.0.203", "token=fj1&text=the+same+message+for+kicks", &dir);
        let txt = std::fs::read_to_string(dir.join("handed-in/notes.txt")).unwrap();
        assert!(txt.contains("Johnny [Xiaomi-11-Lite-5G-NE, 10.42.0.203]:"),
                "the reckoning line needs name, device AND address: {txt}");
    }

    #[test]
    fn a_named_device_hands_in_under_its_name() {
        let dir = tmpdir();
        claim_name("10.42.0.204", "who=Amina");
        let out = upload(&dir, "10.42.0.204", "essay.docx", b"real work");
        assert_eq!(out.tag, "handin");
        assert!(dir.join("handed-in/waiting").join("Amina--essay.docx").exists(),
                "the teacher marks names, not phone models");
    }

    #[test]
    fn the_page_tells_a_sign_in_window_where_to_go_instead() {
        let dir = tmpdir();
        // The server probes writability at start; without that the hand-in
        // form is deliberately hidden and there is nothing to escape from.
        assert!(crate::serve::probe_handin(&dir), "temp dir should be writable");
        claim_name("10.42.0.210", "who=Amina");
        let page = class_page(&dir, None, "10.42.0.210", false, "10.42.0.1");
        assert!(page.contains("it cannot send them"), "the escape hatch must be on the page");
        // A tappable link, never an address to type: mobile browsers rewrite
        // typed addresses from their own history.
        assert!(page.contains("href=\"http://10.42.0.1/\""), "needs a real link: {page}");
        assert!(page.contains("intent://10.42.0.1/"), "needs the Android breakout link");
        assert!(!page.contains("class=addr"), "the type-this-address block should be gone");
    }

    /// One assignment is rarely one file: a project is a document, a
    /// spreadsheet and three photographs of the work.
    #[test]
    fn several_files_in_one_send_all_arrive() {
        let dir = tmpdir();
        let b = "----WebKitFormBoundaryMulti";
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{b}\r\nContent-Disposition: form-data; name=\"token\"\r\n\r\ntok\r\n").as_bytes());
        for (name, content) in [("essay.docx", &b"WORD"[..]), ("data.xlsx", &b"SHEET"[..]), ("photo.jpg", &b"JPEG"[..])] {
            body.extend_from_slice(format!(
                "--{b}\r\nContent-Disposition: form-data; name=\"work\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            ).as_bytes());
            body.extend_from_slice(content);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{b}--\r\n").as_bytes());
        let headers = format!(
            "POST /handin HTTP/1.1\r\nHost: 10.42.0.1\r\nContent-Type: multipart/form-data; boundary={b}\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        // Three bytes at a time, so every boundary straddles a read edge.
        let mut r = BufReader::new(Trickle { data: body, pos: 0 });
        let out = take_upload(&mut r, &headers, "10.42.0.99", &dir).unwrap();
        assert_eq!(out.tag, "handin");
        assert_eq!(out.shown.as_deref(), Some("3 files"));
        let w = dir.join("handed-in/waiting");
        assert_eq!(std::fs::read(w.join("10.42.0.99--essay.docx")).unwrap(), b"WORD");
        assert_eq!(std::fs::read(w.join("10.42.0.99--data.xlsx")).unwrap(), b"SHEET");
        assert_eq!(std::fs::read(w.join("10.42.0.99--photo.jpg")).unwrap(), b"JPEG");
    }

    /// Nothing a child sends reaches the teacher's folder, or the rest of the
    /// class, until she says so.
    #[test]
    fn work_waits_until_accepted_and_a_refusal_is_kept() {
        let dir = tmpdir();
        upload(&dir, "10.42.0.98", "good.txt", b"real work");
        upload(&dir, "10.42.0.98", "bad.jpg", b"not real work");
        assert!(dir.join("handed-in/waiting/10.42.0.98--good.txt").exists());
        assert!(!dir.join("handed-in/10.42.0.98--good.txt").exists(),
                "nothing lands in the teacher's folder unaccepted");

        crate::serve::accept_pending(&dir, "10.42.0.98--good.txt").unwrap();
        assert!(dir.join("handed-in/10.42.0.98--good.txt").exists(), "accepted work moves up");
        assert!(!dir.join("handed-in/waiting/10.42.0.98--good.txt").exists());

        crate::serve::refuse_pending(&dir, "10.42.0.98--bad.jpg").unwrap();
        assert!(dir.join("handed-in/refused/10.42.0.98--bad.jpg").exists(),
                "a refusal is MOVED, never deleted: it may be evidence");
        assert!(!dir.join("handed-in/waiting/10.42.0.98--bad.jpg").exists());
    }

    #[test]
    fn an_empty_note_is_refused() {
        let dir = tmpdir();
        assert_eq!(take_note("10.42.0.95", "token=t&text=++", &dir), "empty");
    }
}
