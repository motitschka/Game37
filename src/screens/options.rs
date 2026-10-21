//! The Options Menu for one Game (FR-DET-3, FR-DET-4), after `screen_options` in
//! `design/generate_mockups.py`: a panel on the right over the dimmed screen. With five Favorites
//! already, adding a sixth shows a picker of them in the same panel (FR-HOME-6, US-6). Rename
//! opens a wider panel with a text field and an on-screen keyboard (US-3): the menu stays one
//! modal, so saving closes it like every other action.

use crate::ctx::{self, Ctx};
use crate::library::db::{GameRow, Shelf};
use crate::systems::{self, Kind};
use crate::ui::cover::{self, ellipsize, playtime_label};
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, InputKind, Pointer};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::text_entry::{KEYBOARD_H, KEYBOARD_W, Outcome, TextEntry};
use crate::ui::theme::{ACCENT, Colour, DIM, LINE, MUTED, STAR, SURFACE, SURFACE2, TEXT};
use crate::ui::{H, Rect, W, hit};

/// At most this many Favorites exist (FR-HOME-6).
pub const MAX_FAVORITES: usize = 5;

const PANEL_W: f32 = 600.0;
const PANEL_X: f32 = W - PANEL_W;
const ITEM_Y: f32 = 252.0;
const ITEM_PITCH: f32 = 86.0;
const PICK_Y: f32 = 290.0;
const PICK_PITCH: f32 = 112.0;
/// The rename panel, centred above the hint bar.
const RENAME_W: f32 = 1240.0;
const RENAME_H: f32 = 768.0;
const RENAME_X: f32 = (W - RENAME_W) / 2.0;
const RENAME_Y: f32 = 120.0;

/// The longest Display Title typing can make, in characters.
pub const MAX_TITLE_CHARS: usize = 100;

/// One action in the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// Add to or remove from Favorites.
    Favorite,
    /// Load one of the Game's Save States, with a slot picker; Embedded Systems only. Shown but
    /// not selectable until it is built.
    LoadSaveState,
    /// Choose different Artwork. Shown but not selectable until it is built.
    ChooseArtwork,
    /// Rename the Display Title; the file on disk stays as it is (FR-DET-5).
    Rename,
    Hide,
    ShowFile,
}

impl Item {
    /// Whether the action is built yet; the others are greyed and skipped by focus.
    pub fn enabled(self) -> bool {
        !matches!(self, Item::LoadSaveState | Item::ChooseArtwork)
    }

    fn label(self, favorite: bool) -> &'static str {
        match self {
            Item::Favorite if favorite => "Remove from favorites",
            Item::Favorite => "Add to favorites",
            Item::LoadSaveState => "Load save state",
            Item::ChooseArtwork => "Choose artwork",
            Item::Rename => "Rename",
            Item::Hide => "Hide game",
            Item::ShowFile => "Show file in file manager",
        }
    }

    /// Whether choosing it opens a further view, shown with a "›".
    fn opens_view(self) -> bool {
        matches!(self, Item::LoadSaveState | Item::ChooseArtwork)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    List,
    /// Five Favorites exist: choose which one the Game replaces.
    Replace,
    /// Typing a new Display Title.
    Rename,
}

pub struct OptionsMenu {
    game: GameRow,
    /// "<System> · <Playtime> played".
    meta: String,
    items: Vec<Item>,
    view: View,
    item: usize,
    /// The current Favorites, newest first, while the replace picker is open.
    favorites: Vec<GameRow>,
    pick: usize,
    item_hits: Vec<(Rect, usize)>,
    pick_hits: Vec<(Rect, usize)>,
    /// The new Display Title being typed, while renaming.
    entry: Option<TextEntry>,
    /// The cleaned-up title a rename overrides, while renaming.
    cleaned: String,
    /// What the player last used, as of the last frame, for the rename hints.
    kind: InputKind,
}

impl OptionsMenu {
    pub fn new(game: GameRow) -> OptionsMenu {
        let system = systems::by_id(&game.system);
        let short = system.map_or("?", |s| s.short);
        let meta = match game.playtime_secs {
            0 => short.to_owned(),
            secs => format!("{short}  ·  {} played", playtime_label(secs)),
        };
        let embedded = system.is_some_and(|s| s.kind == Kind::Embedded);
        let items = [
            Item::Favorite,
            Item::LoadSaveState,
            Item::ChooseArtwork,
            Item::Rename,
            Item::Hide,
            Item::ShowFile,
        ]
        .into_iter()
        .filter(|&i| i != Item::LoadSaveState || embedded)
        .collect();
        OptionsMenu {
            game,
            meta,
            items,
            view: View::List,
            item: 0,
            favorites: Vec::new(),
            pick: 0,
            item_hits: Vec::new(),
            pick_hits: Vec::new(),
            entry: None,
            cleaned: String::new(),
            kind: InputKind::Controller,
        }
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn focused(&self) -> Item {
        self.items[self.item]
    }

    /// Moves focus one step `dir` (-1 or 1), skipping actions that aren't built yet.
    fn move_item(&mut self, dir: isize) {
        let mut i = self.item as isize + dir;
        while (0..self.items.len() as isize).contains(&i) {
            if self.items[i as usize].enabled() {
                self.item = i as usize;
                return;
            }
            i += dir;
        }
    }

    fn choose(&mut self, i: usize, ctx: &mut Ctx) -> Transition {
        let item = self.items[i];
        if !item.enabled() {
            return Transition::None;
        }
        self.item = i;
        let id = self.game.id;
        let done = match item {
            Item::Favorite if self.game.favorite => ctx.library.set_favorite(id, false, ctx::now()),
            Item::Favorite => match ctx.library.shelf(Shelf::Favorites, None) {
                Ok(favorites) if favorites.len() >= MAX_FAVORITES => {
                    self.favorites = favorites;
                    self.pick = 0;
                    self.view = View::Replace;
                    return Transition::None;
                }
                Ok(_) => ctx.library.set_favorite(id, true, ctx::now()),
                Err(e) => Err(e),
            },
            // A Hidden Game is no Favorite: it would count towards the five unseen.
            Item::Hide => ctx
                .library
                .set_hidden(id, true)
                .and_then(|()| ctx.library.set_favorite(id, false, ctx::now())),
            Item::ShowFile => {
                ctx.show_file_request = Some(ctx.games_root.join(&self.game.rel_path));
                return Transition::Pop;
            }
            Item::Rename => {
                self.cleaned = match ctx.library.cleaned_title(id) {
                    Ok(Some(cleaned)) => cleaned,
                    Ok(None) => self.game.title.clone(),
                    Err(e) => return finish(Err(e), ctx),
                };
                self.entry = Some(TextEntry::new(&self.game.title, MAX_TITLE_CHARS));
                self.view = View::Rename;
                return Transition::None;
            }
            Item::LoadSaveState | Item::ChooseArtwork => return Transition::None,
        };
        finish(done, ctx)
    }

    /// Replaces the `i`th Favorite with this Game.
    fn replace(&mut self, i: usize, ctx: &mut Ctx) -> Transition {
        let Some(old) = self.favorites.get(i) else {
            return Transition::None;
        };
        let now = ctx::now();
        let done = ctx
            .library
            .set_favorite(old.id, false, now)
            .and_then(|()| ctx.library.set_favorite(self.game.id, true, now));
        finish(done, ctx)
    }

    /// Saves the typed Display Title, trimmed. Empty, or the cleaned-up title, undoes the rename.
    fn save_rename(&mut self, ctx: &mut Ctx) -> Transition {
        let typed = self.entry.as_ref().map_or("", |e| e.text()).trim();
        let title = (!typed.is_empty() && typed != self.cleaned).then(|| typed.to_owned());
        finish(ctx.library.rename(self.game.id, title), ctx)
    }

    fn back_to_list(&mut self) {
        self.view = View::List;
        self.favorites.clear();
        self.entry = None;
    }

    fn draw_rename(&mut self, gfx: &mut Gfx<'_>) {
        let panel = Rect::new(RENAME_X, RENAME_Y, RENAME_W, RENAME_H);
        gfx.fill_round_rect(panel, 20.0, SURFACE);
        gfx.stroke_round_rect(panel, 20.0, 1.0, LINE);
        let x = RENAME_X + (RENAME_W - KEYBOARD_W) / 2.0;
        let heading = TextStyle::new(30, Weight::ExtraBold, TEXT);
        gfx.text(x, RENAME_Y + 72.0, "Rename", heading, Align::Left);
        let meta = TextStyle::new(18, Weight::Medium, MUTED);
        gfx.text(x, RENAME_Y + 106.0, &self.meta, meta, Align::Left);
        let Some(entry) = &mut self.entry else {
            return;
        };
        let field = Rect::new(x, RENAME_Y + 136.0, KEYBOARD_W, 88.0);
        entry.draw_field(gfx, field, &self.cleaned);
        let note = TextStyle::new(16, Weight::Medium, DIM);
        let reset = format!("Leave empty to use “{}”. The file keeps its name.", self.cleaned);
        let reset = ellipsize(&reset, KEYBOARD_W - 120.0, |s| gfx.measure(s, note));
        gfx.text(x, RENAME_Y + 256.0, &reset, note, Align::Left);
        let count = format!("{} / {MAX_TITLE_CHARS}", entry.text().chars().count());
        gfx.text(x + KEYBOARD_W, RENAME_Y + 256.0, &count, note, Align::Right);
        let top = RENAME_Y + RENAME_H - 48.0 - KEYBOARD_H;
        entry.draw_keyboard(gfx, x, top);
    }

    fn draw_list(&mut self, gfx: &mut Gfx<'_>) {
        self.item_hits.clear();
        for (i, &item) in self.items.iter().enumerate() {
            let y = ITEM_Y + i as f32 * ITEM_PITCH;
            let r = Rect::new(PANEL_X + 24.0, y, PANEL_W - 48.0, 72.0);
            let on = i == self.item;
            if on {
                gfx.fill_round_rect(r, 14.0, SURFACE2);
                gfx.fill_round_rect(Rect::new(r.x, y + 16.0, 5.0, 40.0), 2.0, ACCENT);
            }
            let (ink, icon_ink) = match item {
                _ if !item.enabled() => (DIM, DIM),
                Item::Favorite if self.game.favorite => (TEXT, STAR),
                _ => (TEXT, MUTED),
            };
            icon(gfx, item, PANEL_X + 62.0, y + 38.0, icon_ink);
            let weight = if on { Weight::SemiBold } else { Weight::Medium };
            let label = item.label(self.game.favorite);
            gfx.text(
                PANEL_X + 100.0,
                y + 45.0,
                label,
                TextStyle::new(22, weight, ink),
                Align::Left,
            );
            if item.opens_view() {
                let arrow = TextStyle::new(26, Weight::SemiBold, if item.enabled() { MUTED } else { DIM });
                gfx.text(W - 48.0, y + 45.0, "›", arrow, Align::Right);
            }
            self.item_hits.push((r, i));
        }
    }

    fn draw_replace(&mut self, gfx: &mut Gfx<'_>) {
        self.pick_hits.clear();
        let heading = TextStyle::new(24, Weight::ExtraBold, TEXT);
        gfx.text(PANEL_X + 48.0, 238.0, "Favorites are full", heading, Align::Left);
        let sub = TextStyle::new(18, Weight::Medium, MUTED);
        gfx.text(PANEL_X + 48.0, 268.0, "Choose one to replace", sub, Align::Left);
        for (i, game) in self.favorites.iter().enumerate() {
            let y = PICK_Y + i as f32 * PICK_PITCH;
            let r = Rect::new(PANEL_X + 24.0, y, PANEL_W - 48.0, 100.0);
            if i == self.pick {
                gfx.fill_round_rect(r, 14.0, SURFACE2);
                gfx.fill_round_rect(Rect::new(r.x, y + 30.0, 5.0, 40.0), 2.0, ACCENT);
            }
            cover::draw(
                gfx,
                Rect::new(PANEL_X + 48.0, y + 8.0, 56.0, 84.0),
                game,
                false,
                false,
            );
            let title = TextStyle::new(22, Weight::Bold, TEXT);
            let label = ellipsize(&game.title, PANEL_W - 180.0, |s| gfx.measure(s, title));
            gfx.text(PANEL_X + 128.0, y + 46.0, &label, title, Align::Left);
            let short = systems::by_id(&game.system).map_or("?", |s| s.short);
            gfx.text(PANEL_X + 128.0, y + 76.0, short, sub, Align::Left);
            self.pick_hits.push((r, i));
        }
    }
}

/// Ends an action: the library changed, so screens reload; the menu closes.
fn finish(done: Result<(), crate::library::db::DbError>, ctx: &mut Ctx) -> Transition {
    if let Err(e) = done {
        eprintln!("game37: {e}");
    }
    ctx.revision += 1;
    Transition::Pop
}

/// A 24 px icon centred on (cx, cy), drawn rather than a text glyph (not every font covers them).
fn icon(gfx: &mut Gfx<'_>, item: Item, cx: f32, cy: f32, c: Colour) {
    match item {
        Item::Favorite => super::details::star(gfx, cx, cy, 12.0, c),
        Item::LoadSaveState => {
            gfx.stroke_circle(cx, cy, 9.0, 2.5, c);
            gfx.fill_polygon(
                &[(cx - 13.0, cy - 6.0), (cx - 5.0, cy - 6.0), (cx - 9.0, cy + 1.0)],
                c,
            );
        }
        Item::ChooseArtwork => {
            gfx.stroke_round_rect(Rect::new(cx - 10.0, cy - 10.0, 20.0, 20.0), 3.0, 2.5, c);
            gfx.fill_rect(Rect::new(cx - 5.0, cy - 5.0, 10.0, 10.0), c);
        }
        Item::Rename => {
            gfx.line(cx - 7.0, cy + 7.0, cx + 8.0, cy - 8.0, 4.0, c);
            gfx.fill_polygon(
                &[(cx - 11.0, cy + 11.0), (cx - 9.0, cy + 4.0), (cx - 4.0, cy + 9.0)],
                c,
            );
        }
        Item::Hide => {
            gfx.stroke_circle(cx, cy, 10.0, 2.5, c);
            gfx.line(cx - 7.0, cy + 7.0, cx + 7.0, cy - 7.0, 2.5, c);
        }
        Item::ShowFile => {
            gfx.stroke_round_rect(Rect::new(cx - 10.0, cy - 10.0, 20.0, 20.0), 3.0, 2.5, c);
            for dy in [-3.0, 3.0] {
                gfx.fill_rect(Rect::new(cx - 6.0, cy + dy - 1.0, 12.0, 2.0), c);
            }
        }
    }
}

impl Screen for OptionsMenu {
    fn title(&self) -> &str {
        &self.game.title
    }

    fn is_modal(&self) -> bool {
        true
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        if let Some(entry) = &mut self.entry {
            match entry.handle(input) {
                Outcome::Submit => return self.save_rename(ctx),
                Outcome::Cancel => self.back_to_list(),
                Outcome::None | Outcome::Changed => {}
            }
            return Transition::None;
        }
        let list = self.view == View::List;
        let last_pick = self.favorites.len().saturating_sub(1);
        match input {
            Input::Action(Action::Up) if list => self.move_item(-1),
            Input::Action(Action::Down) if list => self.move_item(1),
            Input::Action(Action::Confirm) if list => return self.choose(self.item, ctx),
            Input::Action(Action::Back) | Input::Pointer(Pointer::Back) if list => return Transition::Pop,
            Input::Action(Action::Up) => self.pick = self.pick.saturating_sub(1),
            Input::Action(Action::Down) => self.pick = (self.pick + 1).min(last_pick),
            Input::Action(Action::Confirm) => return self.replace(self.pick, ctx),
            Input::Action(Action::Back) | Input::Pointer(Pointer::Back) => self.back_to_list(),
            Input::Pointer(Pointer::Move { x, y }) => {
                if let Some(i) = hit(&self.item_hits, x, y).filter(|&i| list && self.items[i].enabled()) {
                    self.item = i;
                } else if let Some(i) = hit(&self.pick_hits, x, y).filter(|_| !list) {
                    self.pick = i;
                }
            }
            Input::Pointer(Pointer::Click { x, .. }) if x < PANEL_X => {
                return if list { Transition::Pop } else { Transition::None };
            }
            Input::Pointer(Pointer::Click { x, y }) => {
                if let Some(i) = hit(&self.item_hits, x, y).filter(|_| list) {
                    return self.choose(i, ctx);
                } else if let Some(i) = hit(&self.pick_hits, x, y).filter(|_| !list) {
                    return self.replace(i, ctx);
                }
            }
            _ => {}
        }
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.kind = ctx.input_kind;
        gfx.fill_rect(Rect::new(0.0, 0.0, W, H), Colour::hex(0x000000).alpha(0.62));
        if self.view == View::Rename {
            self.item_hits.clear();
            return self.draw_rename(gfx);
        }
        gfx.fill_rect(Rect::new(PANEL_X, 0.0, PANEL_W, H), SURFACE);
        gfx.fill_rect(Rect::new(PANEL_X, 0.0, 1.0, H), LINE);
        cover::draw(
            gfx,
            Rect::new(PANEL_X + 48.0, 64.0, 96.0, 144.0),
            &self.game,
            false,
            false,
        );
        let title = TextStyle::new(28, Weight::ExtraBold, TEXT);
        let label = ellipsize(&self.game.title, PANEL_W - 216.0, |s| gfx.measure(s, title));
        gfx.text(PANEL_X + 168.0, 118.0, &label, title, Align::Left);
        let meta = TextStyle::new(18, Weight::Medium, MUTED);
        gfx.text(PANEL_X + 168.0, 152.0, &self.meta, meta, Align::Left);
        match self.view {
            View::List => {
                self.pick_hits.clear();
                self.draw_list(gfx);
            }
            View::Replace => {
                self.item_hits.clear();
                self.draw_replace(gfx);
            }
            View::Rename => {}
        }
    }

    fn hints(&self) -> Vec<Hint> {
        match self.view {
            View::List => vec![
                Hint::new(&[Action::Confirm], "Choose"),
                Hint::new(&[Action::Back], "Close"),
            ],
            View::Replace => vec![
                Hint::new(&[Action::Confirm], "Replace"),
                Hint::new(&[Action::Back], "Back"),
            ],
            View::Rename => TextEntry::hints(self.kind, "Save"),
        }
    }

    fn text_input(&self) -> bool {
        self.view == View::Rename
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::FoundGame;
    use crate::library::db::SortOrder;
    use crate::ui::input::TextInput;

    /// A Ctx with a temporary Games Root holding `n` SNES Games, "G0" to "G<n-1>".
    fn world(n: usize) -> (tempfile::TempDir, Ctx, Vec<GameRow>) {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = dir.path().join("Games");
        let found: Vec<_> = (0..n)
            .map(|i| FoundGame {
                system: "snes",
                rel_path: format!("snes/G{i}.sfc"),
                title: format!("G{i}"),
            })
            .collect();
        ctx.library.sync_system("snes", &found, 0).unwrap();
        let games = ctx.library.system_games("snes", SortOrder::AZ).unwrap();
        (dir, ctx, games)
    }

    fn act(menu: &mut OptionsMenu, ctx: &mut Ctx, action: Action) -> Transition {
        menu.handle(Input::Action(action), ctx)
    }

    fn fresh(ctx: &Ctx, game: &GameRow) -> GameRow {
        ctx.library.game(game.id).unwrap().unwrap()
    }

    fn favorite_ids(ctx: &Ctx) -> Vec<i64> {
        let mut ids: Vec<_> = ctx
            .library
            .shelf(Shelf::Favorites, None)
            .unwrap()
            .iter()
            .map(|g| g.id)
            .collect();
        ids.sort();
        ids
    }

    #[test]
    fn favorite_toggles_and_the_menu_closes() {
        let (_dir, mut ctx, games) = world(1);
        let mut menu = OptionsMenu::new(games[0].clone());
        assert_eq!(menu.focused(), Item::Favorite, "focus starts on Favorite");
        assert_eq!(Item::Favorite.label(menu.game.favorite), "Add to favorites");
        let revision = ctx.revision;
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::Pop
        ));
        assert!(fresh(&ctx, &games[0]).favorite);
        assert!(ctx.revision > revision, "screens reload");

        let mut menu = OptionsMenu::new(fresh(&ctx, &games[0]));
        assert_eq!(Item::Favorite.label(menu.game.favorite), "Remove from favorites");
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::Pop
        ));
        assert!(!fresh(&ctx, &games[0]).favorite);
    }

    /// Five Favorites, G0 to G4, favorited in that order, and a menu for G5.
    fn five_favorites() -> (tempfile::TempDir, Ctx, Vec<GameRow>, OptionsMenu) {
        let (dir, ctx, games) = world(6);
        for (i, game) in games[..5].iter().enumerate() {
            ctx.library.set_favorite(game.id, true, i as i64).unwrap();
        }
        let menu = OptionsMenu::new(games[5].clone());
        (dir, ctx, games, menu)
    }

    #[test]
    fn favorite_with_five_favorites_opens_a_picker_and_the_one_picked_is_replaced() {
        let (_dir, mut ctx, games, mut menu) = five_favorites();
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::None
        ));
        assert_eq!(menu.view(), View::Replace);
        assert_eq!(menu.hints()[0], Hint::new(&[Action::Confirm], "Replace"));
        let shown: Vec<_> = menu.favorites.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(shown, ["G4", "G3", "G2", "G1", "G0"], "the five, newest first");
        assert!(!fresh(&ctx, &games[5]).favorite, "nothing changes yet");

        act(&mut menu, &mut ctx, Action::Down);
        act(&mut menu, &mut ctx, Action::Down);
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::Pop
        ));
        let mut want = vec![games[0].id, games[1].id, games[3].id, games[4].id, games[5].id];
        want.sort();
        assert_eq!(favorite_ids(&ctx), want, "G2 made way for G5");
    }

    #[test]
    fn a_click_on_a_favorite_in_the_picker_replaces_it() {
        let (_dir, mut ctx, games, mut menu) = five_favorites();
        act(&mut menu, &mut ctx, Action::Confirm);
        menu.pick_hits = vec![(Rect::new(PANEL_X, 400.0, 100.0, 100.0), 4)];
        let at = Input::Pointer(Pointer::Click {
            x: PANEL_X + 10.0,
            y: 410.0,
        });
        assert!(matches!(menu.handle(at, &mut ctx), Transition::Pop));
        assert!(!fresh(&ctx, &games[0]).favorite, "G0 is the 5th, oldest");
        assert!(fresh(&ctx, &games[5]).favorite);
        assert_eq!(favorite_ids(&ctx).len(), MAX_FAVORITES);
    }

    #[test]
    fn back_from_the_favorite_picker_changes_nothing() {
        let (_dir, mut ctx, _games, mut menu) = five_favorites();
        let before = favorite_ids(&ctx);
        act(&mut menu, &mut ctx, Action::Confirm);
        act(&mut menu, &mut ctx, Action::Down);
        let revision = ctx.revision;
        let outside = Input::Pointer(Pointer::Click { x: 10.0, y: 10.0 });
        assert!(
            matches!(menu.handle(outside, &mut ctx), Transition::None),
            "a click outside"
        );
        assert_eq!(menu.view(), View::Replace);
        assert!(matches!(act(&mut menu, &mut ctx, Action::Back), Transition::None));
        assert_eq!(menu.view(), View::List, "Back returns to the list");
        assert_eq!(menu.focused(), Item::Favorite);
        assert!(matches!(act(&mut menu, &mut ctx, Action::Back), Transition::Pop));
        assert_eq!(favorite_ids(&ctx), before);
        assert_eq!(ctx.revision, revision);
    }

    #[test]
    fn the_actions_not_built_yet_are_listed_but_not_selectable() {
        let (_dir, mut ctx, games) = world(1);
        let mut menu = OptionsMenu::new(games[0].clone());
        use Item::*;
        assert_eq!(
            menu.items(),
            [Favorite, LoadSaveState, ChooseArtwork, Rename, Hide, ShowFile]
        );
        act(&mut menu, &mut ctx, Action::Down);
        assert_eq!(menu.focused(), Rename, "focus skips them");
        act(&mut menu, &mut ctx, Action::Up);
        assert_eq!(menu.focused(), Favorite);
        act(&mut menu, &mut ctx, Action::Up);
        assert_eq!(menu.focused(), Favorite, "and stops at the ends");

        menu.item_hits = vec![(Rect::new(PANEL_X, 300.0, 100.0, 50.0), 2)];
        menu.handle(
            Input::Pointer(Pointer::Move {
                x: PANEL_X + 1.0,
                y: 301.0,
            }),
            &mut ctx,
        );
        assert_eq!(menu.focused(), Favorite, "nor does the pointer focus them");
        let click = Input::Pointer(Pointer::Click {
            x: PANEL_X + 1.0,
            y: 301.0,
        });
        assert!(matches!(menu.handle(click, &mut ctx), Transition::None));
        assert!(!fresh(&ctx, &games[0]).favorite);
    }

    #[test]
    fn load_save_state_is_listed_for_embedded_systems_only() {
        let mut game = world(1).2.remove(0);
        game.system = "gc".into();
        assert!(!OptionsMenu::new(game).items().contains(&Item::LoadSaveState));
    }

    #[test]
    fn hide_hides_the_game_and_unfavorites_it() {
        let (_dir, mut ctx, games) = world(2);
        ctx.library.set_favorite(games[0].id, true, 0).unwrap();
        let mut menu = OptionsMenu::new(fresh(&ctx, &games[0]));
        act(&mut menu, &mut ctx, Action::Down);
        act(&mut menu, &mut ctx, Action::Down);
        assert_eq!(menu.focused(), Item::Hide);
        let revision = ctx.revision;
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::Pop
        ));
        assert!(ctx.revision > revision);
        assert_eq!(ctx.library.game(games[0].id).unwrap(), None);
        let left = ctx.library.system_games("snes", SortOrder::AZ).unwrap();
        assert_eq!(left.iter().map(|g| g.id).collect::<Vec<_>>(), [games[1].id]);
        assert_eq!(ctx.library.shelf_count(Shelf::Favorites).unwrap(), 0);
    }

    #[test]
    fn show_file_asks_for_the_games_file_and_a_folder_game_for_its_folder() {
        let (_dir, mut ctx, games) = world(1);
        let mut menu = OptionsMenu::new(games[0].clone());
        act(&mut menu, &mut ctx, Action::Down);
        act(&mut menu, &mut ctx, Action::Down);
        act(&mut menu, &mut ctx, Action::Down);
        assert_eq!(menu.focused(), Item::ShowFile);
        let revision = ctx.revision;
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::Pop
        ));
        assert_eq!(
            ctx.show_file_request.take(),
            Some(ctx.games_root.join("snes/G0.sfc"))
        );
        assert_eq!(ctx.revision, revision, "nothing to reload");

        let folder = ctx.games_root.join("ps3/Demon's Souls");
        std::fs::create_dir_all(folder.join("PS3_GAME")).unwrap();
        ctx.scan_all(0).unwrap();
        let game = ctx.library.system_games("ps3", SortOrder::AZ).unwrap().remove(0);
        let mut menu = OptionsMenu::new(game);
        let at = (PANEL_X + 10.0, 600.0);
        menu.item_hits = vec![(Rect::new(PANEL_X, 590.0, 100.0, 50.0), menu.items().len() - 1)];
        menu.handle(Input::Pointer(Pointer::Click { x: at.0, y: at.1 }), &mut ctx);
        assert_eq!(ctx.show_file_request, Some(folder));
    }

    #[test]
    fn back_or_a_click_outside_the_panel_closes_the_menu_and_changes_nothing() {
        let (_dir, mut ctx, games) = world(1);
        let mut menu = OptionsMenu::new(games[0].clone());
        let revision = ctx.revision;
        assert!(matches!(act(&mut menu, &mut ctx, Action::Back), Transition::Pop));
        let outside = Input::Pointer(Pointer::Click {
            x: PANEL_X - 1.0,
            y: 10.0,
        });
        assert!(matches!(menu.handle(outside, &mut ctx), Transition::Pop));
        assert_eq!(ctx.revision, revision);
        assert!(!fresh(&ctx, &games[0]).favorite);
    }

    /// A world of one Game whose file exists on disk with a few bytes, and a menu for it with
    /// the rename view open.
    fn renaming() -> (tempfile::TempDir, Ctx, GameRow, OptionsMenu) {
        let (dir, mut ctx, games) = world(1);
        let snes = ctx.games_root.join("snes");
        std::fs::create_dir_all(&snes).unwrap();
        std::fs::write(snes.join("G0.sfc"), b"not a game").unwrap();
        let mut menu = OptionsMenu::new(games[0].clone());
        act(&mut menu, &mut ctx, Action::Down);
        assert_eq!(menu.focused(), Item::Rename);
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::None
        ));
        assert_eq!(menu.view(), View::Rename);
        assert!(menu.text_input(), "the field takes typed text");
        assert_eq!(
            menu.entry.as_ref().unwrap().text(),
            "G0",
            "it starts as the Display Title"
        );
        (dir, ctx, games[0].clone(), menu)
    }

    fn type_in(menu: &mut OptionsMenu, ctx: &mut Ctx, text: &str) {
        for ch in text.chars() {
            menu.handle(Input::Text(TextInput::Char(ch)), ctx);
        }
    }

    /// The file is where it was, with the same name and bytes, alone in its folder.
    fn file_untouched(ctx: &Ctx) {
        let snes = ctx.games_root.join("snes");
        let names: Vec<_> = std::fs::read_dir(&snes)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, ["G0.sfc"]);
        assert_eq!(std::fs::read(snes.join("G0.sfc")).unwrap(), b"not a game");
    }

    #[test]
    fn rename_with_the_on_screen_keyboard_changes_the_display_title_not_the_file() {
        let (_dir, mut ctx, game, mut menu) = renaming();
        let revision = ctx.revision;
        use Action::*;
        // Y erases "G0"; Shift, m, k, then X for a space and + to save.
        let steps = [
            Options, Options, Down, Down, Confirm, Right, Right, Right, Right, Right, Right, Right, Confirm,
            Up, Left, Confirm, Sort,
        ];
        for step in steps {
            assert!(
                matches!(act(&mut menu, &mut ctx, step), Transition::None),
                "{step:?}"
            );
        }
        assert_eq!(menu.entry.as_ref().unwrap().text(), "Mk ");
        assert!(
            matches!(act(&mut menu, &mut ctx, Start), Transition::Pop),
            "saving closes the menu"
        );
        assert_eq!(fresh(&ctx, &game).title, "Mk", "trimmed");
        assert!(ctx.revision > revision, "screens reload");
        file_untouched(&ctx);
    }

    #[test]
    fn rename_by_typing_and_saving_with_enter() {
        let (_dir, mut ctx, game, mut menu) = renaming();
        menu.handle(Input::Text(TextInput::Backspace), &mut ctx);
        menu.handle(Input::Text(TextInput::Backspace), &mut ctx);
        type_in(&mut menu, &mut ctx, "  Pokémon Sapphire ");
        assert!(matches!(act(&mut menu, &mut ctx, Action::Start), Transition::Pop));
        assert_eq!(fresh(&ctx, &game).title, "Pokémon Sapphire");
        file_untouched(&ctx);
    }

    #[test]
    fn an_empty_name_or_the_cleaned_up_title_undoes_the_rename() {
        for typed in ["", "   ", " G0 "] {
            let (_dir, mut ctx, game, _) = renaming();
            ctx.library.rename(game.id, Some("Mine".into())).unwrap();
            let mut menu = OptionsMenu::new(fresh(&ctx, &game));
            act(&mut menu, &mut ctx, Action::Down);
            act(&mut menu, &mut ctx, Action::Confirm);
            for _ in 0.."Mine".len() {
                act(&mut menu, &mut ctx, Action::Options);
            }
            type_in(&mut menu, &mut ctx, typed);
            assert!(matches!(act(&mut menu, &mut ctx, Action::Start), Transition::Pop));
            assert_eq!(fresh(&ctx, &game).title, "G0", "{typed:?}");
            // Undone, not stored as "G0": a new cleaned-up title shows.
            let found = [FoundGame {
                system: "snes",
                rel_path: "snes/G0.sfc".into(),
                title: "G0 Deluxe".into(),
            }];
            ctx.library.sync_system("snes", &found, 0).unwrap();
            assert_eq!(fresh(&ctx, &game).title, "G0 Deluxe", "{typed:?}");
        }
    }

    #[test]
    fn cancelling_a_rename_changes_nothing() {
        let (_dir, mut ctx, game, mut menu) = renaming();
        let revision = ctx.revision;
        type_in(&mut menu, &mut ctx, "xyz");
        act(&mut menu, &mut ctx, Action::Confirm);
        assert!(matches!(act(&mut menu, &mut ctx, Action::Back), Transition::None));
        assert_eq!(menu.view(), View::List, "B goes back to the list");
        assert!(!menu.text_input());
        assert_eq!(menu.focused(), Item::Rename);
        assert!(matches!(
            act(&mut menu, &mut ctx, Action::Confirm),
            Transition::None
        ));
        assert_eq!(
            menu.entry.as_ref().unwrap().text(),
            "G0",
            "a new rename starts over"
        );
        assert!(matches!(
            menu.handle(Input::Pointer(Pointer::Back), &mut ctx),
            Transition::None
        ));
        assert!(matches!(act(&mut menu, &mut ctx, Action::Back), Transition::Pop));
        assert_eq!(fresh(&ctx, &game).title, "G0");
        assert_eq!(ctx.revision, revision);
        file_untouched(&ctx);
    }

    #[test]
    fn typed_qefs_and_space_reach_the_field_and_switch_no_tab() {
        use crate::ui::controls::{ControlEvent, Controls};
        use crate::ui::nav::{Flow, Hits, Nav};
        use sdl3::keyboard::Keycode;
        let (_dir, mut ctx, game, menu) = renaming();
        let mut nav = Nav::with_tabs(vec![
            ("Home", Box::new(OptionsMenu::new(game.clone()))),
            ("Consoles", Box::new(OptionsMenu::new(game.clone()))),
        ]);
        nav.push(Box::new(menu));
        let mut controls = Controls::new(InputKind::Keyboard);
        controls.text = nav.top().text_input();
        let mut inputs = Vec::new();
        for key in [Keycode::Q, Keycode::E, Keycode::F, Keycode::S, Keycode::Space] {
            controls.handle(0, ControlEvent::KeyDown { key, repeat: false }, &mut inputs);
            controls.handle(1, ControlEvent::KeyUp { key }, &mut inputs);
        }
        controls.typed("qefs ", &mut inputs);
        controls.handle(
            2,
            ControlEvent::KeyDown {
                key: Keycode::Return,
                repeat: false,
            },
            &mut inputs,
        );
        for input in inputs {
            assert_eq!(nav.handle(input, &Hits::default(), &mut ctx), Flow::Continue);
            assert_eq!(nav.active_tab(), 0, "{input:?} switched tabs");
        }
        assert_eq!(nav.depth(), 1, "Enter saved and closed the menu");
        assert_eq!(
            fresh(&ctx, &game).title,
            "G0qefs",
            "no Options, Sort or Confirm changed the text"
        );
    }
}
