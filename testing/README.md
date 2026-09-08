# Testing the hub against a real network

These are the checks that need two machines, a cable and a wifi network. The
unit tests in `src/hub` cover everything that can be decided without one.

They were written after the Linux binary in 0.9.0 and 0.9.1 turned out never to
have been run by anybody, and they have since found every user-facing bug in
0.9.2, 0.9.3 and 0.9.4.

## The scripts

| script | what it does |
|---|---|
| `transfer-tests.sh` | 34 checks: transfers both ways, the page, what must be refused, the captive redirect, hand-in |
| `captive-portal-test.sh` | switches wifi off to see whether the desktop offers a sign-in prompt |
| `network-watchdog.sh` | keeps the wifi route alive while the cable is under test |
| `cable-route-guard.sh` | stops the cable becoming the default route at all |

Nothing about any particular machine is written into them. The wired interface
is whichever one has a cable in it, the wifi one is whatever NetworkManager
reports connected, and the far end is whoever is announcing themselves. An
earlier version hardcoded one laptop's interface names and the other laptop's
address, which meant it only worked on the desk it was written at.

## Running them

```
./testing/transfer-tests.sh                 # the build tree's binary
HUB=/usr/bin/hub ./testing/transfer-tests.sh   # the installed package instead
PEER=10.0.0.5 ./testing/transfer-tests.sh      # a machine that is not announcing
```

Results land in `testing/results/`, which is gitignored. Read the summary
first; the log has everything.

## Why the watchdog exists, and why it is not optional

The hub's address server hands out a default route along with an address.
NetworkManager ranks a wired route above wifi, so plugging the test cable in
moves ALL traffic onto a link that reaches one other laptop and nothing else.

On 2026-09-07 that took the test machine off the network mid-run, and the
person driving the test was working over that wifi. They lost the machine at
the moment the thing being tested came up and could not see or fix it.

`transfer-tests.sh` starts the watchdog and refuses to run without it.
`cable-route-guard.sh apply` is the stronger fix: it tells NetworkManager the
cable may take an address but may never be the way out.

Use `cable-route-guard.sh probe` for the captive-portal test only. NetworkManager
will not run a connectivity check on a device with no default route, so
`apply` suppresses the very thing that test is trying to observe. `probe` lets
the route exist at a worse metric than wifi, so it is checked but never wins.

## Two names that are one letter apart

There were, and it cost time. `portal-test.sh` and `portal-tests.sh` did
completely different things. They are now `captive-portal-test.sh` and
`transfer-tests.sh`.

## What these cannot tell you

- Whether the Arch package installs. Nobody has run `pacman -U` on Arch.
- Whether the Windows build behaves. `packaging/check-both-builds.sh` compiles
  it and runs it under wine, which is not the same as running it on Windows.
- Whether a phone or a tablet behaves. Every check here is a program talking to
  a program. The screenshots in `docs/` are the record of real devices.
