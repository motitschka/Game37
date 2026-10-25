//! Settings → Library → Games Root location (FR-SET-1): typing a new Games Root in a card placed
//! like the Key entry's. Saving makes the folders, rewrites the Config and moves the running app to
//! the new root, so its Games show without a restart.

use std::fs;
use std::io;
use std::path::Path;

use super::artwork::{CARD_H, CARD_W, CARD_X, CARD_Y};
use crate::config::{self, Config};
use crate::ctx::{self, Ctx};
use crate::screens::home::tilde;
use crate::screens::message::dim;
use crate::systems::SYSTEMS;
use crate::ui::Rect;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Input, InputKind};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::text_entry::{KEYBOARD_H, KEYBOARD_W, Outcome, TextEntry};
use crate::ui::theme::{DANGER, LINE, MUTED, SURFACE, TEXT};

/// As long as a path may be on Linux.
const MAX_PATH_CHARS: usize = 4096;
/// What a relative path gets, as in First-run Setup.
const RELATIVE: &str = "Use a full path, starting with / or ~";
const NOTE: &str = "Game37 looks for Games in one folder per System inside it.";

/// Typing a new Games Root, over Settings: the field starts with the current one, `~` standing for
/// the home folder.
pub struct GamesRootEntry {
    entry: TextEntry,
    /// Why the last save didn't happen.
    error: Option<String>,
    /// What the player last used, as of the last frame, for the hints.
    kind: InputKind,
}

impl GamesRootEntry {
    pub fn new(ctx: &Ctx) -> GamesRootEntry {
        let shown = tilde(&ctx.games_root, dirs::home_dir().as_deref());
        GamesRootEntry {
            entry: TextEntry::new(&shown, MAX_PATH_CHARS),
            error: None,
            kind: ctx.input_kind,
        }
    }

    /// Saves the typed Games Root: makes it, its System folders and `bios/`, rewrites the Config,
    /// and rescans there. A Game's identity is its path relative to the Games Root, so Games moved
    /// with the folder keep their Playtime, Favorites and renames, and Games only under the old root
    /// leave every view. The unchanged path closes doing nothing; an empty field, a relative path or
    /// a folder that can't be made stays open, saying why.
    fn save(&mut self, ctx: &mut Ctx) -> Transition {
        let typed = self.entry.text().trim();
        if typed.is_empty() {
            return Transition::None;
        }
        let root = config::expand_home(typed);
        if root.is_relative() {
            self.error = Some(RELATIVE.to_owned());
            return Transition::None;
        }
        if root == ctx.games_root {
            return Transition::Pop;
        }
        if let Err(e) = make_folders(&root) {
            self.error = Some(e);
            return Transition::None;
        }
        let config = Config {
            games_root: root.clone(),
        };
        if let Err(e) = config::save(&ctx.config_path, &config) {
            self.error = Some(e.to_string());
            return Transition::None;
        }
        ctx.games_root = root;
        if let Err(e) = ctx.scan_all(ctx::now()) {
            eprintln!("game37: {e}");
        }
        ctx.games_root_changed = true;
        ctx.revision += 1;
        Transition::Pop
    }
}

/// Makes `root`, then a folder for every System and `bios/` in it. Existing ones are fine.
fn make_folders(root: &Path) -> Result<(), String> {
    let folders = SYSTEMS.iter().map(|s| root.join(s.id)).chain([root.join("bios")]);
    for folder in std::iter::once(root.to_owned()).chain(folders) {
        if let Err(e) = fs::create_dir_all(&folder) {
            let shown = tilde(&folder, dirs::home_dir().as_deref());
            return Err(format!("Can't create {shown}: {}", reason(&e)));
        }
    }
    Ok(())
}

/// Why making a folder failed, without the "(os error n)" the system adds.
fn reason(e: &io::Error) -> String {
    let text = e.to_string();
    match text.find(" (os error") {
        Some(end) => text[..end].to_owned(),
        None => text,
    }
}

impl Screen for GamesRootEntry {
    fn title(&self) -> &str {
        "Games Root"
    }

    fn is_modal(&self) -> bool {
        true
    }

    fn text_input(&self) -> bool {
        true
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        match self.entry.handle(input) {
            Outcome::Submit => self.save(ctx),
            Outcome::Cancel => Transition::Pop,
            Outcome::Changed => {
                self.error = None;
                Transition::None
            }
            Outcome::None => Transition::None,
        }
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.kind = ctx.input_kind;
        dim(gfx);
        let card = Rect::new(CARD_X, CARD_Y, CARD_W, CARD_H);
        gfx.fill_round_rect(card, 20.0, SURFACE);
        gfx.stroke_round_rect(card, 20.0, 1.0, LINE);
        let x = CARD_X + (CARD_W - KEYBOARD_W) / 2.0;
        let heading = TextStyle::new(28, Weight::ExtraBold, TEXT);
        gfx.text(x, CARD_Y + 72.0, self.title(), heading, Align::Left);
        let field = Rect::new(x, CARD_Y + 136.0, KEYBOARD_W, 88.0);
        self.entry.draw_field(gfx, field, "");
        let (line, ink) = match &self.error {
            Some(error) => (error.as_str(), DANGER),
            None => (NOTE, MUTED),
        };
        gfx.text(
            x,
            CARD_Y + 256.0,
            line,
            TextStyle::new(16, Weight::Medium, ink),
            Align::Left,
        );
        let top = CARD_Y + CARD_H - 48.0 - KEYBOARD_H;
        self.entry.draw_keyboard(gfx, x, top);
    }

    fn hints(&self) -> Vec<Hint> {
        TextEntry::hints(self.kind, "Save")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::db::SortOrder;
    use crate::screens::settings::library::Library;
    use crate::screens::settings::{Section, Settings};
    use crate::ui::input::{Action, TextInput};
    use crate::ui::nav::{Hits, Nav};
    use std::path::PathBuf;

    /// A Ctx on the Games Root `old` in a temporary directory, with its Config and data folder
    /// there too. `old` and `new` both hold `nes/Game.nes`; `old` also has `nes/Left.nes` and `new`
    /// `nes/Found.nes`. Only `old` is scanned.
    fn world() -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        for (root, only) in [("old", "Left"), ("new", "Found")] {
            let nes = dir.path().join(root).join("nes");
            fs::create_dir_all(&nes).unwrap();
            fs::write(nes.join("Game.nes"), b"").unwrap();
            fs::write(nes.join(format!("{only}.nes")), b"").unwrap();
        }
        let mut ctx = Ctx::empty();
        ctx.games_root = dir.path().join("old");
        ctx.config_path = dir.path().join("config.toml");
        ctx.data_dir = dir.path().join("data");
        ctx.scan_all(0).unwrap();
        (dir, ctx)
    }

    fn titles(ctx: &Ctx) -> Vec<String> {
        let games = ctx.library.system_games("nes", SortOrder::AZ).unwrap();
        games.into_iter().map(|g| g.title).collect()
    }

    /// Clears the field, types `text` and saves.
    fn save(entry: &mut GamesRootEntry, ctx: &mut Ctx, text: &str) -> Transition {
        retype(entry, ctx, text);
        entry.handle(Input::Action(Action::Start), ctx)
    }

    fn retype(entry: &mut GamesRootEntry, ctx: &mut Ctx, text: &str) {
        for _ in 0..entry.entry.text().chars().count() {
            entry.handle(Input::Text(TextInput::Backspace), ctx);
        }
        for ch in text.chars() {
            entry.handle(Input::Text(TextInput::Char(ch)), ctx);
        }
    }

    fn path(p: &Path) -> String {
        p.to_str().unwrap().to_owned()
    }

    /// What can't be undone by a save that didn't happen: the Config, the Games Root, the library
    /// views and the watcher request.
    fn state(ctx: &Ctx) -> (bool, PathBuf, u64, Vec<String>, bool) {
        let config = ctx.config_path.exists();
        let root = ctx.games_root.clone();
        (config, root, ctx.revision, titles(ctx), ctx.games_root_changed)
    }

    #[test]
    fn confirm_on_the_games_root_row_opens_the_field_with_the_current_path() {
        let (_dir, mut ctx) = world();
        let ctx = &mut ctx;
        let mut nav = Nav::single(Box::new(crate::screens::Stub::new("Home")));
        nav.push(Box::new(Settings::new()));
        for action in [Action::Right, Action::Confirm] {
            nav.handle(Input::Action(action), &Hits::default(), ctx);
        }
        assert_eq!(nav.top().title(), "Games Root");
        assert!(nav.top().is_modal() && nav.top().text_input());
        let entry = GamesRootEntry::new(ctx);
        assert_eq!(entry.entry.text(), path(&ctx.games_root));
        assert_eq!(entry.hints(), TextEntry::hints(ctx.input_kind, "Save"));
    }

    #[test]
    fn the_field_shows_a_root_under_home_in_its_tilde_form() {
        let mut ctx = Ctx::empty();
        ctx.games_root = dirs::home_dir().unwrap().join("Games");
        assert_eq!(GamesRootEntry::new(&ctx).entry.text(), "~/Games");
    }

    #[test]
    fn saving_a_new_root_writes_the_config_makes_the_folders_and_moves_the_library() {
        let (dir, mut ctx) = world();
        let ctx = &mut ctx;
        let game = |ctx: &Ctx, title: &str| {
            let games = ctx.library.system_games("nes", SortOrder::AZ).unwrap();
            games.into_iter().find(|g| g.title == title).unwrap()
        };
        ctx.library.record_play(game(ctx, "Game").id, 100, 60).unwrap();
        assert_eq!(titles(ctx), ["Game", "Left"]);
        let revision = ctx.revision;
        let new = dir.path().join("new");
        let mut entry = GamesRootEntry::new(ctx);
        assert!(matches!(save(&mut entry, ctx, &path(&new)), Transition::Pop));

        let saved = config::load(&ctx.config_path).unwrap().unwrap();
        assert_eq!(saved.games_root, new, "the Config was rewritten");
        for system in &SYSTEMS {
            assert!(new.join(system.id).is_dir(), "{}", system.id);
        }
        assert!(new.join("bios").is_dir());
        assert_eq!(ctx.games_root, new);
        assert_eq!(titles(ctx), ["Found", "Game"], "Left stayed under the old root");
        let moved = game(ctx, "Game");
        assert_eq!(
            (moved.playtime_secs, moved.last_played),
            (60, Some(100)),
            "Playtime moved"
        );
        assert!(ctx.revision > revision, "screens reload");
        assert!(ctx.games_root_changed, "the watcher is asked to follow");
        let rows = Library::default().rows(ctx);
        assert_eq!(rows[0].value.as_deref(), Some(path(&new).as_str()));
    }

    #[test]
    fn saving_keeps_the_rest_of_the_config() {
        let (dir, mut ctx) = world();
        let ctx = &mut ctx;
        fs::write(&ctx.config_path, "# mine\ngames_root = \"/old\"\nother = 1\n").unwrap();
        let new = dir.path().join("new");
        save(&mut GamesRootEntry::new(ctx), ctx, &path(&new));
        let text = fs::read_to_string(&ctx.config_path).unwrap();
        assert!(text.contains("# mine") && text.contains("other = 1"), "{text}");
    }

    #[test]
    fn a_relative_path_is_refused_and_the_field_stays_open() {
        let (_dir, mut ctx) = world();
        let ctx = &mut ctx;
        let before = state(ctx);
        let mut entry = GamesRootEntry::new(ctx);
        assert!(matches!(save(&mut entry, ctx, "Games"), Transition::None));
        assert_eq!(entry.error.as_deref(), Some(RELATIVE));
        assert_eq!(state(ctx), before);
        entry.handle(Input::Text(TextInput::Backspace), ctx);
        assert_eq!(entry.error, None, "typing clears the error");
    }

    #[test]
    fn a_folder_that_cannot_be_made_shows_why_and_writes_no_config() {
        let (dir, mut ctx) = world();
        let ctx = &mut ctx;
        let file = dir.path().join("file");
        fs::write(&file, b"").unwrap();
        let before = state(ctx);
        let mut entry = GamesRootEntry::new(ctx);
        let under = file.join("Games");
        assert!(matches!(save(&mut entry, ctx, &path(&under)), Transition::None));
        let shown = format!("Can't create {}: Not a directory", path(&under));
        assert_eq!(entry.error, Some(shown));
        assert_eq!(state(ctx), before);
    }

    #[test]
    fn saving_the_unchanged_path_closes_doing_nothing() {
        let (_dir, mut ctx) = world();
        let ctx = &mut ctx;
        let before = state(ctx);
        let mut entry = GamesRootEntry::new(ctx);
        let unchanged = format!("{}/", path(&ctx.games_root));
        assert!(matches!(save(&mut entry, ctx, &unchanged), Transition::Pop));
        assert_eq!(state(ctx), before);
        assert!(!ctx.games_root.join("snes").exists(), "no folders made");
    }

    #[test]
    fn an_empty_field_stays_open_and_cancel_changes_nothing() {
        let (dir, mut ctx) = world();
        let ctx = &mut ctx;
        let before = state(ctx);
        let mut entry = GamesRootEntry::new(ctx);
        assert!(matches!(save(&mut entry, ctx, "  "), Transition::None));
        retype(&mut entry, ctx, &path(&dir.path().join("new")));
        assert!(matches!(
            entry.handle(Input::Action(Action::Back), ctx),
            Transition::Pop
        ));
        assert_eq!(state(ctx), before);
        assert!(!dir.path().join("new/snes").exists(), "no folders made");
    }
}
