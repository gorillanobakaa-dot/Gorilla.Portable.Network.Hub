<!-- Version: 1.3.0 · updated 26-08-25-08-14 -->
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

## Somebody other than us can use it now

Everything above worked before this was written, and that is not the same as
being usable. It meant knowing what a subcommand is, what an address is, that
the folder has to be handed out before anyone can ask for it, and which of six
options to type. A teacher standing in front of a class does not have that, and
should not need it.

Now the program opens a screen. There are two things on it:

- **Hand out files to the class.** Point it at a folder. Give the wifi network a
  name and a password if you want it to make one. It shows you the address to
  write on the board, and one line per child as they connect, with how far along
  each of them is.
- **Get files from another computer.** It goes and looks for the teacher by
  itself. If it cannot find them, you can type the address.

On Windows the program is a single file on a memory stick. Double-clicking it
opens the screen. That matters more than it sounds: a program started that way
is given no instructions at all, and the old version answered by printing a page
of help into a window that closed again, which is indistinguishable from being
broken.

## What it deliberately does not do

**No boxes, no lines, no borders.** Not a style choice. A terminal window is a
fixed number of rows and columns, and every line drawn round something takes
some of them away. When the contents no longer fit, the usual tools do not
complain, they fold the text onto the next line, so the mess turns up somewhere
other than the thing that caused it. Nothing is drawn round anything. The line
you are on is shown by swapping the colours, which takes no space at all.

**No jargon on the screen.** No addresses called sockets, no checksums, no
binding. "12 pieces had to be asked for again. That is normal on a weak signal
and nothing is lost by it." A teacher reading their third language should not
have to decode a message before they can act on it.

**Every failure says what to do next.** Not "connection refused". "Nothing is
handing out files at that address. Is the teacher's computer still running it?"

## The wifi always comes back

Making a network takes over the teacher's own wifi for as long as the lesson
lasts. Giving it back cannot be left to the program tidying up after itself,
because a program that is killed does not get to tidy up. Instead the operating
system is asked to hold a three-minute countdown that the program pushes back
every minute while it is running. Stop the program however you like, including
pulling the plug on it, and the wifi returns on its own.

This is written this way because the machine it was built on locked itself off
its own network twice, both times because the cleanup lived somewhere that never
ran. A teacher whose laptop loses its wifi after a lesson will not use the tool
a second time, and will never know what did it.

## The network needs a password

There is no option for an open network. Anyone in range of an open one can reach
the teacher's own laptop, and the teacher has no way of seeing who is on it. The
program offers a password of eight letters and numbers with nothing in it that
can be misread: no capital O next to a zero, no lowercase L next to a one. Write
it on the board.

## Two things a real room found that a test could not

The first hotspot made on real hardware worked, and then a phone joined it and
the screen said nobody had connected. It was only counting devices that were
**downloading**. Joining the wifi and waiting is what every device does first,
and the tool was blind to all of it. It now lists them, by name where the
computer knows the name and by address otherwise.

The second was simply how it looked. On a wide window the highlighted line
stretched right across the screen as a solid bar, which shouts far louder than
the thing it is meant to point at. It is now only as wide as the menu it belongs
to.

Neither could have been found by reading the code or by the automatic tests.
Both took somebody using it for two minutes.

## The names were never missing

Every device says what it is called when it joins a network and asks for an
address. The computer writes that down. The catch was that it writes it into a
file only an administrator can read, so a teacher who started the network the
ordinary way saw a list of numbers.

But the same computer is also answering the question "what is this address
called?" for everything on that network, because handing out addresses and
answering names are the same small program doing both jobs. So the tool asks
over the network instead of reading the file. No password, no terminal, no
administrator. The names were always there, being given out to anyone who asked;
we were reading the wrong one of the two places they live.

## How we know the screen works

You cannot check a screen by reading the code that draws it. A small harness was
written that opens a real terminal, presses the keys a person would press, and
reads back what actually appeared on it.

It found three faults in one evening that careful reading had missed. One of
them meant that **looking for the teacher's computer had never worked at all**:
the screen said "nothing found on this network" while a computer sat there
answering the whole time. Two halves of the program were each correct on their
own and wrong together, which is exactly the kind of thing reading cannot catch.

## The kid's whole journey, minute by minute

The old world put the knowledge in the user's head. This puts all of it in the
machines. The kid's total skill set: join a wifi, tap a notification, tap a big
button. Those are the three things that generation does hundreds of times a day.

**Minute 0, teacher.** Open hub, pick the folder (usually the USB drive that is
also their failsafe), tick what the class may see, start. The board says the
network name and the password. Nothing else.

**Minute 1, kid.** Joins the wifi and types the password. The single act of
typing in the whole flow, and the one ritual every kid already knows.

**Minute 1 and three seconds.** The phone silently asks its built-in question,
"is there internet here?". The laptop answers wrongly on purpose, which is the
mechanism behind every hotel sign-in page ever, and the phone itself announces
the lesson: "Sign in to this network. Tap here." On iPhones the page opens by
itself. On Windows laptops a toast pops AND the machine stops abandoning the
network after 16 seconds, a failure we measured before understanding it.

**Minute 2.** The page is the folder: one big row per file. READ or PLAY opens
it on the spot, and the video streams rather than filling the phone. GET IT
keeps a copy, which lands in Downloads, where every meme they have ever saved
already lives. Nothing to navigate, nothing to configure.

**Minute 3.** Homework goes the other way through the same page, using the
same picker they use to attach a photo. A retried send lands once. A fixed and
resubmitted version is kept alongside the first, labelled. And a kid who does
not understand question 3 can say so in a note that arrives on the teacher's
screen under the device's own name, without raising a hand.

## What the teacher sees while all that happens

One line per device, by name where the network knows it. Who is getting the
video and how fast, who is handing work in, who joined but has not opened the
page (the teacher walks to that desk; the kid never self-diagnoses), the last
few notes. Two warnings exist because real classrooms caused them: the folder
vanishing (a USB drive pulled mid-lesson) and a drive that refuses writes, which
switches hand-in off loudly instead of quietly eating homework.
