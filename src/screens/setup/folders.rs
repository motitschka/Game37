//! Step 3, "Make your folders": the Games Root, with Change, and the folders Game37 finds Games in,
//! one per ticked System plus `bios/`, which "Create folders" makes, after `screen_setup_folders` in
//! `design/generate_mockups.py`. Continue works once every folder is there.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{Button, CONTENT_Y, Go, Model, Pill, SETUP_W, Step, WHITE, pill, pill_width, tick};
use crate::config;
use crate::ctx::Ctx;
use crate::screens::home::tilde;
use crate::ui::cover::ellipsize;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, InputKind, Pointer};
use crate::ui::nav::Hint;
use crate::ui::text::Weight;
use crate::ui::text_entry::{KEYBOARD_W, Outcome, TextEntry};
use crate::ui::theme::{DANGER, DIM, LINE, MUTED, SUCCESS, SURFACE, SURFACE2, TEXT};
use crate::ui::{M, Rect, hit};

/// The most characters a typed Games Root can have, as Linux limits a path.
const MAX_PATH_CHARS: usize = 4096;
const ROOT_H: f32 = 96.0;
const CHANGE_W: f32 = 160.0;
/// Top of the folder list's box.
const LIST_Y: f32 = 464.0;
const COLS: usize = 3;
const COL_W: f32 = 410.0;
const ROW_PITCH: f32 = 52.0;
/// Where a folder's System name starts, from the folder's tick.
const LABEL_X: f32 = 232.0;
const CREATED_W: f32 = 280.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Change,
    Create,
    Button(Button),
}

/// A folder the step makes, with what it is for.
struct Folder {
    path: PathBuf,
    label: &'static str,
}

/// The folders for what is ticked: one per ticked System in System Order, then `bios/`.
fn folders(model: &Model) -> Vec<Folder> {
    let root = &model.games_root;
    model
        .ticked()
        .map(|system| Folder {
            path: root.join(system.id),
            label: system.name,
        })
        .chain([Folder {
            path: root.join("bios"),
            label: "BIOS",
        }])
        .collect()
}

/// How many of the folders are made.
fn made(model: &Model) -> usize {
    folders(model)
        .iter()
        .filter(|f| model.made.contains(&f.path))
        .count()
}

/// Why making a folder failed, without the "(os error n)" the system adds.
pub(super) fn reason(e: &io::Error) -> String {
    let text = e.to_string();
    match text.find(" (os error") {
        Some(end) => text[..end].to_owned(),
        None => text,
    }
}

/// The end of `text` that fits in `max_w`, after "…" when some of its start is cut, so a long
/// path keeps the folder it ends in.
fn trim_start(text: &str, max_w: f32, mut measure: impl FnMut(&str) -> f32) -> String {
    if measure(text) <= max_w {
        return text.to_owned();
    }
    for (i, _) in text.char_indices().skip(1) {
        let trimmed = format!("…{}", &text[i..]);
        if measure(&trimmed) <= max_w {
            return trimmed;
        }
    }
    "…".to_owned()
}

pub struct Folders {
    focus: Focus,
    /// The Games Root being typed, while Change is open.
    entry: Option<TextEntry>,
    /// Why the typed Games Root wasn't taken, shown under the field until it is closed.
    entry_error: Option<&'static str>,
    /// Why each folder that failed on the last "Create folders" wasn't made.
    errors: Vec<(PathBuf, String)>,
    /// What the player last used, as of the last frame, for the text field's hints.
    kind: InputKind,
    /// Where Change and "Create folders" were drawn, for the mouse.
    hits: Vec<(Rect, Focus)>,
}

impl Folders {
    pub fn new() -> Folders {
        Folders {
            focus: Focus::Create,
            entry: None,
            entry_error: None,
            errors: Vec::new(),
            kind: InputKind::Controller,
            hits: Vec::new(),
        }
    }

    /// Makes every folder, recording in the Model the ones that are there afterwards.
    fn create(&mut self, model: &mut Model) {
        self.errors.clear();
        for folder in folders(model) {
            match fs::create_dir_all(&folder.path) {
                Ok(()) if model.made.contains(&folder.path) => {}
                Ok(()) => model.made.push(folder.path),
                Err(e) => self.errors.push((folder.path, reason(&e))),
            }
        }
        if self.errors.is_empty() {
            self.focus = Focus::Button(Button::Continue);
        }
    }

    fn error(&self, path: &Path) -> Option<&str> {
        self.errors
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, e)| e.as_str())
    }

    /// Takes the typed Games Root, `~` standing for the home folder. An empty field stays open, and
    /// so does a relative path, saying why.
    fn save(&mut self, model: &mut Model) {
        let typed = self.entry.as_ref().map_or("", |e| e.text()).trim();
        if typed.is_empty() {
            return;
        }
        let root = config::expand_home(typed);
        if root.is_relative() {
            self.entry_error = Some("Use a full path, starting with / or ~");
            return;
        }
        model.games_root = root;
        model.made.clear();
        self.errors.clear();
        self.close_entry();
        self.focus = Focus::Create;
    }

    fn close_entry(&mut self) {
        self.entry = None;
        self.entry_error = None;
    }

    fn move_focus(&mut self, action: Action) {
        self.focus = match (self.focus, action) {
            (Focus::Change, Action::Down) => Focus::Create,
            (Focus::Create, Action::Up) => Focus::Change,
            (Focus::Create, Action::Down | Action::Right) => Focus::Button(Button::Continue),
            (Focus::Button(Button::Back), Action::Right) => Focus::Button(Button::Continue),
            (Focus::Button(Button::Continue), Action::Left) => Focus::Button(Button::Back),
            (Focus::Button(_), Action::Up) => Focus::Create,
            (focus, _) => focus,
        };
    }

    fn confirm(&mut self, model: &mut Model) -> Go {
        match self.focus {
            Focus::Change => {
                let shown = tilde(&model.games_root, dirs::home_dir().as_deref());
                self.entry = Some(TextEntry::new(&shown, MAX_PATH_CHARS));
            }
            Focus::Create => self.create(model),
            Focus::Button(Button::Back) => return Go::Back,
            Focus::Button(Button::Continue) => return Go::Next,
        }
        Go::Stay
    }

    fn draw_entry(&mut self, gfx: &mut Gfx<'_>) {
        let Some(entry) = &mut self.entry else {
            return;
        };
        let label = TextStyle::new(14, Weight::Bold, MUTED);
        gfx.text(M, CONTENT_Y + 20.0, "GAMES ROOT", label, Align::Left);
        entry.draw_field(gfx, Rect::new(M, CONTENT_Y + 40.0, KEYBOARD_W, 88.0), "~/Games");
        if let Some(error) = self.entry_error {
            let style = TextStyle::new(18, Weight::Medium, DANGER);
            gfx.text(M, CONTENT_Y + 162.0, error, style, Align::Left);
        }
        entry.draw_keyboard(gfx, M, CONTENT_Y + 188.0);
    }

    fn draw_root(&mut self, gfx: &mut Gfx<'_>, root: &Path) {
        let (x, y) = (M, CONTENT_Y);
        let r = Rect::new(x, y, SETUP_W, ROOT_H);
        gfx.fill_round_rect(r, 14.0, SURFACE);
        gfx.stroke_round_rect(r, 14.0, 1.0, LINE);
        let label = TextStyle::new(13, Weight::Bold, MUTED);
        gfx.text(x + 28.0, y + 36.0, "GAMES ROOT", label, Align::Left);
        let path = TextStyle::new(26, Weight::SemiBold, TEXT);
        let shown = tilde(root, dirs::home_dir().as_deref());
        let shown = trim_start(&shown, SETUP_W - 28.0 - 48.0 - CHANGE_W - 24.0, |s| {
            gfx.measure(s, path)
        });
        gfx.text(x + 28.0, y + 74.0, &shown, path, Align::Left);

        let change = Rect::new(x + SETUP_W - 24.0 - CHANGE_W, y + 18.0, CHANGE_W, 60.0);
        if self.focus == Focus::Change {
            gfx.stroke_round_rect(change.inset(-6.0), 36.0, 3.0, WHITE);
        }
        gfx.fill_round_rect(change, 30.0, SURFACE2);
        gfx.stroke_round_rect(change, 30.0, 2.0, LINE);
        let style = TextStyle::new(20, Weight::Bold, TEXT);
        gfx.text(
            change.x + CHANGE_W / 2.0,
            y + 56.0,
            "Change",
            style,
            Align::Centre,
        );
        self.hits.push((change, Focus::Change));
    }

    /// The folder list, down the columns. Returns its bottom.
    fn draw_list(&self, gfx: &mut Gfx<'_>, model: &Model) -> f32 {
        let folders = folders(model);
        let label = TextStyle::new(14, Weight::Bold, MUTED);
        gfx.text(M, LIST_Y - 20.0, "FOLDERS", label, Align::Left);
        let made = made(model);
        if made > 0 {
            let count = format!("{made} of {} made", folders.len());
            let style = TextStyle::new(16, Weight::Medium, MUTED);
            gfx.text(M + 120.0, LIST_Y - 20.0, &count, style, Align::Left);
        }

        let per_col = folders.len().div_ceil(COLS);
        let h = per_col as f32 * ROW_PITCH + 40.0;
        let r = Rect::new(M, LIST_Y, SETUP_W, h);
        gfx.fill_round_rect(r, 14.0, SURFACE);
        gfx.stroke_round_rect(r, 14.0, 1.0, LINE);
        let path_style = TextStyle::new(20, Weight::Medium, TEXT);
        let home = dirs::home_dir();
        for (i, folder) in folders.iter().enumerate() {
            let (col, row) = (i / per_col, i % per_col);
            let fx = M + 28.0 + col as f32 * COL_W;
            let fy = LIST_Y + 20.0 + row as f32 * ROW_PITCH;
            let error = self.error(&folder.path);
            if model.made.contains(&folder.path) {
                tick(gfx, fx, fy + 18.0, SUCCESS);
            } else if error.is_some() {
                gfx.fill_circle(fx + 9.0, fy + 26.0, 6.0, DANGER);
            }
            let shown = format!("{}/", tilde(&folder.path, home.as_deref()));
            let shown = trim_start(&shown, LABEL_X - 34.0 - 12.0, |s| gfx.measure(s, path_style));
            gfx.text(fx + 34.0, fy + 34.0, &shown, path_style, Align::Left);
            let room = if col + 1 == COLS {
                r.x + r.w - 20.0 - (fx + LABEL_X)
            } else {
                COL_W - LABEL_X - 12.0
            };
            let (text, colour) = match error {
                Some(e) => (e, DANGER),
                None => (folder.label, MUTED),
            };
            let style = TextStyle::new(17, Weight::Medium, colour);
            let text = ellipsize(text, room, |s| gfx.measure(s, style));
            gfx.text(fx + LABEL_X, fy + 34.0, &text, style, Align::Left);
        }
        LIST_Y + h
    }

    /// "Create folders", or "Folders created" once every folder is made, at `y`, with the note.
    fn draw_create(&mut self, gfx: &mut Gfx<'_>, y: f32, done: bool) {
        let focused = self.focus == Focus::Create;
        let w = if done {
            let r = Rect::new(M, y, CREATED_W, 64.0);
            if focused {
                gfx.stroke_round_rect(r.inset(-6.0), 38.0, 3.0, WHITE);
            }
            gfx.fill_round_rect(r, 32.0, SURFACE.alpha(0.6));
            gfx.stroke_round_rect(r, 32.0, 2.0, LINE.alpha(0.6));
            tick(gfx, M + 32.0, y + 26.0, SUCCESS);
            let style = TextStyle::new(22, Weight::Bold, MUTED);
            gfx.text(M + 66.0, y + 40.0, "Folders created", style, Align::Left);
            CREATED_W
        } else {
            let label = "Create folders";
            let w = pill_width(gfx, label);
            pill(gfx, Rect::new(M, y, w, 64.0), label, Pill::Normal { focused });
            w
        };
        self.hits.push((Rect::new(M, y, w, 64.0), Focus::Create));
        let note = TextStyle::new(18, Weight::Medium, DIM);
        let text = "Folders that already exist are left as they are.";
        gfx.text(M + w + 24.0, y + 40.0, text, note, Align::Left);
    }
}

impl Step for Folders {
    fn title(&self) -> &'static str {
        "Make your folders"
    }

    fn explanation(&self) -> &'static str {
        "Game37 finds Games by the folder they're in."
    }

    fn enter(&mut self, model: &Model, _: &mut Ctx) {
        self.close_entry();
        self.errors.clear();
        self.focus = if self.can_continue(model) {
            Focus::Button(Button::Continue)
        } else {
            Focus::Create
        };
    }

    fn can_continue(&self, model: &Model) -> bool {
        made(model) == folders(model).len()
    }

    fn handle(&mut self, input: Input, model: &mut Model, _: &mut Ctx) -> Go {
        if let Some(entry) = &mut self.entry {
            match entry.handle(input) {
                Outcome::Submit => self.save(model),
                Outcome::Cancel => self.close_entry(),
                Outcome::None | Outcome::Changed => {}
            }
            return Go::Stay;
        }
        match input {
            Input::Action(Action::Confirm) => return self.confirm(model),
            Input::Action(action) if action.is_direction() => self.move_focus(action),
            Input::Pointer(Pointer::Move { x, y }) => {
                if let Some(focus) = hit(&self.hits, x, y) {
                    self.focus = focus;
                }
            }
            Input::Pointer(Pointer::Click { x, y }) => {
                if let Some(focus) = hit(&self.hits, x, y) {
                    self.focus = focus;
                    return self.confirm(model);
                }
            }
            _ => {}
        }
        Go::Stay
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, model: &Model, ctx: &Ctx) {
        self.kind = ctx.input_kind;
        self.hits.clear();
        if self.entry.is_some() {
            return self.draw_entry(gfx);
        }
        self.draw_root(gfx, &model.games_root);
        let bottom = self.draw_list(gfx, model);
        let done = self.can_continue(model);
        self.draw_create(gfx, bottom + 40.0, done);
    }

    fn hints(&self, _: &Model) -> Vec<Hint> {
        match self.entry {
            Some(_) => TextEntry::hints(self.kind, "Save"),
            None => vec![Hint::new(&[Action::Confirm], "Choose")],
        }
    }

    fn focused_button(&self) -> Option<Button> {
        match self.focus {
            Focus::Button(b) => Some(b),
            _ => None,
        }
    }

    fn text_input(&self) -> bool {
        self.entry.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::super::Setup;
    use super::super::tests::act;
    use super::*;
    use crate::systems::SYSTEMS;
    use crate::ui::input::TextInput;
    use crate::ui::nav::Screen;

    /// Setup on step 1 with `ids` ticked and the Games Root at `root`.
    fn ticking(root: &Path, ids: &[&str]) -> Setup {
        let mut setup = Setup::new(root.to_path_buf());
        for (i, system) in SYSTEMS.iter().enumerate() {
            setup.model.ticked[i] = ids.contains(&system.id);
        }
        setup
    }

    /// Setup on step 3, Embedded Systems only being ticked.
    fn on_folders(root: &Path, ids: &[&str]) -> (Setup, Ctx) {
        let mut ctx = Ctx::empty();
        let mut setup = ticking(root, ids);
        act(&mut setup, &mut ctx, Action::Settings);
        assert_eq!(setup.step(), 3);
        (setup, ctx)
    }

    /// Opens Change, which Up twice reaches from anywhere, erases the path and types `text`,
    /// without finishing.
    fn type_root(setup: &mut Setup, ctx: &mut Ctx, text: &str) {
        act(setup, ctx, Action::Up);
        act(setup, ctx, Action::Up);
        act(setup, ctx, Action::Confirm);
        assert!(setup.text_input(), "Change opens the field");
        // More than the temp paths' length.
        for _ in 0..200 {
            setup.handle(Input::Text(TextInput::Backspace), ctx);
        }
        for ch in text.chars() {
            setup.handle(Input::Text(TextInput::Char(ch)), ctx);
        }
    }

    #[test]
    fn the_folders_are_the_ticked_systems_in_system_order_then_bios() {
        let root = tempfile::tempdir().unwrap();
        let setup = ticking(root.path(), &["psp", "nes", "gc", "gba"]);
        let listed: Vec<_> = folders(&setup.model)
            .into_iter()
            .map(|f| (f.path, f.label))
            .collect();
        let at = |id: &str| root.path().join(id);
        assert_eq!(
            listed,
            [
                (at("nes"), "NES"),
                (at("gba"), "Game Boy Advance"),
                (at("gc"), "GameCube"),
                (at("psp"), "PSP"),
                (at("bios"), "BIOS"),
            ]
        );
    }

    #[test]
    fn create_makes_the_folders_and_leaves_existing_ones_as_they_are() {
        let root = tempfile::tempdir().unwrap();
        let games = root.path().join("Games");
        fs::create_dir_all(games.join("nes")).unwrap();
        fs::write(games.join("nes/Kept.nes"), b"").unwrap();
        let (mut setup, mut ctx) = on_folders(&games, &["nes", "snes"]);
        assert!(setup.hints().iter().any(|h| h.text == "Choose"));
        act(&mut setup, &mut ctx, Action::Confirm);
        for id in ["nes", "snes", "bios"] {
            assert!(games.join(id).is_dir(), "{id}");
        }
        assert!(games.join("nes/Kept.nes").is_file());
        assert_eq!(made(&setup.model), 3);
        assert!(setup.steps[setup.at].can_continue(&setup.model));
        assert_eq!(setup.steps[setup.at].focused_button(), Some(Button::Continue));

        act(&mut setup, &mut ctx, Action::Up);
        act(&mut setup, &mut ctx, Action::Confirm);
        assert_eq!(made(&setup.model), 3, "creating them again is fine");
    }

    #[test]
    fn continue_waits_for_the_folders_and_back_and_forward_keep_them() {
        let root = tempfile::tempdir().unwrap();
        let (mut setup, mut ctx) = on_folders(root.path(), &["gba"]);
        assert!(!setup.hints().iter().any(|h| h.text == "Continue"));
        act(&mut setup, &mut ctx, Action::Settings);
        act(&mut setup, &mut ctx, Action::Down);
        act(&mut setup, &mut ctx, Action::Confirm);
        assert_eq!(setup.step(), 3, "neither + nor the Continue button go on");

        act(&mut setup, &mut ctx, Action::Up);
        act(&mut setup, &mut ctx, Action::Confirm);
        assert!(setup.hints().iter().any(|h| h.text == "Continue"));
        act(&mut setup, &mut ctx, Action::Back);
        act(&mut setup, &mut ctx, Action::Settings);
        assert_eq!(setup.step(), 3);
        assert_eq!(setup.steps[setup.at].focused_button(), Some(Button::Continue));
        act(&mut setup, &mut ctx, Action::Settings);
        assert_eq!(setup.step(), 4);
        act(&mut setup, &mut ctx, Action::Back);
        assert_eq!(setup.step(), 3, "still made");
        assert!(setup.steps[setup.at].can_continue(&setup.model));

        act(&mut setup, &mut ctx, Action::Back);
        act(&mut setup, &mut ctx, Action::Right);
        act(&mut setup, &mut ctx, Action::Confirm);
        act(&mut setup, &mut ctx, Action::Settings);
        assert_eq!(setup.step(), 3, "a newly ticked System's folder isn't made yet");
        assert!(!setup.steps[setup.at].can_continue(&setup.model));
    }

    #[test]
    fn change_sets_the_games_root_and_clears_the_made_folders() {
        let root = tempfile::tempdir().unwrap();
        let (mut setup, mut ctx) = on_folders(&root.path().join("Games"), &["nes"]);
        act(&mut setup, &mut ctx, Action::Confirm);
        assert!(setup.steps[setup.at].can_continue(&setup.model));

        let other = root.path().join("Other");
        type_root(&mut setup, &mut ctx, other.to_str().unwrap());
        assert_eq!(
            setup.hints(),
            TextEntry::hints(InputKind::Controller, "Save"),
            "the field's hints alone"
        );
        act(&mut setup, &mut ctx, Action::Settings);
        assert_eq!(
            setup.step(),
            3,
            "the field's input is the field's, not the frame's"
        );
        act(&mut setup, &mut ctx, Action::Start);
        assert!(!setup.text_input());
        assert_eq!(setup.games_root(), other);
        assert_eq!(made(&setup.model), 0, "the new root's folders aren't made");
        assert!(!setup.steps[setup.at].can_continue(&setup.model));
        assert!(!other.exists(), "saving makes nothing");

        type_root(&mut setup, &mut ctx, "~/Somewhere/Games");
        act(&mut setup, &mut ctx, Action::Start);
        let home = dirs::home_dir().unwrap();
        assert_eq!(setup.games_root(), home.join("Somewhere/Games"));
    }

    #[test]
    fn cancelling_change_keeps_the_games_root_and_the_step() {
        let root = tempfile::tempdir().unwrap();
        let (mut setup, mut ctx) = on_folders(root.path(), &["nes"]);
        type_root(&mut setup, &mut ctx, "/elsewhere");
        act(&mut setup, &mut ctx, Action::Back);
        assert!(!setup.text_input());
        assert_eq!(setup.step(), 3, "B closes the field, not the step");
        assert_eq!(setup.games_root(), root.path());

        type_root(&mut setup, &mut ctx, "  ");
        act(&mut setup, &mut ctx, Action::Start);
        assert!(setup.text_input(), "an empty Games Root isn't taken");
        assert_eq!(setup.games_root(), root.path());
    }

    #[test]
    fn a_relative_games_root_is_refused_and_the_field_stays_open() {
        let root = tempfile::tempdir().unwrap();
        let (mut setup, mut ctx) = on_folders(root.path(), &["nes"]);
        for typed in ["Games", "./Games", "~Games"] {
            type_root(&mut setup, &mut ctx, typed);
            act(&mut setup, &mut ctx, Action::Start);
            assert!(setup.text_input(), "{typed:?} stays open");
            assert_eq!(setup.games_root(), root.path(), "{typed:?} isn't taken");
            act(&mut setup, &mut ctx, Action::Back);
        }

        let mut step = Folders::new();
        let mut model = Model::new(root.path().to_path_buf());
        step.entry = Some(TextEntry::new("Games", MAX_PATH_CHARS));
        step.save(&mut model);
        assert_eq!(step.entry_error, Some("Use a full path, starting with / or ~"));
        step.handle(Input::Action(Action::Back), &mut model, &mut ctx);
        assert_eq!(step.entry_error, None, "closing the field clears it");
        step.entry = Some(TextEntry::new("/mnt/Games", MAX_PATH_CHARS));
        step.save(&mut model);
        assert_eq!((step.entry.is_none(), step.entry_error), (true, None));
        assert_eq!(model.games_root, Path::new("/mnt/Games"));
    }

    #[test]
    fn a_folder_that_cannot_be_made_shows_why_and_blocks_continue() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Games");
        fs::write(&file, b"").unwrap();
        let (mut setup, mut ctx) = on_folders(&file, &["nes"]);
        act(&mut setup, &mut ctx, Action::Confirm);
        act(&mut setup, &mut ctx, Action::Settings);
        assert_eq!(setup.step(), 3);
        assert_eq!(made(&setup.model), 0);

        let mut step = Folders::new();
        let mut model = Model::new(file.clone());
        step.create(&mut model);
        assert_eq!(step.error(&file.join("bios")), Some("Not a directory"));
        assert_eq!(step.focus, Focus::Create, "for trying again");
        assert!(!step.can_continue(&model));
    }

    #[test]
    fn a_long_path_is_trimmed_from_the_start() {
        let measure = |s: &str| s.chars().count() as f32;
        assert_eq!(trim_start("~/Games/nes/", 20.0, measure), "~/Games/nes/");
        assert_eq!(trim_start("/mnt/storage/Games/nes/", 10.0, measure), "…ames/nes/");
    }
}
