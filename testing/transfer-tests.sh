#!/usr/bin/env bash
# portal-tests.sh - exercise the hub end to end: cable, wifi, and the page itself.
# Version: 1.0.0 - updated 26-09-08-10-05
#
# Every check is a real request over a real socket. Nothing is mocked. The
# network watchdog runs alongside so a test that disturbs routing cannot leave
# the machine unreachable.
#
# Usage: ./portal-tests.sh [--hub /path/to/hub]

set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS="$HERE/results"; mkdir -p "$RESULTS"
STAMP="$(date '+%y-%m-%d-%H-%M')"
LOG="$RESULTS/portal-tests-$STAMP.log"
SUM="$RESULTS/portal-tests-summary-$STAMP.txt"
# The binary under test, found relative to this file so a clone works, with an
# override for testing a packaged install instead:  HUB=/usr/bin/hub ./transfer-tests.sh
HUB="${HUB:-$HERE/../src/hub/target/release/hub}"
[ "${1:-}" = "--hub" ] && HUB="$2"

# Nothing about this machine is written down here. The first version hardcoded
# one laptop's interface names and the other laptop's address, which meant the
# harness only ever worked on the desk it was written at.
#
# The wired interface is whichever one has a cable in it, the wifi one is
# whatever NetworkManager says is connected, and the far end is whoever is
# announcing themselves on the cable. PEER can name it by hand when nothing is
# announcing, which is also how this is pointed at a machine on the wifi.
CABLE_DEV="${CABLE_DEV:-$(for d in /sys/class/net/*/; do
    n=$(basename "$d"); [ "$n" = lo ] && continue
    [ -e "$d/wireless" ] && continue
    [ "$(cat "$d/carrier" 2>/dev/null)" = 1 ] && { echo "$n"; break; }
done)}"
WIFI_DEV="${WIFI_DEV:-$(nmcli -t -f DEVICE,TYPE,STATE device 2>/dev/null \
    | awk -F: '$2=="wifi" && $3=="connected" {print $1; exit}')}"
CABLE_IP=$([ -n "$CABLE_DEV" ] && ip -4 -o addr show dev "$CABLE_DEV" 2>/dev/null | awk '{print $4}' | cut -d/ -f1 | head -1)
WIFI_IP=$([ -n "$WIFI_DEV" ] && ip -4 -o addr show dev "$WIFI_DEV" 2>/dev/null | awk '{print $4}' | cut -d/ -f1 | head -1)
# Who is on the other end. Asked for, not assumed.
WIN="${PEER:-}"
if [ -z "$WIN" ] && [ -x "$HUB" ] && [ -n "$CABLE_DEV" ]; then
    # Listen for whoever is announcing, and take only the address out of it.
    # Into a scratch folder that is thrown away: cable-get fetches as well as
    # finds, and "--into /dev/null" would try to write files into a device.
    DISCO=$(mktemp -d)
    WIN=$( (cd "$DISCO" && timeout 25 "$HUB" cable-get --wait 15 2>&1) \
           | grep -oE 'at [0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' | awk '{print $2}' | head -1)
    rm -rf "$DISCO"
fi
PORT=8099                  # our own test server, away from anything real

P=0; F=0; S=0
say(){ printf '%s\n' "$*" | tee -a "$LOG"; }
sec(){ say ""; say "======== $* ========"; }
ok(){  printf '%-4s %-5s %s\n' PASS "$1" "$2" >>"$SUM"; say "[PASS] $1 $2"; P=$((P+1)); }
no(){  printf '%-4s %-5s %s\n' FAIL "$1" "$2" >>"$SUM"; say "[FAIL] $1 $2"; F=$((F+1)); }
sk(){  printf '%-4s %-5s %s\n' SKIP "$1" "$2" >>"$SUM"; say "[SKIP] $1 $2"; S=$((S+1)); }
code(){ timeout 15 curl -s -o "${2:-/dev/null}" -w '%{http_code}' "$1" 2>/dev/null; }

say "portal-tests $STAMP"
say "hub: $HUB ($($HUB --version 2>&1))"
say "cable ${CABLE_DEV:-none} ${CABLE_IP:-none} -> peer ${WIN:-none}     wifi ${WIFI_DEV:-none} ${WIFI_IP:-none}"

"$HERE/network-watchdog.sh" start --lifetime 1800 >/dev/null 2>&1
"$HERE/network-watchdog.sh" is-running && ok W1 "watchdog protecting the wifi route" || no W1 "watchdog did not start"

# ---------------------------------------------------------------- A: receiving
sec "A  Receiving from the Windows laptop over the cable"

if [ -z "$CABLE_DEV" ]; then
    sk A0 "no cable carrier, skipping the whole receive section"
else
    ok A0 "cable on $CABLE_DEV, we hold ${CABLE_IP:-no address}"

    c=$(code "http://$WIN/")
    [ "$c" = 200 ] && ok A1 "the Windows page answers on port 80 ($c)" || no A1 "port 80 answered $c"

    c=$(code "http://gorilla.local/")
    [ "$c" = 200 ] && ok A2 "gorilla.local resolves and answers ($c)" || no A2 "gorilla.local answered $c"

    # Fetch one file two ways and compare. Same bytes from the browser path and
    # the download path, or one of them is lying.
    D="$RESULTS/recv-$STAMP"; mkdir -p "$D"
    code "http://$WIN/readme.txt" "$D/plain.txt" >/dev/null
    code "http://$WIN/readme.txt?dl=1" "$D/dl.txt" >/dev/null
    if [ -s "$D/plain.txt" ] && cmp -s "$D/plain.txt" "$D/dl.txt"; then
        ok A3 "a file fetched with and without ?dl=1 is byte identical"
    else
        no A3 "the two fetch paths differ or returned nothing"
    fi

    # Resume. The help claims files are served in pieces that can be asked for
    # individually, which is what makes a dropped signal survivable.
    RANGE=$(timeout 15 curl -s -r 0-9 -o "$D/range.bin" -w '%{http_code}' "http://$WIN/readme.txt" 2>/dev/null)
    got=$(stat -c %s "$D/range.bin" 2>/dev/null || echo 0)
    if [ "$RANGE" = 206 ] && [ "$got" = 10 ]; then
        ok A4 "a range request returns 206 and exactly the 10 bytes asked for"
    else
        no A4 "range request gave code=$RANGE bytes=$got (want 206 and 10)"
    fi

    # The whole folder as one archive, and it has to actually open.
    Z=$(code "http://$WIN/everything.zip" "$D/everything.zip")
    if [ "$Z" = 200 ] && timeout 60 python3 -c "
import zipfile,sys
z=zipfile.ZipFile('$D/everything.zip')
bad=z.testzip()
sys.exit(1 if bad else 0)
print(len(z.namelist()))
" 2>/dev/null; then
        n=$(python3 -c "import zipfile;print(len(zipfile.ZipFile('$D/everything.zip').namelist()))" 2>/dev/null)
        ok A5 "everything.zip downloads and opens cleanly ($n entries)"
    else
        no A5 "everything.zip code=$Z or the archive does not open"
    fi

    # Folder structure has to survive the archive, not be flattened.
    if python3 -c "
import zipfile,sys
n=zipfile.ZipFile('$D/everything.zip').namelist()
sys.exit(0 if any('/' in x for x in n) else 1)" 2>/dev/null; then
        ok A6 "the archive keeps folders rather than flattening them"
    else
        no A6 "the archive has no nested paths, folders were flattened"
    fi
    # The headline claim: nothing typed. It listens for the sender's beacon on
    # UDP 42424 rather than being told an address, so this is run from an empty
    # folder with no host, no port and no name given to it.
    G="$RESULTS/cableget-$STAMP"; rm -rf "$G"; mkdir -p "$G"
    ( cd "$G" && timeout --kill-after=10 90 "$HUB" cable-get --wait 45 ) >"$RESULTS/cableget-$STAMP.log" 2>&1
    rc=$?
    got=$(find "$G" -type f ! -name fetch.log | wc -l)
    if [ "$rc" = 124 ] || [ "$rc" = 137 ]; then
        no A7 "cable-get timed out, the beacon on UDP 42424 was never heard"
    elif [ "$got" -gt 0 ]; then
        ok A7 "cable-get found the sender with nothing typed and took $got files"
    else
        no A7 "cable-get exited $rc having fetched nothing"
    fi

    # And what it fetched has to be the same bytes the web path serves.
    if [ -f "$G/readme.txt" ] && [ -s "$D/plain.txt" ]; then
        cmp -s "$G/readme.txt" "$D/plain.txt" \
          && ok A8 "a file taken by cable-get matches the same file over HTTP" \
          || no A8 "cable-get and HTTP returned different bytes for readme.txt"
    else
        sk A8 "readme.txt not present on both paths to compare"
    fi
fi

# ---------------------------------------------------------------- B: serving
sec "B  Serving from this Linux laptop"

ROOT="$RESULTS/serveroot-$STAMP"
rm -rf "$ROOT"; mkdir -p "$ROOT/subject/week1" "$ROOT/.hidden" "$ROOT/handed-in"
head -c 200000 /dev/urandom > "$ROOT/big.bin"
echo "top level" > "$ROOT/top.txt"
echo "nested lesson" > "$ROOT/subject/week1/lesson.txt"
echo "secret" > "$ROOT/.hidden/secret.txt"
echo "child work" > "$ROOT/handed-in/homework.txt"

setsid nohup timeout 120 "$HUB" serve "$ROOT" --port $PORT >"$RESULTS/serve-$STAMP.log" 2>&1 &
sleep 5

c=$(code "http://127.0.0.1:$PORT/")
[ "$c" = 200 ] && ok B1 "our own server answers on 127.0.0.1:$PORT" || no B1 "loopback answered $c"

c=$(code "http://$WIFI_IP:$PORT/")
[ "$c" = 200 ] && ok B2 "reachable on the wifi address $WIFI_IP" || no B2 "wifi address answered $c"

c=$(code "http://$CABLE_IP:$PORT/")
[ "$c" = 200 ] && ok B3 "reachable on the cable address $CABLE_IP" || no B3 "cable address answered $c"

# The class page is not one page. It opens with a name gate, and once you have
# said who you are the file list is loaded into an IFRAME from /files. Fetching
# "/" and grepping for a filename finds nothing and looks exactly like a broken
# listing, which is what it looked like here until the iframe was noticed.
GATE="$RESULTS/ourgate-$STAMP.html"
code "http://127.0.0.1:$PORT/" "$GATE" >/dev/null
grep -q 'action="/name"' "$GATE" \
  && ok B4 "the class page asks who you are before showing anything" \
  || no B4 "no name gate on the class page"

JAR="$RESULTS/cookies-$STAMP.txt"; rm -f "$JAR"
timeout 15 curl -s -c "$JAR" -b "$JAR" -o /dev/null -d "who=Test+Device" \
    "http://127.0.0.1:$PORT/name" 2>/dev/null
HOME_PAGE="$RESULTS/ourhome-$STAMP.html"
timeout 15 curl -s -c "$JAR" -b "$JAR" -o "$HOME_PAGE" "http://127.0.0.1:$PORT/" 2>/dev/null
grep -q "Test Device" "$HOME_PAGE" \
  && ok B4b "after naming yourself the page greets you by name" \
  || no B4b "the name did not stick"

PAGE="$RESULTS/ourfiles-$STAMP.html"
timeout 15 curl -s -c "$JAR" -b "$JAR" -o "$PAGE" "http://127.0.0.1:$PORT/files" 2>/dev/null
grep -q "subject/week1/lesson.txt" "$PAGE" \
  && ok B4c "a file three folders deep is listed" \
  || no B4c "the nested file is missing from the listing"
grep -q "everything.zip" "$PAGE" \
  && ok B4d "the whole folder is offered as one download" \
  || no B4d "no GET EVERYTHING on the listing"

ZO="$RESULTS/ourzip-$STAMP.zip"
zc=$(code "http://127.0.0.1:$PORT/everything.zip" "$ZO")
if [ "$zc" = 200 ] && python3 - "$ZO" <<'PYZ'
import sys, zipfile
z = zipfile.ZipFile(sys.argv[1])
names = z.namelist()
assert z.testzip() is None, "corrupt archive"
assert any(n.endswith("subject/week1/lesson.txt") for n in names), names
assert not any(".hidden" in n or "handed-in" in n for n in names), names
PYZ
then
    ok B4e "our own everything.zip opens, keeps folders, and excludes what it must"
else
    no B4e "our everything.zip is wrong or would not open (code=$zc)"
fi

D2="$RESULTS/ourfetch-$STAMP"; mkdir -p "$D2"
code "http://127.0.0.1:$PORT/subject/week1/lesson.txt" "$D2/lesson.txt" >/dev/null
cmp -s "$D2/lesson.txt" "$ROOT/subject/week1/lesson.txt" \
  && ok B5 "the nested file arrives byte identical" \
  || no B5 "the nested file did not match"

code "http://127.0.0.1:$PORT/big.bin" "$D2/big.bin" >/dev/null
cmp -s "$D2/big.bin" "$ROOT/big.bin" \
  && ok B6 "a 200 KB binary arrives byte identical" \
  || no B6 "the binary did not match"

# ---------------------------------------------------------------- C: refusals
sec "C  What it must refuse"

for attempt in "/../../../../etc/passwd" "/%2e%2e/%2e%2e/etc/passwd" "/subject/../../../etc/passwd"; do
    body="$RESULTS/trav-$STAMP.out"
    c=$(code "http://127.0.0.1:$PORT$attempt" "$body")
    if [ "$c" = 200 ] && grep -q "root:" "$body" 2>/dev/null; then
        no C1 "PATH TRAVERSAL served /etc/passwd via $attempt"
        break
    fi
done
grep -q "^FAIL C1" "$SUM" 2>/dev/null || ok C1 "path traversal refused on every shape tried"

c=$(code "http://127.0.0.1:$PORT/.hidden/secret.txt")
[ "$c" = 200 ] && no C2 "a dotfolder was served ($c)" || ok C2 "dotfolders are not served ($c)"

c=$(code "http://127.0.0.1:$PORT/handed-in/homework.txt")
[ "$c" = 200 ] && no C3 "children's handed-in work was served back to the class ($c)" \
               || ok C3 "handed-in work is never served back ($c)"

grep -q "secret.txt" "$PAGE" && no C4 "a dotfolder file is listed on the page" || ok C4 "dotfolder files are not listed"
grep -q "homework.txt" "$PAGE" && no C5 "handed-in work is listed on the page" || ok C5 "handed-in work is not listed"

# ---------------------------------------------------------------- D: the portal
sec "D  The sign-in page behaviour"

REDIR=$(timeout 15 curl -s -o /dev/null -w '%{http_code} %{redirect_url}' \
        -H "Host: nmcheck.gnome.org" "http://127.0.0.1:$PORT/check_network_status.txt" 2>/dev/null)
case "$REDIR" in
  302*) ok D1 "the connectivity probe name gets a 302 ($REDIR)" ;;
  *)    no D1 "the probe got '$REDIR', not a 302" ;;
esac

REDIR=$(timeout 15 curl -s -o /dev/null -w '%{http_code}' -H "Host: bbc.co.uk" "http://127.0.0.1:$PORT/" 2>/dev/null)
[ "$REDIR" = 302 ] && ok D2 "any other site is redirected to the class page ($REDIR)" \
                   || no D2 "a foreign host got $REDIR, not 302"

OWN=$(timeout 15 curl -s -o /dev/null -w '%{http_code}' -H "Host: 127.0.0.1:$PORT" "http://127.0.0.1:$PORT/" 2>/dev/null)
[ "$OWN" = 200 ] && ok D3 "our own address is served, not redirected ($OWN)" \
                 || no D3 "our own address got $OWN"

# ---------------------------------------------------------------- E: hand-in
sec "E  Work coming back the other way"

TOK=$(grep -oE 'value="[a-f0-9]{8,}"' "$HOME_PAGE" | head -1 | sed 's/value="//;s/"//')
WORK="$RESULTS/homework-$STAMP.txt"
echo "my homework, by Test Device" > "$WORK"
BEFORE=$(find "$ROOT" -path "*handed-in/waiting*" -type f 2>/dev/null | wc -l)

# The hidden field is a DUPLICATE guard, not an authorisation check, and the
# code says so: an empty token is tolerated because "ancient browser lost the
# field; the content net below catches repeats". There is nothing to authorise,
# because the hub is built to take work from anyone in the room. So the thing
# worth testing is not whether it refuses, it is whether the same work sent
# twice arrives twice.
NOTOK=$(timeout 20 curl -s -o /dev/null -w '%{http_code}' -c "$JAR" -b "$JAR" \
        -F "work=@$WORK" "http://127.0.0.1:$PORT/handin" 2>/dev/null)
case "$NOTOK" in
  200|303) ok E1 "a browser that lost the hidden field can still hand work in ($NOTOK)" ;;
  *)       no E1 "a hand-in without the hidden field returned $NOTOK" ;;
esac

if [ -n "$TOK" ]; then
    UP=$(timeout 30 curl -s -o /dev/null -w '%{http_code}' -c "$JAR" -b "$JAR" \
         -F "token=$TOK" -F "work=@$WORK" "http://127.0.0.1:$PORT/handin" 2>/dev/null)
    case "$UP" in
      200|303) ok E2 "work handed in with the field present was accepted ($UP)" ;;
      *)       no E2 "hand-in returned $UP" ;;
    esac
else
    sk E2 "no hidden field found on the page"
fi
sleep 2

AFTER=$(find "$ROOT" -path "*handed-in/waiting*" -type f 2>/dev/null | wc -l)
LANDED=$(find "$ROOT" -path "*handed-in/waiting*" -type f 2>/dev/null | head -1)
if [ -n "$LANDED" ]; then
    ok E3 "the work arrived as $(basename "$LANDED")"
    if echo "$LANDED" | grep -q "Test Device"; then
        ok E3b "it is filed under the name the device gave"
    else
        no E3b "the filename does not carry the device name"
    fi
else
    no E3 "nothing arrived in handed-in/waiting"
    sk E3b "nothing arrived"
fi

# The same bytes sent twice must not become two pieces of homework.
NEW=$((AFTER - BEFORE))
if [ "$NEW" = 1 ]; then
    ok E5 "the same work sent twice was filed once, so repeats collapse"
else
    no E5 "two identical hand-ins produced $NEW files, expected 1"
fi

# And whatever landed must not be handed back out to the class. The path has a
# space in it, so it has to be encoded or curl never asks the question.
if [ -n "$LANDED" ]; then
    REL=${LANDED#"$ROOT/"}
    ENC=$(python3 -c "import urllib.parse,sys;print(urllib.parse.quote(sys.argv[1]))" "$REL")
    back=$(code "http://127.0.0.1:$PORT/$ENC")
    case "$back" in
      404|403) ok E4 "handed-in work is not served back to the class ($back)" ;;
      200)     no E4 "handed-in work IS served back to the class ($back)" ;;
      *)       no E4 "unexpected code $back asking for the handed-in file" ;;
    esac
else
    sk E4 "nothing landed to ask for"
fi

for p in $(pgrep -f "$HUB serve"); do kill "$p" 2>/dev/null; done
sleep 1

sec "Totals"
say "PASS=$P  FAIL=$F  SKIP=$S"
{ echo; echo "PASS=$P FAIL=$F SKIP=$S"; } >>"$SUM"
"$HERE/network-watchdog.sh" stop >/dev/null 2>&1
say "default route: $(ip route show default | head -1)"
say "summary: $SUM"
