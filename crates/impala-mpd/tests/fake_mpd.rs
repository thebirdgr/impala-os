//! Integration tests against an in-process fake MPD server that speaks
//! enough of the protocol to exercise the client, and can be killed and
//! restarted on the same port to test reconnects.

use impala_core::{BackendError, BackendStatus, Command, MediaBackend, PlaybackState, PlaylistSource};
use impala_mpd::{MpdBackend, MpdConfig};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Default)]
struct FakeState {
    state: &'static str,
    song: usize,
    elapsed: f64,
    volume: i32,
    queue: Vec<String>,
    version: u32,
    playlists: BTreeMap<String, Vec<String>>,
    art: Option<Vec<u8>>,
    log: Vec<String>,
}

fn library() -> Vec<String> {
    vec!["a/1.flac".into(), "a/2.flac".into(), "b/3.flac".into()]
}

impl FakeState {
    fn new() -> Self {
        let mut playlists = BTreeMap::new();
        playlists.insert("Favourites".to_string(), vec!["b/3.flac".to_string()]);
        Self { state: "stop", volume: 50, queue: library(), version: 1, playlists, ..Default::default() }
    }

    fn song_block(&self, i: usize) -> String {
        let f = &self.queue[i];
        format!("file: {f}\nTitle: Song {i}\nArtist: Tester\nAlbum: Fake\nTime: 100\nduration: 100.000\nPos: {i}\n")
    }

    /// Returns the reply body (without trailing OK) or Err(ack message).
    fn handle(&mut self, line: &str) -> Result<Vec<u8>, String> {
        self.log.push(line.to_string());
        let (verb, arg) = line.split_once(' ').unwrap_or((line, ""));
        let unq = arg.trim().trim_matches('"').to_string();
        let mut out = String::new();
        match verb {
            "status" => {
                out = format!(
                    "volume: {}\nplaylist: {}\nplaylistlength: {}\nstate: {}\n",
                    self.volume,
                    self.version,
                    self.queue.len(),
                    self.state
                );
                if self.state != "stop" {
                    out += &format!("song: {}\nelapsed: {:.3}\nduration: 100.000\n", self.song, self.elapsed);
                }
            }
            "currentsong" => {
                if self.state != "stop" && self.song < self.queue.len() {
                    out = self.song_block(self.song);
                }
            }
            "playlistinfo" => out = (0..self.queue.len()).map(|i| self.song_block(i)).collect(),
            "listplaylists" => out = self.playlists.keys().map(|k| format!("playlist: {k}\n")).collect(),
            "play" => {
                if !unq.is_empty() {
                    let n: usize = unq.parse().map_err(|_| "bad pos".to_string())?;
                    if n >= self.queue.len() {
                        return Err("Bad song index".into());
                    }
                    self.song = n;
                    self.elapsed = 0.0;
                }
                if self.queue.is_empty() {
                    return Err("No songs".into());
                }
                self.state = "play";
            }
            "pause" => self.state = if unq == "1" { "pause" } else { "play" },
            "next" => {
                self.song = (self.song + 1).min(self.queue.len() - 1);
                self.elapsed = 0.0;
            }
            "previous" => self.song = self.song.saturating_sub(1),
            "seekcur" => {
                let v: f64 = unq.parse().map_err(|_| "bad seek".to_string())?;
                self.elapsed = if unq.starts_with('+') || unq.starts_with('-') { (self.elapsed + v).max(0.0) } else { v };
            }
            "setvol" => self.volume = unq.parse().map_err(|_| "bad vol".to_string())?,
            "clear" => {
                self.queue.clear();
                self.version += 1;
                self.state = "stop";
            }
            "add" => {
                self.queue.extend(library());
                self.version += 1;
            }
            "load" => {
                let p = self.playlists.get(&unq).ok_or_else(|| "No such playlist".to_string())?;
                self.queue.extend(p.clone());
                self.version += 1;
            }
            "update" => out = "updating_db: 1\n".into(),
            "albumart" => {
                let mut parts = arg.rsplitn(2, ' ');
                let offset: usize = parts.next().unwrap().parse().unwrap();
                let art = self.art.clone().ok_or_else(|| "No file exists".to_string())?;
                let end = (offset + 4096).min(art.len());
                let mut bytes = format!("size: {}\nbinary: {}\n", art.len(), end - offset).into_bytes();
                bytes.extend_from_slice(&art[offset..end]);
                bytes.push(b'\n');
                return Ok(bytes);
            }
            "readpicture" => {}
            _ => return Err(format!("unknown command \"{verb}\"")),
        }
        Ok(out.into_bytes())
    }
}

struct FakeMpd {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    conns: Arc<Mutex<Vec<TcpStream>>>,
    thread: Option<JoinHandle<()>>,
}

fn serve(stream: TcpStream, state: Arc<Mutex<FakeState>>) {
    let mut w = stream.try_clone().unwrap();
    let mut r = BufReader::new(stream);
    if w.write_all(b"OK MPD 0.24.0\n").is_err() {
        return;
    }
    let mut list: Option<Vec<String>> = None;
    let mut line = String::new();
    loop {
        line.clear();
        match r.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let l = line.trim_end().to_string();
        if l == "command_list_begin" {
            list = Some(Vec::new());
            continue;
        }
        let batch = if l == "command_list_end" {
            list.take().unwrap_or_default()
        } else if let Some(v) = list.as_mut() {
            v.push(l);
            continue;
        } else {
            vec![l]
        };
        let mut reply = Vec::new();
        let mut st = state.lock().unwrap();
        let mut failed = false;
        for (i, cmd) in batch.iter().enumerate() {
            match st.handle(cmd) {
                Ok(b) => reply.extend(b),
                Err(m) => {
                    reply.extend(format!("ACK [5@{i}] {{{cmd}}} {m}\n").into_bytes());
                    failed = true;
                    break;
                }
            }
        }
        drop(st);
        if !failed {
            reply.extend(b"OK\n");
        }
        if w.write_all(&reply).is_err() {
            return;
        }
    }
}

impl FakeMpd {
    fn start(addr: SocketAddr, state: Arc<Mutex<FakeState>>) -> Self {
        // Re-binding the same port right after a stop can briefly fail.
        let deadline = Instant::now() + Duration::from_secs(10);
        let listener = loop {
            match TcpListener::bind(addr) {
                Ok(l) => break l,
                Err(e) if Instant::now() < deadline => {
                    let _ = e;
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => panic!("bind {addr}: {e}"),
            }
        };
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let conns = Arc::new(Mutex::new(Vec::new()));
        let (stop2, conns2) = (stop.clone(), conns.clone());
        let thread = thread::spawn(move || {
            while !stop2.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((s, _)) => {
                        s.set_nonblocking(false).unwrap();
                        conns2.lock().unwrap().push(s.try_clone().unwrap());
                        let st = state.clone();
                        thread::spawn(move || serve(s, st));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Self { addr, stop, conns, thread: Some(thread) }
    }

    /// Simulate `systemctl stop mpd`: stop listening, drop every client.
    fn kill(mut self) -> SocketAddr {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
        for c in self.conns.lock().unwrap().drain(..) {
            let _ = c.shutdown(std::net::Shutdown::Both);
        }
        self.addr
    }
}

fn backend_for(addr: SocketAddr) -> MpdBackend {
    MpdBackend::new(MpdConfig {
        address: addr.to_string(),
        connect_timeout: Duration::from_millis(300),
        backoff_initial: Duration::from_millis(50),
        backoff_max: Duration::from_millis(400),
    })
}

fn fresh() -> (FakeMpd, Arc<Mutex<FakeState>>) {
    let state = Arc::new(Mutex::new(FakeState::new()));
    (FakeMpd::start("127.0.0.1:0".parse().unwrap(), state.clone()), state)
}

#[test]
fn commands_map_to_protocol() {
    let (srv, state) = fresh();
    let mut b = backend_for(srv.addr);

    b.execute(&Command::Play).unwrap();
    let s = b.status().unwrap();
    assert_eq!(s.state, PlaybackState::Playing);
    assert_eq!(s.current.as_ref().unwrap().title.as_deref(), Some("Song 0"));
    assert_eq!(s.duration, Some(Duration::from_secs(100)));

    b.execute(&Command::TogglePlayPause).unwrap();
    assert_eq!(b.status().unwrap().state, PlaybackState::Paused);
    b.execute(&Command::Play).unwrap();
    assert_eq!(state.lock().unwrap().log.last().unwrap(), "pause 0", "resume from pause");

    b.execute(&Command::Next).unwrap();
    assert_eq!(b.status().unwrap().queue_position, Some(1));

    b.execute(&Command::Seek(Duration::from_secs(42))).unwrap();
    b.execute(&Command::SeekRelative(-2)).unwrap();
    assert_eq!(b.status().unwrap().elapsed, Duration::from_secs(40));
    // Past 3 s, Prev restarts the track instead of going back.
    b.execute(&Command::Prev).unwrap();
    let s = b.status().unwrap();
    assert_eq!((s.queue_position, s.elapsed), (Some(1), Duration::ZERO));
    b.execute(&Command::Prev).unwrap();
    assert_eq!(b.status().unwrap().queue_position, Some(0));

    b.execute(&Command::SetVolume(80)).unwrap();
    b.execute(&Command::ChangeVolume(-30)).unwrap();
    assert_eq!(b.status().unwrap().volume, Some(50));

    b.execute(&Command::LoadPlaylist(PlaylistSource::Named("Favourites".into()))).unwrap();
    let q = b.queue().unwrap();
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].uri, "b/3.flac");
    assert_eq!(b.status().unwrap().state, PlaybackState::Playing);

    b.execute(&Command::LoadPlaylist(PlaylistSource::AllTracks)).unwrap();
    assert_eq!(b.queue().unwrap().len(), 3);
    assert_eq!(b.playlists().unwrap(), vec!["Favourites"]);

    b.execute(&Command::RefreshLibrary).unwrap();
    assert_eq!(state.lock().unwrap().log.last().unwrap(), "update");

    // ACKs are errors but keep the connection.
    let e = b.execute(&Command::LoadPlaylist(PlaylistSource::Named("Missing".into())));
    assert!(matches!(e, Err(BackendError::Rejected(ref m)) if m == "No such playlist"), "{e:?}");
    assert!(b.execute(&Command::PlayQueuePosition(99)).is_err());
    assert_eq!(b.connection(), BackendStatus::Connected);
    srv.kill();
}

#[test]
fn album_art_is_reassembled_from_chunks() {
    let (srv, state) = fresh();
    let art: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
    let mut b = backend_for(srv.addr);
    assert_eq!(b.album_art("a/1.flac").unwrap(), None, "no art → None, not an error");
    state.lock().unwrap().art = Some(art.clone());
    assert_eq!(b.album_art("a/1.flac").unwrap(), Some(art));
    srv.kill();
}

#[test]
fn mpd_down_at_startup_is_not_fatal() {
    // Grab a free port, then close it so nothing listens there.
    let addr = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    // Long backoff so the "fail fast" loop below stays inside one window.
    let mut b = MpdBackend::new(MpdConfig {
        address: addr.to_string(),
        connect_timeout: Duration::from_millis(300),
        backoff_initial: Duration::from_secs(1),
        backoff_max: Duration::from_secs(1),
    });
    assert_eq!(b.connection(), BackendStatus::Connecting);
    assert!(matches!(b.status(), Err(BackendError::Disconnected(_))));
    assert!(matches!(b.connection(), BackendStatus::Disconnected { .. }));
    // Within the backoff window calls fail fast without touching the network.
    let t = Instant::now();
    for _ in 0..100 {
        assert!(b.execute(&Command::Play).is_err());
    }
    assert!(t.elapsed() < Duration::from_millis(500));

    // MPD comes up later: we connect on our own.
    let state = Arc::new(Mutex::new(FakeState::new()));
    let srv = FakeMpd::start(addr, state);
    let deadline = Instant::now() + Duration::from_secs(5);
    while b.status().is_err() {
        assert!(Instant::now() < deadline, "never connected");
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(b.connection(), BackendStatus::Connected);
    srv.kill();
}

#[test]
fn reconnects_after_mpd_restart() {
    let (srv, state) = fresh();
    let mut b = backend_for(srv.addr);
    b.execute(&Command::Play).unwrap();
    assert!(b.status().is_ok());

    let addr = srv.kill(); // "systemctl restart mpd" (part 1)
    let e = b.status();
    assert!(matches!(e, Err(BackendError::Disconnected(_))), "{e:?}");
    assert!(matches!(b.connection(), BackendStatus::Disconnected { .. }));

    let srv = FakeMpd::start(addr, state.clone()); // (part 2)
    let deadline = Instant::now() + Duration::from_secs(5);
    let s = loop {
        if let Ok(s) = b.status() {
            break s;
        }
        assert!(Instant::now() < deadline, "did not reconnect");
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(s.state, PlaybackState::Playing, "state survives (it lives in MPD)");
    b.execute(&Command::Next).unwrap();
    srv.kill();
}
