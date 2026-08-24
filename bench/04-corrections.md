<!-- Version: 1.0.0 · updated 26-08-22-16-50 -->
# Corrections log

Claims made during the session on 2026-08-22 that turned out to be wrong, kept
next to what replaced them. The wrong versions are left in deliberately: a
correction that hides the original destroys the record of what changed.

## Assistant's errors

### 1. "The multicast crossover is around three or four receivers"

**Wrong.** Corrected to **roughly seven**. The first estimate ignored that Wi-Fi
transmits multicast and broadcast frames at a low basic rate, typically 1 to 6
Mbps, rather than at the ~43 Mbps a unicast link negotiates. One broadcast pass
can therefore cost as much airtime as six or seven unicast transfers.

Still `[inference]`, still unmeasured. Listed in `05-open-questions.md` item 1
because it is the number the whole design hangs on.

### 2. "Crank compression to -19 --long"

**Directionally right, but the margin was overstated.** Measured on a real
payload, `--fast=4` to `-19 --long` saves 0.34 seconds of airtime per
transmission for a one-time cost of 8.5 seconds. Break-even is about 25
transmissions, not "obviously worth it". And on already-compressed media the
saving is zero, so the higher level is pure waste.

Replaced by the per-situation table in `03-design-burst-carousel.md`, with
`zstd -12` as the likely default.

### 3. "iw: command not found"

**Wrong.** `iw` 6.9-1 is installed at `/sbin/iw`. `command -v iw` returns
nothing for a normal user because `/sbin` is not on the PATH. Checking a binary
with `command -v` alone gives false negatives for anything in `sbin`.

Cost: the card's full interface-mode list, including `monitor` and `outside
context of a BSS`, was not discovered until much later in the session. That
turned out to be the single most useful fact found all day.

**Generalisation:** verify a package with `dpkg -l` or a direct path check, not
with `command -v`.

## Owner's claims corrected by measurement

### 4. "My Wi-Fi card easily hits 150-300 Mbps"

**Measured: 72.2 Mbit/s PHY, 42.86 Mbps real.** MCS 7, one spatial stream, 20
MHz, short guard interval. 42.86 is 59 percent of the PHY rate, which is normal
TCP efficiency. The card is performing to specification; it is simply a 1x1
radio.

The underlying argument was unaffected. 42.86 Mbps against Kurier's 1 kbps is
still a factor of about 42,000, and newer laptops genuinely do reach 150 to 300
Mbps. The claim was right about the class of hardware and wrong about this
specific 2012 card.

### 5. "sudo apt install isa-l"

**Package is named `isal`**, version 2.31.1-1.1 in Trixie main. Related:
`libisal2`, `libisal-dev`, `python3-isal`.

### 6. "ifname wlan0"

**This machine's interface is `Gorilla.WIFI`.** Any command written against
`wlan0` fails here.

### 7. "igzip hardware acceleration will make this fly"

**Optimises the wrong stage.** `zstd --fast=4` already runs at 442 MB/s against
a radio that carries 5.36 MB/s. Compression is 82 times faster than the pipe and
accounts for three percent of the job. Making it infinitely fast saves 0.04
seconds out of 1.47.

Worse, igzip is DEFLATE tuned for throughput, so its ratio lands at or below
`gzip -1`, measured here at 0.455 against `zstd -12` at 0.372. Choosing it means
transmitting roughly 22 percent more bytes to save time on a stage that does not
matter. Ratio is the only axis that converts to saved airtime.

Two parts of the same idea were right and are kept: `/dev/shm` genuinely helps,
by keeping a spinning laptop disk out of the prep stage, and the `tar | zstd |
nc` pipeline is the correct fast path for a one-to-one transfer.

**[inference]** The igzip ratio claim is reasoning from `gzip -1` as a proxy.
The `isal` package is not installed, so it has not been measured directly.
`bench/compression-bench.sh` already includes an igzip row that will populate
itself once the package is installed.

### 8. "I have a feeling I could go 10.0 GB/s"

True of the compressor in RAM. Irrelevant to the transfer. At 10 GB/s the CPU
would be about 1,900 times faster than the radio and would sit idle 99.95
percent of the time waiting for the antenna.
