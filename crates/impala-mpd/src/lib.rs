//! MPD backend for Impala OS.
//!
//! A deliberately small, blocking, dependency-free client for the MPD text
//! protocol. All calls go through [`MpdBackend::with_conn`], which lazily
//! (re)connects with exponential backoff, so MPD being down or restarting is
//! just a [`BackendError::Disconnected`] the UI can display.

pub mod backoff;
pub mod conn;
pub mod protocol;

use backoff::Backoff;
use conn::{ConnError, Connection};
use impala_core::command::apply_volume_delta;
use impala_core::{BackendError, BackendStatus, Command, MediaBackend, PlaybackState, PlayerStatus, PlaylistSource, Track};
use protocol::{parse_playlist_names, parse_songs, parse_status, quote};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct MpdConfig {
    /// `host:port`, e.g. `localhost:6600`.
    pub address: String,
    pub connect_timeout: Duration,
    pub backoff_initial: Duration,
    pub backoff_max: Duration,
}

impl Default for MpdConfig {
    fn default() -> Self {
        Self {
            address: "localhost:6600".into(),
            connect_timeout: Duration::from_millis(800),
            backoff_initial: Duration::from_millis(250),
            backoff_max: Duration::from_secs(5),
        }
    }
}

pub struct MpdBackend {
    cfg: MpdConfig,
    conn: Option<Connection>,
    backoff: Backoff,
    last_error: Option<String>,
}

impl MpdBackend {
    pub fn new(cfg: MpdConfig) -> Self {
        let backoff = Backoff::new(cfg.backoff_initial, cfg.backoff_max);
        Self { cfg, conn: None, backoff, last_error: None }
    }

    fn ensure_connected(&mut self) -> Result<&mut Connection, BackendError> {
        if self.conn.is_none() {
            let now = Instant::now();
            if !self.backoff.ready(now) {
                return Err(BackendError::Disconnected(self.last_error.clone().unwrap_or_else(|| "waiting to retry".into())));
            }
            match Connection::connect(&self.cfg.address, self.cfg.connect_timeout) {
                Ok(c) => {
                    info!(address = %self.cfg.address, version = %c.server_version, "connected to MPD");
                    self.backoff.succeed();
                    self.last_error = None;
                    self.conn = Some(c);
                }
                Err(e) => {
                    let msg = match e {
                        ConnError::Io(e) => e.to_string(),
                        ConnError::Ack(m) | ConnError::Protocol(m) => m,
                    };
                    if self.last_error.as_deref() != Some(&msg) {
                        warn!(address = %self.cfg.address, error = %msg, "MPD connect failed");
                    } else {
                        debug!(error = %msg, "MPD connect failed again");
                    }
                    self.backoff.fail(now);
                    self.last_error = Some(msg.clone());
                    return Err(BackendError::Disconnected(msg));
                }
            }
        }
        Ok(self.conn.as_mut().expect("connected"))
    }

    /// Run `f` on a live connection. I/O errors drop the connection so the
    /// next call reconnects (after backoff); `ACK`s keep it.
    fn with_conn<T>(&mut self, f: impl FnOnce(&mut Connection) -> Result<T, ConnError>) -> Result<T, BackendError> {
        let conn = self.ensure_connected()?;
        match f(conn) {
            Ok(v) => Ok(v),
            Err(ConnError::Ack(m)) => Err(BackendError::Rejected(m)),
            Err(ConnError::Io(e)) => {
                warn!(error = %e, "MPD connection lost");
                self.conn = None;
                self.backoff.fail(Instant::now());
                self.last_error = Some(e.to_string());
                Err(BackendError::Disconnected(e.to_string()))
            }
            Err(ConnError::Protocol(m)) => {
                warn!(error = %m, "MPD protocol error; reconnecting");
                self.conn = None;
                Err(BackendError::Protocol(m))
            }
        }
    }

    fn raw_status(&mut self) -> Result<PlayerStatus, BackendError> {
        self.with_conn(|c| Ok(parse_status(&c.command("status")?)))
    }

    fn fetch_art(c: &mut Connection, cmd: &str, uri: &str) -> Result<Option<Vec<u8>>, ConnError> {
        let mut data = Vec::new();
        loop {
            let (pairs, chunk) = match c.command_binary(&format!("{cmd} {} {}", quote(uri), data.len())) {
                Ok(r) => r,
                Err(ConnError::Ack(_)) => return Ok(None), // no art of this kind
                Err(e) => return Err(e),
            };
            let size: usize = pairs.iter().find(|(k, _)| k == "size").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
            match chunk {
                Some(b) if !b.is_empty() => data.extend_from_slice(&b),
                _ => break,
            }
            if data.len() >= size {
                break;
            }
        }
        Ok((!data.is_empty()).then_some(data))
    }
}

impl MediaBackend for MpdBackend {
    fn name(&self) -> &str {
        "MPD"
    }

    fn connection(&self) -> BackendStatus {
        if self.conn.is_some() {
            BackendStatus::Connected
        } else if let Some(reason) = &self.last_error {
            BackendStatus::Disconnected { reason: reason.clone(), retry_in: self.backoff.retry_in(Instant::now()) }
        } else {
            BackendStatus::Connecting
        }
    }

    fn execute(&mut self, cmd: &Command) -> Result<(), BackendError> {
        let simple = |s: &str| -> Vec<String> { vec![s.to_string()] };
        let cmds: Vec<String> = match cmd {
            Command::Play | Command::Pause | Command::TogglePlayPause | Command::Prev | Command::ChangeVolume(_) => {
                let st = self.raw_status()?;
                match cmd {
                    Command::Play => play_cmd(&st),
                    Command::Pause => simple("pause 1"),
                    Command::TogglePlayPause => {
                        if st.state == PlaybackState::Playing {
                            simple("pause 1")
                        } else {
                            play_cmd(&st)
                        }
                    }
                    Command::Prev => {
                        if st.elapsed > Duration::from_secs(3) {
                            simple("seekcur 0")
                        } else {
                            simple("previous")
                        }
                    }
                    Command::ChangeVolume(d) => match st.volume {
                        Some(v) => vec![format!("setvol {}", apply_volume_delta(v, *d))],
                        None => return Err(BackendError::Rejected("Volume control unavailable".into())),
                    },
                    _ => unreachable!(),
                }
            }
            Command::Next => simple("next"),
            Command::Seek(d) => vec![format!("seekcur {:.1}", d.as_secs_f64())],
            Command::SeekRelative(s) => vec![format!("seekcur {s:+}")],
            Command::SetVolume(v) => vec![format!("setvol {}", (*v).min(100))],
            Command::PlayQueuePosition(p) => vec![format!("play {p}")],
            Command::LoadPlaylist(PlaylistSource::AllTracks) => {
                vec!["clear".into(), "add \"/\"".into(), "play 0".into()]
            }
            Command::LoadPlaylist(PlaylistSource::Named(n)) => {
                vec!["clear".into(), format!("load {}", quote(n)), "play 0".into()]
            }
            Command::RefreshLibrary => simple("update"),
        };
        debug!(?cmds, "mpd");
        self.with_conn(|c| {
            if cmds.len() == 1 {
                c.command(&cmds[0]).map(|_| ())
            } else {
                c.command_list(&cmds).map(|_| ())
            }
        })
    }

    fn status(&mut self) -> Result<PlayerStatus, BackendError> {
        self.with_conn(|c| {
            let mut st = parse_status(&c.command("status")?);
            st.current = parse_songs(&c.command("currentsong")?).into_iter().next();
            if st.duration.is_none() {
                st.duration = st.current.as_ref().and_then(|t| t.duration);
            }
            Ok(st)
        })
    }

    fn queue(&mut self) -> Result<Vec<Track>, BackendError> {
        self.with_conn(|c| Ok(parse_songs(&c.command("playlistinfo")?)))
    }

    fn playlists(&mut self) -> Result<Vec<String>, BackendError> {
        self.with_conn(|c| Ok(parse_playlist_names(&c.command("listplaylists")?)))
    }

    fn album_art(&mut self, uri: &str) -> Result<Option<Vec<u8>>, BackendError> {
        self.with_conn(|c| {
            // Folder cover.{jpg,png} first, then a picture embedded in the file.
            if let Some(a) = Self::fetch_art(c, "albumart", uri)? {
                return Ok(Some(a));
            }
            Self::fetch_art(c, "readpicture", uri)
        })
    }
}

fn play_cmd(st: &PlayerStatus) -> Vec<String> {
    match st.state {
        PlaybackState::Paused => vec!["pause 0".into()],
        _ => vec!["play".into()],
    }
}
