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
// Replaced on every lesson, not set once: a OnceLock kept the wifi from before
// the FIRST lesson, so a later lesson started from a different network was
// restored to the wrong one.
static PREVIOUS_WIFI: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// How long the restore timer waits, and how often it is pushed back.
///
/// The fuse must be comfortably longer than the heartbeat, or a slow tick
/// restores the wifi in the middle of a lesson. Defined once: the command line
/// and the screen both start the same heartbeat, and two copies of these
/// numbers is how one of them ends up shorter than the other.
pub const RESTORE_FUSE: u64 = 180;
pub const HEARTBEAT: u64 = 60;

/// Whether a hotspot is up and its restore should be kept pushed back.
///
/// A lock rather than a flag, so the heartbeat cannot read "armed", lose the
/// processor to a clean Stop that disarms, and then re-arm straight after it.
///
/// WHY. The heartbeat never stopped. After a clean Stop it re-armed the
/// restore within a minute, and one thread was added per lesson. Close the
/// program later and three minutes on the laptop rejoined the wifi it had
/// before the FIRST lesson, pulling down any hotspot running at that moment.
/// Found reading the code, 2026-09-23.
static RESTORE_ARMED: std::sync::Mutex<bool> = std::sync::Mutex::new(false);

/// Keep pushing the restore back for as long as a hotspot is up. One thread
/// for the life of the program, however many lessons are started.
pub fn start_heartbeat() {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_secs(HEARTBEAT));
        let armed = RESTORE_ARMED.lock().unwrap_or_else(|e| e.into_inner());
        if *armed {
            rearm_restore(RESTORE_FUSE);
        }
    });
}

/// Push the restore back out to `seconds` from now.
///
/// Called on a heartbeat while a lesson is running. If the tool stops for any
/// reason at all, including being killed outright, the timer is the thing that
/// still exists and the wifi comes back on its own.
#[cfg(target_os = "linux")]
pub fn rearm_restore(seconds: u64) {
    let Some(prev) = PREVIOUS_WIFI.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
    let prev = &prev;
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
pub(crate) fn start_value_from(text: &str) -> Option<u32> {
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
        "\nThe hub can switch them back on for you. On its first screen choose\n\
         Check this computer, then Turn on the parts of Windows the hub needs.\n\
         Windows asks for permission once. Or type:  hub services --fix\n",
    );
    s.push_str(
        "\nOr by hand: open Windows Terminal or PowerShell AS\n\
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
    *PREVIOUS_WIFI.lock().unwrap_or_else(|e| e.into_inner()) = previous.clone();
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

// ------------------------------------------------ Windows Mobile Hotspot
//
// WHY THE HUB SWITCHES IT ON ITSELF NOW. Until 0.9.9 the Windows screen said
// "switch the hotspot on yourself first: Settings, Network and internet,
// Mobile hotspot", and the owner's verdict was that nobody this is for gets
// through that. Measured on the Windows laptop, 2026-09-23, from an ordinary
// process with no administrator rights:
//
//   switched on                  Success, in 314 ms
//   read the name and password   yes
//   set a new name and password  yes, and put the old ones back
//   with NO internet             yes: made from the unplugged Ethernet
//                                profile, it came up on 192.168.137.1
//
// So the hub does it. The Windows Runtime call is reached through
// PowerShell, which every Windows 10 and 11 has, because calling WinRT from
// Rust directly would need a crate or a thousand lines of COM. The script
// goes as -EncodedCommand so no quoting can mangle it, and the name and
// password travel in environment variables, never on a command line.

#[cfg(windows)]
const WIN_HOTSPOT: &str = r#"
$ErrorActionPreference = 'Stop'
try {
  Add-Type -AssemblyName System.Runtime.WindowsRuntime
  $ext = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 }
  $opT = $ext | Where-Object { $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' } | Select-Object -First 1
  $actT = $ext | Where-Object { $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncAction' } | Select-Object -First 1
  $null = [Windows.Networking.Connectivity.NetworkInformation,Windows.Networking.Connectivity,ContentType=WindowsRuntime]
  $null = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager,Windows.Networking,ContentType=WindowsRuntime]
  $Res = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult]
  $NI = [Windows.Networking.Connectivity.NetworkInformation]
  $TM = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager]
  function Op($o) { $t = $opT.MakeGenericMethod($Res).Invoke($null, @($o)); $null = $t.Wait(30000); $t.Result }
  function Act($o) { $t = $actT.Invoke($null, @($o)); $null = $t.Wait(30000) }
  # The internet connection if there is one; otherwise any connection
  # Windows will share from, which in the bush is the unplugged Ethernet.
  $mgr = $null
  $p = $NI::GetInternetConnectionProfile()
  if ($p) { try { $mgr = $TM::CreateFromConnectionProfile($p) } catch { $first = $_.Exception.Message } }
  if (-not $mgr) {
    foreach ($q in $NI::GetConnectionProfiles()) { try { $mgr = $TM::CreateFromConnectionProfile($q); break } catch { if (-not $first) { $first = $_.Exception.Message } } }
  }
  if (-not $mgr) { "HUB_ERR $first"; exit 2 }
  $cfg = $mgr.GetCurrentAccessPointConfiguration()
  switch ($env:HUB_ACTION) {
    'read' { "HUB_SSID $($cfg.Ssid)"; "HUB_PASS $($cfg.Passphrase)"; "HUB_STATE $($mgr.TetheringOperationalState)" }
    'start' {
      # The hotspot is broadcast BY the wifi card: with wifi switched off
      # there is nothing to broadcast from. Measured 2026-09-23: switching the
      # laptop's wifi off killed the network, and an ordinary program may
      # switch the radio back on (RequestAccessAsync: Allowed).
      try {
        $null = [Windows.Devices.Radios.Radio,Windows.System.Devices,ContentType=WindowsRuntime]
        $radT = $opT.MakeGenericMethod([System.Collections.Generic.IReadOnlyList[Windows.Devices.Radios.Radio]])
        $accT = $opT.MakeGenericMethod([Windows.Devices.Radios.RadioAccessStatus])
        $t = $accT.Invoke($null, @([Windows.Devices.Radios.Radio]::RequestAccessAsync())); $null = $t.Wait(15000)
        $t = $radT.Invoke($null, @([Windows.Devices.Radios.Radio]::GetRadiosAsync())); $null = $t.Wait(15000)
        $wifi = $t.Result | Where-Object { $_.Kind -eq 'WiFi' } | Select-Object -First 1
        if ($wifi -and $wifi.State -ne 'On') {
          $t = $accT.Invoke($null, @($wifi.SetStateAsync('On'))); $null = $t.Wait(15000)
          "HUB_RADIO $($t.Result)"
          Start-Sleep -Seconds 2
        }
      } catch { "HUB_RADIO failed $($_.Exception.Message)" }
      # 2.4 GHz, always. Left on Auto, a laptop joined to nothing let
      # Windows pick 5 GHz, and the card reported the network On while no
      # phone could see it: two phones, 2026-09-23, 17:46 to 17:49, nothing.
      # Many cards may not transmit on 5 GHz until a nearby router has told
      # them the country, and in the bush there is no router. Every phone
      # sees 2.4 GHz, and it is allowed everywhere.
      $band = $cfg.Band
      $want = if ($env:HUB_BAND -eq '5') { 'FiveGigahertz' } else { 'TwoPointFourGigahertz' }
      try { if ($cfg.IsBandSupported($want)) { $band = $want } } catch { }
      $changed = ($cfg.Ssid -ne $env:HUB_SSID) -or ($cfg.Passphrase -ne $env:HUB_PASS) -or ([string]$cfg.Band -ne [string]$band)
      if ($changed -and $mgr.TetheringOperationalState -ne 'Off') { $null = Op ($mgr.StopTetheringAsync()) }
      if ($changed) { $cfg.Ssid = $env:HUB_SSID; $cfg.Passphrase = $env:HUB_PASS; try { $cfg.Band = $band } catch { }; Act ($mgr.ConfigureAccessPointAsync($cfg)) }
      "HUB_BAND $($mgr.GetCurrentAccessPointConfiguration().Band)"
      # Windows switches its hotspot off after five minutes with no phone on
      # it (network.log, 2026-09-24: 09:00 and 09:05 by its clock, between two tests). In a
      # lesson that is any quiet moment. The same switch as Settings' "turn
      # off when no devices are connected", allowed without administrator.
      # A note file says it was on, so stopping puts it back, and only then.
      try {
        if ($TM::IsNoConnectionsTimeoutEnabled()) {
          $TM::DisableNoConnectionsTimeout()
          $null = New-Item -ItemType File -Force -Path $env:HUB_TIMEOUT_NOTE
          'HUB_TIMEOUT switched off'
        }
      } catch { "HUB_TIMEOUT failed $($_.Exception.Message)" }
      if ($mgr.TetheringOperationalState -ne 'On') {
        $r = Op ($mgr.StartTetheringAsync())
        "HUB_START $($r.Status) $($r.AdditionalErrorMessage)"
      } else { 'HUB_START Success' }
      "HUB_STATE $($mgr.TetheringOperationalState)"
    }
    'channel' {
      # The channel the card is really broadcasting on, asked of the card
      # itself on the hotspot's own adapter: the one holding 192.168.137.1.
      $ip = Get-NetIPAddress -IPAddress 192.168.137.1 -ErrorAction SilentlyContinue | Select-Object -First 1
      if (-not $ip) { 'HUB_CHANNEL none'; break }
      $g = (Get-NetAdapter -InterfaceIndex $ip.InterfaceIndex -IncludeHidden).InterfaceGuid
      Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class HubWlan {
  [DllImport("wlanapi.dll")] static extern int WlanOpenHandle(uint v, IntPtr r, out uint n, out IntPtr h);
  [DllImport("wlanapi.dll")] static extern int WlanCloseHandle(IntPtr h, IntPtr r);
  [DllImport("wlanapi.dll")] static extern int WlanQueryInterface(IntPtr h, ref Guid g, int op, IntPtr r, out int s, out IntPtr d, IntPtr t);
  [DllImport("wlanapi.dll")] static extern void WlanFreeMemory(IntPtr p);
  public static int Channel(Guid g) {
    uint n; IntPtr h; if (WlanOpenHandle(2, IntPtr.Zero, out n, out h) != 0) return 0;
    try { int s; IntPtr d; if (WlanQueryInterface(h, ref g, 8, IntPtr.Zero, out s, out d, IntPtr.Zero) != 0) return 0;
          int c = Marshal.ReadInt32(d); WlanFreeMemory(d); return c; }
    finally { WlanCloseHandle(h, IntPtr.Zero); }
  }
}
'@
      "HUB_CHANNEL $([HubWlan]::Channel([Guid]$g))"
    }
    'stop' {
      # Whichever profile it was started from: stop every one that is on.
      foreach ($q in $NI::GetConnectionProfiles()) {
        try { $m = $TM::CreateFromConnectionProfile($q) } catch { continue }
        if ($m.TetheringOperationalState -ne 'Off') { $r = Op ($m.StopTetheringAsync()); "HUB_STOP $($r.Status)" }
      }
      # Put Windows' five-minute switch-off back, if the hub was the one
      # that took it away.
      if ($env:HUB_TIMEOUT_NOTE -and (Test-Path $env:HUB_TIMEOUT_NOTE)) {
        try { $TM::EnableNoConnectionsTimeout(); Remove-Item $env:HUB_TIMEOUT_NOTE; 'HUB_TIMEOUT switched back on' } catch { "HUB_TIMEOUT failed $($_.Exception.Message)" }
      }
      "HUB_STATE $($mgr.TetheringOperationalState)"
    }
  }
} catch { "HUB_ERR $($_.Exception.Message)"; exit 1 }
"#;

/// The router this laptop really uses, read from Windows' route table.
///
/// default_gateway() on Windows is a guess, ".1 of our subnet", and on the
/// test day the phone that was the router sat at .74, so names passed on from
/// the hotspot went nowhere. `route print` lines are numbers in every
/// language: destination 0.0.0.0, mask 0.0.0.0, then the gateway. The lowest
/// metric wins. Kept for thirty seconds, because the hotspot asks per name.
#[cfg(windows)]
pub fn route_gateway() -> Option<Ipv4Addr> {
    static CACHE: std::sync::Mutex<Option<(std::time::Instant, Option<Ipv4Addr>)>> = std::sync::Mutex::new(None);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, gw)) = *c {
        if at.elapsed() < std::time::Duration::from_secs(30) {
            return gw;
        }
    }
    let out = std::process::Command::new("route")
        .args(["print", "-4", "0.0.0.0"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let gw = gateway_from_route_print(&String::from_utf8_lossy(&out.stdout));
    *c = Some((std::time::Instant::now(), gw));
    gw
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn gateway_from_route_print(text: &str) -> Option<Ipv4Addr> {
    let mut best: Option<(u32, Ipv4Addr)> = None;
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 5 || f[0] != "0.0.0.0" || f[1] != "0.0.0.0" {
            continue;
        }
        let (Ok(gw), Ok(metric)) = (f[2].parse::<Ipv4Addr>(), f[4].parse::<u32>()) else { continue };
        if best.is_none_or(|(m, _)| metric < m) {
            best = Some((metric, gw));
        }
    }
    best.map(|(_, g)| g)
}

/// Standard base64, for PowerShell's -EncodedCommand. Twelve lines rather
/// than a crate.
#[cfg(windows)]
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

/// 5 GHz when the person chose it on the start screen; 2.4 GHz otherwise.
/// Kept here so the guard restarts the network on the same band.
static WIFI_BAND_5: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_wifi_band_5(five: bool) {
    WIFI_BAND_5.store(five, std::sync::atomic::Ordering::Relaxed);
}

/// The channel the card was last measured broadcasting on. None: not
/// measured yet, or the network is not up.
static HOTSPOT_CHANNEL: std::sync::Mutex<Option<u32>> = std::sync::Mutex::new(None);

pub fn hotspot_channel() -> Option<u32> {
    *HOTSPOT_CHANNEL.lock().unwrap_or_else(|e| e.into_inner())
}

/// "2.4 GHz, channel 6" for the screen.
pub fn describe_channel(ch: u32) -> String {
    let band = if ch <= 14 { "2.4 GHz" } else if ch <= 177 { "5 GHz" } else { "6 GHz" };
    format!("{band}, channel {ch}")
}

/// Ask the card which channel the hotspot is on. A second or two of
/// PowerShell, so it is done after a start and then now and again, never
/// while drawing.
#[cfg(windows)]
fn measure_channel() {
    let ch = win_hotspot("channel", "", "")
        .ok()
        .and_then(|l| win_get(&l, "CHANNEL").and_then(|c| c.parse::<u32>().ok()))
        .filter(|c| *c > 0);
    *HOTSPOT_CHANNEL.lock().unwrap_or_else(|e| e.into_inner()) = ch;
}

/// What this laptop's wifi card can do as a hotspot, in one line, read from
/// the card's own report once. Written for the fix screen, so nobody hunts
/// for a feature the hardware does not have (asked about: several bands at
/// once, 6 GHz, choosing the channel).
///
/// netsh words its report in the language Windows is installed in; where
/// the English labels are not found, this says nothing rather than guess.
pub fn wifi_card_summary() -> Option<String> {
    static CACHE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        if !cfg!(windows) {
            return None;
        }
        let run = |args: &[&str]| {
            std::process::Command::new("netsh")
                .args(args)
                .stdin(std::process::Stdio::null())
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        };
        let drivers = run(&["wlan", "show", "drivers"])?;
        let caps = run(&["wlan", "show", "wirelesscapabilities"]).unwrap_or_default();
        card_summary_from(&drivers, &caps)
    })
    .clone()
}

pub(crate) fn card_summary_from(drivers: &str, caps: &str) -> Option<String> {
    let field = |text: &str, label: &str| {
        text.lines()
            .find(|l| l.trim_start().starts_with(label))
            .and_then(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()))
    };
    let name = field(drivers, "Driver")?;
    let five = field(caps, "P2P GO on 5 GHz").map(|v| v.starts_with("Supported"));
    let six = field(caps, "P2P GO on 6 GHz").map(|v| v.starts_with("Supported"));
    let mlo = field(caps, "Number of MLO Connections Supported").and_then(|v| v.parse::<u32>().ok());
    let ports = field(caps, "P2P GO ports count").and_then(|v| v.parse::<u32>().ok());
    let mut bands = vec!["2.4 GHz"];
    if five == Some(true) {
        bands.push("5 GHz");
    }
    if six == Some(true) {
        bands.push("6 GHz");
    }
    let mut s = format!("{name}: a network on {}", bands.join(" or "));
    if six == Some(false) {
        s.push_str(", not 6 GHz");
    }
    if ports == Some(1) {
        s.push_str("; one network at a time");
    }
    if mlo == Some(0) {
        s.push_str("; no multi-band (MLO)");
    }
    s.push_str("; Windows picks the channel.");
    Some(s)
}

/// The note that says the hub switched off Windows' five-minute hotspot
/// timeout, so that stopping puts it back. A file rather than memory because
/// the watchman, which does the stopping after a crash or the window's X, is
/// another process.
#[cfg(windows)]
fn timeout_note() -> std::path::PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = base.join("PortableNetworkHub");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("hotspot-timeout-was-on")
}

/// Run the hotspot script and return its `HUB_*` lines as (key, rest).
#[cfg(windows)]
fn win_hotspot(action: &str, ssid: &str, password: &str) -> Result<Vec<(String, String)>, String> {
    let utf16: Vec<u8> = WIN_HOTSPOT.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &base64(&utf16)])
        .env("HUB_ACTION", action)
        .env("HUB_SSID", ssid)
        .env("HUB_PASS", password)
        .env("HUB_BAND", if WIFI_BAND_5.load(std::sync::atomic::Ordering::Relaxed) { "5" } else { "2.4" })
        .env("HUB_TIMEOUT_NOTE", timeout_note())
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("Could not ask Windows to switch the hotspot: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<(String, String)> = text
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("HUB_")?;
            let (k, v) = rest.split_once(' ').unwrap_or((rest, ""));
            Some((k.to_string(), v.trim().to_string()))
        })
        .collect();
    Ok(lines)
}

/// The hidden helper that switches the hotspot off when this program ends.
///
/// WHY. Closing the window with its X ended the hub without it running a
/// line of its own, and the hotspot it had switched on stayed on: a laptop
/// broadcasting a network nobody was serving, draining its battery, until
/// someone found the switch in Settings. The owner asked for this before
/// anything else. The same goes for Ctrl-C on the command line, a crash, and
/// Task Manager, none of which let a program tidy up after itself.
///
/// So the tidying does not live in this program. A second, windowless
/// process waits for this one to end, however it ends, and then switches the
/// hotspot off: the Windows twin of the systemd timer the Linux side uses.
/// It is given its own hidden console and its own process group, so closing
/// the hub's window or pressing Ctrl-C in it does not take the watchman with
/// it. A clean Stop sends it home first (disarm_restore).
#[cfg(windows)]
static WATCHMAN: std::sync::Mutex<Option<std::process::Child>> = std::sync::Mutex::new(None);

#[cfg(windows)]
fn arm_watchman() {
    use std::os::windows::process::CommandExt;
    let mut w = WATCHMAN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(child) = w.as_mut() {
        if matches!(child.try_wait(), Ok(None)) {
            return; // already watching
        }
    }
    let script = format!(
        "Wait-Process -Id {} -ErrorAction SilentlyContinue\n{}",
        std::process::id(),
        WIN_HOTSPOT
    );
    let utf16: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let encoded = base64(&utf16);
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let spawn = |flags: u32| {
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encoded])
            .env("HUB_ACTION", "stop")
            .env("HUB_TIMEOUT_NOTE", timeout_note())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(flags)
            .spawn()
    };
    // Out of the terminal's job if it has one, so closing the terminal does
    // not end the watchman along with everything else in it. A job that
    // forbids that refuses the spawn, and then it goes without.
    let base = CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP;
    *w = spawn(base | CREATE_BREAKAWAY_FROM_JOB).or_else(|_| spawn(base)).ok();
}

/// The name and password the hotspot should have, while the hub wants it
/// up. None once it has been stopped on purpose.
#[cfg(windows)]
static HOTSPOT_WANTED: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);
/// One start or stop at a time, so the guard can never switch the network
/// back on just after a Stop switched it off.
#[cfg(windows)]
static HOTSPOT_OP: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// What the guard last found, in words for the screen. Empty: all well.
static HOTSPOT_PROBLEM: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Something wrong with the network this program made, said plainly, or
/// None when it is up as it should be.
pub fn hotspot_problem() -> Option<String> {
    let p = HOTSPOT_PROBLEM.lock().unwrap_or_else(|e| e.into_inner());
    (!p.is_empty()).then(|| p.clone())
}

#[cfg(windows)]
fn set_problem(s: &str) {
    *HOTSPOT_PROBLEM.lock().unwrap_or_else(|e| e.into_inner()) = s.to_string();
}

/// Keep the hotspot up for as long as it is wanted.
///
/// WHY. Tested with a phone on 2026-09-23: the laptop's wifi was switched
/// off, which took the hotspot with it, and the screen went on showing the
/// codes to scan for a network that was no longer there. Windows also
/// switches the hotspot off by itself after five minutes with nobody on it,
/// on a default setting. A network that silently disappears is the worst
/// thing this can do in a room. So every few seconds the guard looks, and if
/// the network is gone it switches the radio and the network back on, and
/// says so on the screen while it does.
#[cfg(windows)]
fn guard_hotspot() {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(|| {
        let mut misses = 0;
        let mut beats = 0u32;
        let mut down_since: Option<std::time::Instant> = None;
        // Starts Windows accepted that still brought no address.
        let mut empty_starts = 0u32;
        loop {
            // Two seconds while the network is missing, three while it is up.
            let pause = if misses > 0 { 2 } else { 3 };
            std::thread::sleep(std::time::Duration::from_secs(pause));
            beats += 1;
            let wanted = HOTSPOT_WANTED.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let Some((ssid, pass)) = wanted else {
                set_problem("");
                misses = 0;
                down_since = None;
                continue;
            };
            if hotspot_address_of("").is_some() {
                if let Some(t) = down_since.take() {
                    network_log(&format!("network back after {} s", t.elapsed().as_secs()));
                }
                set_problem("");
                misses = 0;
                // Every half minute, and at once if never measured.
                if beats % 10 == 0 || hotspot_channel().is_none() {
                    measure_channel();
                }
                continue;
            }
            *HOTSPOT_CHANNEL.lock().unwrap_or_else(|e| e.into_inner()) = None;
            // Twice in a row, so a moment's hiccup is not a restart.
            misses += 1;
            if misses == 1 {
                down_since = Some(std::time::Instant::now());
                network_log("network gone: the hotspot's address disappeared");
                continue;
            }
            set_problem("The wifi network went off. Switching it back on...");
            let _op = HOTSPOT_OP.lock().unwrap_or_else(|e| e.into_inner());
            // Stopped on purpose while we waited for the lock: leave it off.
            if HOTSPOT_WANTED.lock().unwrap_or_else(|e| e.into_inner()).is_none() {
                continue;
            }
            // A start Windows accepted, twice, and still no address: switch
            // it fully off first. Seen 2026-09-24 after the services had been
            // switched off and on: Windows said On through about two minutes of
            // restarts, the address never came, and a stop then start brought
            // it up at once. (The stop is the hub's own restart, done by hand
            // that morning; this has not yet been seen to cure it by itself.)
            if empty_starts >= 2 {
                let _ = win_hotspot("stop", "", "");
                network_log("still no address after two starts: switched fully off, starting again");
                std::thread::sleep(std::time::Duration::from_secs(2));
                empty_starts = 0;
            }
            let lines = win_hotspot("start", &ssid, &pass).unwrap_or_default();
            let said: Vec<String> = lines.iter().map(|(k, v)| format!("{k} {v}")).collect();
            network_log(&format!("switching back on; Windows said: {}", said.join(" | ")));
            let radio_refused = win_get(&lines, "RADIO").is_some_and(|r| r != "Allowed");
            // WHY THE WAIT IS CONDITIONAL. The guard used to wait ten seconds
            // for the address after every attempt, including attempts Windows
            // had already refused. Measured 2026-09-23, 18:50: Windows switched
            // the hotspot off itself as the last phone left, refused the first
            // tries, and with 3 + 10 s per try the network was gone for 40 s.
            // Only a start Windows accepted is worth waiting for; a refusal is
            // tried again two seconds later.
            let accepted = win_get(&lines, "STATE") == Some("On")
                || win_get(&lines, "START").is_some_and(|s| s.starts_with("Success"));
            let mut up = false;
            if accepted {
                for _ in 0..40 {
                    if hotspot_address_of("").is_some() {
                        up = true;
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
            }
            if accepted && !up {
                empty_starts += 1;
            }
            if up {
                empty_starts = 0;
                if let Some(t) = down_since.take() {
                    network_log(&format!("network back after {} s", t.elapsed().as_secs()));
                }
                set_problem("");
                misses = 0;
                measure_channel();
            } else if radio_refused {
                set_problem(
                    "THE WIFI NETWORK IS OFF: this laptop's wifi is switched off and \
                     Windows would not let the hub switch it on. Switch wifi ON (it does \
                     not need to join anything). The network comes back by itself.",
                );
            } else {
                set_problem(
                    "THE WIFI NETWORK IS OFF and did not come back yet. Make sure this \
                     laptop's wifi is switched ON (aeroplane mode off). The hub keeps trying.",
                );
            }
        }
    });
}

/// One line in %LOCALAPPDATA%\PortableNetworkHub\network.log.
///
/// The network's comings and goings, and what Windows answered each time the
/// hub asked for it back, written as they happen. Asked for by the owner after
/// a day of reconstructing outages from screenshots; the monitor script does
/// the same from outside, this is the hub's own account.
#[cfg(windows)]
fn network_log(line: &str) {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("PortableNetworkHub");
    let _ = std::fs::create_dir_all(&dir);
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("network.log")) {
        let _ = writeln!(f, "{}  {line}", timestamp());
    }
}

#[cfg(windows)]
fn win_get<'a>(lines: &'a [(String, String)], key: &str) -> Option<&'a str> {
    lines.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

#[cfg(windows)]
pub fn hotspot_up(ssid: &str, password: &str, _channel: Option<u16>) -> Result<Hotspot, String> {
    if password.chars().count() < 8 {
        return Err("A wifi password has to be at least 8 characters. That is a rule of WPA2, not ours.".into());
    }
    if ssid.is_empty() || ssid.len() > 32 {
        return Err("The network name has to be between 1 and 32 letters long.".into());
    }
    // Switched off by a tweak list: say so, with the fix, before trying.
    let off = disabled_hotspot_services();
    if let Some(advice) = switched_off_advice(&off) {
        return Err(format!("This laptop cannot make a wifi network yet.\n{advice}"));
    }
    let _op = HOTSPOT_OP.lock().unwrap_or_else(|e| e.into_inner());
    let lines = win_hotspot("start", ssid, password)?;
    if let Some(t) = win_get(&lines, "TIMEOUT") {
        network_log(&format!("Windows' five-minute switch-off: {t}"));
    }
    let state = win_get(&lines, "STATE").unwrap_or("");
    if state != "On" {
        let why = win_get(&lines, "ERR")
            .or_else(|| win_get(&lines, "START"))
            .unwrap_or("no answer from Windows");
        // 0x83120001 is what Windows says when the hotspot services are off,
        // measured 2026-09-08, and the thing Settings shows as an empty box.
        return Err(if why.contains("0x83120001") {
            "Windows would not make the wifi network: parts of Windows it needs are \
             switched off. Choose \"Fix problems with this computer\" on the first \
             screen, then try again."
                .to_string()
        } else {
            format!(
                "Windows would not switch the wifi network on.\n\n{why}\n\n\
                 Check that wifi is switched on (the aeroplane mode button is off), \
                 then try again."
            )
        });
    }
    // Wait until the hotspot's own address is there AND stays there. When the
    // name or password changes Windows switches the network off and on, and
    // for a moment the OLD address is still present: measured 2026-09-23,
    // gone and back about four seconds in. A single look saw the old one and
    // reported ready too early. A second and a half of steady answers does
    // not. Fifteen seconds at most, then the screen follows on its own.
    let mut steady = 0;
    for _ in 0..60 {
        if hotspot_address_of("").is_some() {
            steady += 1;
            if steady >= 6 {
                break;
            }
        } else {
            steady = 0;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    *HOTSPOT_WANTED.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((ssid.to_string(), password.to_string()));
    measure_channel();
    network_log(&format!("network '{ssid}' switched on{}", hotspot_channel().map(|c| format!(", {}", describe_channel(c))).unwrap_or_default()));
    guard_hotspot();
    Ok(Hotspot {
        ssid: ssid.to_string(),
        iface: "Mobile hotspot".to_string(),
        previous: None,
        profile: None,
        password: password.to_string(),
    })
}

#[cfg(not(any(target_os = "linux", windows)))]
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

/// The password this machine last used for a hotspot called `ssid`.
///
/// Every laptop that has joined a network remembers its password. Handing
/// the same name out with a new password means each of them tries the old one
/// first, fails, waits and tries again before anyone is asked, which is what
/// testers saw as "takes ages to connect". NetworkManager already keeps the
/// profile from last time, so its password is read back from there.
#[cfg(target_os = "linux")]
pub fn saved_hotspot_password(ssid: &str) -> Option<String> {
    if ssid.is_empty() {
        return None;
    }
    let out = std::process::Command::new("nmcli")
        .args(["-t", "-f", "NAME,TYPE", "connection", "show"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some((name, kind)) = line.rsplit_once(':') else { continue };
        if !kind.contains("wireless") {
            continue;
        }
        let name = unescape_terse(name);
        let detail = std::process::Command::new("nmcli")
            .args([
                "-s", "-g",
                "802-11-wireless.ssid,802-11-wireless.mode,802-11-wireless-security.psk",
                "connection", "show", &name,
            ])
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&detail.stdout);
        let mut lines = text.lines();
        let (Some(s), Some(mode), Some(psk)) = (lines.next(), lines.next(), lines.next()) else {
            continue;
        };
        // -g escapes colons the same way -t does.
        let (s, psk) = (unescape_terse(s), unescape_terse(psk));
        if s == ssid && mode == "ap" && psk.chars().count() >= 8 {
            return Some(psk);
        }
    }
    None
}

/// Windows keeps one hotspot name and password; if the name is this one,
/// that password is the one every laptop already has saved.
#[cfg(windows)]
pub fn saved_hotspot_password(ssid: &str) -> Option<String> {
    let lines = win_hotspot("read", "", "").ok()?;
    let name = win_get(&lines, "SSID")?;
    let pass = win_get(&lines, "PASS")?;
    (name == ssid && pass.chars().count() >= 8).then(|| pass.to_string())
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn saved_hotspot_password(_ssid: &str) -> Option<String> {
    None
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

    /// Switch Windows' hotspot back off. The name and password are left as
    /// the hub set them, so the laptops that joined today join again next
    /// time without being asked.
    #[cfg(windows)]
    pub fn down(&self) {
        *HOTSPOT_WANTED.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let _op = HOTSPOT_OP.lock().unwrap_or_else(|e| e.into_inner());
        let lines = win_hotspot("stop", "", "").unwrap_or_default();
        set_problem("");
        network_log("network switched off: stopped in the hub");
        if let Some(t) = win_get(&lines, "TIMEOUT") {
            network_log(&format!("Windows' five-minute switch-off: {t}"));
        }
    }

    #[cfg(not(any(target_os = "linux", windows)))]
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

    /// Windows re-forms the network under a new password the same way: the
    /// script sees the change, stops, reconfigures and starts again, and
    /// every device is dropped until it has the new one.
    #[cfg(windows)]
    pub fn change_password(&mut self, new: &str) -> Result<(), String> {
        if new.chars().count() < 8 {
            return Err("A wifi password has to be at least 8 characters. That is a rule of WPA2, not ours.".into());
        }
        let _op = HOTSPOT_OP.lock().unwrap_or_else(|e| e.into_inner());
        let lines = win_hotspot("start", &self.ssid, new)?;
        *HOTSPOT_WANTED.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((self.ssid.clone(), new.to_string()));
        if win_get(&lines, "STATE") != Some("On") {
            return Err("The password was changed but the network did not come back up. \
                        Stop and start handing out again."
                .into());
        }
        self.password = new.to_string();
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", windows)))]
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
        let mut armed = RESTORE_ARMED.lock().unwrap_or_else(|e| e.into_inner());
        *armed = true;
        rearm_restore(seconds);
        #[cfg(windows)]
        arm_watchman();
    }

    /// Cancel the safety net, on the way out of a clean shutdown that has
    /// already put the wifi back itself.
    #[cfg(target_os = "linux")]
    pub fn disarm_restore(&self) {
        // Held while the units stop, so the heartbeat cannot re-arm between.
        let mut armed = RESTORE_ARMED.lock().unwrap_or_else(|e| e.into_inner());
        *armed = false;
        // Stopping the service kills its sleep before the nmcli runs, which
        // is what disarming means. The .timer name is the previous build's.
        for unit in ["hub-wifi-restore.service", "hub-wifi-restore.timer"] {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "stop", unit])
                .output();
        }
    }

    /// On Windows: send the watchman home. Called after a clean stop has
    /// already switched the hotspot off itself.
    #[cfg(windows)]
    pub fn disarm_restore(&self) {
        let mut w = WATCHMAN.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut child) = w.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    pub fn disarm_restore(&self) {}

    /// The address this machine holds ON THE HOTSPOT, asked for rather than
    /// guessed.
    ///
    /// It decides which subnet counts as "the class", so getting it wrong means
    /// either listing nobody or listing a whole office. NetworkManager's shared
    /// mode uses 10.42.0.1 in practice, but that is a default and not a
    /// promise, and a machine with a second interface can easily have another
    /// address that sorts first.
    pub fn address(&self) -> Option<Ipv4Addr> {
        hotspot_address_of(&self.iface)
    }
}

/// The address this machine holds on the hotspot on `iface`, and only once
/// it really holds it. A free function so a waiting thread can ask without
/// holding on to the Hotspot.
#[cfg(target_os = "linux")]
pub fn hotspot_address_of(iface: &str) -> Option<Ipv4Addr> {
    {
        let out = std::process::Command::new("nmcli")
            .args(["-g", "IP4.ADDRESS", "device", "show", iface])
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
}

/// Windows' hotspot is always 192.168.137.1, but asking "which address
/// would I use to reach it" answers with whatever network the laptop is on
/// until the hotspot's own address exists. Measured 2026-09-23: for about
/// four seconds after switching on, that answer was the phone network's
/// the other network's address, and gorilla.local was started there, on the
/// wrong network.
/// So it counts only when the answer is the hotspot's address itself.
#[cfg(windows)]
pub fn hotspot_address_of(_iface: &str) -> Option<Ipv4Addr> {
    let ics = Ipv4Addr::new(192, 168, 137, 1);
    (source_address_for(ics) == Some(ics)).then_some(ics)
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn hotspot_address_of(_iface: &str) -> Option<Ipv4Addr> {
    source_address_for(Ipv4Addr::new(192, 168, 137, 1))
}

#[cfg(test)]
mod card_tests {
    use super::*;

    /// This laptop's own report, 2026-09-23, reduced to the lines used.
    #[test]
    fn the_card_is_described_from_its_own_report() {
        let drivers = "    Driver                    : Intel(R) Wi-Fi 6 AX201 160MHz\n    Vendor : Intel\n";
        let caps = "    P2P GO on 5 GHz                             : Supported\n    P2P GO on 6 GHz                             : Not Supported\n    P2P GO ports count                          : 1\n    Number of MLO Connections Supported         : 0\n";
        assert_eq!(
            card_summary_from(drivers, caps).unwrap(),
            "Intel(R) Wi-Fi 6 AX201 160MHz: a network on 2.4 GHz or 5 GHz, not 6 GHz; one network at a time; no multi-band (MLO); Windows picks the channel."
        );
        assert_eq!(card_summary_from("nothing useful", ""), None);
        assert_eq!(describe_channel(6), "2.4 GHz, channel 6");
        assert_eq!(describe_channel(149), "5 GHz, channel 149");
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;

    /// The test day's route table, and a second default route with a worse
    /// metric that must lose.
    #[test]
    fn the_router_is_read_from_the_route_table_not_guessed() {
        let text = "IPv4 Route Table
  Network Destination        Netmask          Gateway       Interface  Metric
          0.0.0.0          0.0.0.0      10.0.5.74        10.0.5.96     35
          0.0.0.0          0.0.0.0      10.0.0.1         10.0.0.5     50
";
        assert_eq!(gateway_from_route_print(text), Some(Ipv4Addr::new(10, 0, 5, 74)));
        assert_eq!(gateway_from_route_print("no routes here"), None);
    }
}

#[cfg(all(test, windows))]
mod win_hotspot_tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    /// Reading needs nothing switched on and changes nothing, so it can run
    /// on any Windows machine: the script must at least come back with the
    /// hotspot's state, or with an error line, never with silence.
    #[test]
    fn the_script_runs_and_answers() {
        let lines = win_hotspot("read", "", "").expect("powershell runs");
        assert!(
            win_get(&lines, "STATE").is_some() || win_get(&lines, "ERR").is_some(),
            "no answer: {lines:?}"
        );
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
