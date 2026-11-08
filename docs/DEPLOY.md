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

## Pi: one-time setup

Run these on the Pi (Raspberry Pi OS trixie, arm64). Every change is listed
in [PI-CHANGES.md](PI-CHANGES.md).

```bash
sudo apt install mpd mpc rustup build-essential libfontconfig1-dev libfreetype-dev \
     libxkbcommon-dev libinput-dev libudev-dev libgbm-dev libdrm-dev
```

```bash
rustup default stable
```

From the PC, copy the repo over without building yet:

```bash
scripts/deploy.sh --no-build
```

Back on the Pi:

```bash
cd ~/impala-os && sudo scripts/pi-setup-mpd.sh "$USER"
```

```bash
scripts/make-test-audio.sh /srv/music --playlists
```

## Deploy (from the PC, Git Bash or any POSIX shell)

```bash
scripts/deploy.sh
```

```bash
scripts/deploy.sh --background
```

- The first form copies the tracked and untracked (non-ignored) files with
  `tar | ssh`, then runs `cargo build --release --locked -j2` on the Pi.
- `--background` builds detached instead and logs to `~/impala-os/build.log`
  on the Pi.
- Files deleted locally are not deleted on the Pi.

Build times on the Pi 4 (1.8 GB RAM, `-j2`): see [TESTING.md](TESTING.md).
Incremental rebuilds after UI or Rust changes only recompile `impala-ui`.

## Run on the Pi

```bash
cd ~/impala-os
target/release/impala --screenshot /tmp/impala.png           # preview from live MPD state
target/release/impala --headless                             # control MPD from the console
target/release/impala                                        # real UI on the panel (linuxkms)
tests/pi-smoke.sh                                            # end-to-end test (uses sudo to restart MPD)
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
cargo build --release && sudo scripts/install-pi.sh
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

## Offline test (Wi-Fi off, over the Ethernet link-local path)

Find the Pi's IPv6 link-local address with `ip -6 addr show eth0` on the Pi,
and the PC's interface index with `Get-NetAdapter` in PowerShell. Then, from
the PC:

```bash
ssh <user>@fe80::<pi-address>%<pc-interface-index> 'cd impala-os && tests/pi-offline-test.sh'
```

The script disconnects `wlan0` with `nmcli`, checks playback and controls,
reconnects, and checks clean recovery. A systemd timer reconnects Wi-Fi
after 5 minutes even if the script dies.
