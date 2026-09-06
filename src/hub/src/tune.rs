// Version: 0.1.0 · updated 26-09-06-14-10
//
// How fast is the wire, and how big should a bite of the file be.
//
// A 64 KB read buffer is right for wifi and wrong for a cable. On a gigabit
// link it means sixteen thousand read/write pairs per gigabyte, and the
// syscalls cost more than the copying does. On a 100 Mbps link a 4 MB buffer
// is worse than useless: it is four megabytes of RAM per device on a machine
// that may have 64 of them, to hold data the wire will take a third of a
// second to drain.
//
// So we ask the operating system what the adapter actually negotiated, once,
// and size the buffer from the answer.
//
// ONCE is the important word. The first version of this called out to
// PowerShell from inside the file-sending loop, which meant every download by
// every device started a PowerShell process: roughly half a second of cold
// start to save a few milliseconds of copying, thirty times over in a
// classroom. The answer cannot change while the program runs without somebody
// physically pulling the cable out, so it is read once and remembered.

use std::net::TcpStream;
use std::sync::OnceLock;

/// What we found out about the wire, worked out once and then reused.
#[derive(Clone, Debug)]
pub struct Wire {
    /// The adapter's name as the system gives it, for `hub doctor` to print.
    pub adapter: String,
    /// Negotiated speed in megabits per second. A guess of 1000 when unknown.
    pub megabits: u64,
    /// Which cable generation that speed implies, in words a person can check
    /// against the printing on the cable in their hand.
    pub cable: String,
    /// How many bytes to move at a time.
    pub chunk: usize,
    /// False when nothing could be read and `megabits` is the fallback guess.
    /// `hub doctor` says so out loud rather than presenting a guess as a fact.
    pub measured: bool,
}

static WIRE: OnceLock<Wire> = OnceLock::new();

/// The wire, measured on first call and remembered afterwards.
pub fn wire() -> &'static Wire {
    WIRE.get_or_init(detect)
}

/// Speed to buffer size.
///
/// The steps are wide because the exact number does not matter much; being on
/// the right order of magnitude does. Anything at or below 100 Mbps is a CAT 5
/// cable or a wifi link and gets the small buffer.
pub fn chunk_for(megabits: u64) -> (String, usize) {
    if megabits >= 10_000 {
        ("CAT 7 or CAT 8 (10 Gbps)".to_string(), 4 * 1024 * 1024)
    } else if megabits >= 2_500 {
        ("CAT 6a or CAT 8 (2.5 to 5 Gbps)".to_string(), 2 * 1024 * 1024)
    } else if megabits >= 1_000 {
        ("CAT 5e or CAT 6 (1 Gbps)".to_string(), 512 * 1024)
    } else {
        ("CAT 5 or wifi (100 Mbps)".to_string(), 128 * 1024)
    }
}

fn guess() -> Wire {
    let (cable, chunk) = chunk_for(1_000);
    Wire {
        adapter: "not identified".to_string(),
        megabits: 1_000,
        cable,
        chunk,
        measured: false,
    }
}

fn detect() -> Wire {
    #[cfg(target_os = "windows")]
    {
        detect_windows()
    }
    #[cfg(target_os = "linux")]
    {
        detect_linux()
    }
    #[cfg(target_os = "macos")]
    {
        detect_macos()
    }
    // FreeBSD, OpenBSD, NetBSD and anything else. The sockets work, the speed
    // reading does not, and a wrong buffer size is slow rather than broken.
    // Without this arm the crate does not compile on those systems at all,
    // which is a worse outcome than a guess.
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        guess()
    }
}

/// Windows: ask PowerShell, and do the sorting in PowerShell rather than here.
///
/// `Get-NetAdapter` reports LinkSpeed as text: "1 Gbps", "173.3 Mbps". Sorting
/// that text put "173.3 Mbps" above "1 Gbps", because '7' sorts after ' ', so a
/// laptop with gigabit ethernet and wifi picked the wifi and served the cable
/// at CAT 5 buffer sizes. The units are parsed to a number before anything is
/// compared, and physical ethernet is weighted above everything else so a
/// plugged-in cable wins even when the wifi happens to be faster.
#[cfg(target_os = "windows")]
fn detect_windows() -> Wire {
    use std::process::Command;

    const SCRIPT: &str = "\
$a = Get-NetAdapter | Where-Object { $_.Status -eq 'Up' \
-and $_.InterfaceDescription -notlike '*Virtual*' \
-and $_.InterfaceDescription -notlike '*Loopback*' } | ForEach-Object { \
$mbps = 100.0; \
if ($_.LinkSpeed -like '*Gbps*') { $mbps = [double]($_.LinkSpeed -replace '[^\\d.]') * 1000.0 } \
elseif ($_.LinkSpeed -like '*Mbps*') { $mbps = [double]($_.LinkSpeed -replace '[^\\d.]') }; \
$isEth = if ($_.Name -like '*Ethernet*' -or $_.InterfaceDescription -like '*Ethernet*') { 10 } else { 1 }; \
[PSCustomObject]@{ Name = $_.Name; Mbps = $mbps; Priority = ($isEth * 10000 + $mbps) } } \
| Sort-Object Priority -Descending | Select-Object -First 1; \
if ($a) { \"$($a.Name)|$($a.Mbps)\" }";

    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .output();

    if let Ok(o) = out {
        let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if let Some((name, speed)) = text.split_once('|') {
            if let Ok(mbps) = speed.trim().parse::<f64>() {
                let megabits = mbps as u64;
                if megabits > 0 {
                    let (cable, chunk) = chunk_for(megabits);
                    return Wire {
                        adapter: name.trim().to_string(),
                        megabits,
                        cable,
                        chunk,
                        measured: true,
                    };
                }
            }
        }
    }
    guess()
}

/// Linux: the kernel already knows, in a file, with no process to start.
///
/// /sys/class/net/<name>/speed is megabits as a plain number. It reads -1 on
/// an interface with no carrier, which is how an unplugged socket is told
/// apart from a plugged one, and wireless interfaces are skipped so a cable
/// is preferred the same way it is on Windows.
#[cfg(target_os = "linux")]
fn detect_linux() -> Wire {
    let dir = match std::fs::read_dir("/sys/class/net") {
        Ok(d) => d,
        Err(_) => return guess(),
    };
    let mut best: Option<Wire> = None;
    for entry in dir.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "lo" || name.starts_with("wl") {
            continue;
        }
        let up = std::fs::read_to_string(path.join("operstate"))
            .map(|s| s.trim() == "up")
            .unwrap_or(false);
        if !up {
            continue;
        }
        let megabits = match std::fs::read_to_string(path.join("speed")) {
            Ok(s) => match s.trim().parse::<i64>() {
                // -1 means "there is no carrier", not "it is very slow".
                Ok(v) if v > 0 => v as u64,
                _ => continue,
            },
            Err(_) => continue,
        };
        let faster = best.as_ref().map(|b| megabits > b.megabits).unwrap_or(true);
        if faster {
            let (cable, chunk) = chunk_for(megabits);
            best = Some(Wire { adapter: name, megabits, cable, chunk, measured: true });
        }
    }
    best.unwrap_or_else(guess)
}

/// macOS: ifconfig prints the negotiated media type rather than a number.
#[cfg(target_os = "macos")]
fn detect_macos() -> Wire {
    use std::process::Command;

    let out = match Command::new("ifconfig").output() {
        Ok(o) => o,
        Err(_) => return guess(),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut megabits = 0u64;
    for line in text.lines() {
        let l = line.trim();
        if !l.starts_with("media:") {
            continue;
        }
        // Longest names first: "1000baseT" contains "100baseT" as a substring
        // and matching the short one first reports a gigabit link as 100 Mbps.
        let found = if l.contains("10Gbase") {
            10_000
        } else if l.contains("5000base") {
            5_000
        } else if l.contains("2500base") {
            2_500
        } else if l.contains("1000base") {
            1_000
        } else if l.contains("100base") {
            100
        } else {
            0
        };
        if found > megabits {
            megabits = found;
        }
    }
    if megabits == 0 {
        return guess();
    }
    let (cable, chunk) = chunk_for(megabits);
    Wire {
        adapter: "ethernet".to_string(),
        megabits,
        cable,
        chunk,
        measured: true,
    }
}

/// Turn off Nagle's algorithm on a connection we are about to send a file down.
///
/// Nagle holds a small write back waiting for more to fill a packet, and the
/// other end's delayed acknowledgement holds the reply back waiting for data.
/// Together they add about 40 ms to every exchange. That is invisible on a
/// download and very visible on a page of buttons.
///
/// The failure is deliberately ignored: a socket that cannot be tuned still
/// works, and a file that transfers slightly slower beats one that does not
/// transfer at all.
pub fn tune_socket(stream: &TcpStream) {
    let _ = stream.set_nodelay(true);
}


// ---------------------------------------------------------------------------
// Letting this computer be reached at all.
//
// The most common way this tool "does not work" is not a bug in it. It is
// Windows Defender silently dropping every incoming connection, so the server
// starts, prints an address, says nothing is wrong, and no phone in the room
// can reach it. From the teacher's side that is indistinguishable from the
// program being broken, which is why there is a command whose whole job is to
// say whether that has happened and to offer to undo it.
// ---------------------------------------------------------------------------

/// The three ports this tool needs to be reachable on, and what each is for.
/// Named here once so the checker, the repairer and the docs cannot drift.
pub const PORTS: [(&str, u16, &str); 3] = [
    ("TCP", 8080, "handing out files"),
    ("UDP", crate::cable::BEACON_PORT, "being found down a cable"),
    ("UDP", crate::dhcp::SERVER_PORT, "giving the other computer an address"),
];

/// Is this computer reachable from the network?
///
/// `None` means the question could not be answered, which is different from
/// "no" and has to be said differently: a firewall we cannot read is not the
/// same as a firewall we know is blocking.
#[cfg(target_os = "windows")]
pub fn reachable() -> Option<bool> {
    use std::process::Command;
    let out = Command::new("netsh")
        .args(["advfirewall", "firewall", "show", "rule", "name=Gorilla Hub Port 8080"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    Some(text.contains("Enabled") && text.contains("Yes"))
}

/// Everywhere else, the firewall is not usually the thing in the way, and
/// guessing at ufw, firewalld, nftables and pf from a process is worse than
/// admitting we do not know.
#[cfg(not(target_os = "windows"))]
pub fn reachable() -> Option<bool> {
    None
}

/// Open the ports. Returns what happened, rather than printing it.
///
/// The two front ends need the same work and different presentation: the
/// command line prints, and the screen has to put the answer in a box because
/// it owns the whole terminal. Anything that printed here would be drawn over
/// by the next frame, and anything that called exit() would kill the screen.
#[cfg(target_os = "windows")]
pub fn open_ports() -> Result<String, String> {
    use std::process::Command;

    let mut opened = Vec::new();
    let mut refused = Vec::new();
    for (proto, port, why) in PORTS {
        let name = format!("Gorilla Hub Port {port}");
        // Delete first so running this twice does not stack up duplicate
        // rules, which is untidy and makes `reachable` ambiguous.
        let _ = Command::new("netsh")
            .args(["advfirewall", "firewall", "delete", "rule", &format!("name={name}")])
            .output();
        let out = Command::new("netsh")
            .args([
                "advfirewall",
                "firewall",
                "add",
                "rule",
                &format!("name={name}"),
                "dir=in",
                "action=allow",
                &format!("protocol={proto}"),
                &format!("localport={port}"),
                "profile=any",
            ])
            .output();
        match out {
            Ok(o) if o.status.success() => opened.push(format!("{proto} {port}, for {why}")),
            _ => refused.push(format!("{proto} {port}, for {why}")),
        }
    }

    if refused.is_empty() {
        Ok(format!(
            "Other devices can now reach this computer.\n\nOpened:\n  {}\n\nThis only has to be done once.",
            opened.join("\n  ")
        ))
    } else {
        // No self-elevation from here.
        //
        // An earlier version tested for administrator rights with `net
        // session` and re-launched itself elevated when that failed. On a
        // machine with the Server service turned off, `net session` fails even
        // when already elevated, so the elevated copy launched another
        // elevated copy, forever, and the desktop filled with permission
        // dialogs faster than they could be dismissed. Asking a person to do
        // one thing is slower and cannot do that.
        Err(format!(
            "These could not be opened:\n  {}\n\nThis is not running as an administrator.\n\nClose this, find the Gorilla Hub icon, right-click it and choose\n\"Run as administrator\". Then try again.",
            refused.join("\n  ")
        ))
    }
}

/// On Linux and macOS the firewall is usually not in the way, and when it is
/// there are four different ones it could be. Printing the exact line to paste
/// is more use than a guess that silently does nothing.
#[cfg(not(target_os = "windows"))]
pub fn open_ports() -> Result<String, String> {
    let mut s = String::from(
        "On this system the firewall is usually not in the way, so try the\ntransfer first.\n\nIf nothing can reach this computer, one of these matches your system.\nRun it as root:\n\n",
    );
    for (proto, port, why) in PORTS {
        let lower = proto.to_lowercase();
        s.push_str(&format!("  {proto} {port}, for {why}:\n"));
        s.push_str(&format!("    ufw:       sudo ufw allow {port}/{lower}\n"));
        s.push_str(&format!("    firewalld: sudo firewall-cmd --add-port={port}/{lower}\n\n"));
    }
    s.push_str(&format!(
        "Port {} needs root here whatever the firewall says, because ports\nbelow 1024 are reserved. Without it a Linux computer on the far end of\nthe cable may need its address set by hand; a Windows or Mac one will\nsort itself out.",
        crate::dhcp::SERVER_PORT
    ));
    Ok(s)
}

/// The `hub fix-firewall` command: do the work, print the answer.
pub fn fix_firewall() {
    println!("Letting this computer be reached from the network.");
    println!();
    match open_ports() {
        Ok(msg) => println!("{msg}"),
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_grows_with_the_wire() {
        assert_eq!(chunk_for(10).1, 128 * 1024);
        assert_eq!(chunk_for(100).1, 128 * 1024);
        assert_eq!(chunk_for(1_000).1, 512 * 1024);
        assert_eq!(chunk_for(2_500).1, 2 * 1024 * 1024);
        assert_eq!(chunk_for(10_000).1, 4 * 1024 * 1024);
    }

    /// The bug that made a gigabit cable serve at CAT 5 speed. 1 Gbps must
    /// outrank 173.3 Mbps, which it does not if the speeds stay as text.
    #[test]
    fn a_gigabit_link_outranks_faster_looking_wifi_text() {
        assert!(chunk_for(1_000).1 > chunk_for(173).1);
    }

    /// Reading the wire must be free after the first time. This is the whole
    /// reason for the OnceLock: the original called out to PowerShell inside
    /// the sending loop.
    #[test]
    fn the_wire_is_measured_once_and_reused() {
        let a = wire();
        let b = wire();
        assert!(std::ptr::eq(a, b));
    }
}
