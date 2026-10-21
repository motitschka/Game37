//! The player's Config (`~/.config/game37/config.toml`), the secrets next to it, and Game37's own XDG
//! locations.
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
    /// `secrets.toml`'s SteamGridDB API key has characters no key has. The message never shows it.
    BadApiKey(PathBuf),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Read(path, e) => write!(f, "Can't read {}: {e}", path.display()),
            // `message()`, not the error itself: its `Display` quotes the offending line, which in
            // `secrets.toml` may hold the API key.
            ConfigError::Parse(path, e) => write!(f, "{} is not valid: {}", path.display(), e.message()),
            ConfigError::BadApiKey(path) => {
                write!(
                    f,
                    "{}: steamgriddb_api_key is not a SteamGridDB API key",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// `$XDG_CONFIG_HOME/game37/config.toml`.
pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// The SteamGridDB API key (FR-CFG-3). Its `Debug` never shows the key, so it can't end up in a log.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(key: &str) -> ApiKey {
        ApiKey(key.to_owned())
    }

    /// The key itself, for the `Authorization` header and nothing else.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(…)")
    }
}

#[derive(Deserialize)]
struct RawSecrets {
    steamgriddb_api_key: Option<String>,
}

/// `secrets.toml` next to the Config at `config_path`, wherever `--config` put it (FR-CFG-3).
pub fn secrets_path(config_path: &Path) -> PathBuf {
    config_path.with_file_name("secrets.toml")
}

/// Loads the SteamGridDB API key from `secrets.toml` next to the Config at `config_path`. `Ok(None)`
/// means there is no file or no key in it, so Artwork stays placeholder art (FR-ART-3).
pub fn load_api_key(config_path: &Path) -> Result<Option<ApiKey>, ConfigError> {
    let path = secrets_path(config_path);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ConfigError::Read(path, e)),
    };
    let raw: RawSecrets = toml::from_str(&text).map_err(|e| ConfigError::Parse(path.clone(), e))?;
    let key = raw.steamgriddb_api_key.unwrap_or_default();
    let key = key.trim();
    if key.is_empty() {
        return Ok(None);
    }
    // It goes into an HTTP header: refuse anything that isn't a plain token.
    if !key.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(ConfigError::BadApiKey(path));
    }
    Ok(Some(ApiKey::new(key)))
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
    // `notify` reports absolute paths, so a relative root (relative to the current directory,
    // not canonicalised: the folder may not exist yet) would never match its events.
    let games_root = if games_root.is_absolute() {
        games_root
    } else {
        std::path::absolute(&games_root).unwrap_or(games_root)
    };
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
    fn relative_games_root_is_made_absolute() {
        let dir = tempfile::tempdir().unwrap();
        let config = load(&write(&dir, "games_root = \"games\"\n")).unwrap().unwrap();
        assert!(config.games_root.is_absolute());
        assert_eq!(config.games_root.file_name().unwrap(), "games");
    }

    fn write_secrets(dir: &tempfile::TempDir, text: &str) -> PathBuf {
        std::fs::write(dir.path().join("secrets.toml"), text).unwrap();
        dir.path().join("config.toml")
    }

    #[test]
    fn api_key_is_read_from_secrets_next_to_the_config() {
        let dir = tempfile::tempdir().unwrap();
        let key = "0123456789abcdef0123456789abcdef";
        let config = write_secrets(&dir, &format!("steamgriddb_api_key = \"{key}\"\n"));
        assert_eq!(load_api_key(&config).unwrap(), Some(ApiKey::new(key)));
        assert_eq!(format!("{:?}", ApiKey::new(key)), "ApiKey(…)");
    }

    #[test]
    fn no_secrets_or_no_key_means_no_api_key() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_api_key(&dir.path().join("config.toml")).unwrap(), None);
        assert_eq!(load_api_key(&write_secrets(&dir, "")).unwrap(), None);
        assert_eq!(
            load_api_key(&write_secrets(&dir, "steamgriddb_api_key = \" \"\n")).unwrap(),
            None
        );
    }

    #[test]
    fn bad_secrets_are_errors_that_never_show_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let config = write_secrets(&dir, "steamgriddb_api_key = \"secretkey\" trailing\n");
        let message = load_api_key(&config).unwrap_err().to_string();
        assert!(message.contains("secrets.toml"), "{message}");
        assert!(!message.contains("secretkey"), "{message}");

        let config = write_secrets(&dir, "steamgriddb_api_key = \"secret key\"\n");
        let message = load_api_key(&config).unwrap_err().to_string();
        assert!(matches!(load_api_key(&config), Err(ConfigError::BadApiKey(_))));
        assert!(!message.contains("secret key"), "{message}");
    }

    #[test]
    fn example_config_parses() {
        let text = include_str!("../config.example.toml");
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&write(&dir, text)).unwrap().is_some());
    }
}
