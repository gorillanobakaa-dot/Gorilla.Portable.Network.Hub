# Version: 1.0.0 · updated 26-09-07-15-50
#
# Build the published Linux archive, and check it before saying it is built.
#
# WHY THIS EXISTS AT ALL
#
# The Windows archive got a script after it shipped as a tar named .zip: `tar
# -a -c -f out.zip` auto-detects COMPRESSION from the extension, not the
# container, so every check passed except the one a user performs. The Linux
# archive had no script at all. It was assembled by hand, from a directory
# that is in .gitignore, containing a README that existed nowhere else in the
# project. Losing that directory would have lost the readme, and nothing would
# have noticed until somebody unpacked a release and found no instructions.
#
# WHY THE CHECKS
#
# The binary is cross-compiled from Windows. The two ways that can go wrong
# and still produce a file are: it is not actually a Linux executable, and it
# is a Linux executable left over from an earlier build. Both have happened on
# this project. A stale binary with a freshly computed checksum is the worse
# of the two, because every verification a user can run will agree with it.
#
# So the ELF header is read here rather than trusted, and the version string
# is looked for inside the binary's own bytes.

import gzip
import io
import pathlib
import sys
import tarfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
VERSION = "0.9.6"
BIN = ROOT / "src" / "hub" / "target" / "x86_64-unknown-linux-musl" / "release" / "hub"
README = ROOT / "packaging" / "linux-README.txt"
OUT = ROOT / "dist" / f"hub-{VERSION}-linux-x86_64-static.tar.gz"


def die(msg):
    print(msg, file=sys.stderr)
    sys.exit(1)


if not BIN.is_file():
    die(f"No binary at {BIN}\nBuild it first:  bash packaging/build-linux-static.sh")
if not README.is_file():
    die(f"No readme at {README}")

blob = BIN.read_bytes()

# ELF, 64-bit, little-endian, x86-64, and no interpreter to go looking for.
if blob[:4] != b"\x7fELF":
    die(f"{BIN} is not an ELF file. Do not ship this.")
if blob[4] != 2:
    die(f"{BIN} is not 64-bit. Do not ship this.")
if blob[5] != 1:
    die(f"{BIN} is not little-endian. Do not ship this.")
if int.from_bytes(blob[18:20], "little") != 0x3E:
    die(f"{BIN} is not x86-64. Do not ship this.")

# The version the binary reports, found in its own bytes, so a stale artifact
# from a previous release cannot be published under a new number.
if f"hub {VERSION}".encode() not in blob:
    die(
        f"{BIN} does not contain the string 'hub {VERSION}'.\n"
        "This is what a stale binary looks like. Rebuild before shipping."
    )

# A Windows resource section has no business in a Linux binary, and its
# presence would mean the wrong target was built.
if b"VS_VERSION_INFO" in blob:
    die(f"{BIN} carries a Windows resource. Wrong target. Do not ship this.")

OUT.parent.mkdir(parents=True, exist_ok=True)
if OUT.exists():
    OUT.unlink()

# Fixed metadata, so identical inputs give identical bytes and "rebuild it and
# compare the checksum" is a check anybody can run. mtime 0, uid/gid 0, and no
# owner names: a tar that records who built it is a tar only they can
# reproduce.
def entry(name, data, mode):
    info = tarfile.TarInfo(name)
    info.size = len(data)
    info.mtime = 0
    info.mode = mode
    info.uid = info.gid = 0
    info.uname = info.gname = "root"
    info.type = tarfile.REGTYPE
    return info, io.BytesIO(data)


raw = io.BytesIO()
with tarfile.open(fileobj=raw, mode="w", format=tarfile.GNU_FORMAT) as t:
    for name, data, mode in (("hub", blob, 0o755), ("README.txt", README.read_bytes(), 0o644)):
        info, f = entry(name, data, mode)
        t.addfile(info, f)

# mtime=0 in the gzip header too; the default is "now".
with open(OUT, "wb") as f:
    with gzip.GzipFile(filename="", mode="wb", fileobj=f, compresslevel=9, mtime=0) as gz:
        gz.write(raw.getvalue())

# Read it back. An archive that cannot be opened is worse than no archive: it
# looks like a download and fails at the one moment nobody can debug it.
with open(OUT, "rb") as f:
    if f.read(2) != b"\x1f\x8b":
        die(f"{OUT} is not gzip. Do not ship this.")

with tarfile.open(OUT, "r:gz") as t:
    names = t.getnames()
    if sorted(names) != ["README.txt", "hub"]:
        die(f"{OUT} contains {names}, expected hub and README.txt. Do not ship this.")
    back = t.extractfile("hub").read()
    if back != blob:
        die(f"{OUT} does not give back the bytes that went in. Do not ship this.")

print(f"{OUT}  {OUT.stat().st_size} bytes")
print(f"  hub         {len(blob)} bytes, ELF x86-64, contains 'hub {VERSION}'")
print(f"  README.txt  {README.stat().st_size} bytes")
