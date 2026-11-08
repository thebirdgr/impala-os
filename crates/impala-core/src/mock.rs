//! An in-memory backend with plausible demo data. Used for UI development on
//! Windows, `--screenshot` previews and unit tests.

use crate::backend::{BackendError, MediaBackend};
use crate::command::{apply_volume_delta, Command, PlaylistSource};
use crate::state::{BackendStatus, PlaybackState, PlayerStatus, Track};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub struct MockBackend {
    library: Vec<Track>,
    playlists: BTreeMap<String, Vec<usize>>,
    queue: Vec<usize>,
    queue_version: u32,
    pos: Option<usize>,
    state: PlaybackState,
    elapsed: Duration,
    volume: u8,
    /// `None` = manual clock (tests call [`MockBackend::advance`]).
    last_tick: Option<Instant>,
    /// Simulate the daemon being down.
    pub offline: bool,
}

fn t(uri: &str, title: &str, artist: &str, album: &str, secs: u64) -> Track {
    Track {
        uri: uri.into(),
        title: Some(title.into()),
        artist: Some(artist.into()),
        album: Some(album.into()),
        duration: Some(Duration::from_secs(secs)),
    }
}

impl MockBackend {
    /// Demo data with a real-time clock (for the interactive UI).
    pub fn demo() -> Self {
        let mut m = Self::demo_manual_clock();
        m.last_tick = Some(Instant::now());
        m
    }

    /// Demo data whose clock only moves via [`MockBackend::advance`].
    pub fn demo_manual_clock() -> Self {
        let library = vec![
            t("demo/highway-tone.flac", "Highway Hum", "The Test Tones", "Signal Path", 214),
            t("demo/night-drive.flac", "Night Drive (A440)", "The Test Tones", "Signal Path", 187),
            t("demo/rest-stop.flac", "Rest Stop", "The Test Tones", "Signal Path", 243),
            t("demo/odometer.flac", "Odometer Rollover", "Sine Wave Society", "Low Frequencies", 302),
            t("demo/cruise.flac", "Cruise Control", "Sine Wave Society", "Low Frequencies", 199),
            t("demo/v6.flac", "3.5L V6 Idle", "Sine Wave Society", "Low Frequencies", 166),
        ];
        let mut playlists = BTreeMap::new();
        playlists.insert("Favourites".to_string(), vec![1, 3, 4]);
        playlists.insert("Road Trip".to_string(), vec![4, 0, 5, 2]);
        Self {
            queue: (0..library.len()).collect(),
            library,
            playlists,
            queue_version: 1,
            pos: Some(1),
            state: PlaybackState::Playing,
            elapsed: Duration::from_secs(71),
            volume: 60,
            last_tick: None,
            offline: false,
        }
    }

    /// Advance the simulated playback clock.
    pub fn advance(&mut self, dt: Duration) {
        if self.state != PlaybackState::Playing {
            return;
        }
        self.elapsed += dt;
        if let Some(d) = self.current().and_then(|t| t.duration) {
            if self.elapsed >= d {
                self.step(1);
            }
        }
    }

    fn current(&self) -> Option<&Track> {
        self.pos.and_then(|p| self.queue.get(p)).map(|&i| &self.library[i])
    }

    fn step(&mut self, delta: isize) {
        let Some(p) = self.pos else { return };
        let next = p as isize + delta;
        if next < 0 || next as usize >= self.queue.len() {
            self.state = PlaybackState::Stopped;
            self.pos = None;
        } else {
            self.pos = Some(next as usize);
        }
        self.elapsed = Duration::ZERO;
    }

    fn tick(&mut self) {
        if let Some(last) = self.last_tick {
            let now = Instant::now();
            self.advance(now - last);
            self.last_tick = Some(now);
        }
    }

    fn check(&self) -> Result<(), BackendError> {
        if self.offline {
            Err(BackendError::Disconnected("mock backend offline".into()))
        } else {
            Ok(())
        }
    }
}

impl MediaBackend for MockBackend {
    fn name(&self) -> &str {
        "Demo"
    }

    fn connection(&self) -> BackendStatus {
        if self.offline {
            BackendStatus::Disconnected { reason: "offline".into(), retry_in: Duration::from_secs(1) }
        } else {
            BackendStatus::Connected
        }
    }

    fn execute(&mut self, cmd: &Command) -> Result<(), BackendError> {
        self.check()?;
        self.tick();
        match cmd {
            Command::Play => {
                if self.pos.is_none() && !self.queue.is_empty() {
                    self.pos = Some(0);
                }
                if self.pos.is_some() {
                    self.state = PlaybackState::Playing;
                }
            }
            Command::Pause => {
                if self.state == PlaybackState::Playing {
                    self.state = PlaybackState::Paused;
                }
            }
            Command::TogglePlayPause => {
                let next = if self.state == PlaybackState::Playing { Command::Pause } else { Command::Play };
                return self.execute(&next);
            }
            Command::Next => self.step(1),
            Command::Prev => {
                // Like most head units: restart the track unless near its start.
                if self.elapsed > Duration::from_secs(3) {
                    self.elapsed = Duration::ZERO;
                } else {
                    self.step(-1);
                    if self.pos.is_none() && !self.queue.is_empty() {
                        self.pos = Some(0);
                        self.state = PlaybackState::Playing;
                    }
                }
            }
            Command::Seek(to) => {
                let d = self.current().and_then(|t| t.duration).unwrap_or_default();
                self.elapsed = (*to).min(d);
            }
            Command::SeekRelative(s) => {
                let d = self.current().and_then(|t| t.duration).unwrap_or_default();
                let e = (self.elapsed.as_secs() as i64 + *s as i64).max(0) as u64;
                self.elapsed = Duration::from_secs(e).min(d);
            }
            Command::SetVolume(v) => self.volume = (*v).min(100),
            Command::ChangeVolume(d) => self.volume = apply_volume_delta(self.volume, *d),
            Command::PlayQueuePosition(p) => {
                if *p >= self.queue.len() {
                    return Err(BackendError::Rejected(format!("No track {} in queue", p + 1)));
                }
                self.pos = Some(*p);
                self.elapsed = Duration::ZERO;
                self.state = PlaybackState::Playing;
            }
            Command::LoadPlaylist(src) => {
                self.queue = match src {
                    PlaylistSource::AllTracks => (0..self.library.len()).collect(),
                    PlaylistSource::Named(n) => self
                        .playlists
                        .get(n)
                        .cloned()
                        .ok_or_else(|| BackendError::Rejected(format!("No playlist \"{n}\"")))?,
                };
                self.queue_version += 1;
                self.pos = Some(0);
                self.elapsed = Duration::ZERO;
                self.state = PlaybackState::Playing;
            }
            Command::RefreshLibrary => {}
        }
        Ok(())
    }

    fn status(&mut self) -> Result<PlayerStatus, BackendError> {
        self.check()?;
        self.tick();
        let current = self.current().cloned();
        Ok(PlayerStatus {
            state: self.state,
            volume: Some(self.volume),
            elapsed: self.elapsed,
            duration: current.as_ref().and_then(|t| t.duration),
            queue_position: self.pos,
            queue_length: self.queue.len(),
            queue_version: self.queue_version,
            current,
            updating_library: false,
            backend_error: None,
        })
    }

    fn queue(&mut self) -> Result<Vec<Track>, BackendError> {
        self.check()?;
        Ok(self.queue.iter().map(|&i| self.library[i].clone()).collect())
    }

    fn playlists(&mut self) -> Result<Vec<String>, BackendError> {
        self.check()?;
        Ok(self.playlists.keys().cloned().collect())
    }

    fn album_art(&mut self, _uri: &str) -> Result<Option<Vec<u8>>, BackendError> {
        self.check()?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn transport_commands() {
        let mut m = MockBackend::demo_manual_clock();
        m.execute(&Command::Pause).unwrap();
        assert_eq!(m.status().unwrap().state, PlaybackState::Paused);
        m.execute(&Command::TogglePlayPause).unwrap();
        assert_eq!(m.status().unwrap().state, PlaybackState::Playing);
        m.execute(&Command::Next).unwrap();
        let s = m.status().unwrap();
        assert_eq!(s.queue_position, Some(2));
        assert_eq!(s.elapsed, Duration::ZERO);
        m.execute(&Command::Prev).unwrap();
        assert_eq!(m.status().unwrap().queue_position, Some(1));
    }

    #[test]
    fn prev_restarts_track_when_past_three_seconds() {
        let mut m = MockBackend::demo_manual_clock();
        m.advance(secs(10));
        m.execute(&Command::Prev).unwrap();
        let s = m.status().unwrap();
        assert_eq!(s.queue_position, Some(1));
        assert_eq!(s.elapsed, Duration::ZERO);
    }

    #[test]
    fn clock_advances_only_while_playing_and_rolls_over() {
        let mut m = MockBackend::demo_manual_clock();
        let start = m.status().unwrap().elapsed;
        m.advance(secs(5));
        assert_eq!(m.status().unwrap().elapsed, start + secs(5));
        m.execute(&Command::Pause).unwrap();
        m.advance(secs(5));
        assert_eq!(m.status().unwrap().elapsed, start + secs(5));
        m.execute(&Command::Play).unwrap();
        m.advance(secs(1000)); // past the end of track 2
        assert_eq!(m.status().unwrap().queue_position, Some(2));
    }

    #[test]
    fn seek_and_volume_are_clamped() {
        let mut m = MockBackend::demo_manual_clock();
        m.execute(&Command::Seek(secs(10_000))).unwrap();
        let s = m.status().unwrap();
        assert_eq!(Some(s.elapsed), s.duration);
        m.execute(&Command::SeekRelative(-100_000)).unwrap();
        assert_eq!(m.status().unwrap().elapsed, Duration::ZERO);
        m.execute(&Command::SetVolume(200)).unwrap();
        assert_eq!(m.status().unwrap().volume, Some(100));
        m.execute(&Command::ChangeVolume(-30)).unwrap();
        assert_eq!(m.status().unwrap().volume, Some(70));
    }

    #[test]
    fn playlists_load_and_reject_unknown() {
        let mut m = MockBackend::demo_manual_clock();
        let v0 = m.status().unwrap().queue_version;
        m.execute(&Command::LoadPlaylist(PlaylistSource::Named("Favourites".into()))).unwrap();
        let s = m.status().unwrap();
        assert_eq!(s.queue_length, 3);
        assert_ne!(s.queue_version, v0);
        assert_eq!(s.current.unwrap().title.as_deref(), Some("Night Drive (A440)"));
        let err = m.execute(&Command::LoadPlaylist(PlaylistSource::Named("Nope".into())));
        assert!(matches!(err, Err(BackendError::Rejected(_))));
        assert!(m.execute(&Command::PlayQueuePosition(99)).is_err());
    }

    #[test]
    fn offline_backend_errors_instead_of_panicking() {
        let mut m = MockBackend::demo_manual_clock();
        m.offline = true;
        assert!(matches!(m.status(), Err(BackendError::Disconnected(_))));
        assert!(matches!(m.execute(&Command::Play), Err(BackendError::Disconnected(_))));
        assert!(!m.connection().is_connected());
    }
}
