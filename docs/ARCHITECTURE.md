# Architecture and interface boundaries

Impala OS is one process (`impala`) plus MPD, both running on a Raspberry Pi 4.
There are no microservices. Everything that can be hardware-independent is,
and every input feeds one command type.

```
 touch (Slint UI) ─┐
 CAN wheel (M3)  ──┤    Command      ┌──────────────────────┐   MediaBackend   ┌───────────┐
 voice (M4+)     ──┼──────────────►  │ player service thread │ ───────────────► │ MpdBackend│──TCP──► MPD ──ALSA──► jack / USB DAC
 headless stdin  ──┘ CommandSender   │ (impala_core::service)│ ◄─────────────── │ MockBackend│
                                      └──────────┬───────────┘   PlayerStatus    └───────────┘
                                                 │ StateUpdate (AppState + art)
                                                 ▼
                                Slint UI (invoke_from_event_loop) / console log
```

## Crates

| Crate | What it owns | Depends on |
|---|---|---|
| `impala-core` | `Command` with its text syntax, `AppState`, the `MediaBackend` trait, `MockBackend`, steering-wheel mapping, network probe, the player service loop | `tracing` only |
| `impala-mpd` | MPD protocol client and `MpdBackend` (reconnect with backoff, album art) | `impala-core` |
| `impala-ui` (binary `impala`) | Slint UI, `--screenshot`, `--headless`, config, logging | both, plus `slint`, `image`, `toml` |

## Boundaries for later work

### Inputs → `Command` (`impala_core::command`)
- Every input source gets a `CommandSender` (`ServiceHandle::sender()`) and
  calls `send(CommandSource::X, Command::Y)`. Sending is non-blocking and
  thread-safe.
- `Command` has a text form (`"vol +4"`, `"playlist Road Trip"`) that
  round-trips through `FromStr`/`Display`. Voice intents and any future IPC
  socket can reuse it unchanged.

### CAN steering wheel (M3) — `impala_core::input`
- The future CAN driver owns everything hardware-specific: opening the
  custom USB-CAN board and decoding GMLAN frames into `SteeringWheelButton`.
  Frame IDs must be captured from the car; none are guessed here.
- `steering_wheel_command(button)` turns a button into a `Command`. It is
  already unit-tested.
- Plan: a `crates/impala-can` that runs its own thread and holds a
  `CommandSender`. Long-presses (e.g. phone mode) become new `Command`
  variants, not side channels.

### Voice (M4+)
- Push-to-talk is reserved (`SteeringWheelButton::Voice` → `None` today).
- A local speech-to-intent engine should emit `Command` text, e.g.
  `"playlist Favourites"`, parsed with `Command::from_str`. No cloud
  dependency.

### Navigation (later)
- Navigation is a separate Slint screen in the same process. It reads GPS
  through gpsd (not installed in M1) behind a small `PositionSource` trait.
- It shares the window with now-playing: on a 280 px panel, the map takes
  the art and title area while transport and volume stay visible.
- Map tiles stay offline: an MBTiles file on the SD card or SSD.

### Media sources: Navidrome / OpenSubsonic (M2) — `MediaBackend`
- MPD stays the only audio engine. M2 adds a sync component that downloads
  favourites and recommendations into `/srv/music/navidrome-cache/…` (MPD
  plays them offline) and can hand MPD stream URLs when online. See
  [M2-PLAN.md](M2-PLAN.md).
- A non-MPD source would implement `MediaBackend`; the UI and inputs don't
  change.

### Display
- **Pi:** Slint `linuxkms` backend (noseat; libinput for touch) draws
  straight to DRM/KMS. It needs to be the display's only owner.
- **Desktop:** `winit` for development.
- `--screenshot` uses Slint's software renderer through a custom
  `Platform` and needs no display at all.
- Environment knobs from Slint: `SLINT_DRM_OUTPUT` (pick the HDMI
  connector), `SLINT_KMS_ROTATION`, `SLINT_DRM_MODE`.

### Audio
- MPD → ALSA `plughw:CARD=Headphones` in M1.
- The USB DAC to the Alpine unit is a one-line change in
  `config/mpd.conf` (`device "plughw:CARD=<dac>"`).
- If PipeWire is ever needed (LIVI, Bluetooth calls), switch MPD to its
  `pipewire` output instead of sharing the ALSA device.

## Failure handling
- **MPD down or restarting:** `MpdBackend` returns `Disconnected` and
  retries with exponential backoff (250 ms → 5 s). The UI disables
  transport buttons and shows "MPD offline – retry in Ns". The connection
  comes back automatically.
- **Missing or corrupt files, rejected commands:** MPD's `error:` field and
  `ACK` replies are shown in the status pill (command errors for 5 s). The
  app never panics on them.
- **No display:** linuxkms reports "No connected display connector found".
  The app logs it and exits with code 3, so systemd retries every 5 s.
- **No network:** nothing in M1 uses it. The pill shows Online when the
  kernel has a default route and Offline when it doesn't.
- **Logging:** `tracing` to stderr; under systemd that lands in the journal.
  Set the level in `[log] level` or with `RUST_LOG`.
