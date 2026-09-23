// Version: 1.0.0 · updated 26-09-23-13-00
//
// The parts of Windows the hub needs, found switched off and switched back on.
//
// WHY THIS EXISTS. This program is meant for old laptops in remote places, and
// an old laptop running Windows 10 or 11 has very often been "sped up": a
// tweak list, a debloat script, a friend who knows computers. Nearly all of
// them switch off Internet Connection Sharing and the Mobile Hotspot service,
// because almost nobody shares a connection. Windows then says it "can't set
// up mobile hotspot" and shows an empty box, and never says why. Measured on
// the development laptop, 2026-09-08: two Disabled services were the whole
// cause, proven by switching them on (hotspot works) and off again (the same
// failure comes back).
//
// 0.9.7 explained this and printed the commands to type. Typing commands as
// administrator is exactly what the person this is for cannot do, so this
// does it for them: look, say what is off, and with one permission prompt
// switch it back on. What it changed is written down first, so it can be put
// back exactly as it was.
//
// HOW IT READS. Numbers from the registry, never words from a tool: `sc qc`
// prints DISABLED in English and something else in Swahili or Pashto, while
// the registry's Start value is 4 in every language. Same rule as
// net::start_value_from, which this reuses.
//
// NO SELF-ELEVATION LOOP. tune.rs records a copy that re-launched itself
// elevated whenever a check failed, and filled a desktop with permission
// prompts. Here only the person's own action launches the elevated copy, and
// the elevated copy runs `--apply-elevated`, which never launches anything.
// Whether it worked is decided afterwards by reading the registry again from
// the ordinary copy, not by trusting what the elevated one says.

use std::process::{Command, Stdio};

/// A service the hub needs, and what Windows itself sets it to.
///
/// `start` is Windows' own default: 2 starts with Windows, 3 starts when
/// something asks for it. Mobile Hotspot and Connection Sharing are 3 on a
/// fresh Windows, and 3 works: the hotspot switch wakes them. Only something
/// switched further off than Windows' own setting is changed.
pub struct Need {
    pub name: &'static str,
    pub label: &'static str,
    pub start: u32,
    pub for_what: &'static str,
}

/// Measured, not guessed, for the first two (bench/hotspot-service-isolation.ps1).
/// The rest are the services those two and the hub itself stand on, each one
/// read as a dependency from this laptop's registry on 2026-09-23.
pub const NEEDED: [Need; 7] = [
    Need { name: "icssvc", label: "Windows Mobile Hotspot Service", start: 3, for_what: "making a wifi network" },
    Need { name: "SharedAccess", label: "Internet Connection Sharing", start: 3, for_what: "making a wifi network" },
    Need { name: "WlanSvc", label: "WLAN AutoConfig", start: 2, for_what: "wifi at all" },
    Need { name: "Wcmsvc", label: "Windows Connection Manager", start: 2, for_what: "wifi and the hotspot" },
    Need { name: "BFE", label: "Base Filtering Engine", start: 2, for_what: "connection sharing and the firewall" },
    Need { name: "Dhcp", label: "DHCP Client", start: 2, for_what: "getting an address, on wifi and on a cable" },
    Need { name: "Dnscache", label: "DNS Client", start: 2, for_what: "names, including gorilla.local" },
];

/// One service as found.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub name: String,
    pub label: String,
    /// The registry Start value. None: not installed on this Windows.
    pub start: Option<u32>,
    pub running: bool,
    /// The most switched-off it may be and still work.
    pub want: u32,
    /// Named in NEEDED, rather than pulled in as something they stand on.
    /// Only named ones are started; the rest start when these ask for them.
    pub named: bool,
}

impl Found {
    /// Switched further off than works. 0 boot, 1 system, 2 automatic,
    /// 3 on demand, 4 disabled: a larger number is further off.
    pub fn switched_off(&self) -> bool {
        matches!(self.start, Some(s) if s > self.want)
    }
}

/// `sc config` spells the start types as words, and these words are the
/// command's own syntax, not translated output.
fn sc_word(start: u32) -> &'static str {
    match start {
        0 => "boot",
        1 => "system",
        2 => "auto",
        3 => "demand",
        _ => "disabled",
    }
}

/// The names in a `reg query ... /v DependOnService` answer.
///
/// REG_MULTI_SZ comes out as one line with the items joined by a literal
/// `\0`. A name starting with `+` is a load-order group, not a service.
pub(crate) fn dependencies_from(text: &str) -> Vec<String> {
    for line in text.lines() {
        if !line.contains("REG_MULTI_SZ") {
            continue;
        }
        let Some(value) = line.split("REG_MULTI_SZ").nth(1) else { continue };
        return value
            .trim()
            .split("\\0")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !s.starts_with('+'))
            .collect();
    }
    Vec::new()
}

/// Whether `sc query` says RUNNING, read as the number 4 on the STATE line.
/// The field name is the tool's own; the word after the number is translated.
pub(crate) fn running_from(text: &str) -> bool {
    for line in text.lines() {
        let t = line.trim_start();
        if !t.starts_with("STATE") {
            continue;
        }
        let Some((_, rest)) = t.split_once(':') else { continue };
        return rest.split_whitespace().next() == Some("4");
    }
    false
}

fn quiet(cmd: &mut Command) -> Option<String> {
    let out = cmd.stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn key(name: &str) -> String {
    format!(r"HKLM\SYSTEM\CurrentControlSet\Services\{name}")
}

fn start_of(name: &str) -> Option<u32> {
    let text = quiet(Command::new("reg").args(["query", &key(name), "/v", "Start"]))?;
    crate::net::start_value_from(&text)
}

fn dependencies_of(name: &str) -> Vec<String> {
    quiet(Command::new("reg").args(["query", &key(name), "/v", "DependOnService"]))
        .map(|t| dependencies_from(&t))
        .unwrap_or_default()
}

fn is_running(name: &str) -> bool {
    quiet(Command::new("sc").args(["query", name]))
        .map(|t| running_from(&t))
        .unwrap_or(false)
}

/// Everything the hub needs, and everything that stands under it, as it is
/// on this machine right now.
///
/// The dependencies are read, not listed: a laptop tweaked by a different
/// list may have switched off something this one never did, and a
/// hand-written list would only ever cover the machine it was written on.
/// They are allowed to be on demand (3); only Disabled is a fault there.
pub fn check() -> Vec<Found> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let mut out: Vec<Found> = Vec::new();
    let mut queue: Vec<(String, String, u32, bool)> = NEEDED
        .iter()
        .map(|n| (n.name.to_string(), n.label.to_string(), n.start, true))
        .collect();
    while let Some((name, label, want, named)) = (!queue.is_empty()).then(|| queue.remove(0)) {
        if out.iter().any(|f| f.name.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let start = start_of(&name);
        for dep in dependencies_of(&name) {
            let seen = out.iter().any(|f| f.name.eq_ignore_ascii_case(&dep))
                || queue.iter().any(|q| q.0.eq_ignore_ascii_case(&dep));
            if !seen {
                queue.push((dep.clone(), format!("{dep} (needed by {name})"), 3, false));
            }
        }
        let running = start.is_some() && is_running(&name);
        out.push(Found { name, label, start, running, want, named });
    }
    out
}

/// What would be changed: switched-off ones raised to what works, named ones
/// that are not running started. Split from the doing so it can be tested.
pub fn plan(found: &[Found]) -> (Vec<&Found>, Vec<&Found>) {
    let raise: Vec<&Found> = found.iter().filter(|f| f.switched_off()).collect();
    let start: Vec<&Found> = found.iter().filter(|f| f.named && f.start.is_some() && !f.running).collect();
    (raise, start)
}

/// Where the before-state is kept, so it can be put back.
pub fn before_file() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    std::path::Path::new(&base).join("PortableNetworkHub").join("services-before.txt")
}

/// `name start` per line. Existing lines are kept: a second fix must not
/// overwrite the true original with the already-fixed values.
pub(crate) fn merge_before(existing: &str, changed: &[(String, u32)]) -> String {
    let mut out = existing.to_string();
    for (name, start) in changed {
        let known = existing
            .lines()
            .any(|l| l.split_whitespace().next().is_some_and(|n| n.eq_ignore_ascii_case(name)));
        if !known {
            out.push_str(&format!("{name} {start}\n"));
        }
    }
    out
}

pub(crate) fn parse_before(text: &str) -> Vec<(String, u32)> {
    text.lines()
        .filter_map(|l| {
            let mut p = l.split_whitespace();
            Some((p.next()?.to_string(), p.next()?.parse().ok()?))
        })
        .collect()
}

/// The elevated half of "turn on". Runs as administrator, never launches
/// anything elevated itself. Exit code 0 when nothing is left switched off.
pub fn apply_elevated(record: &std::path::Path) -> i32 {
    let found = check();
    let (raise, start) = plan(&found);
    // Written BEFORE changing anything, so a failure halfway still leaves
    // the way back.
    let changed: Vec<(String, u32)> = raise.iter().filter_map(|f| Some((f.name.clone(), f.start?))).collect();
    if !changed.is_empty() {
        let existing = std::fs::read_to_string(record).unwrap_or_default();
        if let Some(dir) = record.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(record, merge_before(&existing, &changed)).is_err() {
            // No record, no change: changing a machine with no way back is
            // the thing this must never do.
            return 3;
        }
    }
    for f in &raise {
        let _ = Command::new("sc")
            .args(["config", &f.name, "start=", sc_word(f.want)])
            .stdin(Stdio::null())
            .output();
    }
    for f in &start {
        let _ = Command::new("sc").args(["start", &f.name]).stdin(Stdio::null()).output();
    }
    if plan(&check()).0.is_empty() { 0 } else { 1 }
}

/// The elevated half of "put back". Stops what was disabled before and
/// restores every recorded start value, then forgets the record.
pub fn put_back_elevated(record: &std::path::Path) -> i32 {
    let Ok(text) = std::fs::read_to_string(record) else { return 0 };
    let before = parse_before(&text);
    let mut ok = true;
    for (name, start) in &before {
        if *start == 4 {
            let _ = Command::new("sc").args(["stop", name]).stdin(Stdio::null()).output();
        }
        let done = Command::new("sc")
            .args(["config", name, "start=", sc_word(*start)])
            .stdin(Stdio::null())
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        ok &= done;
    }
    if ok {
        let _ = std::fs::remove_file(record);
        0
    } else {
        1
    }
}

/// Run this same program as administrator with `args`, and wait for it.
///
/// Err(true) when the person said no to the permission prompt, which is
/// their answer and not a failure.
#[cfg(windows)]
fn run_elevated(args: &str) -> Result<u32, bool> {
    #[repr(C)]
    struct ShellExecuteInfoW {
        cb_size: u32,
        f_mask: u32,
        hwnd: isize,
        verb: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
        inst_app: isize,
        id_list: *mut u8,
        class: *const u16,
        hkey_class: isize,
        hot_key: u32,
        icon_or_monitor: isize,
        process: isize,
    }
    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteExW(info: *mut ShellExecuteInfoW) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn WaitForSingleObject(h: isize, ms: u32) -> u32;
        fn GetExitCodeProcess(h: isize, code: *mut u32) -> i32;
        fn CloseHandle(h: isize) -> i32;
        fn GetLastError() -> u32;
    }
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    let exe = std::env::current_exe().map_err(|_| false)?;
    let verb = wide("runas");
    let file = wide(&exe.to_string_lossy());
    let params = wide(args);
    let mut info = ShellExecuteInfoW {
        cb_size: std::mem::size_of::<ShellExecuteInfoW>() as u32,
        // NOCLOSEPROCESS: hand back the process so it can be waited on.
        // NOASYNC: finish launching before returning.
        f_mask: 0x40 | 0x100,
        hwnd: 0,
        verb: verb.as_ptr(),
        file: file.as_ptr(),
        parameters: params.as_ptr(),
        directory: std::ptr::null(),
        show: 0, // hidden: the answer is shown here, not in a flashing window
        inst_app: 0,
        id_list: std::ptr::null_mut(),
        class: std::ptr::null(),
        hkey_class: 0,
        hot_key: 0,
        icon_or_monitor: 0,
        process: 0,
    };
    unsafe {
        if ShellExecuteExW(&mut info) == 0 {
            // 1223: ERROR_CANCELLED, the person pressed No.
            return Err(GetLastError() == 1223);
        }
        if info.process == 0 {
            return Err(false);
        }
        WaitForSingleObject(info.process, 0xFFFF_FFFF);
        let mut code = 1u32;
        GetExitCodeProcess(info.process, &mut code);
        CloseHandle(info.process);
        Ok(code)
    }
}

#[cfg(not(windows))]
fn run_elevated(_args: &str) -> Result<u32, bool> {
    Err(false)
}

/// What is switched off, in words for the person reading the screen.
pub fn report(found: &[Found]) -> String {
    let off: Vec<&Found> = found.iter().filter(|f| f.switched_off()).collect();
    if off.is_empty() {
        return "Everything the hub needs is switched on.".into();
    }
    let mut s = String::from("Switched off on this computer:\n");
    for f in off {
        let why = NEEDED
            .iter()
            .find(|n| n.name.eq_ignore_ascii_case(&f.name))
            .map(|n| n.for_what)
            .unwrap_or("something the hub needs stands on it");
        s.push_str(&format!("  {}  ({})\n    needed for {}\n", f.label, f.name, why));
    }
    s
}

/// Look, and if anything is off, switch it back on with one permission
/// prompt. Returns the words to show.
pub fn fix() -> String {
    if !cfg!(windows) {
        return "This is only needed on Windows.".into();
    }
    let before = check();
    if plan(&before).0.is_empty() && plan(&before).1.is_empty() {
        return "Everything the hub needs is switched on. Nothing to do.".into();
    }
    let mut s = report(&before);
    let record = before_file();
    let args = format!("services --apply-elevated \"{}\"", record.display());
    match run_elevated(&args) {
        Err(true) => {
            s.push_str("\nNothing was changed: the permission was not given.");
            return s;
        }
        Err(false) => {
            s.push_str(
                "\nWindows would not start the part that switches them on.\n\
                 Right-click the Gorilla Hub icon, choose \"Run as administrator\",\n\
                 and try again.",
            );
            return s;
        }
        Ok(_) => {}
    }
    // Decided by looking again, not by what the elevated copy said.
    let after = check();
    let still = plan(&after).0;
    if still.is_empty() {
        s.push_str("\nAll switched back on. The hub can make a wifi network and use a cable now.\n");
        s.push_str("\nTo put them back exactly as they were later:  hub services --put-back");
    } else {
        s.push_str("\nThese are still switched off:\n");
        for f in still {
            s.push_str(&format!("  {}  ({})\n", f.label, f.name));
        }
        s.push_str(
            "\nSomething is holding them off, often a company or school policy.\n\
             Whoever manages this laptop has to allow them.",
        );
    }
    s
}

/// Undo `fix`, from the record it wrote.
pub fn put_back() -> String {
    let record = before_file();
    let Ok(text) = std::fs::read_to_string(&record) else {
        return "Nothing to put back: the hub has not changed anything here.".into();
    };
    let before = parse_before(&text);
    let args = format!("services --put-back-elevated \"{}\"", record.display());
    match run_elevated(&args) {
        Err(true) => return "Nothing was changed: the permission was not given.".into(),
        Err(false) => return "Windows would not start the part that puts them back.".into(),
        Ok(_) => {}
    }
    let mut wrong = Vec::new();
    for (name, start) in &before {
        if start_of(name) != Some(*start) {
            wrong.push(name.clone());
        }
    }
    if wrong.is_empty() {
        format!("Put back as they were: {}.", before.iter().map(|b| b.0.as_str()).collect::<Vec<_>>().join(", "))
    } else {
        format!("These could not be put back: {}.", wrong.join(", "))
    }
}

/// The `hub services` command.
pub fn run(args: Vec<String>) {
    let arg = |i: usize| args.get(i).map(|s| s.as_str());
    match arg(1) {
        Some("--apply-elevated") => {
            let record = arg(2).map(std::path::PathBuf::from).unwrap_or_else(before_file);
            std::process::exit(apply_elevated(&record));
        }
        Some("--put-back-elevated") => {
            let record = arg(2).map(std::path::PathBuf::from).unwrap_or_else(before_file);
            std::process::exit(put_back_elevated(&record));
        }
        Some("--fix") => println!("{}", fix()),
        Some("--put-back") => println!("{}", put_back()),
        _ => {
            let found = check();
            println!("{}", report(&found).trim_end());
            if found.iter().any(|f| f.switched_off()) {
                println!("\nTo switch them on:  hub services --fix   (Windows will ask permission)");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependencies_are_read_from_the_registry_line() {
        let text = "\r\nHKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Services\\icssvc\r\n    DependOnService    REG_MULTI_SZ    RpcSs\\0wcmsvc\r\n\r\n";
        assert_eq!(dependencies_from(text), vec!["RpcSs", "wcmsvc"]);
        assert_eq!(dependencies_from("    DependOnService    REG_MULTI_SZ    +TDI\\0Afd"), vec!["Afd"]);
        assert!(dependencies_from("ERROR: nothing").is_empty());
    }

    /// The number, not the word: a Windows in another language says 4 too.
    #[test]
    fn running_is_read_as_a_number() {
        assert!(running_from("        STATE              : 4  RUNNING\r\n"));
        assert!(running_from("        STATE              : 4  EN_COURS\r\n"));
        assert!(!running_from("        STATE              : 1  STOPPED\r\n"));
        assert!(!running_from(""));
    }

    fn f(name: &str, start: Option<u32>, running: bool, want: u32, named: bool) -> Found {
        Found { name: name.into(), label: name.into(), start, running, want, named }
    }

    /// This laptop, 2026-09-23: the two hotspot services disabled, everything
    /// else fine. Only those two are raised, to on-demand as on a fresh
    /// Windows, and both are started.
    #[test]
    fn only_what_is_switched_off_is_changed() {
        let found = vec![
            f("icssvc", Some(4), false, 3, true),
            f("SharedAccess", Some(4), false, 3, true),
            f("WlanSvc", Some(2), true, 2, true),
            f("RpcSs", Some(2), true, 3, false),
            f("NlaSvc", Some(3), false, 3, false),
        ];
        let (raise, start) = plan(&found);
        assert_eq!(raise.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["icssvc", "SharedAccess"]);
        assert_eq!(start.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["icssvc", "SharedAccess"]);
        assert_eq!(sc_word(3), "demand");
    }

    /// Automatic-by-default set to on-demand counts as switched off; a
    /// dependency on demand does not; something not installed is left alone.
    #[test]
    fn manual_is_off_only_where_windows_would_start_it() {
        assert!(f("WlanSvc", Some(3), false, 2, true).switched_off());
        assert!(!f("Netman", Some(3), false, 3, false).switched_off());
        assert!(!f("gone", None, false, 2, true).switched_off());
        assert!(f("nativewifip", Some(4), false, 3, false).switched_off());
    }

    /// A second fix keeps the true original, not the already-fixed value.
    #[test]
    fn the_record_keeps_the_first_before() {
        let first = merge_before("", &[("icssvc".into(), 4)]);
        let second = merge_before(&first, &[("icssvc".into(), 3), ("SharedAccess".into(), 4)]);
        assert_eq!(parse_before(&second), vec![("icssvc".into(), 4), ("SharedAccess".into(), 4)]);
    }

    #[test]
    fn the_report_names_what_is_off_and_why() {
        let r = report(&[f("icssvc", Some(4), false, 3, true), f("Dhcp", Some(2), true, 2, true)]);
        assert!(r.contains("icssvc") && r.contains("making a wifi network"));
        assert!(!r.contains("Dhcp"));
        assert_eq!(report(&[]), "Everything the hub needs is switched on.");
    }
}
