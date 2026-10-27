//! The player's Config (`~/.config/game37/config.toml`), the secrets next to it, and Game37's own XDG
//! locations.
//!
//! The Config is written by hand. Its existence means setup is done (FR-CFG-1).

use std::collections::HashMap;
use std::fmt;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml_edit::DocumentMut;

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
    /// A Config or secrets file couldn't be parsed. Carries only `toml`'s `message()`, never the error
    /// itself, whose `Debug` includes the raw input text (which in `secrets.toml` may hold the API key).
    Parse(PathBuf, String),
    /// `secrets.toml`'s SteamGridDB API key has characters no key has. The message never shows it.
    BadApiKey(PathBuf),
    /// Couldn't create, write or replace a Config or secrets file.
    Write(PathBuf, std::io::Error),
    /// A Config or secrets file couldn't be parsed for editing. Carries only `toml_edit`'s
    /// `message()`, never the error itself, whose `Display` quotes the offending line (which in
    /// `secrets.toml` may hold the API key).
    EditParse(PathBuf, String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Read(path, e) => write!(f, "Can't read {}: {e}", path.display()),
            ConfigError::Parse(path, message) => write!(f, "{} is not valid: {message}", path.display()),
            ConfigError::BadApiKey(path) => {
                write!(
                    f,
                    "{}: steamgriddb_api_key is not a SteamGridDB API key",
                    path.display()
                )
            }
            ConfigError::Write(path, e) => write!(f, "Can't write {}: {e}", path.display()),
            ConfigError::EditParse(path, message) => write!(f, "{} is not valid: {message}", path.display()),
        }
    }
}

impl std::error::Error for ConfigError {}

/// `parse_api_key`'s input isn't a SteamGridDB API key: only ASCII letters and digits.
#[derive(Debug)]
pub struct InvalidApiKey;

impl fmt::Display for InvalidApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a SteamGridDB API key")
    }
}

impl std::error::Error for InvalidApiKey {}

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
    let raw: RawSecrets =
        toml::from_str(&text).map_err(|e| ConfigError::Parse(path.clone(), e.message().to_owned()))?;
    parse_api_key(&raw.steamgriddb_api_key.unwrap_or_default()).map_err(|_| ConfigError::BadApiKey(path))
}

/// Checks a SteamGridDB API key the same way `load_api_key` does: trimmed, empty means none, only
/// ASCII letters and digits otherwise (it goes into an HTTP header, so nothing else is accepted).
pub fn parse_api_key(text: &str) -> Result<Option<ApiKey>, InvalidApiKey> {
    let key = text.trim();
    if key.is_empty() {
        return Ok(None);
    }
    if !key.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(InvalidApiKey);
    }
    Ok(Some(ApiKey::new(key)))
}

/// `$XDG_CONFIG_HOME/game37/`.
pub fn config_dir() -> PathBuf {
    dirs::config_dir().expect("no home directory").join("game37")
}

/// `$XDG_DATA_HOME/game37/`: Cores, the library database, cached Artwork (ADR 0005).
#[cfg(unix)]
pub fn data_dir() -> PathBuf {
    dirs::data_dir().expect("no home directory").join("game37")
}

/// `%LOCALAPPDATA%\game37\` on Windows (FR-WIN-2).
#[cfg(windows)]
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir().expect("no home directory").join("game37")
}

/// Loads the Config. `Ok(None)` means there is none, so First-run Setup is due.
pub fn load(path: &Path) -> Result<Option<Config>, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ConfigError::Read(path.to_owned(), e)),
    };
    let raw: RawConfig =
        toml::from_str(&text).map_err(|e| ConfigError::Parse(path.to_owned(), e.message().to_owned()))?;
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

/// `path` with a leading `~` standing for the home folder (on Windows the profile folder, and `~\`
/// works as well as `~/`); anything else as given.
pub fn expand_home(path: &str) -> PathBuf {
    let home = || dirs::home_dir().expect("no home directory");
    match path.strip_prefix('~') {
        Some("") => home(),
        Some(rest) if rest.starts_with(std::path::is_separator) => home().join(&rest[1..]),
        _ => PathBuf::from(path),
    }
}

/// Why a typed Games Root was refused: it isn't a full path.
pub const RELATIVE_GAMES_ROOT: &str = if cfg!(windows) {
    "Use a full path, starting with a drive letter or ~"
} else {
    "Use a full path, starting with / or ~"
};

/// The inverse of `expand_home`: a path under the home folder is written back as `~/…` (`~\…` on
/// Windows), so the Config stays portable across machines; anything else is written as given.
fn portable_games_root(path: &Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() {
            "~".to_owned()
        } else {
            format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display())
        };
    }
    path.display().to_string()
}

/// Writes `config.games_root` into the Config at `path`, keeping everything else in the file (other
/// keys, comments, formatting). Creates the file and its parent folder if either is missing. Keeps
/// the existing file's mode if it has one, or 0644 for a new file: unlike `save_api_key`, the Config
/// holds no secret, so it doesn't need to be mode 600.
pub fn save(path: &Path, config: &Config) -> Result<(), ConfigError> {
    let mut doc = read_document(path)?;
    doc["games_root"] = toml_edit::value(portable_games_root(&config.games_root));
    write_config(path, &doc)
}

/// The Config's `[emulators]` table: the `.exe` the player chose for each Standalone Emulator, by
/// its `emulators::Emulator::key` (FR-WIN-4). Only Windows reads it. No file, no table or a value
/// that isn't a string means nothing chosen there.
pub fn load_emulator_exes(path: &Path) -> Result<HashMap<String, PathBuf>, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(ConfigError::Read(path.to_owned(), e)),
    };
    let table: toml::Table =
        toml::from_str(&text).map_err(|e| ConfigError::Parse(path.to_owned(), e.message().to_owned()))?;
    let Some(toml::Value::Table(emulators)) = table.get("emulators") else {
        return Ok(HashMap::new());
    };
    Ok(emulators
        .iter()
        .filter_map(|(key, value)| Some((key.clone(), PathBuf::from(value.as_str()?))))
        .collect())
}

/// Sets (`Some`) or clears (`None`) the `.exe` chosen for the Standalone Emulator `key` in the
/// Config at `path`'s `[emulators]` table, keeping everything else in the file, like [`save`].
pub fn save_emulator_exe(path: &Path, key: &str, exe: Option<&Path>) -> Result<(), ConfigError> {
    let mut doc = read_document(path)?;
    match exe {
        Some(exe) => {
            let exe = exe.to_str().ok_or_else(|| {
                let e = std::io::Error::new(std::io::ErrorKind::InvalidData, "the path isn't valid Unicode");
                ConfigError::Write(path.to_owned(), e)
            })?;
            if !doc.get("emulators").is_some_and(toml_edit::Item::is_table_like) {
                doc["emulators"] = toml_edit::table();
            }
            doc["emulators"][key] = toml_edit::value(exe);
        }
        None => {
            if let Some(table) = doc
                .get_mut("emulators")
                .and_then(toml_edit::Item::as_table_like_mut)
            {
                table.remove(key);
            }
        }
    }
    write_config(path, &doc)
}

/// Writes the edited Config `doc` to `path`, keeping the existing file's mode or 0644 for a new
/// one (see [`save`]).
fn write_config(path: &Path, doc: &DocumentMut) -> Result<(), ConfigError> {
    #[cfg(unix)]
    let mode = std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o777)
        .unwrap_or(0o644);
    // Windows has no modes: the Config's folder in the profile is the player's own.
    #[cfg(windows)]
    let mode = 0;
    write_atomic(path, doc.to_string().as_bytes(), Some(mode))
}

/// Sets or removes the SteamGridDB API key in `secrets.toml` next to the Config at `config_path`.
/// `Some(key)` sets it, `None` removes it. Keeps everything else in the file, and leaves it mode 600
/// so the key never sits in a world-readable file, not even briefly (NFR-6).
pub fn save_api_key(config_path: &Path, key: Option<&ApiKey>) -> Result<(), ConfigError> {
    let path = secrets_path(config_path);
    let mut doc = read_document(&path)?;
    match key {
        Some(key) => doc["steamgriddb_api_key"] = toml_edit::value(key.expose()),
        None => {
            doc.remove("steamgriddb_api_key");
        }
    }
    write_atomic(&path, doc.to_string().as_bytes(), Some(0o600))
}

/// Reads `path` (an empty document if it doesn't exist yet) and parses it for editing.
fn read_document(path: &Path) -> Result<DocumentMut, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(ConfigError::Write(path.to_owned(), e)),
    };
    text.parse::<DocumentMut>()
        .map_err(|e| ConfigError::EditParse(path.to_owned(), e.message().to_owned()))
}

/// Writes `bytes` to `path`, replacing it atomically: a temp file in the same folder, persisted over
/// the target. Creates the parent folder first if needed. `mode`, if given, is set on the temp file
/// before it is persisted, so the final file is never briefly world-readable.
fn write_atomic(path: &Path, bytes: &[u8], mode: Option<u32>) -> Result<(), ConfigError> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| ConfigError::Write(path.to_owned(), e))?;
    #[cfg(windows)]
    let _ = mode;

    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| ConfigError::Write(path.to_owned(), e))?;
    tmp.write_all(bytes)
        .map_err(|e| ConfigError::Write(path.to_owned(), e))?;
    #[cfg(unix)]
    if let Some(mode) = mode {
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(mode))
            .map_err(|e| ConfigError::Write(path.to_owned(), e))?;
    }
    tmp.as_file()
        .sync_all()
        .map_err(|e| ConfigError::Write(path.to_owned(), e))?;
    tmp.persist(path)
        .map_err(|e| ConfigError::Write(path.to_owned(), e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A full path outside the home folder.
    const ELSEWHERE: &str = if cfg!(windows) { r"D:\Games" } else { "/mnt/games" };

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
    #[cfg(windows)] // `\` separates folders only on Windows.
    fn a_backslash_after_the_tilde_also_stands_for_the_profile_folder() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_home(r"~\Spiele\NES"), home.join("Spiele").join("NES"));
        assert_eq!(expand_home("~/Spiele"), home.join("Spiele"));
        assert_eq!(expand_home(r"~\"), home.join(""));
        assert_eq!(expand_home(r"~Spiele"), PathBuf::from("~Spiele"));
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
        let config = load(&write(&dir, &format!("games_root = {ELSEWHERE:?}\n")))
            .unwrap()
            .unwrap();
        assert_eq!(config.games_root, PathBuf::from(ELSEWHERE));
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
    fn malformed_secrets_never_show_the_key_in_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let config = write_secrets(&dir, "steamgriddb_api_key = \"abc123secret\n");
        let e = load_api_key(&config).unwrap_err();
        assert!(matches!(e, ConfigError::Parse(..)), "{e}");
        assert!(!e.to_string().contains("abc123secret"), "{e}");
        assert!(!format!("{e:?}").contains("abc123secret"), "{e:?}");
    }

    #[test]
    fn example_config_parses() {
        let text = include_str!("../config.example.toml");
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&write(&dir, text)).unwrap().is_some());
    }

    #[test]
    fn save_keeps_a_comment_and_an_unknown_key_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "# a note\ngames_root = \"/old\"\nunknown_key = 42\n");
        let config = Config {
            games_root: PathBuf::from(ELSEWHERE),
        };
        save(&path, &config).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# a note"), "{text}");
        assert!(text.contains("unknown_key = 42"), "{text}");
        assert_eq!(load(&path).unwrap().unwrap(), config);
    }

    #[test]
    fn save_writes_a_home_relative_root_as_tilde() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "");
        let config = Config {
            games_root: dirs::home_dir().unwrap().join("Spiele"),
        };
        save(&path, &config).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let raw: RawConfig = toml::from_str(&text).unwrap();
        let tilde = format!("~{}Spiele", std::path::MAIN_SEPARATOR);
        assert_eq!(raw.games_root, Some(tilde), "{text}");
        assert_eq!(load(&path).unwrap().unwrap(), config);
    }

    #[test]
    fn save_creates_a_missing_file_and_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/config.toml");
        let config = Config {
            games_root: PathBuf::from(ELSEWHERE),
        };
        save(&path, &config).unwrap();
        assert_eq!(load(&path).unwrap().unwrap(), config);
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    #[cfg(unix)] // File modes are Unix only.
    fn save_over_an_existing_0644_file_leaves_it_0644() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "games_root = \"/old\"\n");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let config = Config {
            games_root: PathBuf::from(ELSEWHERE),
        };
        save(&path, &config).unwrap();
        assert_eq!(mode(&path), 0o644);
    }

    #[test]
    #[cfg(unix)] // File modes are Unix only.
    fn save_keeps_a_config_s_own_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "games_root = \"/old\"\n");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let config = Config {
            games_root: PathBuf::from(ELSEWHERE),
        };
        save(&path, &config).unwrap();
        assert_eq!(mode(&path), 0o640);
    }

    #[test]
    #[cfg(unix)] // File modes are Unix only.
    fn save_to_a_new_file_comes_out_0644() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let config = Config {
            games_root: PathBuf::from(ELSEWHERE),
        };
        save(&path, &config).unwrap();
        assert_eq!(mode(&path), 0o644);
    }

    #[test]
    fn save_api_key_sets_replaces_and_removes_the_key_and_keeps_other_content() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        let secrets_path = secrets_path(&config_path);
        std::fs::write(&secrets_path, "# a note\nunknown_key = 42\n").unwrap();

        save_api_key(&config_path, Some(&ApiKey::new("firstkey111"))).unwrap();
        assert_eq!(
            load_api_key(&config_path).unwrap(),
            Some(ApiKey::new("firstkey111"))
        );
        #[cfg(unix)]
        assert_eq!(mode(&secrets_path), 0o600);

        save_api_key(&config_path, Some(&ApiKey::new("secondkey222"))).unwrap();
        assert_eq!(
            load_api_key(&config_path).unwrap(),
            Some(ApiKey::new("secondkey222"))
        );
        let text = std::fs::read_to_string(&secrets_path).unwrap();
        assert!(text.contains("# a note"), "{text}");
        assert!(text.contains("unknown_key = 42"), "{text}");
        #[cfg(unix)]
        assert_eq!(mode(&secrets_path), 0o600);

        save_api_key(&config_path, None).unwrap();
        assert_eq!(load_api_key(&config_path).unwrap(), None);
        let text = std::fs::read_to_string(&secrets_path).unwrap();
        assert!(text.contains("unknown_key = 42"), "{text}");
        #[cfg(unix)]
        assert_eq!(mode(&secrets_path), 0o600);
    }

    #[test]
    fn malformed_secrets_fails_save_api_key_without_showing_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        let secrets_path = secrets_path(&config_path);
        std::fs::write(
            &secrets_path,
            "steamgriddb_api_key = \"realsecretvalue\" trailing\n",
        )
        .unwrap();

        let err = save_api_key(&config_path, Some(&ApiKey::new("newkey123"))).unwrap_err();
        let message = err.to_string();
        assert!(matches!(err, ConfigError::EditParse(..)), "{message}");
        assert!(!message.contains("realsecretvalue"), "{message}");
    }

    #[test]
    fn save_emulator_exe_keeps_a_comment_an_unknown_key_and_games_root_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            &dir,
            "# a note\ngames_root = \"/old\"\nunknown_key = 42\n[emulators]\nunknown = \"z\"\n",
        );
        let exe = if cfg!(windows) {
            PathBuf::from(r"C:\Emulators\Cemu.exe")
        } else {
            PathBuf::from("/opt/Cemu/Cemu.exe")
        };
        save_emulator_exe(&path, "cemu", Some(&exe)).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# a note"), "{text}");
        assert!(text.contains("unknown_key = 42"), "{text}");
        assert!(text.contains("games_root"), "{text}");
        assert_eq!(load_emulator_exes(&path).unwrap().get("cemu"), Some(&exe));
        assert_eq!(
            load_emulator_exes(&path).unwrap().get("unknown"),
            Some(&PathBuf::from("z"))
        );
    }

    #[test]
    fn save_after_save_emulator_exe_keeps_the_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "[emulators]\ncemu = \"/old/Cemu.exe\"\n");
        let config = Config {
            games_root: PathBuf::from(ELSEWHERE),
        };
        save(&path, &config).unwrap();

        assert_eq!(
            load_emulator_exes(&path).unwrap().get("cemu"),
            Some(&PathBuf::from("/old/Cemu.exe"))
        );
        assert_eq!(load(&path).unwrap().unwrap(), config);
    }

    #[test]
    fn clearing_an_emulator_exe_removes_only_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            &dir,
            "[emulators]\ncemu = \"/a/Cemu.exe\"\npcsx2 = \"/b/pcsx2-qt.exe\"\n",
        );
        save_emulator_exe(&path, "cemu", None).unwrap();

        let exes = load_emulator_exes(&path).unwrap();
        assert_eq!(exes.get("cemu"), None);
        assert_eq!(exes.get("pcsx2"), Some(&PathBuf::from("/b/pcsx2-qt.exe")));
    }

    #[test]
    fn no_emulators_table_means_no_chosen_exes() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "games_root = \"/old\"\n");
        assert_eq!(load_emulator_exes(&path).unwrap(), HashMap::new());
    }

    #[test]
    fn parse_api_key_accepts_trims_and_refuses() {
        assert_eq!(parse_api_key("abc123").unwrap(), Some(ApiKey::new("abc123")));
        assert_eq!(parse_api_key("  abc123  ").unwrap(), Some(ApiKey::new("abc123")));
        assert_eq!(parse_api_key("").unwrap(), None);
        assert_eq!(parse_api_key("   ").unwrap(), None);
        assert!(parse_api_key("bad key!").is_err());
    }
}
