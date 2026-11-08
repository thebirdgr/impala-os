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

The Pi run is recorded in the next section.

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

## Pi checks
*(filled in below after the run)*
