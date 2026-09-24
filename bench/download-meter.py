#!/usr/bin/env python3
# Version: 1.0.0 · updated 26-09-24-12-40
"""
download-meter.py - pull one file from the hub as fast as it will go, with N
connections, throw the bytes away, and report MB/s each second.

Two uses:
 1. THE SOFTWARE CEILING. Run it on the hub laptop against 127.0.0.1. No
    radio is involved, so what it reports is how fast the hub itself can hand
    out bytes. If that is far above what the radio carries, the software is
    not what limits the wifi figures. (It is how the Linux bench found that
    loopback prefers many workers while the air prefers four: do not tune
    for the wrong medium.)
 2. A CLIENT FOR REPLICATION. Run it on a second laptop joined to the hub's
    network, to measure the radio without a phone's browser or storage in the
    way.

Nothing is written to disk. Ranges are requested with HTTP Range, so N
connections each take their own slice.

USAGE
    python bench/download-meter.py http://127.0.0.1/big.mkv --connections 4 --seconds 30
"""

import argparse
import http.client
import sys
import threading
import time
import urllib.parse


def worker(url, start, end, counter, lock, deadline, bufsize):
    u = urllib.parse.urlsplit(url)
    conn = http.client.HTTPConnection(u.hostname, u.port or 80, timeout=30)
    conn.request("GET", u.path + ("?" + u.query if u.query else ""),
                 headers={"Range": "bytes={}-{}".format(start, end)})
    r = conn.getresponse()
    if r.status not in (200, 206):
        raise SystemExit("server said {} {}".format(r.status, r.reason))
    while time.time() < deadline:
        chunk = r.read(bufsize)
        if not chunk:
            break
        with lock:
            counter[0] += len(chunk)
    conn.close()


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("url")
    ap.add_argument("--connections", type=int, default=4)
    ap.add_argument("--seconds", type=int, default=30)
    ap.add_argument("--buffer", type=int, default=1 << 20)
    a = ap.parse_args()

    u = urllib.parse.urlsplit(a.url)
    c = http.client.HTTPConnection(u.hostname, u.port or 80, timeout=10)
    c.request("HEAD", u.path)
    size = int(c.getresponse().getheader("Content-Length") or 0)
    c.close()
    if not size:
        sys.exit("Could not learn the file's size.")
    slice_ = size // a.connections
    counter, lock = [0], threading.Lock()
    deadline = time.time() + a.seconds
    threads = [threading.Thread(target=worker, daemon=True,
                                args=(a.url, i * slice_, size - 1 if i == a.connections - 1 else (i + 1) * slice_ - 1,
                                      counter, lock, deadline, a.buffer))
               for i in range(a.connections)]
    t0 = time.time()
    for t in threads:
        t.start()
    last, per = 0, []
    while any(t.is_alive() for t in threads) and time.time() < deadline + 1:
        time.sleep(1)
        with lock:
            now = counter[0]
        per.append((now - last) / 1e6)
        last = now
        print("{:4d}s {:9.1f} MB/s".format(len(per), per[-1]), flush=True)
    total = time.time() - t0
    print("{} connections: {:.1f} MB/s mean ({:.0f} Mbit/s), best second {:.1f} MB/s, {:.2f} GB".format(
        a.connections, counter[0] / total / 1e6, counter[0] * 8 / total / 1e6, max(per or [0]), counter[0] / 1e9))


if __name__ == "__main__":
    main()
