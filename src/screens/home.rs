//! Home: the Shelves (FR-HOME-1 to FR-HOME-5), after `home` and `screen_home_empty` in
//! `design/generate_mockups.py`.

use std::path::Path;

use super::details::DetailsPage;
use super::game_grid::GameGrid;
use super::options::OptionsMenu;
use crate::ctx::Ctx;
use crate::library::db::{GameRow, SHELF_SIZE, Shelf};
use crate::systems;
use crate::ui::chrome;
use crate::ui::cover::{self, COVER_GAP, COVER_H, COVER_RADIUS, COVER_W, RING_REACH, ellipsize};
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer, glyph_label};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{BG, LINE, MUTED, SURFACE, SURFACE2, TEXT};
use crate::ui::{M, Rect, W, hit, scroll};

const SHELVES: usize = Shelf::ALL.len();

/// The visible content area between the top bar and the hint bar.
const VIEW_TOP: f32 = 110.0;
const VIEW_BOTTOM: f32 = 1010.0;
const VIEW_H: f32 = VIEW_BOTTOM - VIEW_TOP;
const VIEW_W: f32 = W - 2.0 * M;
/// Distance from the top of a Shelf's block to its title's baseline.
const TITLE_DY: f32 = 46.0;
/// Block heights: a Shelf with Games leaves room for the caption under the focused cover.
const FILLED_H: f32 = 438.0;
const EMPTY_H: f32 = 218.0;
const PITCH: f32 = COVER_W + COVER_GAP;
const SEE_ALL_W: f32 = 150.0;
/// The focused cover grows by this much on each side.
const GROW: f32 = 8.0;
const WHEEL_STEP: f32 = 120.0;

pub fn name(shelf: Shelf) -> &'static str {
    match shelf {
        Shelf::Favorites => "Favorites",
        Shelf::ContinuePlaying => "Continue playing",
        Shelf::RecentlyAdded => "Recently added",
        Shelf::MostPlayed => "Most played",
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

    /// Moves one step in `dir` given each Shelf's item count. Up and Down go to the neighbouring
    /// Shelf's remembered position; Left and Right stop at the ends. Returns whether focus moved.
    pub fn step(&mut self, dir: Action, counts: [usize; SHELVES]) -> bool {
        let before = self.clone();
        match dir {
            Action::Up => self.shelf = self.shelf.saturating_sub(1),
            Action::Down => self.shelf = (self.shelf + 1).min(SHELVES - 1),
            Action::Left => self.cols[self.shelf] = self.col().saturating_sub(1),
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
    /// Games matching the Shelf's rule, beyond the ones shown.
    total: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    Item(usize, usize),
    /// Page a Shelf left (-1) or right (+1).
    Page(usize, f32),
}

pub struct Home {
    shelves: Vec<ShelfRow>,
    /// The `Ctx::revision` the Shelves were loaded at.
    revision: Option<u64>,
    focus: Focus,
    hscroll: [f32; SHELVES],
    vscroll: f32,
    hits: Vec<(Rect, Target)>,
}

impl Default for Home {
    fn default() -> Self {
        Home::new()
    }
}

impl Home {
    /// The Shelves load from the library on first use.
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
            focus: Focus::default(),
            hscroll: [0.0; SHELVES],
            vscroll: 0.0,
            hits: Vec::new(),
        }
    }

    /// Reloads if `ctx.revision` changed since the last load. The previous frame's hit rects may
    /// point at Games that moved or went, so they are dropped until the next draw.
    fn reload_if_changed(&mut self, ctx: &Ctx) {
        if self.revision == Some(ctx.revision) {
            return;
        }
        self.hits.clear();
        for (i, row) in self.shelves.iter_mut().enumerate() {
            let loaded = ctx
                .library
                .shelf(row.shelf, Some(SHELF_SIZE))
                .and_then(|games| Ok((games, ctx.library.shelf_count(row.shelf)?)));
            let (games, total) = loaded.unwrap_or_else(|e| {
                eprintln!("game37: {e}");
                (Vec::new(), 0)
            });
            self.focus.cols[i] = refocus(&row.games, self.focus.cols[i], &games);
            row.games = games;
            row.total = total;
        }
        self.revision = Some(ctx.revision);
        for i in 0..SHELVES {
            self.reveal(i);
        }
        self.reveal_shelf();
    }

    fn counts(&self) -> [usize; SHELVES] {
        std::array::from_fn(|i| item_count(self.shelves[i].games.len()))
    }

    fn block_h(&self, i: usize) -> f32 {
        if self.shelves[i].games.is_empty() {
            EMPTY_H
        } else {
            FILLED_H
        }
    }

    /// Top of each Shelf's block in content coordinates, and the content height.
    fn block_top(&self, i: usize) -> f32 {
        (0..i).map(|k| self.block_h(k)).sum()
    }

    fn content_h(&self) -> f32 {
        self.block_top(SHELVES)
    }

    fn content_w(&self, i: usize) -> f32 {
        let n = self.shelves[i].games.len();
        n as f32 * PITCH + SEE_ALL_W + RING_REACH
    }

    /// Scrolls Shelf `i` sideways to show its remembered position.
    fn reveal(&mut self, i: usize) {
        let x = self.focus.cols[i] as f32 * PITCH - GROW - RING_REACH;
        let s = scroll::keep_visible(self.hscroll[i], x, COVER_W + 2.0 * (GROW + RING_REACH), VIEW_W);
        self.hscroll[i] = scroll::clamp(s, self.content_w(i), VIEW_W);
    }

    /// Scrolls Home to show the focused Shelf.
    fn reveal_shelf(&mut self) {
        let i = self.focus.shelf;
        let s = scroll::keep_visible(self.vscroll, self.block_top(i), self.block_h(i), VIEW_H);
        self.vscroll = scroll::clamp(s, self.content_h(), VIEW_H);
    }

    fn confirm(&self, ctx: &mut Ctx) -> Transition {
        let row = &self.shelves[self.focus.shelf];
        match item_at(row.games.len(), self.focus.col()) {
            Item::Game(i) => Transition::Push(Box::new(DetailsPage::new(row.games[i].clone(), ctx))),
            Item::SeeAll => {
                let shelf = row.shelf;
                Transition::Push(Box::new(GameGrid::new(
                    name(shelf),
                    Box::new(move |library| library.shelf(shelf, None)),
                )))
            }
            Item::Hint => Transition::None,
        }
    }

    /// Y or a right-click on a Game opens its Options Menu (FR-DET-3).
    fn options(&self) -> Transition {
        let row = &self.shelves[self.focus.shelf];
        match item_at(row.games.len(), self.focus.col()) {
            Item::Game(i) => Transition::Push(Box::new(OptionsMenu::new(row.games[i].clone()))),
            _ => Transition::None,
        }
    }

    fn draw_filled(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx, i: usize, base: f32) {
        let focused_shelf = self.focus.shelf == i;
        let col = self.focus.cols[i];
        let y = base + 44.0;
        let row = &self.shelves[i];
        let n = row.games.len();
        let left = M - self.hscroll[i];
        let first = (((self.hscroll[i] - M) / PITCH).floor().max(0.0)) as usize;
        let last = (((self.hscroll[i] + W) / PITCH).ceil() as usize).min(n);
        let mut focused = None;
        for c in first..last {
            let r = Rect::new(left + c as f32 * PITCH, y, COVER_W, COVER_H);
            self.hits.push((r, Target::Item(i, c)));
            if focused_shelf && c == col {
                focused = Some(r);
            } else {
                let game = &row.games[c];
                cover::draw(gfx, r, game, &ctx.artwork.cover(&ctx.library, game), true, false);
            }
        }
        let see_all = Rect::new(left + n as f32 * PITCH, y, SEE_ALL_W, COVER_H);
        if see_all.x < W {
            draw_see_all(gfx, see_all, row.total, focused_shelf && col == n);
            self.hits.push((see_all, Target::Item(i, n)));
        }
        if let Some(r) = focused {
            let big = Rect::new(r.x - GROW, r.y - 12.0, r.w + 2.0 * GROW, r.h + 24.0);
            let game = &row.games[col];
            cover::draw(gfx, big, game, &ctx.artwork.cover(&ctx.library, game), true, true);
            let max_w = (W - M / 2.0 - big.x).min(4.0 * PITCH);
            let title = TextStyle::new(22, Weight::Bold, TEXT);
            let label = ellipsize(&game.title, max_w, |s| gfx.measure(s, title));
            gfx.text(big.x, y + COVER_H + 44.0, &label, title, Align::Left);
            let mut meta = systems::by_id(&game.system).map_or(String::new(), |s| s.name.to_owned());
            if game.playtime_secs > 0 {
                meta = format!("{meta} · {} played", cover::playtime_label(game.playtime_secs));
            }
            let style = TextStyle::new(17, Weight::Regular, MUTED);
            gfx.text(big.x, y + COVER_H + 72.0, &meta, style, Align::Left);
        }
        // Mouse paging, when the Shelf runs past the screen.
        let cy = y + COVER_H / 2.0;
        let max = scroll::clamp(f32::MAX, self.content_w(i), VIEW_W);
        for (show, x, dir) in [
            (self.hscroll[i] > 0.0, M / 2.0, -1.0),
            (self.hscroll[i] < max, W - M / 2.0, 1.0),
        ] {
            if show {
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

    fn draw_empty(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx, i: usize, base: f32) {
        let r = Rect::new(M, base + 22.0, VIEW_W, 140.0);
        if self.focus.shelf == i {
            cover::focus_ring(gfx, r, 14.0);
        }
        gfx.fill_round_rect(r, 14.0, SURFACE.alpha(0.6));
        gfx.stroke_round_rect_dashed(r, 14.0, 2.0, 10.0, 8.0, LINE);
        for k in 0..5 {
            let sq = Rect::new(M + 28.0 + k as f32 * 116.0, base + 44.0, 96.0, 96.0);
            gfx.fill_round_rect(sq, 8.0, SURFACE2.alpha(0.7));
        }
        self.hits.push((r, Target::Item(i, 0)));
        let style = TextStyle::new(22, Weight::Medium, MUTED);
        let (x, y) = (M + 640.0, base + 100.0);
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

fn draw_see_all(gfx: &mut Gfx<'_>, r: Rect, count: usize, focused: bool) {
    if focused {
        cover::focus_ring(gfx, r, COVER_RADIUS);
    }
    gfx.fill_round_rect(r, COVER_RADIUS, SURFACE);
    gfx.stroke_round_rect(r, COVER_RADIUS, 1.0, LINE);
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    gfx.fill_circle(cx, cy - 24.0, 26.0, SURFACE2);
    gfx.line(cx - 6.0, cy - 36.0, cx + 6.0, cy - 24.0, 3.0, TEXT);
    gfx.line(cx + 6.0, cy - 24.0, cx - 6.0, cy - 12.0, 3.0, TEXT);
    gfx.text(
        cx,
        cy + 30.0,
        "See all",
        TextStyle::new(18, Weight::SemiBold, TEXT),
        Align::Centre,
    );
    let label = if count == 1 {
        "1 game".to_owned()
    } else {
        format!("{count} games")
    };
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
        self.reload_if_changed(ctx);
        match input {
            Input::Action(Action::Confirm) => return self.confirm(ctx),
            Input::Action(Action::Options) => return self.options(),
            Input::Action(dir) if dir.is_direction() => {
                if self.focus.step(dir, self.counts()) {
                    self.reveal(self.focus.shelf);
                    self.reveal_shelf();
                }
            }
            Input::Pointer(Pointer::Move { x, y }) => {
                if let Some(Target::Item(s, c)) = hit(&self.hits, x, y) {
                    self.focus.shelf = s;
                    self.focus.cols[s] = c.min(self.counts()[s].saturating_sub(1));
                }
            }
            Input::Pointer(Pointer::Click { x, y }) => match hit(&self.hits, x, y) {
                Some(Target::Item(s, c)) => {
                    self.focus.shelf = s;
                    self.focus.cols[s] = c.min(self.counts()[s].saturating_sub(1));
                    return self.confirm(ctx);
                }
                Some(Target::Page(s, dir)) => {
                    let s_off = self.hscroll[s] + dir * (VIEW_W - PITCH);
                    self.hscroll[s] = scroll::clamp(s_off, self.content_w(s), VIEW_W);
                }
                None => {}
            },
            Input::Pointer(Pointer::RightClick { x, y }) => {
                if let Some(Target::Item(s, c)) = hit(&self.hits, x, y) {
                    self.focus.shelf = s;
                    self.focus.cols[s] = c.min(self.counts()[s].saturating_sub(1));
                    return self.options();
                }
            }
            Input::Pointer(Pointer::Wheel { dy }) => {
                self.vscroll = scroll::clamp(self.vscroll - dy * WHEEL_STEP, self.content_h(), VIEW_H);
            }
            _ => {}
        }
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.reload_if_changed(ctx);
        self.hits.clear();
        gfx.clip(Some(Rect::new(0.0, VIEW_TOP, W, VIEW_H)));
        let title = TextStyle::new(28, Weight::Bold, TEXT);
        for i in 0..SHELVES {
            let top = VIEW_TOP + self.block_top(i) - self.vscroll;
            if top > VIEW_BOTTOM || top + self.block_h(i) < VIEW_TOP {
                continue;
            }
            let base = top + TITLE_DY;
            if self.shelves[i].games.is_empty() {
                self.draw_empty(gfx, ctx, i, base);
            } else {
                self.draw_filled(gfx, ctx, i, base);
            }
            gfx.text(M, base, name(self.shelves[i].shelf), title, Align::Left);
        }
        if self.content_h() - self.vscroll > VIEW_H {
            gfx.fill_round_rect_vgradient(
                Rect::new(0.0, 930.0, W, VIEW_BOTTOM - 930.0),
                0.0,
                BG.alpha(0.0),
                BG,
            );
        }
        gfx.clip(None);
        self.hits = clip_hits(&self.hits, Rect::new(0.0, VIEW_TOP, W, VIEW_H));
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
    fn left_and_right_stop_at_the_ends() {
        let mut f = Focus::default();
        let counts = [3, 1, 21, 1];
        assert!(!f.step(Action::Left, counts));
        assert!(f.step(Action::Right, counts));
        assert!(f.step(Action::Right, counts));
        assert_eq!(f.col(), 2, "See all of a 2-Game Shelf");
        assert!(!f.step(Action::Right, counts));
        assert_eq!(f.col(), 2);
    }

    #[test]
    fn up_and_down_visit_every_shelf_in_order_including_empty_ones() {
        let mut f = Focus::default();
        let counts = [6, 1, 21, 1];
        assert!(!f.step(Action::Up, counts));
        for shelf in 1..SHELVES {
            assert!(f.step(Action::Down, counts));
            assert_eq!(f.shelf, shelf);
        }
        assert!(!f.step(Action::Down, counts));
        assert_eq!(f.col(), 0, "an empty Shelf's hint");
    }

    #[test]
    fn each_shelf_remembers_its_position() {
        let mut f = Focus::default();
        let counts = [6, 21, 21, 1];
        for _ in 0..4 {
            f.step(Action::Right, counts);
        }
        f.step(Action::Down, counts);
        assert_eq!(f.col(), 0, "a Shelf not visited yet starts at its first Game");
        f.step(Action::Right, counts);
        f.step(Action::Up, counts);
        assert_eq!((f.shelf, f.col()), (0, 4));
        f.step(Action::Down, counts);
        assert_eq!((f.shelf, f.col()), (1, 1));
    }

    #[test]
    fn a_remembered_position_past_a_shrunken_shelf_is_clamped() {
        let mut f = Focus {
            shelf: 0,
            cols: [0, 9, 0, 0],
        };
        f.step(Action::Down, [3, 4, 1, 1]);
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
        // Past the actions not built yet, and Rename, to Hide.
        menu.handle(Input::Action(Action::Down), &mut ctx);
        menu.handle(Input::Action(Action::Down), &mut ctx);
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

    #[test]
    fn a_hit_rect_above_the_view_is_clipped_so_a_click_there_misses() {
        let view = Rect::new(0.0, VIEW_TOP, W, VIEW_H);
        // A cover scrolled so most of it sits above the top bar; only its bottom sliver is
        // visible.
        let hits = vec![(
            Rect::new(100.0, VIEW_TOP - 80.0, COVER_W, COVER_H),
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
}
