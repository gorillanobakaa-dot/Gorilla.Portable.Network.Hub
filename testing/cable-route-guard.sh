#!/usr/bin/env bash
# cable-route-guard.sh - stop the test cable from stealing the default route.
# Version: 1.0.0 - updated 26-09-07-12-24
#
# The hub's DHCP server offers a default route along with an address. NetworkManager
# gives a wired profile route-metric 100 by default, which beats wifi's 600, so the
# cable becomes the way out for all traffic the instant it is plugged in. It leads
# to one Windows laptop, so the machine goes dark.
#
# This sets two properties on the wired profile:
#   ipv4.never-default yes   - accept the address, ignore any offered default route
#   ipv4.route-metric  900   - and even if one appears, rank it below wifi (600)
#
# The address and the on-link route still work, so the cable tests are unaffected.
# Only the "be the whole internet" part is removed.
#
# Usage: ./cable-route-guard.sh {apply|revert|status}

set -uo pipefail

PROFILE="${CABLE_PROFILE:-Wired connection 1}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS="$HERE/results"
STATEFILE="$RESULTS/route-guard.prior-state"
mkdir -p "$RESULTS"

if ! nmcli -t -f connection.id connection show "$PROFILE" >/dev/null 2>&1; then
    echo "ERROR: NetworkManager profile '$PROFILE' not found." >&2
    echo "       Existing profiles:" >&2
    nmcli -t -f NAME,TYPE connection show | sed 's/^/       /' >&2
    exit 1
fi

show() {
    echo "profile:            $PROFILE"
    echo "ipv4.never-default: $(nmcli -g ipv4.never-default connection show "$PROFILE")"
    echo "ipv4.route-metric:  $(nmcli -g ipv4.route-metric  connection show "$PROFILE")"
    echo "autoconnect:        $(nmcli -g connection.autoconnect connection show "$PROFILE")"
    echo "current default route(s):"
    ip route show default | sed 's/^/  /' || echo "  (none)"
}

case "${1:-status}" in
    apply)
        # Record what it was, so revert restores rather than guesses.
        if [ ! -f "$STATEFILE" ]; then
            {
                echo "never_default=$(nmcli -g ipv4.never-default connection show "$PROFILE")"
                echo "route_metric=$(nmcli -g ipv4.route-metric connection show "$PROFILE")"
            } > "$STATEFILE"
            echo "prior state saved to $STATEFILE"
        else
            echo "prior state already recorded, not overwriting: $STATEFILE"
        fi
        sudo -n nmcli connection modify "$PROFILE" ipv4.never-default yes ipv4.route-metric 900 || {
            echo "ERROR: nmcli modify failed" >&2; exit 1; }
        echo "applied. verifying:"
        nd="$(nmcli -g ipv4.never-default connection show "$PROFILE")"
        rm_="$(nmcli -g ipv4.route-metric connection show "$PROFILE")"
        if [ "$nd" = "yes" ] && [ "$rm_" = "900" ]; then
            echo "VERIFIED: never-default=yes route-metric=900"
        else
            echo "FAILED VERIFY: never-default=$nd route-metric=$rm_" >&2; exit 1
        fi
        ;;
    probe)
        # For the captive-portal test only. NetworkManager will not run a
        # connectivity probe on a device that has no default route, so
        # never-default=yes (the "apply" mode) silently suppresses the very
        # thing the portal test is trying to observe.
        #
        # This lets the cable install a default route so NM probes it, but at
        # metric 900 instead of the usual 100, so wifi (600) still outranks it
        # whenever wifi is up. The route can exist without being able to win.
        if [ ! -f "$STATEFILE" ]; then
            {
                echo "never_default=$(nmcli -g ipv4.never-default connection show "$PROFILE")"
                echo "route_metric=$(nmcli -g ipv4.route-metric connection show "$PROFILE")"
            } > "$STATEFILE"
            echo "prior state saved to $STATEFILE"
        fi
        sudo -n nmcli connection modify "$PROFILE" ipv4.never-default no ipv4.route-metric 900 || {
            echo "ERROR: nmcli modify failed" >&2; exit 1; }
        nd="$(nmcli -g ipv4.never-default connection show "$PROFILE")"
        rm_="$(nmcli -g ipv4.route-metric connection show "$PROFILE")"
        if [ "$nd" = "no" ] && [ "$rm_" = "900" ]; then
            echo "VERIFIED probe mode: never-default=no route-metric=900 (wifi at 600 still wins)"
        else
            echo "FAILED VERIFY: never-default=$nd route-metric=$rm_" >&2; exit 1
        fi
        ;;
    revert)
        if [ ! -f "$STATEFILE" ]; then
            echo "no saved prior state at $STATEFILE, refusing to guess" >&2; exit 1
        fi
        # shellcheck disable=SC1090
        . "$STATEFILE"
        sudo -n nmcli connection modify "$PROFILE" \
            ipv4.never-default "${never_default:-no}" \
            ipv4.route-metric  "${route_metric:--1}" || { echo "ERROR: revert failed" >&2; exit 1; }
        echo "reverted to never-default=${never_default:-no} route-metric=${route_metric:--1}"
        mv "$STATEFILE" "$STATEFILE.reverted"
        ;;
    status) show ;;
    *) echo "usage: $0 {apply|probe|revert|status}" >&2; exit 2 ;;
esac
