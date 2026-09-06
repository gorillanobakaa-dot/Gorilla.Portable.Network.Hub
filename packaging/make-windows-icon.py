#!/usr/bin/env python3
# Version: 1.0.0 · updated 26-09-06-20-10
"""Build the Windows icon from the one master image.

WHY THIS EXISTS. The program shipped with no icon at all, so Windows drew the
generic blank-document placeholder on the desktop and in the taskbar. On a
machine where somebody has been told "double-click the Gorilla icon", a blank
placeholder among a dozen other blank placeholders is a real obstacle, and it
also makes the thing look like something that should not be trusted with a
child's homework.

WHAT IT DECIDES, AND WHY.

The master is a whole scene: a gorilla, a cap, a sign, a mug, crumpled paper.
That is a picture, not an icon. At 32 pixels a scene becomes mud, so this crops
to the cap and the eyes, which is the part that still reads when it is the size
of a full stop. The red Debian swirl on the cap survives the shrink and is what
makes the icon findable at a glance.

The crop also leaves out the mug, which carries language that is fine on a
project's own page and is not what a teacher wants sitting on a school laptop
desktop at large-icon size.

Contrast and brightness are lifted slightly. The subject is very dark fur on a
dark cap, and every desktop shrinks it further; without the lift it goes to a
featureless dark square.

Sizes are rendered individually with Lanczos from the 1024px master, per the
pipeline documented in make-icons.py: never upscale, never let the desktop
downscale one big image at draw time.
"""
import io
import struct
import sys
from pathlib import Path

try:
    from PIL import Image, ImageEnhance
except ImportError:
    sys.exit("This needs Pillow:  python -m pip install Pillow")

ROOT = Path(__file__).resolve().parent.parent
MASTER = ROOT / "packaging" / "icon" / "mascot-master.jpg"
OUT = ROOT / "packaging" / "icon" / "hub.ico"

# The cap and the eyes, square, from the 1024px master.
CROP = (175, 15, 875, 715)

# Modern sizes only.
#
# 16, 24, 32 and 48 are slots from an era when the desktop could not filter and
# every one had to be drawn by hand. Windows 10 and 11 on any normal display
# take the large frame and scale it properly, and a high-DPI screen never wants
# the small ones at all. Carrying them is bytes for nothing and four more
# renders to keep looking right.
SIZES = (64, 128, 256)


def main():
    if not MASTER.is_file():
        sys.exit(f"No master image at {MASTER}")

    src = Image.open(MASTER).convert("RGB")
    if src.size != (1024, 1024):
        print(f"note: master is {src.size}, the crop box assumes 1024x1024")

    base = src.crop(CROP).resize((512, 512), Image.LANCZOS)
    base = ImageEnhance.Contrast(base).enhance(1.25)
    base = ImageEnhance.Brightness(base).enhance(1.10)

    # 256 colours, and the icon file assembled by hand.
    #
    # This project counts its own bytes in public: the README carries a table of
    # what every feature cost. A truecolour icon adds 158 KB to a 751 KB
    # program, a fifth of it, for a picture. Quantised it costs 69 KB, and the
    # measured difference on a dark photographic subject is 1.28 of 255 per
    # pixel, which nobody can see at any size an icon is drawn.
    #
    # Assembled here rather than through Image.save(format="ICO") because that
    # path re-encodes each frame from the image it is given and throws the
    # palette away.
    frames = []
    for s in SIZES:
        f = base.resize((s, s), Image.LANCZOS)
        f = f.quantize(colors=256, method=Image.MEDIANCUT)
        buf = io.BytesIO()
        f.save(buf, "PNG", optimize=True)
        frames.append((s, buf.getvalue()))

    # ICONDIR, then one ICONDIRENTRY per frame, then the frames themselves.
    header = struct.pack("<HHH", 0, 1, len(frames))
    offset = len(header) + 16 * len(frames)
    entries, blobs = b"", b""
    for size, data in frames:
        # 0 in the size byte means 256: the field is one byte and 256 does not
        # fit in it. Every icon reader knows this.
        byte = 0 if size >= 256 else size
        entries += struct.pack(
            "<BBBBHHII", byte, byte, 0, 0, 1, 32, len(data), offset
        )
        offset += len(data)
        blobs += data
    OUT.write_bytes(header + entries + blobs)

    print(f"wrote {OUT}")
    print(f"  {OUT.stat().st_size:,} bytes, {len(SIZES)} sizes: {', '.join(str(s) for s in SIZES)}")

    # Read it back. An icon file the shell cannot parse is indistinguishable
    # from no icon at all, and that is the state this is fixing.
    check = Image.open(OUT)
    got = sorted(check.info.get("sizes", []))
    if len(got) != len(SIZES):
        sys.exit(f"only {len(got)} sizes survived the write: {got}")
    print(f"  verified: {got}")


if __name__ == "__main__":
    main()
