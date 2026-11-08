//! The player service: one thread that owns the backend, executes commands
//! from every input source and publishes [`AppState`] snapshots.
//!
//! UI-toolkit independent; the Slint UI, the headless console and the future
//! CAN/voice adapters all talk to it through a [`ServiceHandle`].

use crate::backend::MediaBackend;
use crate::command::{Command, CommandSource};
use crate::state::{AppState, NetworkStatus, Track};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, PartialEq)]
pub enum ArtChange {
    Unchanged,
    /// No art for the current track (show placeholder).
    Cleared,
    /// Encoded image bytes (JPEG/PNG) for the current album.
    New(Arc<Vec<u8>>),
}

#[derive(Debug, Clone)]
pub struct StateUpdate {
    pub state: AppState,
    pub art: ArtChange,
}

pub struct ServiceConfig {
    pub poll_interval: Duration,
    pub network_interval: Duration,
    pub playlist_interval: Duration,
    /// How long a command error stays in the status line.
    pub error_display: Duration,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(500),
            network_interval: Duration::from_secs(2),
            playlist_interval: Duration::from_secs(30),
            error_display: Duration::from_secs(5),
        }
    }
}

enum Msg {
    Command(CommandSource, Command),
    Shutdown,
}

/// Cloneable sender for commands. Cheap; hand one to each input source.
#[derive(Clone)]
pub struct CommandSender(Sender<Msg>);

impl CommandSender {
    /// Returns `false` if the service has stopped.
    pub fn send(&self, source: CommandSource, cmd: Command) -> bool {
        self.0.send(Msg::Command(source, cmd)).is_ok()
    }
}

pub struct ServiceHandle {
    sender: CommandSender,
    join: Option<JoinHandle<()>>,
}

impl ServiceHandle {
    pub fn sender(&self) -> CommandSender {
        self.sender.clone()
    }

    pub fn send(&self, source: CommandSource, cmd: Command) -> bool {
        self.sender.send(source, cmd)
    }

    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let _ = self.sender.0.send(Msg::Shutdown);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

pub type NetworkProbe = Box<dyn FnMut() -> NetworkStatus + Send>;
pub type UpdateSink = Box<dyn FnMut(StateUpdate) + Send>;

/// Identifies "the same cover": album+artist, else the track's directory.
fn art_key(t: &Track) -> String {
    match (&t.album, &t.artist) {
        (Some(al), ar) => format!("{}\u{1f}{}", ar.as_deref().unwrap_or(""), al),
        (None, _) => t.uri.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default(),
    }
}

pub fn spawn(
    mut backend: Box<dyn MediaBackend>,
    mut network: NetworkProbe,
    mut on_update: UpdateSink,
    cfg: ServiceConfig,
) -> ServiceHandle {
    let (tx, rx) = mpsc::channel::<Msg>();
    let join = std::thread::Builder::new()
        .name("impala-player".into())
        .spawn(move || {
            let mut state = AppState { backend_name: backend.name().to_string(), ..Default::default() };
            let mut last_sent: Option<AppState> = None;
            let mut queue_version: Option<u32> = None;
            let mut playlists_due = Instant::now();
            let mut network_due = Instant::now();
            let mut error_until: Option<Instant> = None;
            let mut current_art_key: Option<String> = None;
            let mut was_connected = false;

            loop {
                let mut art = ArtChange::Unchanged;
                match rx.recv_timeout(cfg.poll_interval) {
                    Ok(Msg::Command(source, cmd)) => {
                        info!(%source, command = %cmd, "command");
                        match backend.execute(&cmd) {
                            Ok(()) => {
                                state.last_error = None;
                                if matches!(cmd, Command::RefreshLibrary | Command::LoadPlaylist(_)) {
                                    playlists_due = Instant::now();
                                }
                            }
                            Err(e) => {
                                warn!(command = %cmd, error = %e, "command failed");
                                state.last_error = Some(e.to_string());
                                error_until = Some(Instant::now() + cfg.error_display);
                            }
                        }
                    }
                    Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {}
                }

                let now = Instant::now();
                if error_until.is_some_and(|t| now >= t) {
                    state.last_error = None;
                    error_until = None;
                }
                if now >= network_due {
                    let n = network();
                    if n != state.network {
                        info!(network = ?n, "network status changed");
                    }
                    state.network = n;
                    network_due = now + cfg.network_interval;
                }

                match backend.status() {
                    Ok(status) => {
                        if !was_connected {
                            info!(backend = backend.name(), "backend connected");
                            was_connected = true;
                        }
                        if queue_version != Some(status.queue_version) {
                            match backend.queue() {
                                Ok(q) => {
                                    debug!(len = q.len(), "queue refreshed");
                                    state.queue = q;
                                    queue_version = Some(status.queue_version);
                                }
                                Err(e) => warn!(error = %e, "queue fetch failed"),
                            }
                        }
                        if now >= playlists_due {
                            match backend.playlists() {
                                Ok(p) => state.playlists = p,
                                Err(e) => warn!(error = %e, "playlist fetch failed"),
                            }
                            playlists_due = now + cfg.playlist_interval;
                        }
                        let key = status.current.as_ref().map(art_key);
                        if key != current_art_key {
                            art = match &status.current {
                                None => ArtChange::Cleared,
                                Some(t) => match backend.album_art(&t.uri) {
                                    Ok(Some(bytes)) => ArtChange::New(Arc::new(bytes)),
                                    Ok(None) => ArtChange::Cleared,
                                    Err(e) => {
                                        debug!(error = %e, "no album art");
                                        ArtChange::Cleared
                                    }
                                },
                            };
                            current_art_key = key;
                        }
                        state.player = status;
                    }
                    Err(e) => {
                        if was_connected {
                            warn!(error = %e, "backend lost");
                            was_connected = false;
                        }
                        // Refetch everything once it comes back.
                        queue_version = None;
                        playlists_due = now;
                    }
                }
                state.backend = backend.connection();

                if last_sent.as_ref() != Some(&state) || art != ArtChange::Unchanged {
                    on_update(StateUpdate { state: state.clone(), art });
                    last_sent = Some(state.clone());
                }
            }
            info!("player service stopped");
        })
        .expect("spawn player thread");

    ServiceHandle { sender: CommandSender(tx), join: Some(join) }
}

/// One synchronous poll of everything the UI shows. Used by `--screenshot`
/// (no service thread) and handy in tests.
pub fn snapshot(backend: &mut dyn MediaBackend, network: NetworkStatus) -> StateUpdate {
    let mut state = AppState { backend_name: backend.name().to_string(), network, ..Default::default() };
    let mut art = ArtChange::Cleared;
    if let Ok(status) = backend.status() {
        state.queue = backend.queue().unwrap_or_default();
        state.playlists = backend.playlists().unwrap_or_default();
        if let Some(t) = &status.current {
            if let Ok(Some(bytes)) = backend.album_art(&t.uri) {
                art = ArtChange::New(Arc::new(bytes));
            }
        }
        state.player = status;
    }
    state.backend = backend.connection();
    StateUpdate { state, art }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::BackendError;
    use crate::mock::MockBackend;
    use crate::state::{BackendStatus, PlaybackState, PlayerStatus};
    use std::sync::{Arc, Mutex};

    /// Lets the test flip the mock offline while the service owns it.
    struct Shared(Arc<Mutex<MockBackend>>);

    impl MediaBackend for Shared {
        fn name(&self) -> &str {
            "Demo"
        }
        fn connection(&self) -> BackendStatus {
            self.0.lock().unwrap().connection()
        }
        fn execute(&mut self, cmd: &Command) -> Result<(), BackendError> {
            self.0.lock().unwrap().execute(cmd)
        }
        fn status(&mut self) -> Result<PlayerStatus, BackendError> {
            self.0.lock().unwrap().status()
        }
        fn queue(&mut self) -> Result<Vec<Track>, BackendError> {
            self.0.lock().unwrap().queue()
        }
        fn playlists(&mut self) -> Result<Vec<String>, BackendError> {
            self.0.lock().unwrap().playlists()
        }
        fn album_art(&mut self, uri: &str) -> Result<Option<Vec<u8>>, BackendError> {
            self.0.lock().unwrap().album_art(uri)
        }
    }

    fn fast() -> ServiceConfig {
        ServiceConfig {
            poll_interval: Duration::from_millis(10),
            network_interval: Duration::from_millis(10),
            playlist_interval: Duration::from_secs(60),
            error_display: Duration::from_millis(200),
        }
    }

    fn wait_for(rx: &mpsc::Receiver<StateUpdate>, mut pred: impl FnMut(&AppState) -> bool) -> AppState {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(u) = rx.recv_timeout(Duration::from_millis(100)) {
                if pred(&u.state) {
                    return u.state;
                }
            }
        }
        panic!("condition not reached");
    }

    #[test]
    fn executes_commands_and_publishes_state() {
        let mock = Arc::new(Mutex::new(MockBackend::demo_manual_clock()));
        let (tx, rx) = mpsc::channel();
        let svc = spawn(
            Box::new(Shared(mock.clone())),
            Box::new(|| NetworkStatus::Offline),
            Box::new(move |u| {
                let _ = tx.send(u);
            }),
            fast(),
        );
        let s = wait_for(&rx, |s| s.backend.is_connected() && !s.queue.is_empty());
        assert_eq!(s.network, NetworkStatus::Offline);
        assert_eq!(s.playlists, vec!["Favourites", "Road Trip"]);

        svc.send(CommandSource::Touch, Command::Pause);
        wait_for(&rx, |s| s.player.state == PlaybackState::Paused);
        svc.send(CommandSource::SteeringWheel, Command::Next);
        wait_for(&rx, |s| s.player.queue_position == Some(2));
        svc.send(CommandSource::Console, Command::SetVolume(33));
        wait_for(&rx, |s| s.player.volume == Some(33));
        svc.shutdown();
    }

    #[test]
    fn survives_backend_outage_and_recovers() {
        let mock = Arc::new(Mutex::new(MockBackend::demo_manual_clock()));
        let (tx, rx) = mpsc::channel();
        let svc = spawn(
            Box::new(Shared(mock.clone())),
            Box::new(|| NetworkStatus::Unknown),
            Box::new(move |u| {
                let _ = tx.send(u);
            }),
            fast(),
        );
        wait_for(&rx, |s| s.backend.is_connected());
        mock.lock().unwrap().offline = true;
        wait_for(&rx, |s| matches!(s.backend, BackendStatus::Disconnected { .. }));
        // Commands while down produce a visible error, not a crash.
        svc.send(CommandSource::Touch, Command::Next);
        let s = wait_for(&rx, |s| s.last_error.is_some());
        assert!(s.last_error.unwrap().contains("not connected"));
        mock.lock().unwrap().offline = false;
        let s = wait_for(&rx, |s| s.backend.is_connected());
        assert!(!s.queue.is_empty());
        // Error message times out.
        wait_for(&rx, |s| s.last_error.is_none());
        svc.shutdown();
    }

    #[test]
    fn art_key_groups_by_album() {
        let a = Track { uri: "x/1.flac".into(), album: Some("A".into()), artist: Some("B".into()), ..Default::default() };
        let b = Track { uri: "y/2.flac".into(), ..a.clone() };
        assert_eq!(art_key(&a), art_key(&b));
        let c = Track { uri: "dir/3.flac".into(), ..Default::default() };
        assert_eq!(art_key(&c), "dir");
    }
}
