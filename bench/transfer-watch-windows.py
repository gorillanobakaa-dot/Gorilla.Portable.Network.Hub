#!/usr/bin/env python3
# Version: 1.1.0 · updated 26-09-24-10-40
"""
transfer-watch-windows.py - measure a real transfer over the Windows hotspot
from Windows' own counters, never from anything the hub says about itself.

The Windows counterpart of transfer-watch.py (which reads Linux's /sys and
iw). Same outputs: a CSV of every second, a markdown summary, an SVG chart.

WHAT IS SAMPLED, once a second (psutil, which reads Windows' interface table):
  the hotspot's adapter    bytes each way, errors, drops, Windows' link speed
  the wifi card as a whole bytes each way (shows airtime spent elsewhere)
  CPU                      whole machine, the busiest core, and hub.exe
  power                    on mains or battery (Windows throttles wifi on battery)
and every 10 seconds `netsh wlan show interfaces`, the laptop's OWN link to
any other network, because a hotspot shares the card's airtime with it.

WHAT WINDOWS DOES NOT SAY: the rate each phone negotiated with the hotspot.
Read it on the phone while the transfer runs (Android: Settings, Wi-Fi, the
network: "Link speed", or "Receive/Transmit link speed") and pass it:
    --phone-link 286.8        (for a download: the phone's RECEIVE speed)
The summary then gives efficiency = measured / negotiated, the same measure
the Linux results use.

It waits for the hotspot, then for traffic, and stops by itself IDLE seconds
after the traffic ends (or at --max, Ctrl-C, or a file called STOP in --out).

USAGE
    python bench/transfer-watch-windows.py --label "10 GB to phone, 2.4 GHz"
    python bench/transfer-watch-windows.py --label "..." --phone-link 286.8
Needs: pip install psutil
"""

import argparse
import csv
import os
import re
import statistics
import subprocess
import sys
import time
from pathlib import Path

try:
    import psutil
except ImportError:
    sys.exit("This needs psutil:  python -m pip install psutil")

HOTSPOT_ADDR = "192.168.137.1"  # Windows' Mobile Hotspot is always this


def hotspot_nic():
    """The adapter holding 192.168.137.1. Found by address, not by name,
    because Windows numbers these ("Local Area Connection* 2") as it likes."""
    for name, addrs in psutil.net_if_addrs().items():
        if any(a.address == HOTSPOT_ADDR for a in addrs):
            return name
    return None


def card_nic():
    stats = psutil.net_if_stats()
    for name in ("Wi-Fi", "WiFi", "Wireless"):
        if name in stats:
            return name
    return None


def station_link():
    """The laptop's own wifi link to another network, if any."""
    try:
        out = subprocess.run(["netsh", "wlan", "show", "interfaces"], capture_output=True,
                             text=True, timeout=5).stdout
    except (OSError, subprocess.SubprocessError):
        return ""
    if not re.search(r"State\s+:\s*connected", out):
        return "none"
    def f(key):
        m = re.search(key + r"\s+:\s*(.+)", out)
        return m.group(1).strip() if m else "?"
    return "{} ch {} {} Mbit/s".format(f("Band"), f("Channel"), f(r"Receive rate \(Mbps\)"))


def driver_facts():
    try:
        out = subprocess.run(["netsh", "wlan", "show", "drivers"], capture_output=True,
                             text=True, timeout=5).stdout
    except (OSError, subprocess.SubprocessError):
        return {}
    facts = {}
    for key in ("Driver", "Version", "Radio types supported"):
        m = re.search(r"^\s*" + re.escape(key) + r"\s+:\s*(.+)$", out, re.M)
        if m:
            facts[key] = m.group(1).strip()
    return facts


def hotspot_channel():
    """The hub writes the channel the card really broadcasts on to its log."""
    log = Path(os.environ.get("LOCALAPPDATA", "")) / "PortableNetworkHub" / "network.log"
    try:
        lines = log.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return "?"
    for line in reversed(lines):
        m = re.search(r"switched on, (.+ channel \d+)", line)
        if m:
            return m.group(1)
    return "?"


def hub_procs():
    return [p for p in psutil.process_iter(["name"]) if (p.info["name"] or "").lower() == "hub.exe"]


def svg_chart(rates, path, title):
    w, h, pad = 900, 300, 40
    top = max(rates) * 1.1 or 1
    pts = " ".join("{:.1f},{:.1f}".format(pad + i * (w - 2 * pad) / max(len(rates) - 1, 1),
                                          h - pad - r / top * (h - 2 * pad)) for i, r in enumerate(rates))
    grid = "".join('<line x1="{0}" y1="{1:.1f}" x2="{2}" y2="{1:.1f}" stroke="#ddd"/>'
                   '<text x="4" y="{3:.1f}" font-size="11">{4:.0f}</text>'.format(
                       pad, h - pad - v / top * (h - 2 * pad), w - pad, h - pad - v / top * (h - 2 * pad) + 4, v)
                   for v in [top / 4 * k for k in range(5)])
    path.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" font-family="sans-serif">'
                    '<rect width="100%" height="100%" fill="#fff"/>{grid}'
                    '<polyline fill="none" stroke="#1a6b1a" stroke-width="1.5" points="{pts}"/>'
                    '<text x="{pad}" y="20" font-size="14">{t} (MB/s per second)</text></svg>'.format(
                        w=w, h=h, grid=grid, pts=pts, pad=pad, t=title), encoding="utf-8")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--label", default="transfer")
    ap.add_argument("--out", default=os.path.join(os.environ.get("TEMP", "."), "hub-transfer-watch"))
    ap.add_argument("--idle", type=int, default=20, help="seconds of quiet that end the run")
    ap.add_argument("--max", type=int, default=3600)
    ap.add_argument("--threshold", type=float, default=0.5, help="MB/s that counts as transferring")
    ap.add_argument("--phone-link", type=float, default=0.0,
                    help="the phone's negotiated rate in Mbit/s, read on the phone")
    a = ap.parse_args()

    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime("%y-%m-%d-%H-%M-%S")
    csv_path, md_path, svg_path = (out / "transfer-{}.{}".format(stamp, e) for e in ("csv", "md", "svg"))
    stop = out / "STOP"
    if stop.exists():
        stop.unlink()

    print("Waiting for the hotspot (an adapter holding {})...".format(HOTSPOT_ADDR), flush=True)
    t0 = time.time()
    hs = hotspot_nic()
    while not hs and time.time() - t0 < 600:
        time.sleep(1)
        hs = hotspot_nic()
    if not hs:
        sys.exit("No hotspot came up in ten minutes.")
    card = card_nic()
    facts = driver_facts()
    channel = hotspot_channel()
    link_at_start = station_link()
    print("hotspot adapter: {}   card: {}   {}".format(hs, card, channel), flush=True)
    print("laptop's own link to another network: {}".format(link_at_start), flush=True)
    print("Sampling once a second. Start the transfer now.", flush=True)

    ncpu = psutil.cpu_count()
    psutil.cpu_percent(percpu=True)
    procs = hub_procs()
    for p in procs:
        try:
            p.cpu_percent()
        except psutil.Error:
            pass
    prev = psutil.net_io_counters(pernic=True)
    prev_t = time.time()

    rows, active = [], []
    started, idle, sec, link = False, 0, 0, link_at_start
    fields = ["second", "hotspot_sent", "hotspot_recv", "card_sent", "card_recv", "MBps",
              "errors", "drops", "hotspot_speed_mbps", "cpu_pct", "busiest_core_pct",
              "hub_cpu_pct", "on_mains", "station_link"]
    try:
        with open(csv_path, "w", newline="", encoding="utf-8") as fh:
            w = csv.writer(fh)
            w.writerow(fields)
            while sec < a.max:
                time.sleep(max(0.0, 1.0 - (time.time() - prev_t)))
                sec += 1
                now = psutil.net_io_counters(pernic=True)
                t = time.time()
                dt = t - prev_t
                c, p = now.get(hs), prev.get(hs)
                if not c or not p:
                    prev, prev_t = now, t
                    continue
                sent, recv = c.bytes_sent - p.bytes_sent, c.bytes_recv - p.bytes_recv
                mbps = max(sent, recv) / dt / 1e6
                cs = now.get(card) and prev.get(card)
                card_sent = now[card].bytes_sent - prev[card].bytes_sent if cs else ""
                card_recv = now[card].bytes_recv - prev[card].bytes_recv if cs else ""
                errs = (c.errin + c.errout) - (p.errin + p.errout)
                drops = (c.dropin + c.dropout) - (p.dropin + p.dropout)
                st = psutil.net_if_stats().get(hs)
                cores = psutil.cpu_percent(percpu=True)
                hub = 0.0
                if sec % 5 == 1:
                    # The hub may have been restarted since the last look.
                    procs = hub_procs()
                for pr in procs:
                    try:
                        hub += pr.cpu_percent() / ncpu
                    except psutil.Error:
                        pass
                bat = psutil.sensors_battery()
                mains = "" if bat is None else int(bool(bat.power_plugged))
                if sec % 10 == 0:
                    link = station_link()
                row = [sec, sent, recv, card_sent, card_recv, round(mbps, 3), errs, drops,
                       st.speed if st else "", round(sum(cores) / len(cores), 1), max(cores),
                       round(hub, 1), mains, link]
                w.writerow(row)
                fh.flush()
                rows.append(row)
                print("{:5d}s {:8.2f} MB/s  cpu {:4.1f}%  core max {:5.1f}%  hub {:4.1f}%".format(
                    sec, mbps, row[9], row[10], row[11]), flush=True)
                if mbps >= a.threshold:
                    started, idle = True, 0
                    active.append(row)
                elif started:
                    idle += 1
                    if idle >= a.idle:
                        break
                if stop.exists():
                    break
                prev, prev_t = now, t
    except KeyboardInterrupt:
        pass

    if not active:
        print("No transfer was seen.")
        return 1
    rates = [r[5] for r in active]
    sent_total = sum(r[1] for r in active)
    recv_total = sum(r[2] for r in active)
    direction = "laptop to phone (download)" if sent_total >= recv_total else "phone to laptop (upload)"
    s = sorted(rates)
    n = len(s)
    mean, median = statistics.mean(rates), statistics.median(rates)
    p5 = s[int(0.05 * (n - 1))]
    # Windows updates interface counters in bursts, so one second can hold
    # two seconds' bytes: measured 2026-09-24, a "peak second" of 407 Mbit/s
    # on a 287 Mbit/s link. The best 5 seconds is the honest short peak.
    w5 = min(5, n)
    peak = max(statistics.mean(rates[i:i + w5]) for i in range(n - w5 + 1))
    sd = statistics.pstdev(rates)
    # Best sustained 30 seconds: the fairest single figure for "what the radio
    # can hold", without the ramp-up at the start.
    win = min(30, n)
    best30 = max(statistics.mean(rates[i:i + win]) for i in range(n - win + 1))

    def eff(mb):
        if a.phone_link <= 0:
            return ""
        return " | {:.1f}%".format(100 * mb * 8 / a.phone_link)

    # On Windows the card's own counters do NOT include the hotspot's traffic
    # (measured 2026-09-24: the card showed less than the hotspot alone), so
    # this is the laptop's OTHER network, sharing the same radio's airtime.
    other = sum((r[3] or 0) + (r[4] or 0) for r in active)
    md = [
        "# {}".format(a.label), "",
        "Measured {}. From Windows' interface counters, one sample a second.".format(
            time.strftime("%Y-%m-%d %H:%M")), "",
        "- wifi card: {} (driver {}; {})".format(facts.get("Driver", "?"), facts.get("Version", "?"),
                                                facts.get("Radio types supported", "?")),
        "- hotspot: {}".format(channel),
        "- laptop's own link to another network: {}".format(link_at_start),
        "- direction: {}".format(direction),
        "- phone's negotiated rate, read on the phone: {}".format(
            "{} Mbit/s".format(a.phone_link) if a.phone_link else "not given"),
        "- on mains power: {}".format({1: "yes", 0: "NO (battery)", "": "?"}[active[0][12]]), "",
        "| | MB/s | Mbit/s{} |".format(" | of the link" if a.phone_link else ""),
        "|---|---|---{}|".format("|---" if a.phone_link else ""),
    ]
    for name, v in (("mean", mean), ("median", median), ("best 30 s", best30),
                    ("5th percentile", p5), ("best 5 s", peak)):
        md.append("| {} | {:.2f} | {:.1f}{} |".format(name, v, v * 8, eff(v)))
    md += [
        "| std deviation | {:.2f} | |".format(sd), "",
        "- {:.2f} GB moved in {} active seconds".format(max(sent_total, recv_total) / 1e9, n),
        "- errors {}, drops {} on the hotspot adapter".format(sum(r[6] for r in active), sum(r[7] for r in active)),
        "- the laptop's own other network, same radio, during the run: {:.1f} MB".format(other / 1e6),
        "- CPU mean {:.1f}%, busiest core mean {:.1f}%, hub.exe mean {:.1f}% of the machine".format(
            statistics.mean(r[9] for r in active), statistics.mean(r[10] for r in active),
            statistics.mean(r[11] for r in active)), "",
        "Every second: {}. Chart: {}.".format(csv_path.name, svg_path.name),
    ]
    md_path.write_text("\n".join(md) + "\n", encoding="utf-8")
    svg_chart(rates, svg_path, a.label)
    print("\n" + "\n".join(md))
    return 0


if __name__ == "__main__":
    sys.exit(main())
