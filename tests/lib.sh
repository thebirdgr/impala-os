# Shared helpers for the on-Pi test scripts. Source, don't run.
# shellcheck shell=bash

BIN=${BIN:-target/release/impala}
LOG=${LOG:-/tmp/impala-test.log}
fails=0
passes=0

ok()    { echo "PASS  $*"; passes=$((passes + 1)); }
bad()   { echo "FAIL  $*"; fails=$((fails + 1)); }
check() { local desc=$1; shift; if "$@"; then ok "$desc"; else bad "$desc"; fi; }
note()  { echo "----  $*"; }

# MPD state as seen by mpc (independent of the app under test).
st()    { mpc status '%state%'; }                  # playing | paused | stopped
pos()   { mpc status '%songpos%'; }                # 1-based
qlen()  { mpc status '%length%'; }
vol()   { mpc status '%volume%' | tr -dc '0-9'; }
secs()  { local t; t=$(mpc status '%currenttime%'); echo $((10#${t%%:*} * 60 + 10#${t##*:})); }
title() { mpc current -f '%title%'; }

# Start `impala --headless` with stdin on a FIFO we keep open on fd 3.
app_start() {
    APP_FIFO=$(mktemp -u /tmp/impala-fifo.XXXXXX)
    mkfifo "$APP_FIFO"
    "$BIN" --headless --config config/impala.toml >"$LOG" 2>&1 <"$APP_FIFO" &
    APP_PID=$!
    exec 3>"$APP_FIFO"
    sleep 1.5
}
# send "<command>" [seconds to wait afterwards]
send() { echo "$1" >&3; sleep "${2:-1.2}"; }
app_alive() { kill -0 "$APP_PID" 2>/dev/null; }
app_stop() {
    echo quit >&3 2>/dev/null || true
    exec 3>&-
    local rc=0
    wait "$APP_PID" || rc=$?
    rm -f "$APP_FIFO"
    return $rc
}
# Last STATUS line printed by the app's `status` console command.
app_status() { send status 1; grep '^STATUS' "$LOG" | tail -1; }
log_count() { grep -c -- "$1" "$LOG" || true; }

summary() {
    echo
    echo "== $passes passed, $fails failed (app log: $LOG)"
    [[ $fails -eq 0 ]]
}
