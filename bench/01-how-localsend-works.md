<!-- Version: 1.0.0 · updated 26-08-22-16-50 -->
# How LocalSend 1.18.2 actually moves a file

All references are to the tree this folder sits inside. Flutter/Dart UI on top,
a Rust core in `packages/core` doing the network and disk work.

## Transport: Wi-Fi only. There is no Bluetooth

**[source]** Grepping the whole tree for "bluetooth" returns two hits, neither a
transport:

- `app/lib/util/fingerprint_alphabet.dart:34` an icon name
- `packages/core/src/discovery/store.rs:125` a comment saying other transports
  (WebRTC, Bluetooth) "will" exist

Nothing implements it. Anyone who says LocalSend uses Bluetooth is wrong.

## The four steps of a transfer

**[source]**

1. **Discovery.** UDP multicast to `224.0.0.167:53317`
   (`packages/core/src/multicast/mod.rs:28`). The comment above the constant
   says the group was deliberately chosen inside `224.0.0.0/24` because that is
   the local network control block, which routers must never forward. The
   packets physically cannot leave the link.
2. **Fallback discovery.** If multicast is blocked, it probes all 256 hosts of
   the local `/24` (`packages/core/src/discovery/mod.rs:362`).
3. **Handshake.** `POST /prepare-upload` over TLS. Self-signed certificates,
   peer pinned by SHA-256 fingerprint during the handshake
   (`packages/core/src/http/client/mod.rs`).
4. **Transfer.** One `POST /upload` per file, streamed
   (`packages/core/src/http/client/v3.rs:207`).

## Why "No Internet Required" is completely true

**[source]** Private addressing, a multicast group that cannot be routed off the
link, and a direct TCP connection between two local IPs. No DNS, no account, no
relay. Unplug the WAN and every step still works, because a router is a WAN
gateway and a switch in one box and only the first half dies.

The one exception is the WebRTC path for transfers across different networks,
which does need the internet: `wss://public.localsend.org/v1/ws` for signaling
and `stun.localsend.org` for NAT traversal
(`app/lib/provider/network/webrtc/signaling_provider.dart:53`). Opt-in, not used
on a LAN.

## The code is not badly written

**[source]** The usual suspects were all checked and all done correctly:

- 512 KiB read buffer on the sender (`packages/core/src/model/transfer.rs:11`)
- 512 KiB `BufWriter` on the receiver
  (`packages/core/src/http/server/common/save.rs:15`)
- True streaming with backpressure, 16-slot channels both directions
- No file is ever fully loaded into memory
- On desktop the file path is handed to Rust, so bytes never cross the Dart FFI
  boundary

## What does cost time

**[source]**

| thing | where | effect |
|---|---|---|
| Checksums on by default | `app/lib/provider/persistence_provider.dart:397` returns `?? true` | Sender reads and SHA-256s **every file, one after another, before the first byte is sent** (`app/lib/provider/network/send_provider.dart:165`). Receiver hashes again on the way in (`save.rs:112`). On a CPU without SHA-NI this is a full extra read of everything, during which the progress bar shows nothing |
| HTTP/1.1 only | `packages/core/src/http/client/mod.rs:226` sets `alpn_protocols` to `http/1.1` | No HTTP/2 multiplexing. Every file is its own request/response round trip |
| Two files at a time | `packages/localsend_isolates/lib/src/isolate/child/upload_isolate.dart:17` | A folder of thousands of small files is dominated by per-file overhead, not bandwidth |

## The two structural limits

**[source]**

1. **It cannot create a network.** Grepping the tree for "hotspot" returns
   nothing. No AP mode, no Wi-Fi Direct, no ad-hoc. It assumes a network exists
   and that you are allowed on it.
2. **It is strictly one to one.** Every transfer is a unicast HTTP POST to one
   peer. Sending to N devices costs N times the airtime, and because they share
   one radio channel, running them in parallel does not help.

## On the marketing claims

**[inference, from source]** The four boxes on localsend.org are honest, with
one asterisk each:

- *Cross-platform* true.
- *Secure, end-to-end encryption* true as shipped. Point-to-point TLS with
  fingerprint pinning genuinely is end to end when there is no server in the
  middle. Asterisk: encryption is a setting, not a law. `ProtocolType` has an
  `Http` variant (`packages/core/src/model/discovery.rs:19`) and the default is
  `?? true` (`persistence_provider.dart:494`), so it can be turned off.
- *No Internet Required* completely true, the strongest of the four.
- *Blazingly Fast* read the small print. It says "transfer at the maximum speed
  of your WiFi network. No bandwidth limits." That is a promise of **no
  artificial throttle**, not a promise of speed. The app keeps it. The word
  doing the work is "Blazingly", which is the only word on the card with no
  technical content. It does fall short of its own small print in two cases:
  during the checksum pre-pass you are at zero, and on many small files you are
  limited by round trips, not by the link.
