<!-- Version: 1.1.0 · updated 26-08-22-17-12 -->
# Design sketch: the burst carousel

Nothing here is built. This is where the session's reasoning landed.

## The scenario it is for

A school with poor or no internet. The router and the TV are locked in a metal
cage; only the TV screen is reachable. No access to router configuration. No
ethernet cable, because a cable like that is worth more than the laptops. A pile
of donated machines of varying age, and possibly a phone or two.

A teacher needs to get the same folder onto thirty laptops.

**This is the constraint list the design answers.** Every decision below traces
back to one of these lines.

## Why LocalSend cannot serve it

**[source, see 01]** Two structural reasons, neither of them fixable by
configuration:

1. It cannot create a network. There is no hotspot, AP or Wi-Fi Direct code in
   the tree at all. It assumes a network exists and that you are on it.
2. It is one to one. Thirty laptops is thirty unicast transfers, which is the
   same bytes crossing one shared channel thirty times.

## The idea borrowed from Kurier

The Kriegsmarine's Kurier system (trials 1944) recorded a message onto tape in
advance and fired it as a single burst of roughly half a second. Too short for
Allied direction finding to take a bearing. Receivers recorded the burst and
decoded it afterwards. No handshake, no acknowledgement, no back-channel.
Nothing on the air except payload.

Their reason was survival. Ours is different and the mechanism is identical:
**airtime is the only scarce resource in the room, and everything that is not
payload is stealing it.**

Compare what LocalSend puts on the air before one byte of file moves: multicast
announce, HTTP register, prepare-upload, wait for a human to tap accept, TLS
handshake, then a separate POST per file. Then the whole thing again for the
next laptop.

## The five rules

**Prepare offline.** Tar the folder, compress once, encrypt once, encode once.
All disk and CPU work happens before the radio is touched. When transmission
starts it is nothing but bytes going out. This is the tape being punched.

**Fire, do not negotiate.** No discovery, no registration, no per-file request.
The sender does not know or care who is listening, and does not need to know how
many machines are in the room.

**Record now, understand later.** Receivers dump blocks to disk and decode after
the transmission ends. This is the rule that matters most for weak hardware: a
donated laptop cannot do TLS plus SHA-256 at wire speed, but it can certainly
write UDP packets to disk. Decoupling capture from decode means the radio runs
at full rate regardless of the receiver's CPU.

**Repeat instead of retry.** Broadcast the payload in a continuous loop, a
carousel. Combined with a rateless fountain code (RaptorQ, RFC 6330) a receiver
needs *any* K blocks out of an endless stream, not any particular ones. A laptop
that boots five minutes late completes on the next lap. A laptop losing forty
percent of packets completes, just slower. Neither ever transmits.

**Keys go out of band, physically.** Encrypt the payload once with a pre-shared
key and have the teacher write six words on the whiteboard. No TLS handshake, no
certificates, nothing negotiated on the air. It is also the more honest security
model here: the people allowed to receive it are the people in the room.

## Compression settings

**[measured, see 02]** Compression is already 82 times faster than the radio, so
speed is irrelevant and ratio is everything. But the ratio spread is only 24
percent, so this is a real tradeoff and not a "always use maximum" rule:

| situation | setting | why |
|---|---|---|
| Two machines, one transfer | `zstd --fast=4` | Break-even for higher levels is about 25 transmissions |
| Carousel looping, or many receivers | `zstd -12` | 96 percent of the max ratio benefit for 8 percent of the CPU cost. Probably the default |
| Very large payload, plenty of prep time | `zstd -19 --long` | Pays for itself after ~25 laps and then keeps paying |
| Photos, video, PDFs, anything already compressed | none | Ratio gap collapses to nothing. Burning 8 seconds for zero saved bytes |

## Two transmission modes

### Mode A: laptop as access point

One machine runs the hotspot; everyone joins it. **[measured]** This card
supports `AP` mode, so even 2012 hardware can do it.

```
nmcli device wifi hotspot ifname Gorilla.WIFI ssid BURST_NET password "..."
```

NetworkManager's shared mode puts the host on `10.42.0.1/24` and runs DHCP from
the already-installed dnsmasq.

Because the **sender is the access point**, transmission is a single hop to
every station. The double-air penalty that halves a normal device-to-device
transfer disappears entirely. That alone is a 2x win before anything clever
happens.

**[inference] The unresolved risk that decides this mode.** mac80211 transmits
multicast and broadcast frames at the *lowest basic rate of the BSS*, and a
default 2.4 GHz b/g hotspot advertises 1 Mbps as a basic rate for legacy
compatibility. A carousel at 1 Mbps runs at 125 KB/s to the whole room, and
thirty unicast transfers would beat it. The lever is `hostapd`'s `basic_rates`,
raising the lowest basic rate to 12 or 24 Mbps and dragging multicast up with
it. `nmcli device wifi hotspot` drives wpa_supplicant's AP mode instead, which
exposes no such control and is not built for thirty associated clients.

So: NetworkManager for a two-laptop proof, `hostapd` for the classroom.
**hostapd is not currently installed on this machine.**

In IBSS mode there is a direct knob, `iw dev <if> set mcast_rate 24`, which
makes IBSS a fast way to measure the effect even if it is not what ships.

### Mode B: no association at all

**[measured]** This card advertises `monitor` and `outside context of a BSS`,
which together mean it can transmit and receive raw 802.11 frames without
associating to anything.

The sender injects frames on a fixed channel. Receivers sit in monitor mode on
that channel and capture. No AP, no SSID, no DHCP, no ARP, no association, and
therefore no client limit and no association storm when thirty machines join at
once. Thirty receivers cost the sender exactly what one costs.

Costs: root on both ends, phones excluded entirely, channel agreed in advance
rather than discovered. Not the shipping default. The mode you switch to when
there are thirty laptops and the access point is buckling.

## The one deliberate departure from Kurier

A pure fire-and-forget carousel leaves the teacher with no idea whether anyone
received anything. Kurier accepted that trade because a submarine transmitting a
receipt was a dead submarine. A classroom has no such constraint.

One small unicast "I have it" per laptop, sent once after decoding completes,
carries no payload and costs almost no airtime. It turns an invisible process
into thirty ticks on a screen. Build it, keep it off the critical path.

## Where this should live

**Open question, not decided.** Argument for its own small binary: that school
does not need an AI coding agent, it needs one static Go binary of a couple of
megabytes that says "be the hotspot" and "send this to everyone", downloadable
in four minutes at 8 KB/s. Argument for folding it into gorilla-opencode: the
packaging, signing and release machinery already exists there.

If it does go into gorilla-opencode, one rule from prior lessons applies
directly: **make it a slash command, not an LLM tool.** A registered tool's
schema rides every turn forever and the user pays for it whether or not they
ever transfer a file.

## The fast path worth keeping regardless

For two machines, this beats LocalSend and is four lines:

```
tar -cf - ./folder | zstd --fast=4 -T0 | nc -N <ip> 9999
```

No TLS handshake, no register, no prepare-upload, no per-file POST, no SHA-256
pre-pass, no waiting for someone to tap accept. One TCP stream with tar framing
the files so there is no per-file round trip. **[inference]** It should sit near
line rate the whole way. Not the classroom answer, because `nc` sends to one IP,
but the right answer for two machines and worth shipping as a mode.

---

# Revision note, 2026-08-22 (later the same session)

Everything above is left as written. This note records what changed and why,
rather than editing the original reasoning out of existence.

**Superseded: Mode B is not the architecture.** Raw 802.11 injection with no
association is Linux only. Windows has no usable injection path, ChromeOS has no
raw sockets, Android needs root. For the target device zoo it reaches the fewest
machines, not the most. It survives as a Linux-to-Linux fast path.

**Superseded: a single receiver design.** There are now two receiver tiers, a
native multicast carousel and a plain-HTTP browser fallback for devices where
nothing can be installed. The carousel becomes an optimisation over a universal
base rather than the whole system.

**Changed: the transport encryption position.** The pre-shared key model above
was argued on grounds of airtime and honesty. It turns out to be load-bearing
for a second reason: it lets the browser fallback be served over plain HTTP,
which is the only thing that reaches a Windows XP browser without a certificate
warning.

**Moved, not decided: where this should live.** The browser fallback tier
strengthens the standalone-binary argument.

Full reasoning: `06-device-constraints.md`. New measurements required:
`05-open-questions.md` items 7 to 9.
