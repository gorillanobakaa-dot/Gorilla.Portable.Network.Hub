// Version: 0.1.0 · updated 26-08-24-22-45
//
// Finding the other machine, and creating the network when there is none.
//
// The discovery problem is smaller than it looks. Whoever runs the hotspot IS
// the default gateway for everyone connected to it, so "where is the teacher"
// has the same answer as "what is my gateway". No beacons, no multicast, no
// service discovery protocol: one route lookup and a TCP connect.
//
// Falling back to a fixed list covers the case where the teacher is not the
// gateway (a room that does have a router). Those three addresses are what the
// three common hotspot implementations hand out and they are worth trying
// because trying costs 400 ms.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_millis(400);

/// NetworkManager's shared mode, Windows Mobile Hotspot, Android tethering.
const KNOWN_HOTSPOT_GATEWAYS: [&str; 3] = ["10.42.0.1", "192.168.137.1", "192.168.43.1"];

// ---------------------------------------------------------------- addresses

/// The address this machine would use to reach `target`.
///
/// A connected UDP socket sends nothing: connect() on UDP only sets the default
/// destination, and the kernel picks a source address by consulting the routing
/// table. Reading it back is a route lookup with no packets and no privileges,
/// and it is the same code on Linux and Windows. Enumerating interfaces
/// properly needs getifaddrs or GetAdaptersAddresses, which is per-platform FFI
/// to answer a question this already answers.
pub fn source_address_for(target: Ipv4Addr) -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect(SocketAddr::new(IpAddr::V4(target), 9)).ok()?;
    match sock.local_addr().ok()? {
        SocketAddr::V4(a) if !a.ip().is_unspecified() => Some(*a.ip()),
        _ => None,
    }
}

/// Every address this machine appears to hold, deduplicated.
pub fn local_addresses() -> Vec<Ipv4Addr> {
    let mut probes: Vec<Ipv4Addr> = Vec::new();
    if let Some(g) = default_gateway() {
        probes.push(g);
    }
    for s in KNOWN_HOTSPOT_GATEWAYS {
        if let Ok(a) = s.parse() {
            probes.push(a);
        }
    }
    probes.push(Ipv4Addr::new(8, 8, 8, 8)); // routes via whatever the default is

    let mut found: Vec<Ipv4Addr> = Vec::new();
    for p in probes {
        if let Some(a) = source_address_for(p) {
            if !a.is_loopback() && !found.contains(&a) {
                found.push(a);
            }
        }
    }
    found
}

/// The default gateway, read from the kernel's own routing table.
#[cfg(target_os = "linux")]
pub fn default_gateway() -> Option<Ipv4Addr> {
    // /proc/net/route holds the addresses as little-endian hex, so 0A2A0001
    // reads back as 1.0.42.10 unless the bytes are reversed. Destination
    // 00000000 is the default route.
    let text = std::fs::read_to_string("/proc/net/route").ok()?;
    for line in text.lines().skip(1) {
        let mut f = line.split_whitespace();
        let _iface = f.next()?;
        let dest = f.next()?;
        let gw = f.next()?;
        if dest != "00000000" {
            continue;
        }
        let raw = u32::from_str_radix(gw, 16).ok()?;
        if raw == 0 {
            continue;
        }
        let b = raw.to_le_bytes();
        return Some(Ipv4Addr::new(b[0], b[1], b[2], b[3]));
    }
    None
}

#[cfg(not(target_os = "linux"))]
pub fn default_gateway() -> Option<Ipv4Addr> {
    // No /proc. Guess .1 of whichever subnet we are on, which is right for
    // every hotspot implementation and for nearly every home router. Parsing
    // `route print` would be more accurate and depends on the console language
    // of the machine, which is a bad thing to depend on in Kabul or Bamako.
    let me = source_address_for(Ipv4Addr::new(8, 8, 8, 8))
        .or_else(|| source_address_for(Ipv4Addr::new(10, 42, 0, 1)))?;
    let o = me.octets();
    Some(Ipv4Addr::new(o[0], o[1], o[2], 1))
}

/// Addresses worth asking "are you handing out files?", best guess first.
pub fn candidate_servers() -> Vec<Ipv4Addr> {
    let mut out: Vec<Ipv4Addr> = Vec::new();
    let mut add = |a: Ipv4Addr| {
        if !out.contains(&a) {
            out.push(a);
        }
    };
    if let Some(g) = default_gateway() {
        add(g);
    }
    for s in KNOWN_HOTSPOT_GATEWAYS {
        if let Ok(a) = s.parse() {
            add(a);
        }
    }
    // .1 of our own subnet, for a hotspot on an address nobody listed.
    for a in local_addresses() {
        let o = a.octets();
        add(Ipv4Addr::new(o[0], o[1], o[2], 1));
    }
    out
}

// ---------------------------------------------------------------- probing

/// Ask every candidate at once and keep the ones that answer.
///
/// Sequentially this would be one timeout after another; a machine that is
/// switched off does not refuse a connection, it says nothing at all, so each
/// dead candidate costs the full 400 ms. In parallel the whole sweep costs 400
/// ms regardless of how many are dead.
pub fn find_servers(port: u16) -> Vec<(Ipv4Addr, usize)> {
    let quick = ask_all(candidate_servers(), port);
    if !quick.is_empty() {
        return quick;
    }
    // Nothing on a gateway. That is the classroom that DOES have a router: the
    // teacher is then an ordinary device on it, at an address nobody can guess,
    // and the whole "the teacher is the gateway" shortcut does not apply.
    //
    // So sweep the neighbourhood. 254 addresses sounds like a lot and is not:
    // a TCP connect to a machine that is switched off costs the full timeout,
    // but in batches of 64 the whole sweep is about a second and a half. Only
    // the addresses that actually accept a connection are then asked what they
    // have, which is the expensive part.
    ask_all(sweep_local_subnet(port), port)
}

/// Every address on our /24 that is listening on `port`.
///
/// A /24 and not the real mask. This network is a /20, which is 4,094
/// addresses and about half a minute of sweeping; the teacher and the class are
/// on the same access point and therefore the same /24 in every case this tool
/// is for. Typing the address by hand covers the rest.
fn sweep_local_subnet(port: u16) -> Vec<Ipv4Addr> {
    let mut open = Vec::new();
    for me in local_addresses() {
        open.extend(sweep_subnet(me, port));
    }
    open
}

/// Sweep the /24 that `me` sits in, skipping `me` itself.
fn sweep_subnet(me: Ipv4Addr, port: u16) -> Vec<Ipv4Addr> {
    let mut open = Vec::new();
    {
        let o = me.octets();
        let mut batch: Vec<std::thread::JoinHandle<Option<Ipv4Addr>>> = Vec::new();
        for last in 1..=254u8 {
            if last == o[3] {
                continue; // ourselves
            }
            let ip = Ipv4Addr::new(o[0], o[1], o[2], last);
            batch.push(std::thread::spawn(move || {
                let addr = SocketAddr::new(IpAddr::V4(ip), port);
                TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).ok().map(|_| ip)
            }));
            // Bounded concurrency. 254 simultaneous sockets on a 2 GB laptop
            // with a 1x1 radio is a way to measure the laptop, not the network.
            if batch.len() >= 64 {
                open.extend(batch.drain(..).filter_map(|h| h.join().ok().flatten()));
            }
        }
        open.extend(batch.into_iter().filter_map(|h| h.join().ok().flatten()));
    }
    open
}

/// Ask each address what it is handing out, all at once.
fn ask_all(addrs: Vec<Ipv4Addr>, port: u16) -> Vec<(Ipv4Addr, usize)> {
    let mut handles = Vec::new();
    for ip in addrs {
        handles.push(std::thread::spawn(move || {
            let addr = SocketAddr::new(IpAddr::V4(ip), port);
            let mut sock = TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).ok()?;
            sock.set_read_timeout(Some(PROBE_TIMEOUT)).ok()?;
            sock.set_write_timeout(Some(PROBE_TIMEOUT)).ok()?;
            // Zero files is still an answer: it means a computer IS handing
            // things out, from an empty folder, which the teacher can fix.
            // Something that is not this program will fail to answer at all.
            let count = list_over(&mut sock, ip, port).ok()?.len();
            Some((ip, count))
        }));
    }
    handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
}

/// One file being handed out: name, size in bytes.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub size: u64,
}

/// Fetch the machine-readable listing. The HTML index is for browsers; this
/// asks for `/?list`, which answers `size<TAB>name` per line.
pub fn list_files(ip: Ipv4Addr, port: u16) -> std::io::Result<Vec<Entry>> {
    let addr = SocketAddr::new(IpAddr::V4(ip), port);
    let mut sock = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    sock.set_read_timeout(Some(Duration::from_secs(10)))?;
    sock.set_write_timeout(Some(Duration::from_secs(10)))?;
    list_over(&mut sock, ip, port)
}

fn list_over(sock: &mut TcpStream, ip: Ipv4Addr, port: u16) -> std::io::Result<Vec<Entry>> {
    write!(sock, "GET /?list HTTP/1.1\r\nHost: {ip}:{port}\r\nConnection: close\r\n\r\n")?;
    sock.flush()?;

    // Read exactly Content-Length, never "until the other end hangs up".
    //
    // read_to_end here waited for a close that a keep-alive server has no
    // reason to send, so listing a machine took the full read timeout and then
    // failed. Asking for close is a request, not a guarantee: an old or a
    // different server is entitled to ignore it. The length is in the reply, so
    // use it.
    let mut raw = Vec::new();
    let mut byte = [0u8; 1];
    while raw.len() < 8192 {
        if sock.read(&mut byte)? == 0 {
            break;
        }
        raw.push(byte[0]);
        if raw.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let header = String::from_utf8_lossy(&raw).to_string();
    if !header.starts_with("HTTP/1.1 200") && !header.starts_with("HTTP/1.0 200") {
        return Ok(Vec::new());
    }
    let len: usize = header
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
        .and_then(|l| l.split_once(':'))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0);
    // A folder of files is a few hundred bytes. The cap is there so a wrong
    // answer cannot make a 2 GB machine allocate its way to death.
    let mut body_bytes = vec![0u8; len.min(1 << 20)];
    sock.read_exact(&mut body_bytes)?;
    let body = String::from_utf8_lossy(&body_bytes).to_string();
    let body = body.as_str();
    let mut out = Vec::new();
    for line in body.lines() {
        if let Some((size, name)) = line.split_once('\t') {
            if let Ok(size) = size.trim().parse() {
                out.push(Entry { name: name.trim().to_string(), size });
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- who joined

/// A device that has joined the network but may not have asked for anything.
#[derive(Clone, Debug)]
pub struct Joined {
    pub ip: Ipv4Addr,
    /// The name the device calls itself, when the DHCP lease recorded one.
    pub name: Option<String>,
}

/// Everything sitting on our hotspot's subnet.
///
/// Reported 2026-08-24: a phone joined the network and the screen still said
/// "Nobody has connected yet", because the only thing being counted was
/// DOWNLOADS. For a teacher those are different questions and the first one
/// comes first: is anybody on my network at all, before anybody has tapped a
/// file. Answering "no" when the answer is yes sends them off checking the
/// password when nothing is wrong.
///
/// Two sources, best first:
///
///   1. The DHCP leases NetworkManager's dnsmasq writes. These carry the
///      device's own name, "Xiaomi-11-Lite-5G-NE" rather than 10.42.0.90,
///      which is what a teacher can actually match to a child. Only readable
///      when running as root: /var/lib/NetworkManager is drwx------.
///   2. /proc/net/arp, which is world readable and needs no privileges at all.
///      No names, but it answers "how many and at what addresses", and a
///      hotspot started through polkit as an ordinary user has nothing else.
#[cfg(target_os = "linux")]
pub fn joined_devices(ours: Ipv4Addr) -> Vec<Joined> {
    let mut out: Vec<Joined> = leases_on(ours);
    for j in arp_on(ours) {
        if !out.iter().any(|e| e.ip == j.ip) {
            out.push(j);
        }
    }
    out.sort_by_key(|j| j.ip.octets());
    out
}

#[cfg(not(target_os = "linux"))]
pub fn joined_devices(_ours: Ipv4Addr) -> Vec<Joined> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn same_subnet(a: Ipv4Addr, b: Ipv4Addr) -> bool {
    let (x, y) = (a.octets(), b.octets());
    x[0] == y[0] && x[1] == y[1] && x[2] == y[2]
}

/// dnsmasq lease line: <expiry> <mac> <ip> <hostname> <client-id>
///
/// The MAC is deliberately not carried out of this function. It is a permanent
/// hardware identifier for somebody else's device, it is of no use to a teacher
/// who has the name and the address, and anything on screen ends up in a
/// screenshot.
#[cfg(target_os = "linux")]
fn leases_on(ours: Ipv4Addr) -> Vec<Joined> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/var/lib/NetworkManager") else {
        return out; // not root, which is normal and not an error
    };
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("dnsmasq-") || !name.ends_with(".leases") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
        out.extend(parse_leases(&text, ours));
    }
    out
}

#[cfg(target_os = "linux")]
fn parse_leases(text: &str, ours: Ipv4Addr) -> Vec<Joined> {
    let mut out = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 4 {
            continue;
        }
        let Ok(ip) = f[2].parse::<Ipv4Addr>() else { continue };
        if ip == ours || !same_subnet(ip, ours) {
            continue;
        }
        // dnsmasq writes "*" when the device offered no name.
        let host = if f[3] == "*" { None } else { Some(f[3].to_string()) };
        out.push(Joined { ip, name: host });
    }
    out
}

#[cfg(target_os = "linux")]
fn arp_on(ours: Ipv4Addr) -> Vec<Joined> {
    let mut out = Vec::new();
    let Ok(text) = std::fs::read_to_string("/proc/net/arp") else { return out };
    out.extend(parse_arp(&text, ours));
    out
}

#[cfg(target_os = "linux")]
fn parse_arp(text: &str, ours: Ipv4Addr) -> Vec<Joined> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 4 {
            continue;
        }
        // Flags 0x2 is a complete entry. An incomplete one is an address we
        // asked about and got no answer for, which is not a device.
        if f[2] != "0x2" {
            continue;
        }
        let Ok(ip) = f[0].parse::<Ipv4Addr>() else { continue };
        if ip == ours || !same_subnet(ip, ours) {
            continue;
        }
        out.push(Joined { ip, name: None });
    }
    out
}

// ---------------------------------------------------------------- randomness

/// Password bytes from the operating system, never from the clock.
///
/// A time-seeded generator is guessable by anyone who knows roughly when the
/// class started, which in a classroom is everybody in the room.
pub fn random_bytes(n: usize) -> Option<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::io::Read as _;
        let mut f = std::fs::File::open("/dev/urandom").ok()?;
        let mut buf = vec![0u8; n];
        f.read_exact(&mut buf).ok()?;
        return Some(buf);
    }
    #[cfg(windows)]
    {
        // RtlGenRandom. Exported by name ordinal as SystemFunction036 and
        // present since Windows XP, which is why it is what everybody uses.
        #[link(name = "advapi32")]
        extern "system" {
            #[link_name = "SystemFunction036"]
            fn rtl_gen_random(buf: *mut u8, len: u32) -> u8;
        }
        let mut buf = vec![0u8; n];
        let ok = unsafe { rtl_gen_random(buf.as_mut_ptr(), n as u32) };
        if ok != 0 {
            return Some(buf);
        }
        return None;
    }
    #[cfg(not(any(unix, windows)))]
    None
}

/// Eight characters a child can copy off a blackboard without asking which
/// letter it is: no 0/O, no 1/l/I. 31 symbols, 8 long, is about 39 bits, which
/// is far past what matters for a network that exists for one lesson.
pub fn suggest_password() -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    match random_bytes(16) {
        // Rejection sampling, not modulo. 256 is not a multiple of 31, so
        // plain % would make the first eight letters slightly likelier than the
        // rest. Cheap to do correctly.
        Some(bytes) => {
            let mut out = String::new();
            for b in bytes {
                if out.len() == 8 {
                    break;
                }
                if (b as usize) < 248 {
                    out.push(ALPHABET[b as usize % ALPHABET.len()] as char);
                }
            }
            if out.len() == 8 {
                out
            } else {
                String::new()
            }
        }
        None => String::new(),
    }
}

// ---------------------------------------------------------------- hotspot

/// The wifi connection to put back, reachable from the heartbeat thread as
/// well as from the guard. Set once, when a hotspot is actually created.
static PREVIOUS_WIFI: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// How long the restore timer waits, and how often it is pushed back.
///
/// The fuse must be comfortably longer than the heartbeat, or a slow tick
/// restores the wifi in the middle of a lesson. Defined once: the command line
/// and the screen both start the same heartbeat, and two copies of these
/// numbers is how one of them ends up shorter than the other.
pub const RESTORE_FUSE: u64 = 180;
pub const HEARTBEAT: u64 = 60;

/// Keep pushing the restore back for as long as this program is alive.
pub fn start_heartbeat() {
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_secs(HEARTBEAT));
        rearm_restore(RESTORE_FUSE);
    });
}

/// Push the restore back out to `seconds` from now.
///
/// Called on a heartbeat while a lesson is running. If the tool stops for any
/// reason at all, including being killed outright, the timer is the thing that
/// still exists and the wifi comes back on its own.
#[cfg(target_os = "linux")]
pub fn rearm_restore(seconds: u64) {
    let Some(prev) = PREVIOUS_WIFI.get() else { return };
    let _ = std::process::Command::new("systemd-run")
        .args([
            "--user",
            "--collect",
            "--unit=hub-wifi-restore",
            &format!("--on-active={seconds}"),
            "nmcli",
            "connection",
            "up",
            prev,
        ])
        .output();
}

#[cfg(not(target_os = "linux"))]
pub fn rearm_restore(_seconds: u64) {}

#[derive(Debug)]
pub struct Hotspot {
    pub ssid: String,
    pub iface: String,
    /// The wifi network that was connected before, so it can be put back.
    previous: Option<String>,
}

/// The first wifi interface NetworkManager knows about.
///
/// Not hardcoded: this machine's is named `Gorilla.WIFI`, a teacher's will be
/// `wlan0` or `wlp2s0`, and a USB adapter bought in a market will be something
/// else again.
#[cfg(target_os = "linux")]
pub fn wifi_interface() -> Option<String> {
    let out = std::process::Command::new("nmcli")
        .args(["-t", "-f", "DEVICE,TYPE", "device"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some((dev, kind)) = line.split_once(':') {
            if kind == "wifi" {
                return Some(dev.to_string());
            }
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
pub fn wifi_interface() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn active_wifi_connection() -> Option<String> {
    let out = std::process::Command::new("nmcli")
        .args(["-t", "-f", "NAME,TYPE", "connection", "show", "--active"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some((name, kind)) = line.rsplit_once(':') {
            if kind.contains("wireless") {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Start handing out a network.
///
/// WPA2 always, never open. An open network means every device in radio range
/// can reach the serving port on the teacher's own laptop, and the teacher has
/// no way to see who is on it. The password is written on the board; that is
/// the whole ceremony.
#[cfg(target_os = "linux")]
pub fn hotspot_up(ssid: &str, password: &str) -> Result<Hotspot, String> {
    if password.chars().count() < 8 {
        return Err("A wifi password has to be at least 8 characters. That is a rule of WPA2, not ours.".into());
    }
    let iface = wifi_interface().ok_or("No wifi adapter found. Is wifi switched on?")?;
    let previous = active_wifi_connection();
    let out = std::process::Command::new("nmcli")
        .args(["device", "wifi", "hotspot", "ifname", &iface, "ssid", ssid, "password", password])
        // No stdin. If this machine wants a polkit password there is nowhere to
        // type it while a full-screen program is drawing, and a hang with no
        // message is worse than a refusal with one.
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("Could not run nmcli: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(explain_nmcli(&err));
    }
    if let Some(p) = &previous {
        let _ = PREVIOUS_WIFI.set(p.clone());
    }
    Ok(Hotspot { ssid: ssid.to_string(), iface, previous })
}

#[cfg(not(target_os = "linux"))]
pub fn hotspot_up(_ssid: &str, _password: &str) -> Result<Hotspot, String> {
    Err("On this system, switch the hotspot on yourself first: \
         Settings, Network and internet, Mobile hotspot. \
         Then come back here and the files will be handed out over it.".into())
}

/// nmcli's errors are written for administrators. This is for a teacher.
#[cfg(target_os = "linux")]
fn explain_nmcli(err: &str) -> String {
    let low = err.to_ascii_lowercase();
    if low.contains("not authorized") || low.contains("permission") || low.contains("polkit") {
        return "This computer will not let a normal account create a network. \
                Close this, then start it again with `sudo hub`."
            .into();
    }
    if low.contains("ap mode") || low.contains("not supported") || low.contains("no suitable") {
        return "This wifi adapter cannot create a network, only join one. \
                Nothing is wrong with the computer; some adapters are built that way. \
                A phone hotspot will work instead."
            .into();
    }
    if low.contains("rfkill") || low.contains("disabled") {
        return "Wifi is switched off. Turn it on and try again.".into();
    }
    format!("The network could not be created.\n{err}")
}

impl Hotspot {
    /// Put the wifi back the way it was found.
    ///
    /// This is the whole reason `previous` is recorded. Twice during
    /// development the machine was left with its only network interface
    /// unmanaged, because the teardown lived in a shell trap that never ran.
    /// A teacher whose laptop loses wifi after a lesson will not use the tool a
    /// second time, and will have no idea what did it.
    #[cfg(target_os = "linux")]
    pub fn down(&self) {
        let _ = std::process::Command::new("nmcli")
            .args(["connection", "down", "Hotspot"])
            .output();
        let _ = std::process::Command::new("nmcli")
            .args(["device", "disconnect", &self.iface])
            .output();
        if let Some(prev) = &self.previous {
            let _ = std::process::Command::new("nmcli")
                .args(["connection", "up", prev])
                .output();
        } else {
            let _ = std::process::Command::new("nmcli")
                .args(["device", "connect", &self.iface])
                .output();
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn down(&self) {}

    /// A restore that survives this program being killed outright.
    ///
    /// Drop does not run on SIGKILL, and `panic = "abort"` means it does not
    /// run on a panic either. systemd owns the timer, this process does not, so
    /// the wifi comes back even if the laptop's battery management kills us or
    /// somebody closes the terminal. Re-armed on a heartbeat while the lesson
    /// is running, so it only ever fires after the tool has actually stopped.
    pub fn arm_restore(&self, seconds: u64) {
        rearm_restore(seconds);
    }

    /// Cancel the safety net, on the way out of a clean shutdown that has
    /// already put the wifi back itself.
    #[cfg(target_os = "linux")]
    pub fn disarm_restore(&self) {
        for unit in ["hub-wifi-restore.timer", "hub-wifi-restore.service"] {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "stop", unit])
                .output();
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn disarm_restore(&self) {}

    /// The address this machine holds ON THE HOTSPOT, asked for rather than
    /// guessed.
    ///
    /// It decides which subnet counts as "the class", so getting it wrong means
    /// either listing nobody or listing a whole office. NetworkManager's shared
    /// mode uses 10.42.0.1 in practice, but that is a default and not a
    /// promise, and a machine with a second interface can easily have another
    /// address that sorts first.
    #[cfg(target_os = "linux")]
    pub fn address(&self) -> Option<Ipv4Addr> {
        let out = std::process::Command::new("nmcli")
            .args(["-g", "IP4.ADDRESS", "device", "show", &self.iface])
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let addr = line.trim().split('/').next().unwrap_or("");
            if let Ok(ip) = addr.parse::<Ipv4Addr>() {
                return Some(ip);
            }
        }
        // Fall back to asking the kernel which source it would use to reach the
        // usual shared-mode gateway.
        source_address_for(Ipv4Addr::new(10, 42, 0, 1))
    }

    #[cfg(not(target_os = "linux"))]
    pub fn address(&self) -> Option<Ipv4Addr> {
        source_address_for(Ipv4Addr::new(192, 168, 137, 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// The sweep is the thing that makes discovery work in a room that has a
    /// router, where the teacher is not the gateway and their address cannot be
    /// guessed. It cannot be tested from one machine using the real subnet,
    /// because the only listener would be on the address the sweep deliberately
    /// skips. 127.0.0.0/8 is all local on Linux, so a listener can be put on a
    /// DIFFERENT address in a sweepable /24.
    #[test]
    fn sweep_finds_a_listener_that_is_not_us() {
        let port = 47311;
        let l = TcpListener::bind(("127.0.0.9", port)).expect("bind 127.0.0.9");
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                drop(s);
            }
        });
        let found = sweep_subnet(Ipv4Addr::new(127, 0, 0, 1), port);
        assert!(found.contains(&Ipv4Addr::new(127, 0, 0, 9)),
                "sweep did not find the listener, found {found:?}");
        assert!(!found.contains(&Ipv4Addr::new(127, 0, 0, 1)),
                "sweep should skip the address it started from");
    }

    /// Parsed against the lines these files really contain. The lease line is
    /// the one dnsmasq wrote when a phone joined the test hotspot on
    /// 2026-08-24, with the hardware address replaced: it is a permanent
    /// identifier for somebody else's device and has no business in a repo.
    #[test]
    fn a_phone_on_the_hotspot_is_seen_with_its_own_name() {
        let ours: Ipv4Addr = "10.42.0.1".parse().unwrap();
        let text = "1787613223 aa:bb:cc:dd:ee:ff 10.42.0.90 Xiaomi-11-Lite-5G-NE 01:aa:bb:cc:dd:ee:ff\n\
                    1787613300 aa:bb:cc:dd:ee:00 10.42.0.31 * 01:aa:bb:cc:dd:ee:00\n\
                    1787613400 aa:bb:cc:dd:ee:11 192.168.1.5 SomewhereElse 01:x\n";
        let got = parse_leases(text, ours);
        assert_eq!(got.len(), 2, "expected the two on our subnet, got {got:?}");
        assert_eq!(got[0].ip, "10.42.0.90".parse::<Ipv4Addr>().unwrap());
        assert_eq!(got[0].name.as_deref(), Some("Xiaomi-11-Lite-5G-NE"));
        // dnsmasq writes * for a device that offered no name; that is "unknown",
        // not a device called "*".
        assert_eq!(got[1].name, None);
    }

    #[test]
    fn arp_counts_only_complete_entries_on_our_subnet() {
        let ours: Ipv4Addr = "10.42.0.1".parse().unwrap();
        let text = "IP address       HW type     Flags       HW address            Mask     Device\n\
                    10.42.0.90       0x1         0x2         aa:bb:cc:dd:ee:ff     *        wlan0\n\
                    10.42.0.77       0x1         0x0         00:00:00:00:00:00     *        wlan0\n\
                    10.42.0.1        0x1         0x2         aa:bb:cc:dd:ee:01     *        wlan0\n\
                    192.168.1.5      0x1         0x2         aa:bb:cc:dd:ee:02     *        eth0\n";
        let got = parse_arp(text, ours);
        // Incomplete (0x0) is an address we asked about and got no answer for.
        // Ourselves and another subnet are not devices on our network.
        assert_eq!(got.len(), 1, "got {got:?}");
        assert_eq!(got[0].ip, "10.42.0.90".parse::<Ipv4Addr>().unwrap());
    }

    /// Non-vacuous check on the one above: with nothing listening, the same
    /// sweep must come back empty. A sweep that returned every address would
    /// pass the test above and be useless.
    #[test]
    fn sweep_finds_nothing_when_nothing_listens() {
        let found = sweep_subnet(Ipv4Addr::new(127, 0, 0, 1), 47312);
        assert!(found.is_empty(), "expected nothing, found {found:?}");
    }

    #[test]
    fn suggested_password_is_long_enough_and_readable() {
        let p = suggest_password();
        assert_eq!(p.chars().count(), 8, "WPA2 needs at least 8");
        assert!(p.chars().all(|c| "abcdefghjkmnpqrstuvwxyz23456789".contains(c)),
                "{p} has a character somebody will misread");
        // Two in a row being identical would mean the random source is not one.
        assert_ne!(p, suggest_password());
    }
}
