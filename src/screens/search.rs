//! The Search screen (US-21, FR-SRCH-1–3): the top bar's pill grown into a field, and the visible
//! Games whose Display Title matches it as a grid of covers with System badges, after
//! `search_topbar`, `search_results` and `screen_search*` in `design/generate_mockups.py`.

use super::details::DetailsPage;
use super::game_grid::{refocus, step};
use super::options::OptionsMenu;
use crate::ctx::Ctx;
use crate::library::db::GameRow;
use crate::systems;
use crate::ui::cover::{self, COVER_GAP, COVER_H, COVER_W, RING_REACH};
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, InputKind, Pointer};
use crate::ui::nav::{Entry, Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::text_entry::{KEYBOARD_H, KEYBOARD_W, Outcome, TextEntry};
use crate::ui::theme::{ACCENT, BG, LINE, MUTED, SURFACE, TEXT};
use crate::ui::{M, Rect, W, hit, scroll};

/// The field, where the top bar's pill is on other screens.
const FIELD: Rect = Rect::new(M + 300.0, 40.0, W - M - 174.0 - (M + 300.0), 56.0);
/// Where the query starts, right of the magnifier.
const QUERY_X: f32 = FIELD.x + 62.0;
/// The most characters a query can have.
const MAX_QUERY_CHARS: usize = 100;

/// The count line and the grid below it, sized like a Console Page's.
const COUNT_Y: f32 = 160.0;
const COLS: usize = 8;
const TOP: f32 = 196.0;
const BOTTOM: f32 = 1010.0;
const PITCH_X: f32 = COVER_W + COVER_GAP;
const PITCH_Y: f32 = COVER_H + 72.0;
/// Pixels scrolled per wheel notch.
const WHEEL_STEP: f32 = PITCH_Y / 2.0;
/// How far below a cover the focused Game's Display Title and System name reach.
const CAPTION_H: f32 = 76.0;
/// Where the empty-query hint and the no-match message go.
const MESSAGE_Y: f32 = 520.0;
/// The tray the on-screen keyboard is docked in (FR-SRCH-2), just above the hint bar, with the
/// keyboard centred in it.
const TRAY_PAD: f32 = 24.0;
const TRAY_Y: f32 = BOTTOM - TRAY_PAD - KEYBOARD_H - TRAY_PAD;

/// What has focus: the field, being typed into, or the results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Field,
    Results,
}

pub struct Search {
    entry: TextEntry,
    focus: Focus,
    games: Vec<GameRow>,
    /// How many Games are visible, for the empty query's hint; read with the results so `draw()`
    /// never queries the database (NFR-3).
    total: usize,
    /// The `Ctx::revision` the results were loaded at.
    revision: Option<u64>,
    /// The focused result, while the results have focus.
    grid_focus: usize,
    scroll: f32,
    hits: Vec<(Rect, usize)>,
    /// What the player last used, as of the last input or frame: on a controller the field has
    /// the on-screen keyboard.
    kind: InputKind,
}

impl Search {
    /// An empty query with the field focused.
    pub fn new(ctx: &Ctx) -> Search {
        let mut search = Search {
            entry: TextEntry::new("", MAX_QUERY_CHARS),
            focus: Focus::Field,
            games: Vec::new(),
            total: 0,
            revision: None,
            grid_focus: 0,
            scroll: 0.0,
            hits: Vec::new(),
            kind: ctx.input_kind,
        };
        search.reload(ctx);
        search
    }

    fn query(&self) -> &str {
        self.entry.text()
    }

    /// Runs the query again, keeping focus on the same Game if it still matches.
    fn reload(&mut self, ctx: &Ctx) {
        let games = ctx.library.search(self.query()).unwrap_or_else(|e| {
            eprintln!("game37: {e}");
            Vec::new()
        });
        self.grid_focus = refocus(&self.games, self.grid_focus, &games);
        self.games = games;
        self.total = ctx.library.visible_count().unwrap_or_else(|e| {
            eprintln!("game37: {e}");
            0
        });
        self.revision = Some(ctx.revision);
        // The previous frame's hit rects may point at Games that moved or went: drop them until the
        // next draw.
        self.hits.clear();
        if self.games.is_empty() {
            self.focus = Focus::Field;
        }
        self.scroll = scroll::clamp(self.scroll, self.content_h(), self.bottom() - TOP);
        self.reveal_focus();
    }

    fn reload_if_changed(&mut self, ctx: &Ctx) {
        if self.revision != Some(ctx.revision) {
            self.reload(ctx);
        }
    }

    /// The query changed: match it afresh, from the first result.
    fn requery(&mut self, ctx: &Ctx) {
        self.grid_focus = 0;
        self.scroll = 0.0;
        self.games.clear();
        self.reload(ctx);
    }

    /// Moves focus into the results, if there are any.
    fn enter_results(&mut self) {
        if !self.games.is_empty() {
            self.focus = Focus::Results;
            self.reveal_focus();
        }
    }

    /// Whether the on-screen keyboard is docked at the bottom: while the field has focus on a
    /// controller.
    fn keyboard_shown(&self) -> bool {
        self.focus == Focus::Field && self.kind == InputKind::Controller
    }

    /// Where the grid ends: above the on-screen keyboard while it shows.
    fn bottom(&self) -> f32 {
        if self.keyboard_shown() { TRAY_Y } else { BOTTOM }
    }

    fn content_h(&self) -> f32 {
        self.games.len().div_ceil(COLS) as f32 * PITCH_Y
    }

    fn reveal_focus(&mut self) {
        let row = (self.grid_focus / COLS) as f32;
        let view = self.bottom() - TOP;
        let s = scroll::keep_visible(
            self.scroll,
            row * PITCH_Y - RING_REACH,
            RING_REACH + COVER_H + CAPTION_H,
            view,
        );
        self.scroll = scroll::clamp(s, self.content_h(), view);
    }

    /// Confirm or a click on a result opens its Details Page (FR-DET-1).
    fn confirm(&self, ctx: &mut Ctx) -> Transition {
        match self.games.get(self.grid_focus) {
            Some(game) => Transition::Push(Box::new(DetailsPage::new(game.clone(), ctx))),
            None => Transition::None,
        }
    }

    /// Options or a right-click on a result opens its Options Menu (FR-DET-3).
    fn options(&self, ctx: &Ctx) -> Transition {
        match self.games.get(self.grid_focus) {
            Some(game) => Transition::Push(Box::new(OptionsMenu::new(game.clone(), &ctx.games_root))),
            None => Transition::None,
        }
    }

    /// The result under the pointer, focused.
    fn focus_at(&mut self, x: f32, y: f32) -> bool {
        match hit(&self.hits, x, y) {
            Some(i) if i < self.games.len() => {
                self.grid_focus = i;
                true
            }
            _ => false,
        }
    }

    /// Back from the field clears a non-empty query first, and closes Search once it is empty.
    fn back_from_field(&mut self, ctx: &Ctx) -> Transition {
        if self.query().is_empty() {
            return Transition::Pop;
        }
        self.entry = TextEntry::new("", MAX_QUERY_CHARS);
        self.requery(ctx);
        Transition::None
    }

    fn handle_field(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        match input {
            Input::Action(Action::Back) => return self.back_from_field(ctx),
            Input::Action(Action::Search) => {}
            Input::Pointer(Pointer::Click { x, y } | Pointer::RightClick { x, y })
                if self.focus_at(x, y) || FIELD.contains(x, y) =>
            {
                return self.handle_pointer(input, ctx);
            }
            Input::Pointer(Pointer::Wheel { .. }) => return self.handle_pointer(input, ctx),
            // The on-screen keyboard takes the rest: its keys, Y, X, and Start or Done to finish.
            _ if self.keyboard_shown() => match self.entry.handle(input) {
                Outcome::Changed => self.requery(ctx),
                Outcome::Submit => self.enter_results(),
                Outcome::None | Outcome::Cancel => {}
            },
            Input::Action(Action::Down | Action::Start) => self.enter_results(),
            Input::Text(_) => {
                if self.entry.handle(input) == Outcome::Changed {
                    self.requery(ctx);
                }
            }
            _ => return self.handle_pointer(input, ctx),
        }
        Transition::None
    }

    fn handle_results(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        match input {
            Input::Action(Action::Back) => return Transition::Pop,
            Input::Action(Action::Search) => self.focus = Focus::Field,
            Input::Action(Action::Up) if self.grid_focus < COLS => self.focus = Focus::Field,
            Input::Action(dir) if dir.is_direction() => {
                self.grid_focus = step(self.grid_focus, self.games.len(), COLS, dir);
                self.reveal_focus();
            }
            Input::Action(Action::Confirm) => return self.confirm(ctx),
            Input::Action(Action::Options) => return self.options(ctx),
            Input::Pointer(Pointer::Move { x, y }) => {
                self.focus_at(x, y);
            }
            _ => return self.handle_pointer(input, ctx),
        }
        Transition::None
    }

    /// The mouse, wherever focus is: a click on a result opens it and a right-click its Options
    /// Menu, a click on the field focuses it, and the wheel scrolls.
    fn handle_pointer(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        match input {
            Input::Pointer(Pointer::Click { x, y }) if self.focus_at(x, y) => {
                self.focus = Focus::Results;
                return self.confirm(ctx);
            }
            Input::Pointer(Pointer::RightClick { x, y }) if self.focus_at(x, y) => {
                self.focus = Focus::Results;
                return self.options(ctx);
            }
            Input::Pointer(Pointer::Click { x, y }) if FIELD.contains(x, y) => self.focus = Focus::Field,
            Input::Pointer(Pointer::Wheel { dy }) => {
                let view = self.bottom() - TOP;
                self.scroll = scroll::clamp(self.scroll - dy * WHEEL_STEP, self.content_h(), view);
            }
            _ => {}
        }
        Transition::None
    }

    /// The field: a magnifier, the query (its end, when too long) and a caret while focused, or a
    /// placeholder while empty.
    fn draw_field(&self, gfx: &mut Gfx<'_>) {
        let focused = self.focus == Focus::Field;
        let radius = FIELD.h / 2.0;
        gfx.fill_round_rect(FIELD, radius, SURFACE);
        if focused {
            gfx.stroke_round_rect(FIELD, radius, 2.0, ACCENT);
        } else {
            gfx.stroke_round_rect(FIELD, radius, 1.0, LINE);
        }
        let (mx, my) = (FIELD.x + 32.0, 66.0);
        gfx.stroke_circle(mx, my, 9.0, 2.5, MUTED);
        gfx.line(mx + 7.0, my + 7.0, mx + 13.0, my + 13.0, 2.5, MUTED);
        // Round caps, as in the mockup.
        gfx.fill_circle(mx + 7.0, my + 7.0, 1.25, MUTED);
        gfx.fill_circle(mx + 13.0, my + 13.0, 1.25, MUTED);

        let base = 76.0;
        let style = TextStyle::new(22, Weight::Medium, TEXT);
        let room = FIELD.x + FIELD.w - radius - QUERY_X;
        let mut shown = self.query();
        let mut prefix = "";
        while gfx.measure(shown, style) > room {
            let mut chars = shown.chars();
            chars.next();
            shown = chars.as_str();
            prefix = "…";
        }
        let w = gfx.text(QUERY_X, base, &format!("{prefix}{shown}"), style, Align::Left);
        if focused {
            gfx.fill_rect(
                Rect::new(QUERY_X + w + 5.0, FIELD.y + 15.0, 3.0, FIELD.h - 30.0),
                ACCENT,
            );
        }
        if self.query().is_empty() {
            let placeholder = TextStyle::new(22, Weight::Regular, MUTED);
            gfx.text(QUERY_X + 14.0, base, "Search games", placeholder, Align::Left);
        }
    }

    /// A muted line centred in the space for the results.
    fn draw_message(&self, gfx: &mut Gfx<'_>, message: &str) {
        let style = TextStyle::new(26, Weight::Medium, MUTED);
        // Above the on-screen keyboard, centred between it and the field.
        let y = if self.keyboard_shown() {
            (FIELD.y + FIELD.h + TRAY_Y) / 2.0 + 9.0
        } else {
            MESSAGE_Y
        };
        gfx.text(W / 2.0, y, message, style, Align::Centre);
    }

    /// The count line and the covers, the focused one with its Display Title and System name under
    /// it.
    fn draw_results(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        gfx.text(
            M,
            COUNT_Y,
            &count(self.games.len()),
            TextStyle::new(20, Weight::Medium, MUTED),
            Align::Left,
        );
        let bottom = self.bottom();
        let clip_top = TOP - RING_REACH - 4.0;
        gfx.clip(Some(Rect::new(0.0, clip_top, W, bottom - clip_top)));
        let first_row = ((self.scroll - RING_REACH - CAPTION_H) / PITCH_Y)
            .floor()
            .max(0.0) as usize;
        let last_row = ((self.scroll + bottom - TOP) / PITCH_Y).ceil() as usize;
        let focused = (self.focus == Focus::Results).then_some(self.grid_focus);
        for i in first_row * COLS..((last_row + 1) * COLS).min(self.games.len()) {
            let (row, col) = (i / COLS, i % COLS);
            let r = Rect::new(
                M + col as f32 * PITCH_X,
                TOP + row as f32 * PITCH_Y - self.scroll,
                COVER_W,
                COVER_H,
            );
            let game = &self.games[i];
            let art = ctx.artwork.cover(&ctx.library, game);
            cover::draw(gfx, r, game, &art, true, focused == Some(i));
            if focused == Some(i) {
                caption(gfx, r, game);
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
        if self.content_h() - self.scroll > bottom - TOP {
            let fade = Rect::new(0.0, bottom - 110.0, W, 110.0);
            gfx.fill_round_rect_vgradient(fade, 0.0, BG.alpha(0.0), BG);
        }
        gfx.clip(None);
    }
}

/// The count line over the results: "7 Games", singular when one.
fn count(n: usize) -> String {
    match n {
        1 => "1 Game".to_owned(),
        n => format!("{n} Games"),
    }
}

/// The message shown instead of the results: a hint while the query is empty, else that nothing
/// matched.
fn message(query: &str, total: usize) -> String {
    if query.trim().is_empty() {
        format!("Type to search {total} Games")
    } else {
        format!("No Games match “{query}”")
    }
}

/// The focused Game's Display Title and System name under its cover `r`.
fn caption(gfx: &mut Gfx<'_>, r: Rect, game: &GameRow) {
    let room = W - M - r.x;
    let title = TextStyle::new(22, Weight::Bold, TEXT);
    let shown = cover::ellipsize(&game.title, room, |s| gfx.measure(s, title));
    gfx.text(r.x, r.y + COVER_H + 40.0, &shown, title, Align::Left);
    let system = systems::by_id(&game.system).map_or("?", |s| s.name);
    let name = TextStyle::new(17, Weight::Regular, MUTED);
    gfx.text(r.x, r.y + COVER_H + 68.0, system, name, Align::Left);
}

impl Screen for Search {
    fn title(&self) -> &str {
        "Search"
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        self.kind = ctx.input_kind;
        self.reload_if_changed(ctx);
        match self.focus {
            Focus::Field => self.handle_field(input, ctx),
            Focus::Results => self.handle_results(input, ctx),
        }
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.kind = ctx.input_kind;
        self.reload_if_changed(ctx);
        self.hits.clear();
        self.draw_field(gfx);
        if self.games.is_empty() {
            self.draw_message(gfx, &message(self.query(), self.total));
        } else {
            self.draw_results(gfx, ctx);
        }
        if self.keyboard_shown() {
            gfx.fill_rect(Rect::new(0.0, TRAY_Y, W, BOTTOM - TRAY_Y), SURFACE);
            gfx.fill_rect(Rect::new(0.0, TRAY_Y, W, 1.0), LINE);
            self.entry
                .draw_keyboard(gfx, (W - KEYBOARD_W) / 2.0, TRAY_Y + TRAY_PAD);
        }
    }

    fn hints(&self) -> Vec<Hint> {
        let mut hints = Vec::new();
        match self.focus {
            // The on-screen keyboard's own hints, but Start goes to the results, and only when
            // there are any, and Back closes as everywhere else on Search.
            Focus::Field if self.keyboard_shown() => {
                let keys = TextEntry::hints(InputKind::Controller, "Results");
                hints.extend(keys.into_iter().filter(|h| match h.actions {
                    [Action::Back] => false,
                    [Action::Start] => !self.games.is_empty(),
                    _ => true,
                }));
            }
            Focus::Field => {
                if !self.games.is_empty() {
                    hints.push(Hint::new(&[Action::Down], "Results"));
                }
            }
            Focus::Results => {
                hints.push(Hint::new(&[Action::Confirm], "Details"));
                hints.push(Hint::new(&[Action::Options], "Options"));
            }
        }
        hints.push(Hint::new(&[Action::Back], "Close"));
        hints
    }

    fn text_input(&self) -> bool {
        self.focus == Focus::Field
    }

    fn entry(&self) -> Option<Entry> {
        Some(Entry::Search)
    }

    fn search_pill(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::FoundGame;
    use crate::ui::input::TextInput;
    use crate::ui::text_entry::Key;

    /// A library with these SNES Games, in a temp Games Root, the player on the keyboard.
    fn ctx_with(titles: &[&str]) -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = dir.path().to_owned();
        ctx.input_kind = InputKind::Keyboard;
        let found: Vec<_> = titles
            .iter()
            .map(|t| FoundGame {
                system: "snes",
                rel_path: format!("snes/{t}.sfc"),
                title: (*t).to_owned(),
            })
            .collect();
        ctx.library.sync_system("snes", &found, 0).unwrap();
        (dir, ctx)
    }

    fn type_text(search: &mut Search, text: &str, ctx: &mut Ctx) {
        for ch in text.chars() {
            search.handle(Input::Text(TextInput::Char(ch)), ctx);
        }
    }

    fn act(search: &mut Search, action: Action, ctx: &mut Ctx) -> Transition {
        search.handle(Input::Action(action), ctx)
    }

    fn titles(search: &Search) -> Vec<&str> {
        search.games.iter().map(|g| g.title.as_str()).collect()
    }

    #[test]
    fn typing_filters_the_results_live() {
        let (_dir, mut ctx) = ctx_with(&["Super Mario World", "Paper Mario", "Zelda"]);
        let mut search = Search::new(&ctx);
        assert!(search.text_input(), "the field starts focused");
        type_text(&mut search, "m", &mut ctx);
        assert_eq!(titles(&search), ["Paper Mario", "Super Mario World"]);
        type_text(&mut search, "ario w", &mut ctx);
        assert_eq!(titles(&search), ["Super Mario World"]);
        search.handle(Input::Text(TextInput::Backspace), &mut ctx);
        search.handle(Input::Text(TextInput::Backspace), &mut ctx);
        assert_eq!(titles(&search), ["Paper Mario", "Super Mario World"]);
    }

    #[test]
    fn down_or_enter_moves_into_the_results_and_up_from_the_top_row_back() {
        let titles: Vec<String> = (0..10).map(|i| format!("Game {i:02}")).collect();
        let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
        let (_dir, mut ctx) = ctx_with(&titles);
        let mut search = Search::new(&ctx);
        type_text(&mut search, "game", &mut ctx);
        for into in [Action::Down, Action::Start] {
            act(&mut search, into, &mut ctx);
            assert!(!search.text_input(), "{into:?} leaves the field");
            assert_eq!(search.hints()[0], Hint::new(&[Action::Confirm], "Details"));
            act(&mut search, Action::Down, &mut ctx);
            assert_eq!(search.grid_focus, 8, "Down moves a row in the results");
            act(&mut search, Action::Up, &mut ctx);
            assert!(
                !search.text_input(),
                "Up from the second row stays in the results"
            );
            act(&mut search, Action::Up, &mut ctx);
            assert!(search.text_input(), "Up from the top row goes back to the field");
            assert_eq!(search.query(), "game", "and keeps the query");
        }
    }

    #[test]
    fn confirm_on_a_result_opens_its_details_page_and_options_its_options_menu() {
        let (_dir, mut ctx) = ctx_with(&["Super Mario World", "Paper Mario"]);
        let mut search = Search::new(&ctx);
        type_text(&mut search, "mario", &mut ctx);
        act(&mut search, Action::Down, &mut ctx);
        act(&mut search, Action::Right, &mut ctx);
        let Transition::Push(page) = act(&mut search, Action::Confirm, &mut ctx) else {
            panic!("Confirm should open the Details Page");
        };
        assert_eq!(page.title(), "Super Mario World");
        assert!(ctx.play.is_none() && ctx.launch.is_none(), "Confirm doesn't play");
        let Transition::Push(menu) = act(&mut search, Action::Options, &mut ctx) else {
            panic!("Options should open the Options Menu");
        };
        assert_eq!(menu.title(), "Super Mario World");
        assert_eq!(search.query(), "mario", "the query survives the visit");
        assert!(!search.text_input(), "and so does the focus");
    }

    #[test]
    fn back_from_the_field_clears_the_query_then_closes() {
        let (_dir, mut ctx) = ctx_with(&["Zelda"]);
        let mut search = Search::new(&ctx);
        type_text(&mut search, "ze", &mut ctx);
        assert!(matches!(
            act(&mut search, Action::Back, &mut ctx),
            Transition::None
        ));
        assert_eq!((search.query(), search.games.len()), ("", 0));
        assert!(matches!(
            act(&mut search, Action::Back, &mut ctx),
            Transition::Pop
        ));
    }

    #[test]
    fn back_from_the_results_closes() {
        let (_dir, mut ctx) = ctx_with(&["Zelda"]);
        let mut search = Search::new(&ctx);
        type_text(&mut search, "ze", &mut ctx);
        act(&mut search, Action::Down, &mut ctx);
        assert!(matches!(
            act(&mut search, Action::Back, &mut ctx),
            Transition::Pop
        ));
    }

    #[test]
    fn search_from_the_results_refocuses_the_field() {
        let (_dir, mut ctx) = ctx_with(&["Zelda"]);
        let mut search = Search::new(&ctx);
        type_text(&mut search, "ze", &mut ctx);
        act(&mut search, Action::Down, &mut ctx);
        act(&mut search, Action::Search, &mut ctx);
        assert!(search.text_input());
        assert!(matches!(
            act(&mut search, Action::Search, &mut ctx),
            Transition::None
        ));
        assert!(search.text_input(), "Search in the field stays there");
    }

    #[test]
    fn an_empty_query_hints_at_the_visible_count_and_no_match_says_so() {
        let (_dir, mut ctx) = ctx_with(&["Zelda", "Metroid", "F-Zero"]);
        let mut search = Search::new(&ctx);
        assert_eq!(message(search.query(), search.total), "Type to search 3 Games");
        act(&mut search, Action::Down, &mut ctx);
        assert!(search.text_input(), "with no results Down stays in the field");
        assert_eq!(search.hints(), [Hint::new(&[Action::Back], "Close")]);
        type_text(&mut search, "kirby", &mut ctx);
        assert!(search.games.is_empty());
        assert_eq!(message(search.query(), search.total), "No Games match “kirby”");
        act(&mut search, Action::Start, &mut ctx);
        assert!(search.text_input(), "with no results Enter stays in the field");
    }

    #[test]
    fn the_field_hints_lead_to_the_results_when_there_are_any() {
        let (_dir, mut ctx) = ctx_with(&["Zelda"]);
        let mut search = Search::new(&ctx);
        type_text(&mut search, "z", &mut ctx);
        assert_eq!(
            search.hints(),
            [
                Hint::new(&[Action::Down], "Results"),
                Hint::new(&[Action::Back], "Close")
            ]
        );
    }

    #[test]
    fn a_hidden_game_never_shows_or_counts() {
        let (_dir, mut ctx) = ctx_with(&["Zelda", "Zelda II"]);
        let mut search = Search::new(&ctx);
        type_text(&mut search, "zelda", &mut ctx);
        let id = search.games.iter().find(|g| g.title == "Zelda II").unwrap().id;
        ctx.library.set_hidden(id, true).unwrap();
        ctx.revision += 1;
        act(&mut search, Action::Down, &mut ctx);
        assert_eq!(
            titles(&search),
            ["Zelda"],
            "the results reload when the library changes"
        );
        assert_eq!(Search::new(&ctx).total, 1, "nor counts in the empty query's hint");
    }

    /// The same library with the player on a controller.
    fn pad_ctx_with(titles: &[&str]) -> (tempfile::TempDir, Ctx) {
        let (dir, mut ctx) = ctx_with(titles);
        ctx.input_kind = InputKind::Controller;
        (dir, ctx)
    }

    #[test]
    fn on_a_controller_the_field_types_with_the_on_screen_keyboard() {
        let (_dir, mut ctx) = pad_ctx_with(&["Quest", "Q Bert"]);
        let mut search = Search::new(&ctx);
        assert!(search.keyboard_shown() && search.text_input());
        assert_eq!(search.bottom(), TRAY_Y, "the results end above the keyboard");
        act(&mut search, Action::Confirm, &mut ctx);
        assert_eq!(search.query(), "q", "A types the focused key");
        assert_eq!(titles(&search), ["Q Bert", "Quest"]);
        act(&mut search, Action::Sort, &mut ctx);
        assert_eq!(search.query(), "q ", "X types a space");
        act(&mut search, Action::Options, &mut ctx);
        assert_eq!(search.query(), "q", "Y erases");
        act(&mut search, Action::Down, &mut ctx);
        assert!(search.keyboard_shown(), "Down moves over the keys");
        assert_eq!(search.entry.focused(), Key::Char('a'));
    }

    #[test]
    fn start_or_done_moves_into_the_results_and_up_from_the_top_row_brings_the_keyboard_back() {
        let (_dir, mut ctx) = pad_ctx_with(&["Quest"]);
        let mut search = Search::new(&ctx);
        act(&mut search, Action::Confirm, &mut ctx);
        act(&mut search, Action::Start, &mut ctx);
        assert!(
            !search.keyboard_shown() && !search.text_input(),
            "Start finishes typing"
        );
        assert_eq!(search.bottom(), BOTTOM);
        act(&mut search, Action::Up, &mut ctx);
        assert!(
            search.keyboard_shown(),
            "Up from the top row brings the keyboard back"
        );
        // Down to the bottom row, then round to Done.
        for dir in [Action::Down, Action::Down, Action::Down, Action::Left] {
            act(&mut search, dir, &mut ctx);
        }
        assert_eq!(search.entry.focused(), Key::Done);
        act(&mut search, Action::Confirm, &mut ctx);
        assert!(
            !search.keyboard_shown() && !search.text_input(),
            "Done finishes typing"
        );
    }

    #[test]
    fn done_with_no_results_stays_in_the_field() {
        let (_dir, mut ctx) = pad_ctx_with(&["Quest"]);
        let mut search = Search::new(&ctx);
        act(&mut search, Action::Start, &mut ctx);
        assert!(search.keyboard_shown());
    }

    #[test]
    fn back_on_the_on_screen_keyboard_clears_then_closes() {
        let (_dir, mut ctx) = pad_ctx_with(&["Quest"]);
        let mut search = Search::new(&ctx);
        act(&mut search, Action::Confirm, &mut ctx);
        assert!(matches!(
            act(&mut search, Action::Back, &mut ctx),
            Transition::None
        ));
        assert_eq!(search.query(), "");
        assert!(matches!(
            act(&mut search, Action::Back, &mut ctx),
            Transition::Pop
        ));
    }

    #[test]
    fn the_keyboard_follows_the_input_kind_mid_way() {
        let (_dir, mut ctx) = pad_ctx_with(&["Quest"]);
        let mut search = Search::new(&ctx);
        act(&mut search, Action::Confirm, &mut ctx);
        ctx.input_kind = InputKind::Keyboard;
        act(&mut search, Action::Down, &mut ctx);
        assert!(!search.text_input(), "on the keyboard Down goes into the results");
        act(&mut search, Action::Up, &mut ctx);
        assert!(search.text_input() && !search.keyboard_shown());
        ctx.input_kind = InputKind::Controller;
        act(&mut search, Action::Down, &mut ctx);
        assert!(
            search.keyboard_shown(),
            "back on the controller Down moves over the keys"
        );
    }

    #[test]
    fn the_on_screen_keyboard_hints_end_with_results_and_close() {
        let (_dir, mut ctx) = pad_ctx_with(&["Quest"]);
        let mut search = Search::new(&ctx);
        let texts = |search: &Search| search.hints().iter().map(|h| h.text).collect::<Vec<_>>();
        assert_eq!(texts(&search), ["Type", "Backspace", "Space", "Close"]);
        act(&mut search, Action::Confirm, &mut ctx);
        assert_eq!(texts(&search), ["Type", "Backspace", "Space", "Results", "Close"]);
        assert_eq!(search.hints()[3], Hint::new(&[Action::Start], "Results"));
    }

    #[test]
    fn a_click_on_a_result_opens_it_from_the_field_or_the_results() {
        let (_dir, mut ctx) = ctx_with(&["Super Mario World", "Paper Mario"]);
        let mut search = Search::new(&ctx);
        type_text(&mut search, "mario", &mut ctx);
        search.hits = vec![(Rect::new(300.0, 200.0, 50.0, 50.0), 1)];
        let click = Input::Pointer(Pointer::Click { x: 310.0, y: 210.0 });
        let move_over = Input::Pointer(Pointer::Move { x: 310.0, y: 210.0 });
        search.handle(move_over, &mut ctx);
        assert!(
            search.text_input(),
            "hovering a result doesn't take focus from the field"
        );
        let Transition::Push(page) = search.handle(click, &mut ctx) else {
            panic!("a click on a result should open its Details Page");
        };
        assert_eq!(page.title(), "Super Mario World");
        assert!(!search.text_input(), "and focuses it, for coming back");
        let right_click = Input::Pointer(Pointer::RightClick { x: 310.0, y: 210.0 });
        let Transition::Push(menu) = search.handle(right_click, &mut ctx) else {
            panic!("a right-click on a result should open its Options Menu");
        };
        assert_eq!(menu.title(), "Super Mario World");
        let miss = Input::Pointer(Pointer::Click { x: 10.0, y: 900.0 });
        assert!(matches!(search.handle(miss, &mut ctx), Transition::None));
    }

    #[test]
    fn a_click_on_the_field_focuses_it() {
        let (_dir, mut ctx) = pad_ctx_with(&["Quest"]);
        let mut search = Search::new(&ctx);
        act(&mut search, Action::Confirm, &mut ctx);
        act(&mut search, Action::Start, &mut ctx);
        assert!(!search.text_input());
        let click = Input::Pointer(Pointer::Click {
            x: FIELD.x + 100.0,
            y: 60.0,
        });
        search.handle(click, &mut ctx);
        assert!(search.text_input());
        assert!(
            search.keyboard_shown(),
            "on a controller with the on-screen keyboard"
        );
        search.handle(click, &mut ctx);
        assert_eq!(search.query(), "q", "a click on the field types nothing");
    }

    #[test]
    fn the_count_line_is_singular_for_one_game() {
        assert_eq!(count(1), "1 Game");
        assert_eq!(count(7), "7 Games");
    }
}
