# Screenshots

Every picture here is a real screen on real hardware: a Sony VAIO SVE laptop
and a Xiaomi 11 Lite 5G NE phone, on a hotspot the laptop made, with no router
and no internet. Nothing is staged, nothing is scaled, nothing is cropped for
tidiness. The numbers on screen are the numbers that were there.

Click any picture to open it at full size.

---

## The field test, 25 August 2026 (0.5.x and 0.6.0)

### A phone on the network, seen from the teacher's laptop

[![The hub roster on a Debian laptop, listing one connected device by its own name, Xiaomi-11-Lite-5G-NE, with the wifi name, password and address to type printed above it](gallery/hero-desktop-with-live-roster.png)](gallery/hero-desktop-with-live-roster.png)

This is the whole idea in one frame. The laptop is the network. The phone
joined it, and the roster names the phone rather than showing an address,
because the phone said what it was called when it asked for one. Nothing was
installed on the phone.

### A file moving, and a child's note arriving

[![The roster showing one device at 100 percent on a file transfer, and beneath it a note reading Yo teacher, leave the kids alone, attributed to Xiaomi-11-Lite-5G](gallery/roster-live-progress-and-first-note.png)](gallery/roster-live-progress-and-first-note.png)

A transfer in progress with its own progress bar, and underneath it the first
note a real phone sent. It is attributed to the device that sent it, which is
the point: a note that cannot be traced to a sender is a note a class will
abuse within an hour. The file manager behind it shows the `handed-in` folder
the tool created.

### The kid's page, on the kid's phone

[![The class page in a mobile browser: a green box reading Handed in, your teacher has it, a file with READ and GET IT buttons, a Hand in your work form and a Send a note to your teacher box](gallery/phone-handed-in-confirmation.jpeg)](gallery/phone-handed-in-confirmation.jpeg)

No app, no account, no JavaScript. Buttons big enough for a thumb, words a
ten-year-old can read, and a plain statement that the work arrived. The address
bar at the bottom shows the browser reached the laptop directly.

### The tick list, deciding what the class can see

[![The tick list screen showing files in a real folder with checkboxes, and a line reading how many of them will be visible to the class](gallery/tick-list-live-on-real-folder.png)](gallery/tick-list-live-on-real-folder.png)

Putting a file in the folder is not publishing it. Ticking it is. This screen
works mid-lesson, so a file can be published or withdrawn while the class
watches, and an unticked file does not exist as far as the network is
concerned: not listed, not fetchable, not even by guessing the name.

### Before the captive portal: what a phone used to say

[![A mobile browser showing a connection error page after joining the hotspot, with no sign of the class page](gallery/before-edge-no-internet-error.jpeg)](gallery/before-edge-no-internet-error.jpeg)

Kept deliberately. This is what joining the network looked like before the
sign-in page existed: the phone reports no internet, the browser shows nothing,
and a child has no idea what to do next. The fix that came out of this morning
is the reason the tool answers a phone's connectivity check on purpose.

### Friendly names instead of an address

[![The roster showing a device listed under a readable name rather than a numeric address](gallery/roster-friendly-name-classroom.png)](gallery/roster-friendly-name-classroom.png)

The same dnsmasq that hands out addresses knows what each device called itself,
and answers questions about it without needing a password. So a teacher sees a
name without ever touching a terminal.

### First light: the hotspot coming up

[![The screen at the moment the hotspot is created, showing the wifi name, the password and the address to type](gallery/first-light-hotspot-active.png)](gallery/first-light-hotspot-active.png)

### What the tool says about the machine it is on

[![Output of hub doctor listing the version, processor count, window size, wifi adapter, gateway and address](gallery/hub-doctor-output.png)](gallery/hub-doctor-output.png)

`hub doctor` exists so that "it does not work" can be answered without a
support call. It says what it found and what it did not.

---

## The session that produced 0.7.1, 25 August 2026

A phone and a laptop on the same hotspot, twenty minutes after 0.7.0 was built.
The names are the tester's own, and they are the reason the attribution scheme
exists at all.

### The roster, with the room on it

[![The hub roster showing the wifi name and password, an address with no port on the end and the word classroom as an alternative, two devices both looking at the page, one piece of work waiting, a note, and a key line offering files, notice, waiting, who is on and join code](gallery/roster-two-devices-and-work-waiting.png)](gallery/roster-two-devices-and-work-waiting.png)

Everything 0.7.x added is visible in one frame. The address has no port on the
end, which means the tool holds port 80 and a joining phone gets its own sign-in
screen. `classroom/` is offered as the thing to type instead, because a slash is
on the first keyboard layer and a colon is three deep. Two devices are on, one
piece of work is waiting, and the bottom line carries the two new keys.

### Who is on the network

[![The class screen listing two devices by the names they picked with a short tag after each, one row highlighted, above five lines explaining that a paused device still has the wifi and that a phone can come back under a different name](gallery/class-screen-two-devices-and-the-limits.png)](gallery/class-screen-two-devices-and-the-limits.png)

The screen where a device gets paused, and where the tool is honest about what
that does and does not do. Those five lines are not padding: a teacher who
believes a pause is a lock will be corrected by a child, in front of a class,
which is the worst possible way to find out.

### The bug this session found

[![The waiting work screen showing a piece of work from biggus.dickus with a tag, but the device column cut off to a bare address rather than naming the device](gallery/waiting-work-with-the-device-column-empty.png)](gallery/waiting-work-with-the-device-column-empty.png)

Kept deliberately, the way the "no internet" shot is kept. This is work handed
in by a **laptop**, and where the phone's entry would say
`Xiaomi-11-Lite-5G-NE`, this one falls back to a bare address. Phones tell the
network what they are called when they ask for one; laptops very often tell it
nothing.

That column is the whole point of the scheme, because it is the part a child did
not type. 0.7.1 fixes it by asking the other party that already knows: the
browser, which announces roughly what it is on every request. The same line now
reads `biggus.dickus #nzrm [a Windows laptop, 10.42.0.251]`.

## Still missing

- **The paused page**, as it appears on the child's phone.
- **Join by camera** (`j`), the QR code on a real terminal, ideally with a phone
  actually scanning it.

They belong here as soon as somebody takes them.
