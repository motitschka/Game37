//! Choose artwork (FR-ART-2, US-18): the other SteamGridDB covers and heroes for one Game, opened
//! from the Options Menu. There is no mockup of its own; it is the Options Menu's panel, wider, after
//! `screen_options` in `design/generate_mockups.py`: Covers and Heroes tabs over a grid of previews,
//! with the one the Game shows now marked "Current". Choosing one keeps it for good and goes back
//! to the Options Menu, which shows it; Back changes nothing. Without results it says why
//! (FR-ART-3).

use std::cell::Cell;
use std::sync::Arc;

use crate::artwork::{Art, Candidate, Candidates, Kind, Reason};
use crate::ctx::Ctx;
use crate::library::db::GameRow;
use crate::screens::message::Message;
use crate::systems;
use crate::ui::chrome::glyph;
use crate::ui::cover::{self, COVER_RADIUS, RING_REACH, ellipsize};
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer, glyph_label};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::scroll;
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, Colour, DIM, LINE, MUTED, SURFACE, SURFACE2, TEXT};
use crate::ui::{H, Rect, W, hit};

const PANEL_W: f32 = 1040.0;
const PANEL_X: f32 = W - PANEL_W;
const GRID_X: f32 = PANEL_X + 56.0;
const GRID_W: f32 = PANEL_W - 112.0;
/// The grid scrolls between these, inside the panel and above the credit line.
const GRID_TOP: f32 = 236.0;
const GRID_BOTTOM: f32 = 944.0;
const GAP: f32 = 24.0;
const TAB_Y: f32 = 196.0;
/// How far one wheel notch scrolls.
const WHEEL_STEP: f32 = 80.0;
/// A hero's shape: SteamGridDB's 1920×620.
const HERO_ASPECT: f32 = 620.0 / 1920.0;

const CHOOSE_FAILED_TITLE: &str = "Couldn't keep that artwork";

const TABS: [Kind; 2] = [Kind::Cover, Kind::Hero];

fn tab_index(kind: Kind) -> usize {
    match kind {
        Kind::Cover => 0,
        Kind::Hero => 1,
    }
}

fn tab_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Cover => "Covers",
        Kind::Hero => "Heroes",
    }
}

/// Previews per row: tall covers five abreast, wide heroes two.
fn cols(kind: Kind) -> usize {
    match kind {
        Kind::Cover => 5,
        Kind::Hero => 2,
    }
}

/// A preview's width and height.
fn cell(kind: Kind) -> (f32, f32) {
    let n = cols(kind) as f32;
    let w = (GRID_W - GAP * (n - 1.0)) / n;
    match kind {
        Kind::Cover => (w, w * 1.5),
        Kind::Hero => (w, w * HERO_ASPECT),
    }
}

fn pitch_y(kind: Kind) -> f32 {
    cell(kind).1 + GAP
}

/// The height of `n` previews' rows, with room for the focus ring above and below.
fn content_h(kind: Kind, n: usize) -> f32 {
    let rows = n.div_ceil(cols(kind)) as f32;
    (rows * pitch_y(kind) - GAP).max(0.0) + 2.0 * RING_REACH
}

const VIEW_H: f32 = GRID_BOTTOM - GRID_TOP;

/// What the picker says instead of a grid: a heading and a line under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub heading: String,
    pub body: String,
}

pub struct ArtworkPicker {
    game: GameRow,
    /// "<System> · <Display Title>"-style line under the heading.
    meta: String,
    tab: Kind,
    /// Per tab: the focused preview, the scroll offset, and whether focus was put on the current
    /// one yet.
    focus: [usize; 2],
    scroll: [f32; 2],
    placed: [bool; 2],
    /// Scroll the focused preview into view on the next draw.
    reveal: bool,
    /// Whether the open tab had previews to choose from, as of the last look, for the hints.
    choosable: bool,
    /// Whether the candidates were asked for yet: the first ask tries again lists that failed to
    /// arrive when the picker was open before.
    asked: Cell<bool>,
    cell_hits: Vec<(Rect, usize)>,
    tab_hits: Vec<(Rect, Kind)>,
}

impl ArtworkPicker {
    pub fn new(game: GameRow) -> ArtworkPicker {
        let short = systems::by_id(&game.system).map_or("?", |s| s.short);
        let meta = format!("{}  ·  {short}", game.title);
        ArtworkPicker {
            game,
            meta,
            tab: Kind::Cover,
            focus: [0; 2],
            scroll: [0.0; 2],
            placed: [false; 2],
            reveal: false,
            choosable: false,
            asked: Cell::new(false),
            cell_hits: Vec::new(),
            tab_hits: Vec::new(),
        }
    }

    /// The open tab.
    pub fn tab(&self) -> Kind {
        self.tab
    }

    pub fn focused(&self) -> usize {
        self.focus[tab_index(self.tab)]
    }

    /// The open tab's SteamGridDB results; asking queues the lookup the first time.
    pub fn candidates(&self, ctx: &Ctx) -> Candidates {
        if !self.asked.replace(true) {
            ctx.artwork.retry_failed(&self.game);
        }
        ctx.artwork.candidates(&ctx.library, &self.game, self.tab)
    }

    /// The index of the preview the Game shows now, in `list`.
    pub fn current(&self, ctx: &Ctx, list: &[Candidate]) -> Option<usize> {
        let id = ctx.artwork.chosen(&ctx.library, &self.game, self.tab)?;
        list.iter().position(|c| c.id == id)
    }

    /// The open tab's previews once they are there, with focus kept on one of them: the first
    /// time they arrive, on the current one.
    fn list(&mut self, ctx: &Ctx) -> Option<Arc<[Candidate]>> {
        let tab = tab_index(self.tab);
        let list = match self.candidates(ctx) {
            Candidates::Ready(list) if !list.is_empty() => list,
            _ => {
                self.choosable = false;
                return None;
            }
        };
        self.choosable = true;
        if !self.placed[tab] {
            self.placed[tab] = true;
            self.focus[tab] = self.current(ctx, &list).unwrap_or(0);
            self.reveal = true;
        }
        self.focus[tab] = self.focus[tab].min(list.len() - 1);
        Some(list)
    }

    /// What to say instead of a grid, if there are no previews to show.
    pub fn notice(&self, ctx: &Ctx) -> Option<Notice> {
        let notice = |heading: &str, body: &str| {
            Some(Notice {
                heading: heading.to_owned(),
                body: body.to_owned(),
            })
        };
        match self.candidates(ctx) {
            Candidates::Ready(list) if !list.is_empty() => None,
            Candidates::Ready(_) => notice(
                &format!(
                    "SteamGridDB has no {} for this Game",
                    tab_label(self.tab).to_lowercase()
                ),
                "Games without any show their Display Title and System instead.",
            ),
            Candidates::Loading => notice("Looking on SteamGridDB…", ""),
            Candidates::Unavailable(Reason::NoKey) => notice(
                "Choosing artwork needs a SteamGridDB API key",
                "Put it in ~/.config/game37/secrets.toml as steamgriddb_api_key = \"…\", \
                 then restart Game37. Until then, Games show their Display Title and System.",
            ),
            Candidates::Unavailable(Reason::NoMatch) => notice(
                &format!("SteamGridDB has no match for “{}”", self.game.title),
                "Renaming the Game to its official title searches again.",
            ),
            Candidates::Unavailable(Reason::Failed) => notice(
                "Couldn't reach SteamGridDB",
                "Check the connection and the API key, then open Choose artwork again to retry. \
                 If that still fails, restart Game37.",
            ),
        }
    }

    /// The preview under the pointer, among `n`, if it shows inside the grid.
    fn grid_hit(&self, x: f32, y: f32, n: usize) -> Option<usize> {
        if !(GRID_TOP..GRID_BOTTOM).contains(&y) {
            return None;
        }
        hit(&self.cell_hits, x, y).filter(|&i| i < n)
    }

    fn switch_tab(&mut self, kind: Kind) {
        self.tab = kind;
        self.reveal = true;
    }

    /// Moves focus in the grid of `n` previews.
    fn move_focus(&mut self, action: Action, n: usize) {
        let cols = cols(self.tab);
        let i = self.focused();
        let to = match action {
            Action::Left if !i.is_multiple_of(cols) => i - 1,
            Action::Right if i % cols + 1 < cols && i + 1 < n => i + 1,
            Action::Up if i >= cols => i - cols,
            Action::Down if i + cols < n => i + cols,
            // Down into a shorter last row lands on its last preview.
            Action::Down if (i / cols + 1) * cols < n => n - 1,
            _ => return,
        };
        self.focus[tab_index(self.tab)] = to;
        self.reveal = true;
    }

    /// Makes `list[i]` the Game's Artwork of its kind for good and goes back; if that can't be
    /// saved, says so over the picker.
    fn choose(&mut self, list: &[Candidate], i: usize, ctx: &mut Ctx) -> Transition {
        let Some(candidate) = list.get(i) else {
            return Transition::None;
        };
        if let Err(e) = ctx.artwork.choose(&ctx.library, &self.game, candidate) {
            eprintln!("game37: can't keep the chosen Artwork: {e}");
            return Transition::Push(Box::new(Message::new(
                CHOOSE_FAILED_TITLE,
                "Game37 couldn't save the choice in its library. The Game keeps the artwork it had.",
            )));
        }
        ctx.revision += 1;
        Transition::Pop
    }

    fn draw_tabs(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.tab_hits.clear();
        let mut x = GRID_X;
        x += glyph(
            gfx,
            x,
            TAB_Y,
            glyph_label(Action::PrevTab, ctx.input_kind, ctx.button_style),
        ) + 24.0;
        for kind in TABS {
            let on = kind == self.tab;
            let style = if on {
                TextStyle::new(22, Weight::Bold, TEXT)
            } else {
                TextStyle::new(22, Weight::Medium, MUTED)
            };
            let label = tab_label(kind);
            let w = gfx.measure(label, style);
            gfx.text(x, TAB_Y + 8.0, label, style, Align::Left);
            if on {
                gfx.fill_round_rect(Rect::new(x, TAB_Y + 22.0, w, 4.0), 2.0, ACCENT);
            }
            self.tab_hits
                .push((Rect::new(x - 16.0, TAB_Y - 26.0, w + 32.0, 56.0), kind));
            x += w + 40.0;
        }
        let next = glyph_label(Action::NextTab, ctx.input_kind, ctx.button_style);
        glyph(gfx, x - 16.0, TAB_Y, next);
    }

    fn draw_grid(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx, list: &[Candidate]) {
        let kind = self.tab;
        let tab = tab_index(kind);
        let (cw, ch) = cell(kind);
        let pitch = pitch_y(kind);
        let cols = cols(kind);
        let focus = self.focus[tab];
        if std::mem::take(&mut self.reveal) {
            let row = (focus / cols) as f32;
            let s = scroll::keep_visible(self.scroll[tab], row * pitch, ch + 2.0 * RING_REACH, VIEW_H);
            self.scroll[tab] = s;
        }
        self.scroll[tab] = scroll::clamp(self.scroll[tab], content_h(kind, list.len()), VIEW_H);
        let offset = self.scroll[tab];
        let current = self.current(ctx, list);
        gfx.clip(Some(Rect::new(PANEL_X + 1.0, GRID_TOP, PANEL_W - 1.0, VIEW_H)));
        let first_row = (offset / pitch).floor() as usize;
        let last_row = ((offset + VIEW_H) / pitch).ceil() as usize;
        let shown = list
            .iter()
            .enumerate()
            .take((last_row + 1) * cols)
            .skip(first_row * cols);
        for (i, candidate) in shown {
            let (row, col) = (i / cols, i % cols);
            let r = Rect::new(
                GRID_X + col as f32 * (cw + GAP),
                GRID_TOP + RING_REACH + row as f32 * pitch - offset,
                cw,
                ch,
            );
            if i == focus {
                cover::focus_ring(gfx, r, COVER_RADIUS);
            }
            match ctx.artwork.preview(candidate) {
                Art::Ready(picture) => gfx.draw_picture(&picture, r, COVER_RADIUS),
                Art::Loading => gfx.fill_round_rect(r, COVER_RADIUS, SURFACE2),
                Art::Placeholder => {
                    gfx.fill_round_rect(r, COVER_RADIUS, SURFACE2);
                    gfx.stroke_round_rect(r, COVER_RADIUS, 1.0, LINE);
                    let style = TextStyle::new(16, Weight::Medium, DIM);
                    gfx.text(
                        r.x + r.w / 2.0,
                        r.y + r.h / 2.0 + 6.0,
                        "No preview",
                        style,
                        Align::Centre,
                    );
                }
            }
            if current == Some(i) {
                badge(gfx, r);
            }
            self.cell_hits.push((r, i));
        }
        gfx.clip(None);

        let candidate = &list[focus];
        let size = format!("{}×{}", candidate.width, candidate.height);
        let credit = match &candidate.author {
            Some(author) => format!("By {author}  ·  {size}"),
            None => size,
        };
        let style = TextStyle::new(17, Weight::Medium, MUTED);
        let credit = ellipsize(&credit, GRID_W, |s| gfx.measure(s, style));
        gfx.text(GRID_X, GRID_BOTTOM + 40.0, &credit, style, Align::Left);
    }

    fn draw_notice(&mut self, gfx: &mut Gfx<'_>, notice: &Notice) {
        let heading = TextStyle::new(24, Weight::Bold, TEXT);
        let body = TextStyle::new(18, Weight::Medium, MUTED);
        let cx = PANEL_X + PANEL_W / 2.0;
        let lines = gfx.wrap(&notice.body, body, GRID_W - 120.0);
        let top = GRID_TOP + VIEW_H / 2.0 - 30.0 - lines.len() as f32 * 14.0;
        let title = ellipsize(&notice.heading, GRID_W, |s| gfx.measure(s, heading));
        gfx.text(cx, top, &title, heading, Align::Centre);
        for (i, line) in lines.iter().enumerate() {
            gfx.text(cx, top + 44.0 + i as f32 * 28.0, line, body, Align::Centre);
        }
    }
}

/// The "Current" mark on the preview the Game shows now.
fn badge(gfx: &mut Gfx<'_>, r: Rect) {
    let style = TextStyle::new(13, Weight::Bold, TEXT);
    let w = gfx.measure("Current", style) + 20.0;
    gfx.fill_round_rect(Rect::new(r.x + 10.0, r.y + 10.0, w, 24.0), 12.0, ACCENT);
    gfx.text(r.x + 20.0, r.y + 27.0, "Current", style, Align::Left);
}

impl Screen for ArtworkPicker {
    fn title(&self) -> &str {
        "Choose artwork"
    }

    fn is_modal(&self) -> bool {
        true
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        let list = self.list(ctx);
        let n = list.as_ref().map_or(0, |l| l.len());
        match input {
            Input::Action(Action::Back) | Input::Pointer(Pointer::Back) => return Transition::Pop,
            Input::Action(Action::PrevTab | Action::NextTab) => {
                let other = TABS[1 - tab_index(self.tab)];
                self.switch_tab(other);
            }
            Input::Action(a @ (Action::Up | Action::Down | Action::Left | Action::Right)) => {
                self.move_focus(a, n)
            }
            Input::Action(Action::Confirm) => {
                if let Some(list) = list {
                    return self.choose(&list, self.focused(), ctx);
                }
            }
            Input::Pointer(Pointer::Click { x, .. }) if x < PANEL_X => return Transition::Pop,
            Input::Pointer(Pointer::Click { x, y }) => {
                if let Some(kind) = hit(&self.tab_hits, x, y) {
                    self.switch_tab(kind);
                } else if let (Some(i), Some(list)) = (self.grid_hit(x, y, n), list) {
                    return self.choose(&list, i, ctx);
                }
            }
            Input::Pointer(Pointer::Move { x, y }) => {
                if let Some(i) = self.grid_hit(x, y, n) {
                    self.focus[tab_index(self.tab)] = i;
                }
            }
            Input::Pointer(Pointer::Wheel { dy }) => {
                let tab = tab_index(self.tab);
                let s = self.scroll[tab] - dy * WHEEL_STEP;
                self.scroll[tab] = scroll::clamp(s, content_h(self.tab, n), VIEW_H);
            }
            _ => {}
        }
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.cell_hits.clear();
        gfx.fill_rect(Rect::new(0.0, 0.0, W, H), Colour::hex(0x000000).alpha(0.62));
        gfx.fill_rect(Rect::new(PANEL_X, 0.0, PANEL_W, H), SURFACE);
        gfx.fill_rect(Rect::new(PANEL_X, 0.0, 1.0, H), LINE);
        let heading = TextStyle::new(30, Weight::ExtraBold, TEXT);
        gfx.text(GRID_X, 104.0, "Choose artwork", heading, Align::Left);
        let meta = TextStyle::new(18, Weight::Medium, MUTED);
        let label = ellipsize(&self.meta, GRID_W, |s| gfx.measure(s, meta));
        gfx.text(GRID_X, 138.0, &label, meta, Align::Left);
        self.draw_tabs(gfx, ctx);
        gfx.fill_rect(Rect::new(GRID_X, TAB_Y + 30.0, GRID_W, 1.0), LINE);
        match self.list(ctx) {
            Some(list) => self.draw_grid(gfx, ctx, &list),
            None => {
                if let Some(notice) = self.notice(ctx) {
                    self.draw_notice(gfx, &notice);
                }
            }
        }
    }

    fn hints(&self) -> Vec<Hint> {
        let mut hints = Vec::new();
        if self.choosable {
            hints.push(Hint::new(&[Action::Confirm], "Choose"));
        }
        hints.push(Hint::new(&[Action::PrevTab, Action::NextTab], "Covers / Heroes"));
        hints.push(Hint::new(&[Action::Back], "Back"));
        hints
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artwork::tests::{Fake, settle, steamgriddb, unpaced};
    use crate::artwork::{Artwork, FetchError};
    use crate::config::ApiKey;
    use crate::library::FoundGame;
    use crate::library::db::{Library, SortOrder};
    use std::path::Path;

    /// A Ctx over a library file in `dir` with the SNES Games "Alpha" and "Beta" (in that order),
    /// its Artwork fetched through `fake`, with an API key if `key`.
    fn world(dir: &Path, fake: &Fake, key: bool) -> (Ctx, Vec<GameRow>) {
        let mut library = Library::open(&dir.join("library.db")).unwrap();
        let found: Vec<_> = ["Alpha", "Beta"]
            .iter()
            .map(|title| FoundGame {
                system: "snes",
                rel_path: format!("snes/{title}.sfc"),
                title: (*title).to_owned(),
            })
            .collect();
        library.sync_system("snes", &found, 0).unwrap();
        let games = library.system_games("snes", SortOrder::AZ).unwrap();
        let mut ctx = Ctx::new(library, dir.join("Games"));
        let key = key.then(|| ApiKey::new("0123456789abcdef0123456789abcdef"));
        ctx.artwork = unpaced(Artwork::new(dir.join("artwork"), key, fake.fetch()));
        (ctx, games)
    }

    /// A Game that is in no library.
    fn alpha() -> GameRow {
        GameRow {
            id: 1,
            system: "snes".into(),
            rel_path: "snes/Alpha.sfc".into(),
            title: "Alpha".into(),
            date_added: 0,
            last_played: None,
            playtime_secs: 0,
            favorite: false,
        }
    }

    /// The picker's open tab once its lookup has arrived.
    fn settled(picker: &ArtworkPicker, ctx: &Ctx) -> Candidates {
        let ask = || picker.candidates(ctx);
        settle(&ctx.artwork, &ctx.library, ask, |c| {
            matches!(c, Candidates::Loading)
        })
    }

    fn listed(picker: &ArtworkPicker, ctx: &Ctx) -> Arc<[Candidate]> {
        match settled(picker, ctx) {
            Candidates::Ready(list) => list,
            other => panic!("no candidates: {other:?}"),
        }
    }

    fn ids(picker: &ArtworkPicker, ctx: &Ctx) -> Vec<u64> {
        listed(picker, ctx).iter().map(|c| c.id).collect()
    }

    fn act(picker: &mut ArtworkPicker, ctx: &mut Ctx, action: Action) -> Transition {
        picker.handle(Input::Action(action), ctx)
    }

    /// Waits until `game`'s own cover has been looked up: for Alpha, SteamGridDB's top cover (7)
    /// is then current.
    fn cover_shown(ctx: &Ctx, game: &GameRow) -> Art {
        let ask = || ctx.artwork.cover(&ctx.library, game);
        settle(&ctx.artwork, &ctx.library, ask, |a| matches!(a, Art::Loading))
    }

    #[test]
    fn picker_lists_covers_and_heroes_and_the_tabs_switch() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        let (mut ctx, games) = world(dir.path(), &fake, true);
        let mut picker = ArtworkPicker::new(games[0].clone());
        assert_eq!(picker.tab(), Kind::Cover);
        assert_eq!(ids(&picker, &ctx), [7, 8]);
        assert_eq!(picker.notice(&ctx), None);

        act(&mut picker, &mut ctx, Action::NextTab);
        assert_eq!(picker.tab(), Kind::Hero, "R1 / E");
        assert_eq!(ids(&picker, &ctx), [9]);
        act(&mut picker, &mut ctx, Action::NextTab);
        assert_eq!(picker.tab(), Kind::Cover, "and round again");
        act(&mut picker, &mut ctx, Action::PrevTab);
        assert_eq!(picker.tab(), Kind::Hero, "L1 / Q");

        picker.tab_hits = vec![
            (Rect::new(1000.0, 170.0, 100.0, 50.0), Kind::Cover),
            (Rect::new(1140.0, 170.0, 100.0, 50.0), Kind::Hero),
        ];
        let click = Input::Pointer(Pointer::Click { x: 1010.0, y: 180.0 });
        assert!(matches!(picker.handle(click, &mut ctx), Transition::None));
        assert_eq!(picker.tab(), Kind::Cover, "a click on a tab");
    }

    #[test]
    fn picker_marks_the_current_one_and_starts_on_it() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        let (mut ctx, games) = world(dir.path(), &fake, true);
        let alpha = &games[0];
        cover_shown(&ctx, alpha);
        let list = listed(&ArtworkPicker::new(alpha.clone()), &ctx);
        let picker = ArtworkPicker::new(alpha.clone());
        assert_eq!(
            picker.current(&ctx, &list),
            Some(0),
            "the top one, picked automatically"
        );

        ctx.artwork.choose(&ctx.library, alpha, &list[1]).unwrap();
        let mut picker = ArtworkPicker::new(alpha.clone());
        assert_eq!(picker.current(&ctx, &list), Some(1));
        act(&mut picker, &mut ctx, Action::Up);
        assert_eq!(picker.focused(), 1, "focus starts on the current one");
    }

    #[test]
    fn picker_choice_persists_across_a_fresh_service_and_goes_back() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        let (mut ctx, games) = world(dir.path(), &fake, true);
        let alpha = games[0].clone();
        cover_shown(&ctx, &alpha);
        let mut picker = ArtworkPicker::new(alpha.clone());
        assert_eq!(ids(&picker, &ctx), [7, 8]);
        act(&mut picker, &mut ctx, Action::Right);
        assert_eq!(picker.focused(), 1);
        assert_eq!(picker.hints()[0], Hint::new(&[Action::Confirm], "Choose"));
        let revision = ctx.revision;
        assert!(matches!(
            act(&mut picker, &mut ctx, Action::Confirm),
            Transition::Pop
        ));
        assert!(ctx.revision > revision, "screens reload");
        assert_eq!(ctx.artwork.chosen(&ctx.library, &alpha, Kind::Cover), Some(8));
        drop(ctx);

        let fake = steamgriddb();
        let (ctx, games) = world(dir.path(), &fake, true);
        assert_eq!(ctx.artwork.chosen(&ctx.library, &games[0], Kind::Cover), Some(8));
        match cover_shown(&ctx, &games[0]) {
            Art::Ready(picture) => assert_eq!((picture.width, picture.height), (342, 482)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn picker_choice_that_cannot_be_saved_says_so_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        let (mut ctx, games) = world(dir.path(), &fake, true);
        let alpha = games[0].clone();
        cover_shown(&ctx, &alpha);
        let before = ctx.library.artwork(alpha.id).unwrap();
        let mut picker = ArtworkPicker::new(alpha.clone());
        assert_eq!(ids(&picker, &ctx), [7, 8]);
        // Every save of a record fails.
        rusqlite::Connection::open(dir.path().join("library.db"))
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER no_artwork BEFORE UPDATE OF artwork ON games \
                 BEGIN SELECT RAISE(ABORT, 'read-only'); END;",
            )
            .unwrap();
        act(&mut picker, &mut ctx, Action::Right);
        let revision = ctx.revision;
        match act(&mut picker, &mut ctx, Action::Confirm) {
            Transition::Push(message) => assert_eq!(message.title(), CHOOSE_FAILED_TITLE),
            _ => panic!("expected a Message"),
        }
        assert_eq!(ctx.revision, revision, "nothing to reload");
        assert_eq!(ctx.library.artwork(alpha.id).unwrap(), before);
        assert_eq!(ctx.artwork.chosen(&ctx.library, &alpha, Kind::Cover), Some(7));
    }

    #[test]
    fn picker_click_on_a_preview_chooses_it() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        let (mut ctx, games) = world(dir.path(), &fake, true);
        let mut picker = ArtworkPicker::new(games[0].clone());
        act(&mut picker, &mut ctx, Action::NextTab);
        assert_eq!(ids(&picker, &ctx), [9]);
        picker.cell_hits = vec![(Rect::new(GRID_X, 300.0, 400.0, 700.0), 0)];
        let below = Input::Pointer(Pointer::Click {
            x: GRID_X + 10.0,
            y: GRID_BOTTOM + 5.0,
        });
        assert!(
            matches!(picker.handle(below, &mut ctx), Transition::None),
            "scrolled out of the grid"
        );
        let at = Input::Pointer(Pointer::Click {
            x: GRID_X + 10.0,
            y: 310.0,
        });
        assert!(matches!(picker.handle(at, &mut ctx), Transition::Pop));
        assert_eq!(ctx.artwork.chosen(&ctx.library, &games[0], Kind::Hero), Some(9));
        assert_eq!(ctx.artwork.chosen(&ctx.library, &games[0], Kind::Cover), None);
    }

    #[test]
    fn picker_back_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        let (mut ctx, games) = world(dir.path(), &fake, true);
        let alpha = &games[0];
        cover_shown(&ctx, alpha);
        let before = ctx.library.artwork(alpha.id).unwrap();
        let revision = ctx.revision;
        for back in [
            Input::Action(Action::Back),
            Input::Pointer(Pointer::Back),
            Input::Pointer(Pointer::Click {
                x: PANEL_X - 1.0,
                y: 500.0,
            }),
        ] {
            let mut picker = ArtworkPicker::new(alpha.clone());
            ids(&picker, &ctx);
            act(&mut picker, &mut ctx, Action::Right);
            assert!(
                matches!(picker.handle(back, &mut ctx), Transition::Pop),
                "{back:?}"
            );
        }
        assert_eq!(ctx.revision, revision);
        assert_eq!(ctx.library.artwork(alpha.id).unwrap(), before);
        assert_eq!(ctx.artwork.chosen(&ctx.library, alpha, Kind::Cover), Some(7));
    }

    #[test]
    fn picker_focus_moves_in_the_grid_and_stops_at_its_edges() {
        let mut picker = ArtworkPicker::new(alpha());
        // Twelve covers: rows of five, five and two.
        let mut moves = |action, want| {
            picker.move_focus(action, 12);
            assert_eq!(picker.focused(), want, "{action:?}");
        };
        moves(Action::Left, 0);
        moves(Action::Up, 0);
        for want in 1..5 {
            moves(Action::Right, want);
        }
        moves(Action::Right, 4);
        moves(Action::Down, 9);
        moves(Action::Down, 11);
        moves(Action::Down, 11);
        moves(Action::Up, 6);
        moves(Action::Left, 5);
        moves(Action::Left, 5);
    }

    /// The notice the picker shows for Alpha's covers through `fake` once settled, how many
    /// requests that made, and the hints.
    fn notice_for(fake: &Fake, key: bool) -> (Notice, usize, Vec<Hint>) {
        let dir = tempfile::tempdir().unwrap();
        let (mut ctx, games) = world(dir.path(), fake, key);
        let mut picker = ArtworkPicker::new(games[0].clone());
        settled(&picker, &ctx);
        assert!(matches!(
            act(&mut picker, &mut ctx, Action::Confirm),
            Transition::None
        ));
        let notice = picker.notice(&ctx).expect("a notice");
        (notice, fake.calls(), picker.hints())
    }

    #[test]
    fn picker_reopened_after_a_list_failed_to_arrive_asks_again() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        let search = "https://www.steamgriddb.com/api/v2/search/";
        fake.route_first(search, Err(FetchError::Connect));
        let (ctx, games) = world(dir.path(), &fake, true);
        let picker = ArtworkPicker::new(games[0].clone());
        assert!(matches!(
            settled(&picker, &ctx),
            Candidates::Unavailable(Reason::Failed)
        ));

        // Back online: the open picker keeps its answer, a reopened one asks again.
        fake.unroute(search);
        let calls = fake.calls();
        assert!(matches!(
            picker.candidates(&ctx),
            Candidates::Unavailable(Reason::Failed)
        ));
        assert_eq!(fake.calls(), calls);
        let reopened = ArtworkPicker::new(games[0].clone());
        assert_eq!(ids(&reopened, &ctx), [7, 8]);
    }

    #[test]
    fn picker_says_why_there_is_nothing_to_choose() {
        let (no_key, calls, hints) = notice_for(&steamgriddb(), false);
        assert!(no_key.heading.contains("SteamGridDB API key"), "{no_key:?}");
        assert!(
            no_key.body.contains("~/.config/game37/secrets.toml"),
            "{no_key:?}"
        );
        assert_eq!(calls, 0, "no key, no requests");
        assert!(!hints.iter().any(|h| h.text == "Choose"), "nothing to choose");
        let off = Ctx::empty();
        assert_eq!(
            ArtworkPicker::new(alpha()).notice(&off),
            Some(no_key),
            "Artwork off says the same"
        );

        let fake = Fake::new();
        fake.api("/search/autocomplete/Alpha", "[]");
        let (no_match, ..) = notice_for(&fake, true);
        assert_eq!(no_match.heading, "SteamGridDB has no match for “Alpha”");

        let fake = Fake::new();
        fake.route("https://www.steamgriddb.com/", Err(FetchError::Connect));
        let (failed, ..) = notice_for(&fake, true);
        assert_eq!(failed.heading, "Couldn't reach SteamGridDB");
        assert!(
            failed.body.contains("open Choose artwork again") && failed.body.contains("restart Game37"),
            "reopening tries again; once requests have stopped for the run, a restart does: {failed:?}"
        );

        let dir = tempfile::tempdir().unwrap();
        let (mut ctx, games) = world(dir.path(), &steamgriddb(), true);
        let mut picker = ArtworkPicker::new(games[1].clone());
        act(&mut picker, &mut ctx, Action::NextTab);
        assert!(matches!(settled(&picker, &ctx), Candidates::Ready(list) if list.is_empty()));
        let none = picker.notice(&ctx).unwrap();
        assert_eq!(
            none.heading, "SteamGridDB has no heroes for this Game",
            "Beta has none"
        );
    }
}
