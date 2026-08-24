<!-- Version: 1.0.0 · updated 26-08-22-17-11 -->
# The device zoo, and what it does to the design

Added 2026-08-22, later the same session. This is the constraint that reshapes
the design rather than a detail to handle at the end. It supersedes part of
`03-design-burst-carousel.md`; see the revision note at the end of that file.

## The real target hardware

Not "old laptops". A school in Somalia, or anywhere similarly under-resourced,
will present the full range at once:

- 1 GB RAM Android phones, including Android Go edition
- Cheap or old Chromebooks, often without Crostini, often locked down
- Windows XP and Windows 2000 laptops, still in service
- Windows 7 laptops, very common
- Old Linux laptops, the best case

Anything built for this has to assume all of them are in the same room.

## The matrix

| device | native binary? | UDP multicast receive? | verdict |
|---|---|---|---|
| Old Linux laptop | yes | yes | best case, everything works |
| Android 5 to 14, incl. Go edition | yes, an APK | yes, needs a `MulticastLock` | works, with memory limits |
| Windows 7 | yes, but Go 1.20 or older | yes | works, toolchain pinned |
| Windows XP / 2000 | realistically no | n/a | browser only |
| ChromeOS, cheap or old | no, unless Crostini exists | no | browser only |

### Two hard blocks

**[inference, needs verifying]** Go dropped Windows XP and Vista support at
1.11, and dropped Windows 7 and 8 at 1.21. A Windows 7 receiver therefore pins
the toolchain to Go 1.20 permanently, and an XP receiver is not reachable with
Go at all. Confirm against current Go release notes before committing to a
toolchain; this is item 7 in `05-open-questions.md`.

**[fact]** Browsers cannot receive UDP multicast. There is no API for it and
there is not going to be one. Any device where software cannot be installed is
permanently outside the carousel, by construction.

## What dies: Mode B

Raw 802.11 injection with no association, described in
`03-design-burst-carousel.md`, was the most elegant part of the design and it is
**Linux only**.

- Windows: no usable monitor or injection path without special drivers
- ChromeOS: no raw sockets at all
- Android: needs root

The mode that scaled to unlimited receivers works on precisely the devices least
likely to be in that room. Keep it as a Linux-to-Linux fast path if it is ever
built. It cannot be the architecture.

## What replaces it: asymmetric requirements

The insight that survives the matrix: **the sender is one machine you control,
the receivers are the zoo.** So load every hard requirement onto the sender and
make the receiver as close to nothing as possible.

### Tier 1 receiver: native app, multicast carousel

Android, Linux, Windows 7 and newer. These get the design as sketched in `03`.
One transmission serves all of them simultaneously. This is where the airtime
saving lives and it should cover most of the room.

### Tier 2 receiver: a web browser, plain HTTP unicast

The sender also runs a small HTTP server. Anything that cannot install software
opens a URL and downloads: XP, ChromeOS, a locked-down tablet, a device nobody
has the admin password for.

It is N copies over the air and it is slow. It is also the difference between
that child getting the file and not getting it.

**[source]** LocalSend already proves this tier is viable. There is a web-send
path in its tree (`packages/core/src/http/server/web.rs`, with a
`v2_web_send.rs` test) which exists for exactly this reason: serving devices
that do not have the app.

### Why this is a better shape anyway

The carousel stops being the whole architecture and becomes the optimisation
that handles the bulk of the room, with a universal fallback underneath it. The
system degrades instead of failing. A design that only works when every device
cooperates is not a design for this environment.

## Serve the fallback over plain HTTP, not HTTPS

A self-signed certificate on a LAN gives an ancient browser either a hard
failure or a security warning that a twelve year old is expected to click
through, which is a bad thing to teach.

**[inference]** Chrome 49 and Firefox 52, the last XP-capable builds, do speak
TLS 1.2, so it is not hopeless. But the certificate warning is unavoidable and
the failure modes are ugly.

The pre-shared key model from `03` solves this properly and this is where it
earns its place twice over: encrypt the payload **once, before it ever reaches
the wire**, with the key written on the whiteboard. The transport then does not
need to be encrypted at all, so it can be served over the dumbest possible HTTP
to the dumbest possible browser without losing anything.

Note this is also the honest reading of LocalSend's own settings: HTTPS there is
a toggle, not a law (`packages/core/src/model/discovery.rs:19`,
`app/lib/provider/persistence_provider.dart:494`).

## Android Go specifically

**1 GB of RAM means the receiver must never hold the payload in memory.** The
fountain decoder needs a working set of symbols, and for a 500 MB payload that
has to be disk-backed rather than RAM-backed.

Decide this now, at design time. Discovering it later on a device you cannot
attach a debugger to is the expensive version.

The upside: Android has `LocalOnlyHotspot`, an API that creates an access point
with no internet attached, which is this use case exactly. No root, no router. A
teacher's phone becoming the network is probably the most realistic deployment
of all.

## Effect on "where should this live"

`03-design-burst-carousel.md` left open whether this is its own binary or a mode
inside gorilla-opencode. **The browser fallback tier strengthens the
standalone-binary argument.**

The Tier 2 receiver is a URL. It has no install, no download, no dependency on
anything the school owns. Wrapping the sender side of that inside an AI coding
agent means a teacher downloads an agent in order to run a web server, which is
a strange thing to ask of a metered connection. A small static binary that says
"be the hotspot, serve this, broadcast this" is the thing that fits.

Not decided. But the balance moved.
