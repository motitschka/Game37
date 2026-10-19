//! What screens share: the library database and where the Games Root is.

use std::path::PathBuf;

use crate::library::db::{DbError, Library};
use crate::library::scan;
use crate::systems::SYSTEMS;
use crate::ui::input::{ButtonStyle, InputKind};

pub struct Ctx {
    pub library: Library,
    pub games_root: PathBuf,
    /// Bumped whenever the library's Games changed on disk, so screens know to reload them.
    pub revision: u64,
    /// What the player last used, so screens can show matching glyphs in their own text.
    pub input_kind: InputKind,
    pub button_style: ButtonStyle,
}

impl Ctx {
    pub fn new(library: Library, games_root: PathBuf) -> Ctx {
        Ctx {
            library,
            games_root,
            revision: 0,
            input_kind: InputKind::Controller,
            button_style: ButtonStyle::default(),
        }
    }

    /// An empty in-memory library, for screens shown without a Config, and for tests.
    pub fn empty() -> Ctx {
        let library = Library::open_in_memory().expect("an in-memory database always opens");
        Ctx::new(library, PathBuf::new())
    }

    /// Scans every System folder and brings the library up to date (FR-LIB-1).
    pub fn scan_all(&mut self, now: i64) -> Result<(), DbError> {
        let mut changed = false;
        for system in &SYSTEMS {
            let found = scan::scan_system(&self.games_root, system);
            changed |= self.library.sync_system(system.id, &found, now)?;
        }
        if changed {
            self.revision += 1;
        }
        Ok(())
    }
}

/// The current time as unix seconds.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
