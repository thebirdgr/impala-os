//! Pure parsing/formatting helpers for the MPD text protocol
//! (<https://mpd.readthedocs.io/en/latest/protocol.html>). No I/O here.

use impala_core::{PlaybackState, PlayerStatus, Track};
use std::time::Duration;

pub type Pairs = Vec<(String, String)>;

/// Quote an argument: wrap in double quotes, escape `"` and `\`.
pub fn quote(arg: &str) -> String {
    let mut s = String::with_capacity(arg.len() + 2);
    s.push('"');
    for c in arg.chars() {
        if c == '"' || c == '\\' {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('"');
    s
}

/// Split a response line `key: value`.
pub fn split_pair(line: &str) -> Option<(String, String)> {
    let (k, v) = line.split_once(": ")?;
    Some((k.to_string(), v.to_string()))
}

/// `ACK [50@0] {play} No such song` → `No such song`
pub fn ack_message(line: &str) -> String {
    line.rsplit_once("} ").map(|(_, m)| m.to_string()).unwrap_or_else(|| line.to_string())
}

fn secs_f(v: &str) -> Option<Duration> {
    v.parse::<f64>().ok().filter(|f| f.is_finite() && *f >= 0.0).map(Duration::from_secs_f64)
}

fn get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
}

/// Parse the response of `status` (current song filled in separately).
pub fn parse_status(pairs: &[(String, String)]) -> PlayerStatus {
    let state = match get(pairs, "state") {
        Some("play") => PlaybackState::Playing,
        Some("pause") => PlaybackState::Paused,
        _ => PlaybackState::Stopped,
    };
    let volume = get(pairs, "volume").and_then(|v| v.parse::<i32>().ok()).filter(|v| *v >= 0).map(|v| v.min(100) as u8);
    // `elapsed`/`duration` are precise; fall back to legacy `time: e:d`.
    let legacy = get(pairs, "time").and_then(|t| t.split_once(':'));
    let elapsed = get(pairs, "elapsed")
        .and_then(secs_f)
        .or_else(|| legacy.and_then(|(e, _)| secs_f(e)))
        .unwrap_or_default();
    let duration = get(pairs, "duration")
        .and_then(secs_f)
        .or_else(|| legacy.and_then(|(_, d)| secs_f(d)))
        .filter(|d| !d.is_zero());
    PlayerStatus {
        state,
        volume,
        elapsed,
        duration,
        queue_position: get(pairs, "song").and_then(|v| v.parse().ok()),
        queue_length: get(pairs, "playlistlength").and_then(|v| v.parse().ok()).unwrap_or(0),
        queue_version: get(pairs, "playlist").and_then(|v| v.parse().ok()).unwrap_or(0),
        current: None,
        updating_library: get(pairs, "updating_db").is_some(),
        backend_error: get(pairs, "error").map(str::to_string),
    }
}

/// Parse a list of songs (`playlistinfo`, `currentsong`, `listallinfo`).
/// A new song starts at every `file:` key; `directory:`/`playlist:` entries
/// end the current song and are skipped.
pub fn parse_songs(pairs: &[(String, String)]) -> Vec<Track> {
    let mut out = Vec::new();
    let mut cur: Option<Track> = None;
    for (k, v) in pairs {
        match k.as_str() {
            "file" => {
                if let Some(t) = cur.take() {
                    out.push(t);
                }
                cur = Some(Track { uri: v.clone(), ..Default::default() });
            }
            "directory" | "playlist" => {
                if let Some(t) = cur.take() {
                    out.push(t);
                }
            }
            _ => {
                if let Some(t) = cur.as_mut() {
                    match k.as_str() {
                        "Title" => t.title = Some(v.clone()),
                        "Artist" => t.artist = Some(v.clone()),
                        "Album" => t.album = Some(v.clone()),
                        "duration" => t.duration = secs_f(v).or(t.duration),
                        "Time" if t.duration.is_none() => t.duration = secs_f(v),
                        _ => {}
                    }
                }
            }
        }
    }
    if let Some(t) = cur {
        out.push(t);
    }
    out
}

/// `listplaylists` → names.
pub fn parse_playlist_names(pairs: &[(String, String)]) -> Vec<String> {
    let mut v: Vec<String> = pairs.iter().filter(|(k, _)| k == "playlist").map(|(_, v)| v.clone()).collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(text: &str) -> Pairs {
        text.lines().filter_map(split_pair).collect()
    }

    #[test]
    fn quotes_arguments() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote(r#"a "b" \c"#), r#""a \"b\" \\c""#);
    }

    #[test]
    fn parses_ack() {
        assert_eq!(ack_message("ACK [50@0] {play} No such song"), "No such song");
        assert_eq!(ack_message("ACK weird"), "ACK weird");
    }

    #[test]
    fn parses_status_playing() {
        let s = parse_status(&pairs(
            "volume: 55\nrepeat: 0\nplaylist: 7\nplaylistlength: 3\nstate: play\nsong: 1\n\
             time: 12:200\nelapsed: 12.345\nduration: 200.100\nbitrate: 900\n",
        ));
        assert_eq!(s.state, PlaybackState::Playing);
        assert_eq!(s.volume, Some(55));
        assert_eq!(s.queue_position, Some(1));
        assert_eq!(s.queue_length, 3);
        assert_eq!(s.queue_version, 7);
        assert_eq!(s.elapsed, Duration::from_millis(12_345));
        assert_eq!(s.duration, Some(Duration::from_millis(200_100)));
        assert!(!s.updating_library);
    }

    #[test]
    fn parses_status_without_mixer_and_with_error() {
        let s = parse_status(&pairs("volume: -1\nstate: stop\nupdating_db: 3\nerror: Failed to open \"x.flac\"\n"));
        assert_eq!(s.state, PlaybackState::Stopped);
        assert_eq!(s.volume, None);
        assert_eq!(s.duration, None);
        assert!(s.updating_library);
        assert_eq!(s.backend_error.as_deref(), Some("Failed to open \"x.flac\""));
    }

    #[test]
    fn legacy_time_field_fallback() {
        let s = parse_status(&pairs("state: pause\ntime: 30:120\n"));
        assert_eq!(s.state, PlaybackState::Paused);
        assert_eq!(s.elapsed, Duration::from_secs(30));
        assert_eq!(s.duration, Some(Duration::from_secs(120)));
    }

    #[test]
    fn parses_song_lists() {
        let songs = parse_songs(&pairs(
            "directory: Album\nfile: Album/01.flac\nTitle: One\nArtist: A\nAlbum: Al\nTime: 61\nduration: 60.5\nPos: 0\n\
             file: Album/02.flac\nTime: 90\nplaylist: x.m3u\ndirectory: Other\n",
        ));
        assert_eq!(songs.len(), 2);
        assert_eq!(songs[0].title.as_deref(), Some("One"));
        assert_eq!(songs[0].duration, Some(Duration::from_millis(60_500)));
        assert_eq!(songs[1].title, None);
        assert_eq!(songs[1].duration, Some(Duration::from_secs(90)));
    }

    #[test]
    fn parses_playlist_names_sorted() {
        let names = parse_playlist_names(&pairs("playlist: Road Trip\nLast-Modified: x\nplaylist: Favourites\n"));
        assert_eq!(names, vec!["Favourites", "Road Trip"]);
    }
}
