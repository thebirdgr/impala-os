# LIVI evaluation (read-only)

Source: <https://github.com/f-io/LIVI>, commit `72230ae` (2026-11-08), read
from a shallow clone. Nothing was installed or run. Paths below are relative
to `native/crates/` unless noted. Claims marked *(verified)* were checked a
second time against the source; the rest come from one read-through of it.

## What it is

- A full CarPlay and Android Auto head unit, not just a receiver. It also
  has a media player, DAB+/FM radio, a dashboard, GPS and climate pages.
- Built with Rust, Slint and GStreamer. It runs as several processes, and
  one helper runs as root through a sudoers rule.
- **License:** GPL-3.0-or-later, plus a required attribution "Based on LIVI
  by Lasse Heitgres" (`LICENSE`, `NOTICE`). Linking its code into ours
  would make our app GPL.
- **CarPlay** needs an Apple MFi chip on I2C or a "LIVI Link" dongle.
  Android Auto works without either.

## Does it handle 1424×280?

**Display mode: yes.**
- `livi-compositor/src/drm/modes.rs` (`choose`) uses the panel's preferred
  mode when it is no larger than 1280×720 in area. 1424×280 qualifies.
  *(Verified.)*

**Phone projection: only in principle.**
- **Size sent to the phone:** `livi-core/src/config_file.rs` (`fit_720p`)
  scales the panel size to fit inside 1280×720. For our panel that is
  **1280×252**. *(Verified: 1424 × 0.899 = 1280, 280 × 0.899 ≈ 252.)*
- **Android Auto** only offers fixed sizes from 800×480 up. For our panel it
  asks for 1280×720 with a band left empty, so the phone draws about
  1280×252 of content. The compositor crops the band and scales the rest to
  fit the panel (`livi-aa-stack/src/config.rs`, `livi-compositor/src/layout.rs`).
- **CarPlay** sends the configured width and height as is.
- **Biggest unknown, not confirmable from source:** whether iPhones and
  Android phones actually draw a usable screen at about 5:1 and 250–280 px
  tall. Test this on the real panel before investing in LIVI.

## Display and audio ownership

**Display**
- LIVI runs its own compositor (built on smithay) and takes direct control
  of the display (DRM master) through **seatd** (`livi-compositor/src/drm.rs`).
- Only one program can drive the display at a time. Our app also draws
  directly to the display (linuxkms), so **the two cannot be on screen at
  the same time.**
- It does handle console (VT) switching: it releases the display when
  switched away and takes it back after.
- Other Wayland apps can connect to it, but any window that isn't LIVI's
  own is shown as a centred dialog (`shell.rs`). Hosting our UI inside LIVI
  is not realistic.

**Audio**
- All playback and the mic go through the PulseAudio interface, served by
  PipeWire (`livi-audio/src/player.rs` uses `pulsesink`). *(Verified.)*
- LIVI never opens ALSA itself. It changes the system volume with `pactl`.
- Wireless Android Auto calls need PipeWire's Bluetooth hands-free plugin.
- **Consequence for us:** MPD in M1 writes straight to ALSA (`plughw:Headphones`).
  While LIVI is playing, that device is held by PipeWire, so MPD would get
  "device busy". If LIVI is adopted, move MPD to its `pipewire` (or `pulse`)
  output first.

## What its installer changes

*(Headless path: `scripts/install/headless/install.sh`, verified.)*

- **Packages:** bluez, hostapd, dnsmasq, avahi, PipeWire/WirePlumber, seatd,
  GStreamer.
- **Users and devices:** adds the user to the video, render, input and
  plugdev groups; installs a udev rules file.
- **Root access:** a sudoers entry for its helper.
- **Network:** NetworkManager Wi-Fi settings and an access point for
  wireless Android Auto.
- **Boot:**
  - Enables seatd.
  - Sets **tty1 autologin**.
  - Runs **`systemctl set-default multi-user.target`**.
  - Starts LIVI on tty1.
  - Optionally edits `/boot/firmware/cmdline.txt`.
  - Optionally adds `config.txt` overlays for the MFi chip and a boot splash.

In short, it takes over the whole box. Running it as is would fight both our
MPD-on-ALSA setup and our systemd unit.

## Control surface

- An HTTP `POST /telemetry` endpoint and socket.io on **port 4000, bound to
  `[::]` and `0.0.0.0`** (`livi-core/src/telemetry.rs`). *(Verified.)*
- Sending a `path` value navigates its UI; `/projection` brings up the phone
  screen.
- There is no API for embedding LIVI or handing the display back.

## Can it coexist with our app?

| Option | Verdict |
|---|---|
| Our app as a Wayland client inside LIVI | No: our window would be shown as a centred dialog |
| Both inside a third compositor | Possible, but needs our app moved off linuxkms and costs RAM we don't have (1.8 GB) |
| VT switching between the two | Plausible: LIVI supports it. Unverified whether Slint's linuxkms gives the display back cleanly |
| **Separate mode: stop one, start the other** | **Works with what exists today** |

## Recommendation

**Keep Impala OS standalone, and add LIVI later as a "phone mode" we switch
into.** Integrating with LIVI does not beat a standalone app:
- It owns the display and audio stack.
- It is GPL with an attribution clause.
- It is a whole head unit whose UI was not designed for a 280 px-tall strip.
- Our core use cases (offline music, later navigation and CAN) don't need it.

Concretely, in a later milestone:
1. **Don't run its installer.** Install the AppImage and the packages it
   needs by hand. Skip the getty/autologin, sudoers and boot-config changes.
2. **Own the switch with systemd.** Write our own `livi.service` with
   `Conflicts=impala-os@.service` (and the reverse). A "Phone" button in our
   UI (or a steering-wheel long-press) runs `systemctl start livi`; when LIVI
   exits, our unit is restarted.
3. **Move MPD to PipeWire output** once LIVI or any other PipeWire audio
   user is present, and keep it there for the USB DAC.
4. **Firewall port 4000**, or bind it to loopback with a patch.
5. **Test both phones on the real 1424×280 panel early**, at 1280×252 and
   1424×280. If projection isn't usable at that aspect ratio, phone mode is
   moot.
6. **Expect the phone to disconnect** each time we leave phone mode; it
   reconnects when we come back.
