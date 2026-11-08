#!/usr/bin/env bash
# End-to-end test ON THE PI:  impala --headless → MPD → ALSA.
#
#   cd ~/impala-os && tests/pi-smoke.sh
#
# Needs: scripts/pi-setup-mpd.sh done, test library from
# scripts/make-test-audio.sh --playlists, a release build, and sudo (to
# stop/restart MPD for the reliability checks). Verifies through `mpc`,
# i.e. independently of the app under test.
set -uo pipefail
cd "$(dirname "$0")/.."
LOG=${LOG:-/tmp/impala-smoke.log}
source tests/lib.sh

[[ -x $BIN ]] || { echo "build first: cargo build --release" >&2; exit 2; }
systemctl is-active --quiet mpd || { echo "mpd.service is not running" >&2; exit 2; }

mpc -q stop; mpc -q clear
app_start
check "app connects to MPD" grep -q "connected to MPD" "$LOG"

note "transport"
send "playlist Favourites" 2
check "load playlist → 3 queued, playing" test "$(qlen)/$(st)" = "3/playing"
a=$(secs); sleep 3; b=$(secs)
check "elapsed advances while playing ($a → $b s)" test "$b" -gt "$a"
send pause
a=$(secs); sleep 2; b=$(secs)
check "pause freezes elapsed ($a = $b)" test "$(st)" = paused -a "$a" -eq "$b"
send toggle
check "toggle resumes" test "$(st)" = playing
p=$(pos); send next
check "next ($p → $(pos))" test "$(pos)" -eq $((p + 1))
send prev
check "prev near track start goes back ($(pos))" test "$(pos)" -eq "$p"
send "seek 20"
s=$(secs)
check "seek 20 (at ${s}s)" test "$s" -ge 19 -a "$s" -le 23
send prev
check "prev after 3 s restarts track (pos $(pos), $(secs)s)" test "$(pos)" -eq "$p" -a "$(secs)" -le 3
send "track 2"
check "track 2 → queue position 3" test "$(pos)" -eq 3
check "metadata reaches MPD: '$(title)'" test -n "$(title)"

note "volume"
send "vol 30";  check "vol 30 → $(vol)" test "$(vol)" -eq 30
send "vol +5";  check "vol +5 → $(vol)" test "$(vol)" -eq 35
send "vol -10"; check "vol -10 → $(vol)" test "$(vol)" -eq 25

note "library"
send "playlist *" 2
check "all tracks → $(qlen) queued" test "$(qlen)" -eq 8
send refresh 3
check "refresh library accepted" grep -q 'command=refresh' "$LOG"
s=$(app_status)
check "app state mirrors MPD: $(cut -c1-160 <<<"$s")" grep -q 'state=Playing' <<<"$s"

note "bad media"
n=$(mpc playlist -f '%file%' | grep -n 'not-really-audio' | cut -d: -f1)
send "track $((n - 1))" 3
s=$(app_status)
check "corrupt file: app alive" app_alive
check "corrupt file: error surfaced or skipped ($(grep -o 'error=.*' <<<"$s" | cut -c1-90))" \
    bash -c "grep -q 'error=Some' <<<'$s' || [[ \$(mpc current -f %file%) != *not-really-audio* ]]"
send "playlist Favourites" 2
victim=/srv/music/impala-test/mixed/c5.mp3
mv "$victim" "$victim.hidden"
send "track 2" 3
s=$(app_status)
mv "$victim.hidden" "$victim"
check "missing file: app alive" app_alive
check "missing file: error surfaced or skipped ($(grep -o 'error=.*' <<<"$s" | cut -c1-90))" \
    bash -c "grep -q 'error=Some' <<<'$s' || [[ \$(mpc current -f %file%) != *c5.mp3* ]]"

note "MPD restart / outage"
send "playlist Favourites" 2
c0=$(log_count "connected to MPD")
sudo systemctl restart mpd
sleep 4
check "reconnected after mpd restart ($(log_count 'connected to MPD') connects)" test "$(log_count 'connected to MPD')" -gt "$c0"
p=$(pos); send next
check "commands work after restart ($p → $(pos))" test "$(pos)" -ne "$p"
sudo systemctl stop mpd
sleep 2
send next
s=$(app_status)
check "MPD down: app alive" app_alive
check "MPD down: status shows Disconnected" grep -q 'backend=Disconnected' <<<"$s"
check "MPD down: command error logged" grep -q 'command failed' "$LOG"
sudo systemctl start mpd
sleep 6
send play 2
check "MPD back: playing again ($(st))" test "$(st)" = playing

note "shutdown, screenshot, no display"
app_stop
check "app exits cleanly on quit" test $? -eq 0
check "no panics in app log" bash -c "! grep -qi panick '$LOG'"
"$BIN" --screenshot /tmp/impala-pi-mpd.png >/tmp/impala-shot.log 2>&1
check "screenshot from live MPD state" test -s /tmp/impala-pi-mpd.png
timeout 30 "$BIN" >/tmp/impala-nodisplay.log 2>&1
rc=$?
check "no display: exits with code 3 (got $rc)" test "$rc" -eq 3
check "no display: clear log message" grep -q "no display available" /tmp/impala-nodisplay.log
check "no display: no panic" bash -c "! grep -qi panick /tmp/impala-nodisplay.log"

mpc -q pause
summary
