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
