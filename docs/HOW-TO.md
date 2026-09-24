# How to use the hub, step by step

This guide is for the person standing at the front of the room. You do not
need to know what a terminal is, and you do not need to install anything on the
children's phones. It describes **version 0.10.0**.

Every picture is a real screen. Click any of them to see it full size. The
pictures of Windows use a made-up teacher's folder and a made-up password.

---

## What this is

Your laptop becomes the wifi network. The class joins it, their phones open a
page by themselves, and that page holds the files you are handing out. They
send their work back the same way.

Nothing here needs the internet. Nothing here needs a router. The children
install nothing and sign in to nothing.

**What you get:**

- Files go out to every device at once, and a device that loses the signal
  carries on from where it stopped.
- Work comes back to you, waits until you accept it, and then lands in one
  folder you can find.
- You can see who is on the network, by name, and take somebody off it.
- Each child can write to you and you answer them, from the laptop.
- A child can ask a trusted adult for help privately, without the class knowing
  (Part 6), in their own language.

---

## What you need

| You have | You get |
|---|---|
| A laptop running **Windows 10 or 11** | Everything. From 0.9.9 the hub makes the wifi network itself |
| A laptop running Debian, Ubuntu, Mint or similar | Everything |
| A laptop running Arch, CachyOS or Manjaro | Everything |

The children's devices only need a web browser. Any browser, going back to
about 2009. That is the whole requirement on their side.

The laptop's **wifi must be switched on** (it does not need to be connected to
anything). The wifi network is broadcast by the laptop's wifi card, so with
wifi switched off there is nothing to broadcast from.

---

## Part 1: Get it running

### On Windows

1. Go to the
   [releases page](https://github.com/gorillanobakaa-dot/Gorilla.Portable.Network.Hub/releases)
   and download `hub-0.10.0-windows-x86_64.zip`.
2. Right-click the zip and choose **Extract All**. Put it anywhere: a USB drive
   is fine.
3. Open the folder it made and double-click **hub.exe**.
4. Windows may say it does not recognise the program. That warning appears for
   any program without a paid signing certificate. Choose **More info**, then
   **Run anyway**, or do not run it. Both are reasonable.

The first screen appears. The top line says `Gorilla Portable Network Hub 0.10.0`
and the date the program was built. (In a terminal, `hub --version` prints
`hub 0.10.0` and the same date.)

[![The first screen: four choices; under them, what the highlighted one does ("Makes a wifi network from this laptop. Phones and laptops join it, get the files you choose, and can send work back. No internet needed."), and "This computer is ready"](screenshots/gallery/windows-0.10.0-first-screen.png)](screenshots/gallery/windows-0.10.0-first-screen.png)

Use the **up and down arrow keys** to choose, and **Enter** to open. **q**
quits. Under the four choices, two lines say **what the highlighted one
does**; move up and down to read each.

### If it says THIS COMPUTER IS NOT READY YET

Many laptops, especially second-hand ones, have had parts of Windows switched
off to "speed them up". Nothing looks wrong until the wifi network fails in
front of the class. The hub checks as soon as it opens, and tells you.
(The three pictures in this section are from 0.9.9. In 0.9.10 the first screen
also has the two lines saying what the highlighted choice does.)

[![The first screen saying THIS COMPUTER IS NOT READY YET: Windows Mobile Hotspot Service and Internet Connection Sharing are switched off, choose Fix problems with this computer, Windows will ask permission, say Yes](screenshots/gallery/windows-0.9.9-not-ready.png)](screenshots/gallery/windows-0.9.9-not-ready.png)

**What to do:** choose **Fix problems with this computer**, then **Turn on the
parts of Windows the hub needs**, and press Enter. If you choose *Hand out
files* first, the hub stops you and offers the same fix:

[![This computer is not ready yet: the two parts of Windows that are switched off, why they matter, and Press enter to switch them back on](screenshots/gallery/windows-0.9.9-fix-offer.png)](screenshots/gallery/windows-0.9.9-fix-offer.png)

Windows asks for permission. **Say Yes.** A few seconds later:

[![All switched back on. The hub can make a wifi network and use a cable now](screenshots/gallery/windows-0.9.9-fix-done.png)](screenshots/gallery/windows-0.9.9-fix-done.png)

That is all. Nothing is installed or removed: the parts are set back to what
Windows itself ships with. The hub writes down how they were first, so they can
be put back exactly (see *For the person who looks after the laptop* near the
end).

If Windows asks for an **administrator password you do not have**, the laptop
belongs to somebody else (a school, an employer) and they have to do it. Until
then, use a cable instead: see Part 9. It needs none of this.

### The Fix problems screen

You can open it any time. It says, in one line each, whether the parts of
Windows the hub needs are on, whether the firewall has been opened for the hub,
and **what this laptop's wifi card can do**, read from the card itself.

[![Fix problems with this computer: Windows parts all switched on; the firewall; the wifi card can make a network on 2.4 or 5 GHz, not 6 GHz, one network at a time; the two buttons](screenshots/gallery/windows-0.10.0-fix-problems.png)](screenshots/gallery/windows-0.10.0-fix-problems.png)

Do **both** buttons once on every new laptop, even if it seems to work fine:

- **Turn on the parts of Windows the hub needs** (explained above).
- **Let other devices reach this computer (firewall).** If Windows asks when
  you first hand out files whether to let the hub through its firewall, say
  **Allow** as well.

### On Linux

The newest Linux packages are **0.9.5**; the Linux build of 0.10.0 is being
tested. 0.9.5 works as described here, except where this guide says *Windows*.

On Debian, Ubuntu or Mint, download `gorilla-portable-network-hub_0.9.5_amd64.deb`
from the releases page, open a terminal in that folder, and type:

```bash
sudo dpkg -i gorilla-portable-network-hub_0.9.5_amd64.deb
```

On Arch, CachyOS or Manjaro:

```bash
sudo pacman -U gorilla-portable-network-hub-0.9.5-1-x86_64.pkg.tar.zst
```

Then find **Portable Network Hub** in your applications menu, or type `hub`.

---

## Part 2: Hand out files over wifi

### Step 1: put the files in one folder

Anything in it can be offered to the class. Folders inside it are included,
exactly as they are: subjects, weeks, a resource pack as you downloaded it.
You choose what the class may actually see in step 4.

### Step 2: choose *Hand out files to the class over wifi*

The start screen appears.

[![The start screen: folder to hand out, wifi network Gorilla Hub, password, Wifi band 2.4 GHz (every phone sees it), connections, and where received files go, then Start handing out; under them, what the band line means](screenshots/gallery/windows-0.10.0-start-screen-band.png)](screenshots/gallery/windows-0.10.0-start-screen-band.png)

Move with the **up and down arrows**, press **Enter** to change a line.
**While you stand on a line, the screen explains it underneath**, so the
table below is only for reading ahead.

| Line | What it means | What to do |
|---|---|---|
| **Folder to hand out** | The folder from step 1 | Enter opens a folder browser: move to your folder, open it with Enter, then choose the line at the top |
| **Wifi network to make** | The name the class looks for | *Gorilla Hub* is ready. Change it if you like |
| **Password for it** | The wifi password | Filled in for you. At least 8 letters, a rule of wifi itself. The same name keeps the same password next time, so phones that joined before join again without asking |
| **Wifi band** (Windows) | Which radio band the network uses | Leave it on **2.4 GHz**: every phone sees it and it goes through walls better. Enter switches to 5 GHz, which is faster but some phones cannot see at all |
| **Wifi channel** (Linux) | Which lane the network uses | Leave it on automatic. If the room is slow, try 11 or 13 |
| **Private help goes to** | Who reads a child's private HELP messages (Part 6) | *nobody* (off), *the teacher, on this laptop*, or *a trusted adult, on their own phone*. Enter changes it |
| **Private help password** | Locks the record of private help, and is the trusted adult's sign-in | At least 8 characters. Stars on the screen while you type. Without it, private help stays off |
| **Second adult's password** | Optional: a second adult who may also sign in and read the record | Their own password, at least 8 characters, or leave it empty |
| **Connections to serve at once** | How many requests at the same time | Leave it alone |
| **Received files go to** | The folder children's work goes into | *Documents\Gorilla Hub received*. Enter lets you choose another folder |

### Step 3: choose *Start handing out*

Standing on it, the screen says what comes next: you tick the files, then the
network switches on and two codes appear.

[![Start handing out, highlighted, with: Next you tick which files the class may see. Then the wifi network switches on and two codes appear for the class to scan](screenshots/gallery/windows-0.10.0-start-screen-start.png)](screenshots/gallery/windows-0.10.0-start-screen-start.png)

### Step 4: choose what the class may see

The list shows **one folder at a time**: the folders inside it with how many
files each holds, then its own files. The top of the screen says what the marks
mean.

[![What gets handed out: "Tick what the class may see", what [x], [ ] and [~] mean, then Past papers (36 files), Photos, Reading, Videos and five files, all unticked, and CONTINUE at the top](screenshots/gallery/windows-0.10.0-folder-view.png)](screenshots/gallery/windows-0.10.0-folder-view.png)

- **Space** ticks or unticks what the cursor is on. `[x]` means handed out,
  `[ ]` means not, `[~]` means some of the files inside a folder.
- **Enter** on a folder goes inside it, so you can tick single files.
  **Backspace** (or the `..` line) comes back out.
- **a** ticks everything in the folder you are looking at, **n** unticks it.
- **Page Down / Page Up**, and the arrow keys, move through a long list. If it
  does not all fit, the bottom of the list says how many there are and which
  key shows the rest. A wide window shows several columns.

**A big folder asks first.** Ticking a folder with many files, which could be
thousands, stops and says exactly how many:

[![Space again ticks ALL 36 files inside Past papers, every folder inside included. Some may be files you did not know were there. Space again: tick them all. Enter: go inside and choose instead](screenshots/gallery/windows-0.10.0-big-folder-question.png)](screenshots/gallery/windows-0.10.0-big-folder-question.png)

Press **space again** to tick them all, or **Enter** to go inside and choose.
Any other key cancels.

**Putting a file in the folder does not publish it. Ticking it does.** An
unticked file cannot be reached, even by a child who guesses its name.

When you are happy, go up to **CONTINUE: HAND OUT THE … TICKED** at the top and
press Enter.

### Step 5: the network comes up

On Windows the hub switches the wifi network on itself, a few seconds, no
Settings, no administrator. The screen shows everything the class needs:

[![Handing out: wifi network Gorilla Hub, the password, broadcasting on 2.4 GHz channel 11, the address, gorilla.local, and two codes: 1. Scan to join the wifi, 2. Then scan to open the page; at the bottom, h HELP and the other keys](screenshots/gallery/windows-0.10.0-handing-out-codes.png)](screenshots/gallery/windows-0.10.0-handing-out-codes.png)

- **Wifi network** and **Password**: read them out or write them on the board.
- **Broadcasting on**: the band and channel the laptop is really using, read
  from the wifi card.
- **Code 1** joins a phone to the network, password and all, when its camera is
  pointed at it. **Code 2** opens the class page.
- **Received files go to**: where the children's work will be. **o** opens it.

If the network goes off for any reason (somebody switches the laptop's wifi
off; Windows switches it off by itself), the hub notices within seconds,
switches it back on, and says so on this screen while it does. Windows
normally switches its network off when no phone has been connected for five
minutes; while the hub runs it tells Windows not to, and puts that setting
back when you stop.

### Press h for help

On this screen, **h** opens one page that explains every key, what the class
does step by step, and what to do if something goes wrong. Enter brings you
back. In a small window the page says *more lines below*: the down arrow
shows the rest.

[![Help: handing out files. What the class does in four steps, the keys on this screen, and if something goes wrong](screenshots/gallery/windows-0.10.0-help.png)](screenshots/gallery/windows-0.10.0-help.png)

---

## Part 3: What the children do

1. **Switch wifi on** on the phone (mobile data can stay off).
2. **Scan code 1** with the phone's camera and tap what appears, or choose the
   network by name in the phone's wifi list and type the password.
3. The phone shows **Sign in to network** by itself and opens the class page.
   If it does not, **scan code 2**. On a laptop, type `gorilla.local` in the
   browser.

The first thing the page asks is their **name**, once. Everything they send is
filed under it: thirty identical phones are otherwise impossible to tell apart.

<a href="screenshots/gallery/phone-0.10.0-name.png"><img src="screenshots/gallery/phone-0.10.0-name.png" width="300" alt="Welcome to the class page. First, type your name. Use the name your teacher calls you. THAT'S ME. You only do this once; the page works with no internet."></a>

Then the page is in **numbered parts**, and each says in one line what its
buttons do. The children can read their way through it without you.

**In their own language.** The top of the name page, and the foot of the class
page, offer **English, Français, Kiswahili, Português, دری (Dari) and پښتو
(Pashto)**. The phone remembers the choice. Dari and Pashto read right to left.
*These translations are first drafts and have not yet been checked by native
speakers; please tell us what reads wrong.*

<a href="screenshots/gallery/phone-0.10.0-swahili-name.png"><img src="screenshots/gallery/phone-0.10.0-swahili-name.png" width="260" alt="The name page in Swahili, with the six languages at the top"></a>
<a href="screenshots/gallery/phone-0.10.0-dari-page.png"><img src="screenshots/gallery/phone-0.10.0-dari-page.png" width="260" alt="The class page in Dari, right to left"></a>

<a href="screenshots/gallery/phone-0.10.0-class-page.png"><img src="screenshots/gallery/phone-0.10.0-class-page.png" width="300" alt="The class page: 1. Files from your teacher, with READ, GET IT and GET EVERYTHING; 2. Send your work to your teacher, in three steps; 3. Send a note to your teacher"></a>

**1. Files from your teacher.** **READ** or **PLAY** looks at a file now;
**GET IT** keeps a copy on the phone; the purple **GET EVERYTHING** takes the
whole lot at once. A child who cannot find what they got taps *Where do the
files I GET go?*:

<a href="screenshots/gallery/phone-0.10.0-where-downloads.png"><img src="screenshots/gallery/phone-0.10.0-where-downloads.png" width="300" alt="Where do the files I GET go? Android: the Files app (on Samsung, My Files), then Downloads. iPhone: the Files app, then Downloads."></a>

**2. Send your work to your teacher.** Three numbered steps on the page:
**Choose files** opens the phone's own file picker (several at once, and
choose again to add more); every chosen file is listed with a red **REMOVE**
button, so a wrong picture can be taken back; then **SEND IT TO YOUR
TEACHER**. **START AGAIN** clears the lot.

<a href="screenshots/gallery/phone-0.10.0-choose-and-remove.png"><img src="screenshots/gallery/phone-0.10.0-choose-and-remove.png" width="300" alt="Send your work to your teacher: three steps, two chosen files each with a red REMOVE button, 2 files will be sent, SEND IT TO YOUR TEACHER and START AGAIN"></a>

While it sends, the button counts up and says to keep the page open, so
nobody taps it again or closes the page halfway. When it has arrived, a green
box says so:

<a href="screenshots/gallery/phone-0.10.0-sending.png"><img src="screenshots/gallery/phone-0.10.0-sending.png" width="300" alt="The button reading SENDING... 30% - KEEP THIS PAGE OPEN"></a>
<a href="screenshots/gallery/phone-0.10.0-arrived.png"><img src="screenshots/gallery/phone-0.10.0-arrived.png" width="300" alt="Green box: Your work arrived. It is on your teacher's laptop now, waiting for your teacher to accept it. You can send more, or close this page."></a>

If something goes wrong the box is red and says what to do: nothing chosen,
a file over 1 GB, or your laptop unable to keep it (and that nothing was lost
from the phone).

**3. Talk to your teacher.** A real conversation: the child writes, you
answer from the laptop (Part 5), and your answer appears on their page by
itself within a few seconds. Their own messages show *seen by your teacher*
once you have opened them.

<a href="screenshots/gallery/phone-0.10.0-talk-to-teacher.png"><img src="screenshots/gallery/phone-0.10.0-talk-to-teacher.png" width="300" alt="3. Talk to your teacher: the child's message in the conversation, and a box to write another"></a>

**4. HELP** appears only when private help is switched on (Part 6). It is on
every child's page, so having it says nothing about anybody.

If a phone opened the page in its small "sign in to wifi" window, tapping
*Choose files* does nothing there. The page has a fold for exactly that,
*Tapping Choose files does nothing?*, with a button that opens the page in the
phone's normal browser. The phone stays on the class wifi.

---

## Part 4: Collect the work

Nothing a child sends lands on your computer straight away. It waits, and the
handing-out screen tells you:

[![The handing-out screen showing PIECES OF WORK WAITING FOR YOU. Press w to look](screenshots/gallery/windows-0.10.0-work-arrived.png)](screenshots/gallery/windows-0.10.0-work-arrived.png)

Press **w**.

[![Work waiting for you: five pieces from Amina and Joseph; "Nothing is kept until you accept it"; o looks at it first, a accepts it, e accepts ALL, p all from them, r refuses it: moved aside, never deleted](screenshots/gallery/windows-0.10.0-waiting-accept-all.png)](screenshots/gallery/windows-0.10.0-waiting-accept-all.png)

| Key | Does |
|---|---|
| **e** | **Accept everything waiting**, all at once. Forty children with six pieces each is one key, not 240 |
| **p** | Accept everything from the **person** under the cursor |
| **a** | Accept just the one under the cursor |
| **r** | Refuse it. Refused work is **kept** in a *refused* folder, never deleted: it may be evidence |
| **o** | Look at it first, before deciding |

[![Accepted 5 pieces of work. They are in ... Gorilla Hub received. Press o on the sending screen to open that folder](screenshots/gallery/windows-0.10.0-accepted-all.png)](screenshots/gallery/windows-0.10.0-accepted-all.png)

Accepted work is in **Documents\Gorilla Hub received** (or the folder you chose
on the start screen). On the handing-out screen, **o** opens that folder.
Nothing sent to you is ever handed back out to the class.

Every piece is filed with the name the child typed, what the device is, and a
short tag from the hardware, for example `Amina #wa3x [an Android phone]`. The
name can be typed by anyone; the tag cannot. If two devices claim the same
name, the screen tells you.

---

## Part 5: Messages, both ways

When a child writes, the handing-out screen says **NEW MESSAGE FROM THE
CLASS. Press m**. It never shows the words there, so the class cannot read
them over your shoulder.

Press **m**. Every child who wrote is listed with how many messages are new,
and so is every child on the network who has not written, so **you can write
first**:

[![Messages: Amina, 1 new, 15:32, Lesson 2 will not open on my phone](screenshots/gallery/windows-0.10.0-messages.png)](screenshots/gallery/windows-0.10.0-messages.png)

Choose one with the arrows and press **Enter**. Type your answer and press
**Enter** to send it. **Esc** goes back.

[![Talking with Amina: her message and your answer, and the line to type the next one](screenshots/gallery/windows-0.10.0-conversation.png)](screenshots/gallery/windows-0.10.0-conversation.png)

The child sees your answer on their page within a few seconds, without doing
anything. Your messages show *(seen)* once their page has shown them.

Every ordinary message, both ways, is written to **messages.txt** in the
received folder, with the time and who, like the old notes. A message to the
whole class is still **n** (the notice at the top of every page).

---

## Part 6: Private help

Some children cannot say what is wrong in front of the class, their family or
visiting officials. Private help gives them a quiet way to reach **one
trusted adult**, and the adult a quiet way to reach them. It is **off** until
you switch it on.

> **Before you use it with children**, agree with your organisation who the
> trusted adult is and what they will do when a child asks for help. This
> tool carries the words; it does not replace child-protection training,
> procedures, or the people who follow them.

### Switching it on (the start screen)

[![The start screen on "Private help goes to: a trusted adult, on their own phone", with its explanation](screenshots/gallery/windows-0.10.0-start-screen-private.png)](screenshots/gallery/windows-0.10.0-start-screen-private.png)

1. **Private help goes to:** press Enter to choose *the teacher, on this
   laptop* or *a trusted adult, on their own phone* (a nurse, a protection
   officer: somebody who is not the teacher, for when the teacher may be part
   of the problem).
2. **Private help password:** at least 8 characters. It does two jobs: it is
   the trusted adult's sign-in, and it **locks the record** of what was said.
   Only somebody with this password can ever read it. Stars are shown while you
   type, so the room cannot read it off the screen.
3. **Second adult's password** (optional): a second named adult who may also
   sign in and read the record, with their own password. It protects children
   from a misbehaving adult, and honest adults from false accusations.

When the lesson starts, the handing-out screen says **Private help: ON** and
where the trusted adult signs in:

[![Handing out, with "Private help: ON. The trusted adult signs in at http://192.168.137.1/adult"](screenshots/gallery/windows-0.10.0-handing-out-codes.png)](screenshots/gallery/windows-0.10.0-handing-out-codes.png)

### What the child sees

The **HELP** part of the class page opens a separate page. Its title and
address say nothing about help, because a phone's history lists both.

<a href="screenshots/gallery/phone-0.10.0-help-page.png"><img src="screenshots/gallery/phone-0.10.0-help-page.png" width="300" alt="Talk privately: HIDE THIS at the top, I NEED TO TALK TO SOMEONE, and a box to write privately"></a>
<a href="screenshots/gallery/phone-0.10.0-french-help.png"><img src="screenshots/gallery/phone-0.10.0-french-help.png" width="300" alt="The same page in French"></a>

- **I NEED TO TALK TO SOMEONE**: one tap, nothing to write. For a child who
  cannot write, or not in these languages.
- **Write privately**, if they want to.
- **HIDE THIS**, always at the top: one tap and the phone shows the ordinary
  class page, with no step to go back to. Nothing is kept on the phone.
- **Nothing rings, buzzes or pops up.** When the adult answers or asks, a dot
  appears on the child's HELP button, and that is all.

### What the trusted adult does

On their own phone, joined to the class wifi, they open the address on your
screen (for example `http://192.168.137.1/adult`) and type the password.
Five wrong passwords from one phone make it wait five minutes.

<a href="screenshots/gallery/phone-0.10.0-adult-list.png"><img src="screenshots/gallery/phone-0.10.0-adult-list.png" width="300" alt="Private help: Amina, 2 new, ASKED TO TALK. Find a safe, private moment; do not call them out in front of others."></a>
<a href="screenshots/gallery/phone-0.10.0-adult-conversation.png"><img src="screenshots/gallery/phone-0.10.0-adult-conversation.png" width="300" alt="The conversation, a box to answer, and ASK QUIETLY TO TALK"></a>

- The list shows only children who asked for help, and **ASKED TO TALK** when
  a child used the one tap.
- Opening a child shows the conversation; **SEND** answers.
- **ASK QUIETLY TO TALK** puts a question on the child's help page: *A trusted
  adult would like to talk to you. Is that all right?* with **YES**, **LATER**
  and **NO**. The child answers when it is safe; the answer appears for the
  adult.

<a href="screenshots/gallery/phone-0.10.0-child-sees-answer.png"><img src="screenshots/gallery/phone-0.10.0-child-sees-answer.png" width="300" alt="On the child's help page: the adult's answer, and the question with YES, LATER and NO"></a>

If *the teacher* receives private help, the same conversations appear under
**m** on the laptop marked **PRIVATE**, with the words hidden until you open
one; **Tab** in a private conversation asks quietly to talk. Open them only
when nobody else can see the screen. If *a trusted adult* receives it, the
laptop shows nothing about private help at all, not even a count.

### The locked record

Every private word, and which adult wrote each answer, is kept in a file
called `private-record-<date>-<time>.gpr` in the received folder, **locked**.
Without one of the passwords it is unreadable. To read it, in a terminal:

```
hub private-record
```

It asks for the password and prints the conversations. The adult who took
part cannot delete single lines through the program.

**Said plainly:** the record protects against a laptop that is borrowed or
lost. It does not stop somebody deleting the file, and the words are not
protected while they cross the wifi: anybody with the wifi password and the
right tools, in radio range, could read them. Tell the trusted adult this.

---

## Part 7: During the lesson

All from the handing-out screen:

| Key | Does |
|---|---|
| **h** | Help: every key explained, and what the class does |
| **m** | Messages: read and answer each child (Part 5) |
| **f** | Change which files are handed out, live: tick to publish now, untick to withdraw |
| **n** | A notice at the top of every child's page (the blackboard, on thirty screens) |
| **w** | Work waiting for you |
| **o** | Open the received folder |
| **c** | Who is on the network, by name |
| **j** | Show the join code on its own, larger |
| **q** | Stop handing out |

### Taking somebody off the network

Press **c**, move to the name, and:

- **Space** pauses that device: it keeps the wifi but loses the lesson. Space
  again lets it back; its page comes back by itself.
- **p** changes the wifi password. Everybody is knocked off at once, and only
  the people you give the new password to come back. Blunt, and the screen warns
  you first: use it when a pause is not holding.

A pause recognises a device, and a phone can be told to look like a different
one. The password is the control that cannot be walked around.

---

## Part 8: When the lesson ends

Press **q** to stop, **Enter** to close the message, then **q** again to quit.

The wifi network is switched off, and on Linux your own wifi comes back as it
was. If the window is closed with its **X**, or the program is ended any other
way, the network is still switched off within a few seconds by itself.

---

## Part 9: Send a folder down a cable

Wifi is not always the answer. Some laptops have a wifi card that died years
ago, and thirty gigabytes of video over old wifi takes an afternoon. An
ordinary network cable between two laptops has none of those problems. It is
the rectangular-plug cable that normally goes into a wall socket, it costs very
little, and on most laptops made since about 2005 you do not need a special
"crossover" one. No router, no wifi and no internet are involved.

1. Plug the cable into both laptops. A small light next to each socket comes on.
2. **Wait about thirty seconds.** Both computers have to give up waiting for a
   router that is not there and settle on an address of their own. Skipping this
   is the commonest reason the next step finds nothing.
3. On the computer with the files, choose **Send files down a cable to one
   other computer**.
4. On the other computer, open any web browser and type `gorilla.local`. Some
   computers offer to open the page by themselves. If the name does not work,
   the address the screen shows always will.

Or run the hub on the second computer too and choose **Get files from another
computer**: it finds the sender by itself.

**It will not hand out addresses on a network that already has a router.** Two
things handing out addresses on one school network can take every computer in
the building offline. The hub checks first, keeps checking, and stops if a
router appears.

| What the cable and sockets support | Realistic speed | 10 GB takes about |
| :--- | :--- | :--- |
| 100 Mbps (older laptops) | 11 MB/s | 15 minutes |
| 1 Gbps (most laptops since 2010) | 110 MB/s | 90 seconds |

---

## Part 10: Get a whole folder onto another computer

On the receiving laptop, run the hub and choose **Get files from another
computer**. It finds the teacher's laptop by itself.

- **Enter** gets the file you have highlighted.
- **a** gets every file on the list, rebuilding the folders as it goes.

Files that did not arrive are listed by name at the bottom; run it again and it
picks up only those.

---

## If something does not work

| What you see | Why | What to do |
|---|---|---|
| **THIS COMPUTER IS NOT READY YET** | Parts of Windows the hub needs are switched off | Part 1: *Fix problems with this computer*, say Yes |
| Phones cannot see the network at all | The band is 5 GHz and those phones cannot see it | Start screen: set **Wifi band** to 2.4 GHz |
| The network was seen, then vanished | The laptop's wifi was switched off, or Windows switched the network off | Leave the laptop's wifi on. The hub brings the network back by itself and says so |
| A phone cannot see *any* other wifi | That phone is sharing its own hotspot. Most phones cannot do both | Switch the phone's own hotspot off |
| The code does not scan | The camera cannot fit the code in, or the phone's camera app does not read codes | Step back, or use Google Lens; or join by name and password |
| The phone joined, but no page opened | Some phones do not show the sign-in page | Scan code 2, or type `gorilla.local` on a laptop |
| Typing `gorilla.local` on a phone does a web search | Phone browsers treat it as a search | Use code 2 instead |
| The address on screen ends in `:8080` | Another program on the laptop holds the page phones look for | Close that program, or tell the class to type the address with `:8080` |
| Hand-in is off | The received folder cannot be written to | Check the USB drive has room and is not write-protected, or choose another folder |
| The window is too small for the codes | The codes need room | Make the window bigger (maximise it), or press **j** |
| *Private help is OFF: its password needs at least 8 characters* | The private help password is empty or short | Start screen: type one of 8 or more characters, then start again |
| The trusted adult cannot sign in | Wrong password, or five wrong tries | Check it with whoever set up the lesson; after five wrong tries, wait five minutes |
| A translation reads wrong | The translations are first drafts | Tell us the sentence and the better words |
| A phone's list shows only a few files | The list is a box on the page | Slide the list itself up and down; the page says so |
| A phone's SEND stays on the same percentage | A weak signal | Move closer to the laptop and keep the page open; if it fails, the page says so and they tap SEND again |
| On Linux: *will not let a normal account create a network* | Making a network needs administrator rights there | Start it with `sudo hub` |

If none of these match, run `hub doctor` in a terminal (or look at *Fix problems
with this computer*) and send us what it says.

### For the person who looks after the laptop

- `hub services` lists the parts of Windows the hub needs that are switched off.
  `hub services --fix` switches them on (one permission prompt).
  `hub services --put-back` puts them back exactly as they were before the hub
  changed them.
- The hub writes what the wifi network did (switched on, went off, came back,
  and what Windows said each time) to `%LOCALAPPDATA%\PortableNetworkHub\network.log`,
  and any crash to `crash.log` in the same folder.
- By hand, the two parts are *Windows Mobile Hotspot Service* (`icssvc`) and
  *Internet Connection Sharing* (`SharedAccess`), set to Manual.

---

## How fast is it?

On a 2022 laptop with an Intel Wi-Fi 6 card, a Wi-Fi 6 phone two metres away
downloaded at **about 23 MB every second**: a 1 GB video in under 45 seconds.
Older laptops and older phones are slower; a 2012 laptop with a one-antenna
card managed about 6.5. Every measurement, and how to repeat it, is in
[bench/RESULTS-WINDOWS.md](../bench/RESULTS-WINDOWS.md) (Windows) and
[bench/RESULTS.md](../bench/RESULTS.md) (the 2012 laptop, Linux).

---

## What it does not do

- **It is not the internet.** No web, no email, no search. It moves files
  between machines in one room.
- **A pause is not a lock.** The password is the control that cannot be walked
  around.
- **The wifi password is the only thing protecting the network.** Anyone in
  radio range who has it can join. Change it between classes if that matters.
- **It does not encrypt what is on your disk.** Handed-in work is a normal file
  in a normal folder.
- **Private help is not a helpline.** It carries a child's words to one
  adult in the room. What happens next depends on that adult and your
  organisation's procedures.
- **Messages cross the wifi unprotected.** The locked record protects the
  laptop, not the air.
- **The translations are drafts** until native speakers have checked them.
- **On Windows it cannot pick the exact channel.** Windows chooses the channel
  within the band; the screen shows which one.

---

## Every key, on one page

| Screen | Keys |
|---|---|
| Everywhere | **up/down arrows** move, **Enter** chooses, **Esc** goes back |
| First screen | **q** quits |
| Choosing files | **Space** tick/untick, **Enter** open a folder, **Backspace** out of it, **a** all here, **n** none here, **Page Up/Down**, **Home/End** |
| Handing out | **h** help, **m** messages, **f** files, **n** notice, **w** work waiting, **o** received folder, **c** who is on, **j** join code, **q** stop |
| Messages | **up/down** choose, **Enter** open, **p** private conversation (when you receive private help), **Esc** back |
| A conversation | type, **Enter** sends, **up/down** scroll, **Tab** asks quietly to talk (private), **Esc** back |
| Work waiting | **o** look first, **a** accept one, **e** accept all, **p** all from this person, **r** refuse |
| Who is on | **Space** pause/unpause, **p** new wifi password |

---

## Tell us how it went

Open an issue at
<https://github.com/gorillanobakaa-dot/Gorilla.Portable.Network.Hub/issues>.
You do not need to be technical. The three things that help most:

1. **What kind of laptop, and what it runs.** `hub doctor` covers most of this.
2. **What kind of phones the children have.** Especially old ones, and
   especially ones that failed.
3. **What you expected to happen, and what happened instead.** In your own
   words.

"Nine phones connected and three did not, all three were the same cheap
Android" is a better report than most.
