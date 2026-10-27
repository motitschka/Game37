//! What screens share: the library database and where the Games Root is.

use std::path::{Path, PathBuf};

use crate::artwork::Artwork;
use crate::config::Config;
use crate::cores::{CoreInfo, Download};
use crate::emulators::{Emulator, Emulators};
use crate::library::db::{DbError, GameRow, Library};
use crate::library::scan;
use crate::screens::exe_browser::ExeBrowser;
use crate::session::input::PadState;
use crate::systems::SYSTEMS;
use crate::ui::gfx::WindowMode;
use crate::ui::input::{ButtonStyle, InputKind};
use crate::ui::nav::Screen;

/// A request to play a Game of an Embedded System from `start`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Play {
    pub game: GameRow,
    pub start: Start,
}

/// Where a Game starts. Its In-game Save is loaded either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// From its Resume State, as every launch does (FR-EMB-6); from power-on when it has none.
    Resume,
    /// From the Save State in slot `n` (1 to 3) the player chose (FR-DET-4).
    Slot(usize),
    /// From power-on, loading no state, as the player chose (FR-DET-4). The Resume State stays
    /// on disk until quitting overwrites it.
    PowerOn,
}

impl Start {
    /// The chosen Save State slot, if any.
    pub fn slot(self) -> Option<usize> {
        match self {
            Start::Slot(n) => Some(n),
            Start::Resume | Start::PowerOn => None,
        }
    }
}

impl Play {
    /// Plays `game` from its Resume State, if it has one.
    pub fn resume(game: GameRow) -> Play {
        Play {
            game,
            start: Start::Resume,
        }
    }
}

/// A connected controller as screens see it: its name and this frame's buttons, sticks and
/// triggers (US-34). Plain data, so tests and screenshot mode can make one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Controller {
    pub name: String,
    pub pad: PadState,
}

/// What Choose .exe does with the `.exe` the player picked.
pub type OnChoose = Box<dyn FnOnce(&mut Ctx, PathBuf)>;
/// See [`Ctx::choose_exe`].
pub type ChooseExe = dyn Fn(&'static Emulator, Option<PathBuf>, bool, OnChoose) -> Box<dyn Screen>;

pub struct Ctx {
    pub library: Library,
    pub games_root: PathBuf,
    /// Where the library database and the downloaded Cores live (`config::data_dir()` unless
    /// `--data-dir` overrides it).
    pub data_dir: PathBuf,
    /// The Config file in use (`config::config_path()` unless `--config` overrides it), for Settings
    /// to show and rewrite. Empty in tests.
    pub config_path: PathBuf,
    /// Bumped whenever the library's Games changed on disk, so screens know to reload them.
    pub revision: u64,
    /// Whether screens animate. Off in screenshot mode and tests, so they draw the settled state.
    pub animate: bool,
    /// What the player last used, so screens can show matching glyphs in their own text.
    pub input_kind: InputKind,
    /// The menus' button style; `app.rs` loads it from the library at start and hands it to the
    /// controls before every event, so a change applies from the next press on.
    pub button_style: ButtonStyle,
    /// Whether the window opens fullscreen; `app.rs` loads it from the library at start, and a
    /// change takes effect at the next start.
    pub window_mode: WindowMode,
    /// Which Standalone Emulators are installed, as last checked.
    pub emulators: Emulators,
    /// How Check again re-detects installed emulators (FR-CAR-9): `Emulators::detect` in
    /// production; screenshot mode and tests swap it so they never run `flatpak`.
    pub detect_emulators: Box<dyn Fn() -> Emulators>,
    /// Opens the `.exe` browser for Choose .exe (FR-WIN-4) for an emulator, from a start folder,
    /// in First-run Setup or not: [`ExeBrowser`] in production; tests swap in a screen that picks
    /// a file at once, so no test browses this computer's folders.
    pub choose_exe: Box<ChooseExe>,
    /// Text a screen wants put on the system clipboard (FR-CAR-9): the screen layer has no SDL
    /// handle, so it leaves the request here for the app's event loop to carry out.
    pub clipboard_request: Option<String>,
    /// A web page a screen wants opened in the browser (FR-WIN-5: an emulator's download page);
    /// `app.rs` takes it and opens it with SDL, so no screen or test opens one.
    pub open_url_request: Option<String>,
    /// A Game's file or folder a screen wants shown in the file manager (US-16); `app.rs` takes it
    /// and carries it out, so no screen or test starts a process.
    pub show_file_request: Option<PathBuf>,
    /// A Game a screen asked to play; `app.rs` takes it and starts its Standalone Emulator.
    pub launch: Option<GameRow>,
    /// A Game of an Embedded System a screen asked to play; `app.rs` takes it and starts it on
    /// its Core.
    pub play: Option<Play>,
    /// The Config First-run Setup's Finish wrote (FR-CFG-4); `app.rs` takes it and switches to Home
    /// as if Game37 had started with it.
    pub setup_finished: Option<Config>,
    /// Settings moved the Games Root to `games_root` and rescanned there; `app.rs` takes it and
    /// restarts the watcher on the new root.
    pub games_root_changed: bool,
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
    /// Whether `Action::Confirm` is down this frame, on a controller or the keyboard, for a screen
    /// that needs to tell a tap from a hold (e.g. `screens::confirm::Confirm`'s hold mode); unlike
    /// `Input::Action(Action::Confirm)`, which fires once on the press. `app.rs` sets it; tests and
    /// screenshot mode drive it by hand, when they drive it at all.
    pub confirm_held: bool,
}

impl Ctx {
    pub fn new(library: Library, games_root: PathBuf) -> Ctx {
        Ctx {
            library,
            games_root,
            data_dir: PathBuf::new(),
            config_path: PathBuf::new(),
            revision: 0,
            animate: false,
            input_kind: InputKind::Controller,
            button_style: ButtonStyle::default(),
            window_mode: WindowMode::default(),
            emulators: Emulators::all_installed(),
            // No Config yet: `app.rs` swaps in a detector that reads the Config it starts with.
            detect_emulators: Box::new(|| Emulators::detect(Path::new(""))),
            choose_exe: Box::new(|emulator, start, in_setup, on_choose| {
                let expected = emulator.exe_names[0];
                Box::new(ExeBrowser::new(
                    emulator.name,
                    expected,
                    start,
                    in_setup,
                    on_choose,
                ))
            }),
            clipboard_request: None,
            open_url_request: None,
            show_file_request: None,
            launch: None,
            play: None,
            setup_finished: None,
            games_root_changed: false,
            start_download: Box::new(Download::start),
            start_assets_download: Box::new(Download::start_assets),
            artwork: Artwork::off(),
            controllers: Vec::new(),
            last_controller: None,
            ticks: 0,
            confirm_held: false,
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

    /// Switches the menus' button style (US-32) and saves it. It applies from the next press, and
    /// glyphs follow from the next frame.
    pub fn set_button_style(&mut self, style: ButtonStyle) {
        self.button_style = style;
        if let Err(e) = self.library.set_button_style(style) {
            eprintln!("game37: can't save the button style: {e}");
        }
    }

    /// Switches the startup window mode (US-33) and saves it; it applies from the next start.
    pub fn set_window_mode(&mut self, mode: WindowMode) {
        self.window_mode = mode;
        if let Err(e) = self.library.set_window_mode(mode) {
            eprintln!("game37: can't save the window mode: {e}");
        }
    }

    /// Saves whether the window is fullscreen now, for Remember last's next start.
    pub fn remember_fullscreen(&self, fullscreen: bool) {
        if let Err(e) = self.library.set_last_fullscreen(fullscreen) {
            eprintln!("game37: can't save the window state: {e}");
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setters_switch_and_save() {
        let mut ctx = Ctx::empty();
        ctx.set_button_style(ButtonStyle::PlayStation);
        assert_eq!(ctx.button_style, ButtonStyle::PlayStation);
        assert_eq!(ctx.library.button_style().unwrap(), ButtonStyle::PlayStation);
        ctx.set_window_mode(WindowMode::AlwaysWindowed);
        assert_eq!(ctx.window_mode, WindowMode::AlwaysWindowed);
        assert_eq!(ctx.library.window_mode().unwrap(), WindowMode::AlwaysWindowed);
        ctx.remember_fullscreen(true);
        assert!(ctx.library.last_fullscreen().unwrap());
    }
}
