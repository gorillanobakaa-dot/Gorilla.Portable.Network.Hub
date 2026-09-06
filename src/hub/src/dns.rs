// Version: 1.0.0 · updated 26-08-24-23-40
//
// Just enough DNS to ask "what is this device called?".
//
// WHY THIS EXISTS. The names of the devices in the room are already known: they
// tell the network what they are called when they ask for an address, and
// dnsmasq writes them down. But it writes them into
// /var/lib/NetworkManager/, which is drwx------, so a teacher who started the
// hotspot the normal way cannot read it and gets a list of numbers instead of a
// list of devices.
//
// dnsmasq is also the DNS server for that network, and it answers reverse
// lookups for its own leases. So the same information is available over the
// network, to anybody, with no privileges at all. That is the whole idea here.
//
// WHY NOT THE SYSTEM RESOLVER. `getent hosts` or getaddrinfo would be three
// lines, and both can block for five seconds on a resolver that is not
// answering, with no way to shorten it. This program draws a screen four times
// a second. A five second freeze on a teacher's laptop, in front of a class, is
// not a trade worth three lines. Here the timeout is ours.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

/// Ask `server` what `ip` is called. None means no answer, which is normal:
/// a device that gave no name when it joined does not have one to give.
pub fn reverse_lookup(ip: Ipv4Addr, server: Ipv4Addr, timeout: Duration) -> Option<String> {
    reverse_lookup_at(ip, SocketAddr::from((server, 53)), timeout)
}

/// The same thing with the port spelled out, so a test can point it at a real
/// server on an ephemeral port. Hardcoding 53 left the round-trip test with
/// nothing it could assert, and a test that cannot fail is worse than none.
pub fn reverse_lookup_at(ip: Ipv4Addr, server: SocketAddr, timeout: Duration) -> Option<String> {
    let o = ip.octets();
    let name = format!("{}.{}.{}.{}.in-addr.arpa", o[3], o[2], o[1], o[0]);
    let id = transaction_id();
    let query = build_query(id, &name);

    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.set_read_timeout(Some(timeout)).ok()?;
    sock.send_to(&query, server).ok()?;

    let mut buf = [0u8; 512];
    // Answers that are not ours are dropped and we keep waiting, rather than
    // treating the first packet that arrives as the reply. Reading the ID is
    // the only thing that makes a UDP request/response pair a pair.
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::time::Instant::now() >= deadline {
            return None;
        }
        let (n, from) = sock.recv_from(&mut buf).ok()?;
        if from.ip() != server.ip() {
            continue;
        }
        if n >= 2 && u16::from_be_bytes([buf[0], buf[1]]) == id {
            return parse_ptr(&buf[..n]);
        }
    }
}

/// A predictable query ID lets anything on the network answer on dnsmasq's
/// behalf. This is a classroom rather than a bank, but the cost of doing it
/// properly is one call.
fn transaction_id() -> u16 {
    match crate::net::random_bytes(2) {
        Some(b) => u16::from_be_bytes([b[0], b[1]]),
        None => 0x4a3f,
    }
}

fn build_query(id: u16, name: &str) -> Vec<u8> {
    let mut q = Vec::with_capacity(40);
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&0x0100u16.to_be_bytes()); // standard query, recursion desired
    q.extend_from_slice(&1u16.to_be_bytes());      // one question
    q.extend_from_slice(&[0, 0, 0, 0, 0, 0]);      // no answers, authority, additional
    for label in name.split('.') {
        // A label is length-prefixed and capped at 63 bytes. Nothing here can
        // exceed that, but a length byte with the top bits set is a compression
        // pointer, so writing one by accident would corrupt the query.
        let bytes = label.as_bytes();
        let len = bytes.len().min(63);
        q.push(len as u8);
        q.extend_from_slice(&bytes[..len]);
    }
    q.push(0);                                     // root label ends the name
    q.extend_from_slice(&12u16.to_be_bytes());     // PTR
    q.extend_from_slice(&1u16.to_be_bytes());      // IN
    q
}

/// Pull the first PTR name out of a response.
fn parse_ptr(msg: &[u8]) -> Option<String> {
    if msg.len() < 12 {
        return None;
    }
    // RCODE lives in the low four bits of the second flags byte. NXDOMAIN is
    // an answer, just not a useful one.
    if msg[3] & 0x0f != 0 {
        return None;
    }
    let questions = u16::from_be_bytes([msg[4], msg[5]]);
    let answers = u16::from_be_bytes([msg[6], msg[7]]);
    if answers == 0 {
        return None;
    }

    let mut pos = 12;
    for _ in 0..questions {
        pos = skip_name(msg, pos)?;
        pos = pos.checked_add(4)?; // qtype, qclass
    }
    for _ in 0..answers {
        pos = skip_name(msg, pos)?;
        if pos + 10 > msg.len() {
            return None;
        }
        let rtype = u16::from_be_bytes([msg[pos], msg[pos + 1]]);
        let rdlen = u16::from_be_bytes([msg[pos + 8], msg[pos + 9]]) as usize;
        pos += 10;
        if pos + rdlen > msg.len() {
            return None;
        }
        if rtype == 12 {
            return read_name(msg, pos, 0);
        }
        pos += rdlen;
    }
    None
}

/// Step over a name, which may end in a compression pointer.
fn skip_name(msg: &[u8], mut pos: usize) -> Option<usize> {
    loop {
        let len = *msg.get(pos)?;
        if len & 0xc0 == 0xc0 {
            return Some(pos + 2); // a pointer is two bytes and ends the name
        }
        pos += 1;
        if len == 0 {
            return Some(pos);
        }
        pos = pos.checked_add(len as usize)?;
    }
}

/// Read a name, following compression pointers.
///
/// `depth` is not decoration. A response can point a name at itself, and
/// following that without a limit is an infinite loop inside a program that is
/// meant to be drawing a screen. This is the oldest trap in DNS parsing.
fn read_name(msg: &[u8], mut pos: usize, depth: u8) -> Option<String> {
    if depth > 8 {
        return None;
    }
    let mut out = String::new();
    loop {
        let len = *msg.get(pos)?;
        if len & 0xc0 == 0xc0 {
            let hi = (len & 0x3f) as usize;
            let lo = *msg.get(pos + 1)? as usize;
            let target = (hi << 8) | lo;
            // A pointer must point BACKWARDS. Forward or self pointers are the
            // shape a loop takes even under the depth limit.
            if target >= pos {
                return None;
            }
            let rest = read_name(msg, target, depth + 1)?;
            if !out.is_empty() && !rest.is_empty() {
                out.push('.');
            }
            out.push_str(&rest);
            return Some(out);
        }
        pos += 1;
        if len == 0 {
            return Some(out);
        }
        let end = pos.checked_add(len as usize)?;
        let label = msg.get(pos..end)?;
        if !out.is_empty() {
            out.push('.');
        }
        out.push_str(&String::from_utf8_lossy(label));
        pos = end;
    }
}

/// dnsmasq answers "xiaomi-11-lite.lan" or just "xiaomi-11-lite" depending on
/// how it was configured. A teacher wants the device, not the domain.
pub fn short_name(fqdn: &str) -> String {
    fqdn.split('.').next().unwrap_or(fqdn).to_string()
}

// ---------------------------------------------------------------- answering
//
// The other half: being a name server, not asking one.
//
// WHY. Two problems, one cause. On a bare cable the other computer has no name
// server, so:
//
//   - Nobody can type a name. The address is 169.254.87.61, and a person who
//     has never been told what a full stop means in that context has to copy
//     twelve digits off one screen onto another, correctly, before anything
//     happens. Adults who use computers every day do not know what a forward
//     slash is called. This is the single biggest thing standing between the
//     tool and the person it is for.
//
//   - The "Sign in to this network" prompt never appears. Every operating
//     system quietly fetches a known URL to decide whether it has internet:
//     www.msftconnecttest.com on Windows, captive.apple.com on a Mac,
//     connectivitycheck.gstatic.com on Android. Those are NAMES. With no
//     resolver the lookup fails, the system concludes it has no network at
//     all, and shows nothing worth clicking. With a resolver that points them
//     here, the probe reaches our HTTP server, gets a redirect instead of the
//     answer it wanted, and the system pops the prompt that opens a browser on
//     our page. Nothing typed at either end.
//
// So this answers every A query with our own address. Every one, deliberately:
// that is what a captive portal is, and on a cable with two machines and no
// internet there is no name it could be wrong about.
//
// WHAT IT MUST NEVER DO is the same rule dhcp.rs has, for the same reason. A
// server that claims to be every host on the internet is fine on a cable and a
// catastrophe on a school network, so it starts only under the same
// `dhcp::safe_to_offer` guard, and is never started unless a person asked for a
// cable transfer.

/// The port. Privileged on Unix, like 67, and for the same reason.
pub const SERVER_PORT: u16 = 53;

/// Names we answer for even when a query is not a captive-portal probe.
///
/// A person is told "type gorilla". Browsers treat a single bare word as a
/// search term rather than a host, so `gorilla/` with the slash, or
/// `gorilla.hub`, is what actually navigates. All three resolve here; which
/// one a given browser honours is the browser's business, and the sign-in
/// prompt means most people never type anything at all.
pub const OUR_NAMES: [&str; 3] = ["gorilla", "gorilla.hub", "hub"];

/// Answer one query. Returns the bytes to send back, or None if the packet is
/// not a question we can answer.
///
/// Split out from the socket loop so it can be tested without a network.
pub fn answer(query: &[u8], us: Ipv4Addr) -> Option<Vec<u8>> {
    // 12 bytes of header, then at least one question.
    if query.len() < 13 {
        return None;
    }
    let flags = u16::from_be_bytes([query[2], query[3]]);
    if flags & 0x8000 != 0 {
        return None; // this is a response, not a question
    }
    // OPCODE must be QUERY (0). An UPDATE or NOTIFY is not ours to answer.
    if (flags >> 11) & 0x0f != 0 {
        return None;
    }
    let qdcount = u16::from_be_bytes([query[4], query[5]]);
    if qdcount != 1 {
        return None; // exactly one question, which is what every client sends
    }

    let name_end = skip_name(query, 12)?;
    if name_end + 4 > query.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([query[name_end], query[name_end + 1]]);
    let qclass = u16::from_be_bytes([query[name_end + 2], query[name_end + 3]]);
    if qclass != 1 {
        return None; // IN only
    }

    let mut out = Vec::with_capacity(query.len() + 16);
    out.extend_from_slice(&query[0..2]); // the transaction id, echoed
    // QR=1 response, AA=1 authoritative, RD copied from the question, RA=1.
    // Authoritative matters: a resolver told "no such name, and I am only
    // guessing" retries elsewhere, and on a cable there is nowhere else.
    let rd = flags & 0x0100;
    let answered = qtype == 1 || qtype == 255; // A, or ANY
    let rcode = 0u16; // never NXDOMAIN: see below
    out.extend_from_slice(&(0x8580 | rd | rcode).to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
    out.extend_from_slice(&(if answered { 1u16 } else { 0u16 }).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
    out.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT
    out.extend_from_slice(&query[12..name_end + 4]); // the question, verbatim

    // A query for AAAA gets a real answer with no records rather than an
    // error. Saying "this name does not exist" would be a lie that stops the
    // client asking for the A record it would have accepted, and some
    // resolvers cache the denial for the whole name.
    if answered {
        out.extend_from_slice(&[0xc0, 0x0c]); // name: a pointer back to offset 12
        out.extend_from_slice(&1u16.to_be_bytes()); // TYPE A
        out.extend_from_slice(&1u16.to_be_bytes()); // CLASS IN
        // Sixty seconds. Long enough that a page of images does not re-ask for
        // every one, short enough that a laptop unplugged from this cable and
        // carried to a real network is not still holding our answer for the
        // name of somebody's bank.
        out.extend_from_slice(&60u32.to_be_bytes());
        out.extend_from_slice(&4u16.to_be_bytes()); // RDLENGTH
        out.extend_from_slice(&us.octets());
    }
    Some(out)
}

/// Answer names on this cable until told to stop.
///
/// The guard is `dhcp::safe_to_offer`, checked by the caller, and the socket
/// error is reported the same way for the same reason: on Linux and macOS this
/// port needs root, and a person has to be told that rather than left with a
/// prompt that never appears.
pub fn start(
    us: Ipv4Addr,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    let socket = UdpSocket::bind(("0.0.0.0", SERVER_PORT))?;
    socket.set_read_timeout(Some(Duration::from_millis(500)))?;
    Ok(std::thread::spawn(move || {
        let mut buf = [0u8; 512];
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            let (n, from) = match socket.recv_from(&mut buf) {
                Ok(v) => v,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    continue
                }
                Err(_) => continue,
            };
            if let Some(reply) = answer(&buf[..n], us) {
                let _ = socket.send_to(&reply, from);
            }
        }
    }))
}

#[cfg(test)]
mod server_tests {
    use super::*;

    fn question(name: &str, qtype: u16) -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in name.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        q
    }

    fn answered_ip(reply: &[u8]) -> Option<Ipv4Addr> {
        let n = reply.len();
        if n < 4 {
            return None;
        }
        Some(Ipv4Addr::new(reply[n - 4], reply[n - 3], reply[n - 2], reply[n - 1]))
    }

    const US: Ipv4Addr = Ipv4Addr::new(169, 254, 87, 61);

    /// The whole point: the probe name a Windows machine asks for must come
    /// back pointing at us, because that is what makes the sign-in prompt
    /// appear instead of "no internet".
    #[test]
    fn a_connectivity_probe_is_pointed_at_us() {
        for name in [
            "www.msftconnecttest.com",
            "connectivitycheck.gstatic.com",
            "captive.apple.com",
            "nmcheck.gnome.org",
        ] {
            let r = answer(&question(name, 1), US).expect("must answer");
            assert_eq!(answered_ip(&r), Some(US), "{name} was not pointed at us");
        }
    }

    /// The name a person is told to type.
    #[test]
    fn the_easy_name_resolves() {
        for name in OUR_NAMES {
            let r = answer(&question(name, 1), US).expect("must answer");
            assert_eq!(answered_ip(&r), Some(US));
        }
    }

    #[test]
    fn the_transaction_id_and_question_come_back() {
        let q = question("gorilla", 1);
        let r = answer(&q, US).unwrap();
        assert_eq!(&r[0..2], &q[0..2], "id must be echoed or the client ignores it");
        assert_eq!(r[2] & 0x80, 0x80, "must be marked a response");
        assert_eq!(r[2] & 0x04, 0x04, "must be authoritative");
        assert_eq!(u16::from_be_bytes([r[4], r[5]]), 1, "one question");
        assert_eq!(u16::from_be_bytes([r[6], r[7]]), 1, "one answer");
        assert_eq!(&r[12..12 + (q.len() - 16)], &q[12..q.len() - 4], "question echoed");
    }

    /// AAAA gets an empty success, not a denial. NXDOMAIN would stop the
    /// client asking for the A record it would have accepted, and some
    /// resolvers cache the denial for the whole name.
    #[test]
    fn a_v6_question_is_answered_empty_rather_than_denied() {
        let r = answer(&question("gorilla", 28), US).expect("must still answer");
        assert_eq!(u16::from_be_bytes([r[6], r[7]]), 0, "no answer records");
        assert_eq!(r[3] & 0x0f, 0, "but not an error either");
    }

    #[test]
    fn rubbish_is_not_answered() {
        assert!(answer(&[], US).is_none());
        assert!(answer(&[0u8; 12], US).is_none(), "no question section");
        // A response, not a question: answering it would be a loop.
        let mut resp = question("gorilla", 1);
        resp[2] |= 0x80;
        assert!(answer(&resp, US).is_none());
        // A name whose length byte runs off the end of the packet.
        let mut bad = question("gorilla", 1);
        bad[12] = 200;
        assert!(answer(&bad, US).is_none());
    }

    /// A chaos-class query is not ours, and answering one is how a server ends
    /// up in somebody's fingerprinting write-up.
    #[test]
    fn only_the_internet_class_is_answered() {
        let mut q = question("gorilla", 1);
        let n = q.len();
        q[n - 2] = 0;
        q[n - 1] = 3; // CH
        assert!(answer(&q, US).is_none());
    }
}

// ---------------------------------------------------------------- .local
//
// Answering "gorilla.local" without being anybody's DNS server.
//
// WHY THIS EXISTS, AFTER THE OTHER TWO ALREADY DID.
//
// The plain resolver above only reaches a machine that has been TOLD to ask
// us, and the only way to tell it is DHCP option 6. That is fine on a bare
// cable and refused everywhere else, because handing out addresses on a
// network that has a router is an outage. So on the machine somebody is
// actually testing with, which is on wifi because that is how they talk to
// anyone, the whole naming path switches off and the person on the far end is
// back to copying 169.254.87.61 off one screen onto another. Which is what
// happened, and was reported: "I had to type the whole 169.254.87.61. not
// exactly easy and convenient".
//
// Multicast DNS has none of that. It is link-scoped by design: a query for a
// name ending .local goes to 224.0.0.251 and whoever owns the name answers.
// Nothing has to be configured, no lease has to be accepted, and it is safe on
// a network that has a router because announcing one name is what every
// printer on earth already does. It works with the guard refusing, which is
// the entire point.
//
// Debian resolves .local through avahi or systemd-resolved, macOS through
// Bonjour, Windows 10 and later natively. All three are on by default.

/// The mDNS port and group, fixed by RFC 6762.
pub const MDNS_PORT: u16 = 5353;
const MDNS_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);

/// The names we answer for. Short, because somebody has to say them out loud
/// across a room and then somebody else has to type them.
pub const LOCAL_NAMES: [&str; 2] = ["gorilla.local", "hub.local"];

/// Build the response to an mDNS query, or decide it was not for us.
///
/// Split from the socket so it can be tested without a network.
pub fn mdns_answer(query: &[u8], us: Ipv4Addr) -> Option<Vec<u8>> {
    if query.len() < 13 {
        return None;
    }
    let flags = u16::from_be_bytes([query[2], query[3]]);
    if flags & 0x8000 != 0 {
        return None; // a response, not a question
    }
    let qdcount = u16::from_be_bytes([query[4], query[5]]);
    if qdcount == 0 {
        return None;
    }

    // Walk the questions and answer the first one that is ours. A resolver
    // may pack several into one packet.
    let mut pos = 12;
    for _ in 0..qdcount {
        let (name, next) = read_query_name(query, pos)?;
        if next + 4 > query.len() {
            return None;
        }
        let qtype = u16::from_be_bytes([query[next], query[next + 1]]);
        // The top bit of the class is the "please answer me directly" flag,
        // not part of the class. Masking it off is not optional: without it
        // every query from a modern resolver looks like class 32769 and gets
        // ignored.
        let qclass = u16::from_be_bytes([query[next + 2], query[next + 3]]) & 0x7fff;
        pos = next + 4;

        let ours = LOCAL_NAMES.iter().any(|n| n.eq_ignore_ascii_case(&name));
        if !ours || qclass != 1 || !(qtype == 1 || qtype == 255) {
            continue;
        }

        let mut out = Vec::with_capacity(64);
        // mDNS responses carry id 0: there is no transaction to match, the
        // name in the answer is what identifies it.
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0x8400u16.to_be_bytes()); // response, authoritative
        out.extend_from_slice(&0u16.to_be_bytes()); // no questions echoed
        out.extend_from_slice(&1u16.to_be_bytes()); // one answer
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());

        // The name, written out in full. No compression pointer: there is no
        // question section above to point back into.
        for label in name.split('.') {
            out.push(label.len() as u8);
            out.extend_from_slice(label.as_bytes());
        }
        out.push(0);
        out.extend_from_slice(&1u16.to_be_bytes()); // A
        // Cache-flush bit set with class IN: this is the current answer and
        // replaces anything remembered for this name.
        out.extend_from_slice(&0x8001u16.to_be_bytes());
        // Two minutes. Long enough to be useful across a page of images,
        // short enough that a laptop carried away from this cable is not
        // still holding the answer.
        out.extend_from_slice(&120u32.to_be_bytes());
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&us.octets());
        return Some(out);
    }
    None
}

/// Read a QNAME as dotted text. mDNS queries do not use compression pointers
/// in the question, so this refuses one rather than following it.
fn read_query_name(msg: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let mut parts: Vec<String> = Vec::new();
    loop {
        let len = *msg.get(pos)? as usize;
        if len & 0xc0 == 0xc0 {
            return None;
        }
        pos += 1;
        if len == 0 {
            return Some((parts.join("."), pos));
        }
        let end = pos.checked_add(len)?;
        if end > msg.len() || parts.len() > 8 {
            return None;
        }
        parts.push(String::from_utf8_lossy(&msg[pos..end]).into_owned());
        pos = end;
    }
}

/// A UDP socket on a port somebody else is already using.
///
/// WHY THIS IS NOT `UdpSocket::bind`. Multicast DNS lives on port 5353 and is
/// designed to be shared: every responder on a machine binds the same port
/// with SO_REUSEADDR and they all receive the group traffic. The standard
/// library has no way to set that option before binding, so `bind` loses the
/// port to whoever took it first.
///
/// On a real machine that is a browser. Edge and Chrome both hold 5353 for
/// Chromecast discovery, from startup, on a laptop nobody has configured. So
/// the naming that was supposed to save a person from typing 169.254.87.61
/// silently did not start, on the exact machines it was written for. "Close
/// your browser first" is not an instruction this tool is allowed to give.
///
/// So the socket is made by hand, the option is set, the bind happens, and the
/// result is handed to the standard library to own from then on. This is the
/// same shape as the console handling in term.rs: a few lines of the platform's
/// own API where the portable wrapper cannot express what is needed.
#[cfg(windows)]
fn shared_udp(port: u16) -> std::io::Result<UdpSocket> {
    use std::os::windows::io::FromRawSocket;

    // Winsock has to be started before socket() will work, and the standard
    // library does that the first time it makes a socket of its own. Making
    // and dropping one is the documented way to be sure without linking to
    // WSAStartup ourselves.
    drop(UdpSocket::bind(("0.0.0.0", 0))?);

    #[link(name = "ws2_32")]
    extern "system" {
        fn socket(af: i32, ty: i32, protocol: i32) -> usize;
        fn setsockopt(s: usize, level: i32, name: i32, val: *const u8, len: i32) -> i32;
        fn bind(s: usize, addr: *const u8, len: i32) -> i32;
        fn closesocket(s: usize) -> i32;
    }

    const AF_INET: i32 = 2;
    const SOCK_DGRAM: i32 = 2;
    const SOL_SOCKET: i32 = 0xffff;
    const SO_REUSEADDR: i32 = 0x0004;
    const INVALID_SOCKET: usize = usize::MAX;

    unsafe {
        let s = socket(AF_INET, SOCK_DGRAM, 0);
        if s == INVALID_SOCKET {
            return Err(std::io::Error::last_os_error());
        }
        let yes: i32 = 1;
        if setsockopt(s, SOL_SOCKET, SO_REUSEADDR, (&yes as *const i32).cast(), 4) != 0 {
            closesocket(s);
            return Err(std::io::Error::last_os_error());
        }
        // struct sockaddr_in: family, port (network order), address, padding.
        let mut sa = [0u8; 16];
        sa[0..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
        sa[2..4].copy_from_slice(&port.to_be_bytes());
        // INADDR_ANY, which is the four zero bytes already there.
        if bind(s, sa.as_ptr(), 16) != 0 {
            let e = std::io::Error::last_os_error();
            closesocket(s);
            return Err(e);
        }
        Ok(UdpSocket::from_raw_socket(s as std::os::windows::io::RawSocket))
    }
}

#[cfg(unix)]
fn shared_udp(port: u16) -> std::io::Result<UdpSocket> {
    use std::os::unix::io::FromRawFd;

    extern "C" {
        fn socket(domain: i32, ty: i32, protocol: i32) -> i32;
        fn setsockopt(fd: i32, level: i32, name: i32, val: *const u8, len: u32) -> i32;
        fn bind(fd: i32, addr: *const u8, len: u32) -> i32;
        fn close(fd: i32) -> i32;
    }

    const AF_INET: i32 = 2;
    const SOCK_DGRAM: i32 = 2;
    const SOL_SOCKET: i32 = 1;
    // Linux numbers. The BSDs and macOS use 0x0004 / 0x0200, which is why the
    // return value is checked and a failure is not fatal: the bind is then
    // tried anyway and only that failing is reported.
    #[cfg(target_os = "linux")]
    const SO_REUSEADDR: i32 = 2;
    #[cfg(target_os = "linux")]
    const SO_REUSEPORT: i32 = 15;
    #[cfg(not(target_os = "linux"))]
    const SO_REUSEADDR: i32 = 0x0004;
    #[cfg(not(target_os = "linux"))]
    const SO_REUSEPORT: i32 = 0x0200;

    unsafe {
        let fd = socket(AF_INET, SOCK_DGRAM, 0);
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let yes: i32 = 1;
        let p: *const u8 = (&yes as *const i32).cast();
        // Both, because avahi holds 5353 with both on Linux and a socket
        // without SO_REUSEPORT cannot join it there.
        setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, p, 4);
        setsockopt(fd, SOL_SOCKET, SO_REUSEPORT, p, 4);

        let mut sa = [0u8; 16];
        sa[0] = 16; // sin_len on the BSDs, ignored on Linux
        sa[1] = AF_INET as u8;
        sa[2..4].copy_from_slice(&port.to_be_bytes());
        #[cfg(target_os = "linux")]
        {
            // Linux has no sin_len: the family occupies both bytes.
            sa[0..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
        }
        if bind(fd, sa.as_ptr(), 16) != 0 {
            let e = std::io::Error::last_os_error();
            close(fd);
            return Err(e);
        }
        Ok(UdpSocket::from_raw_fd(fd))
    }
}

#[cfg(not(any(windows, unix)))]
fn shared_udp(port: u16) -> std::io::Result<UdpSocket> {
    UdpSocket::bind(("0.0.0.0", port))
}

/// Answer .local for this machine until told to stop.
///
/// Unlike the plain resolver this needs no guard. It answers for two names it
/// owns, on the local link only, which is what mDNS is for.
pub fn start_mdns(
    us: Ipv4Addr,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    // Shared, not exclusive: a browser is very likely already here.
    let socket = shared_udp(MDNS_PORT)?;
    // Joining on OUR interface rather than letting the system choose, so the
    // answer goes out of the cable and not out of the wifi.
    socket.join_multicast_v4(&MDNS_GROUP, &us)?;
    // Loopback ON. It costs nothing in normal use, where the querier is
    // another machine, and it is the difference between being testable and
    // not: with it off, a query sent from this same computer never reaches
    // this responder and the whole path looks dead when it is fine.
    let _ = socket.set_multicast_loop_v4(true);
    socket.set_read_timeout(Some(Duration::from_millis(500)))?;

    let to_group = SocketAddr::from((MDNS_GROUP, MDNS_PORT));
    Ok(std::thread::spawn(move || {
        let mut buf = [0u8; 1500];
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            let (n, from) = match socket.recv_from(&mut buf) {
                Ok(v) => v,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    continue
                }
                Err(_) => continue,
            };
            if let Some(reply) = mdns_answer(&buf[..n], us) {
                // Both ways. The group is what RFC 6762 asks for; the direct
                // reply is what gets through when a resolver has asked for one
                // and is not listening to the group at that moment.
                let _ = socket.send_to(&reply, to_group);
                let _ = socket.send_to(&reply, from);
            }
        }
    }))
}

#[cfg(test)]
mod mdns_tests {
    use super::*;

    fn query(name: &str, qtype: u16, unicast_bit: bool) -> Vec<u8> {
        let mut q = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in name.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        let class: u16 = if unicast_bit { 0x8001 } else { 1 };
        q.extend_from_slice(&class.to_be_bytes());
        q
    }

    const US: Ipv4Addr = Ipv4Addr::new(169, 254, 87, 61);

    fn answered_ip(r: &[u8]) -> Ipv4Addr {
        let n = r.len();
        Ipv4Addr::new(r[n - 4], r[n - 3], r[n - 2], r[n - 1])
    }

    #[test]
    fn the_name_a_person_types_is_answered() {
        for n in LOCAL_NAMES {
            let r = mdns_answer(&query(n, 1, false), US).expect("must answer");
            assert_eq!(answered_ip(&r), US, "{n}");
            assert_eq!(r[2] & 0x84, 0x84, "response and authoritative");
        }
    }

    /// The bit that breaks every naive implementation: a resolver asking for a
    /// direct reply sets the top bit of the class, so the class reads 32769
    /// instead of 1 and a strict comparison ignores the whole query.
    #[test]
    fn the_unicast_request_bit_does_not_hide_the_class() {
        let r = mdns_answer(&query("gorilla.local", 1, true), US);
        assert!(r.is_some(), "a query asking for a direct reply is still a query");
    }

    #[test]
    fn somebody_elses_name_is_left_alone() {
        assert!(mdns_answer(&query("printer.local", 1, false), US).is_none());
        assert!(mdns_answer(&query("gorilla.example.com", 1, false), US).is_none());
    }

    #[test]
    fn case_does_not_matter() {
        assert!(mdns_answer(&query("GORILLA.local", 1, false), US).is_some());
    }

    #[test]
    fn a_response_is_never_answered() {
        let mut r = query("gorilla.local", 1, false);
        r[2] |= 0x80;
        assert!(mdns_answer(&r, US).is_none(), "answering a response is a packet storm");
    }

    #[test]
    fn rubbish_is_ignored() {
        assert!(mdns_answer(&[], US).is_none());
        assert!(mdns_answer(&[0u8; 12], US).is_none());
        let mut bad = query("gorilla.local", 1, false);
        bad[12] = 200; // a label longer than the packet
        assert!(mdns_answer(&bad, US).is_none());
    }

    /// Several questions in one packet, ours not first.
    #[test]
    fn ours_is_found_among_other_questions() {
        let mut q = vec![0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0];
        for name in ["printer.local", "gorilla.local"] {
            for label in name.split('.') {
                q.push(label.len() as u8);
                q.extend_from_slice(label.as_bytes());
            }
            q.push(0);
            q.extend_from_slice(&1u16.to_be_bytes());
            q.extend_from_slice(&1u16.to_be_bytes());
        }
        assert!(mdns_answer(&q, US).is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::UdpSocket;

    /// A real PTR response, byte for byte, including the compression pointer
    /// that every server uses and that a naive parser reads as a label length.
    fn synthetic_response(id: u16, name: &str) -> Vec<u8> {
        let mut m = Vec::new();
        m.extend_from_slice(&id.to_be_bytes());
        m.extend_from_slice(&0x8180u16.to_be_bytes()); // response, no error
        m.extend_from_slice(&1u16.to_be_bytes());      // questions
        m.extend_from_slice(&1u16.to_be_bytes());      // answers
        m.extend_from_slice(&[0, 0, 0, 0]);
        // question: 90.0.42.10.in-addr.arpa PTR IN
        for label in ["90", "0", "42", "10", "in-addr", "arpa"] {
            m.push(label.len() as u8);
            m.extend_from_slice(label.as_bytes());
        }
        m.push(0);
        m.extend_from_slice(&12u16.to_be_bytes());
        m.extend_from_slice(&1u16.to_be_bytes());
        // answer: pointer back to the question's name, then the PTR record
        m.extend_from_slice(&[0xc0, 0x0c]);
        m.extend_from_slice(&12u16.to_be_bytes());
        m.extend_from_slice(&1u16.to_be_bytes());
        m.extend_from_slice(&60u32.to_be_bytes());
        let mut rdata = Vec::new();
        for label in name.split('.') {
            rdata.push(label.len() as u8);
            rdata.extend_from_slice(label.as_bytes());
        }
        rdata.push(0);
        m.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        m.extend_from_slice(&rdata);
        m
    }

    #[test]
    fn reads_the_name_out_of_a_ptr_response() {
        let msg = synthetic_response(0x1234, "Xiaomi-11-Lite-5G-NE.lan");
        assert_eq!(parse_ptr(&msg).as_deref(), Some("Xiaomi-11-Lite-5G-NE.lan"));
        assert_eq!(short_name("Xiaomi-11-Lite-5G-NE.lan"), "Xiaomi-11-Lite-5G-NE");
    }

    /// Non-vacuous: a response that says "no such name" must not produce one.
    #[test]
    fn nxdomain_is_not_a_name() {
        let mut msg = synthetic_response(0x1234, "nope.lan");
        msg[3] |= 3; // NXDOMAIN
        assert_eq!(parse_ptr(&msg), None);
    }

    /// A name pointing at itself is the classic way to hang a DNS parser. This
    /// must return, not loop, because it runs inside a program drawing a screen.
    #[test]
    fn a_self_referential_pointer_does_not_hang() {
        let mut msg = synthetic_response(0x1234, "loop.lan");
        // Point the answer's RDATA at itself rather than back at the question.
        let rdata_start = msg.len() - 10;
        msg[rdata_start] = 0xc0;
        msg[rdata_start + 1] = rdata_start as u8;
        assert_eq!(parse_ptr(&msg), None, "a forward or self pointer must be refused");
    }

    /// The whole round trip against a real socket, which is the only way to
    /// know the QUERY bytes are right and not just the parser.
    #[test]
    fn asks_a_real_server_and_gets_the_name_back() {
        let server = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = server.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut buf = [0u8; 512];
            let (n, from) = server.recv_from(&mut buf).unwrap();
            // Echo the caller's transaction id back, as a real server must.
            let id = u16::from_be_bytes([buf[0], buf[1]]);
            // Prove the question really asked for the reverse of 10.42.0.90.
            let asked = String::from_utf8_lossy(&buf[..n]).replace(|c: char| !c.is_ascii_graphic(), ".");
            assert!(asked.contains("in-addr"), "query did not ask a reverse question: {asked}");
            let reply = synthetic_response(id, "Xiaomi-11-Lite-5G-NE.lan");
            server.send_to(&reply, from).unwrap();
        });
        let got = reverse_lookup_at(Ipv4Addr::new(10, 42, 0, 90), addr, Duration::from_secs(3));
        assert_eq!(got.as_deref(), Some("Xiaomi-11-Lite-5G-NE.lan"));
    }

    /// Non-vacuous partner to the one above: a server that says nothing must
    /// produce nothing, within the timeout WE set rather than the system's.
    #[test]
    fn a_silent_server_times_out_quickly_and_returns_nothing() {
        let server = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = server.local_addr().unwrap();
        // Held open and deliberately never answered.
        let t0 = std::time::Instant::now();
        let got = reverse_lookup_at(Ipv4Addr::new(10, 42, 0, 90), addr, Duration::from_millis(300));
        let waited = t0.elapsed();
        assert_eq!(got, None);
        assert!(waited < Duration::from_secs(2), "waited {waited:?}, which would freeze the screen");
        drop(server);
    }
}
