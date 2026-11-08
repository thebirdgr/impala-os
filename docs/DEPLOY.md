# Build, run, test, deploy

Nothing here hardcodes a host, user or key. The Pi is reached as
`impala.local` (mDNS) unless `IMPALA_HOST` says otherwise. SSH user and key
come from `~/.ssh/config` or from `config/deploy.env`; copy
`config/deploy.env.example` to start (that file is git-ignored).

## Desktop (Windows / Linux / macOS)

- **Prerequisites:** Rust stable (`winget install Rustlang.Rustup` on
  Windows), plus a C toolchain (MSVC on Windows).
- The first build downloads and compiles Slint, about 3–5 minutes.

```bash
cargo test --workspace                                        # unit + integration tests
cargo run -p impala-ui -- --mock                              # UI window with demo data
cargo run -p impala-ui -- --mpd 127.0.0.1:6600                # UI against some MPD
cargo run -p impala-ui -- --mock --screenshot docs/screenshots/now-playing-1424x280.png
cargo run -p impala-ui -- --mock --library --screenshot docs/screenshots/library-1424x280.png
cargo run -p impala-ui -- --mock --size 2848x560 --screenshot big.png   # scaling check
```

## Command-line options

| Flag | Effect |
|---|---|
| `--config FILE` | Config file. Default search: `$IMPALA_CONFIG`, `/etc/impala-os/impala.toml`, `~/.config/impala-os/impala.toml`, `./config/impala.toml` |
| `--mock` | Demo backend instead of MPD |
| `--mpd HOST:PORT` | Override `[mpd].address` (env var: `IMPALA_MPD_ADDRESS`) |
| `--screenshot OUT.png` | Render one frame offscreen (software renderer), no display needed |
| `--size WxH` | Screenshot size (default 1424x280) |
| `--library` | Screenshot with the library panel open |
| `--headless` | No UI; read commands from stdin |

Exit codes: 0 ok, 2 bad arguments, 3 no display available.

The headless console accepts the `Command` text syntax: `play`, `pause`,
`toggle`, `next`, `prev`, `seek 90`, `seek -10`, `vol 40`, `vol +5`,
`track 2`, `playlist Favourites`, `playlist *`, `refresh`. It also accepts
`status` (prints one STATUS line) and `quit`.

Logging uses `tracing` on stderr. Set `[log] level` in the config or
`RUST_LOG=impala_mpd=debug,info`.

## Workflow

All development, compiling and testing happens on the PC. The Pi is the
hardware test target:

```
edit → cargo test / cargo run --mock (PC) → scripts/deploy-pi.sh (cross-build + copy + restart) → test on the Pi
```

## PC: one-time ARM64 cross-build setup (WSL2)

The Pi binary is built in WSL2 Ubuntu 24.04 with Ubuntu's `aarch64-linux-gnu`
cross gcc. It links against a **sysroot copied from the Pi**, so headers,
libraries and glibc are exactly Raspberry Pi OS trixie's.

```bash
wsl -d Ubuntu-24.04 -u root -- bash scripts/setup-wsl-cross.sh <your-wsl-user>
```

Installs amd64 packages only: the cross gcc/g++, `pkg-config`, `rustup`
(from apt) and the `aarch64-unknown-linux-gnu` Rust target.

```bash
scripts/make-pi-sysroot.sh
```

Copies the `-dev` packages Slint links against (fontconfig, freetype,
xkbcommon, libinput, udev, gbm, drm, libc) plus their installed dependency
closure from the Pi: 128 packages, 86 MB compressed, 269 MB unpacked at
`~/.cache/impala-os/sysroot` in WSL. Re-run it after `apt upgrade` on the
Pi.

**Don't use Ubuntu multiarch instead** (`dpkg --add-architecture arm64` and
`lib*-dev:arm64`). On 24.04 it pulls in `python3:arm64`, which replaces the
host's amd64 python3 and removes about 50 packages: cloud-init, ubuntu-wsl,
wsl-pro-service, unattended-upgrades and more. This happened during M1 and
was rolled back (see TESTING.md).

## Build and deploy (from the PC, Git Bash)

```bash
scripts/build-pi.sh
```

Cross-compiles to `dist/impala-aarch64` (git-ignored). From Git Bash it
runs inside WSL; the target dir lives at `~/.cache/impala-os/target` in WSL.

```bash
scripts/deploy-pi.sh
```

The one-command deploy:
1. Builds (unless `--no-build`).
2. Copies `config/ scripts/ services/ tests/ docs/` and the binary to
   `~/impala-os/` on the Pi (the binary goes to `bin/impala`).
3. Checks the new binary runs (`--version`).
4. Stops a running instance: SIGTERM, then SIGKILL after 5 s.
5. Swaps the binary in atomically.
6. Renders a frame from the live MPD state as a no-display health check.
7. Starts the UI (or restarts the systemd unit if enabled), unless
   `--no-start`.
8. Reports `RUNNING`, or `NOT RUNNING: no display available (exit 3)` while
   no screen is attached.

The Pi needs no Rust toolchain for this path.

## Pi: one-time setup

On the Pi (Raspberry Pi OS trixie, arm64). Every change is listed in
[PI-CHANGES.md](PI-CHANGES.md).

```bash
sudo apt install mpd mpc
```

Then, after the first `scripts/deploy-pi.sh` has copied the scripts:

```bash
cd ~/impala-os && sudo scripts/pi-setup-mpd.sh "$USER"
```

```bash
scripts/make-test-audio.sh /srv/music --playlists
```

## Fallback: build natively on the Pi (slow)

```bash
scripts/deploy.sh --background
```

- Copies the whole tree with `tar | ssh` and runs `cargo build --release
  --locked -j2` on the Pi, logging to `~/impala-os/build.log`.
- Needs `rustup build-essential libfontconfig1-dev libfreetype-dev
  libxkbcommon-dev libinput-dev libudev-dev libgbm-dev libdrm-dev` on the
  Pi.
- A cold build took well over 30 minutes at 71–75 °C and was interrupted
  once by an unexplained reboot (see TESTING.md), which is why the PC
  cross-build is the default.

## Run on the Pi

```bash
cd ~/impala-os
bin/impala --screenshot /tmp/impala.png           # preview from live MPD state
bin/impala --headless                             # control MPD from the console
bin/impala                                        # real UI on the panel (linuxkms)
tests/pi-smoke.sh                                 # end-to-end test (uses sudo to restart MPD)
```

Copy a preview back to the PC:

```bash
scp impala.local:/tmp/impala.png .
```

**Real UI on the touchscreen.** linuxkms needs exclusive use of the
display, so the labwc desktop session must not hold it. Either:
- switch the Pi to text boot: `sudo raspi-config` → System → Boot →
  Console. This is a boot config change; see PI-CHANGES.md before doing
  it; or
- for a quick try, stop the desktop session's compositor first.

Useful Slint variables:
- `SLINT_DRM_OUTPUT=HDMI-A-1` picks the connector.
- `SLINT_KMS_ROTATION=180` rotates the output if the panel is mounted
  upside down.

## Optional autostart (not enabled)

```bash
sudo scripts/install-pi.sh
```

```bash
sudo systemctl enable --now impala-os@$USER
```

```bash
journalctl -u impala-os@$USER -f
```

Remove it again with:

```bash
sudo systemctl disable --now impala-os@$USER
```

- `install-pi.sh` installs `/usr/local/bin/impala`, the config at
  `/etc/impala-os/impala.toml`, and the unit file. It does not enable it.
- Enabling it is what turns autostart on. Do that only once the desktop no
  longer owns the display.
- The unit restarts the app every 5 s while no display is connected (exit
  code 3).

## Offline test (Wi-Fi off)

The script disconnects `wlan0` with `nmcli`, checks playback and controls,
reconnects, and checks clean recovery. A systemd timer reconnects Wi-Fi
after 5 minutes even if the script dies.

The SSH session over Wi-Fi drops with it, so start the script detached:

```bash
ssh impala.local 'cd impala-os && setsid nohup tests/pi-offline-test.sh > /tmp/impala-offline-result.txt 2>&1 < /dev/null &'
```

About 90 s later, once Wi-Fi is back, read the result:

```bash
ssh impala.local cat /tmp/impala-offline-result.txt
```

A link that survives Wi-Fi going down (the direct Ethernet cable) would let
you watch it live. Currently it doesn't work: NetworkManager's `netplan-eth0`
profile wants DHCP, fails on the direct cable, and drops the IPv6
link-local address (see TESTING.md, blockers).
