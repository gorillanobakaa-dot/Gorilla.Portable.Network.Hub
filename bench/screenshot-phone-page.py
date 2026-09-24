"""Photograph the class page as a phone sees it, step by step.

Version 2.0.0, for 0.9.10, when the page was rewritten to explain itself
(numbered sections, a line under each saying what its buttons do, a result box
that says what happened and what next, and a send that shows its progress).

Pictures, each named <prefix>-<step>.png:
  name              the first thing a phone sees: type your name
  class-page        the whole page after the name
  where-downloads   "Where do the files I GET go?" opened
  choose-and-remove three files chosen, the wrong one removed
  sending           the SEND button counting up, the upload slowed on purpose
                    (Chromium's network emulation) so the moment can be seen
  arrived           the green box after the work arrived

The send is real: the work lands in the hub's waiting area like a phone's.

Needs: pip install playwright (drives the Edge already installed; no browser
download). The hub must be serving at --url.

    python bench/screenshot-phone-page.py --url http://127.0.0.1/ --prefix docs/screenshots/gallery/phone-0.9.10
"""

import argparse
import os
import tempfile

from playwright.sync_api import sync_playwright


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--url", default="http://127.0.0.1/")
    ap.add_argument("--prefix", required=True, help="path and start of each picture's name")
    ap.add_argument("--name", default="Joseph")
    a = ap.parse_args()

    tmp = tempfile.mkdtemp(prefix="hub-phone-")
    files = []
    for n, size in [("my leaf drawing.jpg", 2_400_000), ("WRONG FILE - holiday.jpg", 310_000), ("homework answers.docx", 38_000)]:
        p = os.path.join(tmp, n)
        with open(p, "wb") as f:
            f.write(b"x" * size)
        files.append(p)

    def shot(page, step, full=False):
        path = "{}-{}.png".format(a.prefix, step)
        page.screenshot(path=path, full_page=full)
        print("saved", path)

    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel="msedge")
        page = browser.new_page(viewport={"width": 412, "height": 915}, device_scale_factor=2,
                                user_agent="Mozilla/5.0 (Linux; Android 13; Phone) AppleWebKit/537.36 "
                                           "(KHTML, like Gecko) Chrome/120 Mobile Safari/537.36")
        page.goto(a.url + "?rename=1")
        page.fill("input[name=who]", "")
        shot(page, "name")
        page.fill("input[name=who]", a.name)
        page.click("text=THAT'S ME")
        page.wait_for_load_state()
        page.wait_for_timeout(800)                       # the file list frame
        shot(page, "class-page", full=True)

        page.locator("summary", has_text="Where do the files I GET go?").click()
        page.locator("summary", has_text="Where do the files I GET go?").scroll_into_view_if_needed()
        shot(page, "where-downloads")

        page.set_input_files("#pick", files[:2])
        page.set_input_files("#pick", files[2:])          # picking again adds
        page.locator(".pickrow", has_text="WRONG FILE").locator("button.remove").click()
        print("listed after remove:", page.locator(".pickrow span").all_inner_texts())
        page.locator("h2", has_text="Send your work").scroll_into_view_if_needed()
        shot(page, "choose-and-remove")

        # Slow the upload so the counting button can be photographed.
        cdp = page.context.new_cdp_session(page)
        cdp.send("Network.enable")
        cdp.send("Network.emulateNetworkConditions", {
            "offline": False, "latency": 20,
            "downloadThroughput": 5_000_000, "uploadThroughput": 150_000,
        })
        page.click("#sendbtn")
        page.wait_for_function("document.getElementById('sendbtn').textContent.indexOf('%') > 0 "
                               "&& parseInt(document.getElementById('sendbtn').textContent.match(/(\\d+)%/)[1]) >= 30",
                               timeout=60_000)
        print("button said:", page.locator("#sendbtn").inner_text())
        shot(page, "sending")
        cdp.send("Network.emulateNetworkConditions", {
            "offline": False, "latency": 0, "downloadThroughput": -1, "uploadThroughput": -1,
        })
        page.wait_for_url("**/?done=*", timeout=120_000)
        page.wait_for_timeout(500)
        print("result:", page.locator(".done, .bad").first.inner_text())
        shot(page, "arrived")
        browser.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
