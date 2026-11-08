//! Glue between core state/commands and the generated Slint types.

use crate::{MainWindow, NetState, QueueItem, UiCommand};
use impala_core::{format_time, AppState, BackendStatus, Command, NetworkStatus, PlaybackState, PlaylistSource};
use slint::{Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};
use std::cell::RefCell;
use std::time::Duration;

/// Touch action → the shared command vocabulary. `None` = ignore.
pub fn map_ui_command(c: UiCommand, n: i32, s: &str, volume_step: i8) -> Option<Command> {
    Some(match c {
        UiCommand::TogglePlay => Command::TogglePlayPause,
        UiCommand::Next => Command::Next,
        UiCommand::Prev => Command::Prev,
        UiCommand::VolUp => Command::ChangeVolume(volume_step),
        UiCommand::VolDown => Command::ChangeVolume(-volume_step),
        UiCommand::Seek => Command::Seek(Duration::from_secs(u64::try_from(n).ok()?)),
        UiCommand::PlayQueue => Command::PlayQueuePosition(usize::try_from(n).ok()?),
        UiCommand::LoadPlaylist if !s.is_empty() => Command::LoadPlaylist(PlaylistSource::Named(s.to_string())),
        UiCommand::LoadPlaylist => return None,
        UiCommand::LoadAll => Command::LoadPlaylist(PlaylistSource::AllTracks),
        UiCommand::RefreshLibrary => Command::RefreshLibrary,
    })
}

/// Decode cover bytes off the UI thread. Downscaled: the tile is ≤ 500 px.
pub fn decode_art(bytes: &[u8]) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let img = match image::load_from_memory(bytes) {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(error = %e, "could not decode album art");
            return None;
        }
    };
    let img = if img.width() > 512 || img.height() > 512 { img.thumbnail(512, 512) } else { img };
    let rgba = img.to_rgba8();
    Some(SharedPixelBuffer::clone_from_slice(rgba.as_raw(), rgba.width(), rgba.height()))
}

/// Up to two initials for the placeholder cover.
pub fn initials(s: &str) -> String {
    let i: String = s
        .split_whitespace()
        .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    if i.is_empty() {
        "♪".into()
    } else {
        i
    }
}

/// What to do with the cover on this update.
pub enum ArtUpdate {
    Keep,
    Clear,
    Set(SharedPixelBuffer<Rgba8Pixel>),
}

thread_local! {
    // Last state pushed to this UI thread, to avoid resetting list models
    // (and their scroll position) on every 500 ms tick.
    static LAST: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

pub fn apply(ui: &MainWindow, s: &AppState, art: ArtUpdate) {
    let p = &s.player;
    let connected = s.backend.is_connected();

    match &p.current {
        Some(t) => {
            ui.set_track_title(t.display_title().into());
            ui.set_artist(t.display_artist().into());
            ui.set_album(t.display_album().into());
            ui.set_art_initials(initials(t.album.as_deref().unwrap_or(&t.display_title())).into());
        }
        None if !connected => {
            ui.set_track_title("Music service unavailable".into());
            ui.set_artist("Reconnecting automatically…".into());
            ui.set_album("".into());
            ui.set_art_initials("♪".into());
        }
        None => {
            ui.set_track_title("Nothing playing".into());
            ui.set_artist("Tap Library to choose music".into());
            ui.set_album("".into());
            ui.set_art_initials("♪".into());
        }
    }
    match art {
        ArtUpdate::Keep => {}
        ArtUpdate::Clear => {
            ui.set_has_art(false);
            ui.set_art(Image::default());
        }
        ArtUpdate::Set(buf) => {
            ui.set_art(Image::from_rgba8(buf));
            ui.set_has_art(true);
        }
    }

    ui.set_playing(p.state == PlaybackState::Playing);
    ui.set_progress(p.progress());
    ui.set_elapsed_text(format_time(p.elapsed).into());
    ui.set_duration_text(p.duration.map(format_time).unwrap_or_default().into());
    ui.set_duration_secs(p.duration.map(|d| d.as_secs().min(i32::MAX as u64) as i32).unwrap_or(0));
    ui.set_volume(p.volume.map(i32::from).unwrap_or(-1));
    ui.set_backend_ok(connected);
    ui.set_status_text(s.status_line().into());
    ui.set_status_warn(
        matches!(s.backend, BackendStatus::Connecting) || s.last_error.is_some() || p.backend_error.is_some(),
    );
    ui.set_scanning(p.updating_library);
    ui.set_network(match s.network {
        NetworkStatus::Online => NetState::Online,
        NetworkStatus::Offline => NetState::Offline,
        NetworkStatus::Unknown => NetState::Unknown,
    });

    LAST.with(|last| {
        let mut last = last.borrow_mut();
        let prev = last.as_ref();
        if prev.is_none_or(|l| l.queue != s.queue || l.player.queue_position != p.queue_position) {
            let items: Vec<QueueItem> = s
                .queue
                .iter()
                .enumerate()
                .map(|(i, t)| QueueItem {
                    title: t.display_title().into(),
                    subtitle: [t.artist.as_deref(), t.album.as_deref()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" · ")
                        .into(),
                    duration: t.duration.map(format_time).unwrap_or_default().into(),
                    current: Some(i) == p.queue_position,
                })
                .collect();
            ui.set_queue(ModelRc::new(VecModel::from(items)));
        }
        if prev.is_none_or(|l| l.playlists != s.playlists) {
            let names: Vec<SharedString> = s.playlists.iter().map(SharedString::from).collect();
            ui.set_playlists(ModelRc::new(VecModel::from(names)));
        }
        *last = Some(s.clone());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_maps_to_core_commands() {
        assert_eq!(map_ui_command(UiCommand::TogglePlay, 0, "", 5), Some(Command::TogglePlayPause));
        assert_eq!(map_ui_command(UiCommand::VolUp, 0, "", 5), Some(Command::ChangeVolume(5)));
        assert_eq!(map_ui_command(UiCommand::VolDown, 0, "", 3), Some(Command::ChangeVolume(-3)));
        assert_eq!(map_ui_command(UiCommand::Seek, 61, "", 5), Some(Command::Seek(Duration::from_secs(61))));
        assert_eq!(map_ui_command(UiCommand::Seek, -1, "", 5), None);
        assert_eq!(map_ui_command(UiCommand::PlayQueue, 2, "", 5), Some(Command::PlayQueuePosition(2)));
        assert_eq!(
            map_ui_command(UiCommand::LoadPlaylist, 0, "Road Trip", 5),
            Some(Command::LoadPlaylist(PlaylistSource::Named("Road Trip".into())))
        );
        assert_eq!(map_ui_command(UiCommand::LoadPlaylist, 0, "", 5), None);
        assert_eq!(map_ui_command(UiCommand::LoadAll, 0, "", 5), Some(Command::LoadPlaylist(PlaylistSource::AllTracks)));
        assert_eq!(map_ui_command(UiCommand::RefreshLibrary, 0, "", 5), Some(Command::RefreshLibrary));
    }

    #[test]
    fn initials_are_short_and_safe() {
        assert_eq!(initials("Signal Path"), "SP");
        assert_eq!(initials("low frequencies of the night"), "LF");
        assert_eq!(initials("  "), "♪");
        assert_eq!(initials("(Live) 1999"), "L1");
    }

    #[test]
    fn decodes_png_and_rejects_garbage() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(4, 3, image::Rgba([1, 2, 3, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let buf = decode_art(&png).unwrap();
        assert_eq!((buf.width(), buf.height()), (4, 3));
        assert!(decode_art(b"not an image").is_none());
    }
}
