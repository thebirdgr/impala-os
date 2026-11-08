//! `--headless`: run the player service with no display and read commands
//! from stdin, one per line, in the [`impala_core::Command`] text syntax.
//! Used to exercise the full app→MPD path on the screenless Pi, and as the
//! stand-in for future non-touch inputs.
//!
//! Extra console words: `status` (print one STATUS line), `quit`.

use impala_core::service::{self, ServiceConfig, StateUpdate};
use impala_core::{format_time, net, AppState, Command, CommandSource, MediaBackend};
use std::io::BufRead;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

pub fn status_line(s: &AppState) -> String {
    let p = &s.player;
    let track = p.current.as_ref().map(|t| format!("{} – {}", t.display_artist(), t.display_title()));
    format!(
        "STATUS backend={:?} network={:?} state={:?} track={:?} pos={} elapsed={} duration={} volume={} queue={} playlists={:?} error={:?}",
        s.backend,
        s.network,
        p.state,
        track.unwrap_or_default(),
        p.queue_position.map(|q| q.to_string()).unwrap_or("-".into()),
        format_time(p.elapsed),
        p.duration.map(format_time).unwrap_or("-".into()),
        p.volume.map(|v| v.to_string()).unwrap_or("-".into()),
        s.queue.len(),
        s.playlists,
        s.last_error.as_ref().or(p.backend_error.as_ref()),
    )
}

/// Key fields whose change is worth a log line (not elapsed time).
fn summary(s: &AppState) -> (String, String, String, Option<usize>, Option<u8>, String) {
    (
        format!("{:?}", s.backend),
        format!("{:?}", s.network),
        format!("{:?}", s.player.state),
        s.player.queue_position,
        s.player.volume,
        s.player.current.as_ref().map(|t| t.uri.clone()).unwrap_or_default(),
    )
}

pub fn run(backend: Box<dyn MediaBackend>) -> std::process::ExitCode {
    let latest: Arc<Mutex<Option<AppState>>> = Arc::default();
    let latest2 = latest.clone();
    let mut last_summary = None;
    let svc = service::spawn(
        backend,
        Box::new(net::probe),
        Box::new(move |u: StateUpdate| {
            let sum = summary(&u.state);
            if last_summary.as_ref() != Some(&sum) {
                info!(
                    backend = %sum.0, network = %sum.1, state = %sum.2, pos = ?sum.3, volume = ?sum.4, uri = %sum.5,
                    "state changed"
                );
                last_summary = Some(sum);
            }
            *latest2.lock().unwrap() = Some(u.state);
        }),
        ServiceConfig::default(),
    );

    info!("headless console ready; commands: play pause toggle next prev seek vol track playlist refresh status quit");
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        match line {
            "" => continue,
            "quit" | "exit" => break,
            "status" => {
                // Let the service finish a poll after the last command.
                std::thread::sleep(std::time::Duration::from_millis(600));
                match latest.lock().unwrap().as_ref() {
                    Some(s) => println!("{}", status_line(s)),
                    None => println!("STATUS (no data yet)"),
                }
            }
            other => match other.parse::<Command>() {
                Ok(cmd) => {
                    svc.send(CommandSource::Console, cmd);
                }
                Err(e) => warn!("{e}"),
            },
        }
    }
    info!("stdin closed, shutting down");
    svc.shutdown();
    std::process::ExitCode::SUCCESS
}
