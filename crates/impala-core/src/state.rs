//! Application state shared between the service thread and the UI.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

/// One song as seen by the UI.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Track {
    /// Backend-specific identifier (MPD: path relative to music_directory).
    pub uri: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<Duration>,
}

impl Track {
    /// Title, or the file name without extension when the file has no tags.
    pub fn display_title(&self) -> String {
        if let Some(t) = self.title.as_deref().filter(|t| !t.trim().is_empty()) {
            return t.to_string();
        }
        let file = self.uri.rsplit('/').next().unwrap_or(&self.uri);
        match file.rsplit_once('.') {
            Some((stem, _)) if !stem.is_empty() => stem.to_string(),
            _ => file.to_string(),
        }
    }

    pub fn display_artist(&self) -> String {
        self.artist.clone().unwrap_or_else(|| "Unknown artist".into())
    }

    pub fn display_album(&self) -> String {
        self.album.clone().unwrap_or_default()
    }
}

/// Snapshot of the player as reported by a backend.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlayerStatus {
    pub state: PlaybackState,
    /// `None` when the output has no mixer (MPD reports `volume: -1`).
    pub volume: Option<u8>,
    pub elapsed: Duration,
    pub duration: Option<Duration>,
    pub queue_position: Option<usize>,
    pub queue_length: usize,
    /// Changes whenever the queue contents change (MPD `playlist` version).
    pub queue_version: u32,
    pub current: Option<Track>,
    /// Backend is scanning the library (MPD `updating_db`).
    pub updating_library: bool,
    /// Last error the backend itself reported (e.g. missing file).
    pub backend_error: Option<String>,
}

impl PlayerStatus {
    /// 0.0..=1.0 progress through the current track.
    pub fn progress(&self) -> f32 {
        match self.duration {
            Some(d) if !d.is_zero() => (self.elapsed.as_secs_f32() / d.as_secs_f32()).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
}

/// Health of the connection to the media backend (MPD).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BackendStatus {
    #[default]
    Connecting,
    Connected,
    Disconnected { reason: String, retry_in: Duration },
}

impl BackendStatus {
    pub fn is_connected(&self) -> bool {
        matches!(self, BackendStatus::Connected)
    }
}

/// Internet reachability as far as the OS knows (default route present).
/// Purely informational: nothing in M1 needs the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkStatus {
    Online,
    Offline,
    #[default]
    Unknown,
}

/// Everything the UI renders. Cloned and pushed to the UI on change.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AppState {
    pub backend_name: String,
    pub backend: BackendStatus,
    pub network: NetworkStatus,
    pub player: PlayerStatus,
    pub queue: Vec<Track>,
    pub playlists: Vec<String>,
    /// Last command failure, shown briefly in the status area.
    pub last_error: Option<String>,
}

impl AppState {
    /// One-line status for the status pill.
    pub fn status_line(&self) -> String {
        match &self.backend {
            BackendStatus::Connecting => format!("Connecting to {}…", self.backend_name),
            BackendStatus::Disconnected { retry_in, .. } => {
                format!("{} offline – retry in {}s", self.backend_name, retry_in.as_secs().max(1))
            }
            BackendStatus::Connected => {
                if let Some(e) = self.last_error.as_ref().or(self.player.backend_error.as_ref()) {
                    e.clone()
                } else if self.player.updating_library {
                    "Scanning library…".into()
                } else if self.player.queue_length == 0 {
                    "Queue empty – open Library".into()
                } else {
                    format!(
                        "Track {} of {}",
                        self.player.queue_position.map(|p| p + 1).unwrap_or(0),
                        self.player.queue_length
                    )
                }
            }
        }
    }
}

/// `m:ss`, or `h:mm:ss` for an hour or more.
pub fn format_time(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_time() {
        assert_eq!(format_time(Duration::from_secs(0)), "0:00");
        assert_eq!(format_time(Duration::from_secs(65)), "1:05");
        assert_eq!(format_time(Duration::from_millis(59_999)), "0:59");
        assert_eq!(format_time(Duration::from_secs(3600 + 62)), "1:01:02");
    }

    #[test]
    fn progress_is_clamped_and_safe() {
        let mut p = PlayerStatus::default();
        assert_eq!(p.progress(), 0.0);
        p.duration = Some(Duration::ZERO);
        assert_eq!(p.progress(), 0.0);
        p.duration = Some(Duration::from_secs(100));
        p.elapsed = Duration::from_secs(25);
        assert!((p.progress() - 0.25).abs() < 1e-6);
        p.elapsed = Duration::from_secs(500);
        assert_eq!(p.progress(), 1.0);
    }

    #[test]
    fn display_title_falls_back_to_filename() {
        let t = Track { uri: "Artist/Album/03 - Song.flac".into(), ..Default::default() };
        assert_eq!(t.display_title(), "03 - Song");
        let t = Track { uri: "noext".into(), title: Some("  ".into()), ..Default::default() };
        assert_eq!(t.display_title(), "noext");
        let t = Track { uri: "x.mp3".into(), title: Some("Real".into()), ..Default::default() };
        assert_eq!(t.display_title(), "Real");
        assert_eq!(t.display_artist(), "Unknown artist");
    }

    #[test]
    fn status_line_reflects_backend_health() {
        let mut s = AppState { backend_name: "MPD".into(), ..Default::default() };
        assert!(s.status_line().starts_with("Connecting"));
        s.backend = BackendStatus::Disconnected { reason: "refused".into(), retry_in: Duration::from_secs(4) };
        assert_eq!(s.status_line(), "MPD offline – retry in 4s");
        s.backend = BackendStatus::Connected;
        assert_eq!(s.status_line(), "Queue empty – open Library");
        s.player.queue_length = 5;
        s.player.queue_position = Some(1);
        assert_eq!(s.status_line(), "Track 2 of 5");
        s.player.backend_error = Some("Failed to decode x.flac".into());
        assert_eq!(s.status_line(), "Failed to decode x.flac");
    }
}
