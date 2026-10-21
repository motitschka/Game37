//! The Options Menu for one Game (FR-DET-3, FR-DET-4), after `screen_options` in
//! `design/generate_mockups.py`: a panel on the right over the dimmed screen. With five Favorites
//! already, adding a sixth shows a picker of them in the same panel (FR-HOME-6, US-6).

use crate::ctx::{self, Ctx};
use crate::library::db::{GameRow, Shelf};
use crate::systems::{self, Kind};
use crate::ui::cover::{self, ellipsize, playtime_label};
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
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
    /// Rename the Display Title. Shown but not selectable until it is built.
    Rename,
    Hide,
    ShowFile,
}

impl Item {
    /// Whether the action is built yet; the others are greyed and skipped by focus.
    pub fn enabled(self) -> bool {
        !matches!(self, Item::LoadSaveState | Item::ChooseArtwork | Item::Rename)
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
            Item::LoadSaveState | Item::ChooseArtwork | Item::Rename => return Transition::None,
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

    fn back_to_list(&mut self) {
        self.view = View::List;
        self.favorites.clear();
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

    fn draw(&mut self, gfx: &mut Gfx<'_>, _: &Ctx) {
        gfx.fill_rect(Rect::new(0.0, 0.0, W, H), Colour::hex(0x000000).alpha(0.62));
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
        }
    }
}
