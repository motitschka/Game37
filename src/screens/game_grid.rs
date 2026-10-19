//! A titled grid of tall covers for a list of Games: "See all" on Home, and a System's Console
//! Page (FR-CAR-3), after `screen_console_page` in `design/generate_mockups.py`.

use crate::ctx::Ctx;
use crate::library::db::{DbError, GameRow, Library, SortOrder};
use crate::ui::chrome;
use crate::ui::cover::{self, COVER_GAP, COVER_H, COVER_W, RING_REACH};
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer, glyph_label};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{BG, Colour, LINE, MUTED, SURFACE, TEXT};
use crate::ui::{M, Rect, W, hit, scroll};

/// Where a grid's Games come from; called again whenever the library changes.
pub type Source = Box<dyn Fn(&Library) -> Result<Vec<GameRow>, DbError>>;

/// A Console Page's extras, absent on Home's "See all": the colour tint, the sort chip, and the
/// empty-state hint pointing at the System's folder (FR-CAR-4).
struct Extra {
    tint: Colour,
    empty_hint: String,
}

const COLS: usize = 8;
const TOP: f32 = 288.0;
const BOTTOM: f32 = 1010.0;
const PITCH_X: f32 = COVER_W + COVER_GAP;
const PITCH_Y: f32 = COVER_H + 72.0;
/// Pixels scrolled per wheel notch.
const WHEEL_STEP: f32 = PITCH_Y / 2.0;
/// The sort chip, after `screen_console_page` in `design/generate_mockups.py`.
const CHIP_Y: f32 = 168.0;
const CHIP_H: f32 = 52.0;
const CHIP_PAD: f32 = 20.0;
const ARROW_W: f32 = 10.0;

/// The focus after moving one step in `dir` in a grid of `len` items, `cols` per row. Left and
/// Right follow reading order and stop at the ends; Down onto a shorter last row lands on its last
/// item.
pub fn step(focus: usize, len: usize, cols: usize, dir: Action) -> usize {
    if len == 0 {
        return 0;
    }
    match dir {
        Action::Left => focus.saturating_sub(1),
        Action::Right => (focus + 1).min(len - 1),
        Action::Up if focus >= cols => focus - cols,
        Action::Down if focus / cols < (len - 1) / cols => (focus + cols).min(len - 1),
        _ => focus,
    }
}

/// Where focus goes after the Games were reloaded: the same Game if it is still there, else the
/// same position, clamped.
pub fn refocus(old: &[GameRow], focus: usize, new: &[GameRow]) -> usize {
    old.get(focus)
        .and_then(|g| new.iter().position(|n| n.id == g.id))
        .unwrap_or(focus.min(new.len().saturating_sub(1)))
}

/// The header line under the title: "<n> games", singular when one, with the sort order appended
/// for a Console Page (FR-CAR-5).
fn header(count: usize, order: Option<SortOrder>) -> String {
    let games = match count {
        1 => "1 game".to_owned(),
        n => format!("{n} games"),
    };
    match order {
        Some(order) => format!("{games} · Sorted by {}", order.label()),
        None => games,
    }
}

pub struct GameGrid {
    title: String,
    source: Source,
    games: Vec<GameRow>,
    /// The `Ctx::revision` the Games were loaded at.
    revision: Option<u64>,
    focus: usize,
    scroll: f32,
    hits: Vec<(Rect, usize)>,
    extra: Option<Extra>,
    /// Where the sort chip was last drawn, for the mouse.
    sort_hit: Option<Rect>,
    /// The global sort order, cached from `reload()` so `draw()` never queries the database
    /// (NFR-3). Meaningless when `extra` is `None`.
    sort_order: SortOrder,
}

impl GameGrid {
    pub fn new(title: impl Into<String>, source: Source) -> GameGrid {
        GameGrid {
            title: title.into(),
            source,
            games: Vec::new(),
            revision: None,
            focus: 0,
            scroll: 0.0,
            hits: Vec::new(),
            extra: None,
            sort_hit: None,
            sort_order: SortOrder::default(),
        }
    }

    /// Turns this into a Console Page: a `tint` from the System's colour, a sort chip, and
    /// `empty_hint` shown in place of the grid when the System has no Games (FR-CAR-4, FR-CAR-5).
    pub fn sortable(mut self, tint: Colour, empty_hint: String) -> GameGrid {
        self.extra = Some(Extra { tint, empty_hint });
        self
    }

    fn reload(&mut self, ctx: &Ctx) {
        let games = (self.source)(&ctx.library).unwrap_or_else(|e| {
            eprintln!("game37: {e}");
            Vec::new()
        });
        self.focus = refocus(&self.games, self.focus, &games);
        self.games = games;
        self.revision = Some(ctx.revision);
        // The previous frame's hit rects may point at Games that moved or went: drop them until the
        // next draw.
        self.hits.clear();
        self.sort_hit = None;
        if self.extra.is_some() {
            self.sort_order = ctx.library.sort_order().unwrap_or_default();
        }
        self.scroll = scroll::clamp(self.scroll, self.content_h(), BOTTOM - TOP);
        self.reveal_focus();
    }

    fn reload_if_changed(&mut self, ctx: &Ctx) {
        if self.revision != Some(ctx.revision) {
            self.reload(ctx);
        }
    }

    /// Cycles the global sort order and reloads (FR-CAR-5); a no-op on an empty Console Page, where
    /// it would change the order with no visible effect.
    fn cycle_sort(&mut self, ctx: &mut Ctx) {
        if self.extra.is_none() || self.games.is_empty() {
            return;
        }
        let next = self.sort_order.next();
        if let Err(e) = ctx.library.set_sort_order(next) {
            eprintln!("game37: can't save the sort order: {e}");
        }
        self.sort_order = next;
        self.reload(ctx);
    }

    fn content_h(&self) -> f32 {
        self.games.len().div_ceil(COLS) as f32 * PITCH_Y
    }

    fn reveal_focus(&mut self) {
        let row = (self.focus / COLS) as f32;
        let s = scroll::keep_visible(
            self.scroll,
            row * PITCH_Y - RING_REACH,
            COVER_H + 2.0 * RING_REACH + 40.0,
            BOTTOM - TOP,
        );
        self.scroll = scroll::clamp(s, self.content_h(), BOTTOM - TOP);
    }

    /// The sort chip, right-aligned next to the title; returns where it was drawn.
    fn draw_sort_chip(&self, gfx: &mut Gfx<'_>, order: SortOrder, ctx: &Ctx) -> Rect {
        let glyph_label = glyph_label(Action::Sort, ctx.input_kind, ctx.button_style);
        let bold = TextStyle::new(19, Weight::Bold, TEXT);
        let muted = TextStyle::new(19, Weight::Medium, MUTED);
        let label = order.label();
        let glyph_w = chrome::glyph_width(gfx, glyph_label);
        let sort_w = gfx.measure("Sort:", muted);
        let label_w = gfx.measure(label, bold);
        let chip_w = CHIP_PAD + glyph_w + 10.0 + sort_w + 8.0 + label_w + 14.0 + ARROW_W + CHIP_PAD;
        let r = Rect::new(W - M - chip_w, CHIP_Y, chip_w, CHIP_H);
        gfx.fill_round_rect(r, CHIP_H / 2.0, SURFACE);
        gfx.stroke_round_rect(r, CHIP_H / 2.0, 1.0, LINE);
        let cy = r.y + r.h / 2.0;
        let mut x = r.x + CHIP_PAD;
        x += chrome::glyph(gfx, x, cy, glyph_label) + 10.0;
        gfx.text(x, cy + 6.0, "Sort:", muted, Align::Left);
        x += sort_w + 8.0;
        gfx.text(x, cy + 6.0, label, bold, Align::Left);
        // A small chevron, drawn rather than a text glyph (not every font covers "▾").
        let ax = r.x + r.w - CHIP_PAD - ARROW_W / 2.0;
        gfx.line(ax - 5.0, cy - 3.0, ax, cy + 3.0, 2.0, MUTED);
        gfx.line(ax, cy + 3.0, ax + 5.0, cy - 3.0, 2.0, MUTED);
        r
    }

    /// The empty-state message in place of the grid (FR-CAR-4, US-11).
    fn draw_empty(&self, gfx: &mut Gfx<'_>) {
        let Some(extra) = &self.extra else { return };
        gfx.text(
            W / 2.0,
            TOP + 140.0,
            "No games yet",
            TextStyle::new(28, Weight::Bold, TEXT),
            Align::Centre,
        );
        gfx.text(
            W / 2.0,
            TOP + 180.0,
            &extra.empty_hint,
            TextStyle::new(20, Weight::Medium, MUTED),
            Align::Centre,
        );
    }
}

impl Screen for GameGrid {
    fn title(&self) -> &str {
        &self.title
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        self.reload_if_changed(ctx);
        match input {
            Input::Action(Action::Back) => return Transition::Pop,
            Input::Action(Action::Sort) if self.extra.is_some() => self.cycle_sort(ctx),
            Input::Action(dir) if dir.is_direction() => {
                self.focus = step(self.focus, self.games.len(), COLS, dir);
                self.reveal_focus();
            }
            Input::Pointer(Pointer::Click { x, y }) if self.sort_hit.is_some_and(|r| r.contains(x, y)) => {
                self.cycle_sort(ctx);
            }
            Input::Pointer(Pointer::Move { x, y }) | Input::Pointer(Pointer::Click { x, y }) => {
                // Confirm on a Game does nothing until launching exists, so a click only focuses.
                if let Some(i) = hit(&self.hits, x, y) {
                    self.focus = i.min(self.games.len().saturating_sub(1));
                }
            }
            Input::Pointer(Pointer::Wheel { dy }) => {
                self.scroll = scroll::clamp(self.scroll - dy * WHEEL_STEP, self.content_h(), BOTTOM - TOP);
            }
            _ => {}
        }
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.reload_if_changed(ctx);
        let mut title_x = M;
        if let Some(extra) = &self.extra {
            chrome::background_tinted(gfx, extra.tint, 0.35);
            gfx.fill_round_rect(Rect::new(M, 152.0, 8.0, 76.0), 4.0, extra.tint);
            title_x += 30.0;
        }
        gfx.text(
            title_x,
            202.0,
            &self.title,
            TextStyle::new(52, Weight::ExtraBold, TEXT),
            Align::Left,
        );
        self.sort_hit = None;
        // An empty Console Page shows no header line, sort chip or grid: sorting it would change
        // the global order with no visible effect.
        let empty_console_page = self.extra.is_some() && self.games.is_empty();
        if !empty_console_page {
            let order = self.extra.is_some().then_some(self.sort_order);
            gfx.text(
                title_x + 2.0,
                236.0,
                &header(self.games.len(), order),
                TextStyle::new(20, Weight::Medium, MUTED),
                Align::Left,
            );
            if let Some(order) = order {
                self.sort_hit = Some(self.draw_sort_chip(gfx, order, ctx));
            }
        }

        self.hits.clear();
        if empty_console_page {
            self.draw_empty(gfx);
            return;
        }
        let clip_top = TOP - RING_REACH - 4.0;
        gfx.clip(Some(Rect::new(0.0, clip_top, W, BOTTOM - clip_top)));
        let first_row = ((self.scroll - RING_REACH) / PITCH_Y).floor().max(0.0) as usize;
        let last_row = ((self.scroll + BOTTOM - TOP) / PITCH_Y).ceil() as usize;
        let caption = TextStyle::new(17, Weight::Medium, MUTED);
        for i in first_row * COLS..((last_row + 1) * COLS).min(self.games.len()) {
            let (row, col) = (i / COLS, i % COLS);
            let r = Rect::new(
                M + col as f32 * PITCH_X,
                TOP + row as f32 * PITCH_Y - self.scroll,
                COVER_W,
                COVER_H,
            );
            let game = &self.games[i];
            cover::draw(gfx, r, game, true, i == self.focus);
            if game.playtime_secs > 0 {
                gfx.text(
                    r.x,
                    r.y + COVER_H + 32.0,
                    &cover::playtime_label(game.playtime_secs),
                    caption,
                    Align::Left,
                );
            }
            self.hits.push((
                Rect::new(
                    r.x,
                    r.y.max(clip_top),
                    r.w,
                    (r.y + r.h).min(BOTTOM) - r.y.max(clip_top),
                ),
                i,
            ));
        }
        if self.content_h() - self.scroll > BOTTOM - TOP {
            gfx.fill_round_rect_vgradient(Rect::new(0.0, 900.0, W, BOTTOM - 900.0), 0.0, BG.alpha(0.0), BG);
        }
        gfx.clip(None);
        if self.games.is_empty() {
            gfx.text(
                M,
                TOP + 40.0,
                "No games here yet",
                TextStyle::new(22, Weight::Medium, MUTED),
                Align::Left,
            );
        }
    }

    fn hints(&self) -> Vec<Hint> {
        let mut hints = Vec::new();
        if self.extra.is_some() && !self.games.is_empty() {
            hints.push(Hint::new(&[Action::Sort], "Sort"));
        }
        hints.push(Hint::new(&[Action::Back], "Back"));
        hints
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64) -> GameRow {
        GameRow {
            id,
            system: "snes".into(),
            rel_path: format!("snes/{id}.sfc"),
            title: format!("Game {id}"),
            date_added: 0,
            last_played: None,
            playtime_secs: 0,
            favorite: false,
        }
    }

    #[test]
    fn header_is_singular_for_one_game_and_appends_the_sort_order() {
        assert_eq!(header(0, None), "0 games");
        assert_eq!(header(1, None), "1 game");
        assert_eq!(header(6, None), "6 games");
        assert_eq!(header(6, Some(SortOrder::AZ)), "6 games · Sorted by A–Z");
        assert_eq!(
            header(1, Some(SortOrder::MostPlayed)),
            "1 game · Sorted by Most played"
        );
    }

    #[test]
    fn left_and_right_follow_reading_order_and_stop_at_the_ends() {
        assert_eq!(step(0, 20, 8, Action::Left), 0);
        assert_eq!(step(7, 20, 8, Action::Right), 8, "wraps onto the next row");
        assert_eq!(step(8, 20, 8, Action::Left), 7);
        assert_eq!(step(19, 20, 8, Action::Right), 19);
    }

    #[test]
    fn up_and_down_move_by_a_row() {
        assert_eq!(step(3, 20, 8, Action::Down), 11);
        assert_eq!(step(11, 20, 8, Action::Up), 3);
        assert_eq!(step(3, 20, 8, Action::Up), 3, "the top row stays");
        assert_eq!(step(17, 20, 8, Action::Down), 17, "the last row stays");
    }

    #[test]
    fn down_onto_a_shorter_last_row_lands_on_its_last_game() {
        assert_eq!(step(14, 20, 8, Action::Down), 19);
        assert_eq!(step(6, 20, 8, Action::Down), 14);
    }

    #[test]
    fn an_empty_grid_has_nothing_to_move_to() {
        assert_eq!(step(0, 0, 8, Action::Right), 0);
        assert_eq!(step(0, 0, 8, Action::Down), 0);
    }

    #[test]
    fn reload_keeps_focus_on_the_same_game() {
        let old: Vec<_> = (1..=5).map(row).collect();
        let new: Vec<_> = [9, 4, 1, 2].into_iter().map(row).collect();
        assert_eq!(refocus(&old, 3, &new), 1, "Game 4 moved to index 1");
        assert_eq!(refocus(&old, 4, &new), 3, "Game 5 is gone: clamp");
        assert_eq!(refocus(&old, 0, &[]), 0);
    }

    use crate::library::FoundGame;

    /// A Game sorting after "A..", so a System's Console Page starting with these keeps a
    /// predictable A–Z position when earlier-sorting Games are added.
    fn found(n: usize) -> FoundGame {
        FoundGame {
            system: "snes",
            rel_path: format!("snes/b{n:03}.sfc"),
            title: format!("B{n:03}"),
        }
    }

    fn console_source() -> Source {
        Box::new(|library| library.system_games("snes", SortOrder::AZ))
    }

    #[test]
    fn reload_reveals_a_focused_game_that_moved_far_down_the_grid() {
        let mut ctx = Ctx::empty();
        let initial: Vec<_> = (0..50).map(found).collect();
        ctx.library.sync_system("snes", &initial, 0).unwrap();
        let mut grid = GameGrid::new("NES", console_source()).sortable(Colour::hex(0), "hint".into());
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        grid.focus = 40; // row 5
        grid.reveal_focus();

        // 40 Games sorting earlier are added: the focused Game (old index 40) moves to index 80.
        let earlier = (0..40).map(|n| FoundGame {
            system: "snes",
            rel_path: format!("snes/a{n:03}.sfc"),
            title: format!("A{n:03}"),
        });
        let all: Vec<_> = initial.into_iter().chain(earlier).collect();
        ctx.library.sync_system("snes", &all, 0).unwrap();
        ctx.revision += 1;

        grid.handle(Input::Action(Action::Back), &mut ctx);
        assert_eq!(grid.focus, 80, "the focused Game moved to row 10");
        let row_top = (grid.focus / COLS) as f32 * PITCH_Y;
        assert!(
            row_top - grid.scroll >= 0.0 && row_top - grid.scroll <= BOTTOM - TOP,
            "row 10 should be scrolled into view, but scroll is {}",
            grid.scroll
        );
    }

    #[test]
    fn cycle_sort_updates_the_cached_sort_order() {
        let mut ctx = Ctx::empty();
        ctx.library.sync_system("snes", &[found(0)], 0).unwrap();
        let mut grid = GameGrid::new("NES", console_source()).sortable(Colour::hex(0), "hint".into());
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        assert_eq!(grid.sort_order, SortOrder::AZ);

        grid.handle(Input::Action(Action::Sort), &mut ctx);

        assert_eq!(grid.sort_order, SortOrder::LastPlayed);
        assert_eq!(ctx.library.sort_order().unwrap(), SortOrder::LastPlayed);
    }

    #[test]
    fn sort_is_ignored_on_an_empty_console_page() {
        let mut ctx = Ctx::empty();
        let mut grid = GameGrid::new("NES", console_source()).sortable(Colour::hex(0), "hint".into());
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load, no games
        assert!(grid.games.is_empty());

        grid.handle(Input::Action(Action::Sort), &mut ctx);

        assert_eq!(
            ctx.library.sort_order().unwrap(),
            SortOrder::AZ,
            "the global order is unchanged"
        );
        assert_eq!(grid.sort_order, SortOrder::AZ);
    }

    #[test]
    fn a_click_that_lands_in_the_reload_frame_is_ignored() {
        let mut ctx = Ctx::empty();
        let many: Vec<_> = (0..20).map(found).collect();
        ctx.library.sync_system("snes", &many, 0).unwrap();
        let mut grid = GameGrid::new("NES", console_source()).sortable(Colour::hex(0), "hint".into());
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        assert_eq!(grid.games.len(), 20);
        // The previous frame's hit rect for item 10.
        grid.hits = vec![(Rect::new(0.0, 0.0, 50.0, 50.0), 10)];
        grid.focus = 0;

        // The System shrinks to 3 Games, and a click lands where item 10 used to be.
        let shrunk: Vec<_> = (0..3).map(found).collect();
        ctx.library.sync_system("snes", &shrunk, 0).unwrap();
        ctx.revision += 1;
        grid.handle(Input::Pointer(Pointer::Move { x: 10.0, y: 10.0 }), &mut ctx);
        grid.handle(Input::Pointer(Pointer::Click { x: 10.0, y: 10.0 }), &mut ctx);

        assert_eq!(grid.games.len(), 3, "the grid did reload");
        assert_eq!(
            grid.focus, 0,
            "the stale click was ignored, not focusing a gone item"
        );
    }
}
