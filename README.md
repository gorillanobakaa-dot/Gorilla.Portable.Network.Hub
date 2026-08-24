<!-- Version: 1.0.0 · updated 26-08-24-21-21 -->
# Gorilla Portable Network Hub

<!-- WHO-THIS-IS-FOR: managed block, do not edit by hand -->

**Turn a laptop into the network: hand a folder to every device in the room, with no internet and no router.**

Built for the people every other tool prices out: kids with no credit
card, 15-year-old laptops, data sold by the megabyte. Free forever, by
design, not as a trial.
Why, with the numbers: [PHILOSOPHY.md](https://github.com/gorillanobakaa-dot/Gorilla.Opencode/blob/main/PHILOSOPHY.md)

<!-- /WHO-THIS-IS-FOR -->

A laptop that **creates a network where none exists**, and hands a folder to
every device in the room.

Built for classrooms with no internet, no router, and often no mains power. The
teacher's own laptop becomes the access point; the children's machines need no
privileges, no account, no setup and no internet, ever.

**Status: not finished.** What works and is measured: the laptop becomes a
properly configured access point, and files move across it with resume and
per-chunk verification. What is unproven: everything involving thirty devices in
one room at once.

## Read these in order

| | |
|---|---|
| [docs/WHY-THIS-EXISTS.md](docs/WHY-THIS-EXISTS.md) | the layman track. What the problem is and what was proved, in plain language |
| [docs/DEVELOPER.md](docs/DEVELOPER.md) | the developer track. Architecture, wire format, every measurement with its method |
| [bench/](bench/) | the raw research: source reading, measurements, corrections, open questions |

Both tracks are complete. The layman one is a different language, not a
simplification.

## What is here

```
src/fileserver    HTTP/1.1 with byte ranges and keep-alive       389 KB
src/fetch         parallel, resumable, verifying client          386 KB
src/sums          per-chunk SHA-256 across every core            394 KB
src/shared        SHA-256, 142 lines, no dependencies
bench/            measurements, design notes, corrections
```

Rust, `std` only, no dependencies at all. Cross-compiles to Windows with one
flag and comes out **smaller** than the Linux build.

The access point rig used to take the measurements lives separately in
`Scripts.For.Work/wifi-ap-lab/`, because those are bench instruments rather
than part of the product.

## The one-line summary of a day's work

The broadcast rate went from **1 Mbit/s to 54** by changing one line of
configuration. A fourteen-year-old laptop sustained **56 Mbit/s at 78%
efficiency** while being the network. **The hardware was never the limit. The
defaults were.**

## Name

Working title. The binary is not named yet, deliberately. Criteria: named for
what it does rather than the mechanism, no vanity prefix, comprehensible to a
teacher with no technical background, **no charity connotation**, and it has to
tab-complete cleanly.
