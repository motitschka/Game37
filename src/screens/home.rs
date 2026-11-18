//! Home: the Shelves the player switched on, in their order (FR-HOME-1 to FR-HOME-5, FR-HOME-7),
//! after `home` and `screen_home_empty` in `design/generate_mockups.py`.

use std::path::Path;

use super::details::DetailsPage;
use super::game_grid::GameGrid;
use super::options::OptionsMenu;
use crate::ctx::Ctx;
use crate::library::db::{GameRow, SHELF_SIZE, Shelf};
use crate::ui::anim::{Eased, FOCUS_TAU, SCROLL_TAU};
use crate::ui::chrome;
use crate::ui::cover::{self, COVER_GAP, COVER_RADIUS, CoverPlace, RING_REACH, ellipsize};
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer, glyph_label};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::scroll::{self, Notches};
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, BG, LINE, MUTED, SURFACE, SURFACE2, TEXT};
use crate::ui::{M, Rect, W, hit};

/// The most Shelves Home shows: the per-Shelf arrays are this long, and only the first
/// `Home::shelves.len()` of each are used.
const SHELVES: usize = Shelf::ALL.len();

/// The visible content area between the top bar and the hint bar, which follows the bottom of the
/// canvas (`ui::h`): 900 tall on a 1080 canvas.
const VIEW_TOP: f32 = 110.0;
fn view_bottom() -> f32 {
    chrome::hintbar_top()
}
fn view_h() -> f32 {
    view_bottom() - VIEW_TOP
}
const VIEW_W: f32 = W - 2.0 * M;
/// How many Shelves one screen of Home shows (FR-HOME-7).
const SHOWN: usize = 2;
/// Distance from the top of a Shelf's block to its title's baseline.
const TITLE_DY: f32 = 46.0;
/// Distance from the top of a Shelf's block to its covers' top (or its hint box's).
const COVERS_DY: f32 = 78.0;
/// The space kept free under the covers, for a line of text under the focused one.
const CAPTION_H: f32 = 60.0;
/// From the focused cover's resting bottom to its Playtime's baseline, under the grown cover (12
/// lower) and its ring (15 further).
const PLAYTIME_DY: f32 = 43.0;
const SEE_ALL_W: f32 = 150.0;
/// The focused cover grows by this much on each side.
const GROW: f32 = 8.0;
/// How wide the fade under a Shelf's paging arrow is.
const EDGE_FADE_W: f32 = 140.0;
/// The hint an empty Shelf shows: the placeholder squares' size and pitch.
const PLACEHOLDER: f32 = 96.0;
const PLACEHOLDER_PITCH: f32 = 116.0;

/// Home's sizes at the canvas height ([`ui::h`](crate::ui::h)): the view holds exactly [`SHOWN`]
/// Shelves whatever its height, so every Shelf's block, filled or empty, is that share of it, and
/// the 2:3 covers are sized from the block (FR-HOME-7, FR-HOME-5).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    view_h: f32,
    block_h: f32,
    cover_w: f32,
    cover_h: f32,
    /// From one cover's left edge to the next one's.
    pitch: f32,
}

impl Layout {
    /// The layout at the current canvas height.
    fn now() -> Layout {
        let view_h = view_h();
        let block_h = view_h / SHOWN as f32;
        let cover_h = block_h - COVERS_DY - CAPTION_H;
        let cover_w = cover_h * 2.0 / 3.0;
        Layout {
            view_h,
            block_h,
            cover_w,
            cover_h,
            pitch: cover_w + COVER_GAP,
        }
    }
}

/// Top of Shelf `i`'s block in content coordinates: every block, filled or empty, is as tall.
fn block_top(i: usize) -> f32 {
    i as f32 * Layout::now().block_h
}

/// The last Shelf `top` can be with `n` Shelves shown: with one, it sits in the upper half.
fn max_top(n: usize) -> usize {
    n.saturating_sub(SHOWN)
}

/// The first Shelf showing once Shelf `shelf` of `n` has focus, from `top`: the view moves by one
/// whole Shelf, and only when the focus leaves the Shelves showing.
fn follow(top: usize, shelf: usize, n: usize) -> usize {
    top.clamp(shelf.saturating_sub(SHOWN - 1), shelf).min(max_top(n))
}

pub fn name(shelf: Shelf) -> &'static str {
    match shelf {
        Shelf::Favorites => "Favorites",
        Shelf::ContinuePlaying => "Continue playing",
        Shelf::RecentlyAdded => "Recently added",
        Shelf::MostPlayed => "Most played",
    }
}

/// "See all" of `shelf` (FR-HOME-3): every Game, starting in the Shelf's order and sortable for this
/// open grid only; on Favorites only the Favorites, in the Shelf's order.
fn see_all(shelf: Shelf) -> GameGrid {
    match shelf.see_all_order() {
        Some(order) => {
            GameGrid::new("All games", Box::new(|library, order| library.all_games(order))).own_sort(order)
        }
        None => GameGrid::new(
            name(shelf),
            Box::new(move |library, _| library.shelf(shelf, None)),
        ),
    }
}

/// What a position on a Shelf holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Game(usize),
    SeeAll,
    /// The hint an empty Shelf shows instead of Games.
    Hint,
}

/// Focusable positions on a Shelf of `games` Games: each Game and See all, or the hint.
pub fn item_count(games: usize) -> usize {
    if games == 0 { 1 } else { games + 1 }
}

pub fn item_at(games: usize, col: usize) -> Item {
    match col {
        _ if games == 0 => Item::Hint,
        c if c < games => Item::Game(c),
        _ => Item::SeeAll,
    }
}

/// Which Shelf has focus, and the position each Shelf remembers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Focus {
    pub shelf: usize,
    pub cols: [usize; SHELVES],
}

impl Focus {
    pub fn col(&self) -> usize {
        self.cols[self.shelf]
    }

    /// Moves one step in `dir` given each shown Shelf's item count. Up and Down go to the
    /// neighbouring Shelf's remembered position. A press of Left on a Shelf's first Game goes round
    /// to its last item, and a press of Right on that item to its first Game (FR-HOME-3); a `repeat`
    /// (the held direction's auto-repeat) stops at either end instead. Returns whether focus moved.
    pub fn step(&mut self, dir: Action, counts: &[usize], repeat: bool) -> bool {
        let before = self.clone();
        match dir {
            Action::Up => self.shelf = self.shelf.saturating_sub(1),
            Action::Down => self.shelf = (self.shelf + 1).min(counts.len() - 1),
            Action::Left if self.col() == 0 && !repeat => self.cols[self.shelf] = counts[self.shelf] - 1,
            Action::Left => self.cols[self.shelf] = self.col().saturating_sub(1),
            Action::Right if self.col() + 1 >= counts[self.shelf] && !repeat => self.cols[self.shelf] = 0,
            Action::Right => self.cols[self.shelf] = self.col() + 1,
            _ => {}
        }
        let s = self.shelf;
        self.cols[s] = self.cols[s].min(counts[s].saturating_sub(1));
        *self != before
    }
}

/// A Shelf's position after its Games were reloaded: the same Game if it is still there, See all
/// if that had focus, else the same position, clamped.
pub fn refocus(old: &[GameRow], col: usize, new: &[GameRow]) -> usize {
    let last = item_count(new.len()) - 1;
    match item_at(old.len(), col) {
        Item::Game(i) => new
            .iter()
            .position(|g| g.id == old[i].id)
            .unwrap_or(col.min(last)),
        Item::SeeAll => last,
        Item::Hint => 0,
    }
}

/// "~/Games" for a Games Root inside the home directory.
pub fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

struct ShelfRow {
    shelf: Shelf,
    games: Vec<GameRow>,
    /// How many Games its "See all" grid shows, beyond the ones on the Shelf.
    total: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    Item(usize, usize),
    /// Page a Shelf left (-1) or right (+1).
    Page(usize, f32),
}

pub struct Home {
    /// The Shelves shown, in `Ctx::home_shelves`' order as last seen.
    shelves: Vec<ShelfRow>,
    /// The `Ctx::revision` the Shelves were loaded at.
    revision: Option<u64>,
    /// Whether Home's choice of the Shelf it opens on is made or dropped: it waits until a Shelf
    /// shown has Games (a first scan may still be running), and is dropped for good once the player
    /// moves the focus or the view (FR-HOME-8).
    opened: bool,
    focus: Focus,
    hscroll: [f32; SHELVES],
    /// The first Shelf showing: Home rests on whole Shelves, `top` and the one below it.
    top: usize,
    /// The wheel's `dy` turned into whole Shelves: a touchpad sends many small ones.
    notches: Notches,
    /// The canvas height `hscroll` was last fitted to; a resize refits it in `draw`.
    fitted_h: f32,
    /// What is drawn: `hscroll` and `top`'s offset eased, so the Shelves glide to the focus.
    shown_h: [Eased; SHELVES],
    shown_v: Eased,
    /// How far the focused cover has grown, 0 to 1: it grows into place after each move.
    grow: Eased,
    /// The item `grow` belongs to.
    grown: Option<(usize, usize)>,
    hits: Vec<(Rect, Target)>,
}

impl Default for Home {
    fn default() -> Self {
        Home::new()
    }
}

impl Home {
    /// The Shelves load from the library on first use, in `Ctx::home_shelves`' order.
    pub fn new() -> Home {
        Home {
            shelves: Shelf::ALL
                .into_iter()
                .map(|shelf| ShelfRow {
                    shelf,
                    games: Vec::new(),
                    total: 0,
                })
                .collect(),
            revision: None,
            opened: false,
            focus: Focus::default(),
            hscroll: [0.0; SHELVES],
            top: 0,
            notches: Notches::default(),
            fitted_h: crate::ui::h(),
            shown_h: std::array::from_fn(|_| Eased::new(0.0)),
            shown_v: Eased::new(0.0),
            grow: Eased::new(1.0),
            grown: None,
            hits: Vec::new(),
        }
    }

    /// Follows `ctx`: the Shelves switched on, then their Games.
    fn sync(&mut self, ctx: &Ctx) {
        self.sync_shelves(ctx);
        self.reload_if_changed(ctx);
    }

    /// Follows a change to which Shelves show, made in Settings while Home was below it
    /// (FR-HOME-2): the Shelves are laid out again in their new order, each one still shown keeping
    /// its position. The focused Shelf keeps focus, on the same Game, if it is still shown; else
    /// the first Shelf has it. Home lands on whole Shelves without a glide.
    fn sync_shelves(&mut self, ctx: &Ctx) {
        let wanted: Vec<Shelf> = ctx.home_shelves.shown().collect();
        if self.shelves.iter().map(|r| r.shelf).eq(wanted.iter().copied()) {
            return;
        }
        // Before the first load nothing had focus yet: Home starts on its first Shelf.
        let focused = self.revision.map(|_| self.shelves[self.focus.shelf].shelf);
        let mut old: Vec<_> = std::mem::take(&mut self.shelves)
            .into_iter()
            .enumerate()
            .collect();
        let (cols, hscroll, shown_h) = (self.focus.cols, self.hscroll, self.shown_h.clone());
        self.focus = Focus::default();
        self.hscroll = [0.0; SHELVES];
        self.shown_h = std::array::from_fn(|_| Eased::new(0.0));
        for (j, &shelf) in wanted.iter().enumerate() {
            let row = match old.iter().position(|(_, row)| row.shelf == shelf) {
                Some(k) => {
                    let (i, row) = old.swap_remove(k);
                    self.focus.cols[j] = cols[i];
                    self.hscroll[j] = hscroll[i];
                    self.shown_h[j] = shown_h[i].clone();
                    row
                }
                None => ShelfRow {
                    shelf,
                    games: Vec::new(),
                    total: 0,
                },
            };
            self.shelves.push(row);
        }
        self.focus.shelf = wanted.iter().position(|&s| Some(s) == focused).unwrap_or(0);
        // Loads the Shelves new to Home, and reloads the others, keeping each one's Game.
        self.revision = None;
        self.hits.clear();
        self.top = follow(self.top, self.focus.shelf, self.shelves.len());
        self.shown_v.jump(self.rest_offset());
    }

    /// Reloads if `ctx.revision` changed since the last load. The previous frame's hit rects may
    /// point at Games that moved or went, so they are dropped until the next draw.
    fn reload_if_changed(&mut self, ctx: &Ctx) {
        if self.revision == Some(ctx.revision) {
            return;
        }
        self.hits.clear();
        for (i, row) in self.shelves.iter_mut().enumerate() {
            let loaded = ctx.library.shelf(row.shelf, Some(SHELF_SIZE)).and_then(|games| {
                let total = match row.shelf.see_all_order() {
                    Some(_) => ctx.library.game_count()?,
                    None => ctx.library.shelf_count(row.shelf)?,
                };
                Ok((games, total))
            });
            let (games, total) = loaded.unwrap_or_else(|e| {
                eprintln!("game37: {e}");
                (Vec::new(), 0)
            });
            self.focus.cols[i] = refocus(&row.games, self.focus.cols[i], &games);
            row.games = games;
            row.total = total;
        }
        self.revision = Some(ctx.revision);
        if !self.opened && self.shelves.iter().any(|row| !row.games.is_empty()) {
            self.opened = true;
            self.open_on_games();
        }
        for i in 0..self.shelves.len() {
            self.reveal(i);
        }
        self.reveal_shelf();
    }

    /// On the first load with Games, while Favorites and Continue playing are empty or not shown, Home opens
    /// on the first Shelf shown that has Games, at the top of the view, so a new player sees their
    /// Games at once (FR-HOME-8). Otherwise, or with no Games at all, it stays on the first Shelf.
    fn open_on_games(&mut self) {
        let played = |row: &ShelfRow| {
            matches!(row.shelf, Shelf::Favorites | Shelf::ContinuePlaying) && !row.games.is_empty()
        };
        if self.shelves.iter().any(played) {
            return;
        }
        if let Some(i) = self.shelves.iter().position(|row| !row.games.is_empty()) {
            self.focus.shelf = i;
            self.top = i.min(max_top(self.shelves.len()));
            self.shown_v.jump(self.rest_offset());
        }
    }

    /// Drops the choice of the Shelf Home opens on once the player moved the focus or the view
    /// from `before` (`focus` and `top`), so Games arriving later never move them (FR-HOME-8).
    fn drop_opening_if_moved(&mut self, before: (Focus, usize)) {
        if self.focus != before.0 || self.top != before.1 {
            self.opened = true;
        }
    }

    fn counts(&self) -> Vec<usize> {
        self.shelves
            .iter()
            .map(|row| item_count(row.games.len()))
            .collect()
    }

    /// Home's vertical offset at rest: the top of the first Shelf showing.
    fn rest_offset(&self) -> f32 {
        block_top(self.top)
    }

    fn content_w(&self, i: usize) -> f32 {
        let n = self.shelves[i].games.len();
        n as f32 * Layout::now().pitch + SEE_ALL_W + RING_REACH
    }

    /// Scrolls Shelf `i` sideways to show its remembered position.
    fn reveal(&mut self, i: usize) {
        let lay = Layout::now();
        let x = self.focus.cols[i] as f32 * lay.pitch - GROW - RING_REACH;
        let s = scroll::keep_visible(
            self.hscroll[i],
            x,
            lay.cover_w + 2.0 * (GROW + RING_REACH),
            VIEW_W,
        );
        self.hscroll[i] = scroll::clamp(s, self.content_w(i), VIEW_W);
    }

    /// Moves Home by a whole Shelf when the focus left the Shelves showing.
    fn reveal_shelf(&mut self) {
        self.top = follow(self.top, self.focus.shelf, self.shelves.len());
    }

    /// The Shelf showing at rest whose block holds `y`, if any.
    fn shelf_at(&self, y: f32) -> Option<usize> {
        if !(VIEW_TOP..view_bottom()).contains(&y) {
            return None;
        }
        let s = self.top + ((y - VIEW_TOP) / Layout::now().block_h) as usize;
        (s < self.shelves.len()).then_some(s)
    }

    /// Scrolls Shelf `i` sideways to `to`, as far as it goes, then [`Home::keep_in_view`].
    fn scroll_shelf(&mut self, i: usize, to: f32) {
        self.hscroll[i] = scroll::clamp(to, self.content_w(i), VIEW_W);
        self.keep_in_view(i);
    }

    /// After Shelf `i` scrolled sideways, its position (the focus, or what it remembers) moves to
    /// the nearest item wholly in view if it went out of view, so the focus stays on screen.
    fn keep_in_view(&mut self, i: usize) {
        let lay = Layout::now();
        let n = self.shelves[i].games.len();
        let hs = self.hscroll[i];
        let shows = |c: usize| {
            let (x, w) = (c as f32 * lay.pitch, if c < n { lay.cover_w } else { SEE_ALL_W });
            x >= hs && x + w <= hs + VIEW_W
        };
        let col = self.focus.cols[i];
        if n == 0 || shows(col) {
            return;
        }
        let near = (0..=n).filter(|&c| shows(c)).min_by_key(|&c| c.abs_diff(col));
        if let Some(c) = near {
            self.focus.cols[i] = c;
        }
    }

    /// Adds a wheel event's `dy` to what came before in the same direction, and moves Home a whole
    /// Shelf for each notch (1.0) of it, keeping the rest: a mouse notch moves one Shelf, and a
    /// touchpad swipe moves by how far it went.
    fn turn_wheel(&mut self, dy: f32) {
        let notches = self.notches.turn(dy);
        for _ in 0..notches.abs() {
            self.wheel(-notches.signum() as isize);
        }
    }

    /// The target under (`x`, `y`) in the last frame's hit rects, on a Shelf showing at rest (`top`
    /// and the one below it): a Shelf sliding out during the snap takes no hover or click.
    fn hit_at_rest(&self, x: f32, y: f32) -> Option<Target> {
        hit(&self.hits, x, y)
            .filter(|&(Target::Item(s, _) | Target::Page(s, _))| (self.top..self.top + SHOWN).contains(&s))
    }

    /// The mouse wheel moves Home by `by` whole Shelves and pulls the focus onto a Shelf showing,
    /// to the position that Shelf remembers.
    fn wheel(&mut self, by: isize) {
        let n = self.shelves.len();
        self.top = self.top.saturating_add_signed(by).min(max_top(n));
        let shelf = self
            .focus
            .shelf
            .clamp(self.top, (self.top + SHOWN - 1).min(n - 1));
        if shelf != self.focus.shelf {
            self.focus.shelf = shelf;
            self.focus.cols[shelf] = self.focus.cols[shelf].min(self.counts()[shelf].saturating_sub(1));
            self.reveal(shelf);
        }
    }

    /// After a resize (`ui::h` changed) the covers' size changed: scrolls each Shelf again so its
    /// remembered position shows, and lands on the new sizes without a glide. Home stays on the
    /// same whole Shelves.
    fn refit(&mut self) {
        if self.fitted_h == crate::ui::h() {
            return;
        }
        self.fitted_h = crate::ui::h();
        for i in 0..self.shelves.len() {
            self.reveal(i);
            self.shown_h[i].jump(self.hscroll[i]);
        }
        self.shown_v.jump(self.rest_offset());
    }

    fn confirm(&self, ctx: &mut Ctx) -> Transition {
        let row = &self.shelves[self.focus.shelf];
        match item_at(row.games.len(), self.focus.col()) {
            Item::Game(i) => Transition::Push(Box::new(DetailsPage::new(row.games[i].clone(), ctx))),
            Item::SeeAll => Transition::Push(Box::new(see_all(row.shelf))),
            Item::Hint => Transition::None,
        }
    }

    /// Y or a right-click on a Game opens its Options Menu (FR-DET-3).
    fn options(&self, ctx: &Ctx) -> Transition {
        let row = &self.shelves[self.focus.shelf];
        match item_at(row.games.len(), self.focus.col()) {
            Item::Game(i) => Transition::Push(Box::new(OptionsMenu::new(row.games[i].clone(), ctx))),
            _ => Transition::None,
        }
    }

    /// Draws Shelf `i`'s Games in its block, whose top is at `top`.
    fn draw_filled(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx, i: usize, top: f32) {
        let lay = Layout::now();
        let focused_shelf = self.focus.shelf == i;
        let col = self.focus.cols[i];
        let y = top + COVERS_DY;
        let row = &self.shelves[i];
        let n = row.games.len();
        let hs = self.shown_h[i].value();
        let left = M - hs;
        let first = (((hs - M) / lay.pitch).floor().max(0.0)) as usize;
        let last = (((hs + W) / lay.pitch).ceil() as usize).min(n);
        let mut focused = None;
        for (c, game) in row.games.iter().enumerate().take(last).skip(first) {
            let r = Rect::new(left + c as f32 * lay.pitch, y, lay.cover_w, lay.cover_h);
            self.hits.push((r, Target::Item(i, c)));
            if focused_shelf && c == col {
                focused = Some(r);
            } else {
                cover::draw(gfx, r, game, &ctx.artwork.cover(&ctx.library, game), true, false);
            }
        }
        let see_all = Rect::new(left + n as f32 * lay.pitch, y, SEE_ALL_W, lay.cover_h);
        if see_all.x < W {
            draw_see_all(gfx, see_all, row.shelf, row.total, focused_shelf && col == n);
            self.hits.push((see_all, Target::Item(i, n)));
        }
        if let Some(r) = focused {
            let g = self.grow.value();
            let big = Rect::new(
                r.x - GROW * g,
                r.y - 12.0 * g,
                r.w + 2.0 * GROW * g,
                r.h + 24.0 * g,
            );
            let game = &row.games[col];
            cover::draw_focused(gfx, big, game, &ctx.artwork.cover(&ctx.library, game), true, g);
            // In the space kept free under the covers, clear of the grown cover and its ring, and
            // fading in with the ring (FR-UI-8).
            if cover::shows_playtime(ctx.show_playtime, CoverPlace::Home, true, game.playtime_secs) {
                gfx.text(
                    r.x,
                    r.y + r.h + PLAYTIME_DY,
                    &cover::playtime_label(game.playtime_secs),
                    TextStyle::new(17, Weight::Medium, MUTED.alpha(g)),
                    Align::Left,
                );
            }
        }
        // Mouse paging, when the Shelf runs past the screen.
        let cy = y + lay.cover_h / 2.0;
        let max = scroll::clamp(f32::MAX, self.content_w(i), VIEW_W);
        for (show, x, dir) in [
            (self.hscroll[i] > 0.0, M / 2.0, -1.0),
            (self.hscroll[i] < max, W - M / 2.0, 1.0),
        ] {
            if show {
                // The Shelf fades into the background under the arrow, so the arrow sits on an
                // edge rather than on a cover, and the cut-off cover reads as "more this way".
                let scrim = Rect::new(
                    if dir < 0.0 { 0.0 } else { W - EDGE_FADE_W },
                    y - 16.0,
                    EDGE_FADE_W,
                    lay.cover_h + 32.0,
                );
                let (from, to) = if dir < 0.0 {
                    (BG.alpha(0.9), BG.alpha(0.0))
                } else {
                    (BG.alpha(0.0), BG.alpha(0.9))
                };
                gfx.fill_round_rect_gradient(scrim, 0.0, from, to, (1.0, 0.0));
                gfx.fill_circle(x, cy, 26.0, SURFACE2.alpha(0.92));
                let d = 6.0 * dir;
                gfx.line(x - d, cy - 12.0, x + d, cy, 3.0, TEXT);
                gfx.line(x + d, cy, x - d, cy + 12.0, 3.0, TEXT);
                self.hits.insert(
                    0,
                    (Rect::new(x - 30.0, cy - 30.0, 60.0, 60.0), Target::Page(i, dir)),
                );
            }
        }
    }

    /// Draws empty Shelf `i`'s hint in its block, whose top is at `top`: a box as tall as a cover
    /// where the covers would be, its placeholders and text centred in it (FR-HOME-5).
    fn draw_empty(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx, i: usize, top: f32) {
        let r = Rect::new(M, top + COVERS_DY, VIEW_W, Layout::now().cover_h);
        let mid = r.y + r.h / 2.0;
        gfx.fill_round_rect(r, 14.0, SURFACE.alpha(0.6));
        // An empty Shelf only holds a hint, so its focus is a quiet outline, not a cover's ring.
        if self.focus.shelf == i {
            gfx.stroke_round_rect(r, 14.0, 2.0, ACCENT.alpha(0.85));
        } else {
            gfx.stroke_round_rect_dashed(r, 14.0, 2.0, 10.0, 8.0, LINE);
        }
        for k in 0..5 {
            let sq = Rect::new(
                M + 28.0 + k as f32 * PLACEHOLDER_PITCH,
                mid - PLACEHOLDER / 2.0,
                PLACEHOLDER,
                PLACEHOLDER,
            );
            gfx.fill_round_rect(sq, 8.0, SURFACE2.alpha(0.7));
        }
        self.hits.push((r, Target::Item(i, 0)));
        let style = TextStyle::new(22, Weight::Medium, MUTED);
        // The baseline sits a little under the middle, so the 22 px text reads as centred.
        let (x, y) = (M + 640.0, mid + 8.0);
        match self.shelves[i].shelf {
            Shelf::Favorites => {
                let mut x = x + gfx.text(x, y, "Press ", style, Align::Left);
                x += chrome::glyph(
                    gfx,
                    x,
                    y - 8.0,
                    glyph_label(Action::Options, ctx.input_kind, ctx.button_style),
                );
                gfx.text(x, y, " on a game to add a favorite", style, Align::Left);
            }
            shelf => {
                let text = match shelf {
                    Shelf::ContinuePlaying => "Games you play show up here".to_owned(),
                    Shelf::RecentlyAdded => format!(
                        "Put games in their System folder in {}",
                        tilde(&ctx.games_root, dirs::home_dir().as_deref())
                    ),
                    _ => "Fills up as you play".to_owned(),
                };
                let text = ellipsize(&text, W - M - 24.0 - x, |s| gfx.measure(s, style));
                gfx.text(x, y, &text, style, Align::Left);
            }
        }
    }
}

/// Clips each hit rect to `view`, as `GameGrid` does, so a cover scrolled partly under the top
/// bar or hint bar doesn't take clicks there (FR-UI-1).
fn clip_hits(hits: &[(Rect, Target)], view: Rect) -> Vec<(Rect, Target)> {
    hits.iter()
        .filter_map(|&(r, t)| {
            let y0 = r.y.max(view.y);
            let y1 = (r.y + r.h).min(view.y + view.h);
            (y1 > y0).then(|| (Rect::new(r.x, y0, r.w, y1 - y0), t))
        })
        .collect()
}

/// What Shelf `shelf`'s See all item reads, given how many Games its grid shows (FR-HOME-3): "All
/// games" where it opens every Game, "See all" on Favorites, each over the count.
fn see_all_label(shelf: Shelf, count: usize) -> (&'static str, String) {
    let title = match shelf.see_all_order() {
        Some(_) => "All games",
        None => "See all",
    };
    let count = if count == 1 {
        "1 game".to_owned()
    } else {
        format!("{count} games")
    };
    (title, count)
}

fn draw_see_all(gfx: &mut Gfx<'_>, r: Rect, shelf: Shelf, count: usize, focused: bool) {
    if focused {
        cover::focus_ring(gfx, r, COVER_RADIUS);
    }
    gfx.fill_round_rect(r, COVER_RADIUS, SURFACE);
    gfx.stroke_round_rect(r, COVER_RADIUS, 1.0, LINE);
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    gfx.fill_circle(cx, cy - 24.0, 26.0, SURFACE2);
    gfx.line(cx - 6.0, cy - 36.0, cx + 6.0, cy - 24.0, 3.0, TEXT);
    gfx.line(cx + 6.0, cy - 24.0, cx - 6.0, cy - 12.0, 3.0, TEXT);
    let (title, label) = see_all_label(shelf, count);
    gfx.text(
        cx,
        cy + 30.0,
        title,
        TextStyle::new(18, Weight::SemiBold, TEXT),
        Align::Centre,
    );
    gfx.text(
        cx,
        cy + 54.0,
        &label,
        TextStyle::new(15, Weight::Regular, MUTED),
        Align::Centre,
    );
}

impl Screen for Home {
    fn title(&self) -> &str {
        "Home"
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        self.sync(ctx);
        let before = (self.focus.clone(), self.top);
        let transition = match input {
            Input::Action(Action::Confirm) => self.confirm(ctx),
            Input::Action(Action::Options) => self.options(ctx),
            Input::Action(dir) if dir.is_direction() => {
                if self.focus.step(dir, &self.counts(), ctx.repeat) {
                    self.reveal(self.focus.shelf);
                    self.reveal_shelf();
                }
                Transition::None
            }
            Input::Pointer(Pointer::Move { x, y }) => {
                if let Some(Target::Item(s, c)) = self.hit_at_rest(x, y) {
                    self.focus.shelf = s;
                    self.focus.cols[s] = c.min(self.counts()[s].saturating_sub(1));
                }
                Transition::None
            }
            Input::Pointer(Pointer::Click { x, y }) => match self.hit_at_rest(x, y) {
                Some(Target::Item(s, c)) => {
                    self.focus.shelf = s;
                    self.focus.cols[s] = c.min(self.counts()[s].saturating_sub(1));
                    self.confirm(ctx)
                }
                Some(Target::Page(s, dir)) => {
                    self.scroll_shelf(s, self.hscroll[s] + dir * (VIEW_W - Layout::now().pitch));
                    Transition::None
                }
                None => Transition::None,
            },
            Input::Pointer(Pointer::RightClick { x, y }) => match self.hit_at_rest(x, y) {
                Some(Target::Item(s, c)) => {
                    self.focus.shelf = s;
                    self.focus.cols[s] = c.min(self.counts()[s].saturating_sub(1));
                    self.options(ctx)
                }
                _ => Transition::None,
            },
            Input::Pointer(Pointer::Wheel { dy }) => {
                self.turn_wheel(dy);
                Transition::None
            }
            _ => Transition::None,
        };
        self.drop_opening_if_moved(before);
        transition
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.sync(ctx);
        self.refit();
        self.hits.clear();
        for (shown, &target) in self.shown_h.iter_mut().zip(&self.hscroll) {
            shown.set(target);
            shown.step(ctx.animate, SCROLL_TAU);
        }
        let item = (self.focus.shelf, self.focus.col());
        if self.grown != Some(item) {
            // A first draw shows the focus already grown; a move grows the new one.
            self.grow.jump(if self.grown.is_some() { 0.0 } else { 1.0 });
            self.grow.set(1.0);
            self.grown = Some(item);
        }
        self.grow.step(ctx.animate, FOCUS_TAU);
        self.shown_v.set(self.rest_offset());
        let vs = self.shown_v.step(ctx.animate, SCROLL_TAU);
        let (view_bottom, lay) = (view_bottom(), Layout::now());
        let view_h = lay.view_h;
        gfx.clip(Some(Rect::new(0.0, VIEW_TOP, W, view_h)));
        let title = TextStyle::new(28, Weight::Bold, TEXT);
        for i in 0..self.shelves.len() {
            let top = VIEW_TOP + block_top(i) - vs;
            if top >= view_bottom || top + lay.block_h <= VIEW_TOP {
                continue;
            }
            if self.shelves[i].games.is_empty() {
                self.draw_empty(gfx, ctx, i, top);
            } else {
                self.draw_filled(gfx, ctx, i, top);
            }
            gfx.text(M, top + TITLE_DY, name(self.shelves[i].shelf), title, Align::Left);
        }
        gfx.clip(None);
        self.hits = clip_hits(&self.hits, Rect::new(0.0, VIEW_TOP, W, view_h));
    }

    /// Scrolls the Shelf under the pointer sideways, half a cover a notch as the grids scroll by
    /// half a row, keeping its focus in view (FR-HOME-8).
    fn side_wheel(&mut self, dx: f32, _x: f32, y: f32, ctx: &mut Ctx) {
        self.sync(ctx);
        let before = (self.focus.clone(), self.top);
        if let Some(s) = self.shelf_at(y) {
            self.scroll_shelf(s, self.hscroll[s] + dx * Layout::now().pitch / 2.0);
        }
        self.drop_opening_if_moved(before);
    }

    fn hints(&self) -> Vec<Hint> {
        let row = &self.shelves[self.focus.shelf];
        let mut hints = Vec::new();
        match item_at(row.games.len(), self.focus.col()) {
            Item::SeeAll => hints.push(Hint::new(&[Action::Confirm], "See all")),
            Item::Game(_) => {
                hints.push(Hint::new(&[Action::Confirm], "Details"));
                hints.push(Hint::new(&[Action::Options], "Options"));
            }
            _ => {}
        }
        hints.push(Hint::new(&[Action::PrevTab, Action::NextTab], "Switch view"));
        hints.push(Hint::new(&[Action::Search], "Search"));
        hints.push(Hint::new(&[Action::Settings], "Settings"));
        hints
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::db::{HomeShelves, SortOrder};
    use crate::ui;

    fn row(id: i64) -> GameRow {
        GameRow {
            id,
            system: "snes".into(),
            rel_path: format!("snes/{id}.sfc"),
            title: format!("Game {id}"),
            search_title: format!("Game {id}"),
            date_added: 0,
            last_played: None,
            playtime_secs: 0,
            favorite: false,
            release_date: None,
        }
    }

    fn rows(ids: impl IntoIterator<Item = i64>) -> Vec<GameRow> {
        ids.into_iter().map(row).collect()
    }

    #[test]
    fn see_all_follows_the_games_and_an_empty_shelf_has_only_its_hint() {
        assert_eq!(item_count(5), 6);
        assert_eq!(item_at(5, 4), Item::Game(4));
        assert_eq!(item_at(5, 5), Item::SeeAll);
        assert_eq!(item_count(0), 1);
        assert_eq!(item_at(0, 0), Item::Hint);
    }

    #[test]
    fn the_hints_end_with_search_and_settings() {
        let hints = Home::new().hints();
        assert_eq!(
            hints[hints.len() - 3..],
            [
                Hint::new(&[Action::PrevTab, Action::NextTab], "Switch view"),
                Hint::new(&[Action::Search], "Search"),
                Hint::new(&[Action::Settings], "Settings"),
            ]
        );
    }

    #[test]
    fn a_press_of_left_on_the_first_game_goes_round_to_see_all_and_right_on_it_goes_back() {
        let mut f = Focus::default();
        let counts = [3, 1, 21, 1];
        assert!(f.step(Action::Left, &counts, false));
        assert_eq!(f.col(), 2, "See all of a 2-Game Shelf");
        assert!(f.step(Action::Right, &counts, false));
        assert_eq!(f.col(), 0, "a fresh Right on See all wraps to the first Game");
        assert!(f.step(Action::Right, &counts, false));
        assert!(f.step(Action::Left, &counts, false));
        assert_eq!(f.col(), 0);
        f.step(Action::Down, &counts, false);
        assert!(
            !f.step(Action::Left, &counts, false),
            "an empty Shelf's hint stays"
        );
        assert!(!f.step(Action::Right, &counts, false));
        assert!(!f.step(Action::Right, &counts, true));
    }

    #[test]
    fn repeated_right_stops_on_see_all_and_repeated_left_stops_on_the_first_game() {
        let mut f = Focus::default();
        let counts = [4, 1, 1, 1];
        f.step(Action::Right, &counts, false);
        assert!(f.step(Action::Right, &counts, true));
        assert!(f.step(Action::Right, &counts, true));
        assert_eq!(f.col(), 3, "See all of a 3-Game Shelf");
        assert!(!f.step(Action::Right, &counts, true), "a repeat does not wrap");
        assert_eq!(f.col(), 3);
        while f.step(Action::Left, &counts, true) {}
        assert_eq!(f.col(), 0);
        assert!(!f.step(Action::Left, &counts, true), "a repeat does not wrap");
        assert_eq!(f.col(), 0);
    }

    #[test]
    fn a_hold_that_starts_on_see_all_wraps_once_then_walks_back_to_it() {
        let mut f = Focus::default();
        let counts = [4, 1, 1, 1];
        f.cols[0] = 3;
        assert!(f.step(Action::Right, &counts, false));
        assert_eq!(f.col(), 0);
        while f.step(Action::Right, &counts, true) {}
        assert_eq!(f.col(), 3);
        assert!(!f.step(Action::Right, &counts, true));
    }

    /// Left on Recently added's first Game, on Home, reaches its See all and scrolls it into view.
    #[test]
    fn left_on_a_shelfs_first_game_shows_its_see_all() {
        let mut ctx = Ctx::empty();
        let many: Vec<_> = (0..20).map(found).collect();
        ctx.library.sync_system("snes", &many, 0).unwrap();
        let mut home = Home::new();
        home.handle(Input::Pointer(Pointer::Move { x: -1.0, y: -1.0 }), &mut ctx); // initial load
        let recently_added = 2; // Shelf::ALL's index for Shelf::RecentlyAdded
        assert_eq!((home.focus.shelf, home.focus.col()), (recently_added, 0));
        assert_eq!(home.hscroll[recently_added], 0.0);
        home.handle(Input::Action(Action::Left), &mut ctx);
        assert_eq!(home.focus.col(), 20, "See all, after the 20 Games");
        assert_eq!(home.hints()[0], Hint::new(&[Action::Confirm], "See all"));
        let max = scroll::clamp(f32::MAX, home.content_w(recently_added), VIEW_W);
        assert_eq!(home.hscroll[recently_added], max, "scrolled to the end");
    }

    #[test]
    fn see_all_reads_all_games_except_on_favorites_over_the_count() {
        assert_eq!(
            see_all_label(Shelf::Favorites, 5),
            ("See all", "5 games".to_owned())
        );
        for shelf in [Shelf::ContinuePlaying, Shelf::RecentlyAdded, Shelf::MostPlayed] {
            assert_eq!(
                see_all_label(shelf, 23),
                ("All games", "23 games".to_owned()),
                "{shelf:?}"
            );
        }
        assert_eq!(see_all_label(Shelf::RecentlyAdded, 1).1, "1 game");
    }

    #[test]
    fn up_and_down_visit_every_shelf_in_order_including_empty_ones() {
        let mut f = Focus::default();
        let counts = [6, 1, 21, 1];
        assert!(!f.step(Action::Up, &counts, false));
        for shelf in 1..SHELVES {
            assert!(f.step(Action::Down, &counts, false));
            assert_eq!(f.shelf, shelf);
        }
        assert!(!f.step(Action::Down, &counts, false));
        assert_eq!(f.col(), 0, "an empty Shelf's hint");
    }

    #[test]
    fn each_shelf_remembers_its_position() {
        let mut f = Focus::default();
        let counts = [6, 21, 21, 1];
        for _ in 0..4 {
            f.step(Action::Right, &counts, false);
        }
        f.step(Action::Down, &counts, false);
        assert_eq!(f.col(), 0, "a Shelf not visited yet starts at its first Game");
        f.step(Action::Right, &counts, false);
        f.step(Action::Up, &counts, false);
        assert_eq!((f.shelf, f.col()), (0, 4));
        f.step(Action::Down, &counts, false);
        assert_eq!((f.shelf, f.col()), (1, 1));
    }

    #[test]
    fn a_remembered_position_past_a_shrunken_shelf_is_clamped() {
        let mut f = Focus {
            shelf: 0,
            cols: [0, 9, 0, 0],
        };
        f.step(Action::Down, &[3, 4, 1, 1], false);
        assert_eq!(f.col(), 3, "See all of the 3-Game Shelf");
    }

    #[test]
    fn reload_keeps_focus_on_the_same_game() {
        let old = rows(1..=5);
        assert_eq!(refocus(&old, 3, &rows([7, 4, 1])), 1, "Game 4 moved");
        assert_eq!(
            refocus(&old, 2, &rows([7, 4, 1])),
            2,
            "Game 3 is gone: same position"
        );
        assert_eq!(
            refocus(&old, 4, &rows([1, 2])),
            2,
            "Game 5 is gone: clamped to See all"
        );
        assert_eq!(
            refocus(&old, 5, &rows([1, 2, 3, 4, 5, 6])),
            6,
            "See all stays See all"
        );
        assert_eq!(refocus(&old, 3, &[]), 0, "the hint of an emptied Shelf");
        assert_eq!(refocus(&[], 0, &old), 0, "the first Game of a filled Shelf");
    }

    #[test]
    fn games_root_in_home_is_written_with_a_tilde() {
        let home = Path::new("/home/ann");
        assert_eq!(tilde(Path::new("/home/ann/Games"), Some(home)), "~/Games");
        assert_eq!(tilde(Path::new("/home/ann"), Some(home)), "~");
        assert_eq!(tilde(Path::new("/mnt/games"), Some(home)), "/mnt/games");
        assert_eq!(tilde(Path::new("/mnt/games"), None), "/mnt/games");
    }

    use crate::library::FoundGame;

    fn found(n: usize) -> FoundGame {
        FoundGame {
            system: "snes",
            rel_path: format!("snes/g{n:03}.sfc"),
            title: format!("Game {n:03}"),
        }
    }

    #[test]
    fn a_click_that_lands_in_the_reload_frame_is_ignored() {
        let mut ctx = Ctx::empty();
        let many: Vec<_> = (0..20).map(found).collect();
        ctx.library.sync_system("snes", &many, 0).unwrap();
        let mut home = Home::new();
        home.handle(Input::Action(Action::Confirm), &mut ctx); // initial load
        let recently_added = 2; // Shelf::ALL's index for Shelf::RecentlyAdded
        assert_eq!(home.shelves[recently_added].games.len(), 20);

        // Focus Game 10 on the Shelf, and record where a click there would land, as the previous
        // frame's hit rects would.
        home.focus.shelf = recently_added;
        home.focus.cols[recently_added] = 10;
        home.hits = vec![(
            Rect::new(500.0, 400.0, 50.0, 50.0),
            Target::Item(recently_added, 10),
        )];

        // The Shelf shrinks to 3 Games, and the pointer moves and clicks where Game 10 used to be,
        // before the next draw. Acting on the stale rect would open See all.
        let shrunk: Vec<_> = (0..3).map(found).collect();
        ctx.library.sync_system("snes", &shrunk, 0).unwrap();
        ctx.revision += 1;
        home.handle(Input::Pointer(Pointer::Move { x: 510.0, y: 410.0 }), &mut ctx);
        let clicked = home.handle(Input::Pointer(Pointer::Click { x: 510.0, y: 410.0 }), &mut ctx);
        assert!(
            matches!(clicked, Transition::None),
            "a stale click must not open See all"
        );

        assert_eq!(
            home.shelves[recently_added].games.len(),
            3,
            "the Shelf did reload"
        );
        assert_eq!(
            home.focus.cols[recently_added], 3,
            "the stale click was ignored; focus stays where the reload's own refocus clamped it"
        );
    }

    #[test]
    fn confirm_or_a_click_on_a_shelf_game_opens_its_details_page() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = dir.path().to_owned();
        // An installed Standalone Emulator's Game, which Confirm used to launch directly.
        let zelda = FoundGame {
            system: "gc",
            rel_path: "gc/Zelda.iso".into(),
            title: "Zelda".into(),
        };
        ctx.library.sync_system("gc", &[zelda], 0).unwrap();
        let mut home = Home::new();
        home.handle(Input::Action(Action::Down), &mut ctx); // initial load
        let recently_added = 2; // Shelf::ALL's index for Shelf::RecentlyAdded
        home.focus.shelf = recently_added;
        home.reveal_shelf(); // Home at rest on it, so its covers take clicks
        assert_eq!(home.hints()[0], Hint::new(&[Action::Confirm], "Details"));

        home.hits = vec![(Rect::new(0.0, 0.0, 50.0, 50.0), Target::Item(recently_added, 0))];
        for input in [
            Input::Action(Action::Confirm),
            Input::Pointer(Pointer::Click { x: 10.0, y: 10.0 }),
        ] {
            let Transition::Push(page) = home.handle(input, &mut ctx) else {
                panic!("{input:?} should open the Details Page");
            };
            assert_eq!(page.title(), "Zelda");
            assert!(page.hints().contains(&Hint::new(&[Action::Options], "Options")));
            assert!(
                ctx.play.is_none() && ctx.launch.is_none(),
                "{input:?} doesn't play"
            );
        }
    }

    #[test]
    fn options_y_or_a_right_click_on_a_shelf_game_opens_its_options_menu() {
        let mut ctx = Ctx::empty();
        let games: Vec<_> = (0..3).map(found).collect();
        ctx.library.sync_system("snes", &games, 0).unwrap();
        let mut home = Home::new();
        home.handle(Input::Action(Action::Down), &mut ctx); // initial load
        let recently_added = 2; // Shelf::ALL's index for Shelf::RecentlyAdded
        home.focus.shelf = recently_added;
        home.reveal_shelf(); // Home at rest on it, so its covers take clicks
        let title = |home: &Home, col: usize| home.shelves[recently_added].games[col].title.clone();
        assert!(home.hints().contains(&Hint::new(&[Action::Options], "Options")));

        let Transition::Push(menu) = home.handle(Input::Action(Action::Options), &mut ctx) else {
            panic!("Y should open the Options Menu");
        };
        assert!(menu.is_modal());
        assert_eq!(menu.title(), title(&home, 0));

        home.hits = vec![(Rect::new(0.0, 0.0, 50.0, 50.0), Target::Item(recently_added, 2))];
        let Transition::Push(menu) =
            home.handle(Input::Pointer(Pointer::RightClick { x: 10.0, y: 10.0 }), &mut ctx)
        else {
            panic!("a right-click should open the Options Menu");
        };
        assert_eq!(menu.title(), title(&home, 2), "for the Game right-clicked");
        assert_eq!(home.focus.col(), 2, "which takes focus");

        home.focus.cols[recently_added] = 3; // See all
        assert!(matches!(
            home.handle(Input::Action(Action::Options), &mut ctx),
            Transition::None
        ));
        assert!(ctx.play.is_none() && ctx.launch.is_none());
    }

    #[test]
    fn hiding_a_game_from_its_options_menu_takes_it_off_every_shelf() {
        let mut ctx = Ctx::empty();
        let games: Vec<_> = (0..3).map(found).collect();
        ctx.library.sync_system("snes", &games, 0).unwrap();
        let mut home = Home::new();
        home.handle(Input::Action(Action::Down), &mut ctx); // initial load
        let recently_added = 2; // Shelf::ALL's index for Shelf::RecentlyAdded
        home.focus.shelf = recently_added;
        let hidden = home.shelves[recently_added].games[0].clone();
        ctx.library.set_favorite(hidden.id, true, 0).unwrap();
        ctx.revision += 1;
        let Transition::Push(mut menu) = home.handle(Input::Action(Action::Options), &mut ctx) else {
            panic!("Y should open the Options Menu");
        };
        // Past Start from power-on, Choose artwork and Rename to Hide (no Save State to load, no
        // save data to delete).
        for _ in 0..4 {
            menu.handle(Input::Action(Action::Down), &mut ctx);
        }
        assert!(matches!(
            menu.handle(Input::Action(Action::Confirm), &mut ctx),
            Transition::Pop
        ));
        home.handle(Input::Action(Action::Left), &mut ctx);
        assert!(
            home.shelves
                .iter()
                .all(|s| s.games.iter().all(|g| g.id != hidden.id)),
            "on no Shelf, Favorites included"
        );
        assert_eq!(home.shelves[recently_added].games.len(), 2);
    }

    /// Four Games on two Systems, so that every order "See all" can show differs from the next one
    /// in the cycle: Alpha and Beta added first, Mega played most, Beta last, Kid never.
    fn library_for_see_all(ctx: &mut Ctx) {
        let game = |system: &'static str, title: &str| FoundGame {
            system,
            rel_path: format!("{system}/{title}.zip"),
            title: title.into(),
        };
        let lib = &mut ctx.library;
        lib.sync_system("snes", &[game("snes", "Alpha"), game("snes", "Beta")], 100)
            .unwrap();
        lib.sync_system("nes", &[game("nes", "Kid"), game("nes", "Mega")], 200)
            .unwrap();
        let id = |lib: &crate::library::db::Library, title: &str| {
            let rows = lib.all_games(SortOrder::AZ).unwrap();
            rows.iter().find(|r| r.title == title).unwrap().id
        };
        lib.record_play(id(lib, "Alpha"), 300, 10).unwrap();
        lib.record_play(id(lib, "Mega"), 400, 90).unwrap();
        lib.record_play(id(lib, "Beta"), 500, 5).unwrap();
    }

    fn ids(ctx: &Ctx, order: SortOrder) -> Vec<i64> {
        ctx.library
            .all_games(order)
            .unwrap()
            .iter()
            .map(|g| g.id)
            .collect()
    }

    #[test]
    fn see_all_opens_every_game_in_its_shelfs_order_and_sorts_it_without_storing_the_order() {
        let mut ctx = Ctx::empty();
        library_for_see_all(&mut ctx);
        ctx.library.set_sort_order(SortOrder::ZA).unwrap(); // as a Console Page left it
        for (shelf, order) in [
            (Shelf::ContinuePlaying, SortOrder::LastPlayed),
            (Shelf::RecentlyAdded, SortOrder::RecentlyAdded),
            (Shelf::MostPlayed, SortOrder::MostPlayed),
        ] {
            let mut grid = see_all(shelf);
            assert_eq!(grid.title(), "All games");
            grid.handle(Input::Action(Action::Left), &mut ctx); // initial load
            assert_eq!(grid.shown_order(), (Some(order), ids(&ctx, order)), "{shelf:?}");
            assert!(grid.hints().contains(&Hint::new(&[Action::Sort], "Sort")));

            // The list of orders opens on the current one: the next is one row down.
            for action in [Action::Sort, Action::Down, Action::Confirm] {
                grid.handle(Input::Action(action), &mut ctx);
            }
            let next = order.next();
            assert_ne!(ids(&ctx, next), ids(&ctx, order), "the orders differ");
            assert_eq!(grid.shown_order(), (Some(next), ids(&ctx, next)), "{shelf:?}");
            assert_eq!(
                ctx.library.sort_order().unwrap(),
                SortOrder::ZA,
                "the stored order is untouched"
            );

            let mut again = see_all(shelf);
            again.handle(Input::Action(Action::Left), &mut ctx);
            assert_eq!(
                again.shown_order().0,
                Some(order),
                "starts in {shelf:?}'s order again"
            );
        }
    }

    #[test]
    fn see_all_on_favorites_shows_only_the_favorites_and_ignores_sort() {
        let mut ctx = Ctx::empty();
        library_for_see_all(&mut ctx);
        let favorites = [ids(&ctx, SortOrder::AZ)[3], ids(&ctx, SortOrder::AZ)[0]]; // Mega, then Alpha
        for (at, id) in favorites.into_iter().enumerate() {
            ctx.library.set_favorite(id, true, at as i64).unwrap();
        }
        let mut grid = see_all(Shelf::Favorites);
        assert_eq!(grid.title(), "Favorites");
        grid.handle(Input::Action(Action::Left), &mut ctx); // initial load
        let shown = grid.shown_order();
        assert_eq!(shown.1, [favorites[1], favorites[0]], "latest Favorite first");
        assert_eq!(shown.0, None);

        grid.handle(Input::Action(Action::Sort), &mut ctx);
        assert_eq!(grid.shown_order(), shown);
        assert!(!grid.hints().contains(&Hint::new(&[Action::Sort], "Sort")));
        assert_eq!(ctx.library.sort_order().unwrap(), SortOrder::AZ);
    }

    #[test]
    fn see_all_counts_the_games_its_grid_shows() {
        let mut ctx = Ctx::empty();
        library_for_see_all(&mut ctx);
        let alpha = ids(&ctx, SortOrder::AZ)[0];
        ctx.library.set_favorite(alpha, true, 0).unwrap();
        let mut home = Home::new();
        home.handle(Input::Action(Action::Left), &mut ctx); // initial load
        let totals: Vec<_> = home.shelves.iter().map(|r| (r.shelf, r.total)).collect();
        assert_eq!(
            totals,
            [
                (Shelf::Favorites, 1),
                (Shelf::ContinuePlaying, 4),
                (Shelf::RecentlyAdded, 4),
                (Shelf::MostPlayed, 4)
            ]
        );

        let continue_playing = 1; // Shelf::ALL's index for Shelf::ContinuePlaying
        home.focus.shelf = continue_playing;
        home.focus.cols[continue_playing] = 3; // See all, after the 3 Games played
        let Transition::Push(mut grid) = home.handle(Input::Action(Action::Confirm), &mut ctx) else {
            panic!("See all should open a grid");
        };
        assert_eq!(grid.title(), "All games");
        for action in [Action::Sort, Action::Down, Action::Confirm] {
            grid.handle(Input::Action(action), &mut ctx);
        }
        assert_eq!(ctx.library.sort_order().unwrap(), SortOrder::AZ, "not stored");
    }

    #[test]
    fn a_hit_rect_above_the_view_is_clipped_so_a_click_there_misses() {
        let view = Rect::new(0.0, VIEW_TOP, W, view_h());
        // A cover scrolled so most of it sits above the top bar; only its bottom sliver is
        // visible.
        let lay = Layout::now();
        let hits = vec![(
            Rect::new(100.0, VIEW_TOP - 80.0, lay.cover_w, lay.cover_h),
            Target::Item(0, 2),
        )];
        let clipped = clip_hits(&hits, view);
        assert_eq!(
            hit(&clipped, 150.0, VIEW_TOP - 40.0),
            None,
            "a click under the top bar misses"
        );
        assert_eq!(
            hit(&clipped, 150.0, VIEW_TOP + 5.0),
            Some(Target::Item(0, 2)),
            "a click on the visible sliver still hits"
        );
    }

    /// Every Shelf filled with `n` Games.
    fn filled_home(n: i64) -> Home {
        let mut home = Home::new();
        for row in &mut home.shelves {
            row.games = rows(1..=n);
        }
        home
    }

    #[test]
    fn two_shelves_fill_the_view_with_2_by_3_covers_sized_from_the_height() {
        for (h, view, cover_w, cover_h) in [
            (1080.0, 900.0, 208.0, 312.0),
            (1200.0, 1020.0, 248.0, 372.0),
            (1440.0, 1260.0, 328.0, 492.0),
        ] {
            ui::with_h(h, || {
                let lay = Layout::now();
                assert_eq!(view_bottom(), h - 70.0, "at {h}");
                assert_eq!(lay.view_h, view, "at {h}");
                assert_eq!(2.0 * lay.block_h, lay.view_h, "two blocks fill the view at {h}");
                assert_eq!((lay.cover_w, lay.cover_h), (cover_w, cover_h), "at {h}");
                assert_eq!(lay.cover_w * 3.0, lay.cover_h * 2.0, "2:3 at {h}");
                assert_eq!(lay.pitch, cover_w + COVER_GAP, "at {h}");
                assert_eq!(
                    lay.block_h - COVERS_DY - lay.cover_h,
                    60.0,
                    "the space under the covers stays free at {h}"
                );
            });
        }
    }

    #[test]
    fn an_empty_shelf_is_as_tall_as_a_filled_one() {
        let mut home = filled_home(3);
        home.shelves[1].games.clear();
        let lay = Layout::now();
        for i in 0..SHELVES {
            assert_eq!(block_top(i + 1) - block_top(i), lay.block_h, "Shelf {i}");
        }
    }

    /// The resting offset is a whole number of blocks and the focused Shelf lies inside the view.
    fn assert_at_rest_on_the_focus(home: &Home) {
        let lay = Layout::now();
        let offset = home.rest_offset();
        assert_eq!(offset % lay.block_h, 0.0, "{offset} is a whole number of Shelves");
        let top = block_top(home.focus.shelf);
        assert!(
            top >= offset && top + lay.block_h <= offset + lay.view_h,
            "Shelf {} at {top} is inside the view at {offset}",
            home.focus.shelf
        );
    }

    #[test]
    fn home_rests_on_whole_shelves_and_moves_by_one_when_the_focus_leaves_them() {
        for h in [1080.0, 1200.0] {
            ui::with_h(h, || {
                let mut ctx = Ctx::empty();
                let mut home = Home::new();
                let down = Input::Action(Action::Down);
                let up = Input::Action(Action::Up);
                // Down: Shelf 1 is already showing, then each Down leaves the lower Shelf showing.
                for (shelf, top) in [(0, 0), (1, 0), (2, 1), (3, 2), (3, 2)] {
                    assert_eq!((home.focus.shelf, home.top), (shelf, top), "going down at {h}");
                    assert_at_rest_on_the_focus(&home);
                    home.handle(down, &mut ctx);
                }
                // Up: Shelf 2 is still showing, then each Up leaves the upper Shelf showing.
                for (shelf, top) in [(3, 2), (2, 2), (1, 1), (0, 0), (0, 0)] {
                    assert_eq!((home.focus.shelf, home.top), (shelf, top), "going up at {h}");
                    assert_at_rest_on_the_focus(&home);
                    home.handle(up, &mut ctx);
                }
            });
        }
    }

    #[test]
    fn a_wheel_notch_moves_by_one_shelf_and_pulls_the_focus_onto_a_shelf_showing() {
        let mut ctx = Ctx::empty();
        let many: Vec<_> = (0..20).map(found).collect();
        ctx.library.sync_system("snes", &many, 0).unwrap();
        // A Favorite, so Home opens on its first Shelf.
        let first = ctx.library.all_games(SortOrder::AZ).unwrap()[0].id;
        ctx.library.set_favorite(first, true, 0).unwrap();
        let mut home = Home::new();
        home.handle(Input::Action(Action::Up), &mut ctx); // initial load
        assert_eq!((home.focus.shelf, home.top), (0, 0));
        let recently_added = 2; // Shelf::ALL's index for Shelf::RecentlyAdded
        home.focus.cols[recently_added] = 7;
        let notch_down = Input::Pointer(Pointer::Wheel { dy: -1.0 });
        let notch_up = Input::Pointer(Pointer::Wheel { dy: 1.0 });

        home.handle(notch_down, &mut ctx);
        assert_eq!(home.top, 1, "one Shelf down");
        assert_eq!(
            home.focus.shelf, 1,
            "the focus left Shelf 0 for the upper Shelf showing"
        );
        assert_at_rest_on_the_focus(&home);
        home.handle(notch_down, &mut ctx);
        assert_eq!(home.top, 2);
        assert_eq!(
            (home.focus.shelf, home.focus.col()),
            (recently_added, 7),
            "onto Recently added, at the position it remembers"
        );
        assert_at_rest_on_the_focus(&home);
        home.handle(notch_down, &mut ctx);
        assert_eq!(home.top, 2, "the last two Shelves show: no further");

        home.focus.shelf = 3;
        home.handle(notch_up, &mut ctx);
        assert_eq!((home.top, home.focus.shelf), (1, 2), "one Shelf up");
        assert_at_rest_on_the_focus(&home);
        home.handle(notch_up, &mut ctx);
        assert_eq!((home.top, home.focus.shelf), (0, 1));
        home.handle(notch_up, &mut ctx);
        assert_eq!(
            (home.top, home.focus.shelf),
            (0, 1),
            "the first Shelf shows: no further"
        );
    }

    /// Home loaded from an empty library, then given 20 Games on each of its four Shelves.
    fn wheel_home(ctx: &mut Ctx) -> Home {
        let mut home = Home::new();
        home.handle(Input::Action(Action::Left), ctx); // initial load
        for row in &mut home.shelves {
            row.games = rows(1..=20);
        }
        assert_eq!(home.shelves.len(), 4);
        home
    }

    /// A touchpad sends many small `dy`: Home moves a whole Shelf for each notch's worth (1.0) of
    /// them, a mouse notch still moves one, and a turn the other way starts afresh.
    #[test]
    fn a_touchpad_swipe_moves_one_shelf_per_notch_of_small_wheel_steps() {
        let mut ctx = Ctx::empty();
        let swipe_down = Input::Pointer(Pointer::Wheel { dy: -0.1 });
        let swipe_up = Input::Pointer(Pointer::Wheel { dy: 0.1 });
        let mut home = wheel_home(&mut ctx);
        for i in 1..=20 {
            home.handle(swipe_down, &mut ctx);
            assert_eq!(home.top, i / 10, "after {i} steps of 0.1");
        }
        assert_eq!(home.top, 2, "twenty steps of 0.1 move two Shelves");

        let mut home = wheel_home(&mut ctx);
        home.handle(Input::Pointer(Pointer::Wheel { dy: -1.0 }), &mut ctx);
        assert_eq!(home.top, 1, "a mouse notch moves one Shelf");
        for _ in 0..5 {
            home.handle(swipe_down, &mut ctx);
        }
        assert_eq!(home.top, 1, "half a notch down doesn't move");
        for i in 1..=10 {
            home.handle(swipe_up, &mut ctx);
            let top = if i < 10 { 1 } else { 0 };
            assert_eq!(
                home.top, top,
                "after {i} steps up: the half notch down isn't carried"
            );
        }
    }

    /// The hit rects are the last frame's, drawn mid-snap: a Shelf sliding out of view doesn't take
    /// the focus from under the pointer, a Shelf showing at rest does.
    #[test]
    fn hovering_a_shelf_sliding_out_after_a_wheel_move_leaves_the_focus() {
        let mut ctx = Ctx::empty();
        let mut home = wheel_home(&mut ctx);
        home.handle(Input::Pointer(Pointer::Wheel { dy: -1.0 }), &mut ctx);
        assert_eq!((home.top, home.focus.shelf), (1, 1));
        home.focus.cols[1] = 4;
        home.hits = vec![
            (Rect::new(0.0, 0.0, 50.0, 50.0), Target::Item(0, 3)),
            (Rect::new(0.0, 100.0, 50.0, 50.0), Target::Item(2, 6)),
        ];

        home.handle(Input::Pointer(Pointer::Move { x: 10.0, y: 10.0 }), &mut ctx);
        assert_eq!(
            (home.focus.shelf, home.focus.col()),
            (1, 4),
            "Shelf 0 is not showing at rest: the focus stays"
        );
        home.handle(Input::Pointer(Pointer::Move { x: 10.0, y: 110.0 }), &mut ctx);
        assert_eq!(
            (home.focus.shelf, home.focus.col()),
            (2, 6),
            "Shelf 2 shows at rest"
        );
    }

    fn shown(home: &Home) -> Vec<Shelf> {
        home.shelves.iter().map(|row| row.shelf).collect()
    }

    /// With 1, 2 or 3 Shelves switched on, Home shows them in their order, Down visits each, and
    /// every move, by the keys or the wheel, leaves Home on whole Shelves with `top` in
    /// 0..=max(0, n - 2): one Shelf alone sits in the upper half.
    #[test]
    fn with_one_two_or_three_shelves_home_shows_them_in_their_order_on_whole_shelves() {
        use Shelf::{ContinuePlaying, Favorites, MostPlayed, RecentlyAdded};
        let down = Input::Action(Action::Down);
        let up = Input::Action(Action::Up);
        let right = Input::Action(Action::Right);
        let notch_down = Input::Pointer(Pointer::Wheel { dy: -1.0 });
        let notch_up = Input::Pointer(Pointer::Wheel { dy: 1.0 });
        for order in [
            vec![MostPlayed],
            vec![RecentlyAdded, Favorites],
            vec![MostPlayed, ContinuePlaying, RecentlyAdded],
        ] {
            let mut ctx = Ctx::empty();
            ctx.home_shelves = HomeShelves::showing(&order).unwrap();
            let mut home = Home::new();
            home.handle(Input::Action(Action::Left), &mut ctx); // initial load
            assert_eq!(shown(&home), order);
            for row in &mut home.shelves {
                row.games = rows(1..=20);
            }
            let n = order.len();
            let mut visited = vec![home.focus.shelf];
            let moves = [
                [down; 4],
                [right; 4],
                [up; 4],
                [notch_down; 4],
                [notch_up; 4],
                [down; 4],
            ];
            for input in moves.into_iter().flatten() {
                home.handle(input, &mut ctx);
                if input == down && visited.last() != Some(&home.focus.shelf) {
                    visited.push(home.focus.shelf);
                }
                assert!(
                    home.top <= n.saturating_sub(2),
                    "top {} with {n} Shelves",
                    home.top
                );
                assert!(home.focus.shelf < n);
                assert_at_rest_on_the_focus(&home);
            }
            assert_eq!(
                visited,
                (0..n).collect::<Vec<_>>(),
                "Down visits each of {n} Shelves"
            );
        }
    }

    /// Home under Settings: once Settings → Home changes the Shelves, Home shows the new ones.
    /// The focused Shelf keeps focus, on the same Game, while it is still shown; when it is no
    /// longer shown, the first Shelf has focus.
    #[test]
    fn a_change_in_settings_shows_the_new_shelves_keeping_the_focused_shelf_if_shown() {
        use Shelf::{ContinuePlaying, Favorites, MostPlayed, RecentlyAdded};
        let mut ctx = Ctx::empty();
        let many: Vec<_> = (0..20).map(found).collect();
        ctx.library.sync_system("snes", &many, 0).unwrap();
        let mut home = Home::new();
        let act = |home: &mut Home, ctx: &mut Ctx, actions: &[Action]| {
            for &action in actions {
                home.handle(Input::Action(action), ctx);
            }
        };
        // Home opens on Recently added, the only Shelf with Games.
        act(&mut home, &mut ctx, &[Action::Right; 5]);
        let recently_added = 2; // Shelf::ALL's index for Shelf::RecentlyAdded
        assert_eq!((home.focus.shelf, home.top), (recently_added, 2));
        let game = home.shelves[recently_added].games[5].id;

        // Settings over Home: Favorites switched off, then Recently added moved up past it to
        // the top, then Back twice returns to Home.
        let mut settings = crate::screens::settings::Settings::new();
        let mut actions = vec![Action::Down; 5];
        actions.extend([Action::Right, Action::Confirm, Action::Down, Action::Down]);
        actions.extend([Action::Left, Action::Left]);
        for action in actions {
            settings.handle(Input::Action(action), &mut ctx);
        }
        assert!(matches!(
            settings.handle(Input::Action(Action::Back), &mut ctx),
            Transition::None
        ));
        assert!(matches!(
            settings.handle(Input::Action(Action::Back), &mut ctx),
            Transition::Pop
        ));
        let nothing = Input::Pointer(Pointer::Move { x: -1.0, y: -1.0 });
        home.handle(nothing, &mut ctx);
        assert_eq!(shown(&home), [RecentlyAdded, ContinuePlaying, MostPlayed]);
        assert_eq!(home.focus.shelf, 0, "Recently added keeps focus, now first");
        assert_eq!(
            home.shelves[0].games[home.focus.col()].id,
            game,
            "on the same Game"
        );
        assert_eq!(home.top, 0);
        assert_at_rest_on_the_focus(&home);
        assert_eq!(home.shown_v.value(), home.rest_offset(), "no glide");

        // Down to Most played, the third Shelf, which a change then switches off.
        act(&mut home, &mut ctx, &[Action::Down, Action::Down]);
        assert_eq!((home.focus.shelf, home.top), (2, 1));
        ctx.set_home_shelves(HomeShelves::showing(&[Favorites, ContinuePlaying]).unwrap());
        home.handle(nothing, &mut ctx);
        assert_eq!(shown(&home), [Favorites, ContinuePlaying]);
        assert_eq!(
            (home.focus.shelf, home.focus.col()),
            (0, 0),
            "the first Shelf has focus"
        );
        assert_eq!(home.top, 0);
        assert_at_rest_on_the_focus(&home);
    }

    /// Home loaded once from `ctx`, by an input that moves nothing.
    fn opened(ctx: &mut Ctx) -> Home {
        let mut home = Home::new();
        home.handle(Input::Pointer(Pointer::Move { x: -1.0, y: -1.0 }), ctx);
        home
    }

    /// FR-HOME-8: a new player, with no Favorites and nothing played, sees their Games at once.
    #[test]
    fn home_opens_on_the_first_shelf_with_games_while_favorites_and_continue_playing_are_empty() {
        let mut ctx = Ctx::empty();
        let home = opened(&mut ctx);
        assert_eq!((home.focus.shelf, home.top), (0, 0), "no Games: the first Shelf");

        let games: Vec<_> = (0..3).map(found).collect();
        ctx.library.sync_system("snes", &games, 0).unwrap();
        let mut home = opened(&mut ctx);
        let recently_added = 2; // Shelf::ALL's index for Shelf::RecentlyAdded
        assert_eq!((home.focus.shelf, home.top), (recently_added, recently_added));
        assert_eq!(home.shown_v.value(), home.rest_offset(), "no glide");
        assert_at_rest_on_the_focus(&home);

        // A later reload, with a Game now a Favorite, leaves the focus where it is.
        home.handle(Input::Action(Action::Right), &mut ctx);
        let first = home.shelves[recently_added].games[0].id;
        ctx.library.set_favorite(first, true, 0).unwrap();
        ctx.revision += 1;
        home.handle(Input::Pointer(Pointer::Move { x: -1.0, y: -1.0 }), &mut ctx);
        assert_eq!(home.shelves[0].games.len(), 1, "reloaded");
        assert_eq!(
            (home.focus.shelf, home.focus.col(), home.top),
            (recently_added, 1, 2)
        );

        let home = opened(&mut ctx);
        assert_eq!((home.focus.shelf, home.top), (0, 0), "a Favorite: Favorites");

        // With Favorites and Continue playing not shown, the first Shelf shown with Games.
        let mut ctx = Ctx::empty();
        ctx.library.sync_system("snes", &games, 0).unwrap();
        ctx.home_shelves = HomeShelves::showing(&[Shelf::MostPlayed, Shelf::RecentlyAdded]).unwrap();
        let home = opened(&mut ctx);
        assert_eq!(
            (home.focus.shelf, home.top),
            (1, 0),
            "Recently added, below Most played"
        );
    }

    /// FR-HOME-8 after First-run Setup: the library scans in the background, so Home first loads
    /// with no Games; when they arrive it still opens on them, unless the player moved meanwhile.
    #[test]
    fn home_opens_on_games_that_arrive_after_it_loaded_empty_unless_the_player_moved() {
        let games: Vec<_> = (0..3).map(found).collect();
        let recently_added = 2; // Shelf::ALL's index for Shelf::RecentlyAdded
        let nothing = Input::Pointer(Pointer::Move { x: -1.0, y: -1.0 });

        let mut ctx = Ctx::empty();
        let mut home = opened(&mut ctx);
        assert_eq!(
            (home.focus.shelf, home.top),
            (0, 0),
            "no Games yet: the first Shelf"
        );
        ctx.library.sync_system("snes", &games, 0).unwrap();
        ctx.revision += 1;
        home.handle(nothing, &mut ctx);
        assert_eq!((home.focus.shelf, home.top), (recently_added, recently_added));
        assert_eq!(home.shown_v.value(), home.rest_offset(), "no glide");

        let mut ctx = Ctx::empty();
        let mut home = opened(&mut ctx);
        home.handle(Input::Action(Action::Down), &mut ctx);
        assert_eq!(home.focus.shelf, 1, "the player moved before the Games came");
        ctx.library.sync_system("snes", &games, 0).unwrap();
        ctx.revision += 1;
        home.handle(nothing, &mut ctx);
        assert_eq!(home.shelves[recently_added].games.len(), 3, "reloaded");
        assert_eq!(home.focus.shelf, 1, "no jump");
    }

    /// A sideways wheel or swipe scrolls the Shelf under the pointer, half a cover a notch, and
    /// moves that Shelf's position onto the nearest cover wholly in view; the paging arrow too.
    #[test]
    fn a_sideways_wheel_or_the_paging_arrow_scrolls_a_shelf_and_keeps_its_focus_in_view() {
        let mut ctx = Ctx::empty();
        let mut home = wheel_home(&mut ctx);
        let lay = Layout::now();
        let in_view = |home: &Home, s: usize| {
            let x = home.focus.cols[s] as f32 * lay.pitch - home.hscroll[s];
            x >= 0.0 && x + lay.cover_w <= VIEW_W
        };
        let lower = VIEW_TOP + lay.block_h + 10.0;
        home.side_wheel(1.0, 500.0, lower, &mut ctx);
        assert_eq!(
            home.hscroll,
            [0.0, lay.pitch / 2.0, 0.0, 0.0],
            "only the Shelf under it"
        );
        assert_eq!(home.focus.cols[1], 1, "its first cover left the view");
        assert!(in_view(&home, 1));
        assert_eq!(home.focus.shelf, 0, "the focused Shelf stays");

        home.side_wheel(10.0, 500.0, VIEW_TOP + 10.0, &mut ctx);
        assert_eq!(home.hscroll[0], 5.0 * lay.pitch);
        assert_eq!(home.focus.col(), 5, "the nearest cover wholly in view");
        home.side_wheel(-4.0, 500.0, VIEW_TOP + 10.0, &mut ctx);
        assert_eq!(
            (home.hscroll[0], home.focus.col()),
            (3.0 * lay.pitch, 5),
            "still in view"
        );
        home.side_wheel(-100.0, 500.0, VIEW_TOP + 10.0, &mut ctx);
        assert_eq!(home.hscroll[0], 0.0, "no further than the start");
        assert!(in_view(&home, 0));
        home.side_wheel(1.0, 500.0, VIEW_TOP - 10.0, &mut ctx);
        assert_eq!(home.hscroll[0], 0.0, "over the top bar: nothing");

        home.hits = vec![(Rect::new(0.0, 0.0, 50.0, 50.0), Target::Page(0, 1.0))];
        home.handle(Input::Pointer(Pointer::Click { x: 10.0, y: 10.0 }), &mut ctx);
        assert_eq!(home.hscroll[0], VIEW_W - lay.pitch);
        assert!(home.focus.col() > 0 && in_view(&home, 0), "the focus came along");
    }

    #[test]
    fn a_resize_keeps_the_same_whole_shelves_and_the_focused_cover_showing() {
        let mut home = filled_home(20);
        home.focus.shelf = SHELVES - 1;
        home.focus.cols[SHELVES - 1] = 6;
        home.reveal_shelf();
        home.reveal(SHELVES - 1);
        assert_eq!(home.top, SHELVES - 2);
        for h in [1440.0, 1200.0, 1080.0] {
            ui::with_h(h, || {
                home.refit();
                assert_eq!(home.top, SHELVES - 2, "the same Shelves at {h}");
                assert_eq!(home.shown_v.value(), home.rest_offset(), "no glide at {h}");
                let lay = Layout::now();
                let x = 6.0 * lay.pitch - home.hscroll[SHELVES - 1];
                assert!(
                    x >= 0.0 && x + lay.cover_w <= VIEW_W,
                    "the focused cover at {x} shows at {h}"
                );
            });
        }
    }
}
