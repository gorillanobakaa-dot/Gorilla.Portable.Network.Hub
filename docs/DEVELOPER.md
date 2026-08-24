<!-- Version: 1.1.1 · updated 26-08-24-23-23 -->
# Gorilla Portable Network Hub: the developer track

Companion to `WHY-THIS-EXISTS.md`, which is the layman track and is not a
simplification of this. Both are complete; they differ in language, not in
honesty.

Every claim below is tagged **[measured]** (taken on real hardware, with the
command recorded), **[source]** (read out of a source tree, cited), or
**[inference]** (reasoning from those, to be treated as a hypothesis).

Reference hardware for every measurement unless stated otherwise: Sony VAIO
SVE14A3AJ, i7-3632QM Ivy Bridge, 16 GB, Qualcomm Atheros AR9485 (**1x1**,
802.11n, 2.4 GHz only), Kingston DC600M SATA SSD, Debian 13 with a custom
kernel. The client was a Windows 11 ThinkPad with an Intel 2x2 Wi-Fi 6 adapter.

---

## 1. What the thing is

A laptop that **creates a network where none exists** and hands a folder to
every device in the room.

Not a file transfer tool. LocalSend already does that well, at line rate, and
needs no help. The gap it cannot fill is that **it cannot make a network**:
there is no AP, Wi-Fi Direct or ad-hoc code anywhere in its tree **[source]**.
It joins networks. In a school with no router, that is the end of the story.

Two transports, and the distinction governs almost every decision:

| | unicast over the AP | broadcast carousel |
|---|---|---|
| status | **built and measured** | designed, not built |
| encryption | WPA2 handles it | none available, payload must carry its own |
| rate ceiling | the radio's, 72.2 Mbit/s here | **54 Mbit/s, by protocol** |
| scales with better hardware | fully | only via raw injection |
| cost for N receivers | N x | flat |

---

## 2. The measurements that decide the design

### 2.1 The broadcast rate ladder **[measured]**

Group-addressed frames ride the lowest rate in the BSS **basic rate set**.
hostapd's default `hw_mode=g` advertises the legacy b/g set, so broadcast goes
out at **1 Mbit/s** on hardware capable of 54.

| `basic_rates` | advertised basic set | beacon TX rate |
|---|---|---|
| default | 1(B) 2(B) 5.5(B) 11(B) | **1.0 Mbps** |
| `120 240` | 12(B) 24(B) | 12 Mbps |
| `240` | 24(B) | 24 Mbps |
| `360` | 36(B) | 36 Mbps |
| `540` | 54(B) | **54 Mbps** |

**54x from one configuration line.** Verified by sniffing our own beacons on a
monitor interface, filtered by transmitter address.

**[inference]** At the 1 Mbit/s default the carousel loses to plain one-at-a-time
transfers until roughly **43 receivers**, which would have made the entire design
worse than doing nothing clever. At 54 Mbit/s it wins from the second or third.

**The 54 Mbit/s ceiling is the protocol, not the card.** The basic rate set is
legacy OFDM only, in every band, on every device ever made. A Wi-Fi 7 radio
still broadcasts at 54. The only route past it is raw injection in monitor mode,
where the rate is ours to choose. This AR9485 advertises both `monitor` and
`outside context of a BSS` **[measured]**, so it can.

### 2.2 Unicast throughput as the access point **[measured]**

8.4 GB moved, sampled once a second.

| | baseline | tuned |
|---|---|---|
| negotiated | 65.0 Mbit/s, long GI | 72.2 Mbit/s, short GI |
| mean | 5.80 MB/s | **6.16** |
| median | 5.88 | **6.48** |
| 5th percentile | 5.27 | 4.46 |
| std deviation | 0.37 (6.4%) | 0.81 (13.1%) |

Peak observed across the whole day: **7.04 MB/s = 56.3 Mbit/s**, which is **78%**
of the 72.2 Mbit/s PHY. The same card through the office router manages 59%,
the difference being the double-air penalty that disappears when the sender IS
the access point.

**Honest caveat on the tuning**: the median improved 10% and the fifth percentile
got 15% worse, with variance doubling. Short guard interval is faster on a good
second and worse on a bad one. For a classroom, where what matters is everyone
finishing rather than anyone finishing fast, that trade is not obviously correct
and has never been tested at distance.

### 2.3 Connection count **[measured]**

Swept back to back over the real link, same file, each run a separate process.

| connections | mean | median | peak | sd |
|---|---|---|---|---|
| 1 | 6.51 | 6.65 | 7.00 | 0.61 |
| 2 | 6.55 | 6.66 | 7.04 | 0.50 |
| **4** | **6.57** | 6.64 | 7.04 | **0.45** |
| 8 | 6.36 | 6.44 | 7.02 | 0.58 |
| 16 | 6.14 | 6.37 | 6.81 | 0.78 |
| 32 | 6.20 | 6.46 | 6.91 | 0.76 |

**The peak is 7.0 in every row.** The ceiling is airtime; threading does not
create more of it. Four is the best mean and the lowest variance, so that is the
default, and it exists for resilience and granularity rather than speed.

Do NOT scale this with core count. Thirty children at 32 workers each is 960
sockets on the teacher's machine to do a job 120 does better. Loopback gives the
opposite answer (1 worker 158 MB/s, 2 workers 476) because there the bottleneck
is one thread copying bytes. **Benchmarking on loopback would have chosen exactly
the wrong default.**

### 2.4 Kernel bypass, ruled out **[measured]**

DPDK, Seastar and TRex were considered and rejected on arithmetic.

```
loopback through the full kernel TCP stack   379 MB/s = 265,028 packets/sec
what the radio demands                       6.3 MB/s =   4,404 packets/sec
CPU idle across the day                      94%
```

The kernel already delivers **60x** what the radio can carry, at 94% idle. More
fundamentally, **DPDK has no 802.11 support and cannot easily have any**:
association, WPA2, rate control, retries and aggregation live in `mac80211` and
card firmware. The bottleneck is airtime, which is physics.

---

## 3. Things that break, that nobody documents

### 3.1 Windows abandons an offline network in 16 seconds **[measured]**

```
15:06:27  CONNECTED
15:06:43  DISCONNECTED     16 s
15:06:43  CONNECTED
15:06:59  DISCONNECTED     16 s
```

Mechanism, from a packet capture: the client resolves `dns.msftncsi.com` and
fetches `www.msftconnecttest.com/connecttest.txt`, expecting the exact body
`Microsoft Connect Test`. With no upstream it fails, Windows flags the network
dead and leaves for a remembered one.

This produced three symptoms that each looked like a different fault: a transfer
at 30.6 KB/s, `ConnectionRefused` that looked like a firewall, and browser
downloads dying with "network issue".

**Fix**: resolve those domains to the AP and answer the probe locally. A burst
network never has an upstream, so this is a hard requirement, not a nicety.

### 3.2 LocalSend cannot resume **[source]**

```
Range / 206 / seek in packages/core   NONE
receiving a file                      tokio::fs::File::create(&path)   TRUNCATES
MAX_UPLOAD_ATTEMPTS                   3, and only for SaveResult::HashMismatch
```

**[measured]** consequence, same program, same machines, same file:

| link | speed | outcome |
|---|---|---|
| dropping every 16 s | **30.6 KB/s** | 1.1 MB of 4 GB, 36 h estimated |
| stable | **6.1 MB/s** | 4.0 GB complete |

At 6 MB/s, sixteen seconds moves ~96 MB; the connection drops, the destination
is recreated at zero, and it starts again. A browser survived the identical link
because it issues Range requests. **That is the entire 200x difference.**

Not a criticism of LocalSend, which is good software written for a network that
stays up. It is a statement about which assumptions fail in a classroom.

### 3.3 A bounded worker pool without timeouts is a countdown **[measured]**

A Windows client was suspended by its power settings mid-transfer. Sockets
stayed open, `write_all` blocked with no timeout, and each stalled connection
permanently consumed one of 64 workers. The server was dead while reporting
perfect health: unit active, 142 connections established, zero errors, 0.00 MB/s
for eight minutes.

**The classroom failure mode exactly**: one lid closes, one worker gone, until
the whole class stops receiving with nothing in any log.

---

## 4. Architecture as built

Four crates, no dependencies, std only. Sizes are release builds, stripped,
`opt-level="z"`, LTO, one codegen unit, `panic=abort`.

| crate | bytes | what |
|---|---|---|
| `src/fileserver` | 389,240 | HTTP/1.1 with byte ranges and keep-alive |
| `src/fetch` | ~386,000 | parallel, resumable, verifying client |
| `src/sums` | 393,800 | per-chunk SHA-256 across all cores |
| `src/shared/sha256.rs` | 142 lines | FIPS 180-4, written out rather than pulled in |

**Rust over Go**, decided on measurement and three arguments. Same program in
both: Go 2,289,956 bytes raw / 799,496 xz; Rust 352,464 / 148,144. **6.5x and
5.4x.** But size alone did not decide it. The deciders were the `raptorq` crate
for the fountain coding, no GC on a 4 GB receiver, and LocalSend's core already
being Rust. Windows cross-build works out of the box:
`x86_64-pc-windows-gnu` plus mingw, producing a **323,072 byte** PE32+, smaller
than the Linux build.

### 4.1 Wire format

Plain HTTP/1.1. No TLS, deliberately: `std` has none, pulling one in multiplies
the binary, and WPA2 already encrypts the link. The carousel is where payload
encryption belongs, because broadcast has no association and therefore no key.

```
GET /path HTTP/1.1
Range: bytes=START-END
Connection: keep-alive
```

**2 MB chunks.** A lost chunk costs 0.3 s on this AP and ~20 s on a marginal
link, bounded everywhere. Small chunks are only affordable because of
keep-alive: **[measured]** 477 chunks over **9** connections, 53 requests each.
Without it, 476 connections and 31.5 MB/s instead of 2,216 on loopback, a **10x**
penalty for shrinking the chunk without fixing reuse.

### 4.2 Resume and verification

A `.parts` sidecar lists completed chunk indices. On restart, chunks in it are
skipped, **but only after being re-verified** against the server's `.sums` file
in parallel. The sidecar is then rewritten from the verified set.

That re-verification exists because of a bug: a test corrupted a chunk, marked it
complete, and watched the corruption survive untouched. **A sidecar is a claim,
not proof.**

`.sums` format, generated by `sums`, served as an ordinary static file so the
server needs no special support:

```
# chunk-size 2097152
# total 1000000000
0 371ba08e82dbffbb4f60afe10b0ffa13fd1d7ade949f5758dd06fceafad2063c
1 ...
```

Per-chunk rather than whole-file because it parallelises **and** because a
corrupt 2 MB piece can be repaired alone instead of invalidating a 4 GB
download. **[measured]** 153 MB/s single-threaded, 623 across 8 cores warm,
**458 cold** after switching to contiguous spans.

### 4.3 Keeping the machine awake

Windows: `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED |
ES_AWAYMODE_REQUIRED)`, cleared on `Drop`, because leaving it set would drain a
battery nobody can charge.

Linux: `systemd-inhibit ... cat` with its stdin on a pipe we hold. **Not `sleep
infinity`**: Rust does not run `Drop` on SIGKILL, so a killed process would
orphan the inhibitor forever. When the process dies for any reason the kernel
closes the pipe, `cat` sees EOF and the lock releases. **[measured]** verified by
SIGKILLing the parent and confirming the inhibitor count returned to zero.

---

## 5. Deployment reality

**The sender must be Linux.** Windows Mobile Hotspot exposes none of
`basic_rates`, `ht_capab` or the WMM queues, and caps clients around eight.
ChromeOS cannot create an AP without developer mode. Both are fine as receivers.

**The sender needs root**, every time, on every platform: `CAP_NET_ADMIN` for AP
mode, DHCP, and port 80 for the connectivity responder. **Receivers need
nothing**, which is the asymmetry that makes it deployable.

**`hostapd` is not installed on a stock Debian or Ubuntu** (confirmed: it
installed fresh on this machine on 2026-08-24), and there is no internet to
`apt install` it with. Three routes, unresolved:

1. ship `hostapd` and `dnsmasq` alongside in the package
2. speak `nl80211` directly from our own binary, keeping the one-file promise
3. accept NetworkManager's hotspot and lose the carousel entirely

**Regulatory freedom is not portable.** Channel 13 worked here because this
kernel has `reg.c` and `regd.c` patched. A stock machine obeys its region, so
default to channels 1, 6 and 11 and negotiate the country code.

**Exposure.** When the laptop becomes the network, every listening service on it
is reachable by the class. On this bench that was CUPS on 631 and a SearXNG
instance on 8888, with **no IPv4 firewall in the kernel at all**
(`CONFIG_NF_TABLES_IPV4` and `CONFIG_IP_NF_IPTABLES` both unset). Stock targets
have `nftables`, so the tool should add a rule; where it cannot, it must
**enumerate what is listening and say so** rather than stay silent.

---

## 6. What is unproven

1. **Thirty devices in one room.** Everything measured used one or two. Thirty
   connections from one laptop is not thirty stations: no association overhead,
   no per-station queueing, no power-save buffering, no inter-station contention.
2. **Any broadcast receiver at all.** The rate ladder was measured on beacons.
   Group-addressed *data* frames were never confirmed, because an AP with no
   associated station transmits none. 802.11 says both use the basic rate set,
   so they are expected to match. **[inference]**
3. **Range.** 54 Mbit/s is 64-QAM 3/4 and needs about -65 dBm. At 4 m the link
   budget gives roughly -33 dBm, so on paper there is 30 dB of headroom even
   through a wall of children. Untested.
4. **Whether a phone hotspot passes client-to-client traffic and multicast.**
   If it does, the two-to-five device case needs no software at all.
5. **A spinning disk sender.** Everything assumed a 520 MB/s SSD. A $50 ThinkPad
   likely has a 5400rpm drive, where the contiguous-span finding would matter
   for the file server as it did for the hasher.

## 7. The screen

Added 2026-08-24. `hub` with no arguments opens it; every command still works
without it.

### Why there is no TUI library

The program has no dependencies at all, and the reason is the audience: a
download is minutes of somebody's life on a single-digit-KB/s line. A mainstream
Rust terminal stack (a backend plus a widget layer) is hundreds of KB of binary
to draw text that ANSI has drawn since 1979. `term.rs` is about 300 lines of
`std` and does the whole job.

| | bytes | note |
|---|---|---|
| three separate command-line binaries | 1,262,576 | **[measured]** each carried its own runtime |
| merged into one | 514,480 | **[measured]** 2026-08-24 |
| with the screen | 597,648 | **[measured]** 2026-08-24 |
| Windows, `x86_64-pc-windows-gnu` | 520,704 | **[measured]** runs under wine, `doctor` verified |

The screen cost **83,168 bytes**.

### Layout rules and why they are rules

A terminal has a fixed row and column budget. Every border, margin and padding
subtracts from it, and layout engines do not error when content exceeds a
container: they **wrap**, so the failure appears as excess height in a different
place from the cause.

- **No borders, boxes or rules.** Selection is reverse video (`\x1b[7m`), which
  costs zero columns. Grouping is blank lines, which cost one row and cannot
  wrap.
- **The highlight is as wide as its group, not as wide as the terminal.** Seen
  on a real 190-column window on 2026-08-24, full-width reverse video is a slab
  across the whole screen that shouts louder than the thing it points at. Group
  width, not per-item width, so the highlighted edge stays straight as the
  selection moves.
- **Progress bars are ASCII `#` and `-`.** Block and box-drawing characters are
  East Asian **Ambiguous** width: one column here, **two** in a terminal
  configured for Chinese, which silently doubles a bar's length.
- **The frame is exactly `rows` tall.** `Frame::new(rows, cols)` is a budget
  decided before anything is drawn; `push` past the last row is dropped rather
  than allowed to scroll. The test asserts the last non-blank row **equals** the
  terminal height, never "is not taller": "not taller" passes against a frame
  that is silently too short, and too short is the state that wraps.
- **Truncation is by display column, not by char and not by byte.** A CJK
  ideograph is one char and two columns; a byte slice can cut a character in
  half.
- **Lists say what they dropped.** A list that silently stops at ten reads as
  "ten devices" to the person looking at it.

### Platform layer

|  | Linux | Windows |
|---|---|---|
| raw mode | `stty -g` to save, `stty raw -echo`, restore by replaying the saved string | `SetConsoleMode`, clearing `ENABLE_LINE_INPUT`/`ENABLE_ECHO_INPUT`/`ENABLE_PROCESSED_INPUT` |
| ANSI | always | `ENABLE_VIRTUAL_TERMINAL_PROCESSING`, present since Windows 10 1511 |
| size | `ioctl(1, TIOCGWINSZ)` into a 4-`u16` struct | `GetConsoleScreenBufferInfo` |
| randomness | `/dev/urandom` | `RtlGenRandom` (`SystemFunction036`, advapi32) |

`stty` rather than `tcsetattr` because the `termios` struct layout differs per
architecture and is silently wrong when it is wrong; `ioctl` is used for the
size because a 4-`u16` struct has no layout to get wrong and querying every
frame is two syscalls rather than a process spawn.

`panic = "abort"` is set in the release profile, so **`Drop` does not run on a
panic**. The terminal restore is therefore installed as a panic hook as well as
a `Drop` impl. Leaving a teacher with a terminal that echoes nothing, needing
`reset` typed blind, is how somebody stops using a tool for good.

Blocking key reads live on their own thread and arrive as bytes on an `mpsc`
channel, so the draw loop can `recv_timeout` and keep redrawing live numbers. A
thread rather than `poll`/`WaitForSingleObject` because it is the same code on
both platforms with no FFI to get wrong.

If raw mode cannot be entered at all, `hub` prints the command list instead of
drawing. `hub > out.txt` produces a usage message rather than a file of escape
sequences.

### Discovery

Whoever runs the hotspot **is** the default gateway for everyone on it, so
"where is the teacher" and "what is my gateway" have the same answer. No
beacons, no multicast, no service discovery.

1. Gateway from `/proc/net/route` (little-endian hex, `Destination == 00000000`)
   plus the three well-known hotspot gateways: `10.42.0.1` (NetworkManager
   shared), `192.168.137.1` (Windows Mobile Hotspot), `192.168.43.1` (Android).
   All probed in parallel, 400 ms.
2. If that finds nothing, sweep the local **/24** in batches of 64, connect-only,
   then ask the ones that answer. About 1.5 s. This is the room that **does**
   have a router, where the teacher is an ordinary device at an unguessable
   address.
3. Type it by hand, host and optional port.

A /24 and not the real mask: this network is a /20, which is 4,094 addresses and
about half a minute. Teacher and class share an access point and therefore a /24
in every case this tool is for.

Local addresses come from `UdpSocket::connect` plus `local_addr`. A connected
UDP socket sends nothing; `connect` only sets the default destination and the
kernel picks a source address by consulting the routing table. Reading it back
is a route lookup with no packets, no privileges, and the same code on both
platforms. `getifaddrs`/`GetAdaptersAddresses` is per-platform FFI to answer a
question this already answers.

### The network is put back

Creating a hotspot **replaces the teacher's own wifi**. The previous connection
name is recorded before `nmcli device wifi hotspot` runs, and restored three
ways:

1. `Drop`/explicit stop, for a clean exit.
2. A panic hook, since `panic = "abort"` skips `Drop`.
3. **A `systemd-run --user --on-active=180` transient timer**, re-armed every 60
   seconds while the lesson runs. systemd owns it, this process does not, so the
   wifi returns even on `SIGKILL`, a closed terminal, or battery management
   killing the session.

Point 3 exists because this machine locked itself off its own network twice
during development, both times because the teardown lived in a shell `trap` that
never ran. A teacher whose laptop loses wifi after a lesson will not use the tool
again and will have no idea what did it.

WPA2 always, **never open**. An open network lets every device in radio range
reach the serving port on the teacher's own laptop, and the teacher has no way
to see who is on it. `--name` without `--password` is refused. The suggested
password is 8 characters from a 31-symbol alphabet with no `0/O/1/l/I`
(about 39 bits), drawn from the OS random source with rejection sampling, not
`%`: 256 is not a multiple of 31, so plain modulo would make the first eight
letters likelier.

### Who is on the network, which is not the same question as who is downloading

Reported from a real hotspot on 2026-08-24: a phone joined `LOL1` and the screen
still said "Nobody has connected yet". The live table was fed only from
`send_file`, so a device that joined the wifi and waited was invisible. For a
teacher those are two different questions and the first one comes first, and
answering it wrongly sends them off checking the password when nothing is wrong.

Two sources, best first:

1. **The DHCP leases** NetworkManager's dnsmasq writes, at
   `/var/lib/NetworkManager/dnsmasq-<iface>.leases`. These carry the device's
   own name (`Xiaomi-11-Lite-5G-NE` rather than `10.42.0.90`), which is what a
   teacher can match to a child. Readable **only as root**: that directory is
   `drwx------`. A hotspot started through polkit by an ordinary user therefore
   does not get names.
2. **`/proc/net/arp`**, world readable, no privileges. No names, but it answers
   "how many and at what addresses", with flags `0x2` filtering out incomplete
   entries (an address we asked about and got no answer for).

The MAC address is deliberately not carried out of either parser. It is a
permanent hardware identifier for somebody else's device, it is no use to a
teacher who has the name and the address, and anything on screen ends up in a
screenshot.

The subnet that counts as "the class" is asked for, not guessed: `nmcli -g
IP4.ADDRESS device show <iface>`. NetworkManager's shared mode uses 10.42.0.1 in
practice, but that is a default rather than a promise, and a machine with a
second interface can easily hold an address that sorts first. The list is only
gathered when **we** made the network; over a network somebody else provided,
"who is on it" is the whole building.

**Open:** names without root. dnsmasq answers PTR queries for its own leases on
10.42.0.1:53, so a hand-rolled DNS lookup would get them, but shelling out to
`getent` from the draw loop risks a five-second resolver timeout freezing the
screen. Running with `sudo` gets names today.

### Every nmcli call has stdin closed

If a machine wants a polkit password there is nowhere to type it while a
full-screen program is drawing, and `Command::output()` would wait forever with
no message. `Stdio::null()` turns that into a refusal the tool can explain.

### Bugs the pty harness found that review did not

A terminal UI cannot be checked by reading it. The harness (`pty.fork`,
`TIOCSWINSZ`, keystrokes in, escape sequences interpreted into a character grid)
found three faults in one evening:

1. **`Connection: close` ignored server-side.** `serve_one` returned a literal
   `true` for "connection may be reused" regardless of what the client asked
   for. The client sent `Connection: close` and then read to EOF, which never
   came, so listing timed out. Discovery uses a 400 ms timeout, so **nothing was
   ever discovered** while a server sat there answering, and the screen said
   "Nothing found on this network". Neither half is wrong alone. Fixed on both
   sides: the server honours the header and the version, and the client reads
   exactly `Content-Length` instead of to EOF.
2. **A typed port was silently discarded.** `10.42.0.1:9000` connected to 8080
   and reported that the file list could not be read.
3. **`fetch_sums` allocated whatever `Content-Length` claimed.** A test server
   that answered every path with a 400 MB file left the download at zero bytes
   with no message. Capped at 8 MB, derived from the real shape: one ~71-byte
   line per 2 MB piece covers a ~230 GB download.

A fourth, found only once both screens were driven at the same time: the live
transfer table was keyed on the socket's peer address, which includes the
**port**. One laptop downloading with four parallel connections therefore
appeared as four devices, each stuck at 1%, and the header said "5 devices
getting files" with one child in the room. Keyed on the address alone now, with
each write adding its delta to that device's total and the rate measured over a
one-second window rather than between writes.

And one found by a test written to **lie** to the program: a `.parts` sidecar
claiming pieces the file was never long enough to hold was believed, and the
download reported success at **2,861 MB/s having fetched nothing**, differing
from the original at byte 1,000,001. The on-disk length is now read *before*
`set_len` grows the file, and any piece the file could not have held is
discarded. One `stat`, needs nothing from the server, unlike the digest path
which only runs when the peer offers fingerprints. That is the **second**
instance of one trap in a single day; the first was verification that could not
run because of a write-only file handle.
