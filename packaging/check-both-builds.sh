#!/usr/bin/env bash
# check-both-builds.sh - a change has to be true on Windows and on Linux.
# Version: 1.0.0 - updated 26-09-08-11-20
#
# WHY THIS EXISTS. The two builds come from one source tree, so it is easy to
# fix something on the machine in front of you and ship it broken on the other.
# It happened twice in one afternoon:
#
#   - a port 80 message that read "needs administrator rights on Linux ...
#     install the .deb", printed on Windows, where none of that is true and
#     there is no .deb to install
#   - a destination path shown with canonicalize(), which on Windows returns
#     the extended-length form, so a teacher was told their work went into
#     \\?\C:\Users\... and reasonably concluded something had gone wrong
#
# Neither would fail a test. Both compile, both run, and both are wrong only on
# the machine nobody was looking at.
#
# This builds BOTH targets from the same tree and checks that the per-system
# text landed only in the system it belongs to. If wine is present it also runs
# the Windows binary, which catches more than compiling does.
#
# Needs: rustup target add x86_64-pc-windows-gnu, and mingw-w64 for the linker.
# Optional: wine.

set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/src/hub"
FAIL=0
say() { printf '%s\n' "$*"; }
ok()  { printf '[ok]   %s\n' "$*"; }
no()  { printf '[FAIL] %s\n' "$*"; FAIL=$((FAIL+1)); }

say "== building both targets =="
cargo build --release >/dev/null 2>&1 && ok "linux build" || { no "linux build"; exit 1; }
if ! cargo build --release --target x86_64-pc-windows-gnu >/dev/null 2>&1; then
    no "windows build (need: rustup target add x86_64-pc-windows-gnu, and mingw-w64)"
    exit 1
fi
ok "windows build"

LIN=target/release/hub
WIN=target/x86_64-pc-windows-gnu/release/hub.exe

# Text that belongs to one system must not be in the other's binary. Listed as
# "needle : which build may contain it".
check() { # check <needle> <linux|windows>
    local needle="$1" side="$2" inl inw
    inl=$(strings "$LIN" | grep -cF "$needle")
    inw=$(strings "$WIN" | grep -cF "$needle")
    case "$side" in
        linux)   [ "$inl" -gt 0 ] && [ "$inw" -eq 0 ] && ok "linux-only: $needle" || no "linux-only text is wrong (linux=$inl windows=$inw): $needle" ;;
        windows) [ "$inw" -gt 0 ] && [ "$inl" -eq 0 ] && ok "windows-only: $needle" || no "windows-only text is wrong (linux=$inl windows=$inw): $needle" ;;
    esac
}

say ""
say "== per-system text is in the right binary =="
check "install the .deb, which grants it once" linux
check "IIS, Skype or Windows" windows

say ""
say "== both agree on what they are =="
LV=$("$LIN" --version 2>/dev/null)
ok "linux says: $LV"
if command -v wine64 >/dev/null 2>&1 || [ -x /usr/lib/wine/wine64 ]; then
    W=$(command -v wine64 || echo /usr/lib/wine/wine64)
    WV=$(WINEDEBUG=-all timeout 120 "$W" "$WIN" --version 2>/dev/null | tr -d '\r' | tail -1)
    if [ "$WV" = "$LV" ]; then
        ok "windows says the same under wine: $WV"
    else
        no "windows says '$WV', linux says '$LV'"
    fi
else
    say "[skip] wine not installed, so the Windows binary was compiled but not run"
fi

say ""
if [ "$FAIL" = 0 ]; then say "both builds agree."; else say "$FAIL problem(s)."; fi
exit "$FAIL"
