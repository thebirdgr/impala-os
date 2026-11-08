//! The single command vocabulary of Impala OS.
//!
//! Every input source (touchscreen, CAN steering-wheel buttons, voice, the
//! headless stdin console) is translated into a [`Command`] and sent to the
//! same service. Nothing downstream knows or cares where a command came from,
//! except for logging via [`CommandSource`].

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

/// Which playlist to load into the play queue.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlaylistSource {
    /// Every track in the library, in library order.
    AllTracks,
    /// A stored playlist by name (MPD `load <name>`).
    Named(String),
}

/// A playback/library command. Hardware independent.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Play,
    Pause,
    /// Play if paused/stopped, pause if playing. Mapped from a single
    /// touch button or steering-wheel "mode" key.
    TogglePlayPause,
    Next,
    Prev,
    /// Seek to an absolute position in the current track.
    Seek(Duration),
    /// Seek relative to the current position (negative = backwards).
    SeekRelative(i32),
    /// Absolute volume, 0..=100. Values above 100 are clamped.
    SetVolume(u8),
    /// Relative volume change (steering-wheel +/-).
    ChangeVolume(i8),
    /// Jump to an entry of the current play queue (0-based).
    PlayQueuePosition(usize),
    /// Replace the queue with a playlist and start playing it.
    LoadPlaylist(PlaylistSource),
    /// Rescan the music library (MPD `update`).
    RefreshLibrary,
}

/// Where a command came from. Used for logging/diagnostics only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSource {
    Touch,
    SteeringWheel,
    Voice,
    Console,
    Internal,
}

impl fmt::Display for CommandSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            CommandSource::Touch => "touch",
            CommandSource::SteeringWheel => "steering-wheel",
            CommandSource::Voice => "voice",
            CommandSource::Console => "console",
            CommandSource::Internal => "internal",
        };
        f.write_str(s)
    }
}

impl fmt::Display for Command {
    /// Formats in the same text syntax that [`Command::from_str`] accepts,
    /// so `cmd.to_string().parse() == Ok(cmd)` (round-trip, tested).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Command::Play => write!(f, "play"),
            Command::Pause => write!(f, "pause"),
            Command::TogglePlayPause => write!(f, "toggle"),
            Command::Next => write!(f, "next"),
            Command::Prev => write!(f, "prev"),
            Command::Seek(d) => write!(f, "seek {}", d.as_secs()),
            Command::SeekRelative(s) => write!(f, "seek {s:+}"),
            Command::SetVolume(v) => write!(f, "vol {v}"),
            Command::ChangeVolume(d) => write!(f, "vol {d:+}"),
            Command::PlayQueuePosition(p) => write!(f, "track {p}"),
            Command::LoadPlaylist(PlaylistSource::AllTracks) => write!(f, "playlist *"),
            Command::LoadPlaylist(PlaylistSource::Named(n)) => write!(f, "playlist {n}"),
            Command::RefreshLibrary => write!(f, "refresh"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseCommandError(pub String);

impl fmt::Display for ParseCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid command: {}", self.0)
    }
}

impl std::error::Error for ParseCommandError {}

impl FromStr for Command {
    type Err = ParseCommandError;

    /// Text syntax used by the headless console and (later) voice intents:
    /// `play | pause | toggle | next | prev | seek <s> | seek +/-<s> |
    ///  vol <0-100> | vol +/-<n> | track <n> | playlist <name|*> | refresh`
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let (verb, arg) = match s.split_once(char::is_whitespace) {
            Some((v, a)) => (v, a.trim()),
            None => (s, ""),
        };
        let err = || ParseCommandError(s.to_string());
        let need_arg = |a: &str| if a.is_empty() { Err(err()) } else { Ok(()) };
        let cmd = match verb.to_ascii_lowercase().as_str() {
            "play" => Command::Play,
            "pause" => Command::Pause,
            "toggle" | "playpause" => Command::TogglePlayPause,
            "next" => Command::Next,
            "prev" | "previous" => Command::Prev,
            "seek" => {
                need_arg(arg)?;
                if arg.starts_with('+') || arg.starts_with('-') {
                    Command::SeekRelative(arg.parse().map_err(|_| err())?)
                } else {
                    Command::Seek(Duration::from_secs(arg.parse().map_err(|_| err())?))
                }
            }
            "vol" | "volume" => {
                need_arg(arg)?;
                if arg.starts_with('+') || arg.starts_with('-') {
                    Command::ChangeVolume(arg.parse().map_err(|_| err())?)
                } else {
                    let v: u32 = arg.parse().map_err(|_| err())?;
                    Command::SetVolume(v.min(100) as u8)
                }
            }
            "track" => {
                need_arg(arg)?;
                Command::PlayQueuePosition(arg.parse().map_err(|_| err())?)
            }
            "playlist" | "load" => {
                need_arg(arg)?;
                if arg == "*" {
                    Command::LoadPlaylist(PlaylistSource::AllTracks)
                } else {
                    Command::LoadPlaylist(PlaylistSource::Named(arg.to_string()))
                }
            }
            "refresh" | "update" => Command::RefreshLibrary,
            _ => return Err(err()),
        };
        Ok(cmd)
    }
}

/// Clamp a volume to the valid range after applying a relative change.
pub fn apply_volume_delta(current: u8, delta: i8) -> u8 {
    (current as i16 + delta as i16).clamp(0, 100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_verbs() {
        assert_eq!("play".parse(), Ok(Command::Play));
        assert_eq!(" Pause ".parse(), Ok(Command::Pause));
        assert_eq!("toggle".parse(), Ok(Command::TogglePlayPause));
        assert_eq!("next".parse(), Ok(Command::Next));
        assert_eq!("previous".parse(), Ok(Command::Prev));
        assert_eq!("refresh".parse(), Ok(Command::RefreshLibrary));
    }

    #[test]
    fn parses_arguments() {
        assert_eq!("seek 90".parse(), Ok(Command::Seek(Duration::from_secs(90))));
        assert_eq!("seek -10".parse(), Ok(Command::SeekRelative(-10)));
        assert_eq!("seek +5".parse(), Ok(Command::SeekRelative(5)));
        assert_eq!("vol 40".parse(), Ok(Command::SetVolume(40)));
        assert_eq!("vol 250".parse(), Ok(Command::SetVolume(100)));
        assert_eq!("vol -5".parse(), Ok(Command::ChangeVolume(-5)));
        assert_eq!("track 2".parse(), Ok(Command::PlayQueuePosition(2)));
        assert_eq!(
            "playlist Road Trip".parse(),
            Ok(Command::LoadPlaylist(PlaylistSource::Named("Road Trip".into())))
        );
        assert_eq!("playlist *".parse(), Ok(Command::LoadPlaylist(PlaylistSource::AllTracks)));
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "fly", "seek", "seek abc", "vol", "vol loud", "track -1", "playlist"] {
            assert!(bad.parse::<Command>().is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn display_round_trips() {
        let cmds = [
            Command::Play,
            Command::Pause,
            Command::TogglePlayPause,
            Command::Next,
            Command::Prev,
            Command::Seek(Duration::from_secs(42)),
            Command::SeekRelative(-15),
            Command::SetVolume(70),
            Command::ChangeVolume(5),
            Command::PlayQueuePosition(3),
            Command::LoadPlaylist(PlaylistSource::AllTracks),
            Command::LoadPlaylist(PlaylistSource::Named("Favourites".into())),
            Command::RefreshLibrary,
        ];
        for c in cmds {
            assert_eq!(c.to_string().parse::<Command>(), Ok(c.clone()), "{c}");
        }
    }

    #[test]
    fn volume_delta_clamps() {
        assert_eq!(apply_volume_delta(50, 5), 55);
        assert_eq!(apply_volume_delta(98, 5), 100);
        assert_eq!(apply_volume_delta(3, -5), 0);
        assert_eq!(apply_volume_delta(100, 127), 100);
        assert_eq!(apply_volume_delta(0, -128), 0);
    }
}
