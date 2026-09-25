"""The class chat end to end: a teacher and several phones, all at once, in real browsers.

Version 1.0.0, 2026-09-25, for 0.11.0.

Why: the owner's two-phone test of 0.10.0 (2026-09-25) found the messages
could not handle two phones at once and had no room the class shares. This
drives the replacement the way a lesson would, with every phone live at the
same time, and checks each step on every screen it should reach.

Each phone gets its own relay (a local port that forwards to the hub from its
own loopback address, 127.0.0.N), because Windows gives every loopback
connection the source 127.0.0.1, and the hub treats 127.0.0.1 as the teacher.
So the hub sees separate devices, as it would on the wifi.

Steps, each checked, with pictures:

  sign in      three phones type a name; each lands in # main
  people       the teacher's list shows all three, online
  # main       a line from one phone reaches the others and the teacher
  at once      two phones write in the same second; both lines reach everybody
  comms check  the teacher calls it; every phone shows I READ YOU; answers count up
  HELP         one phone taps HELP; the teacher gets a red card and a red entry
  private      the teacher answers two children in two windows at once;
               each child is told, and sees only their own
  quiet        the teacher quiets the room: phones cannot write, HELP still can
  mute         one phone is muted; the others still write
  remove       the teacher removes a device (two taps); its page turns to Paused
  record       messages.txt and sign-ins.txt have it all

Needs: pip install playwright (drives the Edge already installed), and a hub
started with no name set for this test, for example:

    hub serve <folder> --port 8089
    python bench/net-chat-test.py --url http://127.0.0.1:8089 --received <received folder> --out <pictures>
"""
import argparse
import os
import socket
import sys
import threading
import time
import urllib.parse

from playwright.sync_api import sync_playwright

PHONE = dict(viewport={"width": 400, "height": 820}, device_scale_factor=2, is_mobile=True, has_touch=True,
             user_agent="Mozilla/5.0 (Linux; Android 13; Phone) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120 Mobile Safari/537.36")
LAPTOP = dict(viewport={"width": 1280, "height": 760})


def relay(listen_port, hub_host, hub_port, source_ip):
    """Forward every connection on listen_port to the hub, from source_ip."""
    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", listen_port))
    srv.listen(64)

    def pipe(a, b):
        try:
            while True:
                d = a.recv(65536)
                if not d:
                    break
                b.sendall(d)
        except OSError:
            pass
        for s in (a, b):
            try:
                s.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass

    def serve():
        while True:
            c, _ = srv.accept()
            try:
                h = socket.create_connection((hub_host, hub_port), source_address=(source_ip, 0))
            except OSError:
                c.close()
                continue
            threading.Thread(target=pipe, args=(c, h), daemon=True).start()
            threading.Thread(target=pipe, args=(h, c), daemon=True).start()

    threading.Thread(target=serve, daemon=True).start()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--url", default="http://127.0.0.1:8089")
    ap.add_argument("--received", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--headed", action="store_true", help="show the browser windows")
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
        p = os.path.join(a.out, "net-" + name + ".png")
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

    names = ["Amina", "Kofi", "Fatima"]
    kids = {}
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel="msedge", headless=not a.headed)
        teacher = browser.new_context(**LAPTOP).new_page()
        errors = []
        teacher.on("pageerror", lambda e: errors.append("teacher: " + str(e)))
        teacher.goto(a.url + "/op")
        check(sees(teacher, "#chan", "# main"), "the teacher's page opens on # main")

        for i, n in enumerate(names):
            port = 18100 + i
            relay(port, u.hostname, u.port or 80, "127.0.0.%d" % (40 + i))
            p = browser.new_context(**PHONE).new_page()
            p.on("pageerror", lambda e, n=n: errors.append(n + ": " + str(e)))
            # rename=1: the hub remembers a device's name, so a second run
            # would otherwise skip the name page.
            p.goto("http://127.0.0.1:%d/?lang=en&rename=1" % port)
            p.fill("input[name=who]", n)
            p.click("button[type=submit]")
            kids[n] = p
        for n, p in kids.items():
            check(sees(p, "#chan", "# main"), n + " signs in and lands in # main")
        ok = all(sees(teacher, "#nicks", n) for n in names)
        check(ok, "the teacher's people list shows all three")
        shot(teacher, "1-teacher-people")

        # # main, one to everybody
        amina, kofi, fatima = kids["Amina"], kids["Kofi"], kids["Fatima"]
        amina.fill("#text", "hello everybody from Amina")
        amina.click("#sendbtn")
        check(all(sees(p, "#log", "hello everybody from Amina") for p in (kofi, fatima, teacher)),
              "a line from Amina reaches Kofi, Fatima and the teacher")

        # two at once
        kofi.fill("#text", "Kofi at the same time")
        fatima.fill("#text", "Fatima at the same time")
        kofi.click("#sendbtn")
        fatima.click("#sendbtn")
        check(all(sees(p, "#log", "Kofi at the same time") and sees(p, "#log", "Fatima at the same time")
                  for p in (amina, kofi, fatima, teacher)),
              "two phones writing in the same second both reach everybody")
        teacher.fill("#text", "Good morning. The worksheet is in # files.")
        teacher.click("#sendbtn")
        check(sees(amina, "#log", "The worksheet is in # files"), "the teacher's line reaches the phones")
        shot(amina, "2-phone-main")

        # comms check
        teacher.click("[data-act=check]")
        check(all(sees(p, "#above", "I READ YOU") for p in kids.values()), "every phone shows I READ YOU")
        shot(kofi, "3-phone-comms-check")
        amina.click("[data-act=answer]")
        check(sees(teacher, "#nicks", "1 / 3 answered"), "the teacher's count says 1 of 3 after Amina answers")
        kofi.click("[data-act=answer]")
        fatima.click("[data-act=answer]")
        check(sees(teacher, "#nicks", "3 / 3 answered"), "and 3 of 3 when all have answered")
        check(not amina.is_visible("[data-act=answer]"), "an answered phone's button goes away")
        shot(teacher, "4-teacher-comms-check")

        # HELP
        kofi.click("#helpbtn")
        check(sees(kofi, "#above", "safe moment"), "Kofi is told what happens next")
        check(sees(teacher, "#toasts", "Kofi asked for HELP"), "the teacher gets a red HELP card for Kofi")
        check(sees(teacher, "#side", "HELP Kofi"), "and a red HELP Kofi in the private chats")
        check(not sees(amina, "#log", "HELP", 2) and not sees(amina, "#toasts", "HELP", 1), "Amina sees nothing of Kofi's HELP")
        shot(teacher, "5-teacher-help")

        # private, two windows at once
        teacher.click("#toasts [data-act=open]")
        check(teacher.locator("#dock .win").count() == 1, "Open puts Kofi's chat in a window")
        teacher.fill("#dock input", "Kofi, I am coming to your desk")
        teacher.press("#dock input", "Enter")
        check(sees(kofi, "#toasts", "Your teacher wrote to you") or sees(kofi, "#log", "I am coming to your desk"),
              "Kofi is told the teacher wrote")
        # On a phone the rooms and people fold into the panel behind the menu.
        amina.click("#menu")
        amina.click("#side [data-buf^='@']")
        amina.fill("#text", "teacher, my phone cannot open the worksheet")
        amina.click("#sendbtn")
        check(sees(teacher, "#toasts", "Amina wrote to you"), "Amina's private message gives the teacher a card")
        teacher.click("#toasts [data-act=open]")
        check(teacher.locator("#dock .win:not(.min)").count() == 2, "the teacher has Kofi and Amina open side by side")
        wins = teacher.locator("#dock .win:not(.min) input")
        for i in range(wins.count()):
            if "Amina" in (wins.nth(i).get_attribute("placeholder") or ""):
                wins.nth(i).fill("Amina, try the Swahili copy")
                wins.nth(i).press("Enter")
        check(sees(amina, "#log", "try the Swahili copy") or sees(amina, "#toasts", "Your teacher wrote"), "Amina gets her answer")
        check(not sees(kofi, "#log", "Swahili copy", 2) and not sees(fatima, "#log", "Swahili copy", 1),
              "nobody else sees Amina's private chat")
        shot(teacher, "6-teacher-two-windows")
        shot(amina, "7-phone-private")
        for p in (kofi, amina):
            back = p.locator(".top [data-buf='#main']")
            if back.count():
                back.click()

        # quiet
        teacher.click("[data-act=quiet]")
        check(sees(fatima, "#above", "teacher is talking"), "quiet: the phones are told the teacher is talking")
        check(fatima.is_disabled("#text"), "quiet: a phone cannot type in # main")
        check(fatima.is_visible("#helpbtn"), "quiet: HELP is still there")
        teacher.click("[data-act=quiet]")
        check(sees(fatima, "#log", "opened the room"), "the room opens again")

        # mute
        teacher.click("#nicks [data-nick]:has-text('Fatima')")
        teacher.click(".pop [data-do=mute]")
        check(sees(fatima, "#above", "muted you"), "mute: Fatima is told")
        amina.fill("#text", "Amina can still write")
        amina.click("#sendbtn")
        check(sees(teacher, "#log", "Amina can still write"), "mute: the others still write")

        # remove
        teacher.click("#nicks [data-nick]:has-text('Fatima')")
        teacher.click(".pop [data-do=kick]")
        check(sees(teacher, ".pop", "Tap again"), "remove asks for a second tap")
        teacher.click(".pop [data-do=kick]")
        try:
            fatima.wait_for_function("document.body.innerText.indexOf('Paused') >= 0", timeout=40000)
            gone = True
        except Exception:
            gone = False
        check(gone, "remove: Fatima's page turns to Paused")
        shot(fatima, "8-phone-removed")
        check(sees(teacher, "#log", "Fatima was removed"), "the room is told")

        # the files room, and the page in another language
        amina.click("#menu")
        amina.click("#side [data-buf='#files']")
        try:
            amina.frame_locator("#filesframe").locator("text=worksheet").first.wait_for(timeout=8000)
            files_ok = True
        except Exception:
            files_ok = False
        check(files_ok, "# files shows the lesson's files on a phone")
        shot(amina, "9-phone-files")
        sw = browser.new_context(**PHONE).new_page()
        relay(18150, u.hostname, u.port or 80, "127.0.0.60")
        sw.goto("http://127.0.0.1:18150/?lang=sw&rename=1")
        sw.fill("input[name=who]", "Baraka")
        sw.click("button[type=submit]")
        check(sees(sw, "#chan", "# main") and sw.get_attribute("#text", "placeholder") == "Andikia kila mtu",
              "in Swahili the page says Andikia kila mtu (" + str(sw.get_attribute("#text", "placeholder")) + ")")
        check("MSAADA" in sw.inner_text("#helpbtn").upper(), "in Swahili HELP carries the Swahili word")
        shot(sw, "10-phone-swahili")

        check(not errors, "no script errors on any page " + ("; ".join(errors)[:300] if errors else ""))
        browser.close()

    def read(name):
        p = os.path.join(a.received, name)
        return open(p, encoding="utf-8").read() if os.path.exists(p) else ""
    msgs, signs = read("messages.txt"), read("sign-ins.txt")
    check("#main" in msgs and "hello everybody from Amina" in msgs, "messages.txt has the # main lines")
    check("tapped HELP" in msgs and "teacher -> " in msgs, "messages.txt has the HELP and the teacher's answers")
    check("COMMS CHECK" in signs and "answered the comms check" in signs, "sign-ins.txt has the comms check and the answers")
    check("removed from the lesson" in signs, "sign-ins.txt has the removed device")

    print("\n%d checks failed" % len(fails) if fails else "\nall checks passed")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
