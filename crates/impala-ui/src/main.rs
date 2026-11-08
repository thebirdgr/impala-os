//! Impala OS head unit.
//!
//! ```text
//! impala [--config FILE] [--mock] [--mpd HOST:PORT]          interactive UI
//! impala --screenshot OUT.png [--size WxH] [--library] ...   offscreen PNG
//! impala --headless ...                                      stdin console, no display
//! ```

mod bridge;
mod config;
mod headless;
mod screenshot;

slint::include_modules!();

use bridge::ArtUpdate;
use impala_core::mock::MockBackend;
use impala_core::service::{self, ArtChange, ServiceConfig};
use impala_core::{net, CommandSource, MediaBackend};
use impala_mpd::{MpdBackend, MpdConfig};
use slint::ComponentHandle;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use tracing::{error, info, warn};

/// Exit code when no display could be opened (systemd may retry).
const EXIT_NO_DISPLAY: u8 = 3;

#[derive(Debug, Default)]
struct Args {
    config: Option<PathBuf>,
    mock: bool,
    mpd: Option<String>,
    screenshot: Option<PathBuf>,
    size: Option<(u32, u32)>,
    library: bool,
    headless: bool,
}

const USAGE: &str = "usage: impala [--config FILE] [--mock] [--mpd HOST:PORT] \
[--screenshot OUT.png [--size WxH] [--library]] [--headless]";

fn parse_args() -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--config" => a.config = Some(val("--config")?.into()),
            "--mock" => a.mock = true,
            "--mpd" => a.mpd = Some(val("--mpd")?),
            "--screenshot" => a.screenshot = Some(val("--screenshot")?.into()),
            "--size" => {
                let v = val("--size")?;
                let (w, h) = v.split_once('x').ok_or("--size wants WxH")?;
                let (w, h) = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
                if w == 0 || h == 0 || w > 8192 || h > 8192 {
                    return Err("--size out of range".into());
                }
                a.size = Some((w, h));
            }
            "--library" => a.library = true,
            "--headless" => a.headless = true,
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    Ok(a)
}

fn init_logging(level: &str) {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(level))
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let tty = std::io::stderr().is_terminal();
    let b = tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).with_ansi(tty);
    if tty {
        b.init();
    } else {
        // Under systemd stderr goes to the journal, which timestamps lines.
        b.without_time().init();
    }
}

fn make_backend(args: &Args, cfg: &config::Config) -> Box<dyn MediaBackend> {
    if args.mock || cfg.ui.backend == "mock" {
        info!("using demo backend (mock data)");
        Box::new(MockBackend::demo())
    } else {
        if cfg.ui.backend != "mpd" {
            warn!(backend = %cfg.ui.backend, "unknown ui.backend, using mpd");
        }
        let address = args.mpd.clone().unwrap_or_else(|| cfg.mpd.address.clone());
        info!(%address, "using MPD backend");
        Box::new(MpdBackend::new(MpdConfig { address, ..Default::default() }))
    }
}

fn art_update(change: &ArtChange) -> ArtUpdate {
    match change {
        ArtChange::Unchanged => ArtUpdate::Keep,
        ArtChange::Cleared => ArtUpdate::Clear,
        ArtChange::New(bytes) => bridge::decode_art(bytes).map(ArtUpdate::Set).unwrap_or(ArtUpdate::Clear),
    }
}

fn run_screenshot(out: PathBuf, args: &Args, mut backend: Box<dyn MediaBackend>) -> ExitCode {
    let (w, h) = args.size.unwrap_or((1424, 280));
    // MPD: allow a moment for the first connect.
    let mut snap = service::snapshot(backend.as_mut(), net::probe());
    for _ in 0..10 {
        if snap.state.backend.is_connected() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        snap = service::snapshot(backend.as_mut(), net::probe());
    }
    if !snap.state.backend.is_connected() {
        warn!(status = ?snap.state.backend, "backend not connected; screenshot shows the offline state");
    }
    let art = art_update(&snap.art);
    let library = args.library;
    match screenshot::render(&out, w, h, move |ui| {
        bridge::apply(ui, &snap.state, art);
        ui.set_library_open(library);
    }) {
        Ok(()) => {
            info!(path = %out.display(), width = w, height = h, "screenshot written");
            ExitCode::SUCCESS
        }
        Err(e) => {
            error!("screenshot failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_ui(cfg: &config::Config, backend: Box<dyn MediaBackend>) -> ExitCode {
    let ui = match MainWindow::new() {
        Ok(ui) => ui,
        Err(e) => {
            error!(error = %e, "no display available; the UI cannot start (use --headless or --screenshot)");
            return ExitCode::from(EXIT_NO_DISPLAY);
        }
    };

    let weak = ui.as_weak();
    let svc = service::spawn(
        backend,
        Box::new(net::probe),
        Box::new(move |u| {
            let art = art_update(&u.art); // decode on the player thread
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    bridge::apply(&ui, &u.state, art);
                }
            });
        }),
        ServiceConfig::default(),
    );

    let sender = svc.sender();
    let step = cfg.ui.volume_step.clamp(1, 50);
    ui.on_command(move |c, n, s| match bridge::map_ui_command(c, n, &s, step) {
        Some(cmd) => {
            sender.send(CommandSource::Touch, cmd);
        }
        None => warn!(?c, n, %s, "ignored touch command"),
    });

    let result = ui.run();
    svc.shutdown();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(error = %e, "display backend failed");
            ExitCode::from(EXIT_NO_DISPLAY)
        }
    }
}

#[cfg(test)]
mod touch_tests {
    //! Real hit-testing: synthetic pointer clicks on the actual 1424x280
    //! layout, through the same `on_command` → `map_ui_command` path the
    //! app uses. One test function because the Slint platform is global.
    use super::*;
    use impala_core::{Command, PlaylistSource};
    use slint::platform::{PointerEventButton, WindowEvent};
    use slint::{LogicalPosition, PhysicalSize};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn touch_targets_emit_shared_commands() {
        let window = screenshot::offscreen_platform().unwrap();
        let ui = MainWindow::new().unwrap();
        window.set_size(PhysicalSize::new(1424, 280));
        ui.set_backend_ok(true);
        ui.set_volume(50);
        ui.set_duration_secs(200);
        ui.show().unwrap();

        let got: Rc<RefCell<Vec<Option<Command>>>> = Rc::default();
        let sink = got.clone();
        ui.on_command(move |c, n, s| sink.borrow_mut().push(bridge::map_ui_command(c, n, &s, 5)));

        let tap = |x: f32, y: f32| -> Option<Command> {
            let position = LogicalPosition::new(x, y);
            let button = PointerEventButton::Left;
            window.dispatch_event(WindowEvent::PointerMoved { position });
            window.dispatch_event(WindowEvent::PointerPressed { position, button });
            window.dispatch_event(WindowEvent::PointerReleased { position, button });
            window.dispatch_event(WindowEvent::PointerExited);
            got.borrow_mut().pop().flatten()
        };

        // Coordinates from docs/screenshots/now-playing-1424x280.png.
        assert_eq!(tap(868.0, 140.0), Some(Command::Prev));
        assert_eq!(tap(998.0, 140.0), Some(Command::TogglePlayPause));
        assert_eq!(tap(1128.0, 140.0), Some(Command::Next));
        assert_eq!(tap(1246.0, 66.0), Some(Command::ChangeVolume(5)));
        assert_eq!(tap(1246.0, 214.0), Some(Command::ChangeVolume(-5)));
        // Middle of the progress bar → seek to ~half of 200 s.
        match tap(540.0, 212.0) {
            Some(Command::Seek(d)) => assert!((95..=105).contains(&d.as_secs()), "{d:?}"),
            other => panic!("expected seek, got {other:?}"),
        }
        // Dead zone (art tile) does nothing.
        assert_eq!(tap(140.0, 140.0), None);

        // Library: open, pick "All tracks", panel closes again.
        assert_eq!(tap(1360.0, 140.0), None);
        assert!(ui.get_library_open());
        assert_eq!(tap(90.0, 48.0), Some(Command::LoadPlaylist(PlaylistSource::AllTracks)));
        assert!(!ui.get_library_open());

        // Backend down: transport is disabled, taps are swallowed.
        ui.set_backend_ok(false);
        assert_eq!(tap(1128.0, 140.0), None);
    }
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let (cfg, notes) = config::load(args.config.as_deref());
    init_logging(&cfg.log.level);
    for n in notes {
        match n.strip_prefix("WARN ") {
            Some(w) => warn!("{w}"),
            None => info!("{n}"),
        }
    }
    info!(version = env!("CARGO_PKG_VERSION"), "Impala OS starting");

    let backend = make_backend(&args, &cfg);
    if let Some(out) = args.screenshot.clone() {
        return run_screenshot(out, &args, backend);
    }
    if args.headless {
        return headless::run(backend);
    }
    run_ui(&cfg, backend)
}
