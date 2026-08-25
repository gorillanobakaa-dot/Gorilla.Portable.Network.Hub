#!/bin/sh
# Build the Arch / CachyOS / Manjaro package (.pkg.tar.zst).
#
# Usage: packaging/build-arch.sh [version]
#
# Needs bsdtar and zstd, and a release binary already built. It does NOT need
# makepkg or pacman, which is the point: this machine is Debian, and a package
# nobody can build here is a package that silently stops being made.
#
# What this cannot do is INSTALL the result. That takes an Arch machine, and
# until somebody runs `pacman -U` on one, the honest description of this
# artifact is "assembled to spec and structurally verified", not "tested".
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="${1:-$(grep -m1 '^version' "$ROOT/src/hub/Cargo.toml" | cut -d'"' -f2)}"
PKGREL=1
PKGNAME=gorilla-portable-network-hub
BIN="$ROOT/src/hub/target/release/hub"

command -v bsdtar >/dev/null || { echo "bsdtar is missing (apt install libarchive-tools)" >&2; exit 1; }
command -v zstd   >/dev/null || { echo "zstd is missing" >&2; exit 1; }
[ -x "$BIN" ] || { echo "Build it first: cargo build --release --manifest-path src/hub/Cargo.toml" >&2; exit 1; }

# Refuse a binary whose stamp does not match the version being packaged. An
# artifact's name is not its contents: the sibling project wrapped a stale
# binary under two later version numbers before this guard existed, and dpkg
# reported the new version while the program reported the old one.
STAMPED="$("$BIN" --version 2>/dev/null || true)"
[ "$STAMPED" = "hub $VERSION" ] || {
    echo "The binary says '$STAMPED' but you asked to package '$VERSION'." >&2
    echo "Rebuild: cargo build --release --manifest-path src/hub/Cargo.toml" >&2
    exit 1
}

# And refuse a PKGBUILD that has drifted. Same failure, different file: a stale
# pkgver still makes a valid recipe, so nothing complains and Arch users build
# last month's release.
PKGVER=$(grep -m1 '^pkgver=' "$ROOT/packaging/PKGBUILD" | cut -d= -f2)
[ "$PKGVER" = "$VERSION" ] || {
    echo "packaging/PKGBUILD says pkgver=$PKGVER but this is $VERSION." >&2
    exit 1
}

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
PKG="$STAGE/pkg"
mkdir -p "$PKG"

install -Dm755 "$BIN" "$PKG/usr/bin/hub"
install -Dm644 "$ROOT/packaging/etc/hub-captive.conf" \
    "$PKG/etc/NetworkManager/dnsmasq-shared.d/hub-captive.conf"
python3 "$ROOT/packaging/make-icons.py" \
    "$ROOT/packaging/icon/mascot-master.jpg" \
    "$PKG/usr/share/icons/hicolor" --crop-head >/dev/null
install -Dm644 "$ROOT/packaging/gorilla-portable-network-hub.desktop" \
    "$PKG/usr/share/applications/$PKGNAME.desktop"
install -Dm644 "$ROOT/packaging/hub.1" "$PKG/usr/share/man/man1/hub.1"
install -Dm644 "$ROOT/LICENSE" "$PKG/usr/share/licenses/$PKGNAME/LICENSE"
for d in README.md docs/HOW-TO.md docs/WHY-THIS-EXISTS.md docs/DEVELOPER.md docs/SCREENSHOTS.md; do
    install -Dm644 "$ROOT/$d" "$PKG/usr/share/doc/$PKGNAME/$(basename "$d")"
done

# The post-install script, which is what grants the port 80 capability. pacman
# finds it by the member name; nothing in .PKGINFO points at it.
install -m644 "$ROOT/packaging/hub.install" "$PKG/.INSTALL"

SIZE_BYTES=$(du -sb "$PKG" | cut -f1)
BUILD_DATE=$(date -u +%s)
cat > "$PKG/.PKGINFO" <<EOF
pkgname = $PKGNAME
pkgbase = $PKGNAME
pkgver = ${VERSION}-${PKGREL}
pkgdesc = Turn a laptop into the network and hand a folder to the room
url = https://github.com/gorillanobakaa-dot/Gorilla.Portable.Network.Hub
builddate = ${BUILD_DATE}
packager = gorillanobakaa <gorillanobakaa@gmail.com>
size = ${SIZE_BYTES}
arch = x86_64
license = MIT
depend = glibc
optdepend = networkmanager: to create a wifi network where there is none
optdepend = libcap: to answer on port 80, so a joining phone pops its own sign-in screen
backup = etc/NetworkManager/dnsmasq-shared.d/hub-captive.conf
EOF

(
  cd "$PKG"
  LANG=C bsdtar -czf .MTREE --format=mtree \
    --options='!all,use-set,type,uid,gid,mode,time,size,md5,sha256' \
    .PKGINFO .INSTALL etc usr
)

OUT="$ROOT/packaging/build"
mkdir -p "$OUT"
PKGFILE="$OUT/$PKGNAME-${VERSION}-${PKGREL}-x86_64.pkg.tar.zst"
TARFILE="$STAGE/pkg.tar"
# Written as a complete intermediate file before compression. A
# `bsdtar | zstd > out` pipeline returns the COMPRESSOR's status, so a truncated
# tar can still look like a success.
(
  cd "$PKG"
  LANG=C bsdtar -cf "$TARFILE" .PKGINFO .INSTALL .MTREE etc usr
)
test -s "$TARFILE"
zstd -q -z -19 -T1 -f -o "$PKGFILE" "$TARFILE"
zstd -t "$PKGFILE"

# Verify the ARTIFACT, not the exit codes above it.
echo
echo "built $PKGFILE ($(du -h "$PKGFILE" | cut -f1))"
MEMBERS=$(zstd -dc "$PKGFILE" | bsdtar -tf - | wc -l)
ICONS=$(zstd -dc "$PKGFILE" | bsdtar -tf - | grep -c 'icons/hicolor/.*/apps/hub.png')
UNIQUE=$(md5sum "$PKG"/usr/share/icons/hicolor/*/apps/hub.png | awk '{print $1}' | sort -u | wc -l)
for required in .PKGINFO .INSTALL .MTREE usr/bin/hub; do
    zstd -dc "$PKGFILE" | bsdtar -tf - | grep -qx "$required" \
        || { echo "MISSING from the package: $required" >&2; exit 1; }
done
echo "members: $MEMBERS    icons: $ICONS installed, $UNIQUE distinct"
echo "NOT installed or tested on Arch. Structure verified only."
