//! A titled grid of tall covers for a list of Games: "See all" on Home (FR-HOME-3), and a System's
//! Console Page (FR-CAR-3), after `screen_console_page` and `screen_see_all` in
//! `design/generate_mockups.py`.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use super::details::DetailsPage;
use super::options::OptionsMenu;
use crate::ctx::Ctx;
use crate::emulators::Emulator;
use crate::library::db::{DbError, GameRow, Library, SortOrder};
use crate::systems::System;
use crate::ui::anim::{Eased, FOCUS_TAU, SCROLL_TAU};
use crate::ui::chrome;
use crate::ui::cover::{self, CoverPlace, RING_REACH};
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::grid::Grid;
use crate::ui::input::{Action, Input, Pointer, glyph_label};
use crate::ui::logos;
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, BG, Colour, LINE, MUTED, SURFACE, SURFACE2, TEXT, WARN};
use crate::ui::{self, M, Rect, W, hit, scroll};

/// Where a grid's Games come from, in the grid's sort order (ignored by a grid that can't be
/// sorted); called again whenever the library or the order changes.
pub type Source = Box<dyn Fn(&Library, SortOrder) -> Result<Vec<GameRow>, DbError>>;

/// The list of orders the Sort action opens (FR-CAR-5), anchored under the sort chip and
/// right-aligned with it, drawn like the Options Menu's rows.
const LIST_GAP: f32 = 12.0;
const LIST_W: f32 = 360.0;
const LIST_PAD: f32 = 12.0;
const LIST_ROW_H: f32 = 72.0;

/// The muted line under the header line of a grid in a release order (FR-ART-5).
const NOTE_Y: f32 = 268.0;
const NO_KEY_NOTE: &str = "Release dates need a SteamGridDB API key (Settings → Artwork)";
const STOPPED_NOTE: &str = "Release dates can't be fetched right now (SteamGridDB refused the requests)";

/// Whether and how a grid's Sort action changes its order (FR-CAR-5, FR-HOME-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sorting {
    /// No sort chip, no Sort action: Favorites' "See all", in the Shelf's own order.
    Fixed,
    /// The stored global order, which Sort steps for every Console Page: a Console Page.
    Global,
    /// An order of this open grid's own, which Sort steps without reading or writing the stored
    /// one: "See all" of every Game.
    Own,
}

/// A Console Page's extras, absent on Home's "See all": the colour tint, the empty-state hint
/// pointing at the System's folder (FR-CAR-4), and the System itself, to check for a missing
/// Standalone Emulator (FR-CAR-9).
struct Extra {
    tint: Colour,
    empty_hint: String,
    system: &'static System,
}

const TOP: f32 = 288.0;
/// The bottom of the grid's view: the hint bar, which follows the bottom of the canvas (`ui::h`).
fn bottom() -> f32 {
    chrome::hintbar_top()
}
/// The fade over the last row showing, when more lies below.
const FADE_H: f32 = 110.0;
/// The sort chip, after `screen_console_page` in `design/generate_mockups.py`.
const CHIP_Y: f32 = 168.0;
const CHIP_H: f32 = 52.0;
const CHIP_PAD: f32 = 20.0;
const ARROW_W: f32 = 10.0;
/// A Console Page's System logo in place of the title (FR-CAR-2, FR-CAR-3): drawn at about
/// `LOGO_AREA` px² whatever its shape, inside a `LOGO_MAX_W`×`LOGO_MAX_H` box from the title's left
/// edge, centred at `LOGO_MID_Y`, 12 px above the title's capitals: so the tallest logo, the header
/// line and the release note all fit above `TOP`, which keeps two whole rows of 8 showing at 1080.
const LOGO_AREA: f32 = 22_000.0;
const LOGO_MAX_W: f32 = 440.0;
const LOGO_MAX_H: f32 = 96.0;
const LOGO_MID_Y: f32 = 171.0;
/// The baseline of a grid's text title (52 px).
const TITLE_Y: f32 = 202.0;
/// The baseline of the System's name beside a square logo, in the title's style: its capitals
/// (about 0.73 em) centred on the logo.
const NAME_Y: f32 = LOGO_MID_Y + 19.0;
/// Between a square logo and the System's name beside it.
const NAME_GAP: f32 = 20.0;
/// The baseline of the line under the title ("n games · Sorted by …"): its capitals (20 px, about
/// 0.73 em) start 12 px under the tallest logo.
const HEADER_Y: f32 = 246.0;

/// The missing-emulator notice (FR-CAR-9), after `screen_console_page_missing_emulator` in
/// `design/generate_mockups.py`.
const NOTICE_Y: f32 = 278.0;
const NOTICE_H: f32 = 176.0;
/// Gap between the notice and the grid or empty-state message below it.
const NOTICE_GAP: f32 = 40.0;

/// Which of the notice's two buttons has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoticeFocus {
    Copy,
    Check,
}

/// How long "Copied" or "Still not installed" shows on a notice button before reverting to its
/// usual label (US-39).
const FEEDBACK: Duration = Duration::from_secs(2);

/// The least time between two re-sorts of a grid sorted by a release order as Release Dates arrive.
const RESORT_GAP: Duration = Duration::from_secs(1);

/// Whether `order` sorts by Release Date, so a grid in it asks SteamGridDB for the missing ones
/// (FR-ART-5).
fn is_release(order: SortOrder) -> bool {
    matches!(order, SortOrder::NewestRelease | SortOrder::OldestRelease)
}

/// Where focus goes from the notice or the grid combined (US-39): Left/Right move between the
/// notice's two buttons; Down from the notice goes to the grid's first Game, if there are any;
/// Up from the grid's first row returns to the notice's Copy command button. Only called while
/// the notice is shown.
fn step_focus(
    focus: Option<NoticeFocus>,
    grid_focus: usize,
    action: Action,
    len: usize,
    cols: usize,
) -> (Option<NoticeFocus>, usize) {
    match focus {
        Some(NoticeFocus::Copy) if action == Action::Right => (Some(NoticeFocus::Check), grid_focus),
        Some(NoticeFocus::Check) if action == Action::Left => (Some(NoticeFocus::Copy), grid_focus),
        Some(_) if action == Action::Down && len > 0 => (None, 0),
        Some(nf) => (Some(nf), grid_focus),
        None if action == Action::Up && grid_focus < cols => (Some(NoticeFocus::Copy), grid_focus),
        None => (None, step(grid_focus, len, cols, action)),
    }
}

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

/// The focus after a page key in a grid of `len` items, `cols` per row, `rows` of them on a screen
/// (FR-UI-1): Page Up and Page Down move by `rows` rows in the same column, stopping at the first
/// or last row (on a shorter last row, its last item); Home and End go to the first and last item.
pub fn page(focus: usize, len: usize, cols: usize, rows: usize, key: Action) -> usize {
    if len == 0 {
        return 0;
    }
    let by = rows.max(1) * cols;
    match key {
        Action::PageUp if focus >= by => focus - by,
        Action::PageUp => focus % cols,
        Action::PageDown if focus + by < len => focus + by,
        Action::PageDown => ((len - 1) / cols * cols + focus % cols).min(len - 1),
        Action::First => 0,
        Action::Last => len - 1,
        _ => focus,
    }
}

/// How many rows of a grid show in full in a view `view` tall: what a page key moves by, at
/// least 1.
pub fn rows_shown(view: f32, grid: &Grid) -> usize {
    ((view / grid.pitch_y).floor() as usize).max(1)
}

/// Whether `action` is a page key (FR-UI-1).
pub fn is_page_key(action: Action) -> bool {
    matches!(
        action,
        Action::PageUp | Action::PageDown | Action::First | Action::Last
    )
}

/// Where focus goes after the Games were reloaded: the same Game if it is still there, else the
/// same position, clamped.
pub fn refocus(old: &[GameRow], focus: usize, new: &[GameRow]) -> usize {
    old.get(focus)
        .and_then(|g| new.iter().position(|n| n.id == g.id))
        .unwrap_or(focus.min(new.len().saturating_sub(1)))
}

/// The header line under the title: "<n> games", singular when one, with the sort order appended
/// for a sortable grid (FR-CAR-5).
/// The box a Console Page's logo is drawn in from the title's left edge `x`.
fn logo_box(x: f32) -> Rect {
    Rect::new(x, LOGO_MID_Y - LOGO_MAX_H / 2.0, LOGO_MAX_W, LOGO_MAX_H)
}

/// Where the System's name starts beside a Console Page's drawn `logo`: only a square logo
/// (FR-CAR-3) has the name beside it.
fn name_beside(logo: Rect) -> Option<f32> {
    logos::is_square(logo.w, logo.h).then_some(logo.x + logo.w + NAME_GAP)
}

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

/// The notice's headline (FR-CAR-9).
fn notice_title(emulator: &Emulator) -> String {
    format!("{} isn't installed", emulator.name)
}

/// The notice's body line, naming the System above the install command (FR-CAR-9).
fn notice_body(system: &System) -> String {
    format!("Install it to play {} games:", system.name)
}

pub struct GameGrid {
    title: String,
    source: Source,
    games: Vec<GameRow>,
    /// The `Ctx::revision` the Games were loaded at.
    revision: Option<u64>,
    /// The layout for `Ctx::games_per_row` as last seen; a change refits it (FR-UI-7).
    grid: Grid,
    focus: usize,
    scroll: f32,
    /// The canvas height (`ui::h`) `scroll` was last fitted at; a resize refits it in `draw`.
    scroll_h: f32,
    /// What is drawn: `scroll` eased, so the grid glides to the focus.
    shown: Eased,
    /// How far the focused cover's ring has faded in, 0 to 1, and the Game it belongs to.
    ring: Eased,
    ringed: Option<usize>,
    hits: Vec<(Rect, usize)>,
    extra: Option<Extra>,
    sorting: Sorting,
    /// Where the sort chip was last drawn, for the mouse.
    sort_hit: Option<Rect>,
    /// The order shown: for a Console Page the global one, cached from `reload()` so `draw()` never
    /// queries the database (NFR-3). Meaningless when `sorting` is `Fixed`.
    sort_order: SortOrder,
    /// Whether the missing-emulator notice is shown, last computed from `ctx.emulators`
    /// (FR-CAR-9).
    notice_shown: bool,
    /// Which notice button has focus; `None` when the grid has focus, or there's no notice.
    notice_focus: Option<NoticeFocus>,
    /// Where the notice's two buttons were last drawn, for the mouse.
    notice_hits: Option<(Rect, Rect)>,
    /// Transient feedback on a notice button after it's pressed, and until when (US-39).
    feedback: Option<(NoticeFocus, &'static str, Instant)>,
    /// `Artwork::dates_stored` when the Games were loaded: a change re-sorts a grid in a release
    /// order.
    dates_seen: u64,
    /// When the Games were last re-sorted for Release Dates that arrived, to do it at most once
    /// every [`RESORT_GAP`].
    resorted: Option<Instant>,
    /// The `Artwork::release_order_shown` round in which this grid last asked for its Games'
    /// Release Dates: in another, it asks again.
    dating_round: Option<u64>,
    /// This grid's Games whose Release Dates were not stored when it last asked for them, by
    /// library id, for "Getting release dates… n of N" (FR-ART-5); N is `dating_total`.
    dating_ids: HashSet<i64>,
    dating_total: usize,
    /// The list of orders, while open: which row has focus (an index into `SortOrder::ALL`).
    sort_list: Option<usize>,
    /// Where the list's rows were last drawn, for the mouse.
    list_hits: Vec<(Rect, usize)>,
}

impl GameGrid {
    pub fn new(title: impl Into<String>, source: Source) -> GameGrid {
        GameGrid {
            title: title.into(),
            source,
            games: Vec::new(),
            revision: None,
            grid: Grid::default(),
            focus: 0,
            scroll: 0.0,
            scroll_h: ui::h(),
            shown: Eased::new(0.0),
            ring: Eased::new(1.0),
            ringed: None,
            hits: Vec::new(),
            extra: None,
            sorting: Sorting::Fixed,
            sort_hit: None,
            sort_order: SortOrder::default(),
            notice_shown: false,
            notice_focus: None,
            notice_hits: None,
            feedback: None,
            dates_seen: 0,
            resorted: None,
            dating_round: None,
            dating_ids: HashSet::new(),
            dating_total: 0,
            sort_list: None,
            list_hits: Vec::new(),
        }
    }

    /// Confirm on the focused Game opens its Details Page (FR-DET-1).
    fn confirm(&self, ctx: &mut Ctx) -> Transition {
        match self.games.get(self.focus) {
            Some(game) => Transition::Push(Box::new(DetailsPage::new(game.clone(), ctx))),
            None => Transition::None,
        }
    }

    /// Y or a right-click on a Game opens its Options Menu (FR-DET-3).
    fn options(&self, ctx: &Ctx) -> Transition {
        match self.games.get(self.focus) {
            Some(game) => Transition::Push(Box::new(OptionsMenu::new(game.clone(), ctx))),
            None => Transition::None,
        }
    }

    /// Turns this into a Console Page: a `tint` from the System's colour, a sort chip for the stored
    /// global order, `empty_hint` shown in place of the grid when the System has no Games
    /// (FR-CAR-4, FR-CAR-5), and the System itself, so a missing Standalone Emulator can show its
    /// notice (FR-CAR-9).
    pub fn sortable(mut self, tint: Colour, empty_hint: String, system: &'static System) -> GameGrid {
        self.extra = Some(Extra {
            tint,
            empty_hint,
            system,
        });
        self.sorting = Sorting::Global;
        self
    }

    /// Gives this grid a sort chip of its own, starting at `order`: Sort steps the order for this
    /// open grid only, never reading or writing the stored global one (FR-HOME-3).
    pub fn own_sort(mut self, order: SortOrder) -> GameGrid {
        self.sorting = Sorting::Own;
        self.sort_order = order;
        self
    }

    fn is_sortable(&self) -> bool {
        self.sorting != Sorting::Fixed
    }

    /// The order shown, `None` for a grid that can't be sorted, and the Games in it.
    #[cfg(test)]
    pub fn shown_order(&self) -> (Option<SortOrder>, Vec<i64>) {
        let order = self.is_sortable().then_some(self.sort_order);
        (order, self.games.iter().map(|g| g.id).collect())
    }

    /// Loads the Games again; in a release order, also asks SteamGridDB for those of them whose
    /// Release Date is missing (FR-ART-5), which the player's choice of that order allows.
    fn reload(&mut self, ctx: &Ctx) {
        self.load(ctx);
        self.dating_round = None;
        self.keep_dating(ctx, Instant::now());
    }

    /// In a release order, keeps this grid's Release Dates coming while it is drawn (FR-ART-5):
    /// tells Artwork every frame (`now` is the current time), and asks for them again once Artwork
    /// dropped them unasked (no grid in a release order showed for a while, or another grid asked)
    /// or was replaced (an API key entered in Settings).
    fn keep_dating(&mut self, ctx: &Ctx, now: Instant) {
        if !self.is_sortable() || !is_release(self.sort_order) {
            return;
        }
        if self.dating_round != Some(ctx.artwork.release_order_shown(now)) {
            ctx.artwork.ask_release_dates(&ctx.library, &self.games);
            self.dating_ids = self.games.iter().map(|g| g.id).collect();
            self.dating_total = ctx.artwork.dates_waiting(&self.dating_ids);
            // Asking may have dropped another grid's Release Dates, which starts a new round.
            self.dating_round = Some(ctx.artwork.release_order_shown(now));
        }
    }

    /// Loads the Games again in the order shown, keeping focus on the same Game.
    fn load(&mut self, ctx: &Ctx) {
        if self.sorting == Sorting::Global {
            self.sort_order = ctx.library.sort_order().unwrap_or_default();
        }
        let games = (self.source)(&ctx.library, self.sort_order).unwrap_or_else(|e| {
            eprintln!("game37: {e}");
            Vec::new()
        });
        self.focus = refocus(&self.games, self.focus, &games);
        self.games = games;
        if self.games.is_empty() {
            // An empty grid draws no sort chip or list: an open list closes rather than keep
            // taking the input.
            self.sort_list = None;
            self.list_hits.clear();
        }
        self.revision = Some(ctx.revision);
        self.dates_seen = ctx.artwork.dates_stored();
        // The previous frame's hit rects may point at Games that moved or went: drop them until the
        // next draw.
        self.hits.clear();
        self.sort_hit = None;
        self.scroll = scroll::clamp(self.scroll, self.content_h(), bottom() - self.top_y());
        self.reveal_focus();
    }

    fn reload_if_changed(&mut self, ctx: &Ctx) {
        if self.revision != Some(ctx.revision) {
            self.reload(ctx);
        }
    }

    /// In a release order, sorts the Games again once Release Dates arrived, at most once every
    /// [`RESORT_GAP`] (`now` is the current time), keeping focus on the same Game, and keeping it in
    /// view only if it was: the player may have scrolled away from it.
    fn resort_if_dated(&mut self, ctx: &Ctx, now: Instant) {
        let due = self.resorted.is_none_or(|at| now >= at + RESORT_GAP);
        if self.is_sortable()
            && is_release(self.sort_order)
            && ctx.artwork.dates_stored() != self.dates_seen
            && due
        {
            let (in_view, before) = (self.focus_in_view(), self.scroll);
            self.load(ctx);
            if !in_view {
                self.scroll = scroll::clamp(before, self.content_h(), bottom() - self.top_y());
            }
            self.resorted = Some(now);
        }
    }

    /// Whether any of the focused Game's cover is in the grid's view.
    fn focus_in_view(&self) -> bool {
        let top = (self.focus / self.grid.cols) as f32 * self.grid.pitch_y;
        top < self.scroll + bottom() - self.top_y() && top + self.grid.cover_h > self.scroll
    }

    /// Follows a change to the Games per row, made in Settings while this grid was open below it:
    /// the same Game keeps focus and is scrolled into view (FR-UI-7).
    fn sync_grid(&mut self, ctx: &Ctx) {
        let grid = Grid::new(ctx.games_per_row);
        if grid != self.grid {
            self.grid = grid;
            // The previous frame's hit rects are for the old layout.
            self.hits.clear();
            self.reveal_focus();
        }
    }

    /// This Console Page's System's Standalone Emulator, if it's an External System whose
    /// emulator isn't installed (FR-CAR-9, FR-CAR-10): `None` on Home's "See all" and for
    /// Embedded Systems.
    fn missing_emulator(&self, ctx: &Ctx) -> Option<&'static Emulator> {
        self.extra.as_ref().and_then(|e| ctx.emulators.missing(e.system))
    }

    /// Where the grid (or the empty-state message) starts: lower when the notice is shown above
    /// it (FR-CAR-9).
    fn top_y(&self) -> f32 {
        if self.notice_shown {
            NOTICE_Y + NOTICE_H + NOTICE_GAP
        } else {
            TOP
        }
    }

    /// Re-checks whether the notice should show, read fresh from `ctx.emulators` every call
    /// (cheap) rather than cached across frames (FR-CAR-9). Focus starts on Copy command when the
    /// notice newly appears; when it clears, focus moves to the grid, or to nothing on an empty
    /// page (US-39).
    fn sync_notice(&mut self, ctx: &Ctx) {
        let shown = self.missing_emulator(ctx).is_some();
        if shown == self.notice_shown {
            return;
        }
        self.notice_shown = shown;
        self.feedback = None;
        if !shown {
            // The buttons are gone: a pointer over where they were must not focus them.
            self.notice_hits = None;
        }
        if shown {
            self.notice_focus = Some(NoticeFocus::Copy);
        } else {
            self.notice_focus = None;
            self.focus = 0;
        }
    }

    /// Presses the notice button that currently has focus: Copy command puts the install command
    /// on the clipboard through `Ctx`'s seam; Check again re-detects the installed emulators and
    /// re-syncs the notice, which clears it when the emulator is now found (US-39).
    fn activate_notice(&mut self, ctx: &mut Ctx) {
        let (Some(focus), Some(emulator)) = (self.notice_focus, self.missing_emulator(ctx)) else {
            return;
        };
        match focus {
            NoticeFocus::Copy => {
                ctx.clipboard_request = Some(emulator.install_command());
                self.feedback = Some((NoticeFocus::Copy, "Copied", Instant::now()));
            }
            NoticeFocus::Check => {
                ctx.emulators = (ctx.detect_emulators)();
                self.sync_notice(ctx);
                if self.notice_shown {
                    self.feedback = Some((NoticeFocus::Check, "Still not installed", Instant::now()));
                }
            }
        }
    }

    /// Which notice button, if any, is under a point; `None` while the notice is hidden.
    fn notice_button_at(&self, x: f32, y: f32) -> Option<NoticeFocus> {
        if !self.notice_shown {
            return None;
        }
        let (copy, check) = self.notice_hits?;
        if copy.contains(x, y) {
            Some(NoticeFocus::Copy)
        } else if check.contains(x, y) {
            Some(NoticeFocus::Check)
        } else {
            None
        }
    }

    /// Opens the list of orders with the current one focused (FR-CAR-5). A no-op on an empty grid,
    /// where sorting would change the order with no visible effect.
    fn open_sort_list(&mut self) {
        if self.is_sortable() && !self.games.is_empty() {
            self.sort_list = SortOrder::ALL.iter().position(|&o| o == self.sort_order);
        }
    }

    /// Picks `order` from the list and closes it (FR-CAR-5): a Console Page stores it as the
    /// global order, "See all" of every Game keeps it to itself (FR-HOME-3); the grid is sorted
    /// and the focus goes to its first Game, scrolled to the top. Picking the current order only
    /// closes the list.
    fn pick_sort(&mut self, order: SortOrder, ctx: &mut Ctx) {
        self.sort_list = None;
        self.list_hits.clear();
        if order == self.sort_order {
            return;
        }
        if self.sorting == Sorting::Global
            && let Err(e) = ctx.library.set_sort_order(order)
        {
            eprintln!("game37: can't save the sort order: {e}");
        }
        self.sort_order = order;
        self.reload(ctx);
        self.focus = 0;
        self.scroll = 0.0;
    }

    /// Input while the list of orders is open, which takes all of it: Up and Down move, Confirm
    /// or a click on a row picks, Back or a click outside closes it unchanged, and the mouse over
    /// a row focuses it.
    fn handle_sort_list(&mut self, input: Input, ctx: &mut Ctx) {
        let Some(row) = self.sort_list else { return };
        let last = SortOrder::ALL.len() - 1;
        match input {
            Input::Action(Action::Up) => self.sort_list = Some(row.saturating_sub(1)),
            Input::Action(Action::Down) => self.sort_list = Some((row + 1).min(last)),
            Input::Action(Action::Confirm) => self.pick_sort(SortOrder::ALL[row], ctx),
            Input::Action(Action::Back) => {
                self.sort_list = None;
                self.list_hits.clear();
            }
            Input::Pointer(Pointer::Click { x, y }) => match hit(&self.list_hits, x, y) {
                Some(i) => self.pick_sort(SortOrder::ALL[i], ctx),
                None => {
                    self.sort_list = None;
                    self.list_hits.clear();
                }
            },
            Input::Pointer(Pointer::Move { x, y }) => {
                if let Some(i) = hit(&self.list_hits, x, y) {
                    self.sort_list = Some(i);
                }
            }
            _ => {}
        }
    }

    /// The muted line under the header line of a grid in a release order, saying why its Games
    /// are not in date order yet (FR-ART-5): without an API key, that one is needed; with one but
    /// SteamGridDB's requests stopped this run, that they can't be fetched; while this grid's
    /// Release Dates are being asked for, how many of them are in.
    fn release_note(&self, ctx: &Ctx) -> Option<String> {
        if !self.is_sortable() || !is_release(self.sort_order) || self.games.is_empty() {
            return None;
        }
        if ctx.artwork.key().is_none() {
            return Some(NO_KEY_NOTE.to_owned());
        }
        if !ctx.artwork.online() {
            return Some(STOPPED_NOTE.to_owned());
        }
        let waiting = ctx.artwork.dates_waiting(&self.dating_ids);
        (waiting > 0).then(|| {
            let total = self.dating_total.max(waiting);
            format!("Getting release dates… {} of {total}", total - waiting)
        })
    }

    /// How many rows show in full: what Page Up and Page Down move by (FR-UI-1).
    fn page_rows(&self) -> usize {
        rows_shown(bottom() - self.top_y(), &self.grid)
    }

    fn content_h(&self) -> f32 {
        self.grid.content_h(self.games.len())
    }

    fn reveal_focus(&mut self) {
        let row = (self.focus / self.grid.cols) as f32;
        let bottom_minus_top = bottom() - self.top_y();
        let s = scroll::keep_visible(
            self.scroll,
            row * self.grid.pitch_y - RING_REACH,
            self.grid.cover_h + 2.0 * RING_REACH + 40.0,
            bottom_minus_top,
        );
        self.scroll = scroll::clamp(s, self.content_h(), bottom_minus_top);
        self.scroll_h = ui::h();
    }

    /// After a resize (`ui::h` changed), scrolls again so the focused Game shows and no space is
    /// left below the last row.
    fn refit(&mut self) {
        if self.scroll_h != ui::h() {
            self.reveal_focus();
        }
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

    /// The open list of orders under the sort chip `chip`, right-aligned with it: one row per
    /// order, the focused one highlighted as in the Options Menu, the current one ticked.
    fn draw_sort_list(&mut self, gfx: &mut Gfx<'_>, chip: Rect, focused: usize) {
        self.list_hits.clear();
        let h = SortOrder::ALL.len() as f32 * LIST_ROW_H + 2.0 * LIST_PAD;
        let panel = Rect::new(chip.x + chip.w - LIST_W, chip.y + chip.h + LIST_GAP, LIST_W, h);
        gfx.fill_round_rect(panel, 20.0, SURFACE);
        gfx.stroke_round_rect(panel, 20.0, 1.0, LINE);
        for (i, &order) in SortOrder::ALL.iter().enumerate() {
            let r = Rect::new(
                panel.x + LIST_PAD,
                panel.y + LIST_PAD + i as f32 * LIST_ROW_H,
                panel.w - 2.0 * LIST_PAD,
                LIST_ROW_H,
            );
            let on = i == focused;
            if on {
                gfx.fill_round_rect(r, 14.0, SURFACE2);
                gfx.fill_round_rect(Rect::new(r.x, r.y + 16.0, 5.0, 40.0), 2.0, ACCENT);
            }
            let weight = if on { Weight::SemiBold } else { Weight::Medium };
            gfx.text(
                r.x + 32.0,
                r.y + 45.0,
                order.label(),
                TextStyle::new(22, weight, TEXT),
                Align::Left,
            );
            if order == self.sort_order {
                // A tick, drawn rather than a text glyph, as the chip's chevron is.
                let (tx, ty) = (r.x + r.w - 24.0 - 18.0, r.y + 30.0);
                gfx.line(tx, ty + 7.0, tx + 6.0, ty + 13.0, 3.0, ACCENT);
                gfx.line(tx + 6.0, ty + 13.0, tx + 18.0, ty, 3.0, ACCENT);
            }
            self.list_hits.push((r, i));
        }
    }

    /// The empty-state message in place of the grid, below the notice when it's shown (FR-CAR-4,
    /// FR-CAR-9, US-11).
    fn draw_empty(&self, gfx: &mut Gfx<'_>) {
        let Some(extra) = &self.extra else { return };
        let top = self.top_y();
        gfx.text(
            W / 2.0,
            top + 140.0,
            "No games yet",
            TextStyle::new(28, Weight::Bold, TEXT),
            Align::Centre,
        );
        gfx.text(
            W / 2.0,
            top + 180.0,
            &extra.empty_hint,
            TextStyle::new(20, Weight::Medium, MUTED),
            Align::Centre,
        );
    }

    /// The missing-emulator notice above the grid or the empty-state message (FR-CAR-9), after
    /// `screen_console_missing_emulator` in `design/generate_mockups.py`. Returns where its Copy
    /// command and Check again buttons were drawn, for the mouse.
    fn draw_notice(&self, gfx: &mut Gfx<'_>, emulator: &Emulator, system: &System) -> (Rect, Rect) {
        let r = Rect::new(M, NOTICE_Y, W - 2.0 * M, NOTICE_H);
        gfx.fill_round_rect(r, 18.0, WARN.alpha(0.1));
        gfx.stroke_round_rect(r, 18.0, 2.0, WARN.alpha(0.55));
        gfx.fill_circle(M + 52.0, NOTICE_Y + 52.0, 24.0, WARN);
        gfx.text(
            M + 52.0,
            NOTICE_Y + 62.0,
            "!",
            TextStyle::new(28, Weight::ExtraBold, BG),
            Align::Centre,
        );
        gfx.text(
            M + 100.0,
            NOTICE_Y + 50.0,
            &notice_title(emulator),
            TextStyle::new(26, Weight::Bold, TEXT),
            Align::Left,
        );
        gfx.text(
            M + 100.0,
            NOTICE_Y + 82.0,
            &notice_body(system),
            TextStyle::new(18, Weight::Medium, MUTED),
            Align::Left,
        );

        let cy = NOTICE_Y + 106.0;
        let cmd = emulator.install_command();
        let cmd_box = Rect::new(M + 100.0, cy, 640.0, 48.0);
        gfx.fill_round_rect(cmd_box, 10.0, BG.alpha(0.4));
        let mono = TextStyle::new(18, Weight::Medium, TEXT);
        gfx.text(
            cmd_box.x + 20.0,
            cy + 31.0,
            "$",
            TextStyle::new(18, Weight::SemiBold, MUTED),
            Align::Left,
        );
        gfx.text(cmd_box.x + 42.0, cy + 31.0, &cmd, mono, Align::Left);

        let copy_label = self.notice_label(NoticeFocus::Copy, "Copy command");
        let copy = Rect::new(M + 760.0, cy, 206.0, 48.0);
        self.draw_notice_button(gfx, copy, NoticeFocus::Copy, copy_label);

        let check_label = self.notice_label(NoticeFocus::Check, "Check again");
        let check = Rect::new(copy.x + copy.w + 20.0, cy, 168.0, 48.0);
        self.draw_notice_button(gfx, check, NoticeFocus::Check, check_label);

        (copy, check)
    }

    /// A notice button's current label: transient feedback while it hasn't expired, else
    /// `default` (US-39).
    fn notice_label(&self, button: NoticeFocus, default: &'static str) -> &'static str {
        match self.feedback {
            Some((b, text, since)) if b == button && since.elapsed() < FEEDBACK => text,
            _ => default,
        }
    }

    fn draw_notice_button(&self, gfx: &mut Gfx<'_>, r: Rect, button: NoticeFocus, label: &str) {
        let focused = self.notice_focus == Some(button);
        if focused {
            gfx.stroke_round_rect(r.inset(-5.0), 15.0, 3.0, TEXT);
        }
        gfx.fill_round_rect(r, 10.0, SURFACE);
        if !focused {
            gfx.stroke_round_rect(r, 10.0, 2.0, LINE);
        }
        gfx.text(
            r.x + r.w / 2.0,
            r.y + r.h / 2.0 + 6.0,
            label,
            TextStyle::new(18, Weight::SemiBold, TEXT),
            Align::Centre,
        );
    }
}

impl Screen for GameGrid {
    fn title(&self) -> &str {
        &self.title
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        self.sync_grid(ctx);
        self.reload_if_changed(ctx);
        self.resort_if_dated(ctx, Instant::now());
        self.sync_notice(ctx);
        if self.sort_list.is_some() {
            self.handle_sort_list(input, ctx);
            return Transition::None;
        }
        match input {
            Input::Action(Action::Back) => return Transition::Pop,
            Input::Action(Action::Confirm) if self.notice_focus.is_some() => self.activate_notice(ctx),
            Input::Action(Action::Sort) if self.is_sortable() && self.notice_focus.is_none() => {
                self.open_sort_list()
            }
            Input::Action(key) if is_page_key(key) && self.notice_focus.is_none() => {
                self.focus = page(
                    self.focus,
                    self.games.len(),
                    self.grid.cols,
                    self.page_rows(),
                    key,
                );
                self.reveal_focus();
            }
            Input::Action(dir) if dir.is_direction() => {
                self.feedback = None;
                if self.notice_shown {
                    let cols = self.grid.cols;
                    let (nf, gf) = step_focus(self.notice_focus, self.focus, dir, self.games.len(), cols);
                    self.notice_focus = nf;
                    self.focus = gf;
                } else {
                    self.focus = step(self.focus, self.games.len(), self.grid.cols, dir);
                }
                if self.notice_focus.is_none() {
                    self.reveal_focus();
                }
            }
            Input::Pointer(Pointer::Click { x, y }) if self.sort_hit.is_some_and(|r| r.contains(x, y)) => {
                self.open_sort_list();
            }
            Input::Pointer(Pointer::Click { x, y }) if self.notice_button_at(x, y).is_some() => {
                self.notice_focus = self.notice_button_at(x, y);
                self.activate_notice(ctx);
            }
            Input::Pointer(Pointer::Move { x, y }) if self.notice_button_at(x, y).is_some() => {
                self.notice_focus = self.notice_button_at(x, y);
            }
            Input::Action(Action::Confirm) => return self.confirm(ctx),
            Input::Action(Action::Options) if self.notice_focus.is_none() => return self.options(ctx),
            Input::Pointer(Pointer::RightClick { x, y }) => {
                if let Some(i) = hit(&self.hits, x, y) {
                    self.focus = i.min(self.games.len().saturating_sub(1));
                    self.notice_focus = None;
                    return self.options(ctx);
                }
            }
            Input::Pointer(Pointer::Move { x, y }) => {
                if let Some(i) = hit(&self.hits, x, y) {
                    self.focus = i.min(self.games.len().saturating_sub(1));
                    self.notice_focus = None;
                }
            }
            Input::Pointer(Pointer::Click { x, y }) => {
                if let Some(i) = hit(&self.hits, x, y) {
                    self.focus = i.min(self.games.len().saturating_sub(1));
                    self.notice_focus = None;
                    return self.confirm(ctx);
                }
            }
            Input::Pointer(Pointer::Wheel { dy }) => {
                let bottom_minus_top = bottom() - self.top_y();
                // Half a row per wheel notch.
                let wheel_step = self.grid.pitch_y / 2.0;
                self.scroll =
                    scroll::clamp(self.scroll - dy * wheel_step, self.content_h(), bottom_minus_top);
                self.scroll_h = ui::h();
            }
            _ => {}
        }
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.sync_grid(ctx);
        self.reload_if_changed(ctx);
        self.resort_if_dated(ctx, Instant::now());
        self.keep_dating(ctx, Instant::now());
        self.sync_notice(ctx);
        self.refit();
        let mut title_x = M;
        if let Some(extra) = &self.extra {
            chrome::background_tinted(gfx, extra.tint, 0.35);
            gfx.fill_round_rect(Rect::new(M, 152.0, 8.0, 76.0), 4.0, extra.tint);
            title_x += 30.0;
        }
        // A Console Page is headed by its System's logo in place of its name (FR-CAR-3), with the
        // name beside a square one.
        let logo = self.extra.as_ref().and_then(|extra| {
            let r = logos::draw(gfx, extra.system.id, LOGO_AREA, logo_box(title_x), Align::Left)?;
            Some((r, extra.system.name))
        });
        let title = TextStyle::new(52, Weight::ExtraBold, TEXT);
        match logo {
            Some((r, name)) => {
                if let Some(x) = name_beside(r) {
                    gfx.text(x, NAME_Y, name, title, Align::Left);
                }
            }
            None => {
                gfx.text(title_x, TITLE_Y, &self.title, title, Align::Left);
            }
        }
        self.sort_hit = None;
        self.notice_hits = None;
        if let Some(emulator) = self.missing_emulator(ctx) {
            let system = self
                .extra
                .as_ref()
                .expect("the notice only shows on a Console Page")
                .system;
            self.notice_hits = Some(self.draw_notice(gfx, emulator, system));
        }
        // An empty Console Page shows no header line, sort chip or grid: sorting it would change
        // the global order with no visible effect.
        let empty_console_page = self.extra.is_some() && self.games.is_empty();
        if !empty_console_page {
            let order = (self.is_sortable() && !self.games.is_empty()).then_some(self.sort_order);
            gfx.text(
                title_x + 2.0,
                HEADER_Y,
                &header(self.games.len(), order),
                TextStyle::new(20, Weight::Medium, MUTED),
                Align::Left,
            );
            if let Some(order) = order {
                self.sort_hit = Some(self.draw_sort_chip(gfx, order, ctx));
            }
            if let Some(note) = self.release_note(ctx) {
                let style = TextStyle::new(17, Weight::Medium, MUTED);
                gfx.text(title_x + 2.0, NOTE_Y, &note, style, Align::Left);
            }
        }

        self.hits.clear();
        if empty_console_page {
            self.draw_empty(gfx);
            return;
        }
        let (top, bottom) = (self.top_y(), bottom());
        self.shown.set(self.scroll);
        let scroll = self.shown.step(ctx.animate, SCROLL_TAU);
        if self.ringed != Some(self.focus) {
            self.ring.jump(if self.ringed.is_some() { 0.0 } else { 1.0 });
            self.ring.set(1.0);
            self.ringed = Some(self.focus);
        }
        let ring = self.ring.step(ctx.animate, FOCUS_TAU);
        let clip_top = top - RING_REACH - 4.0;
        gfx.clip(Some(Rect::new(0.0, clip_top, W, bottom - clip_top)));
        let grid = self.grid;
        let first_row = ((scroll - RING_REACH) / grid.pitch_y).floor().max(0.0) as usize;
        let last_row = ((scroll + bottom - top) / grid.pitch_y).ceil() as usize;
        // The focused cover's Playtime fades in with its ring (FR-UI-8).
        let caption = TextStyle::new(17, Weight::Medium, MUTED.alpha(ring));
        for i in first_row * grid.cols..((last_row + 1) * grid.cols).min(self.games.len()) {
            let r = grid.cover(i, top - scroll);
            let game = &self.games[i];
            // While a notice button has focus, no cover is drawn focused.
            let art = ctx.artwork.cover(&ctx.library, game);
            let focused = i == self.focus && self.notice_focus.is_none();
            cover::draw_focused(gfx, r, game, &art, true, if focused { ring } else { 0.0 });
            if cover::shows_playtime(ctx.show_playtime, CoverPlace::Grid, focused, game.playtime_secs) {
                gfx.text(
                    r.x,
                    r.y + r.h + 32.0,
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
                    (r.y + r.h).min(bottom) - r.y.max(clip_top),
                ),
                i,
            ));
        }
        if self.content_h() - scroll > bottom - top {
            let fade = Rect::new(0.0, bottom - FADE_H, W, FADE_H);
            gfx.fill_round_rect_vgradient(fade, 0.0, BG.alpha(0.0), BG);
        }
        gfx.clip(None);
        if self.games.is_empty() {
            gfx.text(
                M,
                top + 40.0,
                "No games here yet",
                TextStyle::new(22, Weight::Medium, MUTED),
                Align::Left,
            );
        }
        if let (Some(focused), Some(chip)) = (self.sort_list, self.sort_hit) {
            self.draw_sort_list(gfx, chip, focused);
        }
    }

    fn hints(&self) -> Vec<Hint> {
        if self.sort_list.is_some() {
            return vec![
                Hint::new(&[Action::Confirm], "Sort"),
                Hint::new(&[Action::Back], "Cancel"),
            ];
        }
        match self.notice_focus {
            Some(NoticeFocus::Copy) => vec![
                Hint::new(&[Action::Confirm], "Copy command"),
                Hint::new(&[Action::Back], "Back"),
            ],
            Some(NoticeFocus::Check) => vec![
                Hint::new(&[Action::Confirm], "Check again"),
                Hint::new(&[Action::Back], "Back"),
            ],
            None => {
                let mut hints = Vec::new();
                if !self.games.is_empty() {
                    hints.push(Hint::new(&[Action::Confirm], "Details"));
                    hints.push(Hint::new(&[Action::Options], "Options"));
                }
                if self.is_sortable() && !self.games.is_empty() {
                    hints.push(Hint::new(&[Action::Sort], "Sort"));
                }
                hints.push(Hint::new(&[Action::Back], "Back"));
                hints
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulators::Emulators;
    use crate::systems;

    /// SNES: an Embedded System, so it never shows the missing-emulator notice.
    fn snes() -> &'static System {
        systems::by_id("snes").unwrap()
    }

    /// Wii U: an External System, with Cemu as its Standalone Emulator.
    fn wii_u() -> &'static System {
        systems::by_id("wiiu").unwrap()
    }

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

    /// A square logo (Switch 2's) is held by the box's height and has the System's name 20 px to its
    /// right, centred on it in the title's style; a wide one (3DS's) has no name beside it. Either
    /// way the line under the header starts at least 12 px under the logo.
    #[test]
    fn a_square_logo_has_the_systems_name_beside_it() {
        let x = M + 30.0;
        let square = logos::fit_area(340.0, 340.0, LOGO_AREA, logo_box(x), Align::Left);
        assert_eq!(square, Rect::new(x, 123.0, 96.0, 96.0));
        assert_eq!(name_beside(square), Some(x + 96.0 + 20.0));
        // Capitals 0.73 em of 52 px, centred on the logo.
        assert!((NAME_Y - 0.73 * 52.0 / 2.0 - (square.y + square.h / 2.0)).abs() < 1.0);

        let wide = logos::fit_area(800.0, 95.0, LOGO_AREA, logo_box(x), Align::Left);
        assert!((wide.w * wide.h / LOGO_AREA - 1.0).abs() < 0.15, "{wide:?}");
        assert!(wide.w <= LOGO_MAX_W && wide.h <= LOGO_MAX_H);
        assert_eq!(name_beside(wide), None);

        let lowest_logo = LOGO_MID_Y + LOGO_MAX_H / 2.0;
        assert!(HEADER_Y - 0.73 * 20.0 >= lowest_logo + 12.0);
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
        Box::new(|library, _| library.system_games("snes", SortOrder::AZ))
    }

    /// Opens the list of orders with the Sort action and picks `order` from it with Up or Down
    /// and Confirm.
    fn pick(grid: &mut GameGrid, ctx: &mut Ctx, order: SortOrder) {
        grid.handle(Input::Action(Action::Sort), ctx);
        let target = SortOrder::ALL.iter().position(|&o| o == order).unwrap();
        while let Some(row) = grid.sort_list
            && row != target
        {
            let dir = if row < target { Action::Down } else { Action::Up };
            grid.handle(Input::Action(dir), ctx);
        }
        grid.handle(Input::Action(Action::Confirm), ctx);
    }

    /// SNES's Console Page of 50 Games (7 rows at 8 per row), sorted by the stored global order,
    /// loaded, the first Game focused.
    fn fifty_sorted(ctx: &mut Ctx) -> GameGrid {
        let games: Vec<_> = (0..50).map(found).collect();
        ctx.library.sync_system("snes", &games, 0).unwrap();
        let source: Source = Box::new(|library, order| library.system_games("snes", order));
        let mut grid = GameGrid::new("SNES", source).sortable(Colour::hex(0), "hint".into(), snes());
        grid.handle(Input::Action(Action::Back), ctx); // initial load
        grid
    }

    #[test]
    fn sort_opens_the_list_on_the_current_order_and_back_closes_it_unchanged() {
        let mut ctx = Ctx::empty();
        ctx.library.set_sort_order(SortOrder::MostPlayed).unwrap();
        let mut grid = fifty_sorted(&mut ctx);
        grid.handle(Input::Action(Action::Sort), &mut ctx);
        assert_eq!(grid.sort_list, Some(3), "Most played, the fourth order");
        assert_eq!(
            grid.hints(),
            [
                Hint::new(&[Action::Confirm], "Sort"),
                Hint::new(&[Action::Back], "Cancel")
            ]
        );
        grid.handle(Input::Action(Action::Down), &mut ctx);
        assert!(
            matches!(
                grid.handle(Input::Action(Action::Back), &mut ctx),
                Transition::None
            ),
            "Back closes the list, not the grid"
        );
        assert_eq!(grid.sort_list, None);
        assert_eq!(grid.sort_order, SortOrder::MostPlayed);
        assert_eq!(ctx.library.sort_order().unwrap(), SortOrder::MostPlayed);
    }

    /// A grid has nothing to scroll sideways: Shift and the wheel, or a sideways swipe, scroll it
    /// as the wheel does, right as down.
    #[test]
    fn a_sideways_wheel_scrolls_a_grid_as_the_wheel_does() {
        let mut ctx = Ctx::empty();
        let mut grid = fifty_sorted(&mut ctx);
        grid.side_wheel(2.0, 960.0, 540.0, &mut ctx);
        assert_eq!(grid.scroll, grid.grid.pitch_y, "two notches: a row down");
        let mut wheeled = fifty_sorted(&mut ctx);
        wheeled.handle(Input::Pointer(Pointer::Wheel { dy: -2.0 }), &mut ctx);
        assert_eq!(grid.scroll, wheeled.scroll);
        grid.side_wheel(-2.0, 960.0, 540.0, &mut ctx);
        assert_eq!(grid.scroll, 0.0, "left: back up");
    }

    /// A background reload that empties the grid while the list is open closes the list: an empty
    /// grid draws none, so it must not keep taking the input.
    #[test]
    fn a_reload_that_empties_the_grid_closes_the_sort_list() {
        let mut ctx = Ctx::empty();
        let mut grid = fifty_sorted(&mut ctx);
        grid.handle(Input::Action(Action::Sort), &mut ctx);
        assert!(grid.sort_list.is_some());
        ctx.library.sync_system("snes", &[], 0).unwrap();
        ctx.revision += 1;
        let back = grid.handle(Input::Action(Action::Back), &mut ctx);
        assert!(grid.games.is_empty());
        assert_eq!(grid.sort_list, None);
        assert!(matches!(back, Transition::Pop), "Back reaches the grid");
    }

    #[test]
    fn picking_an_order_sorts_stores_it_and_focuses_the_first_game_at_the_top() {
        let mut ctx = Ctx::empty();
        let mut grid = fifty_sorted(&mut ctx);
        grid.handle(Input::Action(Action::Last), &mut ctx);
        grid.handle(Input::Action(Action::PageUp), &mut ctx); // Game 33, far from the first
        assert!(grid.focus == 33 && grid.scroll > 0.0);
        let last = grid.games[49].id;

        grid.handle(Input::Action(Action::Sort), &mut ctx);
        grid.handle(Input::Action(Action::Down), &mut ctx);
        grid.handle(Input::Action(Action::Confirm), &mut ctx);

        assert_eq!(grid.sort_list, None);
        assert_eq!(grid.sort_order, SortOrder::ZA);
        assert_eq!(ctx.library.sort_order().unwrap(), SortOrder::ZA);
        assert_eq!(grid.games[0].id, last, "Z–A puts the last Game first");
        assert_eq!((grid.focus, grid.scroll), (0, 0.0), "the first Game, at the top");
    }

    #[test]
    fn picking_on_all_games_leaves_the_stored_order_untouched() {
        let mut ctx = Ctx::empty();
        ctx.library.sync_system("snes", &[found(0), found(1)], 0).unwrap();
        let source: Source = Box::new(|library, order| library.all_games(order));
        let mut grid = GameGrid::new("All games", source).own_sort(SortOrder::RecentlyAdded);
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        pick(&mut grid, &mut ctx, SortOrder::ZA);
        assert_eq!(grid.sort_order, SortOrder::ZA);
        assert_eq!(
            ctx.library.sort_order().unwrap(),
            SortOrder::AZ,
            "the default, still"
        );
    }

    #[test]
    fn a_click_on_the_chip_opens_the_list_a_click_on_a_row_picks_and_one_outside_closes() {
        let mut ctx = Ctx::empty();
        let mut grid = fifty_sorted(&mut ctx);
        let chip = Rect::new(1500.0, CHIP_Y, 300.0, CHIP_H);
        grid.sort_hit = Some(chip); // as last drawn
        grid.handle(Input::Pointer(Pointer::Click { x: 1600.0, y: 190.0 }), &mut ctx);
        assert_eq!(grid.sort_list, Some(0));

        // As drawn: one row per order.
        let row = |i: usize| Rect::new(1450.0, 240.0 + i as f32 * LIST_ROW_H, 336.0, LIST_ROW_H);
        grid.list_hits = (0..SortOrder::ALL.len()).map(|i| (row(i), i)).collect();
        grid.handle(
            Input::Pointer(Pointer::Move {
                x: 1500.0,
                y: 240.0 + 2.5 * LIST_ROW_H,
            }),
            &mut ctx,
        );
        assert_eq!(grid.sort_list, Some(2), "hovering a row focuses it");
        grid.handle(
            Input::Pointer(Pointer::Click {
                x: 1500.0,
                y: 240.0 + 4.5 * LIST_ROW_H,
            }),
            &mut ctx,
        );
        assert_eq!(grid.sort_list, None);
        assert_eq!(grid.sort_order, SortOrder::RecentlyAdded);

        grid.sort_hit = Some(chip); // drawn again after the re-sort
        grid.handle(Input::Pointer(Pointer::Click { x: 1600.0, y: 190.0 }), &mut ctx);
        assert_eq!(grid.sort_list, Some(4));
        grid.list_hits = (0..SortOrder::ALL.len()).map(|i| (row(i), i)).collect();
        grid.handle(Input::Pointer(Pointer::Click { x: 200.0, y: 600.0 }), &mut ctx);
        assert_eq!(grid.sort_list, None, "a click outside closes it");
        assert_eq!(grid.sort_order, SortOrder::RecentlyAdded, "unchanged");
        assert_eq!(grid.focus, 0, "and opens no Game");
    }

    #[test]
    fn page_keys_move_by_the_rows_that_show_and_home_and_end_to_the_ends() {
        let mut ctx = Ctx::empty();
        let mut grid = fifty_sorted(&mut ctx);
        assert_eq!(grid.page_rows(), 2, "two whole rows show at 8 per row");
        let press = |grid: &mut GameGrid, ctx: &mut Ctx, key| {
            grid.handle(Input::Action(key), ctx);
            grid.focus
        };
        assert_eq!(press(&mut grid, &mut ctx, Action::Right), 1);
        let downs: Vec<_> = (0..4)
            .map(|_| press(&mut grid, &mut ctx, Action::PageDown))
            .collect();
        assert_eq!(downs, [17, 33, 49, 49], "the shorter last row ends on its last");
        let ups: Vec<_> = (0..4)
            .map(|_| press(&mut grid, &mut ctx, Action::PageUp))
            .collect();
        assert_eq!(ups, [33, 17, 1, 1], "stops on the first row, same column");
        assert_eq!(press(&mut grid, &mut ctx, Action::Last), 49);
        assert!(grid.focus_in_view(), "scrolled with the focus");
        assert_eq!(press(&mut grid, &mut ctx, Action::First), 0);
        assert_eq!(grid.scroll, 0.0);
    }

    #[test]
    fn page_moves_stay_in_the_grid() {
        assert_eq!(page(3, 50, 8, 2, Action::PageDown), 19);
        assert_eq!(page(35, 50, 8, 2, Action::PageDown), 49);
        assert_eq!(
            page(40, 50, 8, 2, Action::PageDown),
            48,
            "same column on the last row"
        );
        assert_eq!(page(20, 50, 8, 2, Action::PageUp), 4);
        assert_eq!(page(5, 50, 8, 0, Action::PageDown), 13, "at least one row");
        assert_eq!(page(0, 0, 8, 2, Action::Last), 0);
    }

    #[test]
    fn reload_reveals_a_focused_game_that_moved_far_down_the_grid() {
        let mut ctx = Ctx::empty();
        let initial: Vec<_> = (0..50).map(found).collect();
        ctx.library.sync_system("snes", &initial, 0).unwrap();
        let mut grid = GameGrid::new("NES", console_source()).sortable(Colour::hex(0), "hint".into(), snes());
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
        let row_top = (grid.focus / grid.grid.cols) as f32 * grid.grid.pitch_y;
        assert!(
            row_top - grid.scroll >= 0.0 && row_top - grid.scroll <= bottom() - TOP,
            "row 10 should be scrolled into view, but scroll is {}",
            grid.scroll
        );
    }

    /// A Console Page of 50 Games (7 rows), loaded, with the last Game focused and in view.
    fn grid_scrolled_to_the_end(ctx: &mut Ctx) -> GameGrid {
        let games: Vec<_> = (0..50).map(found).collect();
        ctx.library.sync_system("snes", &games, 0).unwrap();
        let mut grid = GameGrid::new("NES", console_source()).sortable(Colour::hex(0), "hint".into(), snes());
        grid.handle(Input::Action(Action::Back), ctx); // initial load
        grid.focus = 49;
        grid.reveal_focus();
        grid
    }

    /// Where the last row's focus ring and caption end, in content coordinates, at 8 per row.
    fn last_row_end() -> f32 {
        let grid = Grid::default();
        6.0 * grid.pitch_y + grid.cover_h + RING_REACH + 40.0
    }

    #[test]
    fn a_taller_canvas_shows_more_rows_and_scrolls_less_to_the_last() {
        let mut ctx = Ctx::empty();
        let grid = grid_scrolled_to_the_end(&mut ctx);
        assert_eq!(grid.scroll, last_row_end() - (1010.0 - TOP));
        ui::with_h(1200.0, || {
            assert_eq!(bottom(), 1130.0);
            let grid = grid_scrolled_to_the_end(&mut ctx);
            assert_eq!(grid.scroll, last_row_end() - (1130.0 - TOP));
        });
    }

    #[test]
    fn a_resize_refits_the_grid_scroll() {
        let mut ctx = Ctx::empty();
        let mut grid = grid_scrolled_to_the_end(&mut ctx);
        ui::with_h(1200.0, || {
            grid.refit();
            assert_eq!(
                grid.scroll,
                grid.content_h() - (1130.0 - TOP),
                "no space left under the last row"
            );
        });
        grid.refit();
        assert_eq!(
            grid.scroll,
            last_row_end() - (1010.0 - TOP),
            "the focused Game shows in full"
        );
        // Wheeled away from the focused Game: an unchanged height leaves the scroll alone.
        grid.scroll = 0.0;
        grid.refit();
        assert_eq!(grid.scroll, 0.0);
    }

    #[test]
    fn with_6_games_per_row_down_moves_by_6() {
        let mut ctx = Ctx::empty();
        ctx.games_per_row = 6;
        let games: Vec<_> = (0..20).map(found).collect();
        ctx.library.sync_system("snes", &games, 0).unwrap();
        let mut grid =
            GameGrid::new("SNES", console_source()).sortable(Colour::hex(0), "hint".into(), snes());
        grid.handle(Input::Action(Action::Down), &mut ctx);
        assert_eq!(grid.focus, 6);
        grid.handle(Input::Action(Action::Up), &mut ctx);
        assert_eq!(grid.focus, 0);
    }

    /// The player changes the Games per row in Settings, opened over this grid: back on the grid,
    /// the same Game has focus, in view in the new layout.
    #[test]
    fn a_change_to_the_games_per_row_keeps_the_focused_game_and_scrolls_to_it() {
        let mut ctx = Ctx::empty();
        let mut grid = grid_scrolled_to_the_end(&mut ctx);
        let focused = grid.games[49].id;
        let end_at_8 = grid.scroll;

        ctx.set_games_per_row(6);
        grid.handle(Input::Pointer(Pointer::Move { x: 0.0, y: 0.0 }), &mut ctx);

        assert_eq!(grid.grid.cols, 6);
        assert_eq!(grid.games[grid.focus].id, focused, "the same Game has focus");
        let row_top = (grid.focus / 6) as f32 * grid.grid.pitch_y;
        assert!(
            row_top - grid.scroll >= 0.0 && row_top + grid.grid.cover_h - grid.scroll <= bottom() - TOP,
            "its row is in view, but scroll is {}",
            grid.scroll
        );
        assert!(grid.scroll > end_at_8, "9 taller rows reach further down than 7");
    }

    #[test]
    fn cycle_sort_updates_the_cached_sort_order() {
        let mut ctx = Ctx::empty();
        ctx.library.sync_system("snes", &[found(0)], 0).unwrap();
        let mut grid = GameGrid::new("NES", console_source()).sortable(Colour::hex(0), "hint".into(), snes());
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        assert_eq!(grid.sort_order, SortOrder::AZ);

        pick(&mut grid, &mut ctx, SortOrder::ZA);

        assert_eq!(grid.sort_order, SortOrder::ZA);
        assert_eq!(ctx.library.sort_order().unwrap(), SortOrder::ZA);
    }

    #[test]
    fn sort_is_ignored_on_an_empty_console_page() {
        let mut ctx = Ctx::empty();
        let mut grid = GameGrid::new("NES", console_source()).sortable(Colour::hex(0), "hint".into(), snes());
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load, no games
        assert!(grid.games.is_empty());

        grid.handle(Input::Action(Action::Sort), &mut ctx);
        assert_eq!(grid.sort_list, None, "no list opens");

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
        let mut grid = GameGrid::new("NES", console_source()).sortable(Colour::hex(0), "hint".into(), snes());
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

    #[test]
    fn confirm_or_a_click_on_a_grid_game_opens_its_details_page() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = dir.path().to_owned();
        // An installed Standalone Emulator's Game, which Confirm used to launch directly.
        ctx.library.sync_system("wiiu", &[found_wii_u(0)], 0).unwrap();
        let mut grid =
            GameGrid::new("Wii U", wii_u_source()).sortable(Colour::hex(0), "hint".into(), wii_u());
        grid.handle(Input::Action(Action::Down), &mut ctx); // initial load
        assert_eq!(grid.hints()[0], Hint::new(&[Action::Confirm], "Details"));

        grid.hits = vec![(Rect::new(0.0, 0.0, 50.0, 50.0), 0)];
        for input in [
            Input::Action(Action::Confirm),
            Input::Pointer(Pointer::Click { x: 10.0, y: 10.0 }),
        ] {
            let Transition::Push(page) = grid.handle(input, &mut ctx) else {
                panic!("{input:?} should open the Details Page");
            };
            assert_eq!(page.title(), "B000");
            assert!(page.hints().contains(&Hint::new(&[Action::Options], "Options")));
            assert!(
                ctx.play.is_none() && ctx.launch.is_none(),
                "{input:?} doesn't play"
            );
        }
    }

    #[test]
    fn options_y_or_a_right_click_on_a_grid_game_opens_its_options_menu() {
        let mut ctx = Ctx::empty();
        let games: Vec<_> = (0..3).map(found).collect();
        ctx.library.sync_system("snes", &games, 0).unwrap();
        let mut grid =
            GameGrid::new("SNES", console_source()).sortable(Colour::hex(0), "hint".into(), snes());
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        assert!(grid.hints().contains(&Hint::new(&[Action::Options], "Options")));

        let Transition::Push(menu) = grid.handle(Input::Action(Action::Options), &mut ctx) else {
            panic!("Y should open the Options Menu");
        };
        assert!(menu.is_modal());
        assert_eq!(menu.title(), "B000");

        grid.hits = vec![(Rect::new(0.0, 0.0, 50.0, 50.0), 2)];
        let Transition::Push(menu) =
            grid.handle(Input::Pointer(Pointer::RightClick { x: 10.0, y: 10.0 }), &mut ctx)
        else {
            panic!("a right-click should open the Options Menu");
        };
        assert_eq!(menu.title(), "B002", "for the Game right-clicked");
        assert_eq!(grid.focus, 2, "which takes focus");
        assert!(matches!(
            grid.handle(Input::Pointer(Pointer::RightClick { x: 99.0, y: 99.0 }), &mut ctx),
            Transition::None
        ));
    }

    #[test]
    fn hiding_a_game_from_its_options_menu_takes_it_off_the_grid() {
        let mut ctx = Ctx::empty();
        let games: Vec<_> = (0..3).map(found).collect();
        ctx.library.sync_system("snes", &games, 0).unwrap();
        let mut grid =
            GameGrid::new("SNES", console_source()).sortable(Colour::hex(0), "hint".into(), snes());
        grid.handle(Input::Action(Action::Right), &mut ctx); // initial load, focus on B001
        let Transition::Push(mut menu) = grid.handle(Input::Action(Action::Options), &mut ctx) else {
            panic!("Y should open the Options Menu");
        };
        assert_eq!(menu.title(), "B001");
        // Past Start from power-on, Choose artwork and Rename to Hide (no Save State to load, no
        // save data to delete).
        for _ in 0..4 {
            menu.handle(Input::Action(Action::Down), &mut ctx);
        }
        assert!(matches!(
            menu.handle(Input::Action(Action::Confirm), &mut ctx),
            Transition::Pop
        ));
        grid.handle(Input::Action(Action::Back), &mut ctx);
        let titles: Vec<_> = grid.games.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, ["B000", "B002"]);
    }

    #[test]
    fn options_on_an_empty_grid_opens_nothing() {
        let mut ctx = Ctx::empty();
        let mut grid =
            GameGrid::new("SNES", console_source()).sortable(Colour::hex(0), "hint".into(), snes());
        assert!(matches!(
            grid.handle(Input::Action(Action::Options), &mut ctx),
            Transition::None
        ));
        assert!(!grid.hints().contains(&Hint::new(&[Action::Options], "Options")));
    }

    #[test]
    fn step_focus_moves_between_the_notice_buttons_and_the_grid() {
        use NoticeFocus::{Check, Copy};
        assert_eq!(step_focus(Some(Copy), 0, Action::Right, 20, 8), (Some(Check), 0));
        assert_eq!(step_focus(Some(Check), 0, Action::Left, 20, 8), (Some(Copy), 0));
        assert_eq!(
            step_focus(Some(Check), 0, Action::Right, 20, 8),
            (Some(Check), 0),
            "Right at the last button stays"
        );
        assert_eq!(
            step_focus(Some(Copy), 0, Action::Down, 20, 8),
            (None, 0),
            "down to the grid"
        );
        assert_eq!(
            step_focus(Some(Copy), 0, Action::Down, 0, 8),
            (Some(Copy), 0),
            "no Games to go down to"
        );
        assert_eq!(
            step_focus(None, 3, Action::Up, 20, 8),
            (Some(Copy), 3),
            "up from the first row returns to the notice"
        );
        assert_eq!(
            step_focus(None, 11, Action::Up, 20, 8),
            (None, 3),
            "up from a later row moves within the grid"
        );
        assert_eq!(
            step_focus(None, 5, Action::Right, 20, 8),
            (None, 6),
            "the grid moves normally once it has focus"
        );
    }

    fn wii_u_source() -> Source {
        Box::new(|library, _| library.system_games("wiiu", SortOrder::AZ))
    }

    fn found_wii_u(n: usize) -> FoundGame {
        FoundGame {
            system: "wiiu",
            rel_path: format!("wiiu/b{n:03}.wua"),
            title: format!("B{n:03}"),
        }
    }

    /// A Console Page for an External System whose Standalone Emulator isn't installed.
    fn missing_cemu_page() -> (Ctx, GameGrid) {
        let mut ctx = Ctx::empty();
        ctx.emulators = Emulators::from_installed(Vec::<String>::new());
        let grid = GameGrid::new("Wii U", wii_u_source()).sortable(Colour::hex(0), "hint".into(), wii_u());
        (ctx, grid)
    }

    #[test]
    fn the_notice_shows_for_an_external_system_missing_its_emulator_and_starts_on_copy_command() {
        let (mut ctx, mut grid) = missing_cemu_page();
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        assert!(grid.notice_shown);
        assert_eq!(grid.notice_focus, Some(NoticeFocus::Copy));
        assert_eq!(
            grid.hints(),
            [
                Hint::new(&[Action::Confirm], "Copy command"),
                Hint::new(&[Action::Back], "Back")
            ]
        );
    }

    #[test]
    fn an_embedded_system_never_gets_a_notice() {
        let mut ctx = Ctx::empty();
        ctx.emulators = Emulators::from_installed(Vec::<String>::new());
        let mut grid =
            GameGrid::new("SNES", console_source()).sortable(Colour::hex(0), "hint".into(), snes());
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        assert!(!grid.notice_shown);
        assert_eq!(grid.notice_focus, None);
    }

    #[test]
    fn check_again_clears_the_notice_once_the_emulator_is_found() {
        let (mut ctx, mut grid) = missing_cemu_page();
        let found = [found_wii_u(0), found_wii_u(1)];
        ctx.library.sync_system("wiiu", &found, 0).unwrap();
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        assert!(grid.notice_shown);
        ctx.detect_emulators = Box::new(|| Emulators::from_installed(["info.cemu.Cemu"]));

        grid.handle(Input::Action(Action::Right), &mut ctx); // focus Check again
        grid.handle(Input::Action(Action::Confirm), &mut ctx);

        assert!(!grid.notice_shown, "Cemu is now installed");
        assert_eq!(grid.notice_focus, None, "focus moves to the grid");
        assert_eq!(grid.focus, 0);
    }

    #[test]
    fn check_again_keeps_the_notice_when_still_missing() {
        let (mut ctx, mut grid) = missing_cemu_page();
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        ctx.detect_emulators = Box::new(|| Emulators::from_installed(Vec::<String>::new()));

        grid.handle(Input::Action(Action::Right), &mut ctx); // focus Check again
        grid.handle(Input::Action(Action::Confirm), &mut ctx);

        assert!(grid.notice_shown, "Cemu is still missing");
        assert_eq!(
            grid.notice_focus,
            Some(NoticeFocus::Check),
            "focus stays on Check again"
        );
    }

    #[test]
    fn a_pointer_over_the_cleared_notices_buttons_focuses_nothing() {
        let (mut ctx, mut grid) = missing_cemu_page();
        ctx.library.sync_system("wiiu", &[found_wii_u(0)], 0).unwrap();
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        let check = Rect::new(100.0, 100.0, 50.0, 50.0);
        grid.notice_hits = Some((Rect::new(0.0, 100.0, 50.0, 50.0), check)); // as last drawn
        ctx.detect_emulators = Box::new(|| Emulators::from_installed(["info.cemu.Cemu"]));

        grid.handle(Input::Action(Action::Right), &mut ctx); // focus Check again
        grid.handle(Input::Action(Action::Confirm), &mut ctx); // clears the notice
        // Before the next draw, the mouse moves over where Check again was.
        grid.handle(Input::Pointer(Pointer::Move { x: 120.0, y: 120.0 }), &mut ctx);

        assert!(!grid.notice_shown);
        assert_eq!(grid.notice_focus, None, "the hidden button can't take focus");
        assert_eq!(grid.hints().last(), Some(&Hint::new(&[Action::Back], "Back")));
        assert!(
            !grid
                .hints()
                .contains(&Hint::new(&[Action::Confirm], "Check again"))
        );
    }

    #[test]
    fn check_again_in_screenshot_mode_keeps_the_simulated_state() {
        let (mut ctx, mut grid) = missing_cemu_page();
        // As `--missing-emulators wiiu` does.
        ctx.simulate_emulators(Emulators::from_installed(Vec::<String>::new()));
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load

        grid.handle(Input::Action(Action::Right), &mut ctx); // focus Check again
        grid.handle(Input::Action(Action::Confirm), &mut ctx);

        assert!(grid.notice_shown, "the simulated missing emulator stays missing");
        assert_eq!(ctx.emulators, Emulators::from_installed(Vec::<String>::new()));
    }

    #[test]
    fn copy_command_leaves_a_clipboard_request() {
        let (mut ctx, mut grid) = missing_cemu_page();
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load, focus on Copy

        grid.handle(Input::Action(Action::Confirm), &mut ctx);

        assert_eq!(
            ctx.clipboard_request.as_deref(),
            Some("flatpak install flathub info.cemu.Cemu")
        );
    }

    mod release_dates {
        use super::*;
        use crate::artwork::Artwork;
        use crate::artwork::tests::{Fake, settle, unpaced};
        use crate::config::ApiKey;
        use std::path::Path;

        /// A Ctx with the SNES Games "Alpha", "Beta" and "Gamma" and the NES Game "Kid", none
        /// dated yet, whose Artwork asks `fake` for Release Dates.
        fn world(dir: &Path, fake: &Fake) -> Ctx {
            let mut ctx = Ctx::empty();
            let game = |system: &'static str, title: &str| FoundGame {
                system,
                rel_path: format!("{system}/{title}.sfc"),
                title: title.to_owned(),
            };
            let snes: Vec<_> = ["Alpha", "Beta", "Gamma"]
                .iter()
                .map(|t| game("snes", t))
                .collect();
            ctx.library.sync_system("snes", &snes, 0).unwrap();
            ctx.library.sync_system("nes", &[game("nes", "Kid")], 0).unwrap();
            ctx.artwork = online(dir, fake);
            ctx
        }

        fn online(dir: &Path, fake: &Fake) -> Artwork {
            let key = ApiKey::new("0123456789abcdef0123456789abcdef");
            unpaced(Artwork::new(dir.join("artwork"), Some(key), fake.fetch()))
        }

        /// SNES's Console Page, sorted by the stored global order.
        fn console_page() -> GameGrid {
            let source: Source = Box::new(|library, order| library.system_games("snes", order));
            GameGrid::new("SNES", source).sortable(Colour::hex(0), "hint".into(), snes())
        }

        fn ids(games: &[GameRow]) -> Vec<i64> {
            games.iter().map(|g| g.id).collect()
        }

        fn titles(grid: &GameGrid) -> Vec<&str> {
            grid.games.iter().map(|g| g.title.as_str()).collect()
        }

        #[test]
        fn sort_landing_on_newest_release_asks_for_the_console_pages_games_only() {
            let dir = tempfile::tempdir().unwrap();
            let mut ctx = world(dir.path(), &Fake::new());
            ctx.library.set_sort_order(SortOrder::RecentlyAdded).unwrap();
            let mut grid = console_page();
            grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
            assert!(
                ctx.artwork.queued_dates().is_empty(),
                "Recently added asks for none"
            );

            pick(&mut grid, &mut ctx, SortOrder::NewestRelease);

            assert_eq!(grid.sort_order, SortOrder::NewestRelease);
            let snes = ctx.library.system_games("snes", SortOrder::AZ).unwrap();
            assert_eq!(ctx.artwork.queued_dates(), ids(&snes), "not NES's Kid");
        }

        #[test]
        fn sort_landing_on_a_to_z_asks_for_no_release_dates() {
            let dir = tempfile::tempdir().unwrap();
            let fake = Fake::new();
            let mut ctx = world(dir.path(), &fake);
            ctx.library.set_sort_order(SortOrder::OldestRelease).unwrap();
            // Opened before an API key was entered, so nothing was asked for then.
            ctx.artwork = Artwork::off();
            let mut grid = console_page();
            grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
            ctx.artwork = online(dir.path(), &fake);

            pick(&mut grid, &mut ctx, SortOrder::AZ);

            assert_eq!(grid.sort_order, SortOrder::AZ);
            assert!(ctx.artwork.queued_dates().is_empty());
        }

        #[test]
        fn a_console_page_opened_in_oldest_release_asks_for_its_games_dates() {
            let dir = tempfile::tempdir().unwrap();
            let mut ctx = world(dir.path(), &Fake::new());
            ctx.library.set_sort_order(SortOrder::OldestRelease).unwrap();
            let mut grid = console_page();
            grid.handle(Input::Action(Action::Back), &mut ctx); // initial load

            assert_eq!(ctx.artwork.queued_dates(), ids(&grid.games));
            assert_eq!(titles(&grid), ["Alpha", "Beta", "Gamma"]);
        }

        #[test]
        fn all_games_in_a_release_order_asks_for_its_games_dates() {
            let dir = tempfile::tempdir().unwrap();
            let mut ctx = world(dir.path(), &Fake::new());
            let source: Source = Box::new(|library, order| library.all_games(order));
            let mut grid = GameGrid::new("All games", source).own_sort(SortOrder::NewestRelease);
            grid.handle(Input::Action(Action::Back), &mut ctx); // initial load

            assert_eq!(titles(&grid), ["Alpha", "Beta", "Gamma", "Kid"]);
            assert_eq!(ctx.artwork.queued_dates(), ids(&grid.games));
        }

        /// `Artwork::dates_stored`, a grid in a release order having been drawn now.
        fn shown_dates_stored(ctx: &Ctx) -> u64 {
            ctx.artwork.release_order_shown(Instant::now());
            ctx.artwork.dates_stored()
        }

        #[test]
        fn arriving_release_dates_re_sort_the_grid_at_most_once_a_second_keeping_the_focus() {
            let dir = tempfile::tempdir().unwrap();
            // Asked in A–Z order: Alpha has no Release Date, Beta's comes first, then Gamma's.
            let fake = Fake::new();
            fake.api("/search/autocomplete/Alpha", "[{\"id\":1}]")
                .api("/search/autocomplete/Beta", "[{\"id\":2,\"release_date\":100}]")
                .api("/search/autocomplete/Gamma", "[{\"id\":3,\"release_date\":200}]");
            let mut ctx = world(dir.path(), &fake);
            ctx.library.set_sort_order(SortOrder::NewestRelease).unwrap();
            let mut grid = console_page();
            grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
            assert_eq!(titles(&grid), ["Alpha", "Beta", "Gamma"]);
            assert_eq!(grid.focus, 0, "Alpha");

            // Only Beta's date is in: a result is collected only by a poll, one per poll.
            settle(
                &ctx.artwork,
                &ctx.library,
                || shown_dates_stored(&ctx),
                |n| *n < 1,
            );
            let before = Instant::now() - RESORT_GAP;
            grid.resort_if_dated(&ctx, before);
            assert_eq!(titles(&grid), ["Beta", "Alpha", "Gamma"]);
            assert_eq!(grid.focus, 1, "Alpha keeps the focus");

            settle(
                &ctx.artwork,
                &ctx.library,
                || shown_dates_stored(&ctx),
                |n| *n < 2,
            );
            grid.resort_if_dated(&ctx, before + RESORT_GAP - Duration::from_millis(1));
            assert_eq!(titles(&grid), ["Beta", "Alpha", "Gamma"], "not within a second");

            // The next frame, a second after the last re-sort.
            grid.handle(Input::Action(Action::Back), &mut ctx);
            assert_eq!(titles(&grid), ["Gamma", "Beta", "Alpha"]);
            assert_eq!(grid.focus, 2, "Alpha keeps the focus");
        }

        /// Polls at `now` until no Release Date is queued or on its way.
        fn settle_dates_at(ctx: &Ctx, now: Instant) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while ctx.artwork.dating() {
                ctx.artwork.poll_at(&ctx.library, now);
                assert!(Instant::now() < deadline, "never settled");
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        #[test]
        fn stepping_through_the_release_orders_asks_for_no_more_dates_once_they_are_left() {
            let dir = tempfile::tempdir().unwrap();
            let fake = Fake::new();
            let mut ctx = world(dir.path(), &fake);
            ctx.library.set_sort_order(SortOrder::RecentlyAdded).unwrap();
            let mut grid = console_page();
            grid.handle(Input::Action(Action::Back), &mut ctx); // initial load

            pick(&mut grid, &mut ctx, SortOrder::NewestRelease);
            assert_eq!(grid.sort_order, SortOrder::NewestRelease);
            // A frame: the first Release Date's request goes.
            let now = Instant::now();
            grid.keep_dating(&ctx, now);
            ctx.artwork.poll_at(&ctx.library, now);
            pick(&mut grid, &mut ctx, SortOrder::OldestRelease);
            pick(&mut grid, &mut ctx, SortOrder::AZ);
            assert_eq!(grid.sort_order, SortOrder::AZ);

            // Frames go on in A–Z for more than a second.
            settle_dates_at(&ctx, now + Duration::from_secs(2));
            assert_eq!(fake.calls(), 1, "only the request already on its way");
            assert!(ctx.artwork.queued_dates().is_empty());
        }

        #[test]
        fn a_grid_that_stays_in_newest_release_keeps_getting_its_dates() {
            let dir = tempfile::tempdir().unwrap();
            let fake = Fake::new();
            fake.api("/search/autocomplete/Alpha", "[{\"id\":1}]")
                .api("/search/autocomplete/Beta", "[{\"id\":2,\"release_date\":100}]")
                .api("/search/autocomplete/Gamma", "[{\"id\":3,\"release_date\":200}]");
            let mut ctx = world(dir.path(), &fake);
            ctx.library.set_sort_order(SortOrder::NewestRelease).unwrap();
            let mut grid = console_page();
            grid.handle(Input::Action(Action::Back), &mut ctx); // initial load

            // Frames half a second apart, each drawing the grid.
            let mut now = Instant::now();
            let deadline = now + Duration::from_secs(10);
            while ctx.artwork.dating() {
                now += Duration::from_millis(500);
                grid.keep_dating(&ctx, now);
                ctx.artwork.poll_at(&ctx.library, now);
                assert!(Instant::now() < deadline, "never settled");
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(fake.calls(), 3);
            assert_eq!(ctx.artwork.dates_stored(), 2);
        }

        #[test]
        fn closing_a_grid_in_newest_release_stops_its_date_requests() {
            let dir = tempfile::tempdir().unwrap();
            let fake = Fake::new();
            let mut ctx = world(dir.path(), &fake);
            ctx.library.set_sort_order(SortOrder::NewestRelease).unwrap();
            let mut grid = console_page();
            grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
            let now = Instant::now();
            grid.keep_dating(&ctx, now);
            ctx.artwork.poll_at(&ctx.library, now);
            drop(grid);

            settle_dates_at(&ctx, now + Duration::from_secs(2));
            assert_eq!(fake.calls(), 1, "only the request already on its way");
            assert!(ctx.artwork.queued_dates().is_empty());
        }

        #[test]
        fn a_grid_shown_again_asks_again_for_the_dates_dropped_meanwhile() {
            let dir = tempfile::tempdir().unwrap();
            let mut ctx = world(dir.path(), &Fake::new());
            ctx.library.set_sort_order(SortOrder::NewestRelease).unwrap();
            let mut grid = console_page();
            grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
            assert_eq!(ctx.artwork.queued_dates(), ids(&grid.games));

            // Covered by another screen for two seconds: dropped unasked.
            let later = Instant::now() + Duration::from_secs(2);
            ctx.artwork.poll_at(&ctx.library, later);
            assert!(ctx.artwork.queued_dates().is_empty());

            grid.keep_dating(&ctx, later);
            assert_eq!(ctx.artwork.queued_dates(), ids(&grid.games));
        }

        #[test]
        fn a_release_order_grid_asks_once_an_api_key_is_entered() {
            let dir = tempfile::tempdir().unwrap();
            let fake = Fake::new();
            let mut ctx = world(dir.path(), &fake);
            ctx.library.set_sort_order(SortOrder::OldestRelease).unwrap();
            ctx.artwork = Artwork::off();
            let mut grid = console_page();
            grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
            assert!(ctx.artwork.queued_dates().is_empty());

            // Settings replaces Artwork with one that has the key; the grid is drawn again.
            ctx.artwork = online(dir.path(), &fake);
            grid.keep_dating(&ctx, Instant::now());
            assert_eq!(ctx.artwork.queued_dates(), ids(&grid.games));
        }

        /// SNES's Console Page of 50 Games in Newest release, none with a Release Date, the first
        /// focused.
        fn fifty_in_newest_release(ctx: &mut Ctx) -> GameGrid {
            let games: Vec<_> = (0..50).map(found).collect();
            ctx.library.sync_system("snes", &games, 0).unwrap();
            ctx.library.set_sort_order(SortOrder::NewestRelease).unwrap();
            let mut grid = console_page();
            grid.handle(Input::Action(Action::Back), ctx); // initial load
            assert_eq!(grid.focus, 0);
            grid
        }

        #[test]
        fn a_re_sort_leaves_the_scroll_alone_when_the_focused_game_was_scrolled_away_from() {
            let mut ctx = Ctx::empty();
            let mut grid = fifty_in_newest_release(&mut ctx);
            grid.handle(Input::Pointer(Pointer::Wheel { dy: -100.0 }), &mut ctx);
            let scrolled = grid.scroll;
            assert!(scrolled > 0.0 && !grid.focus_in_view());

            grid.dates_seen += 1; // as if a Release Date arrived
            grid.resort_if_dated(&ctx, Instant::now());
            assert!(grid.resorted.is_some(), "re-sorted");
            assert_eq!(grid.scroll, scrolled);
        }

        #[test]
        fn a_re_sort_keeps_a_focused_game_in_view_that_was() {
            let mut ctx = Ctx::empty();
            let mut grid = fifty_in_newest_release(&mut ctx);
            assert!(grid.focus_in_view());
            // Every other Game gets a Release Date: the focused one moves to the end.
            for (i, game) in grid.games.iter().enumerate().skip(1) {
                ctx.library.set_release_date(game.id, Some(i as i64)).unwrap();
            }

            grid.dates_seen += 1; // as if a Release Date arrived
            grid.resort_if_dated(&ctx, Instant::now());
            assert_eq!(grid.focus, 49);
            assert!(grid.focus_in_view());
        }

        #[test]
        fn without_an_api_key_a_release_order_says_one_is_needed() {
            let mut ctx = Ctx::empty();
            let mut grid = fifty_in_newest_release(&mut ctx);
            assert!(!ctx.artwork.online());
            assert_eq!(grid.release_note(&ctx).as_deref(), Some(NO_KEY_NOTE));
            assert_eq!(
                NO_KEY_NOTE,
                "Release dates need a SteamGridDB API key (Settings → Artwork)"
            );
            pick(&mut grid, &mut ctx, SortOrder::MostPlayed);
            assert_eq!(grid.release_note(&ctx), None, "nothing for other orders");
        }

        /// With a key, but SteamGridDB refused the requests this run, the note says the dates
        /// can't be fetched now, not that a key is needed.
        #[test]
        fn once_steamgriddb_stopped_the_requests_the_note_says_dates_cant_be_fetched() {
            let dir = tempfile::tempdir().unwrap();
            let fake = Fake::new();
            fake.route(
                "https://www.steamgriddb.com/api/v2",
                Err(crate::artwork::FetchError::Status(401)),
            );
            let mut ctx = world(dir.path(), &fake);
            ctx.library.set_sort_order(SortOrder::NewestRelease).unwrap();
            let mut grid = console_page();
            grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
            let deadline = Instant::now() + Duration::from_secs(10);
            while ctx.artwork.online() {
                grid.keep_dating(&ctx, Instant::now());
                ctx.artwork.poll(&ctx.library);
                assert!(Instant::now() < deadline, "never refused");
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(ctx.artwork.key().is_some());
            assert_eq!(grid.release_note(&ctx).as_deref(), Some(STOPPED_NOTE));
            assert_eq!(
                STOPPED_NOTE,
                "Release dates can't be fetched right now (SteamGridDB refused the requests)"
            );
        }

        #[test]
        fn while_dates_are_asked_for_the_note_counts_this_grids_games_done() {
            let dir = tempfile::tempdir().unwrap();
            let fake = Fake::new();
            fake.api("/search/autocomplete/Alpha", "[{\"id\":1}]")
                .api("/search/autocomplete/Beta", "[{\"id\":2,\"release_date\":100}]")
                .api("/search/autocomplete/Gamma", "[{\"id\":3,\"release_date\":200}]");
            let mut ctx = world(dir.path(), &fake);
            ctx.library.set_sort_order(SortOrder::NewestRelease).unwrap();
            let mut grid = console_page();
            grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
            let note = |grid: &GameGrid| grid.release_note(&ctx);
            assert_eq!(note(&grid).as_deref(), Some("Getting release dates… 0 of 3"));

            // Frames until the first answer is in and the second request is on its way.
            let deadline = Instant::now() + Duration::from_secs(10);
            while ctx.artwork.queued_dates().len() > 1 {
                grid.keep_dating(&ctx, Instant::now());
                ctx.artwork.poll(&ctx.library);
                assert!(Instant::now() < deadline, "never asked");
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(note(&grid).as_deref(), Some("Getting release dates… 1 of 3"));

            settle_dates_at(&ctx, Instant::now());
            assert_eq!(note(&grid), None, "all in: nothing");
        }
    }
}
