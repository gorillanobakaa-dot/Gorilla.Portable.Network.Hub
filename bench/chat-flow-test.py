"""Drive the messages and the HELP button end to end, as a child's phone would.

Version 2.0.0, 2026-09-24, for 0.10.0.

2.0.0: help moved into the conversation. The owner's second real-phone test
showed the old design (switched on at the start of the lesson, behind a
separate page) never got switched on, and would lose a child's impulse to ask.
Now HELP is always there, under the conversation, and needs one tap. The old
steps for the trusted adult's page are gone with it.

Against a running hub started with NO help settings at all (that is the
point: nothing to switch on). Steps, each checked, with a picture after each:

  child   types a name, writes to the teacher
  child   taps HELP once: no new page, a green line says what happens next,
          and the tap is in the conversation
  child   taps HELP again at once: still accepted (asking for help is never
          refused for going too fast)
  record  messages.txt in the received folder has the HELP line
  child   in Swahili: the button says HELP and the Swahili word

The teacher's side (HELP first on the list, "A CHILD ASKED FOR HELP. Press m")
is checked by the unit test tui::tests::one_tap_on_help_reaches_the_teacher_first.

Needs: pip install playwright (drives the Edge already installed).

    python bench/chat-flow-test.py --url http://127.0.0.1:8089 --received <received folder> --out <folder for pictures>
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
    ap.add_argument("--received", required=True, help="the hub's received folder, where messages.txt is written")
    ap.add_argument("--out", required=True)
    ap.add_argument("--prefix", default="chat")
    a = ap.parse_args()
    # The Windows console is not UTF-8 by default; the button's hand would crash the print.
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
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

        check(child.locator("button.need").count() == 1, "HELP is on the page with nothing switched on")
        before = child.url
        child.click("button.need")
        child.wait_for_selector("#talkok", state="visible", timeout=15000)
        check(child.url == before, "one tap on HELP stays on the same page")
        check("safe moment" in child.inner_text("#talkok"), "a green line says what happens next")
        child.wait_for_function("document.getElementById('thread').textContent.indexOf('✋ HELP') >= 0", timeout=15000)
        check(True, "the tap is in the conversation")
        child.locator("button.need").scroll_into_view_if_needed()
        shot(child, "help-in-chat")

        child.click("button.need")
        child.wait_for_timeout(1500)
        check(child.locator("#talkmsg").is_hidden(), "a second tap straight away is still accepted")

        path = os.path.join(a.received, "messages.txt")
        text = open(path, encoding="utf-8").read() if os.path.exists(path) else ""
        check("Amina" in text and "tapped HELP" in text, "messages.txt has the HELP line")

        child.goto(a.url + "/?lang=sw")
        child.wait_for_load_state()
        label = child.inner_text("button.need")
        check("HELP" in label and "MSAADA" in label.upper(), f"in Swahili the button says HELP and the Swahili word ({label.strip()})")
        child.locator("button.need").scroll_into_view_if_needed()
        shot(child, "help-in-chat-swahili")
        child.goto(a.url + "/?lang=en")
        browser.close()

    print("\n%d checks failed" % len(failures) if failures else "\nall checks passed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
