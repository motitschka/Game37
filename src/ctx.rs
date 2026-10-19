//! What screens share: the library database and where the Games Root is.

use std::path::PathBuf;

use crate::cores::{CoreInfo, Download};
use crate::emulators::Emulators;
use crate::library::db::{DbError, GameRow, Library};
use crate::library::scan;
use crate::systems::SYSTEMS;
use crate::ui::input::{ButtonStyle, InputKind};

pub struct Ctx {
    pub library: Library,
    pub games_root: PathBuf,
    /// Where the library database and the downloaded Cores live (`config::data_dir()` unless
    /// `--data-dir` overrides it).
    pub data_dir: PathBuf,
    /// Bumped whenever the library's Games changed on disk, so screens know to reload them.
    pub revision: u64,
    /// Whether screens animate. Off in screenshot mode and tests, so they draw the settled state.
    pub animate: bool,
    /// What the player last used, so screens can show matching glyphs in their own text.
    pub input_kind: InputKind,
    pub button_style: ButtonStyle,
    /// Which Standalone Emulators are installed, as last checked.
    pub emulators: Emulators,
    /// How Check again re-detects installed emulators (FR-CAR-9): `Emulators::detect` in
    /// production; screenshot mode and tests swap it so they never run `flatpak`.
    pub detect_emulators: Box<dyn Fn() -> Emulators>,
    /// Text a screen wants put on the system clipboard (FR-CAR-9): the screen layer has no SDL
    /// handle, so it leaves the request here for the app's event loop to carry out.
    pub clipboard_request: Option<String>,
    /// A Game a screen asked to play; `app.rs` takes it and starts its Standalone Emulator.
    pub launch: Option<GameRow>,
    /// A Game of an Embedded System a screen asked to play; `app.rs` takes it and starts it on
    /// its Core.
    pub play: Option<GameRow>,
    /// Starts downloading a Core (FR-EMB-2): `Download::start` in production; tests and
    /// screenshot mode's offline cases swap it so they never hit the network.
    pub start_download: Box<dyn Fn(&'static CoreInfo, PathBuf) -> Download>,
}

impl Ctx {
    pub fn new(library: Library, games_root: PathBuf) -> Ctx {
        Ctx {
            library,
            games_root,
            data_dir: PathBuf::new(),
            revision: 0,
            animate: false,
            input_kind: InputKind::Controller,
            button_style: ButtonStyle::default(),
            emulators: Emulators::all_installed(),
            detect_emulators: Box::new(Emulators::detect),
            clipboard_request: None,
            launch: None,
            play: None,
            start_download: Box::new(Download::start),
        }
    }

    /// An empty in-memory library, for screens shown without a Config, and for tests.
    pub fn empty() -> Ctx {
        let library = Library::open_in_memory().expect("an in-memory database always opens");
        Ctx::new(library, PathBuf::new())
    }

    /// Fixes which emulators count as installed, for screenshot mode's `--missing-emulators`:
    /// Check again re-detects the same state rather than running `flatpak list`.
    pub fn simulate_emulators(&mut self, emulators: Emulators) {
        self.emulators = emulators.clone();
        self.detect_emulators = Box::new(move || emulators.clone());
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
