#!/usr/bin/env python3
# Version: 1.0.0 · updated 26-09-06-14-40
#
# Build the Windows release archive, and prove it is one.
#
# WHY THIS EXISTS AS A SCRIPT
#
# The 0.9.0 Windows archive was first built with `tar -a -c -f out.zip`, on the
# assumption that bsdtar picks the container from the extension. It does not.
# The -a flag auto-detects COMPRESSION, not archive format, so the result was an
# uncompressed tar with a .zip name. It was published in that state.
#
# Nothing caught it, and that is the interesting part. `tar -tvf` listed the
# contents happily, because it was a valid tar. The sha256 in the release notes
# matched, because it was the sha256 of the wrong thing. Only unzipping it the
# way an actual Windows user does, with Expand-Archive, produced the error:
# "Split or spanned archives are not supported."
#
# For the audience this tool is for, a teacher double-clicking a .zip that
# Explorer refuses to open has no way to tell that from the program being
# broken, and no second option. So the archive is now built by a library that
# only makes zips, and the script opens the result and reads it back before
# saying it succeeded.
#
#   python3 packaging/build-windows-zip.py
#
# Requires: cargo build --release, run first (or cross-built) so hub.exe exists.

import hashlib
import pathlib
import sys
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
VERSION = "0.9.1"
BIN = ROOT / "src" / "hub" / "target" / "release" / "hub.exe"
OUT = ROOT / "dist" / f"hub-{VERSION}-windows-x86_64.zip"
README = ROOT / "packaging" / "windows-README.txt"


def die(msg):
    print(msg, file=sys.stderr)
    sys.exit(1)


if not BIN.is_file():
    die(f"No binary at {BIN}\nBuild it first:  cd {ROOT / 'src' / 'hub'} && cargo build --release")
if not README.is_file():
    die(f"No readme at {README}")

OUT.parent.mkdir(parents=True, exist_ok=True)
if OUT.exists():
    OUT.unlink()

# A fixed timestamp, so the same inputs give the same bytes.
#
# Zip records each file's modification time, so an archive built twice from
# identical files has two different checksums, and "rebuild it and compare"
# stops being a check anybody can run. A project that asks to be audited should
# not have an artifact only its author can reproduce.
FIXED = (1980, 1, 1, 0, 0, 0)

with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for src, name in ((BIN, "hub.exe"), (README, "README.txt")):
        info = zipfile.ZipInfo(name, date_time=FIXED)
        info.compress_type = zipfile.ZIP_DEFLATED
        info.external_attr = 0o644 << 16
        z.writestr(info, src.read_bytes())

# Read it back. An archive that cannot be opened is worse than no archive: it
# looks like a download and fails at the one moment nobody can debug it.
with open(OUT, "rb") as f:
    magic = f.read(4)
if magic != b"PK\x03\x04":
    die(f"{OUT} does not start with a zip signature (got {magic!r}). Do not ship this.")

with zipfile.ZipFile(OUT) as z:
    broken = z.testzip()
    if broken is not None:
        die(f"CRC failure in {broken}. Do not ship this.")
    names = sorted(z.namelist())
    if names != ["README.txt", "hub.exe"]:
        die(f"Unexpected contents: {names}")

digest = hashlib.sha256(OUT.read_bytes()).hexdigest()
print(f"built    {OUT}")
print(f"size     {OUT.stat().st_size:,} bytes")
print(f"contents {names}")
print(f"sha256   {digest}")
print()
print("Verified: real zip container, CRCs check out, both files present.")
print("Put that sha256 in the release notes.")
