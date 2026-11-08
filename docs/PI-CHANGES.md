# System changes made on the Pi (impala.local)

This is the log for Milestone 1, 2026-11-08. Each entry says who made the
change and how to undo it. No change was made to SSH, the firewall, partitions or boot config. The
only network change is #11, approved by the user.

| # | Change | By | Undo |
|---|---|---|---|
| 1 | `/etc/sudoers.d/90-impala` (`<user> ALL=(ALL) NOPASSWD:ALL`, 0440 root:root, passes `visudo -c`): the earlier cloud-init NOPASSWD entry was gone when M1 started. An `apt full-upgrade` ran at 13:21, before M1; whether it removed the entry is unconfirmed. | Created 13:36 outside this session after the user chose option (a), "Passwordless sudo (a)". Claude did not edit sudoers. | `sudo rm /etc/sudoers.d/90-impala` |
| 2 | apt: `mpd mpc rustup build-essential libfontconfig1-dev libfreetype-dev libxkbcommon-dev libinput-dev libudev-dev libgbm-dev libdrm-dev` (plus their dependencies) | Outside this session, 13:36 (apt history), the approved list | `sudo apt remove <same list> && sudo apt autoremove` |
| 3 | `rustup default stable`: Rust 1.99.1 in `~/.rustup` and `~/.cargo` (per user, no sudo) | Claude | `rustup self uninstall` or `rm -rf ~/.rustup ~/.cargo` |
| 4 | `/srv/music` created, owned by the login user, group `audio`, mode 0755 | Claude via `scripts/pi-setup-mpd.sh` | `sudo rm -r /srv/music` |
| 5 | `/etc/impala-os/mpd.conf`: our MPD config (music in `/srv/music`, loopback only, ALSA `plughw:CARD=Headphones`, hardware `PCM` mixer) | Claude via script | `sudo rm -r /etc/impala-os` |
| 6 | `/etc/systemd/system/mpd.service.d/impala.conf`: drop-in so `mpd.service` uses (5). Debian's `/etc/mpd.conf` is untouched. | Claude via script | `sudo rm -r /etc/systemd/system/mpd.service.d && sudo systemctl daemon-reload` |
| 7 | `mpd.service` **enabled and started**. `mpd.socket` explicitly **disabled**: Debian's `Also=` would enable it, and it listens on port 6600 on all interfaces. | Claude via script (approved: "MPD as a system service") | `sudo systemctl disable --now mpd` |
| 8 | `/var/lib/mpd/playlists` (owned by mpd). MPD created `tag_cache`, `state` and `sticker.sql`. Playlists "Favourites" and "Road Trip" saved there. | Claude | `sudo rm /var/lib/mpd/{tag_cache,state,sticker.sql} /var/lib/mpd/playlists/*.m3u` |
| 9 | `/srv/music/impala-test/`: generated CC0 test tones, covers and one corrupt file | Claude via `scripts/make-test-audio.sh` | `rm -r /srv/music/impala-test` |
| 10 | `~/impala-os`: repo copy, `bin/impala` (cross-built), partial native build output (`target/`, `build.log`) | Claude via `scripts/deploy.sh` / `deploy-pi.sh` | `rm -rf ~/impala-os` |
| 11 | NetworkManager profile `netplan-eth0`: `ipv4.method auto → link-local`, `ipv6.method auto → link-local` (persisted by netplan in `/etc/netplan/90-NM-75a1216a-….yaml`). eth0 now keeps 169.254.x.x and fe80:: on the direct PC cable instead of retrying DHCP. | Claude, **approved by the user** ("Yes, fix eth0") | `sudo nmcli con mod netplan-eth0 ipv4.method auto ipv6.method auto && sudo nmcli con up netplan-eth0` |
| 12 | `~/memwatch.log`, `/tmp/mw*.sh`: build-time memory/temperature logger (stopped) | Claude | `rm ~/memwatch.log` |

## Changes made by LIVI (not by Claude)

At 14:40, `~/livi-test/LIVI.AppImage` was started from a terminal on the
Pi's desktop. It created:
- `/etc/sudoers.d/99-LIVI-gvfs` and `/etc/sudoers.d/99-LIVI-wifi-ap`
- `/usr/local/sbin/livi-helperd`
- `/usr/local/lib/livi/`
- `livi-wifi-ap.service` (installed, not enabled)
- `/etc/NetworkManager/conf.d/99-livi-ap-unmanaged.conf`: this makes wlan0
  unmanaged, so **the Pi is off Wi-Fi**
- `~/.config/LIVI/`

Claude did not modify or revert any of these.

## Not done (needs separate approval)
- Autostart unit: `impala-os@.service` is in the repo; not installed, not
  enabled.
- Switching boot to console (`multi-user.target`). The desktop session
  currently owns seat0, so the KMS UI needs this once the panel is attached.
- Firewall, SSH, partitions, boot config: untouched. Network: only change #11 above.
- LIVI, Navidrome, gpsd, Docker, AI models: not installed.

## Transient changes during tests
- `tests/pi-smoke.sh` stops and restarts `mpd.service`. It also briefly
  renames one test file to simulate a missing file, then renames it back.
- `tests/pi-offline-test.sh` disconnects and reconnects `wlan0` with
  `nmcli`. It also creates a transient `impala-wifi-safety` systemd timer
  that is removed at the end.
