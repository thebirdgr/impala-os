#!/usr/bin/env bash
# Offline test ON THE PI. Disconnects Wi-Fi (the Pi's only internet route),
# checks that playback and controls keep working, then reconnects.
#
# Run it over the Ethernet IPv6 link-local path so the SSH session survives
# the Wi-Fi drop, e.g. from the PC:
#   ssh <user>@fe80::<pi-addr>%<if-index> 'cd impala-os && tests/pi-offline-test.sh'
#
# Safety net: a transient systemd timer reconnects wlan0 after 5 minutes
# even if this script dies half way.
set -uo pipefail
cd "$(dirname "$0")/.."
LOG=${LOG:-/tmp/impala-offline.log}
IFACE=${IFACE:-wlan0}
source tests/lib.sh

has_default_route() { ip -4 route show default | grep -q .; }

[[ -x $BIN ]] || { echo "build first" >&2; exit 2; }
has_default_route || { echo "no default route before the test; aborting" >&2; exit 2; }

sudo systemd-run --quiet --unit impala-wifi-safety --on-active=300 /usr/bin/nmcli dev connect "$IFACE"
trap 'sudo nmcli dev connect "$IFACE" >/dev/null 2>&1; sudo systemctl stop impala-wifi-safety.timer 2>/dev/null' EXIT

mpc -q stop; mpc -q clear
app_start
send "playlist Road Trip" 2
s=$(app_status)
check "online baseline: playing" test "$(st)" = playing
check "online baseline: app shows network=Online" grep -q 'network=Online' <<<"$s"

note "disconnecting $IFACE at $(date +%T)"
sudo nmcli dev disconnect "$IFACE"
sleep 5
check "no default route (truly offline)" bash -c '! ip -4 route show default | grep -q .'
check "internet unreachable" bash -c '! timeout 5 getent ahosts deb.debian.org >/dev/null'
s=$(app_status)
check "app shows network=Offline" grep -q 'network=Offline' <<<"$s"
check "MPD still connected" grep -q 'backend=Connected' <<<"$s"
a=$(secs); sleep 5; b=$(secs)
check "playback continues offline ($a → $b s)" test "$(st)" = playing -a "$b" -gt "$a"
p=$(pos); send next
check "next offline ($p → $(pos))" test "$(pos)" -eq $((p + 1))
send pause;  check "pause offline" test "$(st)" = paused
send toggle; check "resume offline" test "$(st)" = playing
send "vol 33"; check "volume offline ($(vol))" test "$(vol)" -eq 33
send "playlist Favourites" 2
check "load playlist offline ($(qlen) queued)" test "$(qlen)" -eq 3 -a "$(st)" = playing
a=$(secs); sleep 20; b=$(secs)
check "20 s more offline playback ($a → $b s)" test "$b" -ge $((a + 18))

note "reconnecting $IFACE at $(date +%T)"
sudo nmcli dev connect "$IFACE" >/dev/null
for _ in $(seq 30); do has_default_route && break; sleep 1; done
check "default route back" has_default_route
sleep 3
s=$(app_status)
check "app shows network=Online again" grep -q 'network=Online' <<<"$s"
check "playback undisturbed by reconnect" test "$(st)" = playing
check "MPD connection never dropped (1 connect in log)" test "$(log_count 'connected to MPD')" -eq 1
app_stop
check "app exits cleanly" test $? -eq 0
mpc -q pause
summary
