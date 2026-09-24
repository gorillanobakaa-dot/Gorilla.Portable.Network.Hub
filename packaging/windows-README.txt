Gorilla Portable Network Hub 0.9.10  -  Windows

WHAT THIS IS
This laptop becomes the network. Phones, tablets and laptops in the room join
its wifi, get the files you choose, and can send work back to you. No
internet, no router, no accounts, nothing to install on the phones.

START HERE
1. Unzip this folder anywhere. A USB drive is fine.
2. Double-click hub.exe. A black window opens with four choices. Move with
   the up and down arrow keys, and press Enter to choose.
3. On a laptop you have not used it on before, choose
   "Fix problems with this computer (wifi, cable, firewall)" first.
   If Windows asks for permission, say Yes.
4. Choose "Hand out files to the class over wifi". Pick the folder with the
   lesson's files, tick the files the class may see, then Start.
5. The screen shows the network's name, its password and two codes. The
   class scans code 1 with a phone camera to join the wifi, and the class
   page opens by itself.
6. On that screen, press h. It explains every key, and what the class does.

IF WINDOWS SAYS "WINDOWS PROTECTED YOUR PC"
That is Windows being careful with a program downloaded from the internet.
Click "More info", then "Run anyway".

IF THE PHONES CANNOT SEE THE WIFI NETWORK
- Stand closer. Walls, metal and vehicles block wifi.
- The laptop's own wifi must be switched ON. It does not need to be joined
  to anything.
- On the start screen, "Wifi band" should say 2.4 GHz. Every phone can see
  2.4 GHz; some cannot see 5 GHz.

IF THE PHONES JOIN BUT CANNOT OPEN THE PAGE
Choose "Fix problems with this computer" and fix the firewall line: Windows
may be quietly blocking every phone. Say Yes to the permission box.

HOW FAST
Measured on a 2022 laptop (Intel Wi-Fi 6 card) to a Wi-Fi 6 phone two metres
away: about 23 MB every second, so a 1 GB video in under a minute. Older
laptops and older phones are slower. The full measurements are in
bench/RESULTS-WINDOWS.md in the source repository.

FROM A COMMAND PROMPT INSTEAD
  hub serve C:\lessons --name "Gorilla Hub" --password chalkdust
                              make the wifi network and hand out a folder
  hub cable C:\lessons        hand it down a cable to one other laptop
  hub cable-get               take everything from the laptop on the far end
  hub doctor                  what this computer can and cannot do
  hub services --fix          switch on the parts of Windows the hub needs
  hub fix-firewall            let other devices reach this computer
  hub --help                  all of it

USING A CABLE
Plug an ordinary network cable between the two laptops. You do not need a
crossover cable on anything made since about 2005. WAIT ABOUT THIRTY SECONDS
before running anything: both computers have to give up waiting for a router
that is not there. Skipping that wait is the commonest reason it says it found
nothing.

If the other laptop runs Linux it will be given an address automatically.

WHAT IS NOT YET PROVEN IN THIS RELEASE
The cable transfer has not been run end to end with this exact build. The parts
are tested and the design is unchanged from a version that demonstrably worked,
but please test it before relying on it in front of a class.

Step-by-step guide, with a picture of every screen:
  https://github.com/gorillanobakaa-dot/Gorilla.Portable.Network.Hub/blob/main/docs/HOW-TO.md
Licence: AGPL-3.0
