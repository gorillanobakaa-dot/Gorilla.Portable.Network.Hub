#!/usr/bin/env bash
# network-watchdog.sh - keep the wifi route alive while the cable is under test.
# Version: 1.0.0 - updated 26-09-08-10-50
#
# WHY THIS EXISTS. The hub's address server hands out a default route along
# with an address. NetworkManager ranks a wired route above wifi, so plugging
# the test cable in silently moves ALL traffic onto a link that reaches one
# other laptop and nothing else.
#
# On 2026-09-07 that took the test machine off the network mid-run, and the
# person driving the test was working over that wifi: they lost the machine at
# the exact moment the thing they were testing came up, and could not see or
# fix what had happened. Somebody had to unplug the cable by hand.
#
# So this runs detached, checks the property that actually matters, and puts it
# back when it moves. It has a hard lifetime and can never outlive the test.
#
#   ./network-watchdog.sh start [--lifetime S] [--interval S]
#   ./network-watchdog.sh stop | status | is-running
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS="${HUB_TEST_RESULTS:-$HERE/results}"
PIDFILE="$RESULTS/watchdog.pid"
LOGFILE="$RESULTS/watchdog.log"
INTERVAL=10
LIFETIME=2400
FAILS_BEFORE_ACTION=2

log() { printf '%s  %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >>"$LOGFILE"; }

discover() {
    WIFI_DEV="$(nmcli -t -f DEVICE,TYPE,STATE device 2>/dev/null | awk -F: '$2=="wifi" && $3=="connected" {print $1; exit}')"
    WIFI_CON="$(nmcli -t -f DEVICE,TYPE,STATE,CONNECTION device 2>/dev/null | awk -F: '$2=="wifi" && $3=="connected" {print $4; exit}')"
    WIFI_GW="$(ip route show default 2>/dev/null | awk -v d="$WIFI_DEV" '$0 ~ ("dev " d) {print $3; exit}')"
}

healthy() {
    local defdev
    defdev="$(ip route show default 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="dev") print $(i+1)}' | head -1)"
    [ "$defdev" = "$WIFI_DEV" ] || { log "UNHEALTHY: default route on '${defdev:-none}', expected '$WIFI_DEV'"; return 1; }
    if [ -n "$WIFI_GW" ]; then
        ping -c1 -W2 -I "$WIFI_DEV" "$WIFI_GW" >/dev/null 2>&1 \
            || { log "UNHEALTHY: wifi gateway $WIFI_GW did not answer"; return 1; }
    fi
    return 0
}

remediate() {
    log "REMEDIATE: restoring wifi as the default path"
    while read -r line; do
        [ -z "$line" ] && continue
        local dev
        dev="$(echo "$line" | awk '{for(i=1;i<=NF;i++) if($i=="dev") print $(i+1)}')"
        if [ "$dev" != "$WIFI_DEV" ]; then
            sudo -n ip route del default dev "$dev" 2>>"$LOGFILE" \
                && log "REMEDIATE: deleted default via $dev" \
                || log "REMEDIATE: could not delete default via $dev"
        fi
    done < <(ip route show default 2>/dev/null)
    if ! nmcli -t -f DEVICE,STATE device 2>/dev/null | grep -q "^${WIFI_DEV}:connected$"; then
        sudo -n nmcli connection up "$WIFI_CON" >>"$LOGFILE" 2>&1 \
            && log "REMEDIATE: wifi back up" || log "REMEDIATE: could not bring wifi up"
    fi
    healthy && log "REMEDIATE: recovered" || log "REMEDIATE: still unhealthy"
}

run_loop() {
    discover
    log "=== watchdog start (pid $$) dev=$WIFI_DEV con=$WIFI_CON gw=$WIFI_GW lifetime=${LIFETIME}s ==="
    [ -z "$WIFI_DEV" ] && { log "FATAL: no connected wifi to protect"; rm -f "$PIDFILE"; exit 1; }
    local deadline=$(( $(date +%s) + LIFETIME )) fails=0
    while [ "$(date +%s)" -lt "$deadline" ]; do
        if healthy; then fails=0
        else
            fails=$((fails+1))
            [ "$fails" -ge "$FAILS_BEFORE_ACTION" ] && { remediate; fails=0; }
        fi
        sleep "$INTERVAL"
    done
    log "=== lifetime reached, final pass ==="
    healthy || remediate
    log "=== watchdog exit ==="
    rm -f "$PIDFILE"
}

cmd="${1:-status}"; shift || true
while [ $# -gt 0 ]; do
    case "$1" in
        --lifetime) LIFETIME="$2"; shift 2 ;;
        --interval) INTERVAL="$2"; shift 2 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done
mkdir -p "$RESULTS"

case "$cmd" in
    start)
        if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE" 2>/dev/null)" 2>/dev/null; then
            echo "already running (pid $(cat "$PIDFILE"))"; exit 0
        fi
        setsid nohup "$0" __run --lifetime "$LIFETIME" --interval "$INTERVAL" >/dev/null 2>&1 &
        echo $! >"$PIDFILE"; sleep 1
        echo "watchdog started (pid $(cat "$PIDFILE")), log: $LOGFILE" ;;
    __run) echo $$ >"$PIDFILE"; run_loop ;;
    stop)
        if [ -f "$PIDFILE" ]; then
            pid="$(cat "$PIDFILE")"
            kill "$pid" 2>/dev/null && echo "stopped (pid $pid)" || echo "pid $pid not running"
            rm -f "$PIDFILE"
        else echo "no pidfile"; fi ;;
    # Exit status only, for scripts. Piping `status` into head made status die
    # of SIGPIPE, and pipefail then reported a healthy watchdog as a failed one.
    is-running)
        if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE" 2>/dev/null)" 2>/dev/null; then exit 0; else exit 1; fi ;;
    status)
        if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE" 2>/dev/null)" 2>/dev/null; then
            echo "running (pid $(cat "$PIDFILE"))"; else echo "not running"; fi
        echo "--- last 15 log lines ---"; tail -15 "$LOGFILE" 2>/dev/null || echo "(no log yet)" ;;
    *) echo "usage: $0 {start|stop|status|is-running} [--lifetime S] [--interval S]" >&2; exit 2 ;;
esac
