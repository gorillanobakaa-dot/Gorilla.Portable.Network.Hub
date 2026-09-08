#!/usr/bin/env bash
# check-both-builds.sh - a change has to be true on Windows and on Linux.
# Version: 1.1.0 - updated 26-09-08-11-30
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
# text landed only in the system it belongs to.
#
# 1.1.0 RUNS ON EITHER HOST. Until now it assumed a Linux host: the native
# build was the Linux one and the Windows one was cross-compiled with mingw.
# Run on Windows it would have built the Windows binary twice and then reported
# that every windows-only string was present in the "linux" build, which is the
# failure mode a check like this must not have: it passes, loudly, having
# checked nothing. The host is now detected, the OTHER target is the one that
# gets cross-compiled, and the binary that can be executed natively is the one
# that gets run.
#
# Needs, on a Linux host:   rustup target add x86_64-pc-windows-gnu, mingw-w64
#                           optional: wine, to run the Windows binary
# Needs, on a Windows host: whatever packaging/build-linux-static.sh needs,
#                           which is zig or musl-gcc plus the musl target
#
# Neither host can run the other's binary unaided, so on Windows the Linux
# binary is compiled and inspected but not executed. That is a real gap and it
# is said out loud in the output rather than left to be assumed otherwise.

set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/src/hub"
FAIL=0
say() { printf '%s\n' "$*"; }
ok()  { printf '[ok]   %s\n' "$*"; }
no()  { printf '[FAIL] %s\n' "$*"; FAIL=$((FAIL+1)); }

case "$(uname -s 2>/dev/null || echo Windows)" in
    MINGW*|MSYS*|CYGWIN*|Windows) HOST=windows ;;
    *)                            HOST=linux   ;;
esac
say "host: $HOST"
say ""

say "== building both targets =="
if [ "$HOST" = linux ]; then
    LIN=target/release/hub
    WIN=target/x86_64-pc-windows-gnu/release/hub.exe
    cargo build --release >/dev/null 2>&1 && ok "linux build (native)" || { no "linux build"; exit 1; }
    if ! cargo build --release --target x86_64-pc-windows-gnu >/dev/null 2>&1; then
        no "windows build (need: rustup target add x86_64-pc-windows-gnu, and mingw-w64)"
        exit 1
    fi
    ok "windows build (cross)"
else
    LIN=target/x86_64-unknown-linux-musl/release/hub
    WIN=target/release/hub.exe
    cargo build --release >/dev/null 2>&1 && ok "windows build (native)" || { no "windows build"; exit 1; }
    # The same script the release is cut with, rather than a second recipe here
    # that could drift away from it.
    if ! bash "$ROOT/packaging/build-linux-static.sh" >/dev/null 2>&1; then
        no "linux build (see packaging/build-linux-static.sh for what it needs)"
        exit 1
    fi
    ok "linux build (cross, static musl)"
fi

# Text that belongs to one system must not be in the other's binary. Listed as
# "needle : which build may contain it".
#
# grep straight at the file rather than strings(1): a Windows host has grep,
# from git, and generally has no strings at all. -a treats the binary as text,
# which is all this needs.
check() { # check <needle> <linux|windows>
    local needle="$1" side="$2" inl inw
    # grep -c prints its count AND exits 1 when the count is zero, so a plain
    # `|| echo 0` appends a second line and every later [ ] test dies on
    # "integer expected". `|| true` keeps the count it already printed.
    inl=$(grep -acF -- "$needle" "$LIN" 2>/dev/null || true)
    inw=$(grep -acF -- "$needle" "$WIN" 2>/dev/null || true)
    inl=${inl:-0}
    inw=${inw:-0}
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
if [ "$HOST" = linux ]; then
    NATIVE="$LIN"; OTHER="$WIN"; NATIVE_NAME=linux;   OTHER_NAME=windows
else
    NATIVE="$WIN"; OTHER="$LIN"; NATIVE_NAME=windows; OTHER_NAME=linux
fi

NV=$("$NATIVE" --version 2>/dev/null | tr -d '\r' | tail -1)
ok "$NATIVE_NAME says: $NV"

RAN=""
if [ "$HOST" = linux ]; then
    if command -v wine64 >/dev/null 2>&1 || [ -x /usr/lib/wine/wine64 ]; then
        W=$(command -v wine64 || echo /usr/lib/wine/wine64)
        RAN=$(WINEDEBUG=-all timeout 120 "$W" "$OTHER" --version 2>/dev/null | tr -d '\r' | tail -1)
    fi
fi

if [ -n "$RAN" ]; then
    if [ "$RAN" = "$NV" ]; then
        ok "$OTHER_NAME says the same when run: $RAN"
    else
        no "$OTHER_NAME says '$RAN', $NATIVE_NAME says '$NV'"
    fi
else
    # Not run, so the version is read out of the file instead. Weaker evidence
    # than running it, and labelled as such. It still catches the thing most
    # likely to be wrong with a binary nobody executed: a stale one left in
    # target/ from an earlier version.
    WANT=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
    if grep -aqF "hub $WANT" "$OTHER"; then
        ok "$OTHER_NAME carries the string 'hub $WANT' (NOT run: this host cannot execute it)"
    else
        no "$OTHER_NAME does not contain 'hub $WANT'"
    fi
fi

say ""
if [ "$FAIL" = 0 ]; then say "both builds agree."; else say "$FAIL problem(s)."; fi
exit "$FAIL"
