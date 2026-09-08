Gorilla Portable Network Hub 0.9.7  -  Linux (x86-64, static)

WHAT THIS IS
A laptop that becomes the network. It hands a folder to every device in the
room over wifi, or straight down a network cable to one other laptop. No
internet, no router, no accounts, nothing installed on the receiving devices.

TO START
Run ./hub in a terminal. A screen opens with four choices. That is the way in.

  1. Hand out files to the class over wifi
  2. Send files down a cable to one other computer
  3. Get files from another computer
  4. Check this computer

IF NOTHING CAN REACH THIS COMPUTER
This is the most common problem, and it does not look like a problem: every
other line reads as healthy while the firewall quietly drops every incoming
connection. Choose "Check this computer" and read the "reachable" line. If it
says NO, press enter on the fix and approve the Windows permission box.

FROM A COMMAND PROMPT INSTEAD
  hub serve C:\lessons        hand out a folder over the network that exists
  hub cable C:\lessons        hand it down a cable to one other laptop
  hub cable-get               take everything from the laptop on the far end
  hub doctor                  what this computer can and cannot do
  hub fix-firewall            let other devices reach this computer
  hub --help                  all of it

USING A CABLE
Plug an ordinary network cable between the two laptops. You do not need a
crossover cable on anything made since about 2005. WAIT ABOUT THIRTY SECONDS
before running anything: both computers have to give up waiting for a router
that is not there. Skipping that wait is the commonest reason it says it found
nothing.

If the other laptop runs Linux it will be given an address automatically. That
case used to fail completely.

WHAT IS NOT YET PROVEN IN THIS RELEASE
The cable transfer has not been run end to end with this exact build. The parts
are tested and the design is unchanged from a version that demonstrably worked,
but please test it before relying on it in front of a class.

Full guide:  docs/HOW-TO.md in the source repository
Licence:     AGPL-3.0

LINUX NOTES
This binary is statically linked. It has no dependencies at all and runs on any
distribution, including Alpine. Make it executable if your browser cleared the
flag:  chmod +x hub

Giving out addresses over a cable uses UDP port 67, which is privileged on
Linux. If you are SENDING from this machine to another Linux machine, run it
with sudo. Receiving, and sending to Windows or Mac, need nothing.

The firewall check cannot read ufw/firewalld/nftables from inside the program,
so "hub doctor" reports "cannot tell on this system" rather than guessing.
Run "hub fix-firewall" and it prints the exact line to paste for your firewall.
