<!-- Version: 1.4.0 · updated 26-08-25-12-01 -->
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

**Field tested 25 August 2026** against a real phone on a real hotspot: nine
releases in one morning, every one of them from somebody tapping a button and
saying it did nothing. The whole record, including what broke, is in the two
documents below.

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
| [docs/HOW-TO.md](docs/HOW-TO.md) | **start here if you want to use it.** Step by step with pictures, written for somebody who has never opened a terminal |
| [docs/WHY-THIS-EXISTS.md](docs/WHY-THIS-EXISTS.md) | the layman track. What the problem is and what was proved, in plain language |
| [docs/DEVELOPER.md](docs/DEVELOPER.md) | the developer track. Architecture, wire format, every measurement with its method |
| [docs/SCREENSHOTS.md](docs/SCREENSHOTS.md) | every screen, photographed on real hardware |
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
| **with the whole screen added** | **596,960** |
| the same thing built for Windows | **521,216** |

The screen cost 82,480 bytes. On the connections this is for, about ten seconds. On the connections this is for, that is about ten
seconds.

The access point rig used to take the measurements lives separately in
`Scripts.For.Work/wifi-ap-lab/`, because those are bench instruments rather
than part of the product.

## Installing

Built packages for all three are on the
[releases page](https://github.com/gorillanobakaa-dot/Gorilla.Portable.Network.Hub/releases).
[docs/HOW-TO.md](docs/HOW-TO.md) walks through each one with pictures.

**Debian, Ubuntu, Mint:**

```
sudo dpkg -i gorilla-portable-network-hub_0.7.1_amd64.deb
```

**Arch, CachyOS, Manjaro:**

```
sudo pacman -U gorilla-portable-network-hub-0.7.1-1-x86_64.pkg.tar.zst
```

Or from source with `makepkg -si` in `packaging/`. The Arch package is
assembled to spec and structurally verified on a Debian machine; it has not yet
been installed on an Arch one, and that is exactly the kind of thing worth
telling us about.

**Windows:** unzip `hub-0.7.1-windows-x86_64.zip` and read
`READ-THIS-FIRST.txt`. Windows will not let a normal program create a wifi
network, so you switch the hotspot on in Settings first. Everything else works
the same.

**From this repository:**

```
cd src/hub && cargo build --release && cd ../..
./packaging/build-deb.sh          # or ./packaging/build-arch.sh
sudo dpkg -i packaging/build/gorilla-portable-network-hub_0.7.1_amd64.deb
```

It installs `hub`, a menu entry called **Portable Network Hub**, a man page, and
both documentation tracks under `/usr/share/doc`. Nothing is required at run
time beyond the C library; NetworkManager is only needed to CREATE a wifi
network, not to hand files out over one that already exists.

The icon is rendered at every size from one master rather than one file copied
into nine slots, which is what makes it sharp in a menu instead of grainy. The
build refuses to finish if any two sizes turn out to be the same file.

**One name clash to know about:** Debian already ships a package called `hub`
(GitHub's command-line wrapper), which also owns `/usr/bin/hub`. They cannot be
installed at the same time. dpkg will say so rather than overwrite anything, and
the binary name here is not settled yet.

## How it is used

**Handing out.** Open `hub`, choose *Hand out files to the class*, point it at a
folder (a USB drive is fine, that is what teachers actually carry), tick which
files the class may see, and optionally give the wifi network a name and a
password. It shows the address for the board and one line per device.

**The kids need nothing installed, ever.** A phone or laptop that joins the
wifi is told by its own operating system that something is waiting: the same
"Sign in to this network" screen every hotel wifi uses, and the sign-in screen
IS the class page. Big buttons: READ or PLAY opens a file right there (video
streams instead of filling an 8 GB phone), GET IT keeps a copy in Downloads.
The page also has *Hand in your work*, which takes **several files at once**
and understands the formats a real curriculum produces (Word, Excel,
PowerPoint, OpenDocument, PDF, Markdown, CSV, zip), and *Send a note to your
teacher*. No addresses typed, no apps, no JavaScript needed, works in browsers
back to 2009.

**Every device says who it is.** Each one is asked its name once, and every
note and every piece of work is filed with that name, the device, and a short
tag derived from the hardware that a child cannot type their way out of. When
two devices claim one name the teacher is told, rather than left to work it
out.

**Nothing lands on the teacher's computer unasked.** Work arrives in a holding
area and waits. She sees who sent it, what it is and how big, and accepts or
refuses without opening anything. A refusal is kept, never deleted. Nothing
sent in is ever handed back out to the class, not even while it waits.

**The teacher stays in charge.** A notice at the top of every kid's page (the
blackboard, duplicated), a tick list that publishes or withdraws a file live
during the lesson, and a roster naming every device: who is getting, who is
handing in, how long is left, and who has not opened the page yet.

**Two ways to remove somebody, and they are not the same.** Pausing a device
leaves it on the wifi and cuts it off from the lesson: no files, no handing in,
no notes, and a page that says its teacher paused it. It refreshes itself, so
letting them back in needs nothing from the child. Changing the wifi password
knocks the whole room off at once. The screen is honest about the difference:
a pause recognises a device, and a phone can come back wearing a different
name, which is what the password is for.

**Joining by camera, as a bonus and never the way in.** Press `j` and the
screen draws a code a phone camera can read. The network name and password stay
printed underneath at the same size, because plenty of these phones have a
cracked camera or a camera app that wants an account first, and a screen
showing only a code locks those children out invisibly.

**Going and getting, tool to tool.** Open `hub`, choose *Get files from another
computer*. It looks for the teacher by itself and downloads with resume and
per-piece verification, which is the upgrade over a browser for huge files on a
bad signal.

Everything is still there from the command line, for anybody who prefers it:

```
hub serve ~/lessons --name Classroom --password chalkdust --notice "Test on Friday."
hub get http://10.42.0.1/lessons.zip
hub doctor
```

## Tell us how it went

This has been used in one room, on one morning, by one person. A report from a
second room is worth more than anything that can be worked out from here.

[Open an issue](https://github.com/gorillanobakaa-dot/Gorilla.Portable.Network.Hub/issues).
There are two forms, one for when it went wrong and one for when it worked, and
neither needs you to be technical. The most useful box on either is the one
asking what was awkward even though it worked.

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
