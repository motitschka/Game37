//! In-game Saves: the Core's battery-backed save memory, kept in
//! `<Games Root>/saves/<system>/<subfolder>/<game file stem>.srm` (FR-LIB-9) and always written to
//! disk (FR-EMB-7).

use std::fs;
use std::hash::{DefaultHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How often changed save memory is written while the game runs.
pub const WRITE_EVERY: Duration = Duration::from_secs(10);

/// Where the Core keeps its own saves for `system`: `<Games Root>/saves/<system>/`.
pub fn save_dir(games_root: &Path, system: &str) -> PathBuf {
    games_root.join("saves").join(system)
}

/// The folder in `system_dir` (`saves/<system>/` or `states/<system>/`) for the Game at
/// `rel_path`: the Game's own subfolder inside its System's folder, mirrored, so Games with the
/// same file stem in different subfolders keep separate files. Files are named by stem alone, so
/// a RetroArch `.srm` placed by hand still matches; two Games in one folder whose names differ
/// only in extension (`Super Mario World.sfc` and `.smc`) share them.
pub fn game_dir(system_dir: &Path, system: &str, rel_path: &Path) -> PathBuf {
    let folder = rel_path.parent().unwrap_or(Path::new(""));
    system_dir.join(folder.strip_prefix(system).unwrap_or(folder))
}

/// The In-game Save file of the Game at `rel_path`.
pub fn srm_path(games_root: &Path, system: &str, rel_path: &Path) -> PathBuf {
    let stem = rel_path.file_stem().unwrap_or(rel_path.as_os_str());
    let mut name = stem.to_owned();
    name.push(".srm");
    game_dir(&save_dir(games_root, system), system, rel_path).join(name)
}

/// Copies the save file at `path`, if there is one, into the Core's save memory. On a size
/// mismatch the overlap is copied and the mismatch logged. Returns whether a file was loaded.
pub fn load(path: &Path, ram: &mut [u8]) -> io::Result<bool> {
    let data = match fs::read(path) {
        Ok(data) => data,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if data.len() != ram.len() {
        eprintln!(
            "game37: {} is {} bytes but the Core's save memory is {}; loading the first {}",
            path.display(),
            data.len(),
            ram.len(),
            data.len().min(ram.len())
        );
    }
    let n = data.len().min(ram.len());
    ram[..n].copy_from_slice(&data[..n]);
    Ok(true)
}

/// Writes `data` to `path` through a temporary file and a rename, so a crash never leaves half a
/// save.
pub fn write(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)
}

fn hash(data: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    h.write(data);
    h.finish()
}

/// Remembers what was last written, so only changed save memory is written again.
#[derive(Debug)]
pub struct Tracker {
    written: u64,
    since_check: Duration,
}

impl Tracker {
    /// Starts from the save memory as loaded.
    pub fn new(data: &[u8]) -> Tracker {
        Tracker {
            written: hash(data),
            since_check: Duration::ZERO,
        }
    }

    /// Whether `data` should be written now, `elapsed` after the last call: every
    /// [`WRITE_EVERY`], when it changed since the last write.
    pub fn due(&mut self, elapsed: Duration, data: &[u8]) -> bool {
        self.since_check += elapsed;
        if self.since_check < WRITE_EVERY {
            return false;
        }
        self.since_check = Duration::ZERO;
        self.changed(data)
    }

    /// Whether `data` differs from what was last written.
    pub fn changed(&self, data: &[u8]) -> bool {
        hash(data) != self.written
    }

    pub fn wrote(&mut self, data: &[u8]) {
        self.written = hash(data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_save_file_is_named_after_the_game_file_in_its_systems_saves_folder() {
        let p = srm_path(
            Path::new("/g"),
            "snes",
            Path::new("snes/Super Metroid (Europe).sfc"),
        );
        assert_eq!(p, Path::new("/g/saves/snes/Super Metroid (Europe).srm"));
    }

    #[test]
    fn games_with_the_same_stem_in_different_subfolders_keep_separate_saves() {
        let top = srm_path(Path::new("/g"), "snes", Path::new("snes/Super Mario World.sfc"));
        let hack = srm_path(
            Path::new("/g"),
            "snes",
            Path::new("snes/hacks/Super Mario World.sfc"),
        );
        assert_eq!(top, Path::new("/g/saves/snes/Super Mario World.srm"));
        assert_eq!(hack, Path::new("/g/saves/snes/hacks/Super Mario World.srm"));
        let deep = srm_path(Path::new("/g"), "gb", Path::new("gb/a/b/Tetris.gb"));
        assert_eq!(deep, Path::new("/g/saves/gb/a/b/Tetris.srm"));
    }

    #[test]
    fn writing_a_save_creates_its_subfolders() {
        let dir = tempfile::tempdir().unwrap();
        let path = srm_path(dir.path(), "snes", Path::new("snes/hacks/Super Mario World.sfc"));
        write(&path, &[1]).unwrap();
        assert_eq!(
            fs::read(dir.path().join("saves/snes/hacks/Super Mario World.srm")).unwrap(),
            [1]
        );
    }

    #[test]
    fn a_save_round_trips_and_a_size_mismatch_loads_the_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saves/gb/Tetris.srm");
        let mut ram = [0u8; 4];
        assert!(!load(&path, &mut ram).unwrap(), "no file yet");
        write(&path, &[1, 2, 3, 4]).unwrap();
        assert!(!path.with_extension("srm.tmp").exists());
        assert!(load(&path, &mut ram).unwrap());
        assert_eq!(ram, [1, 2, 3, 4]);
        let mut bigger = [9u8; 6];
        load(&path, &mut bigger).unwrap();
        assert_eq!(bigger, [1, 2, 3, 4, 9, 9]);
        let mut smaller = [0u8; 2];
        load(&path, &mut smaller).unwrap();
        assert_eq!(smaller, [1, 2]);
    }

    #[test]
    fn changed_memory_is_written_every_ten_seconds() {
        let mut t = Tracker::new(&[0, 0]);
        assert!(!t.due(Duration::from_secs(11), &[0, 0]), "unchanged");
        assert!(!t.due(Duration::from_secs(5), &[1, 0]), "not yet");
        assert!(t.due(Duration::from_secs(5), &[1, 0]));
        t.wrote(&[1, 0]);
        assert!(!t.due(Duration::from_secs(10), &[1, 0]));
        assert!(t.changed(&[2, 0]));
    }
}
