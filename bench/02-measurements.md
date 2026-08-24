<!-- Version: 1.0.0 · updated 26-08-22-16-50 -->
# Measurements taken on 2026-08-22

Machine: Sony VAIO SVE, Intel Core i7-3632QM, 4 cores / 8 threads, Debian 13.
Every number below was taken on this machine on this date. The command that
produced it is recorded so it can be re-run and disagreed with.

## CPU

**[measured]**

```
model name : Intel(R) Core(TM) i7-3632QM CPU @ 2.20GHz
flags of interest: aes avx pclmulqdq sse4_2
```

Command: `grep -o -E 'avx2|avx|sse4_2|pclmulqdq|aes|bmi2' /proc/cpuinfo | sort -u`

**No AVX2. No SHA extensions.** Consequences: SHA-256 runs in software, and any
library advertising multi-gigabyte throughput from AVX2 or AVX-512 paths will
fall back to its SSE or AVX path here.

## Wi-Fi hardware

**[measured]**

```
01:00.0 Network controller: Qualcomm Atheros AR9485 Wireless Network Adapter (rev 01)
WIFI-PROPERTIES.2GHZ:   yes
WIFI-PROPERTIES.5GHZ:   no
WIFI-PROPERTIES.AP:     yes
WIFI-PROPERTIES.ADHOC:  yes
```

Command: `lspci | grep -i network` and `nmcli -f WIFI-PROPERTIES dev show Gorilla.WIFI`

Interface is named **`Gorilla.WIFI`**, not `wlan0`. Any command written against
`wlan0` will fail on this machine.

### Supported interface modes

**[measured]** Command: `/sbin/iw phy` (note: `iw` lives in `/sbin`, which is
not on a normal user's PATH. `command -v iw` gives a false negative.)

```
Supported interface modes:
     * IBSS
     * managed
     * AP
     * AP/VLAN
     * monitor
     * P2P-client
     * P2P-GO
     * outside context of a BSS
```

This is the important find of the session. `AP` means this laptop can be the
access point. `monitor` plus `outside context of a BSS` means it can transmit
and receive raw 802.11 frames with no association at all, which is the primitive
the broadcast design needs.

### Live link

**[measured]** Command: `/sbin/iw dev Gorilla.WIFI link`

```
SSID: the office network
freq: 2412.0                          (channel 1)
signal: -59 dBm
rx bitrate: 72.2 MBit/s MCS 7 short GI
tx bitrate: 72.2 MBit/s MCS 7 short GI
```

MCS 7, one spatial stream, 20 MHz. 72.2 Mbit/s is the ceiling of this silicon.

### Throughput

**[measured]** speedtest.net against Exascale London, 2026-08-22:

```
download 42.86 Mbps    upload 15.48 Mbps    ping 4 ms
```

42.86 Mbps = **5.36 MB/s**. That is 59 percent of the 72.2 Mbit/s PHY rate,
which is textbook TCP efficiency over 802.11n. The card is performing to
specification. It is simply a 1x1 radio.

**[inference]** The download figure is air-limited rather than ISP-limited,
because it lands exactly where the PHY rate predicts. Not separately verified.

**[inference]** A device-to-device transfer through a shared access point
crosses the air twice on the same channel, so it should land near half of the
one-hop figure: **roughly 2.6 MB/s**. This matches the "very very slow"
experience that started the session. Not directly measured, listed in
`05-open-questions.md`.

## Software present and absent

**[measured]** Command: `dpkg -l` and direct checks of `/usr/sbin`, `/sbin`

| package | status |
|---|---|
| zstd | 1.5.7+dfsg-1, installed |
| iw | 6.9-1, installed at `/sbin/iw` |
| wpasupplicant | 2:2.10-24, installed |
| dnsmasq | binary present at `/usr/sbin/dnsmasq` |
| tcpdump | installed |
| **hostapd** | **not installed** |
| **isal** | **not installed**, available in Trixie main as `isal` 2.31.1-1.1 |

Note the package name is `isal`, not `isa-l`. Related packages: `libisal2`,
`libisal-dev`, `python3-isal`.

## Compression benchmark

**[measured]** Payload: 16.4 MiB (17,192,960 byte) tar of the LocalSend 1.18.2
source tree, a realistic mix of source, docs and small binaries. All work done
in `/dev/shm` so no disk I/O is in the timing. Script:
`bench/compression-bench.sh`. Full output: `bench/results-26-08-22-16-48.md`.

| method | bytes out | ratio | seconds | MB/s |
|---|---|---|---|---|
| gzip -1 | 7,814,067 | 0.454 | 0.35 | 46.7 |
| zstd --fast=4 -T0 | 8,046,752 | 0.468 | 0.04 | 442.6 |
| zstd -1 -T0 | 7,292,959 | 0.424 | 0.04 | 372.2 |
| zstd -12 -T0 | 6,399,124 | 0.372 | 0.72 | 22.8 |
| zstd -19 --long -T0 | 6,110,512 | 0.355 | 8.56 | 1.9 |
| xz -9 -T0 | 6,052,768 | 0.352 | 5.58 | 2.9 |

### What this table means

**[inference, arithmetic from the measured rows]**

`zstd --fast=4` compresses at 442 MB/s. The radio carries 5.36 MB/s.
**Compression is already 82 times faster than the pipe it feeds.** Compressing
this payload takes 0.04 seconds; transmitting it takes 1.43 seconds. Compression
is three percent of the job. Making it infinitely fast saves 0.04 seconds out of
1.47.

Therefore compression **speed** is not worth optimising in this design.
Compression **ratio** is, because every byte not sent is airtime not spent.

The ratio spread from fastest to slowest is 0.468 to 0.355, which is 24 percent
fewer bytes. In airtime at 5.36 MB/s that is 1.43 s versus 1.09 s, a saving of
**0.34 seconds per transmission**, bought for a one-time cost of 8.5 seconds.
Break-even is about **25 transmissions**.

Note `-12`: it captures 96 percent of the ratio benefit of `-19` for 8 percent
of the CPU time. It is probably the right default.
