# Milestone 2 plan: Navidrome / OpenSubsonic with offline caching

**Goal:** the home Navidrome server becomes the library. Favourites and
recommendations are cached on the Pi, so the car plays them with no
network. When online (home Wi-Fi, later cellular), more of the library is
reachable.

## Principles
- **MPD stays the only player.** Cached tracks are ordinary files under
  `/srv/music/navidrome/…`, so offline playback is the exact path M1
  already tests.
- **Sync is opportunistic and resumable.** It runs only when the network is
  up (the existing `NetworkStatus`), never blocks the UI, and never deletes
  anything that is playing.
- **No secrets in the repo.** Server URL and username live in
  `/etc/impala-os/impala.toml`, the token in
  `/etc/impala-os/navidrome.secret` (mode 0600). Use OpenSubsonic token
  auth (salted md5) or an API key, never a plaintext password in URLs.

## Work items
1. **`crates/impala-subsonic`: blocking OpenSubsonic client.**
   - Endpoints: `ping`, `getStarred2`, `getPlaylists`, `getPlaylist`,
     `getSimilarSongs2` / `getTopSongs`, `getAlbumList2?type=frequent|recent`,
     `getCoverArt`, `download`, `scrobble`.
   - Testing: unit-test against recorded JSON fixtures, plus an in-process
     fake server like `tests/fake_mpd.rs`.
2. **Cache policy (`impala-core::cache`, pure and testable).**
   - **Inputs:** starred songs, chosen playlists, recommendations (similar
     to starred, frequent and recent albums), the free-space budget, and
     each track's last-played time.
   - **Output:** download and evict lists.
   - **Priority:** pinned playlists > starred > recently played >
     recommendations.
   - **Budget:** default 20 GB, leaving 10 GB free (SD card today, SSD
     later).
3. **Sync worker** (a thread in `impala`, or a oneshot `impala-sync.timer`).
   - Downloads to `*.part`, then fsyncs and renames, so a power cut
     mid-download never leaves half a file. That matters in a car.
   - Writes `cover.jpg` per album and triggers MPD `update navidrome/`.
   - Mirrors server playlists as MPD playlists named `Navidrome: <name>`,
     containing only cached tracks.
4. **Scrobbling:** queue plays locally (append-only file) and flush to
   Navidrome when online.
5. **UI.**
   - Library tabs: "On this car" (cached) vs "Server" (online only, greyed
     when offline).
   - Download badge on cached items, and a sync status line in the pill.
   - Still one-tap actions only while driving.
6. **Streaming fallback when online:** `add http://…/rest/stream?...` into
   MPD's queue (the `curl` input is already enabled in `config/mpd.conf`).
   The token goes in the URL here, which is why it's a dedicated
   low-privilege API key.
7. **Tests.**
   - Cache-policy unit tests: budget, eviction order, never evicting the
     current track.
   - Sync against a fake server.
   - A repeat of the M1 offline test with a cache-only library.
   - A power-cut test: kill the sync mid-download; no partial files are
     visible to MPD.

## Open questions for the user
- Is the Navidrome server reachable from the car only on home Wi-Fi, or
  also over cellular or a VPN (Tailscale/WireGuard)?
- Cache size budget and storage: SD card now, or wait for the SSD?
- Which playlists should always be pinned?
