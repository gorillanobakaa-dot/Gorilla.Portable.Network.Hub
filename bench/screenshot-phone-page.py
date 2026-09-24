"""Photograph the class page as a phone sees it, with files chosen and one removed.

For the 0.9.9 release pages: the hand-in form now lists chosen files with a
REMOVE button each, and picking again adds. This opens the page the hub is
serving in Edge at a phone's size, gives it a name, chooses three files,
removes one, and saves a picture of the page only.

Needs: pip install playwright (drives the Edge already installed; no browser
download). The hub must be serving at --url.

    python bench/screenshot-phone-page.py --url http://127.0.0.1/ --out docs/screenshots/gallery/phone-choose-and-remove.png
"""

import argparse
import os
import tempfile

from playwright.sync_api import sync_playwright


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--url", default="http://127.0.0.1/")
    ap.add_argument("--out", required=True)
    ap.add_argument("--name", default="Joseph")
    a = ap.parse_args()

    tmp = tempfile.mkdtemp(prefix="hub-phone-")
    files = []
    for n, size in [("my leaf drawing.jpg", 240_000), ("WRONG FILE - holiday.jpg", 310_000), ("homework answers.docx", 38_000)]:
        p = os.path.join(tmp, n)
        with open(p, "wb") as f:
            f.write(b"x" * size)
        files.append(p)

    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel="msedge")
        page = browser.new_page(viewport={"width": 412, "height": 915}, device_scale_factor=2,
                                user_agent="Mozilla/5.0 (Linux; Android 13; Phone) AppleWebKit/537.36 "
                                           "(KHTML, like Gecko) Chrome/120 Mobile Safari/537.36")
        page.goto(a.url)
        if page.locator("input[name=who]").count():
            page.fill("input[name=who]", a.name)
            page.click("text=THAT'S ME")
            page.wait_for_load_state()
        page.set_input_files("#pick", files[:2])
        page.set_input_files("#pick", files[2:])          # picking again adds
        page.locator(".pickrow", has_text="WRONG FILE").locator("button.remove").click()
        rows = page.locator(".pickrow span").all_inner_texts()
        print("listed after remove:", rows)
        page.locator("form[action='/handin']").scroll_into_view_if_needed()
        page.screenshot(path=a.out, full_page=True)
        browser.close()
    print("saved", a.out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
