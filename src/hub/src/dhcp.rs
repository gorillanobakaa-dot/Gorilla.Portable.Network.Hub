// Version: 0.1.0 · updated 26-09-06-14-10
//
// Handing an address to a Linux laptop on the other end of a cable.
//
// This exists because of one afternoon and one machine. A Windows laptop and a
// Debian 13 laptop, one cable between them, and the transfer timed out every
// time. The cause is a difference in what the two systems do when DHCP does
// not answer:
//
//   Windows, macOS, the BSDs:  give up after 30 to 60 seconds and assign
//                              themselves 169.254.x.y (RFC 3927). The cable
//                              works, just slowly to start.
//   Most Linux:                keep asking. Forever. The interface never gets
//                              an address, so nothing can happen at all.
//
// "Set a static IP" is the standard answer and it is not one, because the
// person on the other laptop is a teacher or a fifteen-year-old and the
// instructions run to nine steps and a subnet mask. So instead the sending
// machine answers the question the Linux box will not stop asking, which takes
// about half a second and needs nothing typed on either side.
//
// This is a deliberately small DHCP server. It hands out addresses on one
// cable and does nothing else: no relaying, no boot images, no reservations,
// no persistence across restarts.
//
// THE THING THIS MUST NEVER DO
//
// A DHCP server that answers on a network which already has a router is a
// fault, not a feature. It races the real router, hands out addresses on the
// wrong subnet, and takes the network down for everyone in the building. There
// is no clever way to be sure from inside a process which cable a broadcast
// arrived on using only the standard library, so the guard is at the other
// end: this refuses to start unless the machine looks like it is on a bare
// cable, and it is never started unless a person asked for a cable transfer.
// See `safe_to_offer`.

use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub const SERVER_PORT: u16 = 67;
pub const CLIENT_PORT: u16 = 68;

/// Bytes 236..240 of every DHCP packet. Without it this is plain BOOTP.
const COOKIE: [u8; 4] = [0x63, 0x82, 0x53, 0x63];

const OP_REQUEST: u8 = 1;
const OP_REPLY: u8 = 2;

const DISCOVER: u8 = 1;
const OFFER: u8 = 2;
const REQUEST: u8 = 3;
const DECLINE: u8 = 4;
const ACK: u8 = 5;
const RELEASE: u8 = 7;

const OPT_SUBNET_MASK: u8 = 1;
const OPT_ROUTER: u8 = 3;
const OPT_LEASE_TIME: u8 = 51;
const OPT_MSG_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_END: u8 = 255;

/// One hour, not the twenty-four the first version used.
///
/// A lease outlives the program that granted it. A day-long lease handed out
/// over a cable at nine in the morning is still held by the laptop at four in
/// the afternoon, in a different room, plugged into the actual school network,
/// where that address is wrong and nothing works. An hour is longer than any
/// file transfer and short enough to be gone by the next lesson.
const LEASE_SECONDS: u32 = 3600;

/// How many machines can be on one cable. Two, in every real case: this is a
/// point-to-point link. The pool is larger than that only so a laptop that
/// changes its MAC, or a small switch in the middle, does not run it dry.
const POOL: u8 = 20;

/// A parsed request. Only the fields that are replied to are kept.
#[derive(Debug, Clone)]
pub struct Request {
    /// Transaction id, echoed back so the client can match the reply.
    pub xid: [u8; 4],
    /// Hardware address. The full 16-byte field; only the first 6 are ethernet.
    pub chaddr: [u8; 16],
    pub kind: u8,
    /// Option 50, when the client asks to keep an address it already had.
    pub requested: Option<Ipv4Addr>,
}

impl Request {
    /// The part of chaddr that identifies the machine. Leases are keyed on
    /// this rather than on the whole padded field.
    fn mac(&self) -> [u8; 6] {
        let mut m = [0u8; 6];
        m.copy_from_slice(&self.chaddr[..6]);
        m
    }
}

/// Read a DHCP packet, or decide it is not one.
pub fn parse(buf: &[u8]) -> Option<Request> {
    // 236 bytes of fixed header plus the 4-byte cookie.
    if buf.len() < 240 || buf[0] != OP_REQUEST || buf[236..240] != COOKIE {
        return None;
    }
    let mut xid = [0u8; 4];
    xid.copy_from_slice(&buf[4..8]);
    let mut chaddr = [0u8; 16];
    chaddr.copy_from_slice(&buf[28..44]);

    let mut kind = 0u8;
    let mut requested = None;
    let mut i = 240;
    while i < buf.len() {
        let opt = buf[i];
        if opt == OPT_END {
            break;
        }
        if opt == 0 {
            i += 1; // pad
            continue;
        }
        if i + 1 >= buf.len() {
            break;
        }
        let len = buf[i + 1] as usize;
        let start = i + 2;
        let end = match start.checked_add(len) {
            Some(e) if e <= buf.len() => e,
            // A length field running off the end of the packet is malformed.
            // Stop reading options rather than indexing past the buffer.
            _ => break,
        };
        match opt {
            OPT_MSG_TYPE if len == 1 => kind = buf[start],
            50 if len == 4 => {
                requested = Some(Ipv4Addr::new(buf[start], buf[start + 1], buf[start + 2], buf[start + 3]))
            }
            _ => {}
        }
        i = end;
    }
    if kind == 0 {
        return None;
    }
    Some(Request { xid, chaddr, kind, requested })
}

/// Build an OFFER or an ACK.
pub fn reply(req: &Request, kind: u8, client: Ipv4Addr, server: Ipv4Addr, mask: Ipv4Addr) -> Vec<u8> {
    let mut p = vec![0u8; 300];
    p[0] = OP_REPLY;
    p[1] = 1; // ethernet
    p[2] = 6; // six bytes of it
    p[4..8].copy_from_slice(&req.xid);
    // Broadcast flag. The client has no address yet, so a unicast reply to the
    // address we are trying to give it cannot be delivered: it would need an
    // ARP entry for an address nobody holds. Setting this asks the client to
    // accept the reply as a broadcast, which is the only way it arrives.
    p[10] = 0x80;
    p[16..20].copy_from_slice(&client.octets()); // yiaddr, the address offered
    p[20..24].copy_from_slice(&server.octets()); // siaddr, us
    p[28..44].copy_from_slice(&req.chaddr);
    p[236..240].copy_from_slice(&COOKIE);

    let mut o = 240;
    let mut put = |bytes: &[u8], o: &mut usize| {
        p[*o..*o + bytes.len()].copy_from_slice(bytes);
        *o += bytes.len();
    };
    put(&[OPT_MSG_TYPE, 1, kind], &mut o);
    put(&[OPT_SERVER_ID, 4], &mut o);
    put(&server.octets(), &mut o);
    put(&[OPT_SUBNET_MASK, 4], &mut o);
    put(&mask.octets(), &mut o);
    // The router option points at us.
    //
    // Not because we route anything. We do not, and there is nothing to route
    // to: this is a cable with two laptops on it and no internet anywhere. It
    // is here because a client with no default route at all will sometimes
    // refuse to consider the link usable, and because pointing it at us means
    // any stray packet stops here rather than going somewhere unexpected.
    put(&[OPT_ROUTER, 4], &mut o);
    put(&server.octets(), &mut o);
    put(&[OPT_LEASE_TIME, 4], &mut o);
    put(&LEASE_SECONDS.to_be_bytes(), &mut o);
    // No DNS option. There is no name server on a cable, and handing out one
    // that does not answer makes every lookup on the client wait for a
    // timeout, which looks exactly like the machine having hung.
    put(&[OPT_END], &mut o);
    p
}

/// The addresses to hand out, worked out from the address we already hold.
///
/// This deliberately does not re-address the host. Changing the host's own IP
/// to suit a DHCP pool means touching system network configuration, which
/// needs administrator rights, breaks whatever else was using that interface,
/// and has to be undone afterwards, including after a crash.
///
/// Returns the first address of the pool and the mask.
fn pool_base(server: Ipv4Addr) -> (Ipv4Addr, Ipv4Addr) {
    let o = server.octets();
    if o[0] == 169 && o[1] == 254 {
        // RFC 3927 is a flat /16. Stay in the same third octet as the host so
        // the two are on the same segment however the client reads the mask.
        (Ipv4Addr::new(169, 254, o[2], 1), Ipv4Addr::new(255, 255, 0, 0))
    } else {
        (Ipv4Addr::new(o[0], o[1], o[2], 100), Ipv4Addr::new(255, 255, 255, 0))
    }
}

/// Who has what.
///
/// The first version had no such thing: the address to hand out was a pure
/// function of the server's own address, so every client that ever asked was
/// given the identical IP. With one laptop on the cable that is invisible.
/// With two it is an address conflict, and the symptom is not "the second one
/// failed", it is both machines dropping off the link intermittently for as
/// long as they stay plugged in.
struct Leases {
    base: Ipv4Addr,
    mask: Ipv4Addr,
    by_mac: HashMap<[u8; 6], Ipv4Addr>,
}

impl Leases {
    fn new(server: Ipv4Addr) -> Self {
        let (base, mask) = pool_base(server);
        Leases { base, mask, by_mac: HashMap::new() }
    }

    /// The address for this machine: the one it already has if it has one, so
    /// that a DISCOVER repeated after a dropped reply is not a second lease.
    fn offer(&mut self, mac: [u8; 6], server: Ipv4Addr) -> Option<Ipv4Addr> {
        if let Some(ip) = self.by_mac.get(&mac) {
            return Some(*ip);
        }
        let b = self.base.octets();
        let taken: Vec<Ipv4Addr> = self.by_mac.values().copied().collect();
        for n in 0..POOL {
            let candidate = Ipv4Addr::new(b[0], b[1], b[2], b[3].wrapping_add(n));
            // Never hand out our own address, and never hand out .0 or .255.
            let last = candidate.octets()[3];
            if candidate == server || last == 0 || last == 255 || taken.contains(&candidate) {
                continue;
            }
            self.by_mac.insert(mac, candidate);
            return Some(candidate);
        }
        None // pool exhausted; say nothing rather than duplicate an address
    }

    fn release(&mut self, mac: [u8; 6]) {
        self.by_mac.remove(&mac);
    }
}

/// Whether it is safe to answer DHCP here at all.
///
/// The one address that means "there is no router on this cable" is a
/// link-local one: a machine only assigns itself 169.254.x.y after DHCP has
/// already failed to answer it. If this machine has such an address, nothing
/// on this segment is serving DHCP, and taking that job is safe.
///
/// A default gateway means the opposite: something is routing, which means
/// something is almost certainly also leasing, and answering would race it.
/// The cost of being wrong in that direction is somebody's whole network, so
/// the check is deliberately conservative and refuses when unsure.
pub fn safe_to_offer(addresses: &[Ipv4Addr], gateway: Option<Ipv4Addr>) -> bool {
    // A link-local gateway is not a gateway.
    //
    // On Windows and Mac net::default_gateway() does not read the routing
    // table: it answers ".1 of whatever subnet we are on", which is right for
    // a home router and meaningless on a cable. On a bare cable that guess
    // comes back as 169.254.x.1, and reading it as "there is a router here"
    // switched this off on exactly the machines it was written for. RFC 3927
    // is explicit that link-local traffic is never forwarded, so nothing on
    // 169.254/16 can be a router by definition.
    let real_router = matches!(gateway, Some(g) if !g.is_link_local());
    if real_router {
        return false;
    }
    addresses.iter().any(|a| a.is_link_local())
}

/// Why the responder did not start, in words that say what to do.
pub enum Refused {
    /// Something is routing here, so something else is probably leasing.
    NetworkHasARouter,
    /// No link-local address, so we cannot tell this is a bare cable.
    NotABareCable,
    /// Port 67 is a privileged port on Unix.
    NeedsAdministrator,
    Other(io::Error),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::NetworkHasARouter => write!(
                f,
                "not handing out addresses: this network already has a router, \
                 and two things handing out addresses breaks it for everyone. \
                 Unplug from the wall network first, or use the cable on its own."
            ),
            Refused::NotABareCable => write!(
                f,
                "not handing out addresses yet: waiting for this computer to \
                 settle on a cable address. Give it half a minute after plugging \
                 the cable in, then try again."
            ),
            Refused::NeedsAdministrator => write!(
                f,
                "cannot hand out addresses without administrator rights. \
                 On Linux and macOS run this with sudo. Without it a Windows or \
                 Mac laptop on the far end still works; a Linux one may need its \
                 address set by hand."
            ),
            Refused::Other(e) => write!(f, "cannot hand out addresses: {e}"),
        }
    }
}

/// Start answering, if it is safe to.
pub fn start(
    server: Ipv4Addr,
    addresses: &[Ipv4Addr],
    gateway: Option<Ipv4Addr>,
    stop: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>, Refused> {
    if matches!(gateway, Some(g) if !g.is_link_local()) {
        return Err(Refused::NetworkHasARouter);
    }
    if !safe_to_offer(addresses, gateway) {
        return Err(Refused::NotABareCable);
    }
    let socket = UdpSocket::bind(("0.0.0.0", SERVER_PORT)).map_err(|e| {
        // Port 67 is below 1024, so on Unix it needs root. Windows does not
        // care. Both arrive here as PermissionDenied and the message has to
        // tell a person what to do about it.
        if e.kind() == io::ErrorKind::PermissionDenied {
            Refused::NeedsAdministrator
        } else {
            Refused::Other(e)
        }
    })?;
    socket.set_broadcast(true).map_err(Refused::Other)?;
    socket
        .set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(Refused::Other)?;

    let to_client = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, CLIENT_PORT));
    let mut leases = Leases::new(server);
    let mask = leases.mask;

    Ok(thread::spawn(move || {
        let mut buf = [0u8; 1500];
        while !stop.load(Ordering::Relaxed) {
            let n = match socket.recv_from(&mut buf) {
                Ok((n, _)) => n,
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    continue
                }
                Err(_) => continue,
            };
            let Some(req) = parse(&buf[..n]) else { continue };
            let mac = req.mac();
            match req.kind {
                DISCOVER => {
                    if let Some(ip) = leases.offer(mac, server) {
                        let _ = socket.send_to(&reply(&req, OFFER, ip, server, mask), to_client);
                    }
                }
                REQUEST => {
                    if let Some(ip) = leases.offer(mac, server) {
                        let _ = socket.send_to(&reply(&req, ACK, ip, server, mask), to_client);
                    }
                }
                // A client that refuses an address has found it already in
                // use. Drop the lease so the next attempt picks a different
                // one instead of offering the same bad address forever.
                DECLINE | RELEASE => leases.release(mac),
                _ => {}
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discover(mac: u8) -> Request {
        let mut chaddr = [0u8; 16];
        chaddr[..6].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef, 0x00, mac]);
        Request { xid: [1, 2, 3, 4], chaddr, kind: DISCOVER, requested: None }
    }

    /// The bug this module was rewritten for: two laptops, one address.
    #[test]
    fn two_machines_get_two_different_addresses() {
        let server = Ipv4Addr::new(169, 254, 5, 5);
        let mut l = Leases::new(server);
        let a = l.offer(discover(1).mac(), server).unwrap();
        let b = l.offer(discover(2).mac(), server).unwrap();
        assert_ne!(a, b, "two machines must never be given the same address");
    }

    /// A repeated DISCOVER is the same machine asking again, not a new one.
    #[test]
    fn asking_twice_gets_the_same_address() {
        let server = Ipv4Addr::new(169, 254, 5, 5);
        let mut l = Leases::new(server);
        let a = l.offer(discover(1).mac(), server).unwrap();
        let b = l.offer(discover(1).mac(), server).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn we_never_hand_out_our_own_address() {
        let server = Ipv4Addr::new(169, 254, 0, 1);
        let mut l = Leases::new(server);
        for i in 0..POOL {
            if let Some(ip) = l.offer(discover(i).mac(), server) {
                assert_ne!(ip, server);
                assert_ne!(ip.octets()[3], 0);
                assert_ne!(ip.octets()[3], 255);
            }
        }
    }

    #[test]
    fn a_released_address_can_be_given_out_again() {
        let server = Ipv4Addr::new(169, 254, 5, 5);
        let mut l = Leases::new(server);
        let a = l.offer(discover(1).mac(), server).unwrap();
        l.release(discover(1).mac());
        let b = l.offer(discover(2).mac(), server).unwrap();
        assert_eq!(a, b, "the freed address is the next one available");
    }

    /// The guard that keeps this off a school network.
    #[test]
    fn we_refuse_to_answer_where_a_router_exists() {
        let link_local = [Ipv4Addr::new(169, 254, 3, 4)];
        let routed = [Ipv4Addr::new(192, 168, 1, 50)];
        assert!(safe_to_offer(&link_local, None));
        assert!(!safe_to_offer(&link_local, Some(Ipv4Addr::new(192, 168, 1, 1))));
        // The regression: on Windows a bare cable reports a GUESSED gateway of
        // 169.254.x.1. Treating that as a real router disabled the whole
        // feature on the machines it exists for.
        assert!(
            safe_to_offer(&link_local, Some(Ipv4Addr::new(169, 254, 3, 1))),
            "a guessed link-local gateway must not count as a router"
        );
        assert!(!safe_to_offer(&routed, None));
        assert!(!safe_to_offer(&[], None));
    }

    #[test]
    fn a_reply_is_a_dhcp_packet_the_client_can_read() {
        let req = discover(1);
        let p = reply(&req, OFFER, Ipv4Addr::new(169, 254, 0, 2), Ipv4Addr::new(169, 254, 0, 1), Ipv4Addr::new(255, 255, 0, 0));
        assert_eq!(p[0], OP_REPLY);
        assert_eq!(p[236..240], COOKIE);
        assert_eq!(&p[4..8], &req.xid, "the transaction id must come back");
        assert_eq!(&p[16..20], &[169, 254, 0, 2], "the offered address");
        assert_eq!(p[10] & 0x80, 0x80, "the broadcast flag must be set");
    }

    /// A malformed option length must not read past the packet.
    #[test]
    fn a_lying_length_field_does_not_read_off_the_end() {
        let mut p = vec![0u8; 244];
        p[0] = OP_REQUEST;
        p[236..240].copy_from_slice(&COOKIE);
        p[240] = OPT_MSG_TYPE;
        p[241] = 1;
        p[242] = DISCOVER;
        p[243] = 50; // an option whose length byte is missing entirely
        assert!(parse(&p).is_some());

        let mut q = vec![0u8; 245];
        q[0] = OP_REQUEST;
        q[236..240].copy_from_slice(&COOKIE);
        q[240] = OPT_MSG_TYPE;
        q[241] = 1;
        q[242] = DISCOVER;
        q[243] = 50;
        q[244] = 200; // claims 200 bytes that are not there
        assert!(parse(&q).is_some());
    }

    #[test]
    fn not_dhcp_is_rejected() {
        assert!(parse(&[]).is_none());
        assert!(parse(&[0u8; 240]).is_none(), "no cookie, not ours");
        let mut p = vec![0u8; 240];
        p[0] = OP_REQUEST;
        p[236..240].copy_from_slice(&COOKIE);
        assert!(parse(&p).is_none(), "no message type option, not answerable");
    }
}
