<!-- Version: 1.0.0 · updated 26-09-24-13-30 -->
# Results on Windows, at a glance

Measured on 24 September 2026 in a field, with no other wifi
infrastructure around: the laptop, one phone, and the phone that gave the
laptop its internet. Every number comes from a counter the hub does not
control: Windows' own interface counters, and the exact byte count the phone
itself received. Nothing here comes from the hub reporting on itself.

The earlier results, on a 2012 Sony laptop running Linux, are in
[RESULTS.md](RESULTS.md). They are true for that laptop. This page is the
same kind of test on a different laptop and a different operating system,
and the two should not be mixed up.

---

## In plain words

- **Fast.** A modern phone two metres away downloaded at **23 MB every
  second**, steadily: a 1 GB video in under 45 seconds. That is about
  **three and a half times** what the 2012 Sony managed.
- **Far.** Walking away to 40 or 50 metres, out through an open door,
  the phone still got **6 to 12 MB every second** (the last seconds of the
  walk). That is the 2012 laptop's best speed, from across a field. (One walk,
  not yet repeated, and the distance is the owner's estimate.)
- **Effortless.** The laptop's processor sat at about 4% busy. The hub itself
  used under 1%.
- **Not 85%.** The goal was 85% of the radio's top speed. The best we measured
  was **67%**. On this laptop the radio is run by Intel's closed driver, which
  decides how it packs and paces the data, and nothing a program can change
  from outside moved it. What we tried, and what is still untried, is below.

---

## The two laptops

| | Sony VAIO SVE14A3AJ (the earlier results) | Lenovo ThinkPad L15 Gen 3 (this page) |
|---|---|---|
| made | 2012 | 2022 |
| processor | Intel Core i7-3632QM, 4 cores, 8 threads | Intel Core i7-1255U, 2 performance + 8 efficient cores, 12 threads |
| memory | 16 GB DDR3L | 64 GB DDR4-3200 |
| storage | Kingston DC600M SATA SSD | 1 TB PCIe 4.0 NVMe SSD |
| **wifi card** | **Atheros AR9485, 802.11n, 1 antenna stream** | **Intel Wi-Fi 6 AX201, 802.11ax, 2 antenna streams** |
| **fastest rate the card offered the phone** | **72.2 Mbit/s** (read with `iw`) | **287 Mbit/s** on 2.4 GHz (Windows' link speed for the hotspot) |
| system | Debian 13, the owner's own kernel | Windows 11 Home |
| who drives the radio | the open `ath9k` driver and `hostapd`: every setting reachable | Intel's closed driver and firmware, through Windows' Mobile Hotspot |

---

## The measurements

The phone: a Samsung Galaxy S24 Ultra, Wi-Fi 6, two metres from the laptop,
both indoors, running `bench/phone-speedtest.html` in Chrome (the
page pulls a big file into memory, counts every byte, and saves nothing). The
network: 2.4 GHz, channel 11, made by the hub.

"Of 287" is efficiency: what arrived, divided by the fastest rate the radio
link offered. It is the same measure the Linux results use.

| run | connections | seconds | the phone received | mean | best 30 s | of 287 (best 30 s) |
|---|---|---|---|---|---|---|
| **baseline** | **1** | 120 | 2,751,427,920 bytes | **22.93 MB/s = 183 Mbit/s** | **23.86 MB/s = 191 Mbit/s** | **66.6%** |
| more connections | 4 | 60 | 1,323,999,288 bytes | 22.04 MB/s = 176 Mbit/s | 22.63 MB/s = 181 Mbit/s | 63.1% |
| more connections, walking away | 8 | 600 | 8,369,217,937 bytes | 13.94 MB/s = 112 Mbit/s | 22.41 MB/s = 179 Mbit/s | 62.5% |
| Throughput Booster ON | 1 | 120 | 2,650,205,584 bytes | 22.08 MB/s = 177 Mbit/s | 23.87 MB/s = 191 Mbit/s | 66.6% |

Earlier, the phone's own browser downloaded the 10.6 GB film normally (its
download manager, not the test page). Windows counted 25.2 MB/s over the best
30 seconds, which is about **23.9 MB/s of file** once headers are taken off:
the same ceiling.

**What these say:**

1. **One connection is best**, as it was on the Sony. More connections share
   the same air and only add overhead.
2. **Throughput Booster** (a setting of Intel's driver, off by default) made
   no difference to the best speed and made it slightly less steady. It was
   put back off.
3. **The ceiling is about 191 Mbit/s**, whatever was changed. In every run the
   laptop was barely working, so the limit is the radio link, not the hub.

---

## For developers: how it was measured, and what to trust

### The instruments

| tool | what it measures |
|---|---|
| `bench/phone-speedtest.html` | served by the hub; on the phone, N HTTP Range requests over one file, read into memory and discarded; counts every byte the browser received. **The ground truth for payload.** |
| `bench/transfer-watch-windows.py` | once a second: Windows' interface counters for the hotspot adapter (found by holding 192.168.137.1), CPU, hub.exe CPU, mains or battery, and `netsh wlan show interfaces` every 10 s. CSV, markdown summary, SVG chart. |
| `bench/download-meter.py` | the same N-range pull from Python. On the hub laptop against 127.0.0.1 it measures the software ceiling. |
| `bench/console-read.cs` | copies the hub window's text, for its per-download log lines, without redirecting its output |

### Can Windows' counter be trusted? Yes, with a known offset

Three runs where the phone reported its exact byte count against Windows' count
over the same seconds:

| phone received | Windows counted out | ratio |
|---|---|---|
| 8,369,217,937 | 8.85 GB | 1.057 |
| 2,751,427,920 | 2.89 GB | 1.050 |
| 1,323,999,288 | 1.42 GB | 1.072 |

Windows counts about 5 to 7% more than the file bytes: IP and TCP headers and
retransmissions, which are real traffic on the air but not file content. The
figures on this page are the **phone's** counts.

**One counter reading is not usable: the single best second.** Windows updates
its counters in bursts, so one second can hold two seconds' bytes. The sampler
once reported 407 Mbit/s in one second on a 287 Mbit/s link, which is
impossible. The sampler now reports the best 5 seconds instead.

**The 14.1 GB for a 10.6 GB film** in the first run was real traffic, not a
miscount: at 1.05 to 1.07 per byte it is about 13.3 GB of payload, so the
phone's download manager fetched roughly 2.8 GB twice. The hub did not log
downloads that broke off; it does now (`broke off after N of M bytes`).

### The software is not the limit

`download-meter.py` against 127.0.0.1, no radio involved: **756 MB/s on one
connection**, 1,319 MB/s on four, with the send loop's 128 KB reads. The radio
carried 24 MB/s. The hub is about thirty times faster than the air it feeds.

### Where "287" comes from, and its limits

Windows does not report the rate each hotspot client negotiated. It does
report a link speed for the hotspot's virtual adapter: 0 with nobody on it,
**287 Mbit/s** with the S24 Ultra connected. 286.8 Mbit/s is 802.11ax, 2
spatial streams, 20 MHz, MCS 11, short guard interval: the top rate for a
2-stream Wi-Fi 6 link on a 20 MHz 2.4 GHz channel, which both ends support.
The phone showed Wi-Fi 6 in its status bar in the first runs. In the
Throughput Booster run, after the card had been reset, the "6" was gone from
the icon while Windows still reported 287 and the speed was unchanged. That is
not explained. The phone's own link-speed field was not visible on this model.

### What was the same in every run, and may have cost speed

- **The laptop was also joined to the phone that gave it internet**, on the
  same channel (11). One radio serves both. Measured traffic on that link
  during the runs: 0.8 to 3.9 MB per run, so its data was negligible, but the
  radio still kept that link alive. In the field the laptop is joined to
  nothing. **Not yet measured that way.**
- **On battery** for most runs (the laptop's power supply was off). The power
  plan was High performance with the wifi power setting at Maximum Performance
  on battery as on mains (`powercfg`), so Windows was not throttling the card
  for power. The one run on mains (8 connections, walking) is not comparable
  for speed.

### Why 85% was not reached [inference]

Every wifi transmission pays a fixed cost in time (waiting for a quiet channel,
the preamble, the acknowledgement) that does not shrink as the data rate grows.
At 72 Mbit/s that cost is a small share of each transmission; at 287 Mbit/s it
is four times larger in proportion. Packing more data into each transmission
(aggregation) is what pays it back, and on Windows the size of those packs is
decided inside Intel's driver and firmware. On Linux, with the open driver,
`hostapd` and a custom kernel, those are settings; here they are not. A rough
estimate puts the protocol's own ceiling for TCP at this rate around 80 to 85%
before any real-world losses. This paragraph is reasoning, not measurement.

### Not yet tried

| | why it matters | needs |
|---|---|---|
| laptop joined to nothing (the field condition) | the radio then serves only the hotspot | a phone, with the laptop offline for the test |
| 5 GHz | a far faster channel (80 MHz, up to 1,201 Mbit/s for two streams) | the same, and phones that can see it |
| MIMO power save off | the card may idle one of its two antennas | an administrator prompt |
| upload (phone to laptop) | handing work in | the same page, sending instead |
| many phones at once | a real class | a real class |

### Reproduce it

1. Put a big file and `bench/phone-speedtest.html` in a folder.
2. `hub serve <folder> --name "Gorilla Hub" --password <8+ characters> --band 2.4 --port 80`
3. `python bench/transfer-watch-windows.py --label "<what this run is>" --phone-link 286.8`
4. On the phone, join the network and open `http://192.168.137.1/phone-speedtest.html`
   (whatever the file is called in the folder), choose connections and seconds,
   press START, and screenshot the result.
5. Compare the phone's "bytes received" with the sampler's GB for the same run.
