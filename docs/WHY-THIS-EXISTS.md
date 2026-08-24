<!-- Version: 1.0.0 · updated 26-08-24-19-48 -->
# Why this exists

Draft of the layman track, written 2026-08-24, destined for the repository
README when this ships. Every number in it was measured that day on a Sony VAIO
SVE14A3AJ from 2012, and the command that produced each one is recorded in
`Documents/localsend-1.18.2/gorilla.messing.around/bench/`.

---

## The problem, in one paragraph

A teacher in a village school has a folder of lessons and thirty laptops. There
is no internet, no router, and often no mains electricity. Every existing tool
for moving files assumes a network already exists. **None of them can make one.**
So the teacher is stuck with a USB stick and thirty trips around the room.

## What we set out to prove

That a teacher's own laptop, even a very old one, can **become the network**, and
hand the folder to the whole class at once.

## What was proved, with numbers

| | |
|---|---|
| Moved a 4 GB file with no router and no internet | **6.1 MB/s**, completed |
| Peak speed off a 2012 laptop's wifi card | **56 Mbit/s** |
| Efficiency against the hardware's theoretical maximum | **78%** (typical is 55 to 65) |
| Speed of broadcasting to a whole room, before the fix | 1 Mbit/s |
| After changing **one line** of configuration | **54 Mbit/s** |

That last row is the whole day in miniature. The laptop was broadcasting at one
megabit because the default is written to accommodate a device from 1999 that
nobody has owned in twenty years. **The hardware was never the limit. The
defaults were.**

## Three things nobody had noticed

**Windows runs away.** A Windows laptop joining a network with no internet checks
with Microsoft, gets no answer, decides the network is broken, and leaves.
**Sixteen seconds.** In a classroom that means every transfer dies repeatedly and
nobody knows why. Fixed by answering the check locally.

**LocalSend cannot resume.** If the connection drops it discards everything it
received and starts the file again from zero. On a wobbly link that made it **two
hundred times slower**: 30 kilobytes a second instead of 6 megabytes. It looks
like slowness. It is actually amnesia. This is not a criticism of LocalSend,
which is good software built for a home network that stays up. It is a statement
about which assumptions fail in a classroom.

**More connections do not go faster.** Tested at 1, 2, 4, 8, 16 and 32
connections back to back on the same link, one after another, same file:

| connections | speed |
|---|---|
| 1 | 6.51 MB/s |
| 2 | 6.55 |
| **4** | **6.57**, and the steadiest of the six |
| 8 | 6.36 |
| 16 | 6.14 |
| 32 | 6.20 |

The peak in every single row is 7.0 MB/s. The ceiling never moves, because the
limit is airtime and no amount of software cleverness makes more air. Above four
it gets slower and less predictable. This matters for a full class: thirty
children at four connections each is 120 on the teacher's laptop, where the
obvious "more is faster" instinct would have put 960 there for nothing.

## What was built

Two programs, in Rust, **380 kilobytes each**. That size matters: on the
connections these children have, these tools download in twenty seconds while a
typical alternative takes forty minutes.

They resume where they left off, verify every piece against a checksum, keep the
laptop awake so it does not fall asleep mid-transfer, and size themselves to
whatever machine they are running on rather than to the machine they were
written on.

---

## The money question, answered honestly

This section stays in. It is the part that explains the method.

**What the budget bought was being told the assistant was wrong, repeatedly, by
evidence.** Ten times in one day a confident statement was contradicted by a
measurement:

- Predicted 400 connections would be 20% slower. They were 6% slower.
- Predicted memory would balloon to 6.4 gigabytes. It was 6.7 megabytes.
  **Wrong by a factor of a thousand.**
- Claimed a transfer had gone over the office wifi. The timestamps proved it was
  the laptop's own network.
- Said the network could not identify a client's capabilities. It could, and the
  owner's instinct was right.
- Suggested a phone hotspot might do the job. It cannot, for a reason that had
  to be measured rather than reasoned about.
- Quoted a hashing speed that turned out to be partly a caching illusion.
- Proposed an optimisation, then did the arithmetic and found it would change
  nothing.
- Predicted a wifi setting would collapse under load. It did not.

And the test suite found **three bugs in code written minutes earlier**,
including a verification feature that had never once run and said nothing about
it.

**That is the product.** Not speed of typing. A cheaper, more agreeable
assistant would have produced plausible numbers, agreed with the assumptions,
and left the discovery for a classroom in Somalia: that the broadcast runs at one
megabit, and that every Windows laptop leaves after sixteen seconds.

Every number above is measured on real hardware, recorded with the command that
produced it, and several of them contradict what was confidently expected.
**The expensive part is not the answers. It is the checking.**

---

## Status

Not finished. What works today: the laptop becomes a properly configured access
point, and files move across it with resume and per-chunk verification.

What is unproven: everything involving thirty devices in one room at once.
Nothing here has been tested beyond two machines, and the broadcast-to-everyone
design is measured only at the transmitter, never at a receiver. Those are
honest gaps and they are listed in
`bench/results-26-08-24-12-20.md` and `bench/results-26-08-24-17-00.md`.
