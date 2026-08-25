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

## 0.7.0

Not yet photographed. Three screens are new in this release and none of them
has a picture:

- **Who is on the network** (`c` from the roster), with a device paused and the
  row reading `PAUSED BY YOU`.
- **The paused page**, as it appears on the child's phone.
- **Join by camera** (`j`), the QR code on a real terminal, ideally with a
  phone actually scanning it.

They belong here as soon as somebody takes them.
