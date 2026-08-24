<!-- Version: 1.1.0 · updated 26-08-24-12-17 -->
# What to measure before building anything

In order. Item 1 decides whether the rest is worth doing at all.

## 1. Broadcast throughput at default basic rates

**The question that kills or saves the design.** Twenty minute job, needs two
machines.

Bring up an NM hotspot, join it with a second machine, and measure actual UDP
multicast throughput from the AP host to the station.

```bash
nmcli device wifi hotspot ifname Gorilla.WIFI ssid BURST_NET password "temporary-passphrase"
```

**Hypothesis [inference]:** it lands near 1 Mbps, because mac80211 sends
multicast at the lowest basic rate and a default b/g hotspot advertises 1 Mbps
for legacy compatibility. If so, a carousel would run at 125 KB/s and thirty
unicast transfers would beat it outright.

**If the hypothesis holds, item 2 becomes mandatory rather than an optimisation.**

## 2. The same with hostapd and basic_rates forced up

```bash
sudo apt install hostapd
```

Configure `basic_rates` so the lowest is 12 or 24 Mbps and re-run the item 1
measurement. Expected to drag multicast up with it.

Also worth checking in the same session: `multicast_to_unicast` must be **off**.
If hostapd converts multicast to per-client unicast, it silently reintroduces
the N-copies problem the whole design exists to avoid, and the throughput number
will look fine while the airtime cost is thirty times what it should be.

Quicker proxy if hostapd is awkward: IBSS mode exposes the rate directly with
`iw dev <if> set mcast_rate 24`, which measures the effect without a full AP
configuration.

## 3. RaptorQ encode and decode speed on weak hardware

Measure on this i7-3632QM first, then on the weakest machine available.

The design's "record now, understand later" rule assumes decode can be deferred
and then run at acceptable speed. If decode on a donated laptop is slower than
the transmission it is decoding, the carousel has to slow down to match and the
design changes shape.

**Related unmeasured assumption:** that a weak laptop can write captured UDP
packets to disk at line rate. Probably fine at 5 MB/s, worth confirming on the
slowest disk in the room.

## 4. Injection and monitor capture between two machines

`monitor` and `outside context of a BSS` are **advertised** by this card
`[measured]`. Whether ath9k actually injects and captures reliably in practice is
a separate question, and driver capability tables have been optimistic before.

Confirm before treating Mode B as real.

## 5. Two-hop device-to-device throughput

The 2.6 MB/s figure that explains the original "very very slow" experience is
`[inference]`, derived by halving the measured one-hop number. Worth measuring
directly, because it is the baseline every improvement will be quoted against.

## 6. igzip ratio, to close out the corrections log

```bash
sudo apt install isal
```

Then re-run `bench/compression-bench.sh`, which already contains an igzip row
that reports MISSING until the package is present. The prediction is that its
ratio lands at or below `gzip -1` (0.455) and therefore well behind `zstd -12`
(0.372). Cheap to settle, and it either confirms or overturns correction 7.

## Decisions not yet made

- **Own binary or a mode inside gorilla-opencode.** See the end of
  `03-design-burst-carousel.md`. This decides packaging, download size and
  audience, so it should be settled before code is written, not after.
- **Whether phones are in scope.** Mode B excludes them completely. If a phone
  has to be able to receive, Mode A is the only option and item 2 becomes
  load-bearing.
- **Whether to implement the LocalSend protocol for interoperability**, so the
  tool can also talk to real LocalSend apps, or to ignore it entirely and be a
  separate thing. Interoperability costs protocol compatibility work but means
  not having to write both ends.

---

# Added 2026-08-22 (later): cross-platform constraints

See `06-device-constraints.md` for the reasoning. Items 1 to 6 above are
unchanged and still stand, but **item 7 now outranks item 1**: there is no point
measuring multicast rates for a design that cannot reach the devices in the
room.

## 7. Confirm the Go platform support cutoffs

**[inference, unverified]** Go dropped Windows XP and Vista at 1.11, and dropped
Windows 7 and 8 at 1.21. If Windows 7 is in scope, the toolchain is pinned to Go
1.20 from day one, which is a decision with a long tail: no newer stdlib, no
newer crypto, no newer runtime.

Check against current Go release notes. Ten minute job, and it constrains
everything built afterwards.

## 8. Decide whether the sender may assume decent hardware

The asymmetric design in `06` puts every hard requirement on the sender and
makes receivers as dumb as possible. That only holds if the sender can be one
machine you control: a teacher's laptop or the one good phone.

If the sender must also run on a 1 GB Android Go device, the compression
settings, the fountain encoder and the hotspot strategy all change. **Settle
this before writing code, not after.**

## 9. Fountain decoder working set on a 1 GB device

Either measure it, or design the decoder disk-backed from the start and skip the
measurement. The second option is probably correct: a RAM-backed decoder that
works on the development machine and dies on an Android Go phone in a classroom
is the worst possible place to discover the limit.

## Revised order

1. Item 7, Go cutoffs. Cheap, and it gates the toolchain.
2. Item 8, sender hardware assumption. Free, it is a decision not a measurement.
3. Item 1, broadcast throughput at default basic rates. Still the measurement
   that kills or saves the carousel, but only for Tier 1 devices now.
4. Item 2, hostapd basic_rates.
5. Everything else as previously listed.

## What is no longer worth measuring first

**Item 4, injection and monitor capture**, is demoted rather than dropped. Mode B
is Linux-to-Linux only and cannot be the architecture, so confirming ath9k's
injection behaviour is now optimisation work, not feasibility work.

---

# ANSWERED 2026-08-24: items 2 and 7

Full detail and captures in `bench/results-26-08-24-12-20.md`.

**Item 2, hostapd basic_rates.** Answered, and it is the difference between a
good idea and a bad one. Measured on this AR9485: a default `hw_mode=g` AP
advertises 1/2/5.5/11 as basic and beacons at **1.0 Mbps**. `basic_rates=120
240` gives **12 Mbps**. `basic_rates=240` gives **24 Mbps**. A 24x range, set
by one config line.

Consequence for item 1: at 1 Mbps the carousel loses to plain unicast until
about forty-three receivers, so the default configuration would have made the
whole design worse than brute force. At 24 Mbps it wins from the second or
third laptop. Still `[inference]`, being arithmetic over the measured rate.

What remains open inside item 1: the rate was measured on BEACONS, not on
group-addressed DATA frames, because an AP with no associated station sends no
broadcast. 802.11 requires both to use the basic rate set, but this session did
not measure it. One phone joining the AP closes it. No router involved.

**Item 7, Go platform cutoffs.** Both halves of the assumption confirmed
against Go's own documents:

- Go 1.11 release notes: "Go 1.11 now requires OpenBSD 6.2 or later, macOS
  10.10 Yosemite or later, or Windows 7 or later; support for previous versions
  of these operating systems has been removed." So XP and Vista ended at 1.11.
- go.dev/wiki/MinimumRequirements: "For Go 1.21 and later: Windows 10 and
  higher or Windows Server 2016 and higher." So 7 and 8 ended at 1.21.

So the note was right: shipping to Windows 7 means pinning to Go 1.20, which
went out of security support in August 2024. That is a real cost, not a
formality.

**But "unsupported" may not mean "will not run", and that is worth checking
before accepting the pin.** `[measured]` A Go 1.24 Windows binary built from
this estate statically imports exactly one DLL, `kernel32.dll`, and 46
functions from it, none of which is Windows 8 or 10 only. Everything else Go
needs is resolved at runtime with `GetProcAddress`, which is why the 1.24
runtime still carries "Running on Windows 7, where we don't need it anyway"
fallback branches. `[inference, unverified]` A current-Go binary may therefore
start and run on Windows 7 SP1 while being formally unsupported.

Checking it costs one Windows 7 VM and would remove the toolchain pin
entirely. Until someone runs it, treat Windows 7 as unsupported and do not
plan around a maybe.
