#!/usr/bin/env python3
# Version: 1.0.0 · updated 26-08-25-15-00
"""
transfer-watch.py - measure a real transfer over the hotspot, from the kernel's
own counters rather than from anything this project reports about itself.

Run it, do the transfer, press Ctrl-C. It writes a CSV of every second, a
markdown report and an SVG chart.

WHY THE KERNEL'S COUNTERS. The hub prints its own throughput, and a tool
grading its own homework is not evidence. Everything sampled here comes from
somewhere the hub cannot influence:

  /sys/class/net/<iface>/statistics/   bytes, packets, errors, drops
  iw dev <iface> station dump          per client: signal, bitrate, RETRIES
  /proc/stat                           CPU actually used
  coretemp hwmon                       what it did to the laptop
  intel-rapl energy_uj                 what it cost in watts
  cpu*/thermal_throttle                whether the machine had to slow down

The wifi retry count is the one that matters most for "is this hotspot stable".
Throughput can look fine while the radio retransmits a third of its frames, and
that is the state that collapses when a second device joins.

USAGE
    python3 bench/transfer-watch.py --label "2.66 GB archive to Windows"
    python3 bench/transfer-watch.py --label "..." --capture      # + tshark

--capture adds a packet capture, which needs sudo and gives TCP retransmission
figures. It records HEADERS ONLY (96-byte snaplen), so no file content and no
message content is written to disk. On an access point you are carrying every
connected device's traffic; capturing their payloads is not yours to do.
"""

import argparse
import csv
import glob
import os
import re
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
IW = shutil.which("iw") or "/usr/sbin/iw"


def read(path, default=None):
    """Read one sysfs value. Absent is a blank column, never a crash: this runs
    on machines that do not have a Sony fan or an Intel RAPL counter."""
    try:
        with open(path) as fh:
            return fh.read().strip()
    except OSError:
        return default


def read_int(path, default=None):
    v = read(path)
    try:
        return int(v)
    except (TypeError, ValueError):
        return default


def find_ap_interface():
    """The interface serving the hotspot.

    Asked for, not assumed. NetworkManager's shared mode uses 10.42.0.1 in
    practice but that is a default, not a promise, and this machine's wifi is
    named Gorilla.WIFI rather than wlan0.
    """
    out = subprocess.run(["ip", "-4", "-o", "addr", "show"],
                         capture_output=True, text=True).stdout
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 4 and parts[3].startswith("10.42."):
            return parts[1]
    # Fall back to whichever wifi device NetworkManager knows about.
    out = subprocess.run(["nmcli", "-t", "-f", "DEVICE,TYPE", "device"],
                         capture_output=True, text=True).stdout
    for line in out.splitlines():
        if line.endswith(":wifi"):
            return line.split(":")[0]
    return None


def net_counters(iface):
    base = "/sys/class/net/{}/statistics/".format(iface)
    keys = ["tx_bytes", "rx_bytes", "tx_packets", "rx_packets",
            "tx_errors", "rx_errors", "tx_dropped", "rx_dropped"]
    return {k: read_int(base + k, 0) for k in keys}


def stations(iface):
    """Per-connected-device radio statistics.

    tx retries and tx failed are the honest measure of a wifi link. A retry is
    a frame the radio had to send again; the count rising fast under load is
    what a marginal link looks like from the inside, long before throughput
    drops enough for anybody to notice.
    """
    try:
        out = subprocess.run([IW, "dev", iface, "station", "dump"],
                             capture_output=True, text=True, timeout=3).stdout
    except (OSError, subprocess.SubprocessError):
        return []
    out_list, cur = [], None
    for line in out.splitlines():
        line = line.strip()
        m = re.match(r"Station ([0-9a-f:]{17})", line)
        if m:
            cur = {"mac": m.group(1)}
            out_list.append(cur)
            continue
        if cur is None or ":" not in line:
            continue
        key, _, val = line.partition(":")
        key, val = key.strip(), val.strip()
        if key == "signal":
            cur["signal_dbm"] = _first_number(val)
        elif key == "tx bitrate":
            cur["tx_mbps"] = _first_number(val)
        elif key == "rx bitrate":
            cur["rx_mbps"] = _first_number(val)
        elif key == "tx retries":
            cur["tx_retries"] = _first_number(val)
        elif key == "tx failed":
            cur["tx_failed"] = _first_number(val)
        elif key == "tx packets":
            cur["tx_packets"] = _first_number(val)
        elif key == "expected throughput":
            cur["expected_mbps"] = _first_number(val)
    return out_list


def _first_number(text):
    m = re.search(r"-?\d+(?:\.\d+)?", text)
    return float(m.group(0)) if m else None


def cpu_busy():
    """Fraction of a core-second spent NOT idle, as raw jiffies to difference."""
    line = read("/proc/stat", "").splitlines()[0] if read("/proc/stat") else ""
    f = [int(x) for x in line.split()[1:] if x.isdigit()]
    if len(f) < 4:
        return None, None
    idle = f[3] + (f[4] if len(f) > 4 else 0)
    return sum(f), idle


def temps():
    vals = []
    for p in glob.glob("/sys/devices/platform/coretemp.0/hwmon/hwmon*/temp*_input"):
        v = read_int(p)
        if v:
            vals.append(v / 1000.0)
    return max(vals) if vals else None


def rapl_energy_uj():
    for d in glob.glob("/sys/class/powercap/intel-rapl:0"):
        v = read_int(os.path.join(d, "energy_uj"))
        if v is not None:
            return v
    return None


def throttle_count():
    total = 0
    found = False
    for p in glob.glob("/sys/devices/system/cpu/cpu*/thermal_throttle/core_throttle_count"):
        v = read_int(p)
        if v is not None:
            total += v
            found = True
    pkg = read_int("/sys/devices/system/cpu/cpu0/thermal_throttle/package_throttle_count")
    if pkg is not None:
        total += pkg
        found = True
    return total if found else None


def fan_percent():
    v = read_int("/sys/devices/platform/sony-laptop/fanspeed")
    return v


def start_capture(iface, outdir):
    """Headers only, size-capped, and it must never fill the disk.

    dumpcap cannot write under /home here (it drops privileges and cannot
    traverse), so the capture lands in /tmp and is moved afterwards.
    """
    tmp = Path("/tmp/hub-transfer-capture.pcapng")
    if tmp.exists():
        tmp.unlink()
    cmd = ["sudo", "-n", "dumpcap", "-i", iface,
           "-s", "96",             # headers only: no payload ever touches disk
           "-b", "filesize:51200",  # 50 MB per file
           "-b", "files:6",         # 300 MB ceiling, then it recycles
           "-w", str(tmp)]
    try:
        proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    except OSError as exc:
        print("  [!] could not start dumpcap: {}".format(exc))
        return None, None
    time.sleep(1.5)
    if proc.poll() is not None:
        err = proc.stderr.read().decode(errors="replace").strip()
        print("  [!] dumpcap stopped straight away: {}".format(err.splitlines()[-1] if err else "?"))
        return None, None
    return proc, tmp


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--label", default="transfer", help="what this run is, for the report")
    ap.add_argument("--seconds", type=int, default=0, help="stop after N seconds (0 = until Ctrl-C)")
    ap.add_argument("--interval", type=float, default=1.0, help="sample interval, seconds")
    ap.add_argument("--iface", help="override the interface to watch")
    ap.add_argument("--capture", action="store_true", help="also capture packet headers (needs sudo)")
    ap.add_argument("--outdir", default=str(HERE / "results"), help="where to write the results")
    args = ap.parse_args()

    iface = args.iface or find_ap_interface()
    if not iface:
        print("\n  No interface found. Is the hotspot up?\n"
              "  Start the lesson first, then run this.\n", file=sys.stderr)
        raise SystemExit(2)

    outdir = Path(args.outdir)
    outdir.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime("%y-%m-%d-%H-%M")
    csv_path = outdir / "transfer-{}.csv".format(stamp)

    print("\n  watching {}   sampling every {:.0f}s".format(iface, args.interval))
    print("  writing  {}".format(csv_path))

    cap_proc = cap_path = None
    if args.capture:
        cap_proc, cap_path = start_capture(iface, outdir)
        if cap_proc:
            print("  capturing packet HEADERS only (96 bytes), 300 MB ceiling")

    print("\n  Do the transfer now. Press Ctrl-C when it has finished.\n")

    fields = ["t", "wall", "tx_bytes", "rx_bytes", "tx_Bps", "rx_Bps",
              "tx_packets", "tx_errors", "tx_dropped", "rx_errors", "rx_dropped",
              "clients", "signal_dbm", "tx_mbps", "rx_mbps", "tx_retries",
              "tx_failed", "cpu_pct", "temp_c", "watts", "fan_pct", "throttles"]
    rows = []
    stop = {"now": False}
    signal.signal(signal.SIGINT, lambda *a: stop.__setitem__("now", True))

    prev_net = net_counters(iface)
    prev_cpu = cpu_busy()
    prev_energy = rapl_energy_uj()
    prev_st = {s["mac"]: s for s in stations(iface)}
    t0 = time.time()
    started_wall = time.strftime("%Y-%m-%d %H:%M:%S")

    while not stop["now"]:
        time.sleep(args.interval)
        now = time.time()
        elapsed = now - t0

        net = net_counters(iface)
        dt = args.interval
        tx_bps = (net["tx_bytes"] - prev_net["tx_bytes"]) / dt
        rx_bps = (net["rx_bytes"] - prev_net["rx_bytes"]) / dt

        cpu = cpu_busy()
        cpu_pct = None
        if cpu[0] and prev_cpu[0]:
            dtot = cpu[0] - prev_cpu[0]
            didle = cpu[1] - prev_cpu[1]
            cpu_pct = round(100.0 * (dtot - didle) / dtot, 1) if dtot > 0 else None

        energy = rapl_energy_uj()
        watts = None
        if energy is not None and prev_energy is not None and energy >= prev_energy:
            watts = round((energy - prev_energy) / 1e6 / dt, 2)

        st = stations(iface)
        by_mac = {s["mac"]: s for s in st}
        # Retries and failures are counters since association, so report the
        # DELTA. A cumulative number rising is not information; the rate is.
        d_retries = d_failed = 0
        for mac, s in by_mac.items():
            p = prev_st.get(mac, {})
            for key, acc in (("tx_retries", "r"), ("tx_failed", "f")):
                cur, old = s.get(key), p.get(key)
                if cur is not None and old is not None and cur >= old:
                    if acc == "r":
                        d_retries += int(cur - old)
                    else:
                        d_failed += int(cur - old)
        sig = [s["signal_dbm"] for s in st if s.get("signal_dbm") is not None]
        txr = [s["tx_mbps"] for s in st if s.get("tx_mbps") is not None]
        rxr = [s["rx_mbps"] for s in st if s.get("rx_mbps") is not None]

        row = {
            "t": round(elapsed, 1),
            "wall": time.strftime("%H:%M:%S"),
            "tx_bytes": net["tx_bytes"], "rx_bytes": net["rx_bytes"],
            "tx_Bps": int(tx_bps), "rx_Bps": int(rx_bps),
            "tx_packets": net["tx_packets"],
            "tx_errors": net["tx_errors"], "tx_dropped": net["tx_dropped"],
            "rx_errors": net["rx_errors"], "rx_dropped": net["rx_dropped"],
            "clients": len(st),
            "signal_dbm": round(sum(sig) / len(sig), 1) if sig else None,
            "tx_mbps": round(sum(txr) / len(txr), 1) if txr else None,
            "rx_mbps": round(sum(rxr) / len(rxr), 1) if rxr else None,
            "tx_retries": d_retries, "tx_failed": d_failed,
            "cpu_pct": cpu_pct, "temp_c": temps(), "watts": watts,
            "fan_pct": fan_percent(), "throttles": throttle_count(),
        }
        rows.append(row)

        best = max(tx_bps, rx_bps)
        print("\r  {:>5.0f}s  out {:>7.2f} MB/s  in {:>7.2f} MB/s  "
              "{} client(s)  {} dBm  retries {:>4}  cpu {:>5}%  {:>4}C  "
              .format(elapsed, tx_bps / 1e6, rx_bps / 1e6, len(st),
                      row["signal_dbm"] if row["signal_dbm"] is not None else "  ?",
                      d_retries,
                      row["cpu_pct"] if row["cpu_pct"] is not None else "?",
                      int(row["temp_c"]) if row["temp_c"] else "?"),
              end="", flush=True)

        prev_net, prev_cpu, prev_energy, prev_st = net, cpu, energy, by_mac
        if args.seconds and elapsed >= args.seconds:
            break

    print("\n")
    if cap_proc:
        cap_proc.send_signal(signal.SIGINT)
        try:
            cap_proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            cap_proc.kill()

    if not rows:
        print("  Nothing was sampled.\n")
        raise SystemExit(1)

    with open(csv_path, "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=fields)
        w.writeheader()
        w.writerows(rows)

    moved = collect_capture(cap_path, outdir, stamp) if cap_path else None
    report(rows, outdir, stamp, args.label, iface, started_wall, moved)


def collect_capture(tmp, outdir, stamp):
    """Move the capture out of /tmp and take ownership of it."""
    found = sorted(glob.glob("/tmp/hub-transfer-capture*.pcapng"))
    if not found:
        return None
    dest = outdir / "capture-{}".format(stamp)
    dest.mkdir(exist_ok=True)
    moved = []
    for f in found:
        target = dest / os.path.basename(f)
        subprocess.run(["sudo", "-n", "mv", f, str(target)], check=False)
        subprocess.run(["sudo", "-n", "chown", "{}:{}".format(os.getuid(), os.getgid()),
                        str(target)], check=False)
        moved.append(target)
    return moved


def tcp_stats(pcaps):
    """Retransmissions, as a share of TCP segments. The number a network
    engineer asks for, and one this project cannot fake."""
    if not pcaps or not shutil.which("tshark"):
        return None
    total = retrans = 0
    for p in pcaps:
        try:
            out = subprocess.run(
                ["tshark", "-r", str(p), "-q", "-z", "io,stat,0,tcp,tcp.analysis.retransmission"],
                capture_output=True, text=True, timeout=600).stdout
        except (OSError, subprocess.SubprocessError):
            continue
        nums = re.findall(r"\|\s*(\d+)\s*\|\s*\d+\s*\|\s*(\d+)\s*\|", out)
        for a, b in nums:
            total += int(a)
            retrans += int(b)
    if total == 0:
        return None
    return {"segments": total, "retransmissions": retrans,
            "percent": round(100.0 * retrans / total, 4)}


def report(rows, outdir, stamp, label, iface, started, pcaps):
    def col(name):
        return [r[name] for r in rows if r[name] is not None]

    tx = [r["tx_Bps"] for r in rows]
    rx = [r["rx_Bps"] for r in rows]
    # Whichever direction actually carried the transfer.
    out_direction = sum(tx) >= sum(rx)
    moving = tx if out_direction else rx
    # Idle seconds at either end would drag the mean down and understate the
    # link. Only seconds that actually carried data count towards the rate.
    active = [v for v in moving if v > 100_000]
    total_bytes = sum(moving)
    dur = rows[-1]["t"]

    temps_c = col("temp_c")
    watts = col("watts")
    sig = col("signal_dbm")
    txr = col("tx_mbps")
    retries = sum(r["tx_retries"] for r in rows)
    failed = sum(r["tx_failed"] for r in rows)
    pkts = rows[-1]["tx_packets"] - rows[0]["tx_packets"]
    errs = (rows[-1]["tx_errors"] - rows[0]["tx_errors"]) + (rows[-1]["rx_errors"] - rows[0]["rx_errors"])
    drops = (rows[-1]["tx_dropped"] - rows[0]["tx_dropped"]) + (rows[-1]["rx_dropped"] - rows[0]["rx_dropped"])
    thr0, thr1 = rows[0]["throttles"], rows[-1]["throttles"]

    def mean(v):
        return sum(v) / len(v) if v else 0

    def stdev(v):
        if len(v) < 2:
            return 0
        m = mean(v)
        return (sum((x - m) ** 2 for x in v) / (len(v) - 1)) ** 0.5

    tcp = tcp_stats(pcaps)

    md = outdir / "transfer-{}.md".format(stamp)
    L = []
    L.append("# Transfer over the hotspot: {}".format(label))
    L.append("")
    L.append("Measured {}, interface `{}`, sampled every second from the kernel's".format(started, iface))
    L.append("own counters. The hub reports nothing here about itself.")
    L.append("")
    L.append("## What moved")
    L.append("")
    L.append("| | |")
    L.append("|---|---|")
    L.append("| bytes carried | **{:,}** ({:.2f} GB) |".format(int(total_bytes), total_bytes / 1e9))
    L.append("| duration | {:.0f} s ({:.1f} min) |".format(dur, dur / 60))
    L.append("| seconds actually moving data | {} of {} |".format(len(active), len(rows)))
    L.append("| mean while moving | **{:.2f} MB/s** |".format(mean(active) / 1e6))
    L.append("| standard deviation | {:.2f} MB/s |".format(stdev(active) / 1e6))
    L.append("| slowest second | {:.2f} MB/s |".format(min(active) / 1e6 if active else 0))
    L.append("| fastest second | {:.2f} MB/s |".format(max(active) / 1e6 if active else 0))
    L.append("")
    L.append("## Was the radio healthy")
    L.append("")
    L.append("| | |")
    L.append("|---|---|")
    L.append("| frames sent | {:,} |".format(pkts))
    L.append("| wifi retries | {:,}{} |".format(
        retries, "  ({:.2f}% of frames)".format(100.0 * retries / pkts) if pkts else ""))
    L.append("| wifi frames given up on | {:,} |".format(failed))
    L.append("| interface errors | {} |".format(errs))
    L.append("| interface drops | {} |".format(drops))
    if sig:
        L.append("| signal | {:.0f} dBm mean, {:.0f} worst |".format(mean(sig), min(sig)))
    if txr:
        L.append("| negotiated rate | {:.0f} Mbit/s mean, {:.0f} worst |".format(mean(txr), min(txr)))
    if tcp:
        L.append("| TCP segments | {:,} |".format(tcp["segments"]))
        L.append("| TCP retransmissions | {:,} (**{}%**) |".format(tcp["retransmissions"], tcp["percent"]))
    L.append("")
    L.append("A retry is a frame the radio had to send again. The rate matters more")
    L.append("than the total: a link can hold its throughput while retransmitting")
    L.append("heavily, and that is the state that collapses when another device joins.")
    L.append("")
    L.append("## What it cost the laptop")
    L.append("")
    L.append("| | |")
    L.append("|---|---|")
    if temps_c:
        L.append("| CPU temperature | {:.0f} C start, {:.0f} C peak |".format(temps_c[0], max(temps_c)))
    if watts:
        L.append("| CPU package power | {:.1f} W mean, {:.1f} W peak |".format(mean(watts), max(watts)))
    cpu = col("cpu_pct")
    if cpu:
        L.append("| CPU busy | {:.0f}% mean, {:.0f}% peak |".format(mean(cpu), max(cpu)))
    fan = col("fan_pct")
    if fan:
        L.append("| fan | {:.0f}% mean, {:.0f}% peak |".format(mean(fan), max(fan)))
    if thr0 is not None and thr1 is not None:
        L.append("| thermal throttling | {} |".format(
            "**none**" if thr1 == thr0 else "**{} events during the run**".format(thr1 - thr0)))
    L.append("")
    L.append("![throughput](transfer-{}.svg)".format(stamp))
    L.append("")
    L.append("Raw per-second samples: `transfer-{}.csv`".format(stamp))
    if pcaps:
        L.append("")
        L.append("Packet headers (96-byte snaplen, no payload): `capture-{}/`".format(stamp))
    md.write_text("\n".join(L) + "\n", encoding="utf-8")

    svg(rows, outdir / "transfer-{}.svg".format(stamp), out_direction)

    print("  {:,} bytes ({:.2f} GB) in {:.0f}s".format(int(total_bytes), total_bytes / 1e9, dur))
    print("  mean while moving: {:.2f} MB/s   sd {:.2f}".format(mean(active) / 1e6, stdev(active) / 1e6))
    print("  wifi retries: {:,}   given up on: {:,}   errors: {}   drops: {}".format(retries, failed, errs, drops))
    if tcp:
        print("  TCP retransmissions: {:,} of {:,} ({}%)".format(
            tcp["retransmissions"], tcp["segments"], tcp["percent"]))
    if thr0 is not None:
        print("  thermal throttling: {}".format("none" if thr1 == thr0 else thr1 - thr0))
    print("\n  report: {}\n".format(md))


def svg(rows, path, out_direction):
    """A chart with no dependency and no external font.

    Explicit colours on an explicit background, because GitHub renders SVG on
    both a light and a dark page and a chart that assumes one is unreadable on
    the other.
    """
    W, H, PAD = 900, 320, 52
    key = "tx_Bps" if out_direction else "rx_Bps"
    vals = [r[key] / 1e6 for r in rows]
    temps_c = [r["temp_c"] if r["temp_c"] else 0 for r in rows]
    if not vals:
        return
    vmax = max(max(vals), 0.1) * 1.15
    tmin, tmax = 30.0, max(max(temps_c), 60.0) + 5
    n = len(vals)

    def x(i):
        return PAD + (W - PAD - 20) * (i / max(n - 1, 1))

    def y(v):
        return H - PAD - (H - PAD - 20) * (v / vmax)

    def yt(v):
        return H - PAD - (H - PAD - 20) * ((v - tmin) / (tmax - tmin))

    parts = ['<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" '
             'viewBox="0 0 {} {}" font-family="monospace" font-size="11">'.format(W, H, W, H)]
    parts.append('<rect width="{}" height="{}" fill="#ffffff"/>'.format(W, H))
    # gridlines and the throughput scale
    for frac in (0, 0.25, 0.5, 0.75, 1.0):
        v = vmax * frac
        yy = y(v)
        parts.append('<line x1="{}" y1="{:.1f}" x2="{}" y2="{:.1f}" stroke="#dddddd"/>'.format(PAD, yy, W - 20, yy))
        parts.append('<text x="{}" y="{:.1f}" fill="#555555" text-anchor="end">{:.1f}</text>'.format(PAD - 6, yy + 4, v))
    parts.append('<text x="{}" y="16" fill="#1a6b1a">MB/s over the wifi</text>'.format(PAD))
    parts.append('<text x="{}" y="16" fill="#b34700" text-anchor="end">CPU temperature C</text>'.format(W - 20))
    # temperature first, so throughput draws over it
    tpts = " ".join("{:.1f},{:.1f}".format(x(i), yt(t)) for i, t in enumerate(temps_c) if t)
    if tpts:
        parts.append('<polyline points="{}" fill="none" stroke="#b34700" stroke-width="1.2" opacity="0.75"/>'.format(tpts))
    pts = " ".join("{:.1f},{:.1f}".format(x(i), y(v)) for i, v in enumerate(vals))
    parts.append('<polyline points="{}" fill="none" stroke="#1a6b1a" stroke-width="1.6"/>'.format(pts))
    # seconds along the bottom
    for i in range(0, n, max(n // 8, 1)):
        parts.append('<text x="{:.1f}" y="{}" fill="#555555" text-anchor="middle">{:.0f}s</text>'.format(
            x(i), H - PAD + 18, rows[i]["t"]))
    parts.append('<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="#888888"/>'.format(PAD, H - PAD, W - 20, H - PAD))
    parts.append("</svg>")
    path.write_text("\n".join(parts), encoding="utf-8")


if __name__ == "__main__":
    main()
