"""The briefing, end to end: the person in charge sets it, a phone reads it first.

Version 1.0.0, 2026-10-06, for 0.11.0.

Why: a team leader opening a net needs everybody who joins to read one thing
before anything else ("ZERO has been established..."), and needs to see who
has. This drives that on the real pages, through a relay so the hub sees the
phone as a separate device (see net-chat-test.py for why).

Steps, each checked, with pictures:
  no briefing   the teacher's page says there is none yet; a phone shows no card
  set           the teacher sets it (the same door the Set briefing button uses)
  phone         the card is first on the phone, with I HAVE READ IT
  read          the phone taps it; the button goes; the teacher's list says read it
  change        the teacher changes it; the phone's button is back; the list
                says not read yet; the room is told
  record        sign-ins.txt has "read the briefing"

Needs: pip install playwright (drives the Edge already installed), and a hub
started with no name set, for example:
    hub serve <folder> --port 8089
    python bench/net-briefing-test.py --url http://127.0.0.1:8089 --received <received folder> --out <pictures>
"""
import argparse
import os
import sys
import urllib.parse
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from playwright.sync_api import sync_playwright

PHONE = dict(viewport={"width": 400, "height": 820}, device_scale_factor=2, is_mobile=True, has_touch=True,
             user_agent="Mozilla/5.0 (Linux; Android 13; Phone) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120 Mobile Safari/537.36")
LAPTOP = dict(viewport={"width": 1280, "height": 760})
FIRST = "ZERO has been established at the school, north gate. Report to ZERO on arrival."
SECOND = "Triage has moved to the tent behind the hall. Full kit before entering."


def post(url, fields):
    """As the page's script posts: the hub takes X-Hub as the sign it is ours."""
    data = urllib.parse.urlencode(fields).encode()
    req = urllib.request.Request(url, data=data, headers={"X-Hub": "1", "Content-Type": "application/x-www-form-urlencoded"})
    return urllib.request.urlopen(req, timeout=10).read().decode()


def main():
    from importlib import import_module
    relay = import_module("net-chat-test").relay
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--url", default="http://127.0.0.1:8089")
    ap.add_argument("--received", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--headed", action="store_true")
    a = ap.parse_args()
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    os.makedirs(a.out, exist_ok=True)
    u = urllib.parse.urlparse(a.url)
    fails = []

    def check(ok, what):
        print(("PASS  " if ok else "FAIL  ") + what, flush=True)
        if not ok:
            fails.append(what)

    def shot(page, name):
        p = os.path.join(a.out, "brief-" + name + ".png")
        page.screenshot(path=p)
        print("      picture", p)

    def sees(page, sel, text, secs=8):
        try:
            page.wait_for_function(
                "([s,t]) => { const e = document.querySelector(s); return !!e && e.innerText.indexOf(t) >= 0; }",
                arg=[sel, text], timeout=secs * 1000)
            return True
        except Exception:
            return False

    def gone(page, sel, secs=8):
        try:
            page.wait_for_function("(s) => !document.querySelector(s)", arg=sel, timeout=secs * 1000)
            return True
        except Exception:
            return False

    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel="msedge", headless=not a.headed)
        errors = []
        teacher = browser.new_context(**LAPTOP).new_page()
        teacher.on("pageerror", lambda e: errors.append("teacher: " + str(e)))
        teacher.goto(a.url + "/op")
        check(sees(teacher, "#chan", "# main"), "the teacher's page opens on # main")
        check(sees(teacher, "#brief", "No briefing yet"), "with no briefing, the teacher's card says so")

        relay(18170, u.hostname, u.port or 80, "127.0.0.70")
        phone = browser.new_context(**PHONE).new_page()
        phone.on("pageerror", lambda e: errors.append("phone: " + str(e)))
        phone.goto("http://127.0.0.1:18170/?lang=en&rename=1")
        phone.fill("input[name=who]", "Amadou")
        phone.click("button[type=submit]")
        check(sees(phone, "#chan", "# main"), "Amadou signs in and lands in # main")
        check(phone.is_hidden("#brief"), "with no briefing, the phone shows no card")

        # The same door the Set briefing button uses (it types /topic ...).
        check(post(a.url + "/net/topic", {"text": FIRST}) == "ok", "the teacher sets the briefing")
        check(sees(phone, "#brief", "north gate"), "the briefing is on Amadou's page")
        check(sees(phone, "#brief", "I HAVE READ IT"), "with I HAVE READ IT under it")
        check(sees(teacher, "#nicks", "not read yet"), "the teacher's list says Amadou has not read it")
        check(sees(teacher, "#nicks", "0 / 1 read the briefing"), "and counts 0 / 1")
        shot(phone, "1-phone-unread")
        shot(teacher, "2-teacher-unread")

        phone.click("#brief [data-act=readbrief]")
        check(gone(phone, "#brief [data-act=readbrief]"), "the button goes once tapped")
        check(sees(teacher, "#nicks", "read it"), "the teacher's list says read it")
        check(sees(teacher, "#nicks", "1 / 1 read the briefing"), "and counts 1 / 1")
        check(sees(teacher, "#log", "Amadou read the briefing"), "the room is told Amadou read it")
        shot(teacher, "3-teacher-read")

        check(post(a.url + "/net/topic", {"text": SECOND}) == "ok", "the teacher changes the briefing")
        check(sees(phone, "#brief", "tent behind the hall"), "the new briefing is on Amadou's page")
        check(sees(phone, "#brief", "I HAVE READ IT"), "with the button back")
        check(sees(teacher, "#nicks", "not read yet"), "the teacher's list says not read yet again")
        check(sees(teacher, "#log", "set the briefing: Triage"), "the room is told of the change")
        shot(phone, "4-phone-changed")

        check(not errors, "no script errors on any page " + ("; ".join(errors)[:300] if errors else ""))
        browser.close()

    try:
        signs = open(os.path.join(a.received, "sign-ins.txt"), encoding="utf-8", errors="replace").read()
    except OSError:
        signs = ""
    check("read the briefing" in signs, "sign-ins.txt has the read")
    print("\n%d checks failed" % len(fails) if fails else "\nall checks passed")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
