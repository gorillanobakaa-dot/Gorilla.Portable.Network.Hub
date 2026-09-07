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
# A HOST TOOLCHAIN THAT CAN COMPILE A BUILD SCRIPT
#
# build.rs is compiled for the HOST, not the target, whatever is being
# cross-compiled to. On Windows the rustup default host is MSVC, which needs
# link.exe from Visual C++ Build Tools, which is not installed here. So adding
# build.rs for the Windows icon broke this script with "linker link.exe not
# found", while the Windows build itself was fine, and the error named a
# Microsoft linker during a build for Linux: an error message pointing at
# nothing to do with the problem.
#
# The GNU host toolchain links with the gcc that is already here for zig:
#
#   rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal
#   rustup target add x86_64-unknown-linux-musl --toolchain stable-x86_64-pc-windows-gnu
#
# REQUIREMENTS
#   rustup, with the x86_64-unknown-linux-musl target installed
#   on Windows: the stable-x86_64-pc-windows-gnu toolchain, as above
#   zig on PATH
#
#   rustup target add x86_64-unknown-linux-musl
#   # zig: https://ziglang.org/download/ , or `scoop install zig`, or your
#   # distribution's package
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TARGET=x86_64-unknown-linux-musl
OUT=$ROOT/src/hub/target/$TARGET/release/hub

# zig is the preferred linker because one download covers every host, including
# a Windows machine with no Visual C++ Build Tools. But it is not the only one,
# and requiring it meant a plain Debian box could not build the Linux release at
# all, despite Debian shipping a musl toolchain in its own archive:
#
#   sudo apt install musl-tools
#   rustup target add x86_64-unknown-linux-musl
#
# Verified on the VAIO 2026-09-07: musl-gcc produced a working static binary of
# 992,592 bytes that runs and reports its version. NOTE it is static-pie where
# the zig build is plain static, so the two are not the same size and a figure
# from one must not be quoted for the other.
USE=""
if command -v zig >/dev/null 2>&1; then
    USE=zig
elif command -v musl-gcc >/dev/null 2>&1; then
    USE=musl-gcc
else
    echo "Neither zig nor musl-gcc is on PATH." >&2
    echo "  zig:      https://ziglang.org/download/" >&2
    echo "  or musl:  sudo apt install musl-tools" >&2
    echo "and either way: rustup target add x86_64-unknown-linux-musl" >&2
    exit 1
fi
echo "linking with $USE"

# A wrapper, because cargo wants one command and zig needs two words plus a
# target triple. Written next to the build rather than into the source tree.
WRAP=$(mktemp -d)
trap 'rm -rf "$WRAP"' EXIT
if [ "$USE" = musl-gcc ]; then
    # musl-gcc is already one command taking cc arguments, so it needs no
    # wrapper and no target triple. cargo is told to use it directly.
    LINKER=musl-gcc
    export CC_x86_64_unknown_linux_musl=musl-gcc
fi
[ "$USE" = musl-gcc ] || case "$(uname -s 2>/dev/null || echo Windows)" in
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

# On Windows, build with the GNU host so build.rs compiles without MSVC.
#
# `cargo +toolchain` is a rustup directive, not a cargo one, and a plain cargo
# answers "no such command: `+stable-x86_64-pc-windows-gnu`". That is not an
# academic case. A machine can have the toolchain AND the musl standard
# library both installed and still have no rustup on PATH, because the shim
# lives in a package manager's shim directory while the toolchains live in a
# persisted directory beside it, and only the former is on PATH. This machine
# was exactly that, and the error names the toolchain rather than the missing
# rustup, so it reads as "the toolchain is not installed" when it is.
#
# So: rustup when it is there, and the toolchain's own cargo when it is not.
CARGO="cargo"
TOOLCHAIN=""
case "$(uname -s 2>/dev/null || echo Windows)" in
    MINGW*|MSYS*|CYGWIN*|Windows)
        NEEDED=stable-x86_64-pc-windows-gnu
        if command -v rustup >/dev/null 2>&1; then
            TOOLCHAIN="+$NEEDED"
        else
            for home in "${RUSTUP_HOME:-}" "$HOME/.rustup"                         "${USERPROFILE:-}/scoop/persist/rustup/.rustup"                         "$HOME/scoop/persist/rustup/.rustup"; do
                [ -n "$home" ] || continue
                if [ -x "$home/toolchains/$NEEDED/bin/cargo.exe" ]; then
                    CARGO="$home/toolchains/$NEEDED/bin/cargo.exe"
                    break
                fi
            done
            if [ "$CARGO" = "cargo" ]; then
                echo "Need the $NEEDED toolchain, and neither rustup nor the" >&2
                echo "toolchain itself could be found. Install rustup, then:" >&2
                echo "  rustup toolchain install $NEEDED --profile minimal" >&2
                echo "  rustup target add $TARGET --toolchain $NEEDED" >&2
                exit 1
            fi
            # cargo does not carry its own rustc. Called directly it runs
            # whichever rustc is on PATH, and that one has a different
            # sysroot: it reported "can't find crate for `std`" and advised
            # installing a target that was already installed, in the other
            # toolchain. Put this toolchain's bin first so cargo and rustc
            # agree about where the standard library lives.
            PATH="$(dirname "$CARGO"):$PATH"
            export PATH
            echo "rustup is not on PATH; using $CARGO directly."
        fi
        ;;
esac

# link-self-contained differs between the two linkers, and getting it wrong
# fails at the link step with a message that names neither.
#
#   zig      ships its own musl CRT. Leaving rustc's self-contained objects in
#            as well duplicates _start and the link fails on a symbol clash,
#            which is why this is "no" for zig.
#   musl-gcc ships the CRT but NOT libunwind, so rustc's self-contained objects
#            are the only source of it. With "no" the link fails on
#            "cannot find -lunwind", which reads like a missing system package
#            and is not one.
if [ "$USE" = musl-gcc ]; then
    RFLAGS="-C target-feature=+crt-static"
else
    RFLAGS="-C target-feature=+crt-static -C link-self-contained=no"
fi
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="$LINKER" CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="$RFLAGS"     "$CARGO" $TOOLCHAIN build --release --target "$TARGET"

# Verify rather than assume. A dynamically linked artifact here would work on
# the build machine and fail on the machines this exists for, which is the
# worst way for it to fail.
#
# The test is "does it need an interpreter", not "does file say the words
# statically linked". A static-pie binary IS static, and the zig build and the
# musl-gcc build disagree on which of the two phrasings `file` prints, so the
# literal check rejected a perfectly good binary and would have sent somebody
# hunting a linking fault that did not exist. An ELF that needs no PT_INTERP
# loads with no dynamic loader present, which is the property being claimed.
if command -v readelf >/dev/null 2>&1; then
    command -v file >/dev/null && file "$OUT"
    if readelf -l "$OUT" 2>/dev/null | grep -q "INTERP"; then
        echo "Needs a dynamic loader. NOT static. Do not ship this." >&2
        exit 1
    fi
    echo "verified: no interpreter, so it needs no loader on the target"
elif command -v file >/dev/null; then
    file "$OUT"
    file "$OUT" | grep -qE "statically linked|static-pie linked" || {
        echo "NOT statically linked. Do not ship this." >&2
        exit 1
    }
fi

echo
echo "Built $OUT"
ls -la "$OUT"
