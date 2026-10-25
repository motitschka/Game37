//! Step 1, "Pick your Systems": every System as a tile with a checkbox, after
//! `screen_setup_systems` in `design/generate_mockups.py`.

use super::{Button, CONTENT_Y, Go, Model, PILL_H, Step, checkbox, system_chip};
use crate::ctx::Ctx;
use crate::emulators;
use crate::systems::{Kind, SYSTEMS, System};
use crate::ui::chrome::glyph;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer, glyph_label};
use crate::ui::nav::Hint;
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, LINE, MUTED, SURFACE, SURFACE2, TEXT};
use crate::ui::{M, Rect, W, hit};

const COLS: usize = 6;
const GAP: f32 = 24.0;
const TILE_H: f32 = 148.0;
const TILE_W: f32 = (W - 2.0 * M - (COLS - 1) as f32 * GAP) / COLS as f32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    /// A System's tile, by its index in `SYSTEMS`.
    Tile(usize),
    SelectAll,
    Button(Button),
}

pub struct Systems {
    focus: Focus,
    /// The tile Up from the bottom row returns to.
    last_tile: usize,
    tile_hits: Vec<(Rect, usize)>,
    select_all_hit: Option<Rect>,
}

impl Systems {
    pub fn new() -> Systems {
        Systems {
            focus: Focus::Tile(0),
            last_tile: 0,
            tile_hits: Vec::new(),
            select_all_hit: None,
        }
    }

    fn focus_tile(&mut self, i: usize) {
        self.focus = Focus::Tile(i);
        self.last_tile = i;
    }

    fn move_focus(&mut self, action: Action) {
        let n = SYSTEMS.len();
        let bottom = [
            Focus::SelectAll,
            Focus::Button(Button::Back),
            Focus::Button(Button::Continue),
        ];
        match self.focus {
            Focus::Tile(i) => {
                let col = i % COLS;
                match action {
                    Action::Left if col > 0 => self.focus_tile(i - 1),
                    Action::Right if col + 1 < COLS && i + 1 < n => self.focus_tile(i + 1),
                    Action::Up if i >= COLS => self.focus_tile(i - COLS),
                    Action::Down if i + COLS < n => self.focus_tile(i + COLS),
                    // From the row above the last, onto the last row's end.
                    Action::Down if i / COLS < (n - 1) / COLS => self.focus_tile(n - 1),
                    Action::Down if col < COLS / 2 => self.focus = Focus::SelectAll,
                    Action::Down => self.focus = Focus::Button(Button::Continue),
                    _ => {}
                }
            }
            _ => {
                let at = bottom.iter().position(|&f| f == self.focus).unwrap_or(0);
                match action {
                    Action::Left if at > 0 => self.focus = bottom[at - 1],
                    Action::Right if at + 1 < bottom.len() => self.focus = bottom[at + 1],
                    Action::Up => self.focus = Focus::Tile(self.last_tile),
                    _ => {}
                }
            }
        }
    }

    fn select_all(model: &mut Model) {
        model.ticked.iter_mut().for_each(|on| *on = true);
    }

    fn confirm(&mut self, model: &mut Model) -> Go {
        match self.focus {
            Focus::Tile(i) => model.ticked[i] = !model.ticked[i],
            Focus::SelectAll => Systems::select_all(model),
            Focus::Button(Button::Back) => return Go::Back,
            Focus::Button(Button::Continue) => return Go::Next,
        }
        Go::Stay
    }

    fn draw_tile(&self, gfx: &mut Gfx<'_>, r: Rect, system: &System, on: bool, focused: bool) {
        if focused {
            gfx.stroke_round_rect(r.inset(-6.0), 20.0, 4.0, ACCENT);
        }
        gfx.fill_round_rect(r, 14.0, if on { SURFACE2 } else { SURFACE });
        gfx.stroke_round_rect(r, 14.0, 1.0, LINE);
        system_chip(gfx, r.x + 20.0, r.y + 20.0, 52.0, system);
        checkbox(gfx, r.x + r.w - 50.0, r.y + 31.0, on);
        let name = TextStyle::new(21, Weight::Bold, TEXT);
        gfx.text(r.x + 20.0, r.y + 106.0, system.name, name, Align::Left);
        let sub = TextStyle::new(16, Weight::Medium, MUTED);
        gfx.text(r.x + 20.0, r.y + 132.0, &plays_in(system), sub, Align::Left);
    }
}

/// What a tile says about where the System plays.
fn plays_in(system: &System) -> String {
    match (system.kind, emulators::for_system(system)) {
        (Kind::External, Some(emulator)) => format!("Needs {}", emulator.name),
        _ => "Plays in Game37".to_owned(),
    }
}

/// "8 Systems selected".
fn selected_label(n: usize) -> String {
    match n {
        1 => "1 System selected".to_owned(),
        n => format!("{n} Systems selected"),
    }
}

impl Step for Systems {
    fn title(&self) -> &'static str {
        "Pick your Systems"
    }

    fn explanation(&self) -> &'static str {
        "Tick the Systems you have Games for. You can add folders for others later."
    }

    fn can_continue(&self, model: &Model) -> bool {
        model.ticked().next().is_some()
    }

    fn handle(&mut self, input: Input, model: &mut Model, _: &mut Ctx) -> Go {
        match input {
            Input::Action(Action::Sort) => Systems::select_all(model),
            Input::Action(Action::Confirm) => return self.confirm(model),
            Input::Action(action) if action.is_direction() => self.move_focus(action),
            Input::Pointer(Pointer::Move { x, y }) => {
                if let Some(i) = hit(&self.tile_hits, x, y) {
                    self.focus_tile(i);
                }
            }
            Input::Pointer(Pointer::Click { x, y }) => {
                if let Some(i) = hit(&self.tile_hits, x, y) {
                    self.focus_tile(i);
                    return self.confirm(model);
                }
                if self.select_all_hit.is_some_and(|r| r.contains(x, y)) {
                    Systems::select_all(model);
                }
            }
            _ => {}
        }
        Go::Stay
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, model: &Model, ctx: &Ctx) {
        self.tile_hits.clear();
        for (i, system) in SYSTEMS.iter().enumerate() {
            let (row, col) = (i / COLS, i % COLS);
            let x = M + col as f32 * (TILE_W + GAP);
            let y = CONTENT_Y + row as f32 * (TILE_H + GAP);
            let r = Rect::new(x, y, TILE_W, TILE_H);
            self.draw_tile(gfx, r, system, model.ticked[i], self.focus == Focus::Tile(i));
            self.tile_hits.push((r, i));
        }

        // The Select all pill, as `action_pill` draws it, with X's glyph.
        let label = TextStyle::new(22, Weight::Bold, TEXT);
        let y = super::BUTTONS_Y;
        let w = 28.0 + 46.0 + gfx.measure("Select all", label) + 28.0;
        let r = Rect::new(M, y, w, PILL_H);
        if self.focus == Focus::SelectAll {
            gfx.stroke_round_rect(r.inset(-6.0), r.h / 2.0 + 6.0, 3.0, super::WHITE);
        }
        gfx.fill_round_rect(r, r.h / 2.0, SURFACE.alpha(0.85));
        gfx.stroke_round_rect(r, r.h / 2.0, 2.0, LINE);
        glyph(
            gfx,
            M + 28.0,
            y + 32.0,
            glyph_label(Action::Sort, ctx.input_kind, ctx.button_style),
        );
        gfx.text(M + 74.0, y + 40.0, "Select all", label, Align::Left);
        self.select_all_hit = Some(r);
        let count = selected_label(model.ticked().count());
        let muted = TextStyle::new(20, Weight::Medium, MUTED);
        gfx.text(M + w + 24.0, y + 39.0, &count, muted, Align::Left);
    }

    fn hints(&self, _: &Model) -> Vec<Hint> {
        let a = match self.focus {
            Focus::Tile(_) => "Tick",
            Focus::SelectAll => "Select all",
            Focus::Button(Button::Back) => "Quit",
            Focus::Button(Button::Continue) => "Continue",
        };
        vec![
            Hint::new(&[Action::Confirm], a),
            Hint::new(&[Action::Sort], "Select all"),
        ]
    }

    fn focused_button(&self) -> Option<Button> {
        match self.focus {
            Focus::Button(b) => Some(b),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Setup;
    use super::super::tests::act;
    use super::*;
    use std::path::PathBuf;

    fn ticked(setup: &Setup) -> Vec<&'static str> {
        setup.model.ticked().map(|s| s.id).collect()
    }

    #[test]
    fn a_ticks_and_unticks_the_focused_tile_and_none_start_ticked() {
        let ctx = &mut Ctx::empty();
        let mut setup = Setup::new(PathBuf::from("/g"));
        assert!(ticked(&setup).is_empty());
        act(&mut setup, ctx, Action::Right);
        act(&mut setup, ctx, Action::Right);
        act(&mut setup, ctx, Action::Confirm);
        act(&mut setup, ctx, Action::Down);
        act(&mut setup, ctx, Action::Confirm);
        assert_eq!(ticked(&setup), ["snes", "wii"], "in System Order");
        act(&mut setup, ctx, Action::Confirm);
        assert_eq!(ticked(&setup), ["snes"]);
    }

    #[test]
    fn select_all_ticks_every_system() {
        let ctx = &mut Ctx::empty();
        let mut setup = Setup::new(PathBuf::from("/g"));
        act(&mut setup, ctx, Action::Confirm);
        act(&mut setup, ctx, Action::Sort);
        assert_eq!(ticked(&setup).len(), SYSTEMS.len());
        assert_eq!(selected_label(SYSTEMS.len()), "17 Systems selected");
        assert_eq!(selected_label(1), "1 System selected");
    }

    #[test]
    fn continue_is_blocked_until_a_system_is_ticked() {
        let ctx = &mut Ctx::empty();
        let mut setup = Setup::new(PathBuf::from("/g"));
        // Down to the bottom row's Continue button, then A.
        for _ in 0..3 {
            act(&mut setup, ctx, Action::Down);
        }
        act(&mut setup, ctx, Action::Right);
        act(&mut setup, ctx, Action::Right);
        act(&mut setup, ctx, Action::Confirm);
        assert_eq!(setup.step(), 1, "nothing is ticked");
        act(&mut setup, ctx, Action::Up);
        act(&mut setup, ctx, Action::Confirm);
        act(&mut setup, ctx, Action::Down);
        act(&mut setup, ctx, Action::Right);
        act(&mut setup, ctx, Action::Right);
        act(&mut setup, ctx, Action::Confirm);
        assert_eq!(
            setup.step(),
            3,
            "PlayStation needs no emulator, so step 2 is passed over"
        );
    }

    #[test]
    fn tiles_say_where_each_system_plays() {
        let by_id = |id| SYSTEMS.iter().find(|s| s.id == id).unwrap();
        assert_eq!(plays_in(by_id("nes")), "Plays in Game37");
        assert_eq!(plays_in(by_id("gc")), "Needs Dolphin");
        assert_eq!(plays_in(by_id("ps4")), "Needs shadPS4");
    }
}
