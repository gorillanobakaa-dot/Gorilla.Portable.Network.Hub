# Testing the cable, with the network genuinely off

The address server only runs when the machine is on nothing but the cable. That
makes it awkward to test from the machine itself: switching the network off is
the thing being tested, and it also cuts off whoever is watching. Every fault in
this area was found by a person doing it by hand and reported afterwards, from a
machine that had by then been reconnected, which is exactly the evidence that
cannot settle the question.

So these run unattended, write down what they saw, and put the network back.

Nothing here is needed to use the tool. It is here because it is the only thing
that exercises the guard end to end, and because the last person to need it had
to write it from scratch under time pressure.

## The scripts

**`offline-guard-test.ps1`** is the important one. It starts the hub with the
wifi up, switches the wifi off the way a person does, and watches whether the
address server starts on its own. It then checks that `gorilla.local` resolves,
that the page serves under every name it should, and that a connectivity probe
receives the redirect that makes a desktop offer its sign-in prompt.

**`far-end-test.ps1`** opens a longer window, three minutes, and watches for the
computer on the other end of the cable to appear and answer.

**`recorder.py`** writes a line to the Desktop whenever the network state
changes: which addresses are held, which of them the platform still considers
connected, which of the three servers are listening, and whether the far end
replies. Leave it running while doing anything by hand.

## Running them

    powershell -ExecutionPolicy Bypass -File bench\cable\offline-guard-test.ps1

The SSID is at the top of each script and will need changing. Logs are written
to the Desktop.

## They must put the network back

Each script does its work inside a `try`, restores the wifi in a `finally`, and
registers a scheduled task first that reconnects it anyway if the script is
killed outright. A test that leaves a laptop with no way to reach anybody is
worse than a test that fails, and during this work the machine was left without
name resolution once already, by a fault these scripts were written to find.

## What they proved, and what they did not

With the wifi genuinely off, measured rather than assumed:

    noticed after about 5 seconds
    addresses (dhcp)  RUNNING
    names (dns)       RUNNING
    gorilla.local -> 169.254.87.61
    Host: gorilla.local  HTTP 200
    captive probe        HTTP 302
    169.254.87.1 replies: YES
    the page serves: HTTP 200

They do not prove a transfer. Nothing here can click Accept on the other laptop,
so the last step is still a person, and the 45 MB run has not been repeated
since the repairs these scripts were written to verify.

They also cannot reproduce the original fault on demand. `netsh wlan disconnect`
does not release the address: verified over 22 seconds, the adapter still held
it and a socket still selected it. That discovery is what explained the bug, and
it means the scripts release the address explicitly rather than trusting a
disconnect to do it.
