#!/bin/bash
# Version: 1.0.0 · updated 26-08-25-08-06
#
# Build the Debian package. Everything the .deb contains is generated here, so
# the package can be rebuilt from a clone rather than from somebody's shell
# history.
set -euo pipefail

VERSION=${VERSION:-0.7.1}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$ROOT/packaging/build
STAGE=$OUT/gorilla-portable-network-hub_${VERSION}_amd64
DEB=$OUT/gorilla-portable-network-hub_${VERSION}_amd64.deb
BIN=$ROOT/src/hub/target/release/hub

# Refuse rather than invent. A missing binary here used to mean packaging an
# old one silently, which is worse than stopping.
if [ ! -x "$BIN" ]; then
    echo "No binary at $BIN" >&2
    echo "Build it first:  cd $ROOT/src/hub && cargo build --release" >&2
    exit 1
fi

# The package version and the binary's own version have to agree, or `hub
# --version` contradicts `dpkg -l` and there is no way to tell which is right.
BINVER=$("$BIN" --version | awk '{print $2}')
if [ "$BINVER" != "$VERSION" ]; then
    echo "Version mismatch: binary says $BINVER, packaging says $VERSION" >&2
    exit 1
fi

rm -rf "$STAGE"
mkdir -p "$STAGE"/{DEBIAN,usr/bin,usr/share/applications,usr/share/man/man1}
mkdir -p "$STAGE/etc/NetworkManager/dnsmasq-shared.d"
DOC=$STAGE/usr/share/doc/gorilla-portable-network-hub
mkdir -p "$DOC"

install -m755 "$BIN" "$STAGE/usr/bin/hub"

# The captive-portal answers. Marked as a conffile below so a teacher's local
# edit (a different hotspot address) survives upgrades.
install -m644 "$ROOT/packaging/etc/hub-captive.conf"         "$STAGE/etc/NetworkManager/dnsmasq-shared.d/hub-captive.conf"
echo "/etc/NetworkManager/dnsmasq-shared.d/hub-captive.conf" > "$STAGE/DEBIAN/conffiles"

# Every icon size rendered for real from the master, never one file copied.
python3 "$ROOT/packaging/make-icons.py" \
        "$ROOT/packaging/icon/mascot-master.jpg" \
        "$STAGE/usr/share/icons/hicolor" --crop-head

cp "$ROOT/packaging/gorilla-portable-network-hub.desktop" \
   "$STAGE/usr/share/applications/"
cp "$ROOT/packaging/copyright" "$DOC/copyright"
cp "$ROOT/README.md" "$ROOT/docs/WHY-THIS-EXISTS.md" "$ROOT/docs/DEVELOPER.md" "$DOC/"
gzip -9n -c "$ROOT/packaging/hub.1" > "$STAGE/usr/share/man/man1/hub.1.gz"

# The version has no Debian revision, so this is a native package and the
# changelog is changelog.gz, not changelog.Debian.gz. lintian checks.
sed "s/@VERSION@/$VERSION/; s/@DATE@/$(date -R)/" \
    "$ROOT/packaging/changelog.in" | gzip -9n -c > "$DOC/changelog.gz"

install -m755 "$ROOT/packaging/postinst" "$STAGE/DEBIAN/postinst"
install -m755 "$ROOT/packaging/postrm"   "$STAGE/DEBIAN/postrm"

find "$STAGE" -type d -exec chmod 755 {} +
find "$STAGE" -type f -not -path '*/DEBIAN/*' -exec chmod 644 {} +
chmod 755 "$STAGE/usr/bin/hub"

SIZE=$(du -sk --exclude=DEBIAN "$STAGE" | cut -f1)
sed "s/@VERSION@/$VERSION/; s/@SIZE@/$SIZE/" "$ROOT/packaging/control.in" \
    > "$STAGE/DEBIAN/control"

fakeroot dpkg-deb --build "$STAGE" "$DEB" >/dev/null

# Verify the artifact, not the exit code. dpkg-deb reports success for a
# package whose contents are wrong.
echo
echo "built $DEB ($(du -h "$DEB" | cut -f1))"
COUNT=$(dpkg-deb -c "$DEB" | grep -c 'icons/hicolor/.*/apps/hub.png')
UNIQUE=$(md5sum "$STAGE"/usr/share/icons/hicolor/*/apps/hub.png | awk '{print $1}' | sort -u | wc -l)
echo "icons: $COUNT installed, $UNIQUE distinct"
[ "$COUNT" = "$UNIQUE" ] || { echo "an icon size is a copy of another, which is the whole thing this avoids" >&2; exit 1; }
command -v lintian >/dev/null && lintian --tag-display-limit 0 "$DEB"
echo
echo "install with:  sudo dpkg -i $DEB"
