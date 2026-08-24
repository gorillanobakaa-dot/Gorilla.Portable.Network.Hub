<!-- Version: 1.1.0 · updated 26-08-24-22-12 -->
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
properly configured access point, files move across it with resume and
per-piece verification, and there is now a screen to drive it from instead of a
list of flags. What is unproven: everything involving thirty devices in one
room at once.

Run `hub` with no arguments and it opens the screen. That is the way in for
anybody who is not the person who wrote it.

## Read these in order

| | |
|---|---|
| [docs/WHY-THIS-EXISTS.md](docs/WHY-THIS-EXISTS.md) | the layman track. What the problem is and what was proved, in plain language |
| [docs/DEVELOPER.md](docs/DEVELOPER.md) | the developer track. Architecture, wire format, every measurement with its method |
| [bench/](bench/) | the raw research: source reading, measurements, corrections, open questions |

Both tracks are complete. The layman one is a different language, not a
simplification.

## What is here

One binary, `hub`, with a screen and four commands inside it.

```
src/hub/src/tui.rs      the screen a teacher uses: hand out, or go and get
src/hub/src/term.rs     raw mode, size, keys, and a frame that is exactly
                        the height of the terminal
src/hub/src/net.rs      finding the other machine, and making the wifi network
src/hub/src/serve.rs    HTTP/1.1 with byte ranges and keep-alive
src/hub/src/fetch.rs    parallel, resumable, verifying client
src/hub/src/sums.rs     per-piece SHA-256 across every core
src/hub/src/sha256.rs   142 lines, no dependencies
bench/                  measurements, design notes, corrections
```

Rust, `std` only, **no dependencies at all**, screen included. There is no
terminal-UI library in here: a mainstream Rust TUI stack would add more to the
download than everything else in the program put together, to draw text that
ANSI has drawn since 1979.

| | bytes |
|---|---|
| three separate command-line tools | 1,262,576 |
| merged into one binary | 514,480 |
| **with the whole screen added** | **597,648** |
| the same thing built for Windows | **520,704** |

The screen cost 83,168 bytes. On the connections this is for, that is about ten
seconds.

The access point rig used to take the measurements lives separately in
`Scripts.For.Work/wifi-ap-lab/`, because those are bench instruments rather
than part of the product.

## How it is used

**Handing out.** Open `hub`, choose *Hand out files to the class*, point it at a
folder, and optionally give the wifi network a name and a password. It shows the
address to write on the board and one line per device as they connect.

**Going and getting.** Open `hub`, choose *Get files from another computer*. It
looks for the teacher by itself: whoever runs the hotspot is the gateway, so
that is the first place it asks, and if the room has a real router it sweeps the
local addresses instead. Pick a file and it downloads, resuming by itself if the
signal drops.

Everything is still there from the command line, for anybody who prefers it:

```
hub serve ~/lessons --name Classroom --password chalkdust
hub get http://10.42.0.1:8080/lessons.zip
hub doctor
```

## A network that comes back

Making a wifi network **replaces the teacher's own wifi** while the lesson runs.
Putting it back is not left to a shutdown handler, because a shutdown handler
does not run when a process is killed. A systemd timer owned by the operating
system, re-armed every minute while the lesson is going, restores the previous
connection within three minutes of the tool stopping for any reason at all.

That is written the hard way because it was learned the hard way: this machine
locked itself off its own network twice during development, both times because
the teardown lived somewhere that never ran.

## The one-line summary of a day's work

The broadcast rate went from **1 Mbit/s to 54** by changing one line of
configuration. A fourteen-year-old laptop sustained **56 Mbit/s at 78%
efficiency** while being the network. **The hardware was never the limit. The
defaults were.**

## Name

Working title. The binary is called `hub` and the rest is not named yet,
deliberately. Criteria: named for
what it does rather than the mechanism, no vanity prefix, comprehensible to a
teacher with no technical background, **no charity connotation**, and it has to
tab-complete cleanly.
