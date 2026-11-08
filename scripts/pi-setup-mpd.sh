#!/usr/bin/env bash
# Configure MPD as a system service for Impala OS. Run ON THE PI from the
# repo checkout:   sudo scripts/pi-setup-mpd.sh [music-owner]
#
# Idempotent. Changes (all reversible, see docs/PI-CHANGES.md):
#   /srv/music                                  music library (owner: music-owner)
#   /etc/impala-os/mpd.conf                     our MPD config
#   /etc/systemd/system/mpd.service.d/impala.conf   ExecStart drop-in
#   mpd.service enabled + started (mpd.socket stays disabled)
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
    echo "run with sudo" >&2
    exit 1
fi
here="$(cd "$(dirname "$0")/.." && pwd)"
owner="${1:-${SUDO_USER:-}}"
if [[ -z "$owner" ]]; then
    echo "usage: sudo $0 <music-owner>" >&2
    exit 1
fi

install -d -m 0755 -o "$owner" -g audio /srv/music
install -d -m 0755 /etc/impala-os
install -m 0644 "$here/config/mpd.conf" /etc/impala-os/mpd.conf
install -d -m 0755 /etc/systemd/system/mpd.service.d
install -m 0644 "$here/services/mpd-impala.conf" /etc/systemd/system/mpd.service.d/impala.conf
install -d -o mpd -g audio -m 0755 /var/lib/mpd/playlists

# Validate before (re)starting so a typo cannot leave MPD down.
if ! out=$(runuser -u mpd -- mpd --no-daemon --stdout /etc/impala-os/mpd.conf --version 2>&1); then
    echo "$out" >&2
fi

systemctl daemon-reload
systemctl disable --now mpd.socket 2>/dev/null || true
systemctl enable mpd.service
systemctl restart mpd.service
sleep 1
systemctl --no-pager --lines=5 status mpd.service || true
mpc -q status >/dev/null && echo "MPD is up on localhost:6600"
