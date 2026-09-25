"""Many phones at once against a running hub: does the server keep answering?

Version 1.0.1, 2026-09-25. 1.0.1: held waits reported apart from ordinary
requests, and chat lines delivered to the phones counted from "lines".

Why: the owner's real-phone test (2026-09-25) with two phones reported the
network "failing and dropping". Before rebuilding anything, this measures the
SERVER half on its own, with no radio in the way: a crowd of simulated phones
doing what the page makes a real phone do, every request timed, every failure
kept. If the server holds, the drops are in the radio or Windows' hotspot and
need their own over-the-air test (bench/hotspot-monitor.ps1).

Each simulated phone uses its own loopback address (127.0.0.2, .3, ...), so
the hub sees separate devices with separate names and message budgets, as it
would on the wifi.

Two modes, for the two page designs:

  classic  the 0.10.0 page: sign in, ask /talk every 4 s, the file list every
           10 s, a message now and then.
  net      the chat page (0.11): sign in, keep one /net/wait open (the server
           answers when something happens or after 25 s), chat in #main now
           and then, answer comms checks.

    python bench/net-load-test.py --url http://127.0.0.1:8089 --phones 30 --minutes 5 --mode classic

Prints a summary and writes every failure, with its time, to --log.
"""
import argparse
import http.client
import json
import random
import statistics
import sys
import threading
import time
import urllib.parse

lock = threading.Lock()
# Writing stops a few seconds before listening does, so the last line of a
# run is not counted as "missed" by phones that had already hung up.
writing = threading.Event()
writing.set()
lat = []          # seconds, every request that got an answer, except held waits
waits = []        # seconds, the /net/wait requests (held until news, or 25 s)
fails = []        # (time, phone, what, error)
counts = {"requests": 0, "sent": 0, "received": 0}


def note_ok(t, held=False):
    with lock:
        counts["requests"] += 1
        (waits if held else lat).append(t)


def note_fail(phone, what, err):
    with lock:
        counts["requests"] += 1
        fails.append((time.strftime("%H:%M:%S"), phone, what, repr(err)[:160]))


class Phone:
    def __init__(self, n, host, port, stop, mode):
        self.n, self.host, self.port, self.stop, self.mode = n, host, port, stop, mode
        self.src = ("127.0.0.%d" % (n + 2), 0)
        self.name = "Phone%02d" % n
        self.first_id = None   # the newest line in the first answer: older ones were history
        self.got = set()       # ids of chat lines this phone received

    def req(self, method, path, body=None, timeout=35, what=None):
        """One request on a fresh connection, like a phone's browser after
        its idle connection has been closed. Returns (status, text) or None."""
        t0 = time.monotonic()
        try:
            c = http.client.HTTPConnection(self.host, self.port, timeout=timeout, source_address=self.src)
            h = {"User-Agent": "Mozilla/5.0 (Linux; Android 13; Phone) Chrome/120 Mobile"}
            if body is not None:
                h["Content-Type"] = "application/x-www-form-urlencoded"
                h["X-Hub"] = "1"
            c.request(method, path, body=body, headers=h)
            r = c.getresponse()
            text = r.read().decode("utf-8", "replace")
            c.close()
            note_ok(time.monotonic() - t0, held=path.startswith("/net/wait"))
            if r.status >= 500:
                note_fail(self.n, what or path, "HTTP %d" % r.status)
            return r.status, text
        except Exception as e:
            note_fail(self.n, what or path.split("?")[0], e)
            return None

    def run(self):
        form = urllib.parse.urlencode
        self.req("GET", "/?lang=en")
        self.req("POST", "/name", form({"who": self.name}))
        if self.mode == "classic":
            self.classic()
        else:
            self.net()

    def classic(self):
        v, nxt_files, nxt_say = 0, 0, time.time() + random.uniform(5, 60)
        while not self.stop.is_set():
            r = self.req("GET", "/talk?p=0&v=%d&r=%f" % (v, random.random()), what="/talk poll")
            if r and r[0] == 200:
                v = int(r[1].split("\n", 1)[0] or 0)
            if time.time() > nxt_files:
                self.req("GET", "/files")
                nxt_files = time.time() + 10
            if time.time() > nxt_say:
                tok = "%x" % random.getrandbits(48)
                r = self.req("POST", "/talk", urllib.parse.urlencode(
                    {"text": "hello from %s" % self.name, "kind": "text", "p": "0", "token": tok}), what="/talk send")
                if r and r[1] == "sent":
                    with lock:
                        counts["sent"] += 1
                nxt_say = time.time() + random.uniform(30, 90)
            self.stop.wait(4)

    def net(self):
        since, line, nxt_say = 0, 0, time.time() + random.uniform(5, 60)
        answered = None
        while not self.stop.is_set():
            # Both numbers, as the page sends them: without `line` every answer
            # would carry the room's last 150 lines again.
            r = self.req("GET", "/net/wait?since=%d&line=%d" % (since, line), timeout=40, what="/net/wait")
            if r and r[0] == 200:
                try:
                    d = json.loads(r[1])
                    since = d.get("v", since)
                    for l in d.get("lines", []):
                        line = max(line, l.get("id", 0))
                        if l.get("kind") == "text":
                            self.got.add(l["id"])
                    if self.first_id is None:
                        self.first_id = line
                    with lock:
                        counts["received"] += len([l for l in d.get("lines", []) if l.get("kind") == "text"])
                    chk = d.get("check")
                    if chk and chk.get("id") != answered and not chk.get("me"):
                        answered = chk.get("id")
                        self.req("POST", "/net/answer", "", what="/net/answer")
                except ValueError as e:
                    note_fail(self.n, "/net/wait json", e)
            elif not r:
                self.stop.wait(2)   # the page backs off the same way
            if time.time() > nxt_say and writing.is_set():
                tok = "%x" % random.getrandbits(48)
                r = self.req("POST", "/net/say", urllib.parse.urlencode(
                    {"to": "#main", "text": "hello from %s" % self.name, "token": tok}), what="/net/say")
                if r and r[1].startswith("sent"):
                    with lock:
                        counts["sent"] += 1
                nxt_say = time.time() + random.uniform(30, 90)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--url", default="http://127.0.0.1:8089")
    ap.add_argument("--phones", type=int, default=30)
    ap.add_argument("--minutes", type=float, default=5)
    ap.add_argument("--mode", choices=["classic", "net"], default="classic")
    ap.add_argument("--log", default=None, help="file for every failure line")
    a = ap.parse_args()
    u = urllib.parse.urlparse(a.url)
    stop = threading.Event()
    phones = [Phone(i, u.hostname, u.port or 80, stop, a.mode) for i in range(a.phones)]
    threads = []
    for p in phones:
        t = threading.Thread(target=p.run, daemon=True)
        t.start()
        threads.append(t)
        time.sleep(0.2)     # a class does not join in the same millisecond
    t_end = time.time() + a.minutes * 60
    while time.time() < t_end:
        time.sleep(15)
        with lock:
            print("%s  requests %d  failures %d  messages sent %d" % (
                time.strftime("%H:%M:%S"), counts["requests"], len(fails), counts["sent"]), flush=True)
    writing.clear()
    time.sleep(5)
    stop.set()
    for t in threads:
        t.join(timeout=45)
    with lock:
        l = sorted(lat)
        print("\n%d phones, %.1f minutes, mode %s" % (a.phones, a.minutes, a.mode))
        print("requests answered %d, failed %d" % (len(l), len(fails)))
        if l:
            q = lambda p: l[min(len(l) - 1, int(p * len(l)))]
            print("time to answer (sign-in, sending, answering): median %.3f s, 95%% %.3f s, slowest %.3f s" % (
                statistics.median(l), q(0.95), l[-1]))
        if waits:
            w = sorted(waits)
            print("held waits: %d, median %.1f s (answered early whenever there was news), longest %.1f s" % (
                len(w), statistics.median(w), w[-1]))
        print("messages sent %d, chat lines received by phones %d (first loads of earlier lines included)" % (
            counts["sent"], counts["received"]))
        if a.mode == "net":
            seen = set().union(*[p.got for p in phones])
            missed = sum(len({i for i in seen if i > (p.first_id or 0)} - p.got) for p in phones)
            owed = sum(len({i for i in seen if i > (p.first_id or 0)}) for p in phones)
            print("every line checked against every phone connected when it was written: %d owed, %d missed" % (owed, missed))
            for p in phones:
                gap = sorted({i for i in seen if i > (p.first_id or 0)} - p.got)
                if gap:
                    print("  %s missed lines %s; the last it received was %s, the newest anywhere %s" % (
                        p.name, gap, max(p.got) if p.got else None, max(seen)))
            if missed:
                fails.append(("", "", "delivery", "%d lines missed" % missed))
        kinds = {}
        for f in fails:
            kinds[(f[2], f[3].split("(")[0])] = kinds.get((f[2], f[3].split("(")[0]), 0) + 1
        for (w, e), n in sorted(kinds.items(), key=lambda x: -x[1]):
            print("  %4d x %s: %s" % (n, w, e))
        if a.log:
            with open(a.log, "w", encoding="utf-8") as f:
                for x in fails:
                    f.write("  ".join(map(str, x)) + "\n")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
