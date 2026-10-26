//! First-run Setup (FR-CFG-4, US-36), shown when there is no Config: one screen that walks through
//! its steps under a "Setup · step n of 5" header, after `setup_frame` and `setup_buttons` in
//! `design/generate_mockups.py`.
//!
//! What the player chose lives in one [`Model`] that every step reads and changes, so it survives
//! going Back and forward. Each step keeps its own focus. A step that doesn't apply to what is
//! ticked is passed over both ways, and Back from the first step quits Game37. Finish, on the last
//! step, writes the Config and leaves it in `Ctx::setup_finished` for `app.rs` to switch to Home;
//! nothing before it writes the Config, so quitting earlier brings Setup back next time.

mod cores;
mod emulators;
mod folders;
mod key;
mod systems;

use std::path::{Path, PathBuf};

use crate::config::{self, Config, ConfigError};
use crate::ctx::Ctx;
use crate::systems::{SYSTEMS, System};
use crate::ui::cover::ellipsize;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, BG, Colour, DANGER, DIM, LINE, MUTED, SURFACE, TEXT};
use crate::ui::{M, Rect, W, hit};

/// How many steps the header counts, the later ones included.
const STEPS: usize = 5;
/// Top of each step's content, below the title and the explanation.
const CONTENT_Y: f32 = 296.0;
/// Width of the lists and boxes below the title, left-aligned at `M`.
const SETUP_W: f32 = 1200.0;
/// Top of the pill buttons above the hint bar.
const BUTTONS_Y: f32 = 906.0;
const PILL_H: f32 = 64.0;

/// What the player has chosen so far.
pub struct Model {
    /// Whether each System, by its index in `SYSTEMS`, is ticked.
    ticked: Vec<bool>,
    games_root: PathBuf,
    /// The folders "Create folders" made or found already there, under this Games Root.
    made: Vec<PathBuf>,
}

impl Model {
    fn new(games_root: PathBuf) -> Model {
        Model {
            ticked: vec![false; SYSTEMS.len()],
            games_root,
            made: Vec::new(),
        }
    }

    /// The ticked Systems, in System Order.
    pub fn ticked(&self) -> impl Iterator<Item = &'static System> + '_ {
        SYSTEMS
            .iter()
            .zip(&self.ticked)
            .filter(|&(_, &on)| on)
            .map(|(s, _)| s)
    }
}

/// The two buttons at the bottom right of every step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Back,
    Continue,
}

/// Where a step's input leads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Go {
    Stay,
    Back,
    Next,
}

/// One step: its own focus, input and drawing, over the shared [`Model`]. The header, title,
/// explanation and the Back and Continue buttons are drawn around it.
trait Step {
    fn title(&self) -> &'static str;
    fn explanation(&self) -> &'static str;
    /// Whether the step is shown for what is ticked.
    fn applies(&self, _model: &Model) -> bool {
        true
    }
    /// Called each time the step is shown.
    fn enter(&mut self, _model: &Model, _ctx: &mut Ctx) {}
    /// Called each frame while the step is shown.
    fn tick(&mut self, _model: &Model, _ctx: &mut Ctx) {}
    /// Called each time the step is left, forward or back, before the next one is entered: a step
    /// cancels its downloads, or clears something sensitive such as the typed SteamGridDB key.
    fn leave(&mut self) {}
    fn can_continue(&self, model: &Model) -> bool;
    /// Handles input other than Back and Continue (+), which the frame handles. Confirm on a
    /// focused [`Button`] returns its [`Go`].
    fn handle(&mut self, input: Input, model: &mut Model, ctx: &mut Ctx) -> Go;
    fn draw(&mut self, gfx: &mut Gfx<'_>, model: &Model, ctx: &Ctx);
    /// The hints between A's and Continue's: Continue and Back are added after them.
    fn hints(&self, model: &Model) -> Vec<Hint>;
    /// Which of the bottom buttons has focus, if one does.
    fn focused_button(&self) -> Option<Button>;
    /// Runs just before Finish writes the Config; `Err` shows why and keeps the step, writing no
    /// Config (default: nothing to do).
    fn before_finish(&mut self, _model: &Model, _ctx: &mut Ctx) -> Result<(), String> {
        Ok(())
    }
    /// Whether a text field is open: it then takes all input and its hints, and the bottom
    /// buttons are hidden.
    fn text_input(&self) -> bool {
        false
    }
}

pub struct Setup {
    model: Model,
    /// Every step in order; its position gives its number.
    steps: Vec<Box<dyn Step>>,
    at: usize,
    buttons: Vec<(Rect, Button)>,
    /// Why Finish couldn't write the Config, shown on the last step until it is left.
    error: Option<String>,
}

impl Setup {
    /// Setup proposing `games_root` as the Games Root: `~/Games`, or a folder under `--data-dir` in
    /// screenshot mode, so no smoke run makes folders in the real one.
    pub fn new(games_root: PathBuf) -> Setup {
        Setup {
            model: Model::new(games_root),
            steps: vec![
                Box::new(systems::Systems::new()),
                Box::new(emulators::Install::new()),
                Box::new(folders::Folders::new()),
                Box::new(cores::Cores::new()),
                Box::new(key::Key::new()),
            ],
            at: 0,
            buttons: Vec::new(),
            error: None,
        }
    }

    /// The Games Root the player chose; Finish writes it to the Config.
    pub fn games_root(&self) -> &Path {
        &self.model.games_root
    }

    /// The step shown, counted from 1.
    pub fn step(&self) -> usize {
        self.at + 1
    }

    /// The nearest step before or after `at` that applies to what is ticked.
    fn neighbour(&self, forward: bool) -> Option<usize> {
        let applies = |&i: &usize| self.steps[i].applies(&self.model);
        if forward {
            (self.at + 1..self.steps.len()).find(applies)
        } else {
            (0..self.at).rev().find(applies)
        }
    }

    fn go(&mut self, go: Go, ctx: &mut Ctx) -> Transition {
        let next = match go {
            Go::Stay => return Transition::None,
            Go::Back => match self.neighbour(false) {
                Some(i) => i,
                None => return Transition::Quit,
            },
            Go::Next if self.steps[self.at].can_continue(&self.model) => match self.neighbour(true) {
                Some(i) => i,
                None => {
                    self.finish(ctx);
                    return Transition::None;
                }
            },
            Go::Next => return Transition::None,
        };
        self.steps[self.at].leave();
        self.at = next;
        self.error = None;
        self.steps[next].enter(&self.model, ctx);
        Transition::None
    }

    /// Writes the Config with the chosen Games Root and asks `app.rs` to switch to Home, or shows
    /// why it couldn't and stays. First gives the current step a chance to object (the key step
    /// saving a bad or unwritable SteamGridDB key, say), writing no Config either.
    fn finish(&mut self, ctx: &mut Ctx) {
        if let Err(e) = self.steps[self.at].before_finish(&self.model, ctx) {
            self.error = Some(e);
            return;
        }
        let config = Config {
            games_root: self.model.games_root.clone(),
        };
        match config::save(&ctx.config_path, &config) {
            Ok(()) => {
                self.error = None;
                ctx.setup_finished = Some(config);
            }
            Err(ConfigError::Write(_, e)) => {
                self.error = Some(format!("Can't write the Config: {}", folders::reason(&e)));
            }
            Err(e) => self.error = Some(format!("Can't write the Config: {e}")),
        }
    }

    /// The label of the button that goes on: Finish on the last step.
    fn continue_label(&self) -> &'static str {
        if self.neighbour(true).is_some() {
            "Continue"
        } else {
            "Finish"
        }
    }

    fn back_label(&self) -> &'static str {
        if self.neighbour(false).is_some() {
            "Back"
        } else {
            "Quit"
        }
    }
}

impl Screen for Setup {
    fn title(&self) -> &str {
        "First-run Setup"
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        let go = match input {
            _ if self.steps[self.at].text_input() => self.steps[self.at].handle(input, &mut self.model, ctx),
            Input::Action(Action::Back) | Input::Pointer(Pointer::Back) => Go::Back,
            // + outside a text field: Start on a controller arrives as Settings.
            Input::Action(Action::Settings) => Go::Next,
            Input::Pointer(Pointer::Click { x, y }) if hit(&self.buttons, x, y).is_some() => {
                match hit(&self.buttons, x, y).unwrap() {
                    Button::Back => Go::Back,
                    Button::Continue => Go::Next,
                }
            }
            _ => self.steps[self.at].handle(input, &mut self.model, ctx),
        };
        self.go(go, ctx)
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Transition {
        self.steps[self.at].tick(&self.model, ctx);
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        let crumb = format!("Setup · step {} of {STEPS}", self.step());
        gfx.text(
            M + 150.0,
            70.0,
            &crumb,
            TextStyle::new(20, Weight::Medium, MUTED),
            Align::Left,
        );
        let (seg, gap) = (44.0, 10.0);
        let mut x = W - M - STEPS as f32 * seg - (STEPS - 1) as f32 * gap;
        for i in 1..=STEPS {
            let fill = match i.cmp(&self.step()) {
                std::cmp::Ordering::Less => ACCENT,
                std::cmp::Ordering::Equal => WHITE,
                std::cmp::Ordering::Greater => LINE,
            };
            gfx.fill_round_rect(Rect::new(x, 60.0, seg, 8.0), 4.0, fill);
            x += seg + gap;
        }
        let step = &mut self.steps[self.at];
        let title = TextStyle::new(44, Weight::ExtraBold, TEXT);
        gfx.text(M - 2.0, 200.0, step.title(), title, Align::Left);
        let explanation = TextStyle::new(20, Weight::Medium, MUTED);
        gfx.text(M, 244.0, step.explanation(), explanation, Align::Left);
        step.draw(gfx, &self.model, ctx);
        if step.text_input() {
            self.buttons.clear();
            return;
        }

        let back_label = self.back_label();
        let step = &self.steps[self.at];
        let focused = step.focused_button();
        let can_continue = step.can_continue(&self.model);
        let continue_kind = match (can_continue, focused == Some(Button::Continue)) {
            (false, focused) => Pill::Disabled { focused },
            (true, focused) => Pill::Primary { focused },
        };
        let continue_label = self.continue_label();
        let continue_w = pill_width(gfx, continue_label);
        let continue_r = Rect::new(W - M - continue_w, BUTTONS_Y, continue_w, PILL_H);
        let back_w = pill_width(gfx, back_label);
        let back_r = Rect::new(continue_r.x - 16.0 - back_w, BUTTONS_Y, back_w, PILL_H);
        let back_kind = Pill::Normal {
            focused: focused == Some(Button::Back),
        };
        pill(gfx, back_r, back_label, back_kind);
        pill(gfx, continue_r, continue_label, continue_kind);
        if let Some(error) = &self.error {
            let style = TextStyle::new(20, Weight::Medium, DANGER);
            let error = ellipsize(error, back_r.x - 32.0 - M, |s| gfx.measure(s, style));
            gfx.text(M, BUTTONS_Y + PILL_H / 2.0 + 7.0, &error, style, Align::Left);
        }
        self.buttons = vec![(back_r, Button::Back), (continue_r, Button::Continue)];
    }

    fn hints(&self) -> Vec<Hint> {
        let mut hints = self.steps[self.at].hints(&self.model);
        if self.steps[self.at].text_input() {
            return hints;
        }
        if self.steps[self.at].can_continue(&self.model) {
            hints.push(Hint::new(&[Action::Settings], self.continue_label()));
        }
        hints.push(Hint::new(&[Action::Back], self.back_label()));
        hints
    }

    fn clock(&self) -> bool {
        false
    }

    fn text_input(&self) -> bool {
        self.steps[self.at].text_input()
    }
}

const WHITE: Colour = Colour::hex(0xFFFFFF);

/// How a pill button looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pill {
    Normal { focused: bool },
    Primary { focused: bool },
    Disabled { focused: bool },
}

/// A pill button's width for `label`.
fn pill_width(gfx: &mut Gfx<'_>, label: &str) -> f32 {
    (gfx.measure(label, TextStyle::new(22, Weight::Bold, TEXT)) + 72.0).max(160.0)
}

/// A pill button, as `setup_buttons` draws them. Focus is a white ring, as an accent one would
/// vanish against the accent fill.
fn pill(gfx: &mut Gfx<'_>, r: Rect, label: &str, kind: Pill) {
    let (Pill::Normal { focused } | Pill::Primary { focused } | Pill::Disabled { focused }) = kind;
    let radius = r.h / 2.0;
    if focused {
        gfx.stroke_round_rect(r.inset(-6.0), radius + 6.0, 3.0, WHITE);
    }
    let (cx, base) = (r.x + r.w / 2.0, r.y + r.h / 2.0 + 8.0);
    match kind {
        Pill::Primary { .. } => {
            gfx.fill_round_rect(r, radius, ACCENT);
            let style = TextStyle::new(22, Weight::ExtraBold, TEXT);
            gfx.text(cx, base, label, style, Align::Centre);
        }
        Pill::Disabled { .. } => {
            gfx.fill_round_rect(r, radius, ACCENT.alpha(0.25));
            let style = TextStyle::new(22, Weight::ExtraBold, DIM);
            gfx.text(cx, base, label, style, Align::Centre);
        }
        Pill::Normal { .. } => {
            gfx.fill_round_rect(r, radius, SURFACE.alpha(0.85));
            gfx.stroke_round_rect(r, radius, 2.0, LINE);
            gfx.text(
                cx,
                base,
                label,
                TextStyle::new(22, Weight::Bold, TEXT),
                Align::Centre,
            );
        }
    }
}

/// A small swatch in the System's colour, styled like a Carousel card.
fn system_chip(gfx: &mut Gfx<'_>, x: f32, y: f32, size: f32, system: &System) {
    let colour = Colour::hex(system.colour);
    gfx.fill_round_rect_gradient(Rect::new(x, y, size, size), size * 0.2, colour, BG, (0.7, 1.0));
    gfx.fill_circle(x + size * 0.7, y + size * 0.3, size * 0.26, WHITE.alpha(0.1));
}

/// A tick mark about 18 by 13 with its top left at (`x`, `y`), as `check_path` draws it.
fn tick(gfx: &mut Gfx<'_>, x: f32, y: f32, colour: Colour) {
    let points = [(x, y + 7.0), (x + 6.0, y + 13.0), (x + 18.0, y)];
    gfx.line(points[0].0, points[0].1, points[1].0, points[1].1, 3.0, colour);
    gfx.line(points[1].0, points[1].1, points[2].0, points[2].1, 3.0, colour);
    // Round caps and join, as in the mockup.
    for (px, py) in points {
        gfx.fill_circle(px, py, 1.5, colour);
    }
}

/// A 30 px checkbox with its top left at (`x`, `y`).
fn checkbox(gfx: &mut Gfx<'_>, x: f32, y: f32, on: bool) {
    let r = Rect::new(x, y, 30.0, 30.0);
    if on {
        gfx.fill_round_rect(r, 8.0, ACCENT);
        tick(gfx, x + 6.0, y + 9.0, WHITE);
    } else {
        gfx.stroke_round_rect(r, 8.0, 2.0, MUTED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn act(setup: &mut Setup, ctx: &mut Ctx, action: Action) -> Transition {
        setup.handle(Input::Action(action), ctx)
    }

    #[test]
    fn the_proposed_games_root_is_the_one_passed_in() {
        let root = tempfile::tempdir().unwrap();
        let setup = Setup::new(root.path().join("Games"));
        assert_eq!(setup.games_root(), root.path().join("Games"));
    }

    /// Setup on its last step with NES ticked and its folders made under `root`, the Config to be
    /// written at `config_path`. NES's Core is installed in a data dir beside `root`, so the Cores
    /// step downloads nothing.
    fn on_last_step(root: &Path, config_path: &Path) -> (Setup, Ctx) {
        let mut ctx = Ctx::empty();
        ctx.config_path = config_path.to_path_buf();
        ctx.data_dir = root.with_file_name("data");
        let mesen = crate::cores::installed_path(&ctx.data_dir, &crate::cores::MESEN);
        std::fs::create_dir_all(mesen.parent().unwrap()).unwrap();
        std::fs::write(mesen, b"dummy").unwrap();
        ctx.start_download = Box::new(|_, _| panic!("Mesen is already installed"));
        let mut setup = Setup::new(root.to_path_buf());
        act(&mut setup, &mut ctx, Action::Confirm);
        act(&mut setup, &mut ctx, Action::Settings);
        assert_eq!(setup.step(), 3);
        act(&mut setup, &mut ctx, Action::Confirm);
        act(&mut setup, &mut ctx, Action::Settings);
        assert_eq!(setup.step(), 4);
        assert_eq!(setup.continue_label(), "Continue");
        act(&mut setup, &mut ctx, Action::Confirm);
        assert_eq!(setup.step(), 5, "A on a later step's focused Continue goes on");
        (setup, ctx)
    }

    #[test]
    fn finish_writes_the_config_with_the_chosen_games_root_and_asks_for_home() {
        let dir = tempfile::tempdir().unwrap();
        let (root, path) = (dir.path().join("Games"), dir.path().join("config/config.toml"));
        let (mut setup, mut ctx) = on_last_step(&root, &path);
        assert_eq!(setup.continue_label(), "Finish");
        assert!(setup.hints().iter().any(|h| h.text == "Finish"));
        assert!(!path.exists(), "no step before Finish writes the Config");
        assert_eq!(ctx.setup_finished, None);

        assert!(matches!(
            act(&mut setup, &mut ctx, Action::Settings),
            Transition::None
        ));
        let config = Config {
            games_root: root.clone(),
        };
        assert_eq!(config::load(&path).unwrap(), Some(config.clone()));
        assert_eq!(ctx.setup_finished, Some(config));
        assert_eq!(setup.error, None);
    }

    #[test]
    fn a_config_that_cannot_be_written_shows_why_and_stays() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        std::fs::write(&file, b"").unwrap();
        let (mut setup, mut ctx) = on_last_step(&dir.path().join("Games"), &file.join("config.toml"));
        act(&mut setup, &mut ctx, Action::Settings);
        assert_eq!(setup.step(), 5);
        assert_eq!(ctx.setup_finished, None);
        assert_eq!(
            setup.error.as_deref(),
            Some("Can't write the Config: Not a directory")
        );

        act(&mut setup, &mut ctx, Action::Back);
        assert_eq!(
            (setup.step(), setup.error.as_deref()),
            (4, None),
            "leaving the step clears it"
        );
    }

    #[test]
    fn back_on_the_first_step_quits() {
        let ctx = &mut Ctx::empty();
        let mut setup = Setup::new(PathBuf::from("/g"));
        assert_eq!(setup.back_label(), "Quit");
        assert!(matches!(act(&mut setup, ctx, Action::Back), Transition::Quit));
        let back = setup.handle(Input::Pointer(Pointer::Back), ctx);
        assert!(matches!(back, Transition::Quit), "the mouse Back button too");
    }

    #[test]
    fn continue_needs_a_ticked_system_then_back_returns() {
        let ctx = &mut Ctx::empty();
        let mut setup = Setup::new(PathBuf::from("/g"));
        assert!(!setup.hints().iter().any(|h| h.text == "Continue"));
        act(&mut setup, ctx, Action::Settings);
        assert_eq!(setup.step(), 1, "nothing ticked yet");

        act(&mut setup, ctx, Action::Confirm);
        act(&mut setup, ctx, Action::Settings);
        assert_eq!(setup.step(), 3, "NES needs no emulator, so step 2 is passed over");
        assert_eq!(setup.back_label(), "Back");
        assert!(matches!(act(&mut setup, ctx, Action::Back), Transition::None));
        assert_eq!(setup.step(), 1);
        let ticked: Vec<_> = setup.model.ticked().map(|s| s.id).collect();
        assert_eq!(ticked, ["nes"], "the ticks survive going back");
    }
}
