#!/usr/bin/env python3
# Version: 1.0.0 · updated 26-08-24-23-05
"""Render every icon size from one master, the way the estate already decided.

The doctrine is recorded in the brain as Lanczos_Downsample_Icon_Pipeline
(verified 2026-07-18) and it is not optional:

  - Every icon file must be REALLY that many pixels. Copying one big PNG into
    all the size slots wastes memory and looks grainy, because the desktop then
    downscales it cheaply at draw time and dithers it.
  - Always downscale from a big master with Lanczos. Never upscale, ever.
  - Crop to a square FIRST, then resize per target.

The tool that used to do this, `branding_engine.py` (later `gorilla brand
gen-icons` in the firefox toolkit), is not on this machine any more: the README
survives in SECOND.BRAIN/docs and CONSOLIDATION_STATUS.md records the move, but
neither gorilla_brand.py nor the toolkit directory exists. This reimplements the
documented pipeline so the capability is not lost twice.
"""
import sys
from collections import deque
from PIL import Image

# hicolor sizes a Debian desktop actually looks for, plus 1024 for future use.
SIZES = (16, 22, 24, 32, 48, 64, 128, 256, 512)


def strip_flat_checkerboard(img, lo=190, spread=14):
    """Remove a transparency checkerboard that was flattened into real pixels.

    The master here is a PNG that was saved as JPEG, so the grey-and-white
    chequer that MEANT transparency is now ordinary opaque pixels. Making every
    light grey transparent would punch holes in the white text on the cap and
    in the cardboard sign, so instead this floods inward from the border and
    only clears background that is CONNECTED to the edge. Enclosed white stays.
    """
    img = img.convert("RGBA")
    w, h = img.size
    px = img.load()

    def is_background(x, y):
        r, g, b, _ = px[x, y]
        return min(r, g, b) >= lo and (max(r, g, b) - min(r, g, b)) <= spread

    seen = bytearray(w * h)
    q = deque()
    for x in range(w):
        for y in (0, h - 1):
            if not seen[y * w + x] and is_background(x, y):
                seen[y * w + x] = 1
                q.append((x, y))
    for y in range(h):
        for x in (0, w - 1):
            if not seen[y * w + x] and is_background(x, y):
                seen[y * w + x] = 1
                q.append((x, y))

    while q:
        x, y = q.popleft()
        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            nx, ny = x + dx, y + dy
            if 0 <= nx < w and 0 <= ny < h and not seen[ny * w + nx] and is_background(nx, ny):
                seen[ny * w + nx] = 1
                q.append((nx, ny))

    cleared = 0
    for y in range(h):
        row = y * w
        for x in range(w):
            if seen[row + x]:
                r, g, b, _ = px[x, y]
                px[x, y] = (r, g, b, 0)
                cleared += 1
    return img, cleared


# The mascot is a whole composition: a gorilla, a cardboard sign, a giant zero
# and a mug. All of it is legible at 256 and none of it at 48, which is the size
# a desktop actually draws in a menu or a dock. Cropping to the head and cap
# keeps the two things that identify it, the face and the Debian swirl, and
# lets them fill the tile. Compared side by side at 48 and 64 before choosing.
MASCOT_CROP = (70, 0, 860, 790)


def square(img):
    """Centre-crop to the largest square, then trim to what is actually drawn.

    Trimming matters at 16 pixels: empty margin is subject the icon does not
    get, and a subject that fills the tile survives the downsample.
    """
    box = img.getbbox()           # bbox of the non-transparent content
    if box:
        img = img.crop(box)
    w, h = img.size
    s = max(w, h)
    out = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    out.paste(img, ((s - w) // 2, (s - h) // 2))
    return out


def main():
    if len(sys.argv) < 3:
        print("usage: make-icons.py <master image> <output dir> [--keep-background]")
        return 2
    src, outdir = sys.argv[1], sys.argv[2]
    master = Image.open(src)

    if "--keep-background" not in sys.argv:
        master, cleared = strip_flat_checkerboard(master)
        print(f"background: cleared {cleared} px ({cleared / (master.size[0] * master.size[1]):.1%})")
    if "--crop-head" in sys.argv:
        master = master.crop(MASCOT_CROP)
    master = square(master)
    print(f"master after square+trim: {master.size[0]}x{master.size[1]}")

    import os
    for px in SIZES:
        if px > master.size[0]:
            # The rule that is easiest to break by accident and worst to break.
            print(f"  {px}: SKIPPED, that would be an upscale from {master.size[0]}")
            continue
        d = f"{outdir}/{px}x{px}/apps"
        os.makedirs(d, exist_ok=True)
        out = master.resize((px, px), Image.LANCZOS)
        path = f"{d}/hub.png"
        out.save(path, optimize=True)
        # Verify the artifact, never the exit code: confirm the file really is
        # the size it claims, because "copied the big one everywhere" is
        # exactly the failure this pipeline exists to prevent.
        back = Image.open(path)
        assert back.size == (px, px), f"{path} is {back.size}, not {px}"
        print(f"  {px}x{px}  {os.path.getsize(path):>7} bytes  verified {back.size[0]}px")
    return 0


if __name__ == "__main__":
    sys.exit(main())
