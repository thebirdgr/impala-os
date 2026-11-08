# System changes made on the Pi (impala.local)

This is the log for Milestone 1, 2026-11-08. Each entry says who made the
change and how to undo it. No change was made to the network
configuration, SSH, the firewall, partitions or boot config.

| # | Change | By | Undo |
|---|---|---|---|
| 1 | `/etc/sudoers.d/90-impala` (`<user> ALL=(ALL) NOPASSWD:ALL`): the earlier cloud-init NOPASSWD entry was gone when M1 started. An `apt full-upgrade` ran at 13:21, before M1; whether it removed the entry is unconfirmed. | Outside this session, 13:36, after Claude asked for option (a) restore or (b) manual apt. Claude did not edit sudoers. | `sudo rm /etc/sudoers.d/90-impala` |
| 2 | apt: `mpd mpc rustup build-essential libfontconfig1-dev libfreetype-dev libxkbcommon-dev libinput-dev libudev-dev libgbm-dev libdrm-dev` (plus their dependencies) | Outside this session, 13:36 (apt history), the approved list | `sudo apt remove <same list> && sudo apt autoremove` |
| 3 | `rustup default stable`: Rust 1.99.1 in `~/.rustup` and `~/.cargo` (per user, no sudo) | Claude | `rustup self uninstall` or `rm -rf ~/.rustup ~/.cargo` |
| 4 | `/srv/music` created, owned by the login user, group `audio`, mode 0755 | Claude via `scripts/pi-setup-mpd.sh` | `sudo rm -r /srv/music` |
| 5 | `/etc/impala-os/mpd.conf`: our MPD config (music in `/srv/music`, loopback only, ALSA `plughw:CARD=Headphones`, hardware `PCM` mixer) | Claude via script | `sudo rm -r /etc/impala-os` |
| 6 | `/etc/systemd/system/mpd.service.d/impala.conf`: drop-in so `mpd.service` uses (5). Debian's `/etc/mpd.conf` is untouched. | Claude via script | `sudo rm -r /etc/systemd/system/mpd.service.d && sudo systemctl daemon-reload` |
| 7 | `mpd.service` **enabled and started**. `mpd.socket` explicitly **disabled**: Debian's `Also=` would enable it, and it listens on port 6600 on all interfaces. | Claude via script (approved: "MPD as a system service") | `sudo systemctl disable --now mpd` |
| 8 | `/var/lib/mpd/playlists` (owned by mpd). MPD created `tag_cache`, `state` and `sticker.sql`. Playlists "Favourites" and "Road Trip" saved there. | Claude | `sudo rm /var/lib/mpd/{tag_cache,state,sticker.sql} /var/lib/mpd/playlists/*.m3u` |
| 9 | `/srv/music/impala-test/`: generated CC0 test tones, covers and one corrupt file | Claude via `scripts/make-test-audio.sh` | `rm -r /srv/music/impala-test` |
| 10 | `~/impala-os`: repo copy, build output and `build.log` | Claude via `scripts/deploy.sh` | `rm -rf ~/impala-os` |

## Not done (needs separate approval)
- Autostart unit: `impala-os@.service` is in the repo; not installed, not
  enabled.
- Switching boot to console (`multi-user.target`). The desktop session
  currently owns seat0, so the KMS UI needs this once the panel is attached.
- Firewall, SSH, network interfaces, partitions, boot config: untouched.
- LIVI, Navidrome, gpsd, Docker, AI models: not installed.

## Transient changes during tests
- `tests/pi-smoke.sh` stops and restarts `mpd.service`. It also briefly
  renames one test file to simulate a missing file, then renames it back.
- `tests/pi-offline-test.sh` disconnects and reconnects `wlan0` with
  `nmcli`. It also creates a transient `impala-wifi-safety` systemd timer
  that is removed at the end.
