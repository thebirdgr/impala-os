//! The boundary between the app and whatever actually plays audio.
//!
//! M1 has two implementations: [`crate::mock::MockBackend`] (UI development,
//! screenshots, tests) and `impala_mpd::MpdBackend`. M2's Navidrome/OpenSubsonic
//! streaming is expected to feed MPD (cached files / stream URLs) so it does
//! not need a third implementation, but it could add one here.

use crate::command::Command;
use crate::state::{BackendStatus, PlayerStatus, Track};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    /// Not connected (daemon down, socket refused…). Retried automatically.
    Disconnected(String),
    /// The backend understood but refused the request (MPD `ACK`).
    Rejected(String),
    /// The backend sent something we could not parse.
    Protocol(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::Disconnected(m) => write!(f, "not connected: {m}"),
            BackendError::Rejected(m) => write!(f, "{m}"),
            BackendError::Protocol(m) => write!(f, "protocol error: {m}"),
        }
    }
}

impl std::error::Error for BackendError {}

/// A media player backend. Implementations must never panic on I/O failure;
/// they report [`BackendError`] and keep trying to reconnect on later calls.
pub trait MediaBackend: Send {
    /// Short human name for the status line ("MPD", "Demo").
    fn name(&self) -> &str;
    /// Current connection health (cheap, no I/O).
    fn connection(&self) -> BackendStatus;
    fn execute(&mut self, cmd: &Command) -> Result<(), BackendError>;
    fn status(&mut self) -> Result<PlayerStatus, BackendError>;
    fn queue(&mut self) -> Result<Vec<Track>, BackendError>;
    fn playlists(&mut self) -> Result<Vec<String>, BackendError>;
    /// Encoded image bytes (JPEG/PNG) for a track, if any.
    fn album_art(&mut self, uri: &str) -> Result<Option<Vec<u8>>, BackendError>;
}
