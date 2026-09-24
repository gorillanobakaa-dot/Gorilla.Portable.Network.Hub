"""Drive the messages and private help end to end, as a child and an adult would.

Version 1.0.0, 2026-09-24, for 0.9.10.

Two separate browser sessions against a running hub started with
`--help-password`: one is a child's phone (Edge at phone size), the other the
trusted adult's phone. Steps, each checked, with a picture after each:

  child   types a name, writes to the teacher (ordinary conversation)
  child   opens HELP, taps I NEED TO TALK, writes privately
  adult   signs in at /adult with a wrong password (refused), then the right one
  adult   sees the child in the list, opens the conversation, answers,
          and asks quietly to talk
  child   sees the answer and the question appear by themselves, taps LATER
  adult   sees "Later, not now."
  child   taps HIDE: back on the class page, and the private words are not on it

Needs: pip install playwright (drives the Edge already installed).

    python bench/chat-flow-test.py --url http://127.0.0.1:8089 --password <help password> --out <folder for pictures>
"""

import argparse
import os
import sys

from playwright.sync_api import sync_playwright

PHONE = dict(viewport={"width": 412, "height": 915}, device_scale_factor=2,
             user_agent="Mozilla/5.0 (Linux; Android 13; Phone) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120 Mobile Safari/537.36")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--url", default="http://127.0.0.1:8089")
    ap.add_argument("--password", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--prefix", default="chat")
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    failures = []

    def check(ok, what):
        print(("PASS  " if ok else "FAIL  ") + what)
        if not ok:
            failures.append(what)

    def shot(page, name, full=False):
        path = os.path.join(a.out, f"{a.prefix}-{name}.png")
        page.screenshot(path=path, full_page=full)
        print("      picture", path)

    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel="msedge")
        child = browser.new_context(**PHONE).new_page()
        adult = browser.new_context(**PHONE).new_page()

        # English explicitly: every session here shares 127.0.0.1, so a language
        # chosen by an earlier run would otherwise carry over.
        child.goto(a.url + "/?lang=en&rename=1")
        child.fill("input[name=who]", "Amina")
        child.click("text=THAT'S ME")
        child.wait_for_load_state()
        child.fill("#talk ~ form.talk textarea[name=text]", "Lesson 2 will not open on my phone")
        child.click("#talk ~ form.talk button")
        child.wait_for_function("document.getElementById('thread').textContent.indexOf('Lesson 2 will not open') >= 0", timeout=15000)
        check(True, "the child's message to the teacher appears in the conversation")
        child.locator("#talk").scroll_into_view_if_needed()
        shot(child, "talk-to-teacher")

        child.click("a.helpbtn")
        child.wait_for_load_state()
        check(child.title() == "Class page", "the help page's title says nothing about help")
        check("/page2" in child.url, "the help page's address says nothing about help")
        child.click("button.need")
        child.wait_for_selector("#talkok", state="visible", timeout=15000)
        check("safe moment" in child.inner_text("#talkok"), "one tap sends I NEED TO TALK and says what happens next")
        child.fill("form.talk textarea[name=text]", "Something is happening at home")
        child.click("text=SEND PRIVATELY")
        child.wait_for_function("document.getElementById('thread').textContent.indexOf('Something is happening') >= 0", timeout=15000)
        check(True, "the private message appears on the child's help page")
        shot(child, "help-page", full=True)

        adult.goto(a.url + "/adult")
        adult.fill("input[name=password]", "not-the-password")
        adult.click("text=SIGN IN")
        adult.wait_for_load_state()
        check("not right" in adult.content(), "a wrong password is refused and says so")
        adult.fill("input[name=password]", a.password)
        adult.click("text=SIGN IN")
        adult.wait_for_load_state()
        check("Amina" in adult.content() and "ASKED TO TALK" in adult.content(), "the adult sees the child, flagged as asking to talk")
        shot(adult, "adult-list", full=True)
        adult.click("a.row")
        adult.wait_for_load_state()
        adult.fill("textarea[name=text]", "Thank you for telling me. Are you safe right now?")
        adult.click("text=SEND")
        adult.wait_for_load_state()
        adult.click("text=ASK QUIETLY TO TALK")
        adult.wait_for_load_state()
        frame = adult.frame_locator("iframe")
        check("Something is happening at home" in frame.locator("body").inner_text(), "the adult reads the child's private words")
        shot(adult, "adult-conversation", full=True)

        child.wait_for_function("document.getElementById('thread').textContent.indexOf('Are you safe') >= 0", timeout=15000)
        check(True, "the adult's answer appears on the child's page by itself")
        check(child.locator("text=would like to talk to you").count() > 0, "the quiet request appears, with YES, LATER and NO")
        shot(child, "child-sees-answer", full=True)
        child.click("button:has-text('LATER')")
        child.wait_for_timeout(1500)
        adult.reload()
        adult.wait_for_timeout(6000)
        check("Later, not now." in adult.frame_locator("iframe").locator("body").inner_text(), "the adult sees the child's answer")

        child.click("text=HIDE THIS")
        child.wait_for_load_state()
        body = child.content()
        check(child.url.rstrip("/").endswith(("8089", "127.0.0.1")) or child.url.endswith("/"), "HIDE goes back to the class page")
        check("Something is happening" not in body and "Are you safe" not in body, "the private words are not on the class page")
        browser.close()

    print("\n%d checks failed" % len(failures) if failures else "\nall checks passed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
