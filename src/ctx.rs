//! What screens share: the library database and where the Games Root is.

use std::path::PathBuf;

use crate::artwork::Artwork;
use crate::cores::{CoreInfo, Download};
use crate::emulators::Emulators;
use crate::library::db::{DbError, GameRow, Library};
use crate::library::scan;
use crate::session::input::PadState;
use crate::systems::SYSTEMS;
use crate::ui::input::{ButtonStyle, InputKind};

/// A request to play a Game of an Embedded System: from its Resume State, as every launch does
/// (FR-EMB-6), or from the Save State in `slot` (1 to 3) the player chose (FR-DET-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Play {
    pub game: GameRow,
    pub slot: Option<usize>,
}

impl Play {
    /// Plays `game` from its Resume State, if it has one.
    pub fn resume(game: GameRow) -> Play {
        Play { game, slot: None }
    }
}

/// A connected controller as screens see it: its name and this frame's buttons, sticks and
/// triggers (US-34). Plain data, so tests and screenshot mode can make one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Controller {
    pub name: String,
    pub pad: PadState,
}

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
    /// A Game's file or folder a screen wants shown in the file manager (US-16); `app.rs` takes it
    /// and carries it out, so no screen or test starts a process.
    pub show_file_request: Option<PathBuf>,
    /// A Game a screen asked to play; `app.rs` takes it and starts its Standalone Emulator.
    pub launch: Option<GameRow>,
    /// A Game of an Embedded System a screen asked to play; `app.rs` takes it and starts it on
    /// its Core.
    pub play: Option<Play>,
    /// Starts downloading a Core (FR-EMB-2): `Download::start` in production; tests and
    /// screenshot mode's offline cases swap it so they never hit the network.
    pub start_download: Box<dyn Fn(&'static CoreInfo, PathBuf) -> Download>,
    /// Starts downloading PPSSPP's assets zip: `Download::start_assets` in production; tests and
    /// screenshot mode's offline cases swap it so they never hit the network.
    pub start_assets_download: Box<dyn Fn(PathBuf) -> Download>,
    /// Games' covers and heroes (FR-ART-1–4). Off (placeholder art only) until `app.rs` starts it
    /// with the data dir and the API key; tests swap in one with a fake fetcher.
    pub artwork: Artwork,
    /// The connected controllers in connection order, as of this frame; `app.rs` fills it.
    pub controllers: Vec<Controller>,
    /// Which of `controllers` had a button pressed or a stick or trigger pushed last, if any.
    pub last_controller: Option<usize>,
    /// Milliseconds since SDL started, as of this frame: the clock screens time held buttons with.
    /// `app.rs` sets it; tests drive it by hand.
    pub ticks: u64,
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
            show_file_request: None,
            launch: None,
            play: None,
            start_download: Box::new(Download::start),
            start_assets_download: Box::new(Download::start_assets),
            artwork: Artwork::off(),
            controllers: Vec::new(),
            last_controller: None,
            ticks: 0,
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
