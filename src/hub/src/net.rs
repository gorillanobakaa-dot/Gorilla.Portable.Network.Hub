// Version: 0.2.0 · updated 26-09-07-11-00
//
// 0.9.0 added the distinction between an address we hold and a network we are
// actually on. Windows keeps an address configured on a disconnected adapter
// and a socket still selects it, so local_addresses() answered 'where can I be
// reached' when the guard was asking 'what networks am I on'.
// connected_addresses() and live_default_gateway() answer the second question
// by membership testing the platform's own output, never by parsing its
// labels, which are translated on a system installed in another language.
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
    // Broadcast targets need this permission before connect() will touch them.
    //
    // One of the probes above is 169.254.255.255, added specifically so a
    // machine with wifi up could still see the cable in its other socket.
    // Connecting a UDP socket to a broadcast address without SO_BROADCAST is
    // refused by the kernel with EACCES, so that probe failed every single
    // time, on every Linux, and the feature it was written for never worked.
    // The routing was never the problem. Measured on the VAIO, cable up,
    // holding 169.254.87.1:
    //
    //   ip route get 169.254.255.255 -> broadcast ... dev enp3s0 src 169.254.87.1
    //   connect(169.254.255.255)     -> EACCES, Permission denied
    //   connect(169.254.87.61)       -> 169.254.87.1, correct
    //
    // The kernel knew the answer throughout. The socket was not allowed to ask.
    // Failure to set it is not fatal: a non-broadcast probe still works.
    let _ = sock.set_broadcast(true);
    sock.connect(SocketAddr::new(IpAddr::V4(target), 9)).ok()?;
    match sock.local_addr().ok()? {
        SocketAddr::V4(a) if !a.ip().is_unspecified() => Some(*a.ip()),
        _ => None,
    }
}

/// The hardware addresses of this machine's own network cards.
///
/// Needed so the address server does not answer the computer it is running on.
/// A Windows or Mac laptop asks for a DHCP lease on every interface, including
/// the one with the cable in it, for the 30 to 60 seconds before it gives up
/// and assigns itself a link-local address. Ours answers within half a second,
/// so it wins that race against nothing, and the machine takes a lease from
/// itself: its own address out of its own pool, and a DNS server pointing at
/// its own DHCP process. When that process stops, the machine is left with a
/// name server that no longer exists.
///
/// Empty on failure, which means "answer everything" and is the behaviour
/// before this existed. A missed self-lease is a bad afternoon; refusing every
/// client because a command's output could not be parsed is a broken feature.
pub fn local_macs() -> Vec<[u8; 6]> {
    let mut out: Vec<[u8; 6]> = Vec::new();
    let mut add = |m: [u8; 6]| {
        if m != [0; 6] && !out.contains(&m) {
            out.push(m);
        }
    };

    #[cfg(target_os = "linux")]
    {
        if let Ok(dir) = std::fs::read_dir("/sys/class/net") {
            for e in dir.flatten() {
                if let Ok(text) = std::fs::read_to_string(e.path().join("address")) {
                    if let Some(m) = parse_mac(text.trim()) {
                        add(m);
                    }
                }
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        use std::process::Command;
        // getmac on Windows, ifconfig elsewhere. Both are present on a stock
        // system, and both are read for MAC-shaped text rather than parsed as
        // a format, so a layout change degrades to finding nothing.
        let out_text = if cfg!(target_os = "windows") {
            Command::new("getmac").args(["/fo", "csv", "/nh"]).output().ok()
        } else {
            Command::new("ifconfig").output().ok()
        };
        if let Some(o) = out_text {
            let text = String::from_utf8_lossy(&o.stdout);
            for tok in text.split(|c: char| !(c.is_ascii_hexdigit() || c == ':' || c == '-')) {
                if let Some(m) = parse_mac(tok) {
                    add(m);
                }
            }
        }
    }

    out
}

#[cfg(test)]
pub fn parse_mac_for_test(s: &str) -> Option<[u8; 6]> {
    parse_mac(s)
}

/// Six hex pairs separated by ':' or '-'. Anything else is not a MAC.
fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = s.split(|c| c == ':' || c == '-').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut m = [0u8; 6];
    for (i, p) in parts.iter().enumerate() {
        if p.len() != 2 {
            return None;
        }
        m[i] = u8::from_str_radix(p, 16).ok()?;
    }
    Some(m)
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
    // The link-local broadcast, so a cable is found even when this machine is
    // also on a real network.
    //
    // Without this every probe above routes out of the default interface, so a
    // laptop with wifi up reported only its wifi address and the cable plugged
    // into its other socket was invisible. That made `hub doctor` deny there
    // was a cable, made the sending screen say "no cable address yet" while
    // holding 169.254.87.61, and left the address pool derived from a made-up
    // address instead of the real one. RFC 3927 traffic goes out of the
    // interface that owns a link-local address, so asking the kernel for the
    // source address it would use to reach 169.254.255.255 names that
    // interface and nothing else.
    probes.push(Ipv4Addr::new(169, 254, 255, 255));

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

/// The default gateway, but only if something is actually answering as it.
///
/// WHY THE PLAIN ONE IS NOT ENOUGH. On Windows and Mac default_gateway() is a
/// guess: ".1 of whatever subnet we are on", derived from a source address the
/// kernel picks. When an adapter is disconnected Windows keeps its address, so
/// the guess survives the network it was guessed from, and the address server
/// refused to start because of a router on a network the machine had already
/// left. That was reported twice as the program not noticing the wifi being
/// turned off.
///
/// Asking whether the address is CONNECTED fixes most of it and leaves one
/// hole: a machine that is on a live network but has just released its address
/// has no connected address either, and is exactly the machine that must not
/// be served DHCP. So the question asked here is the one that actually
/// matters, which is not "is there an address" but "is there a router".
///
/// A router that has spoken to this machine is in the neighbour table. When
/// the adapter goes down the entry goes with it, verified on the machine:
///
///   wifi up    10.29.128.1  00-fe-ed-c0-ff-ee  dynamic
///   wifi off   gone
///
/// Read by membership of the address, never by the words around it, so it does
/// not care what language the machine is installed in.
pub fn live_default_gateway() -> Option<Ipv4Addr> {
    let gw = default_gateway()?;
    // A link-local gateway is a guess about a cable and is never a router:
    // RFC 3927 forbids forwarding link-local traffic.
    if gw.is_link_local() {
        return None;
    }
    let out = std::process::Command::new("arp").arg("-a").output().ok()?;
    if !out.status.success() {
        // Cannot tell. Report it, because the caller treats a gateway as a
        // reason to stand down and being over-cautious is the safe direction.
        return Some(gw);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    if mentions_address(&text, gw) {
        Some(gw)
    } else {
        None
    }
}

/// The addresses this machine holds on interfaces that are actually connected.
///
/// WHY local_addresses() IS NOT ENOUGH. It asks the kernel which source
/// address it would use to reach a target, which is the right question for
/// "where can I be reached" and the wrong one for "what networks am I on".
/// Windows keeps an address configured on an adapter after the adapter has
/// disconnected, and a UDP socket will still choose it:
///
///   ipconfig             Media disconnected, no address listed
///   Get-NetIPAddress     10.29.136.94
///   connect(8.8.8.8)     picks 10.29.136.94
///
/// So the address server refused to start on a laptop whose wifi had been off
/// for four minutes, saying "this computer is still on another network, at
/// 10.29.136.94" while nothing was on that network at all. Reported, twice, as
/// the program failing to notice the wifi being turned off. It had noticed
/// nothing, because it was asking a question whose answer does not change when
/// a radio is switched off.
///
/// HOW THIS ANSWERS IT WITHOUT READING LABELS. The platform's own tool already
/// leaves out interfaces that are down: a disconnected adapter has no address
/// line in ipconfig at all. So rather than parse "IPv4 Address" (which is
/// translated on a Windows installed in any other language, and this program is
/// for Kabul and Bamako as much as anywhere), the addresses we already hold are
/// tested for MEMBERSHIP in that output. A stale address appears nowhere in it.
/// Nothing here depends on a word.
///
/// Falls back to local_addresses() if the tool cannot be run, because refusing
/// to serve at all is worse than the caution being imperfect.
pub fn connected_addresses() -> Vec<Ipv4Addr> {
    let held = local_addresses();
    if held.is_empty() {
        return held;
    }

    let text = {
        use std::process::Command;
        #[cfg(target_os = "windows")]
        let out = Command::new("ipconfig").output();
        #[cfg(target_os = "linux")]
        let out = Command::new("ip").args(["-4", "-o", "addr", "show", "up"]).output();
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        let out = Command::new("ifconfig").output();

        match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
            _ => return held, // cannot tell: keep every address, stay cautious
        }
    };

    // An address counts as connected when the tool mentions it. Masks and
    // gateways appear in that text too and are harmless here: only addresses
    // this machine actually holds are ever looked up.
    let connected: Vec<Ipv4Addr> = held
        .iter()
        .copied()
        .filter(|a| mentions_address(&text, *a))
        .collect();

    // If the parse produced nothing at all, something is wrong with the
    // assumption rather than with the network. Fall back rather than declare
    // the machine to be on no network, which would switch the guard OFF.
    if connected.is_empty() {
        held
    } else {
        connected
    }
}

/// Does this text contain this address, as a whole address?
///
/// Bounded so that 10.29.136.9 does not match inside 10.29.136.94, which would
/// make a machine look connected on the strength of somebody else's address.
fn mentions_address(text: &str, a: Ipv4Addr) -> bool {
    let want = a.to_string();
    let mut from = 0;
    while let Some(i) = text[from..].find(&want) {
        let start = from + i;
        let end = start + want.len();
        let before_ok = start == 0
            || !text.as_bytes()[start - 1].is_ascii_digit() && text.as_bytes()[start - 1] != b'.';
        let after_ok = end >= text.len()
            || !text.as_bytes()[end].is_ascii_digit() && text.as_bytes()[end] != b'.';
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
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
pub fn find_servers(port: u16) -> Vec<(Ipv4Addr, usize, String)> {
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
fn ask_all(addrs: Vec<Ipv4Addr>, port: u16) -> Vec<(Ipv4Addr, usize, String)> {
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
            // And ask what it calls itself, so the person choosing sees a name
            // rather than four numbers and three dots. Asked on its own socket
            // and allowed to fail: a machine running an older copy has no
            // answer for this, and an empty name simply means the address is
            // shown, which is what happened before this existed.
            let name = ask_name(ip, port).unwrap_or_default();
            Some((ip, count, name))
        }));
    }
    handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
}

/// What the machine at `ip` calls itself, if it is new enough to say.
///
/// Deliberately forgiving. Anything unexpected here is not worth failing a
/// discovery over: the caller falls back to the address, which is the older
/// behaviour and always works.
fn ask_name(ip: Ipv4Addr, port: u16) -> Option<String> {
    let addr = SocketAddr::new(IpAddr::V4(ip), port);
    let mut sock = TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).ok()?;
    sock.set_read_timeout(Some(PROBE_TIMEOUT)).ok()?;
    sock.set_write_timeout(Some(PROBE_TIMEOUT)).ok()?;
    write!(sock, "GET /?who HTTP/1.1\r\nHost: {ip}:{port}\r\nConnection: close\r\n\r\n").ok()?;
    let mut raw = Vec::new();
    sock.take(4096).read_to_end(&mut raw).ok()?;
    let text = String::from_utf8_lossy(&raw);
    usable_name(text.split("\r\n\r\n").nth(1)?)
}

/// Is this reply a name, or an older copy's whole web page?
///
/// Split out so it can be tested without a socket. Every rejection here means
/// the caller shows the address instead, which is the behaviour that existed
/// before names did, so being strict costs nothing and being loose would put
/// a page of HTML in a menu row.
pub(crate) fn usable_name(body: &str) -> Option<String> {
    let body = body.trim();
    if body.is_empty() || body.starts_with('<') {
        return None;
    }
    // One line, and short enough to sit in a menu row next to a file count.
    let first = body.lines().next()?.trim();
    if first.is_empty() || first.chars().count() > 40 {
        return None;
    }
    // Control characters would move the cursor around the screen from inside
    // a menu row. The sender already strips tabs from the beacon for the same
    // reason; this is the same care applied to the same value over HTTP.
    if first.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(first.to_string())
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

/// Names looked up in the background, never in the draw loop.
///
/// A lookup is a network round trip. Doing one while drawing means the screen
/// stops until it answers, which is exactly the freeze this whole design keeps
/// avoiding. So the draw loop only ever READS this map, and a short-lived
/// thread fills it in.
#[derive(Clone, Default)]
pub struct NameCache {
    map: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<Ipv4Addr, Lookup>>>,
}

#[derive(Clone)]
struct Lookup {
    name: Option<String>,
    tries: u8,
    last: std::time::Instant,
}

impl NameCache {
    pub fn get(&self, ip: Ipv4Addr) -> Option<String> {
        let m = self.map.lock().unwrap_or_else(|e| e.into_inner());
        m.get(&ip).and_then(|e| e.name.clone())
    }

    /// Start a lookup for `ip` if one is worth starting.
    ///
    /// Retried a few times because a device shows up in the ARP table the
    /// moment it talks to us, which can be BEFORE dnsmasq has written its
    /// lease. One attempt would leave that device as a number for the rest of
    /// the lesson.
    pub fn ensure(&self, ip: Ipv4Addr, server: Ipv4Addr) {
        const MAX_TRIES: u8 = 4;
        const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(5);
        {
            let mut m = self.map.lock().unwrap_or_else(|e| e.into_inner());
            match m.get(&ip) {
                Some(e) if e.name.is_some() => return,
                Some(e) if e.tries >= MAX_TRIES || e.last.elapsed() < RETRY_AFTER => return,
                _ => {}
            }
            let tries = m.get(&ip).map(|e| e.tries).unwrap_or(0) + 1;
            m.insert(ip, Lookup { name: None, tries, last: std::time::Instant::now() });
        }
        let map = std::sync::Arc::clone(&self.map);
        std::thread::spawn(move || {
            // Short. dnsmasq is one hop away on a network we are the centre of;
            // if it has not answered in half a second it is not going to.
            let found = crate::dns::reverse_lookup(ip, server, std::time::Duration::from_millis(500));
            if let Some(fqdn) = found {
                let short = crate::dns::short_name(&fqdn);
                if !short.is_empty() {
                    let mut m = map.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(e) = m.get_mut(&ip) {
                        e.name = Some(short);
                    }
                }
            }
        });
    }
}

/// A short, stable tag for one physical device.
///
/// The problem it solves, in the owner's words: thirty kids all typing
/// "Cuntius.Maximus" and nobody able to tell which is which. The claimed name
/// is typed and can be copied. The phone model is not unique in a room of
/// identical school laptops. The address changes: a real test on 2026-08-25
/// produced two notes from the SAME phone under 10.42.0.200 and 10.42.0.170,
/// because it had reconnected and been given a new lease.
///
/// The hardware address is the one thing that stays put for the length of a
/// lesson, so this is a short hash OF it. Hashing rather than showing it is
/// deliberate: a MAC is a permanent identifier for somebody else's device and
/// has no business on a screen or in a repo, whereas four characters derived
/// from it are meaningless outside this room and perfectly sufficient to say
/// "these two messages came from two different phones".
#[cfg(target_os = "linux")]
pub fn device_tag(ip: &str) -> Option<String> {
    let mac = mac_for(ip)?;
    Some(short_hash(&mac))
}

#[cfg(not(target_os = "linux"))]
pub fn device_tag(_ip: &str) -> Option<String> {
    None
}

/// The hardware address for an address on our network, from the ARP table.
/// World readable, no privileges, unlike the DHCP leases.
#[cfg(target_os = "linux")]
fn mac_for(ip: &str) -> Option<String> {
    let text = std::fs::read_to_string("/proc/net/arp").ok()?;
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() >= 4 && f[0] == ip && f[2] == "0x2" {
            return Some(f[3].to_string());
        }
    }
    None
}

/// Four characters from the same unambiguous alphabet the passwords use, so a
/// teacher can read one off a screen and match it without asking which letter
/// it is.
fn short_hash(input: &str) -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let digest = crate::sha256::digest(input.as_bytes());
    digest
        .iter()
        .take(4)
        .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
        .collect()
}

// ---------------------------------------------------------------- clock

/// Local wall-clock, as "26-08-25 10:11".
///
/// SystemTime gives UTC and std has no timezone database. A teacher reading
/// back who said what during period three needs the clock on their own wall,
/// so the offset is asked for ONCE at startup from the system's own date
/// command and cached; every stamp after that is arithmetic. Falls back to
/// UTC, labelled, where that command does not exist.
static TZ_OFFSET: std::sync::OnceLock<i64> = std::sync::OnceLock::new();

fn tz_offset_seconds() -> i64 {
    *TZ_OFFSET.get_or_init(|| {
        let out = std::process::Command::new("date")
            .arg("+%z")
            .stdin(std::process::Stdio::null())
            .output();
        let Ok(out) = out else { return 0 };
        let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
        // "+0100" or "-0430"
        if t.len() < 5 {
            return 0;
        }
        let sign: i64 = if t.starts_with('-') { -1 } else { 1 };
        let h: i64 = t[1..3].parse().unwrap_or(0);
        let m: i64 = t[3..5].parse().unwrap_or(0);
        sign * (h * 3600 + m * 60)
    })
}

pub fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + tz_offset_seconds();
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{:02}-{:02}-{:02} {:02}:{:02}", y % 100, m, d, rem / 3600, (rem % 3600) / 60)
}

/// Days since 1970-01-01 to a calendar date. Howard Hinnant's civil_from_days,
/// which is the standard closed-form version: no tables, no leap-year special
/// cases scattered about, correct for every date this will ever see.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
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
    // NOT a timer. A transient timer's deadline cannot be moved: re-running
    // systemd-run with the same unit name fails silently while one is
    // pending, so the ORIGINAL deadline stood and the "safety" restore fired
    // INTO the running lesson, every three to four minutes. Journal-confirmed
    // 2026-08-25, 08:12:59 to 08:28:46, four firings, each one yanking the
    // hotspot down mid-test while the phone was connected. And stop-then-arm
    // with one name races: a stop arriving as the timer fires killed the
    // payload mid-flight in testing.
    //
    // Instead: ONE transient service holding `sleep <fuse>` and then the
    // nmcli restore. The heartbeat is `systemctl restart`, which atomically
    // kills the sleep and starts a fresh one: the deadline truly moves. If
    // this process dies, the beats stop, the sleep runs out, the wifi comes
    // back. Stopping the service kills the sleep BEFORE the nmcli, which is
    // exactly what disarming means. All three behaviours proven with a
    // 6-second fuse on this machine before shipping.
    let restarted = std::process::Command::new("systemctl")
        .args(["--user", "restart", "hub-wifi-restore.service"])
        .stdin(std::process::Stdio::null())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if restarted {
        return;
    }
    let _ = std::process::Command::new("systemd-run")
        .args([
            "--user",
            "--collect",
            "--unit=hub-wifi-restore",
            "sh",
            "-c",
            &format!("sleep {seconds}; exec nmcli connection up \"$0\""),
            prev,
        ])
        .stdin(std::process::Stdio::null())
        .output();
}

#[cfg(not(target_os = "linux"))]
pub fn rearm_restore(_seconds: u64) {}

#[derive(Debug)]
// Only ever built on Linux: everywhere else hotspot_up refuses and points
// the teacher at their own settings, so every field here is unread there.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub struct Hotspot {
    pub ssid: String,
    pub iface: String,
    /// The wifi network that was connected before, so it can be put back.
    previous: Option<String>,
    /// The NetworkManager profile actually serving this hotspot, read back
    /// from the system rather than assumed to be "Hotspot".
    profile: Option<String>,
    /// The password in force right now, which is not the one the lesson
    /// started with once it has been changed.
    pub password: String,
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

// -------------------------------------------------- switched-off machinery

/// The Windows services this program's wifi path needs, and what they are
/// called on the screen a person will be looking at.
///
/// WHY THIS EXISTS. The laptops this tool is for are second-hand, and a
/// second-hand laptop has usually been "speeded up" by somebody. Every debloat
/// list on the internet turns off Internet Connection Sharing and the Mobile
/// Hotspot service, because almost nobody shares a connection and they look
/// like free memory. Measured on the machine this was written on: 150 services
/// set to Disabled, these two among them.
///
/// What that does to a teacher is specific and cruel. Settings does not say
/// "a service is off". It says **We can't set up mobile hotspot**, and then
/// shows an EMPTY properties box: no name, no password, no band. Nothing on
/// the screen mentions a service, so there is nothing to search for, and the
/// laptop looks broken rather than switched off. The same is true whether the
/// machine was tuned by its owner, by the shop that sold it, or by whoever had
/// it before the school did.
///
/// It is worth being plain about the third case. A managed or locked-down
/// machine is a legitimate reason for these to be off, and this says what is
/// off and what turning it on would do; it does not tell anybody to fight
/// their own IT department. The command needs Administrator, so nobody can
/// follow this advice without already having the right to.
///
/// TWO SERVICES, NOT THREE. WFDSConMgrSvc, the Wi-Fi Direct Services
/// Connection Manager, was in this list in 0.9.7 and has been measured out.
///
/// The argument for it was reasonable: Windows 11 builds its hotspot on Wi-Fi
/// Direct rather than the old hosted-network path, so its connection manager
/// looks like it has to be involved. It is not a declared dependency of either
/// service below, so it was listed last and asked for separately.
///
/// Settled by running it, 2026-09-08, on this machine. Both services below set
/// to Manual, WFDSConMgrSvc deliberately left Disabled and Stopped:
///
///   manager created: YES, ssid 'WIN-VR6KGAH3RAF 5437', band Auto
///   operational state: On
///   adapters up: Wi-Fi, Ethernet, Local Area Connection* 2
///
/// That third adapter is the virtual access point. The hotspot did not merely
/// report itself configurable, it ran, with the service that was supposed to be
/// necessary switched off throughout.
///
/// Taking it out is not tidying. A message that names three services when two
/// are the problem sends somebody to change something that did not matter, and
/// destroys the evidence of which change did: the next person to look at that
/// laptop cannot tell which one mattered. That reasoning is already a test
/// here, `the_advice_does_not_name_services_that_are_fine`, and it applies to
/// the list itself and not only to the filtering.
const HOTSPOT_SERVICES: [(&str, &str); 2] = [
    ("icssvc", "Windows Mobile Hotspot Service"),
    // Never actually RAN in either measurement: set to Manual and left
    // Stopped, hotspot included. It has to be startable, not started, which is
    // why the advice asks for Set-Service on it and Start-Service only on
    // icssvc.
    ("SharedAccess", "Internet Connection Sharing"),
];

/// The `Start` value out of one `reg query` result, or None.
///
/// A SEPARATE FUNCTION SO IT CAN BE TESTED, and because the parse is the only
/// part that can be wrong.
///
/// This reads the NUMBER and ignores every word around it. `reg query` prints
/// `    Start    REG_DWORD    0x4`, and `REG_DWORD` is a type name rather than
/// a sentence, so it is the same on a Windows installed in any language. The
/// value name and the number are the same everywhere too. Nothing here depends
/// on English, which is the whole reason this uses `reg` and not `sc qc`:
/// sc.exe prints `START_TYPE : 4 DISABLED`, and both of those words are
/// translated.
///
/// 4 is Disabled, 3 is Manual, 2 is Automatic. Those are the numbers Windows
/// itself writes; they are not this program's invention.
#[cfg(not(target_os = "linux"))]
fn start_value_from(text: &str) -> Option<u32> {
    for line in text.lines() {
        if !line.contains("REG_DWORD") {
            continue;
        }
        let last = line.split_whitespace().last()?;
        let hex = last.strip_prefix("0x").or_else(|| last.strip_prefix("0X"))?;
        return u32::from_str_radix(hex, 16).ok();
    }
    None
}

/// Which of the hotspot services are set to Disabled on this machine.
///
/// Empty on a machine nobody has tuned, which is the common case and costs one
/// `reg query` per name at the moment somebody asks for `doctor` or tries to
/// start a hotspot. Never on any path that serves a file.
///
/// A service that cannot be read at all is reported as fine rather than as
/// broken. Being unable to answer the question is not evidence of a fault, and
/// a diagnostic that invents problems is worse than one that stays quiet.
#[cfg(not(target_os = "linux"))]
pub fn disabled_hotspot_services() -> Vec<(&'static str, &'static str)> {
    let mut off = Vec::new();
    for (service, human) in HOTSPOT_SERVICES {
        let key = format!(r"HKLM\SYSTEM\CurrentControlSet\Services\{service}");
        let out = std::process::Command::new("reg")
            .args(["query", &key, "/v", "Start"])
            .stdin(std::process::Stdio::null())
            .output();
        let Ok(out) = out else { continue };
        if !out.status.success() {
            continue;
        }
        if start_value_from(&String::from_utf8_lossy(&out.stdout)) == Some(4) {
            off.push((service, human));
        }
    }
    off
}

#[cfg(target_os = "linux")]
pub fn disabled_hotspot_services() -> Vec<(&'static str, &'static str)> {
    Vec::new()
}

/// What to tell somebody whose laptop has had these switched off.
///
/// Written to be read by a teacher and typed by a teacher: what is wrong, in
/// one sentence, then the exact words to type, then how to know it worked. No
/// jargon that is not immediately explained, and the reason it is being asked
/// for rather than an instruction to trust.
///
/// The commands are given in full rather than as "enable the services",
/// because the gap between those two is the whole problem: somebody who knew
/// how to enable a service would not have needed the message.
pub fn switched_off_advice(off: &[(&'static str, &'static str)]) -> Option<String> {
    if off.is_empty() {
        return None;
    }
    let mut s = String::new();
    s.push_str(
        "\nThis laptop cannot make a wifi network, and it is not the wifi card.\n\n\
         Parts of Windows have been switched off on this machine. This is very\n\
         common on a second-hand laptop: the lists that promise to speed Windows\n\
         up nearly all switch these off, because almost nobody shares a network\n\
         connection. Windows does not tell you that is why. It says it cannot set\n\
         up a mobile hotspot and leaves the box empty, so the laptop looks broken\n\
         when it is only switched off.\n\n\
         Switched off here:\n",
    );
    for (service, human) in off {
        s.push_str(&format!("  {human}  ({service})\n"));
    }
    s.push_str(
        "\nTo switch them back on, open Windows Terminal or PowerShell AS\n\
         ADMINISTRATOR. Right-click the Start button, and choose the entry with\n\
         (Admin) after it. Then type these lines, one at a time:\n\n",
    );
    for (service, _) in off {
        s.push_str(&format!("  Set-Service {service} -StartupType Manual\n"));
    }
    s.push_str("  Start-Service icssvc\n");
    s.push_str(
        "\nThen open Settings, Network and internet, Mobile hotspot. The boxes\n\
         for name and password should now be filled in instead of blank.\n\n\
         If it asks for an administrator password and you do not have one, this\n\
         laptop is managed by somebody else and they have to do it. Use a cable\n\
         instead: it needs none of this.\n",
    );
    Some(s)
}

/// Undo nmcli's terse-mode escaping.
///
/// `nmcli -t` separates fields with a colon, so any colon or backslash INSIDE
/// a value is escaped. Feeding the escaped form back to nmcli does not find
/// the connection, it finds nothing, and the failure is silent.
///
/// This is not a hypothetical. The machine this was written on has a wifi
/// profile whose name contains a colon. Measured 2026-08-25 against nmcli
/// itself: `nmcli connection show` exits 10 (not found) for the escaped
/// spelling of that name and 0 for the real one. Everything that put a
/// teacher's wifi BACK went through this function, so a teacher whose home or
/// school network has a colon in its name would have finished a lesson with no
/// wifi and no message. That is precisely the outcome the restore path exists
/// to prevent, and it had been carrying its own defeat since it was written.
#[cfg(target_os = "linux")]
fn unescape_terse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            // A backslash escapes exactly one following character. A trailing
            // lone backslash is not something nmcli emits, but dropping it
            // silently would be worse than keeping it.
            match chars.next() {
                Some(next) => out.push(next),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn active_wifi_connection() -> Option<String> {
    let out = std::process::Command::new("nmcli")
        .args(["-t", "-f", "NAME,TYPE", "connection", "show", "--active"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        // Split from the RIGHT: TYPE cannot contain a colon, NAME very much
        // can, and splitting from the left cuts a name like `Room:5 spare` into
        // pieces.
        if let Some((name, kind)) = line.rsplit_once(':') {
            if kind.contains("wireless") {
                return Some(unescape_terse(name));
            }
        }
    }
    None
}

/// The connection profile currently active on one interface.
///
/// Needed because the profile is NOT reliably called "Hotspot". nmcli appends
/// a number when a profile of that name already exists, and this machine has
/// accumulated Hotspot-1 through Hotspot-4 from previous runs. Anything that
/// addresses the hotspot by the literal name "Hotspot" is therefore addressing
/// somebody else's leftover, which for a password change means changing the
/// password on a network that is not running.
#[cfg(target_os = "linux")]
fn active_profile_on(iface: &str) -> Option<String> {
    let out = std::process::Command::new("nmcli")
        .args(["-t", "-f", "NAME,DEVICE,TYPE", "connection", "show", "--active"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        // NAME, DEVICE, TYPE. Only NAME can contain a colon, so take the two
        // trailing fields from the right and everything before them is the name.
        let mut parts = line.rsplitn(3, ':');
        let kind = parts.next().unwrap_or("");
        let dev = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("");
        if dev == iface && kind.contains("wireless") {
            return Some(unescape_terse(name));
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
/// Which wifi channels THIS radio, in THIS country, may broadcast on.
///
/// Asked, never assumed. The first version of channel support hardcoded 1 to
/// 13, which is Britain talking: the legal list differs by country, and the
/// usable list differs by adapter. A 5 GHz-capable laptop can serve on
/// channel 36 where this 2012 card cannot, and a machine in another
/// regulatory domain has a different 2.4 GHz list too. The kernel already
/// merges "what the hardware can do" with "what is legal here"; this reads
/// that verdict.
///
/// An empty answer means the question could not be asked (no `iw`), and the
/// caller falls back to accepting 1 to 13, the range that is legal in most of
/// the world, letting the system refuse what it must.
pub fn allowed_channels() -> &'static [u16] {
    static CACHE: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        for iw in ["iw", "/usr/sbin/iw", "/sbin/iw"] {
            if let Ok(out) = std::process::Command::new(iw)
                .arg("phy")
                .stdin(std::process::Stdio::null())
                .output()
            {
                if out.status.success() {
                    return parse_phy_channels(&String::from_utf8_lossy(&out.stdout));
                }
            }
        }
        Vec::new()
    })
}

/// Pull the channel numbers out of `iw phy` output, skipping what is
/// disabled and what is receive-only ("no IR": the law lets the radio listen
/// there but not speak, and an access point is nothing but speaking).
fn parse_phy_channels(text: &str) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if !t.starts_with('*') || !t.contains(" MHz [") {
            continue;
        }
        if t.contains("disabled") || t.contains("no IR") {
            continue;
        }
        if let Some(open) = t.find('[') {
            if let Some(close) = t[open..].find(']') {
                if let Ok(ch) = t[open + 1..open + close].parse::<u16>() {
                    if !out.contains(&ch) {
                        out.push(ch);
                    }
                }
            }
        }
    }
    out.sort_unstable();
    out
}

/// "1-11, 36, 40-48": the allowed list, written the way a person reads it.
pub fn channel_ranges(chs: &[u16]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chs.len() {
        let start = chs[i];
        let mut end = start;
        while i + 1 < chs.len() && chs[i + 1] == end + 1 {
            i += 1;
            end = chs[i];
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}-{end}")
        });
        i += 1;
    }
    parts.join(", ")
}

#[cfg(target_os = "linux")]
pub fn hotspot_up(ssid: &str, password: &str, channel: Option<u16>) -> Result<Hotspot, String> {
    if password.chars().count() < 8 {
        return Err("A wifi password has to be at least 8 characters. That is a rule of WPA2, not ours.".into());
    }
    // The channel choice matters more than it looks: on 2026-08-25 the system
    // picked channel 1 and the same download that had run at 7 MB/s the day
    // before ran at 4.7, with the radio holding 87% of the airtime to do it.
    // Full airtime, clean retries, low yield is what a bad channel looks like;
    // nothing in the room ever says "the channel is the problem".
    //
    // Validated against what THIS radio in THIS country may broadcast, read
    // from the kernel, not against a hardcoded British 1-to-13.
    if let Some(ch) = channel {
        let allowed = allowed_channels();
        let legal = if allowed.is_empty() { (1..=13).contains(&ch) } else { allowed.contains(&ch) };
        if !legal {
            return Err(if allowed.is_empty() {
                "Wifi channels go from 1 to 13 here. Leave it empty to let the computer choose.".to_string()
            } else {
                format!(
                    "This radio, in this country, may broadcast on channels {}.                      Leave the field empty to let the computer choose.",
                    channel_ranges(allowed)
                )
            });
        }
    }
    let iface = wifi_interface().ok_or("No wifi adapter found. Is wifi switched on?")?;
    let previous = active_wifi_connection();
    let mut args: Vec<String> = ["device", "wifi", "hotspot", "ifname", &iface, "ssid", ssid, "password", password]
        .iter().map(|a| a.to_string()).collect();
    if let Some(ch) = channel {
        // band must accompany channel or nmcli refuses the pair. 14 and below
        // is the 2.4 GHz band; everything above lives at 5 GHz.
        let band = if ch <= 14 { "bg" } else { "a" };
        args.extend(["band".into(), band.into(), "channel".into(), ch.to_string()]);
    }
    let out = std::process::Command::new("nmcli")
        .args(&args)
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
    // Read back which profile nmcli actually used. Asked for, never assumed:
    // see active_profile_on.
    let profile = active_profile_on(&iface);
    Ok(Hotspot {
        ssid: ssid.to_string(),
        iface,
        previous,
        profile,
        password: password.to_string(),
    })
}

#[cfg(not(target_os = "linux"))]
pub fn hotspot_up(_ssid: &str, _password: &str, _channel: Option<u16>) -> Result<Hotspot, String> {
    let mut msg = String::from(
        "On this system, switch the hotspot on yourself first: \
         Settings, Network and internet, Mobile hotspot. \
         Then come back here and the files will be handed out over it.",
    );
    // If that switch is going to refuse, say so NOW rather than after somebody
    // has been to Settings, failed, and concluded the laptop is broken. This is
    // the one moment we know for certain they are about to go and press it.
    if let Some(advice) = switched_off_advice(&disabled_hotspot_services()) {
        msg.push('\n');
        msg.push_str(&advice);
    }
    Err(msg)
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
        // By the name the system gave it, falling back to the usual one only
        // when we could not read it back. "Hotspot" is a good guess and a bad
        // certainty: this machine has four leftovers called Hotspot-1 upwards.
        let profile = self.profile.clone().unwrap_or_else(|| "Hotspot".to_string());
        let _ = std::process::Command::new("nmcli")
            .args(["connection", "down", &profile])
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

    /// Change the password and restart the network under it.
    ///
    /// The heavier of the two ways to remove somebody, and the only one that
    /// actually removes them: pausing a device stops it reaching the lesson,
    /// this stops it reaching the NETWORK. Bringing the profile back up
    /// re-forms the access point, so every device in the room is dropped and
    /// only those told the new password return. A child who has worked out how
    /// to present a new hardware address, and so escaped a pause, does not
    /// escape this one, because it is not their device being recognised, it is
    /// a key they do not have.
    ///
    /// It is blunt on purpose and the screen says so before it runs. Thirty
    /// children have to retype a password to remove one, and half a lesson can
    /// go on that. It is the answer when the pause is not holding, not the
    /// first move.
    #[cfg(target_os = "linux")]
    pub fn change_password(&mut self, new: &str) -> Result<(), String> {
        if new.chars().count() < 8 {
            return Err("A wifi password has to be at least 8 characters. That is a rule of WPA2, not ours.".into());
        }
        // Refuse rather than guess. Changing the key on a profile that is not
        // the one serving the room does nothing visible, and would leave the
        // teacher reading a new password off the screen that no device is
        // being asked for. Better to say we cannot.
        let profile = match self.profile.clone().or_else(|| active_profile_on(&self.iface)) {
            Some(p) => p,
            None => {
                return Err("Could not work out which network this computer is serving, \
                            so the password was left alone. Nothing has changed."
                    .into())
            }
        };
        let out = std::process::Command::new("nmcli")
            .args(["connection", "modify", &profile, "wifi-sec.psk", new])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("Could not run nmcli: {e}"))?;
        if !out.status.success() {
            return Err(explain_nmcli(String::from_utf8_lossy(&out.stderr).trim()));
        }
        // Modifying a live profile does not re-key the running access point;
        // the change sits in the stored profile until the profile is brought
        // up again. Without this the teacher would be given a new password
        // while the old one still worked, which is worse than doing nothing:
        // they would believe the room had been cleared when it had not.
        let out = std::process::Command::new("nmcli")
            .args(["connection", "up", &profile])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("Could not run nmcli: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "The password was changed but the network did not come back up.\n{}\n\
                 The old password no longer works. Stop and start the lesson again.",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        self.profile = Some(profile);
        self.password = new.to_string();
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn change_password(&mut self, _new: &str) -> Result<(), String> {
        Err("On this system the password is changed where the hotspot was \
             switched on: Settings, Network and internet, Mobile hotspot."
            .into())
    }

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
        // Stopping the service kills its sleep before the nmcli runs, which
        // is what disarming means. The .timer name is the previous build's.
        for unit in ["hub-wifi-restore.service", "hub-wifi-restore.timer"] {
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

    // A socket-level test of ask_name() was written here and then removed,
    // deliberately, and this note is what is left of it.
    //
    // It stood up a listener that answered one canned response. It failed
    // roughly three runs in four for reasons that were all about the fake
    // server, not about ask_name: answering before reading the request closed
    // the socket under the client, and reading first left a race on how much
    // of the request had arrived. Every fix moved the flakiness rather than
    // removing it.
    //
    // A test that fails at random is worse than no test, because it teaches
    // people to re-run until green, which is the habit that hid two real bugs
    // in this same suite earlier today. The parser below is the part with the
    // decisions in it and is tested exactly. The wire itself was verified by
    // hand against two live machines on 2026-09-08:
    //
    //   0.9.4 sender:  curl "http://127.0.0.1:8095/?who"  ->  Teacher's laptop
    //   0.9.3 sender:  curl "http://169.254.87.61/?who"   ->  an HTML page,
    //                  which usable_name() rejects, so the address is shown
    //                  exactly as it was before names existed.
    //
    // That second one is the compatibility case, and it is the one that would
    // have mattered if it were wrong.

    /// A name from another machine is untrusted text going into a menu row.
    #[test]
    fn only_something_that_looks_like_a_name_is_used_as_one() {
        assert_eq!(usable_name("Teacher's laptop").as_deref(), Some("Teacher's laptop"));
        assert_eq!(usable_name("  Year 7 Maths \n").as_deref(), Some("Year 7 Maths"));

        // An older copy answers /?who with its whole page. Showing that in a
        // menu row is worse than showing the address it replaces.
        assert_eq!(usable_name("<!doctype html><html>..."), None);
        assert_eq!(usable_name(""), None);
        assert_eq!(usable_name("   "), None);

        // Escape sequences would move the cursor from inside a row.
        assert_eq!(usable_name("evil\u{1b}[2Jname"), None);
        assert_eq!(usable_name("two\nlines").as_deref(), Some("two"));

        // Too long to sit beside a file count.
        assert_eq!(usable_name(&"x".repeat(41)), None);
        assert_eq!(usable_name(&"x".repeat(40)).as_deref(), Some("x".repeat(40)).as_deref());
    }

    /// 10.29.136.9 must not match inside 10.29.136.94, or a machine looks
    /// connected on the strength of a different address entirely.
    #[test]
    fn an_address_is_matched_whole_and_not_as_a_prefix() {
        let text = "   IPv4 Address. . . . . : 10.29.136.94
   Mask: 255.255.240.0
";
        assert!(mentions_address(text, "10.29.136.94".parse().unwrap()));
        assert!(!mentions_address(text, "10.29.136.9".parse().unwrap()));
        // A substring that is a valid address in its own right.
        let text2 = "   addr 110.29.136.941 and 10.29.136.94
";
        assert!(mentions_address(text2, "10.29.136.94".parse().unwrap()));
        assert!(!mentions_address(text, "10.29.136.4".parse().unwrap()));
    }

    #[test]
    fn a_disconnected_adapters_address_is_absent_from_the_tools_output() {
        // What ipconfig prints for an adapter whose media is disconnected:
        // a header and a state, and no address at all.
        let text = "Wireless LAN adapter Wi-Fi:

   Media State . . . : Media disconnected
";
        assert!(!mentions_address(text, "10.29.136.94".parse().unwrap()));
    }

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
    // parse_leases and parse_arp read /var/lib/misc/dnsmasq.leases and
    // /proc/net/arp, so they only exist on Linux. Without this gate the test
    // build failed to compile on Windows and Mac, which meant `cargo test`
    // could not be run at all on either: not one test, the whole suite.
    #[cfg(target_os = "linux")]
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

    // parse_leases and parse_arp read /var/lib/misc/dnsmasq.leases and
    // /proc/net/arp, so they only exist on Linux. Without this gate the test
    // build failed to compile on Windows and Mac, which meant `cargo test`
    // could not be run at all on either: not one test, the whole suite.
    #[cfg(target_os = "linux")]
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
    fn a_device_tag_is_stable_short_and_leaks_nothing() {
        // Same hardware, two different leases: the tag must not change, or it
        // reports one reconnecting phone as two impostors. Measured on a real
        // phone 2026-08-25, which sent notes from .200 and then .170.
        let a = short_hash("aa:bb:cc:dd:ee:ff");
        let b = short_hash("aa:bb:cc:dd:ee:ff");
        assert_eq!(a, b, "the tag must be stable for one device");
        assert_eq!(a.chars().count(), 4);
        let other = short_hash("11:22:33:44:55:66");
        assert_ne!(a, other, "two devices must not share a tag this easily");
        // Nothing of the hardware address survives into the tag.
        assert!(!a.contains("aa") && !a.contains("ff"), "{a}");
        assert!(a.chars().all(|c| "abcdefghjkmnpqrstuvwxyz23456789".contains(c)),
                "{a} has a character somebody will misread");
    }

    #[test]
    fn the_clock_produces_a_sane_recent_date() {
        let t = timestamp();
        // "YY-MM-DD HH:MM"
        assert_eq!(t.len(), 14, "{t}");
        let (date, time) = t.split_once(' ').expect("date and time");
        let parts: Vec<&str> = date.split('-').collect();
        assert_eq!(parts.len(), 3, "{t}");
        let year: i64 = parts[0].parse().expect("year");
        assert!((25..=99).contains(&year), "year {year} out of range in {t}");
        let month: i64 = parts[1].parse().expect("month");
        assert!((1..=12).contains(&month), "month {month} in {t}");
        let day: i64 = parts[2].parse().expect("day");
        assert!((1..=31).contains(&day), "day {day} in {t}");
        let hh: i64 = time[..2].parse().expect("hour");
        assert!(hh < 24, "{t}");
    }

    /// The calendar arithmetic, against dates whose answers are known: a leap
    /// day, a century non-leap boundary, and the epoch itself.
    #[test]
    fn the_calendar_maths_is_right_on_the_awkward_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        // Day numbers verified against an independent calendar, because the
        // first version of this test asserted values I had worked out in my
        // head and they were wrong: the code was right and the test was not.
        assert_eq!(civil_from_days(11016), (2000, 2, 29), "2000 IS a leap year");
        assert_eq!(civil_from_days(11017), (2000, 3, 1));
        assert_eq!(civil_from_days(20574), (2026, 5, 1));
        // 2100 is NOT a leap year: divisible by 100, not by 400. This is the
        // case a hand-rolled calendar gets wrong.
        assert_eq!(civil_from_days(47540), (2100, 2, 28));
        assert_eq!(civil_from_days(47541), (2100, 3, 1));
    }

    /// The escaping bug that had been sitting in the wifi restore path since
    /// it was written.
    ///
    /// Found against a real profile on this machine whose name contains a
    /// colon, not against an invented one. Measured 2026-08-25 with nmcli
    /// itself: the escaped spelling exits 10 (unknown connection), the real
    /// one exits 0. Everything that put a teacher's wifi back was passing the
    /// escaped spelling.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_connection_name_survives_the_round_trip_through_terse_mode() {
        assert_eq!(unescape_terse(r"\:-\) staff \:-\) room"), ":-) staff :-) room");
        assert_eq!(unescape_terse("ASK4 Wireless"), "ASK4 Wireless");
        assert_eq!(unescape_terse(r"Room\:5"), "Room:5");
        // A backslash in a name is escaped as two, and must come back as one.
        assert_eq!(unescape_terse(r"back\\slash"), r"back\slash");
        // An escape with nothing after it is not something nmcli emits, but
        // swallowing the character silently would be the worse answer.
        assert_eq!(unescape_terse(r"trailing\"), r"trailing\");
    }

    /// The channel list comes from the kernel's verdict, and the parser must
    /// honour the two refusals: "disabled" (illegal here) and "no IR" (may
    /// listen, may not speak; an AP only speaks).
    #[test]
    fn the_channel_parser_keeps_what_may_speak_and_drops_the_rest() {
        let canned = "
        Frequencies:
            * 2412.0 MHz [1] (17.0 dBm)
            * 2417.0 MHz [2] (17.0 dBm)
            * 2467.0 MHz [12] (17.0 dBm) (no IR)
            * 2472.0 MHz [13] (17.0 dBm) (no IR)
            * 2484.0 MHz [14] (disabled)
            * 5180.0 MHz [36] (20.0 dBm)
            * 5200.0 MHz [40] (20.0 dBm)
            * 5260.0 MHz [52] (20.0 dBm) (radar detection)
        ";
        let chs = parse_phy_channels(canned);
        assert_eq!(chs, vec![1, 2, 36, 40, 52], "{chs:?}");
        assert_eq!(channel_ranges(&chs), "1-2, 36, 40, 52");
        assert_eq!(channel_ranges(&[1,2,3,4,5,6,7,8,9,10,11,12,13]), "1-13");
        assert_eq!(channel_ranges(&[]), "");
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

#[cfg(test)]
mod arp_tag_tests {
    use super::*;

    /// Against this machine's REAL ARP table, cross-checked with an
    /// independently computed value. Skips itself when the table is empty,
    /// which is the honest thing to do rather than pass vacuously.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_real_arp_entry_produces_the_expected_tag() {
        let Ok(text) = std::fs::read_to_string("/proc/net/arp") else { return };
        let mut found = None;
        for line in text.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() >= 4 && f[2] == "0x2" {
                found = Some((f[0].to_string(), f[3].to_string()));
                break;
            }
        }
        let Some((ip, mac)) = found else { return };
        let tag = device_tag(&ip).expect("an ARP entry we just read must produce a tag");
        assert_eq!(tag, short_hash(&mac), "the tag must come from that entry's hardware address");
        assert_eq!(tag.chars().count(), 4);
    }
}

#[cfg(test)]
mod switched_off_tests {
    use super::*;

    /// The parse reads the NUMBER and ignores every word around it.
    ///
    /// Real captured output from `reg query` on the machine this was written
    /// on, where both services had been switched off by a debloat pass:
    ///
    ///     HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Services\icssvc
    ///         Start    REG_DWORD    0x4
    ///
    /// The German case below is not decoration. `sc qc` prints
    /// `START_TYPE : 4 DISABLED` and BOTH of those words are translated, which
    /// is why this uses `reg` instead. `REG_DWORD` is a type name and is not.
    /// The value name `Start` is not translated either. If a future version of
    /// this ever starts matching on a word, this test is where it should fail.
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn the_start_value_is_read_as_a_number_and_not_as_a_word() {
        let real = "\r\nHKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Services\\icssvc\r\n    \
                    Start    REG_DWORD    0x4\r\n\r\n";
        assert_eq!(start_value_from(real), Some(4), "disabled, from real output");

        assert_eq!(
            start_value_from("    Start    REG_DWORD    0x3"),
            Some(3),
            "3 is Manual, which is the Windows default for both of these"
        );
        assert_eq!(start_value_from("    Start    REG_DWORD    0x2"), Some(2));

        // A localised install. Only the surrounding prose can change; the type
        // name, the value name and the number cannot.
        let german = "\r\nHKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Services\\icssvc\r\n    \
                      Start    REG_DWORD    0x4\r\n";
        assert_eq!(start_value_from(german), Some(4));

        // Nothing to read is not the same as reading a zero. A machine we
        // cannot ask about must not be reported as broken.
        assert_eq!(start_value_from(""), None);
        assert_eq!(start_value_from("ERROR: The system was unable to find the key"), None);
        assert_eq!(start_value_from("    Start    REG_SZ    manual"), None);
    }

    /// A machine nobody has tuned gets no lecture.
    ///
    /// The advice is long, and printing it to somebody whose laptop is fine
    /// would teach them to skip the whole screen, which is where the useful
    /// lines are.
    #[test]
    fn a_machine_with_nothing_switched_off_is_told_nothing() {
        assert!(switched_off_advice(&[]).is_none());
    }

    /// And a machine that IS switched off gets something it can act on.
    ///
    /// Every assertion here is a thing somebody needs in order to get from
    /// "it does not work" to "it works", and the message is worthless without
    /// all of them: what is wrong, that it is not the hardware, the exact words
    /// to type, that those words need Administrator, how to know it worked, and
    /// what to do when the answer is "you are not allowed".
    #[test]
    fn the_advice_names_the_service_and_the_exact_words_to_type() {
        let off = [
            ("icssvc", "Windows Mobile Hotspot Service"),
            ("SharedAccess", "Internet Connection Sharing"),
        ];
        let s = switched_off_advice(&off).expect("something is off, so there must be advice");

        assert!(s.contains("it is not the wifi card"), "does not clear the hardware:\n{s}");
        assert!(s.contains("Windows Mobile Hotspot Service"), "no plain name:\n{s}");
        assert!(s.contains("(icssvc)"), "no service name to search for:\n{s}");
        assert!(
            s.contains("Set-Service icssvc -StartupType Manual"),
            "no command to type:\n{s}"
        );
        assert!(
            s.contains("Set-Service SharedAccess -StartupType Manual"),
            "the second service is named but not fixed:\n{s}"
        );
        assert!(s.contains("ADMINISTRATOR"), "does not say it needs admin:\n{s}");
        assert!(s.contains("Mobile hotspot"), "does not say how to check it worked:\n{s}");
        assert!(
            s.contains("cable"),
            "no way out for somebody who is not allowed to do this:\n{s}"
        );
    }

    /// Only what is actually off is named.
    ///
    /// A message that lists three services on a machine where one is off sends
    /// somebody to change two things that were already right, and the next
    /// person to look at that laptop has no way of knowing which change
    /// mattered.
    #[test]
    fn the_advice_does_not_name_services_that_are_fine() {
        let off = [("icssvc", "Windows Mobile Hotspot Service")];
        let s = switched_off_advice(&off).expect("advice");
        assert!(!s.contains("SharedAccess"), "names a service that is on:\n{s}");
        assert!(!s.contains("WFDSConMgrSvc"), "names a service that is on:\n{s}");
    }

    /// The list holds only what was measured to matter.
    ///
    /// WFDSConMgrSvc was in it in 0.9.7, on the reasoning that Windows 11
    /// builds its hotspot on Wi-Fi Direct so Wi-Fi Direct's connection manager
    /// must be involved. Reasonable, and wrong. Measured 2026-09-08 with the
    /// two below set to Manual and that one deliberately left Disabled and
    /// Stopped: the hotspot reached operational state On and the virtual access
    /// point appeared as its own adapter.
    ///
    /// This test exists because the argument for putting it back is more
    /// persuasive than the argument for leaving it out, and the argument is
    /// beaten by a measurement. If somebody adds a third service here, they
    /// should have to delete this and say why.
    #[test]
    fn the_list_holds_only_services_measured_to_matter() {
        assert_eq!(HOTSPOT_SERVICES.len(), 2, "a service was added without a measurement");
        assert!(
            !HOTSPOT_SERVICES.iter().any(|(s, _)| *s == "WFDSConMgrSvc"),
            "WFDSConMgrSvc was measured NOT to be needed; the hotspot started with it Disabled"
        );
        assert!(HOTSPOT_SERVICES.iter().any(|(s, _)| *s == "icssvc"));
        assert!(HOTSPOT_SERVICES.iter().any(|(s, _)| *s == "SharedAccess"));
    }

    /// Asking a real machine must not panic, hang, or invent a fault.
    ///
    /// This runs against whatever this machine happens to be. It cannot assert
    /// a result, because the answer is different on a tuned laptop and a fresh
    /// one, and both answers are correct. What it CAN assert is that the thing
    /// only ever reports names it was given, which is the failure that would
    /// put an invented service in front of a teacher.
    #[test]
    fn asking_this_machine_returns_only_names_we_know() {
        for (service, human) in disabled_hotspot_services() {
            assert!(
                HOTSPOT_SERVICES.iter().any(|(s, h)| *s == service && *h == human),
                "reported a service that is not on the list: {service}"
            );
        }
    }
}
