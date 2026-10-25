//! Settings → Library → Hidden games (US-15): every present Hidden Game with its cover, Display
//! Title and System. Confirm unhides the focused one, which returns to its views.

use super::TOP;
use crate::ctx::Ctx;
use crate::library::db::GameRow;
use crate::systems;
use crate::ui::cover::{self, ellipsize};
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::scroll;
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, MUTED, SURFACE, SURFACE2, TEXT};
use crate::ui::{M, Rect, W, hit};

const ROW_H: f32 = 112.0;
const ROW_PITCH: f32 = 124.0;
/// The list's bottom edge, above the hint bar.
const BOTTOM: f32 = 990.0;
const THUMB_W: f32 = 64.0;
const THUMB_H: f32 = 96.0;

pub struct HiddenGames {
    games: Vec<GameRow>,
    focus: usize,
    scroll: f32,
}

impl HiddenGames {
    pub fn new(ctx: &Ctx) -> HiddenGames {
        let mut screen = HiddenGames {
            games: Vec::new(),
            focus: 0,
            scroll: 0.0,
        };
        screen.reload(ctx);
        screen
    }

    /// Re-reads the Hidden Games, keeping the focus at the same place in the list.
    fn reload(&mut self, ctx: &Ctx) {
        self.games = ctx.library.hidden_games().unwrap_or_else(|e| {
            eprintln!("game37: {e}");
            Vec::new()
        });
        self.focus = self.focus.min(self.games.len().saturating_sub(1));
    }

    fn move_focus(&mut self, dir: isize) {
        let last = self.games.len().saturating_sub(1) as isize;
        self.focus = (self.focus as isize + dir).clamp(0, last) as usize;
    }

    fn unhide(&mut self, i: usize, ctx: &mut Ctx) {
        let Some(game) = self.games.get(i) else {
            return;
        };
        if let Err(e) = ctx.library.set_hidden(game.id, false) {
            eprintln!("game37: {e}");
        }
        ctx.revision += 1;
        self.focus = i;
        self.reload(ctx);
    }

    /// Where row `i` is drawn at the current scroll.
    fn row_rect(&self, i: usize) -> Rect {
        Rect::new(M, TOP + i as f32 * ROW_PITCH - self.scroll, W - 2.0 * M, ROW_H)
    }

    fn row_hits(&self) -> Vec<(Rect, usize)> {
        (0..self.games.len())
            .map(|i| (self.row_rect(i), i))
            .filter(|(r, _)| r.y + r.h > TOP && r.y < BOTTOM)
            .collect()
    }
}

impl Screen for HiddenGames {
    fn title(&self) -> &str {
        "Hidden games"
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        match input {
            Input::Action(Action::Up) => self.move_focus(-1),
            Input::Action(Action::Down) => self.move_focus(1),
            Input::Action(Action::Confirm) => self.unhide(self.focus, ctx),
            Input::Action(Action::Back) => return Transition::Pop,
            Input::Pointer(Pointer::Wheel { dy }) => self.move_focus(if dy > 0.0 { -1 } else { 1 }),
            Input::Pointer(Pointer::Click { x, y }) => match hit(&self.row_hits(), x, y) {
                Some(i) if i == self.focus => self.unhide(i, ctx),
                Some(i) => self.focus = i,
                None => {}
            },
            _ => {}
        }
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        let heading = TextStyle::new(48, Weight::ExtraBold, TEXT);
        gfx.text(M, 196.0, "Hidden games", heading, Align::Left);
        if self.games.is_empty() {
            let style = TextStyle::new(22, Weight::Medium, MUTED);
            let text = "No hidden Games. Hide a Game from its Options Menu.";
            gfx.text(M, TOP + 40.0, text, style, Align::Left);
            return;
        }
        let content = self.games.len() as f32 * ROW_PITCH - (ROW_PITCH - ROW_H);
        let view = BOTTOM - TOP;
        let start = self.focus as f32 * ROW_PITCH;
        self.scroll = scroll::clamp(
            scroll::keep_visible(self.scroll, start, ROW_H, view),
            content,
            view,
        );
        gfx.clip(Some(Rect::new(0.0, TOP - 16.0, W, view + 16.0)));
        for (r, i) in self.row_hits() {
            let game = &self.games[i];
            let focused = i == self.focus;
            if focused {
                gfx.fill_round_rect(r, 14.0, SURFACE2);
                gfx.fill_round_rect(Rect::new(r.x, r.y + r.h / 2.0 - 20.0, 5.0, 40.0), 2.0, ACCENT);
            } else {
                gfx.fill_round_rect(r, 14.0, SURFACE.alpha(0.6));
            }
            let thumb = Rect::new(r.x + 32.0, r.y + (r.h - THUMB_H) / 2.0, THUMB_W, THUMB_H);
            let art = ctx.artwork.cover(&ctx.library, game);
            cover::draw(gfx, thumb, game, &art, false, false);
            let x = thumb.x + THUMB_W + 28.0;
            let weight = if focused { Weight::Bold } else { Weight::SemiBold };
            let title = TextStyle::new(22, weight, TEXT);
            let label = ellipsize(&game.title, r.x + r.w - 32.0 - x, |s| gfx.measure(s, title));
            gfx.text(x, r.y + r.h / 2.0 - 6.0, &label, title, Align::Left);
            let system = systems::by_id(&game.system).map_or("", |s| s.name);
            let meta = TextStyle::new(18, Weight::Medium, MUTED);
            gfx.text(x, r.y + r.h / 2.0 + 26.0, system, meta, Align::Left);
        }
        gfx.clip(None);
    }

    fn hints(&self) -> Vec<Hint> {
        let back = Hint::new(&[Action::Back], "Back");
        if self.games.is_empty() {
            vec![back]
        } else {
            vec![Hint::new(&[Action::Confirm], "Unhide"), back]
        }
    }
}
