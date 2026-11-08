# Tests, results, limitations

Everything below was actually run on 2026-11-08 unless marked **not run**.

## Automated tests (`cargo test --workspace`)

**Windows 11, Rust 1.99.1, MSVC: 43 passed, 0 failed; `cargo clippy --workspace --all-targets` clean.**

| Suite | Count | Covers |
|---|---|---|
| `impala-core` unit | 22 | Command text parse/print round-trip; volume and seek clamping; time formatting; progress maths; status line; mock backend transport/playlists/offline; steering-wheel mapping; `/proc/net/route` parsing (Wi-Fi online, link-local only, route down); service loop executes commands, survives backend outage and recovers, error message expiry |
| `impala-mpd` unit | 8 | MPD protocol parsing: status, legacy `time:`, no mixer, `error:` field, song lists, ACK, quoting; backoff doubling, cap and reset |
| `impala-mpd` integration (`tests/fake_mpd.rs`) | 4 | Against an in-process fake MPD: every `Command` → protocol; ACKs keep the connection; chunked album-art reassembly; MPD down at startup (fails fast within backoff, connects when MPD appears); **kill + restart MPD on the same port → automatic reconnect** |
| `impala-ui` unit | 9 | Touch → `Command` mapping; config defaults, partial files, typo rejection, repo config validity, missing file fallback; album-art decode; **synthetic pointer taps on the real 1424×280 layout** (prev, play, next, vol ±, seek, library → All tracks, disabled when MPD down) |

The Pi runs are recorded below.

## Windows checks
- `impala --mock` opens a desktop window titled "Impala OS" and runs; it
  was killed after 5 s.
- `--screenshot` previews:
  - `docs/screenshots/now-playing-1424x280.png` and `library-1424x280.png`
    (mock data).
  - Scaling: renders at 2848×560 and letterboxes at 1280×720; inspected
    visually.
- Against the Pi's real MPD 0.24.4 through an SSH tunnel (Windows binary,
  `--mpd 127.0.0.1:16600`):
  - `--headless`: load playlist, next, vol 20 and pause all reached MPD;
    metadata and elapsed time came back.
  - `--screenshot` with **embedded cover** (`readpicture`) and with
    **folder `cover.jpg`** (`albumart`):
    `docs/screenshots/windows-live-mpd-1424x280.png`.

## Build

| Where | How | Result |
|---|---|---|
| Pi 4, native, `cargo build --release -j2` | `scripts/deploy.sh --background` | **Abandoned.** 164/271 crates in ~30 min at 71–75 °C. Lost once to a reboot; stopped at the user's request ("why are we building it on the Raspberry Pi?") |
| PC, WSL2 Ubuntu 24.04 → aarch64 | `scripts/build-pi.sh` (Pi sysroot) | **82 s cold**, 13 MB `ELF 64-bit ARM aarch64`. `impala --version` on the Pi: `impala 0.1.0 (aarch64-linux)` |

## Pi checks (Raspberry Pi 4, Raspberry Pi OS trixie, binary from the PC cross-build)

| Test | Result |
|---|---|
| `scripts/deploy-pi.sh` | Binary runs; live-MPD render check passes; UI exits 3 with every renderer's reason logged (see "Display") |
| `tests/pi-smoke.sh` | **33 passed, 0 failed.** Transport (play/pause/toggle/next/prev, prev-restarts after 3 s, seek, track N), volume (abs/±), playlists, all tracks, library refresh, corrupt file (MPD "Failed to decode", app alive), missing file (renamed after scan: error shown, app alive), `systemctl restart mpd` (auto-reconnect, commands work), MPD stopped (status Disconnected, command error logged, no crash) then started (playing again), clean quit, no panics, screenshot from live MPD, no-display exit 3 with clear log message |
| `tests/pi-offline-test.sh --already-offline` | **13 passed, 0 failed.** No default route, DNS fails; app shows Offline; MPD connected; playback, next, pause/resume, volume, playlist load and 20 s of continuous playback work; MPD never reconnected |
| Live preview from the Pi | `docs/screenshots/pi-live-mpd-1424x280.png` (rendered on the Pi by the software renderer from real MPD state) |
| MPD → 3.5 mm jack | ALSA `bcm2835 Headphones` PCM state `RUNNING`, elapsed advancing (`mpc`). Not listened to |
| MPD → Bluetooth AirPods (PipeWire, A2DP) | App `--headless` loaded "Favourites"; PipeWire showed the MPD stream `[active]` on the AirPods sink. **Heard by the user:** "yes i can hear on my airpods" |

### Offline test: what was and wasn't run
- **Approved:** the user said "Approve offline test" for the
  disconnect-and-reconnect variant (`nmcli dev disconnect wlan0`).
- **Couldn't run that variant:** at 14:40 LIVI was started from a terminal
  on the Pi's desktop. Its `99-livi-ap-unmanaged.conf` took wlan0 away from
  NetworkManager, so the Pi was **already offline**. Toggling wlan0 would
  have fought LIVI.
- **What ran instead:** `--already-offline`, which touches no interfaces.
- **Still open:** the reconnect half ("Online again; playback undisturbed")
  is unverified on the Pi. It is covered by the mock-backend service tests
  and the network-probe unit tests.

## Display
- The UI cannot open the panel yet:
  - Both HDMI connectors report `disconnected`.
  - The labwc desktop session is DRM master on vc4. Seen in
    `/sys/kernel/debug/dri/1/clients`.
  - linuxkms therefore gets `Permission denied` on DRM resources and finds
    no framebuffer.
- The app logs each renderer's reason and exits with code 3; no panic.
- **Not tested:** the real 1424×280 panel, touch input on it, rotation.
  These need the panel, and the desktop session stopped (console boot).
- Touch hit-testing is covered by the offscreen pointer test on the PC.

## Incidents
1. **Two unexplained reboots, caused by under-voltage.**
   - When: about 14:11 (during the Pi build) and again during the offline
     test.
   - Evidence after the second one:
     - the kernel log has `hwmon1: Undervoltage detected!` at 13 s
       uptime;
     - `vcgencmd get_throttled` = `0x50000` (under-voltage and throttling
       have occurred).
   - Fix (hardware): an official 5.1 V / 3 A supply and a short, thick
     USB-C cable.
   - The journal is not persistent, so earlier boots left no logs.
2. **The WSL2 Ubuntu package database was damaged and restored.**
   - The first cross-build setup used Ubuntu multiarch (`lib*-dev:arm64`).
     apt replaced amd64 `python3` with `python3:arm64` and removed 53 amd64
     packages: cloud-init, ubuntu-wsl, wsl-pro-service,
     unattended-upgrades, and the python3-* libraries.
   - Rolled back: purged all arm64 packages, removed the architecture and
     the ports source, reinstalled all 53 packages. Result: 0 missing,
     `dpkg --audit` clean, `apt-get check` OK, Python 3.12.3,
     `wsl-pro.service` active.
   - The build now uses a sysroot copied from the Pi instead.
3. **The Ethernet link-local path kept dropping.**
   - NetworkManager's `netplan-eth0` profile retried DHCP on the direct
     cable, and each failure removed the link-local address.
   - With Wi-Fi gone (LIVI), the Pi was unreachable for minutes at a time.
   - Fixed with the user's approval: `ipv4.method link-local`,
     `ipv6.method link-local` (see PI-CHANGES.md).

## Limitations and blockers
- **Power:** under-voltage reboots the Pi under load. Fix the supply before
  anything else.
- **Wi-Fi:** off since LIVI's run. To restore, remove
  `/etc/NetworkManager/conf.d/99-livi-ap-unmanaged.conf` and restart
  NetworkManager. That's the user's call; untouched.
- **Display:** console boot is needed for the KMS UI. This is a boot config
  change and needs approval. Panel and touch are untested.
- **Audio:** confirmed audible on Bluetooth AirPods (via PipeWire). The
  3.5 mm jack hasn't been listened to. The USB DAC to the Alpine unit isn't
  available.
- **Clock:** the Pi has no RTC and no network time while offline, so after
  a reboot its clock was about 3.5 min behind.
- **Not available yet:** CAN board, GPS, cellular and the Navidrome server.
  Only the interfaces exist (see ARCHITECTURE.md).
- **Native Pi builds** are impractical on this hardware and power supply;
  use the PC cross-build.
