//! The player's Config (`~/.config/game37/config.toml`) and Game37's own XDG locations.
//!
//! The Config is written by hand. Its existence means setup is done (FR-CFG-1).

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub games_root: PathBuf,
}

#[derive(Deserialize)]
struct RawConfig {
    games_root: Option<String>,
}

#[derive(Debug)]
pub enum ConfigError {
    Read(PathBuf, std::io::Error),
    Parse(PathBuf, toml::de::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Read(path, e) => write!(f, "Can't read {}: {e}", path.display()),
            ConfigError::Parse(path, e) => write!(f, "{} is not valid: {}", path.display(), e.message()),
        }
    }
}

impl std::error::Error for ConfigError {}

/// `$XDG_CONFIG_HOME/game37/config.toml`.
pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// `$XDG_CONFIG_HOME/game37/`.
pub fn config_dir() -> PathBuf {
    dirs::config_dir().expect("no home directory").join("game37")
}

/// `$XDG_DATA_HOME/game37/`: Cores, the library database, cached Artwork (ADR 0005).
pub fn data_dir() -> PathBuf {
    dirs::data_dir().expect("no home directory").join("game37")
}

/// Loads the Config. `Ok(None)` means there is none, so First-run Setup is due.
pub fn load(path: &Path) -> Result<Option<Config>, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ConfigError::Read(path.to_owned(), e)),
    };
    let raw: RawConfig = toml::from_str(&text).map_err(|e| ConfigError::Parse(path.to_owned(), e))?;
    let games_root = expand_home(raw.games_root.as_deref().unwrap_or("~/Games"));
    Ok(Some(Config { games_root }))
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().expect("no home directory").join(rest),
        None if path == "~" => dirs::home_dir().expect("no home directory"),
        None => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &tempfile::TempDir, text: &str) -> PathBuf {
        let path = dir.path().join("config.toml");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn missing_config_means_setup_is_due() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(&dir.path().join("config.toml")).unwrap(), None);
    }

    #[test]
    fn games_root_expands_home() {
        let dir = tempfile::tempdir().unwrap();
        let config = load(&write(&dir, "games_root = \"~/Spiele\"\n"))
            .unwrap()
            .unwrap();
        assert_eq!(config.games_root, dirs::home_dir().unwrap().join("Spiele"));
    }

    #[test]
    fn games_root_defaults_to_home_games() {
        let dir = tempfile::tempdir().unwrap();
        let config = load(&write(&dir, "")).unwrap().unwrap();
        assert_eq!(config.games_root, dirs::home_dir().unwrap().join("Games"));
    }

    #[test]
    fn absolute_games_root_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let config = load(&write(&dir, "games_root = \"/mnt/games\"\n"))
            .unwrap()
            .unwrap();
        assert_eq!(config.games_root, PathBuf::from("/mnt/games"));
    }

    #[test]
    fn malformed_config_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = load(&write(&dir, "games_root = \n")).unwrap_err();
        assert!(matches!(err, ConfigError::Parse(..)), "{err}");
    }

    #[test]
    fn example_config_parses() {
        let text = include_str!("../config.example.toml");
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&write(&dir, text)).unwrap().is_some());
    }
}
