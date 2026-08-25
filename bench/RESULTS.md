<!-- Version: 1.0.0 · updated 26-08-25-15-10 -->
# Results, at a glance

Every number here was measured on one machine: a Sony VAIO SVE from 2012, Intel
i7-3632QM, with a **1x1 Atheros AR9485** wifi card. It is deliberately the worst
hardware we could test on.

No number on this page comes from the tool reporting on itself. They come from
the kernel's counters, the wifi driver, and independent tools.

---

## The headline

**72.8% of the radio's physical ceiling, sustained.**

| | measured | of the 72.2 Mbit/s ceiling |
|---|---|---|
| **this tool, four connections** | **6.57 MB/s** = 52.56 Mbit/s | **72.8%** |
| this tool, best second | 7.04 MB/s = 56.32 Mbit/s | 78.0% |
| speedtest.net over the ISP | 5.36 MB/s = 42.86 Mbit/s | 59.4% |

The ceiling is not a guess. `iw` reports the negotiated rate as **72.2 Mbit/s,
MCS 7, one spatial stream, 20 MHz, short guard interval**. That is the fastest
this silicon can transmit, and it is what the card actually negotiated.

Around 60% of the PHY rate is normal TCP efficiency over 802.11n, and that is
what the internet path delivers. Over its own hotspot this tool holds 73%.

**The arithmetic, so you can check it:** 6.57 x 8 = 52.56 Mbit/s.
52.56 / 72.2 = 0.728.

---

## Why more connections do not help

Swept back to back over the real link, same file, each run a separate process.

| connections | mean MB/s | median | peak | sd |
|---|---|---|---|---|
| 1 | 6.51 | 6.65 | 7.00 | 0.61 |
| 2 | 6.55 | 6.66 | 7.04 | 0.50 |
| **4** | **6.57** | 6.64 | 7.04 | **0.45** |
| 8 | 6.36 | 6.44 | 7.02 | 0.58 |
| 16 | 6.14 | 6.37 | 6.81 | 0.78 |
| 32 | 6.20 | 6.46 | 6.91 | 0.76 |

**The peak is 7.0 in every single row.** The limit is airtime. Threading does
not create more of it, and past four connections the mean falls while the
variance rises.

Four is the default because it has the best mean and the lowest variance. It
exists for resilience and granularity, not for speed.

**A benchmark on loopback would have chosen the opposite.** There, one worker
gives 158 MB/s and two give 476, because the bottleneck is one thread copying
bytes rather than the air. Measuring on the wrong medium would have shipped 32
workers and made every classroom slower.

---

## What it costs

| | |
|---|---|
| binary | 733,256 bytes, no dependencies |
| Debian package | 967,476 bytes |
| non-streaming vs SSE | 27x cheaper per token, measured |
| download at 8 KB/s | under two minutes |

---

## Real transfers

Runs measured with `bench/transfer-watch.py`, which samples the kernel's own
counters once a second and never asks the hub how it thinks it is doing.

<!-- REAL-TRANSFERS -->
### 25 August 2026: 7.56 GB to a Windows laptop, browser only

The whole 68,153-file folder as one GET EVERYTHING download, Edge on the far
side, nothing installed there. Measured from the kernel's counters and a
headers-only packet capture, not from the tool's own opinion of itself.

| | |
|---|---|
| carried | **7,560,168,427 bytes** in 1,626 moving seconds |
| mean while moving | **4.65 MB/s**, sd 0.91, best second 5.64 |
| wifi retries | 92,427 of 4.9M frames = **1.87%** |
| TCP retransmissions | 5,930 of 1.9M segments = **0.31%** |
| receiver stalls | **zero** zero-window, zero window-full events |
| ACK latency under load | 20.1 ms mean, 78 ms worst |
| the serving laptop | 4% mean CPU, 66 C peak, **no throttling** |

Every suspect was checked and exonerated by name: the air is clean, the
receiver never said stop, Defender was switched off mid-run with no change,
and the 2012 laptop was loafing. What remains is the honest finding:

**A browser's single TCP connection pays about a 30% tax on this radio.**
4.65 MB/s against the 6.57 this tool's own four-connection client gets from
the same air. One stream at 20 ms of queue simply keeps less data in flight
than four streams do, and no setting on either side changes that. The tool
exists for exactly this reason; the button exists for the person who cannot
install it.

Also on the record: the Windows laptop spent the entire lesson trying to
reach Microsoft, Akamai and Facebook through the classroom hotspot, dozens
of doomed HTTPS attempts a minute. Byte-wise it is nothing. It is worth
knowing whose traffic a classroom access point carries.

Chart and per-second data: `results/transfer-26-08-25-16-20-runA.md`
<!-- /REAL-TRANSFERS -->

---

## What is NOT proven

Stated here rather than left for somebody to discover.

- **One radio, one room, one morning.** Everything above is a single 2012 card
  in one building. It has never been measured at distance, through a wall, or
  with thirty devices associated at once.
- **The tuning trade is not obviously right.** Short guard interval improved the
  median 10% and made the fifth percentile 15% worse, with variance doubling.
  For a classroom, where everybody finishing matters more than anybody finishing
  fast, that may be the wrong trade. Never tested at distance.
- **The device-to-device figure is an inference.** A transfer through a shared
  access point crosses the air twice on one channel, so it should land near half
  the one-hop rate, roughly 2.6 MB/s. Not directly measured.
- **The wifi password change has never dropped a real room.**

Working notes, source reading and the corrections are in this folder:
[01-how-localsend-works.md](01-how-localsend-works.md),
[02-measurements.md](02-measurements.md),
[04-corrections.md](04-corrections.md),
[05-open-questions.md](05-open-questions.md).
