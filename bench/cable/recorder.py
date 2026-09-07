"""
Records what happens on the cable while nobody is watching.

Claude cannot be online while the wifi is off, so this writes down what it
would otherwise have watched: which addresses this machine holds, who is on
the other end of the cable, and whether the far end is reachable. Every five
seconds, with a timestamp, to a file on the Desktop.

It touches no ports the hub wants (67, 53, 80, 8080), so it can run alongside.

Start it, run the transfer, stop it with Ctrl-C or by closing the window.
"""
import re
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

LOG = Path.home() / "Desktop" / "CABLE-TEST-log.txt"
CABLE_PEER = "169.254.87.1"   # what the hub offers the far end


def sh(*cmd):
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=12).stdout
    except Exception as e:
        return f"<{e}>"


def addresses():
    out = []
    adapter = None
    for line in sh("ipconfig").splitlines():
        if line and not line.startswith(" "):
            adapter = line.strip().rstrip(":")
        m = re.search(r"IPv4 Address[.\s]*:\s*([0-9.]+)", line)
        if m:
            short = (adapter or "?").replace("adapter ", "")
            out.append(f"{m.group(1)} on {short}")
    return out


def neighbours():
    out = []
    for line in sh("arp", "-a").splitlines():
        if "169.254." in line and "ff-ff-ff" not in line.lower():
            parts = line.split()
            if len(parts) >= 2:
                out.append(f"{parts[0]} is {parts[1]}")
    return out


def reachable(ip):
    o = sh("ping", "-n", "1", "-w", "1500", ip)
    return "TTL=" in o


def hub_running():
    o = sh("tasklist", "/FI", "IMAGENAME eq hub.exe")
    return "hub.exe" in o


def servers():
    """Which of the three are actually listening.

    Read out of the UDP endpoint table rather than by trying to bind them.
    Binding port 67 to find out whether something is on it would take packets
    away from the thing being watched, which is the one outcome a recorder must
    never cause.
    """
    out = sh(
        "powershell", "-NoProfile", "-Command",
        "Get-NetUDPEndpoint -ErrorAction SilentlyContinue | "
        "Where-Object { $_.LocalPort -in 67,53,5353 } | "
        "ForEach-Object { $_.LocalPort }",
    )
    ports = {int(x) for x in out.split() if x.strip().isdigit()}
    return {
        "addresses (dhcp 67)": 67 in ports,
        "names (dns 53)": 53 in ports,
        "gorilla.local (5353)": 5353 in ports,
    }


def main():
    with LOG.open("a", encoding="utf-8") as f:
        def say(s=""):
            print(s)
            f.write(s + "\n")
            f.flush()

        say("")
        say("=" * 64)
        say(f"recording started {datetime.now():%Y-%m-%d %H:%M:%S}")
        say("=" * 64)
        say("Leave this window open while you do the transfer.")
        say("Close it or press Ctrl-C when you are finished.")
        say("")

        last = None
        while True:
            now = datetime.now().strftime("%H:%M:%S")
            state = {
                "hub": hub_running(),
                "addrs": addresses(),
                "neigh": neighbours(),
                "peer": reachable(CABLE_PEER),
                "srv": servers(),
            }
            # Only write when something changed, so the log stays readable.
            if state != last:
                say(f"[{now}]")
                say(f"   hub running     {'yes' if state['hub'] else 'no'}")
                for a in state["addrs"]:
                    say(f"   this computer   {a}")
                if state["neigh"]:
                    for n in state["neigh"]:
                        say(f"   on the cable    {n}")
                else:
                    say("   on the cable    nobody seen yet")
                for label, up in state["srv"].items():
                    say(f"   {label:22} {'RUNNING' if up else 'not running'}")
                say(f"   far end replies {'YES' if state['peer'] else 'no'}")
                say("")
                last = state
            time.sleep(5)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        with LOG.open("a", encoding="utf-8") as f:
            f.write(f"recording stopped {datetime.now():%H:%M:%S}\n")
        print("\nstopped. The log is on your Desktop: CABLE-TEST-log.txt")
        sys.exit(0)
