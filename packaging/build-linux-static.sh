#!/bin/bash
# Version: 1.0.0 · updated 26-09-06-14-30
#
# Build the Linux binary, statically linked against musl, from any host.
#
# WHY STATIC, AND WHY MUSL
#
# The .deb and the Arch package link against the glibc of whatever machine
# built them, so a package built on Debian 13 refuses to start on Debian 11
# with a GLIBC_2.34 not found. The people this tool is for are running whatever
# was on the machine when it was donated. A statically linked musl binary has
# no such dependency: one file, no loader, runs on any distribution and any
# glibc, including Alpine, which has no glibc at all.
#
# WHY ZIG AND NOT A CROSS-COMPILER
#
# zig ships its own C compiler, linker and a copy of musl for every target it
# supports, in one download and with no system packages. `cargo-zigbuild` is
# the usual front end for this and is worth using where it installs, but it
# needs a working host C toolchain to build its own proc-macro dependencies,
# which a Windows machine without Visual C++ Build Tools does not have. This
# crate has no build scripts, no proc-macros and no C dependencies, so zig can
# be used as the linker directly and none of that applies.
#
# -C link-self-contained=no is not optional. Without it rustc supplies its own
# musl CRT alongside zig's and the link fails on duplicate _start and
# _start_c.
#
# REQUIREMENTS
#   rustup, with the x86_64-unknown-linux-musl target installed
#   zig on PATH
#
#   rustup target add x86_64-unknown-linux-musl
#   # zig: https://ziglang.org/download/ , or `scoop install zig`, or your
#   # distribution's package
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TARGET=x86_64-unknown-linux-musl
OUT=$ROOT/src/hub/target/$TARGET/release/hub

command -v zig >/dev/null || { echo "zig is not on PATH" >&2; exit 1; }

# A wrapper, because cargo wants one command and zig needs two words plus a
# target triple. Written next to the build rather than into the source tree.
WRAP=$(mktemp -d)
trap 'rm -rf "$WRAP"' EXIT
case "$(uname -s 2>/dev/null || echo Windows)" in
    MINGW*|MSYS*|CYGWIN*|Windows)
        printf '@echo off\r\nzig cc -target x86_64-linux-musl %%*\r\n' > "$WRAP/zigcc.bat"
        LINKER="$WRAP/zigcc.bat"
        ;;
    *)
        printf '#!/bin/sh\nexec zig cc -target x86_64-linux-musl "$@"\n' > "$WRAP/zigcc"
        chmod +x "$WRAP/zigcc"
        LINKER="$WRAP/zigcc"
        ;;
esac

cd "$ROOT/src/hub"
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="$LINKER" \
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-C target-feature=+crt-static -C link-self-contained=no" \
    cargo build --release --target "$TARGET"

# Verify rather than assume. A dynamically linked artifact here would work on
# the build machine and fail on the machines this exists for, which is the
# worst way for it to fail.
if command -v file >/dev/null; then
    file "$OUT"
    file "$OUT" | grep -q "statically linked" || {
        echo "NOT statically linked. Do not ship this." >&2
        exit 1
    }
fi

echo
echo "Built $OUT"
ls -la "$OUT"
