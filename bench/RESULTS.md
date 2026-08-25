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
_No full-size transfer has been measured over a live hotspot yet. The next one
lands here, with its chart and its per-second CSV._
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
