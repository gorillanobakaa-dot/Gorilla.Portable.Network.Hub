<!-- Version: 1.1.0 · updated 26-08-22-17-12 -->
# Burst transfer notes: what LocalSend does, what it cannot do, and what to build instead

Working notes from a brainstorming session on 2026-08-22. Source read,
hardware measured, design sketched. Nothing here has been built yet.

## Read these in order

| file | what is in it |
|---|---|
| `01-how-localsend-works.md` | What the shipped source actually does, with file and line references |
| `02-measurements.md` | Every number taken on this machine, and the command that produced it |
| `03-design-burst-carousel.md` | The design this session arrived at, and why |
| `04-corrections.md` | Claims made during the session that turned out to be wrong, and what replaced them |
| `05-open-questions.md` | What has to be measured before any of this is worth building |
| `06-device-constraints.md` | The real device zoo (Android Go, ChromeOS, Windows XP) and what it forces the design to become |
| `bench/compression-bench.sh` | Reusable benchmark that produced the compression table |

## Evidence marking

Every claim in these files carries a tag:

- **[measured]** a number taken on this machine on 2026-08-22, with the command recorded
- **[source]** read directly out of the LocalSend 1.18.2 tree, cited by file and line
- **[inference]** reasoning from the above, not itself measured. Treat as a hypothesis

Nothing tagged `[inference]` should be quoted later as a fact. Several of them
are specifically listed in `05-open-questions.md` as things to go and measure.

## The plain-language version

LocalSend moves files between two devices over your local Wi-Fi. It never
touches the internet, because everything it does is confined to the network in
the room. That part works exactly as advertised.

It was slow on this laptop for a reason that has nothing to do with the app: a
2012 Wi-Fi card can only carry about 5 MB per second, and when two devices talk
through a shared access point every byte crosses the air twice, so it halves
again to roughly 2.6 MB per second. The app was not the bottleneck. The radio
was.

Two things it genuinely cannot do, and these are the reasons to build something:

1. **It cannot create a network.** It assumes one already exists and that
   someone has let you onto it. In a school where the router is locked in a
   cage, that assumption fails completely.
2. **It can only send to one device at a time.** Sending the same folder to
   thirty laptops means sending the same bytes thirty times over one shared
   radio. Airtime is the only resource in the room and this spends thirty times
   more of it than necessary.

The proposed answer borrows from a 1944 German naval radio system called
Kurier, which transmitted a whole message in a single half-second burst with no
handshake and no acknowledgement, because a submarine that transmitted twice
got found. The same discipline solves the classroom problem for a different
reason: everything that is not payload is stolen airtime. Prepare the whole
package in advance, broadcast it once to everyone at the same time, repeat the
broadcast in a loop so late arrivals catch up, and never send anything back.

## Status

Brainstorm. Not a plan, not a commitment, not code.

**Revised 2026-08-22 (later):** `06-device-constraints.md` changed the shape of
the design. The elegant no-association broadcast mode turned out to be Linux
only, and the target devices include Android Go phones, Chromebooks and Windows
XP laptops. The design is now asymmetric: one capable sender, and receivers
split into a native multicast tier and a plain-HTTP browser tier for anything
that cannot install software.

`03-design-burst-carousel.md` keeps its original reasoning with a revision note
appended rather than edited in place, so the change of mind stays visible.

The next step is `05-open-questions.md` item 7, confirming the Go platform
support cutoffs. It is a ten minute job and it gates the toolchain for
everything built afterwards.
