// Version: 0.1.0 · updated 26-09-06-14-10
//
// Two laptops, one cable, no room in between.
//
// Wifi is the wrong answer often enough to need this one. A staffroom at four
// o'clock has thirty phones and a microwave sharing the same 2.4 GHz; a school
// in a valley has a wifi card that died in 2019 and no budget to replace it; a
// 30 GB folder of video over 802.11n takes most of an afternoon. A CAT cable
// between two laptops has none of those problems and costs about two pounds.
//
// What it does not have is a router. No router means no DHCP, and no DHCP
// normally means no addresses and no way for either machine to find the other.
// Two things fix that, and they are separate:
//
//   - Addresses. Windows, macOS and the BSDs give up waiting for DHCP after
//     30 to 60 seconds and assign themselves 169.254.x.y (RFC 3927). Linux
//     mostly does not, which is what dhcp.rs is for.
//   - Finding. That is this file. The sending machine says "I am here" into
//     the broadcast address once a second, and the receiving machine listens.
//     Nobody types an IP address, because nobody should have to.
//
// The beacon says the folder's name out loud to everything on the network
// segment, so it is only ever started for a deliberate cable transfer. On a
// shared wifi that would be telling the whole building what is being handed
// out, which is not ours to announce.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// The port the beacon speaks on. Nothing else in common use wants it.
pub const BEACON_PORT: u16 = 42424;

/// First field of every beacon. A version number is in it because the day
/// there is a v2 beacon, a v1 receiver has to ignore it rather than parse it
/// wrongly and connect to something it does not understand.
pub const MAGIC: &str = "GORILLA_CABLE_HUB_V1";

/// How often to say it. Once a second is fast enough that plugging the cable
/// in feels instant and slow enough to be invisible on the wire.
const EVERY: Duration = Duration::from_secs(1);

/// A machine that answered, and where to get the files from it.
#[derive(Debug, Clone, PartialEq)]
pub struct Peer {
    pub ip: Ipv4Addr,
    pub port: u16,
    /// What the sender calls itself.
    pub name: String,
    /// The folder being handed out, so the receiver can say what it found
    /// before anything is downloaded.
    pub folder: String,
}

/// Build the packet.
///
/// Fields are separated by tabs, not spaces.
///
/// The first version of this replaced spaces with underscores on the way out
/// and underscores with spaces on the way back, which works until somebody
/// serves a folder called `Year_7_Maths` and the other laptop reports finding
/// `Year 7 Maths`. Names belonging to other people are not ours to rewrite.
/// A tab cannot occur in a Windows or Linux folder name in practice, and any
/// that somehow appears is turned into a space here so it cannot forge a
/// field boundary.
pub fn beacon(port: u16, name: &str, folder: &str) -> Vec<u8> {
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    format!("{MAGIC}\t{port}\t{}\t{}", clean(name), clean(folder)).into_bytes()
}

/// Read a packet, or decide it was not one of ours.
///
/// The address comes from the UDP header rather than the payload. A sender
/// cannot then claim to be at an address it is not at, and there is no reason
/// to trust the body about something the network already knows.
pub fn read_beacon(data: &[u8], from: SocketAddr) -> Option<Peer> {
    let ip = match from {
        SocketAddr::V4(v4) => *v4.ip(),
        // No IPv6: link-local cable transfer is an IPv4 arrangement.
        SocketAddr::V6(_) => return None,
    };
    let text = std::str::from_utf8(data).ok()?;
    let mut parts = text.split('\t');
    if parts.next()? != MAGIC {
        return None;
    }
    let port: u16 = parts.next()?.parse().ok()?;
    if port == 0 {
        return None;
    }
    let name = parts.next()?.trim().to_string();
    let folder = parts.next()?.trim().to_string();
    Some(Peer { ip, port, name, folder })
}

/// Say "I am here" once a second until told to stop.
///
/// Both broadcast addresses are used. 255.255.255.255 is the limited broadcast
/// and is what reaches a machine that has not got an address yet;
/// 169.254.255.255 is the link-local one and is what reaches a machine that
/// has. Which of the two arrives depends on how far through the process the
/// other laptop is, so both go out and the receiver takes whichever it sees.
pub fn start_beacon(
    port: u16,
    name: String,
    folder: String,
    stop: Arc<AtomicBool>,
) -> io::Result<thread::JoinHandle<()>> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_broadcast(true)?;
    let payload = beacon(port, &name, &folder);
    let targets = [
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(255, 255, 255, 255), BEACON_PORT)),
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(169, 254, 255, 255), BEACON_PORT)),
    ];
    Ok(thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            for t in &targets {
                // A send that fails is normal: 169.254.255.255 is
                // unreachable until the interface has a link-local address,
                // which is exactly the window this is trying to cover.
                let _ = socket.send_to(&payload, t);
            }
            thread::sleep(EVERY);
        }
    }))
}

/// Listen until a sender is found or `patience` runs out.
///
/// The socket is woken every half second rather than blocked on for the whole
/// timeout, so that a person who has changed their mind gets their terminal
/// back in half a second instead of two minutes.
pub fn find_peer(patience: Duration) -> io::Result<Option<Peer>> {
    let socket = UdpSocket::bind(("0.0.0.0", BEACON_PORT))?;
    socket.set_read_timeout(Some(Duration::from_millis(500)))?;
    let start = Instant::now();
    let mut buf = [0u8; 1024];
    while start.elapsed() < patience {
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                if let Some(p) = read_beacon(&buf[..n], from) {
                    return Ok(Some(p));
                }
                // Anything else on the port is somebody else's traffic. Keep
                // listening rather than failing.
            }
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut => {}
            Err(e) => return Err(e),
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from(ip: [u8; 4]) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from(ip), BEACON_PORT))
    }

    #[test]
    fn a_beacon_survives_the_round_trip() {
        let p = read_beacon(&beacon(8080, "Front Desk", "Grade 6 Science"), from([169, 254, 12, 34]))
            .expect("our own beacon must parse");
        assert_eq!(p.ip, Ipv4Addr::new(169, 254, 12, 34));
        assert_eq!(p.port, 8080);
        assert_eq!(p.name, "Front Desk");
        assert_eq!(p.folder, "Grade 6 Science");
    }

    /// The bug this format exists to avoid: underscores are part of the name,
    /// not an encoding of a space.
    #[test]
    fn underscores_in_a_folder_name_are_left_alone() {
        let p = read_beacon(&beacon(8080, "laptop_2", "Year_7_Maths"), from([169, 254, 1, 1]))
            .expect("must parse");
        assert_eq!(p.name, "laptop_2");
        assert_eq!(p.folder, "Year_7_Maths");
    }

    /// A tab in a name must not be able to invent an extra field.
    #[test]
    fn a_tab_in_a_name_cannot_forge_a_field() {
        let p = read_beacon(&beacon(8080, "a\tb", "c"), from([169, 254, 1, 1])).expect("must parse");
        assert_eq!(p.name, "a b");
        assert_eq!(p.folder, "c");
    }

    #[test]
    fn other_traffic_on_the_port_is_ignored() {
        assert!(read_beacon(b"", from([169, 254, 1, 1])).is_none());
        assert!(read_beacon(b"hello", from([169, 254, 1, 1])).is_none());
        assert!(read_beacon(b"GORILLA_CABLE_HUB_V2\t8080\ta\tb", from([169, 254, 1, 1])).is_none());
        // A truncated beacon is not half-accepted.
        assert!(read_beacon(b"GORILLA_CABLE_HUB_V1\t8080", from([169, 254, 1, 1])).is_none());
        // Port 0 is not somewhere you can connect to.
        assert!(read_beacon(b"GORILLA_CABLE_HUB_V1\t0\ta\tb", from([169, 254, 1, 1])).is_none());
    }
}
