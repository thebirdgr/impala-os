//! Runtime configuration (TOML). Never fatal: a missing or broken file
//! falls back to defaults with a logged warning.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub mpd: MpdSection,
    pub ui: UiSection,
    pub log: LogSection,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MpdSection {
    /// `host:port` of MPD. Local by design: playback must not need Wi-Fi.
    pub address: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct UiSection {
    /// `"mpd"` or `"mock"` (demo data, no daemon needed).
    pub backend: String,
    /// Volume change per touch tap, percent.
    pub volume_step: i8,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LogSection {
    /// tracing EnvFilter syntax, e.g. `info` or `info,impala_mpd=debug`.
    /// `RUST_LOG` overrides it.
    pub level: String,
}

impl Default for MpdSection {
    fn default() -> Self {
        Self { address: "localhost:6600".into() }
    }
}
impl Default for UiSection {
    fn default() -> Self {
        Self { backend: "mpd".into(), volume_step: 5 }
    }
}
impl Default for LogSection {
    fn default() -> Self {
        Self { level: "info".into() }
    }
}

/// Where we look, in order, when `--config` is not given.
pub fn search_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("IMPALA_CONFIG") {
        v.push(PathBuf::from(p));
    }
    v.push(PathBuf::from("/etc/impala-os/impala.toml"));
    if let Some(home) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| {
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(|h| PathBuf::from(h).join(".config"))
    }) {
        v.push(home.join("impala-os").join("impala.toml"));
    }
    v.push(PathBuf::from("config/impala.toml"));
    v
}

pub fn parse(text: &str) -> Result<Config, String> {
    toml::from_str(text).map_err(|e| e.to_string())
}

/// Returns the config plus human-readable notes to log once logging is up.
pub fn load(explicit: Option<&Path>) -> (Config, Vec<String>) {
    let mut notes = Vec::new();
    let candidates = match explicit {
        Some(p) => vec![p.to_path_buf()],
        None => search_paths(),
    };
    let mut cfg = None;
    for p in &candidates {
        match std::fs::read_to_string(p) {
            Ok(text) => {
                match parse(&text) {
                    Ok(c) => {
                        notes.push(format!("config loaded from {}", p.display()));
                        cfg = Some(c);
                    }
                    Err(e) => notes.push(format!("WARN ignoring invalid config {}: {e}", p.display())),
                }
                break;
            }
            Err(e) if explicit.is_some() => notes.push(format!("WARN cannot read {}: {e}", p.display())),
            Err(_) => {}
        }
    }
    let mut cfg = cfg.unwrap_or_else(|| {
        notes.push("no config file found, using defaults".into());
        Config::default()
    });
    if let Ok(addr) = std::env::var("IMPALA_MPD_ADDRESS") {
        notes.push(format!("IMPALA_MPD_ADDRESS overrides mpd.address = {addr}"));
        cfg.mpd.address = addr;
    }
    (cfg, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(parse("").unwrap(), Config::default());
    }

    #[test]
    fn partial_file_overrides_only_given_keys() {
        let c = parse("[mpd]\naddress = \"127.0.0.1:6601\"\n").unwrap();
        assert_eq!(c.mpd.address, "127.0.0.1:6601");
        assert_eq!(c.ui.backend, "mpd");
        assert_eq!(c.ui.volume_step, 5);
    }

    #[test]
    fn typos_are_rejected_not_silently_ignored() {
        assert!(parse("[mpd]\nadress = \"x\"\n").is_err());
        assert!(parse("[ui]\nvolume_step = \"big\"\n").is_err());
    }

    #[test]
    fn repo_config_file_parses() {
        let text = include_str!("../../../config/impala.toml");
        parse(text).expect("config/impala.toml must stay valid");
    }

    #[test]
    fn missing_explicit_file_falls_back() {
        let (c, notes) = load(Some(Path::new("/definitely/not/here.toml")));
        assert_eq!(c.ui, UiSection::default());
        assert!(notes.iter().any(|n| n.contains("cannot read")));
    }
}
