//! A Game's Details Page (FR-DET-1, FR-DET-2): its hero, record and file, with Play and Options,
//! after `details_page` in `design/generate_mockups.py`.
//!
//! Until Artwork arrives (FR-ART-3), the hero is placeholder art in the System's colour, like the
//! cover. Options and Y open the Game's Options Menu; once the Game is hidden or gone from its
//! folder, the page leaves.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use walkdir::WalkDir;

use super::home::tilde;
use super::options::OptionsMenu;
use crate::artwork::Art;
use crate::cores;
use crate::ctx::Ctx;
use crate::emulators::{self, Emulator, InstallHint};
use crate::launch;
use crate::library::db::GameRow;
use crate::session::sdl::{local_time, now_local};
use crate::session::states::{Local, SLOTS, StatePaths, days_ago};
use crate::systems::{self, Kind, System};
use crate::ui::cover::{self, ellipsize, playtime_label};
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer, glyph_label};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{BG, Colour, DIM, LINE, MUTED, STAR, SURFACE, SURFACE2, TEXT, WARN};
use crate::ui::{H, M, Rect, W, chrome, hit};

/// Left edge of everything beside the cover.
const X: f32 = 560.0;
const COVER: Rect = Rect::new(M, 176.0, 380.0, 570.0);
const STAT_Y: f32 = 410.0;
const STAT_H: f32 = 104.0;
const STAT_GAP: f32 = 20.0;
/// Stat box widths: four boxes as in the mockup; five (an Embedded System's, with Save States)
/// narrowed to fit before the right margin.
const STAT_W: f32 = 250.0;
const STAT_W_NARROW: f32 = 232.0;
const PLAY: Rect = Rect::new(X, 604.0, 270.0, 76.0);
const OPTIONS: Rect = Rect::new(X + 290.0, 604.0, 236.0, 76.0);
const BUTTON_RADIUS: f32 = 38.0;
const WARN_Y: f32 = 738.0;

const WHITE: Colour = Colour::hex(0xFFFFFF);
const BLACK: Colour = Colour::hex(0x000000);

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Play,
    Options,
}

/// What measuring the Game on disk found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measured {
    pub bytes: u64,
    /// A folder Game (PS3, PS4, Wii U), whose size is the sum of its files.
    pub folder: bool,
}

/// The Game's size: measured on a thread, since a folder Game can hold thousands of files (NFR-3).
enum Size {
    Pending(Receiver<Option<Measured>>),
    /// `None` when the Game's file or folder can't be read.
    Known(Option<Measured>),
}

/// Measures the file or folder at `path`; `None` if it isn't there.
pub fn measure(path: &Path) -> Option<Measured> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_dir() {
        return Some(Measured {
            bytes: meta.len(),
            folder: false,
        });
    }
    let bytes = WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok()?.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum();
    Some(Measured { bytes, folder: true })
}

/// Measures `path` on its own thread; the receiver gets the result.
fn measure_in_background(path: PathBuf) -> Receiver<Option<Measured>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(measure(&path));
    });
    rx
}

/// A size as "512 KB", "1.36 GB" or "22.4 GB": three significant digits, in units of 1024.
pub fn size_label(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let digits = match value {
        v if v < 10.0 => 2,
        v if v < 100.0 => 1,
        _ => 0,
    };
    format!("{value:.digits$} {}", UNITS[unit])
}

/// A date as "3 Oct 2026".
pub fn date_label(date: Local) -> String {
    let month = MONTHS[(date.month.clamp(1, 12) - 1) as usize];
    format!("{} {month} {}", date.day, date.year)
}

/// A day relative to `now`: "Today", "Yesterday", else as [`date_label`].
pub fn day_label(then: Local, now: Local) -> String {
    match days_ago(then, now) {
        0 => "Today".to_owned(),
        1 => "Yesterday".to_owned(),
        _ => date_label(then),
    }
}

/// Unix seconds in local time.
fn local(secs: i64) -> Option<Local> {
    local_time(secs.saturating_mul(1_000_000_000))
}

/// How many of the Game's Save State slots are filled; `None` for an External System's Game,
/// whose Standalone Emulator keeps its own.
fn save_state_count(games_root: &Path, game: &GameRow, system: Option<&System>) -> Option<usize> {
    if system?.kind != Kind::Embedded {
        return None;
    }
    let paths = StatePaths::new(games_root, &game.system, Path::new(&game.rel_path));
    Some((1..=SLOTS).filter(|&n| paths.slot(n).is_file()).count())
}

/// What runs the Game: its Standalone Emulator, or Game37 itself on a Core.
fn opens_in(system: &System) -> Option<String> {
    match system.kind {
        Kind::External => emulators::for_system(system).map(|e| format!("Opens in {}", e.name)),
        Kind::Embedded => Some(match cores::for_system(system) {
            Some(core) => format!("Opens in Game37 · {} core", core.name),
            None => "Opens in Game37".to_owned(),
        }),
    }
}

/// The line under the buttons when Play is off (FR-CAR-9): on Windows, when the emulator has no
/// winget package, there's no install command to point at, only the Console Page's Choose .exe
/// and download page (FR-WIN-5).
fn missing_line(emulator: &Emulator, system: &System) -> String {
    let promise = if cfg!(windows) && matches!(emulator.windows_install_hint(), InstallHint::Page(_)) {
        format!(
            "Choose its program, or get it, on the {} Console Page.",
            system.name
        )
    } else {
        format!("The install command is on the {} Console Page.", system.name)
    };
    format!("{} isn't installed, so Play is off. {promise}", emulator.name)
}

pub struct DetailsPage {
    game: GameRow,
    system: Option<&'static System>,
    /// The `Ctx::revision` the Game was last read at, so Playtime and last played refresh after
    /// a play, which pops back here.
    revision: u64,
    /// The Game was hidden or left its folder: the page pops on its next tick or input.
    gone: bool,
    last_played: String,
    added: String,
    save_states: Option<usize>,
    /// The Game's file or folder, as "~/Games/…".
    path: String,
    size: Size,
    /// The Standalone Emulator, if it isn't installed: Play is then off (FR-CAR-9).
    missing: Option<&'static Emulator>,
    focus: Button,
}

impl DetailsPage {
    pub fn new(game: GameRow, ctx: &Ctx) -> DetailsPage {
        let size = measure_in_background(ctx.games_root.join(&game.rel_path));
        DetailsPage::with_size(game, ctx, size)
    }

    /// The page with its size coming from `size`.
    fn with_size(game: GameRow, ctx: &Ctx, size: Receiver<Option<Measured>>) -> DetailsPage {
        let path = tilde(&ctx.games_root.join(&game.rel_path), dirs::home_dir().as_deref());
        let mut page = DetailsPage {
            system: systems::by_id(&game.system),
            game,
            revision: ctx.revision,
            gone: false,
            last_played: String::new(),
            added: String::new(),
            save_states: None,
            path,
            size: Size::Pending(size),
            missing: None,
            focus: Button::Play,
        };
        page.load_record(ctx);
        page.sync_missing(ctx);
        page
    }

    /// Re-reads the Game if the library changed since, e.g. after it was played.
    fn reload_if_changed(&mut self, ctx: &Ctx) {
        if self.revision == ctx.revision {
            return;
        }
        self.revision = ctx.revision;
        match ctx.library.game(self.game.id) {
            Ok(Some(game)) => self.game = game,
            // Hidden or gone from its folder: the page draws what it last knew until it pops.
            Ok(None) => self.gone = true,
            Err(e) => eprintln!("game37: {e}"),
        }
        self.load_record(ctx);
    }

    fn load_record(&mut self, ctx: &Ctx) {
        self.last_played = match self.game.last_played.map(local) {
            None => "Never".to_owned(),
            Some(then) => match (then, now_local()) {
                (Some(then), Some(now)) => day_label(then, now),
                (Some(then), None) => date_label(then),
                (None, _) => String::new(),
            },
        };
        self.added = local(self.game.date_added).map_or_else(String::new, date_label);
        self.save_states = save_state_count(&ctx.games_root, &self.game, self.system);
    }

    /// Re-checks the Standalone Emulator, cheap enough for every call; while it is missing, focus
    /// stays on Options (FR-CAR-9).
    fn sync_missing(&mut self, ctx: &Ctx) {
        self.missing = self.system.and_then(|s| ctx.emulators.missing(s));
        if self.missing.is_some() {
            self.focus = Button::Options;
        }
    }

    fn poll_size(&mut self) {
        if let Size::Pending(rx) = &self.size {
            match rx.try_recv() {
                Ok(measured) => self.size = Size::Known(measured),
                Err(TryRecvError::Disconnected) => self.size = Size::Known(None),
                Err(TryRecvError::Empty) => {}
            }
        }
    }

    /// The stat boxes' labels and values: Save States only for an Embedded System's Game.
    fn stats(&self) -> Vec<(&'static str, String)> {
        let playtime = match self.game.playtime_secs {
            0 => "—".to_owned(),
            secs => playtime_label(secs),
        };
        let mut stats = vec![("PLAYTIME", playtime), ("LAST PLAYED", self.last_played.clone())];
        if let Some(n) = self.save_states {
            stats.push(("SAVE STATES", format!("{n} of {SLOTS}")));
        }
        stats.push(("ADDED", self.added.clone()));
        let size = match &self.size {
            Size::Pending(_) => "…".to_owned(),
            Size::Known(None) => "—".to_owned(),
            Size::Known(Some(m)) => size_label(m.bytes),
        };
        stats.push(("SIZE", size));
        stats
    }

    /// The button under a point; Play only while it is on.
    fn button_at(&self, x: f32, y: f32) -> Option<Button> {
        let buttons = [(PLAY, Button::Play), (OPTIONS, Button::Options)];
        hit(&buttons, x, y).filter(|&b| b != Button::Play || self.missing.is_none())
    }

    fn press(&self, button: Button, ctx: &mut Ctx) -> Transition {
        match button {
            Button::Play if self.missing.is_none() => launch::confirm_game(&self.game, ctx),
            Button::Play => Transition::None,
            Button::Options => self.options(ctx),
        }
    }

    fn options(&self, ctx: &Ctx) -> Transition {
        Transition::Push(Box::new(OptionsMenu::new(self.game.clone(), &ctx.games_root)))
    }

    fn draw_play(&self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        let on = self.missing.is_none();
        let ink = if on { BG } else { DIM };
        if on {
            gfx.fill_round_rect(PLAY, BUTTON_RADIUS, WHITE);
        } else {
            gfx.fill_round_rect(PLAY, BUTTON_RADIUS, SURFACE2.alpha(0.6));
            gfx.stroke_round_rect(PLAY, BUTTON_RADIUS, 2.0, LINE);
        }
        let cy = PLAY.y + PLAY.h / 2.0;
        let label = glyph_label(Action::Confirm, ctx.input_kind, ctx.button_style);
        let w = button_glyph(gfx, PLAY.x + 35.0, cy, label, on);
        gfx.text(
            PLAY.x + 35.0 + w + 17.0,
            cy + 10.0,
            "Play",
            TextStyle::new(28, Weight::ExtraBold, ink),
            Align::Left,
        );
    }

    fn draw_options(&self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        gfx.fill_round_rect(OPTIONS, BUTTON_RADIUS, SURFACE.alpha(0.85));
        gfx.stroke_round_rect(OPTIONS, BUTTON_RADIUS, 2.0, LINE);
        let cy = OPTIONS.y + OPTIONS.h / 2.0;
        let label = glyph_label(Action::Options, ctx.input_kind, ctx.button_style);
        let w = chrome::glyph(gfx, OPTIONS.x + 32.0, cy, label);
        gfx.text(
            OPTIONS.x + 32.0 + w + 14.0,
            cy + 9.0,
            "Options",
            TextStyle::new(24, Weight::Bold, TEXT),
            Align::Left,
        );
    }
}

/// The hero behind the page (FR-DET-2): the Game's hero Artwork `art` filling the screen, or while
/// there is none a gradient in the System's colour with soft shapes. Either is darkened towards the
/// text on the left and the bottom.
fn draw_hero(gfx: &mut Gfx<'_>, colour: Colour, art: &Art) {
    let screen = Rect::new(0.0, 0.0, W, H);
    match art {
        Art::Ready(picture) => gfx.draw_picture(picture, screen, 0.0),
        Art::Loading | Art::Placeholder => {
            gfx.fill_round_rect_gradient(screen, 0.0, colour, cover::shade(colour), (1.0, 1.0));
            gfx.fill_circle(1500.0, 330.0, 420.0, WHITE.alpha(0.08));
            gfx.fill_circle(1500.0, 330.0, 230.0, WHITE.alpha(0.08));
            gfx.fill_circle(1150.0, 900.0, 300.0, BLACK.alpha(0.15));
        }
    }
    let mid = 0.55 * W;
    gfx.fill_round_rect_gradient(
        Rect::new(0.0, 0.0, mid, H),
        0.0,
        BG.alpha(0.97),
        BG.alpha(0.82),
        (1.0, 0.0),
    );
    gfx.fill_round_rect_gradient(
        Rect::new(mid, 0.0, W - mid, H),
        0.0,
        BG.alpha(0.82),
        BG.alpha(0.35),
        (1.0, 0.0),
    );
    gfx.fill_round_rect_vgradient(Rect::new(0.0, 640.0, W, H - 640.0), 0.0, BG.alpha(0.0), BG);
}

/// The Confirm glyph inside the Play button: dark on white, or an outline when Play is off.
/// Returns its width.
fn button_glyph(gfx: &mut Gfx<'_>, x: f32, cy: f32, label: &str, on: bool) -> f32 {
    let (ink, letter) = if on { (BG, WHITE) } else { (DIM, DIM) };
    let style = TextStyle::new(17, Weight::ExtraBold, letter);
    if chrome::is_round(label) {
        if on {
            gfx.fill_circle(x + 17.0, cy, 17.0, ink);
        } else {
            gfx.stroke_circle(x + 17.0, cy, 17.0, 2.0, ink);
        }
        gfx.text(x + 17.0, cy + 6.0, label, style, Align::Centre);
        return 34.0;
    }
    let style = TextStyle::new(14, Weight::Bold, letter);
    let w = (gfx.measure(label, style) + 18.0).max(40.0);
    let r = Rect::new(x, cy - 14.0, w, 28.0);
    if on {
        gfx.fill_round_rect(r, 8.0, ink);
    } else {
        gfx.stroke_round_rect(r, 8.0, 2.0, ink);
    }
    gfx.text(x + w / 2.0, cy + 5.0, label, style, Align::Centre);
    w
}

/// A five-pointed star centred on (cx, cy), drawn rather than a text glyph (not every font
/// covers "★").
pub(super) fn star(gfx: &mut Gfx<'_>, cx: f32, cy: f32, r: f32, c: Colour) {
    let point = |i: usize, radius: f32| {
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        (cx + radius * a.cos(), cy + radius * a.sin())
    };
    let inner = r * 0.42;
    let pentagon: Vec<_> = (0..5).map(|i| point(2 * i + 1, inner)).collect();
    gfx.fill_polygon(&pentagon, c);
    for i in 0..5 {
        gfx.fill_polygon(
            &[point(2 * i + 9, inner), point(2 * i, r), point(2 * i + 1, inner)],
            c,
        );
    }
}

impl Screen for DetailsPage {
    fn title(&self) -> &str {
        &self.game.title
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        self.reload_if_changed(ctx);
        if self.gone {
            return Transition::Pop;
        }
        self.sync_missing(ctx);
        match input {
            Input::Action(Action::Back) => return Transition::Pop,
            Input::Action(Action::Options) | Input::Pointer(Pointer::RightClick { .. }) => {
                return self.options(ctx);
            }
            Input::Action(Action::Left) if self.missing.is_none() => self.focus = Button::Play,
            Input::Action(Action::Right) => self.focus = Button::Options,
            Input::Action(Action::Confirm) => return self.press(self.focus, ctx),
            Input::Pointer(Pointer::Move { x, y }) => {
                if let Some(button) = self.button_at(x, y) {
                    self.focus = button;
                }
            }
            Input::Pointer(Pointer::Click { x, y }) => {
                if let Some(button) = self.button_at(x, y) {
                    self.focus = button;
                    return self.press(button, ctx);
                }
            }
            _ => {}
        }
        Transition::None
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Transition {
        self.reload_if_changed(ctx);
        if self.gone {
            return Transition::Pop;
        }
        self.poll_size();
        Transition::None
    }

    fn busy(&self) -> bool {
        matches!(self.size, Size::Pending(_))
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.reload_if_changed(ctx);
        self.sync_missing(ctx);
        self.poll_size();
        let colour = Colour::hex(self.system.map_or(0x5B6477, |s| s.colour));
        draw_hero(gfx, colour, &ctx.artwork.hero(&ctx.library, &self.game));
        let art = ctx.artwork.cover(&ctx.library, &self.game);
        cover::draw(gfx, COVER, &self.game, &art, false, false);

        let pill_style = TextStyle::new(14, Weight::Bold, TEXT);
        let short = self.system.map_or("?", |s| s.short);
        let pill_w = gfx.measure(short, pill_style) + 48.0;
        gfx.fill_round_rect(Rect::new(X, 196.0, pill_w, 32.0), 16.0, colour);
        gfx.text(X + pill_w / 2.0, 218.0, short, pill_style, Align::Centre);
        if self.game.favorite {
            let x = X + pill_w + 20.0;
            star(gfx, x + 9.0, 213.0, 10.0, STAR);
            let style = TextStyle::new(18, Weight::SemiBold, STAR);
            gfx.text(x + 30.0, 220.0, "Favorite", style, Align::Left);
        }

        let title = TextStyle::new(84, Weight::ExtraBold, TEXT);
        let label = ellipsize(&self.game.title, W - M - X, |s| gfx.measure(s, title));
        gfx.text(X - 4.0, 322.0, &label, title, Align::Left);
        if let Some(line) = self.system.and_then(opens_in) {
            let style = TextStyle::new(22, Weight::Medium, MUTED);
            gfx.text(X, 368.0, &line, style, Align::Left);
        }

        let stats = self.stats();
        let w = if stats.len() > 4 { STAT_W_NARROW } else { STAT_W };
        let key = TextStyle::new(14, Weight::Bold, MUTED);
        let value = TextStyle::new(30, Weight::Bold, TEXT);
        for (i, (k, v)) in stats.iter().enumerate() {
            let x = X + i as f32 * (w + STAT_GAP);
            gfx.fill_round_rect(Rect::new(x, STAT_Y, w, STAT_H), 14.0, SURFACE.alpha(0.75));
            gfx.text(x + 22.0, STAT_Y + 38.0, k, key, Align::Left);
            let v = ellipsize(v, w - 44.0, |s| gfx.measure(s, value));
            gfx.text(x + 22.0, STAT_Y + 80.0, &v, value, Align::Left);
        }

        let folder = matches!(self.size, Size::Known(Some(Measured { folder: true, .. })));
        let path = if folder {
            format!("{}{}", self.path, std::path::MAIN_SEPARATOR)
        } else {
            self.path.clone()
        };
        let style = TextStyle::new(18, Weight::Regular, MUTED);
        let path = ellipsize(&path, W - M - X, |s| gfx.measure(s, style));
        gfx.text(X, 560.0, &path, style, Align::Left);

        self.draw_play(gfx, ctx);
        self.draw_options(gfx, ctx);
        let focused = match self.focus {
            Button::Play => PLAY,
            Button::Options => OPTIONS,
        };
        gfx.stroke_round_rect(focused.inset(-5.0), BUTTON_RADIUS + 5.0, 3.0, WHITE);

        if let (Some(emulator), Some(system)) = (self.missing, self.system) {
            gfx.fill_circle(X + 14.0, WARN_Y - 7.0, 14.0, WARN);
            let bang = TextStyle::new(18, Weight::ExtraBold, BG);
            gfx.text(X + 14.0, WARN_Y, "!", bang, Align::Centre);
            let style = TextStyle::new(19, Weight::Medium, WARN);
            let line = ellipsize(&missing_line(emulator, system), W - M - X - 40.0, |s| {
                gfx.measure(s, style)
            });
            gfx.text(X + 40.0, WARN_Y, &line, style, Align::Left);
        }
    }

    fn hints(&self) -> Vec<Hint> {
        let mut hints = Vec::new();
        if self.focus == Button::Play {
            hints.push(Hint::new(&[Action::Confirm], "Play"));
        }
        hints.push(Hint::new(&[Action::Back], "Back"));
        hints.push(Hint::new(&[Action::Options], "Options"));
        hints
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulators::Emulators;
    use crate::library::FoundGame;
    use crate::library::db::SortOrder;
    use std::fs::File;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        File::create(path).unwrap();
    }

    /// A temporary Games Root and data dir holding one Game of `system` at `rel_path`.
    fn world(system: &'static str, rel_path: &str) -> (tempfile::TempDir, Ctx, GameRow) {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = dir.path().join("Games");
        ctx.data_dir = dir.path().join("data");
        touch(&ctx.games_root.join(rel_path));
        let found = FoundGame {
            system,
            rel_path: rel_path.into(),
            title: "Zelda".into(),
        };
        ctx.library.sync_system(system, &[found], 0).unwrap();
        let game = ctx.library.system_games(system, SortOrder::AZ).unwrap().remove(0);
        (dir, ctx, game)
    }

    fn act(page: &mut DetailsPage, ctx: &mut Ctx, action: Action) -> Transition {
        page.handle(Input::Action(action), ctx)
    }

    fn stat(page: &DetailsPage, key: &str) -> Option<String> {
        page.stats().into_iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    const PLAY_HINT: Hint = Hint::new(&[Action::Confirm], "Play");

    #[test]
    fn play_on_the_details_page_plays_an_embedded_game_whose_core_is_installed() {
        let (_dir, mut ctx, game) = world("snes", "snes/Zelda.sfc");
        touch(&cores::installed_path(&ctx.data_dir, &cores::BSNES));
        let mut page = DetailsPage::new(game.clone(), &ctx);
        assert_eq!(page.focus, Button::Play, "focus starts on Play");
        assert_eq!(page.hints()[0], PLAY_HINT);

        assert!(matches!(
            act(&mut page, &mut ctx, Action::Confirm),
            Transition::None
        ));
        assert_eq!(ctx.play.take().map(|p| p.game.id), Some(game.id));
        assert!(ctx.launch.is_none());
    }

    #[test]
    fn play_on_the_details_page_launches_an_external_game_whose_emulator_is_installed() {
        let (_dir, mut ctx, game) = world("gc", "gc/Zelda.iso");
        let mut page = DetailsPage::new(game.clone(), &ctx);
        let play = (PLAY.x + 10.0, PLAY.y + 10.0);
        page.handle(Input::Pointer(Pointer::Click { x: play.0, y: play.1 }), &mut ctx);
        assert_eq!(ctx.launch.take().map(|g| g.id), Some(game.id), "a click on Play");
        assert!(ctx.play.is_none());

        assert!(matches!(
            act(&mut page, &mut ctx, Action::Confirm),
            Transition::None
        ));
        assert_eq!(ctx.launch.take().map(|g| g.id), Some(game.id), "Confirm on Play");
    }

    #[test]
    fn options_y_confirm_on_options_or_a_right_click_on_the_details_page_opens_its_options_menu() {
        let (_dir, mut ctx, game) = world("gc", "gc/Zelda.iso");
        let mut page = DetailsPage::new(game, &ctx);
        let opens = |t: Transition| matches!(t, Transition::Push(m) if m.is_modal() && m.title() == "Zelda");
        assert!(opens(act(&mut page, &mut ctx, Action::Options)), "Y");
        act(&mut page, &mut ctx, Action::Right);
        assert_eq!(page.focus, Button::Options);
        assert!(!page.hints().contains(&PLAY_HINT));
        assert!(
            opens(act(&mut page, &mut ctx, Action::Confirm)),
            "Confirm on Options"
        );
        let at = (OPTIONS.x + 10.0, OPTIONS.y + 10.0);
        assert!(opens(page.handle(
            Input::Pointer(Pointer::Click { x: at.0, y: at.1 }),
            &mut ctx
        )));
        assert!(opens(page.handle(
            Input::Pointer(Pointer::RightClick { x: 5.0, y: 5.0 }),
            &mut ctx
        )));
        assert!(ctx.launch.is_none() && ctx.play.is_none());
        act(&mut page, &mut ctx, Action::Left);
        assert_eq!(page.focus, Button::Play);
        assert!(matches!(act(&mut page, &mut ctx, Action::Back), Transition::Pop));
    }

    #[test]
    fn hiding_the_game_from_the_details_pages_options_menu_pops_the_page_too() {
        let (_dir, mut ctx, game) = world("gc", "gc/Zelda.iso");
        let mut page = DetailsPage::new(game.clone(), &ctx);
        let Transition::Push(mut menu) = act(&mut page, &mut ctx, Action::Options) else {
            panic!("Y should open the Options Menu");
        };
        // Past Choose artwork and Rename to Hide (no Save State to load).
        menu.handle(Input::Action(Action::Down), &mut ctx);
        menu.handle(Input::Action(Action::Down), &mut ctx);
        menu.handle(Input::Action(Action::Down), &mut ctx);
        assert!(matches!(
            menu.handle(Input::Action(Action::Confirm), &mut ctx),
            Transition::Pop
        ));
        assert_eq!(ctx.library.game(game.id).unwrap(), None, "hidden");
        assert!(matches!(page.tick(&mut ctx), Transition::Pop));
    }

    #[test]
    fn the_details_page_leaves_once_its_game_is_hidden_or_gone_from_its_folder() {
        let (_dir, mut ctx, game) = world("gc", "gc/Zelda.iso");
        let mut page = DetailsPage::new(game.clone(), &ctx);
        assert!(matches!(page.tick(&mut ctx), Transition::None));
        ctx.library.set_hidden(game.id, true).unwrap();
        ctx.revision += 1;
        assert!(matches!(page.tick(&mut ctx), Transition::Pop), "hidden");

        let (_dir, mut ctx, game) = world("gc", "gc/Zelda.iso");
        let mut page = DetailsPage::new(game, &ctx);
        ctx.library.sync_system("gc", &[], 1).unwrap();
        ctx.revision += 1;
        assert!(
            matches!(act(&mut page, &mut ctx, Action::Right), Transition::Pop),
            "gone"
        );
    }

    #[test]
    fn with_its_emulator_missing_the_details_page_starts_on_options_and_play_does_nothing() {
        let (_dir, mut ctx, game) = world("wiiu", "wiiu/Zelda.wua");
        ctx.emulators = Emulators::from_installed(Vec::<String>::new());
        let mut page = DetailsPage::new(game, &ctx);
        assert_eq!(page.focus, Button::Options);
        assert_eq!(
            page.hints(),
            [
                Hint::new(&[Action::Back], "Back"),
                Hint::new(&[Action::Options], "Options")
            ]
        );

        act(&mut page, &mut ctx, Action::Left);
        assert_eq!(page.focus, Button::Options, "Play can't take focus");
        assert!(
            matches!(act(&mut page, &mut ctx, Action::Confirm), Transition::Push(_)),
            "Confirm opens the Options Menu, not Play"
        );
        let play = (PLAY.x + 10.0, PLAY.y + 10.0);
        page.handle(Input::Pointer(Pointer::Move { x: play.0, y: play.1 }), &mut ctx);
        assert_eq!(page.focus, Button::Options, "nor the pointer over it");
        let click = page.handle(Input::Pointer(Pointer::Click { x: play.0, y: play.1 }), &mut ctx);
        assert!(matches!(click, Transition::None));
        assert!(ctx.launch.is_none() && ctx.play.is_none());
        assert_eq!(
            missing_line(page.missing.unwrap(), page.system.unwrap()),
            "Cemu isn't installed, so Play is off. The install command is on the Wii U Console Page."
        );
    }

    /// On Windows, Dolphin has no winget package (FR-WIN-5), so the line can't promise an
    /// "install command"; it points at the Console Page's Choose .exe and download page instead.
    #[test]
    #[cfg(windows)]
    fn on_windows_a_download_page_only_emulator_gets_no_install_command_promise() {
        let (_dir, mut ctx, game) = world("gc", "gc/Metroid.iso");
        ctx.emulators = Emulators::from_installed(Vec::<String>::new());
        let page = DetailsPage::new(game, &ctx);
        assert_eq!(
            missing_line(page.missing.unwrap(), page.system.unwrap()),
            "Dolphin isn't installed, so Play is off. Choose its program, or get it, on the GameCube Console Page."
        );
    }

    #[test]
    fn details_playtime_and_last_played_refresh_when_the_library_changes_after_a_play() {
        let (_dir, mut ctx, game) = world("gc", "gc/Zelda.iso");
        let mut page = DetailsPage::new(game.clone(), &ctx);
        assert_eq!(stat(&page, "PLAYTIME").as_deref(), Some("—"));
        assert_eq!(stat(&page, "LAST PLAYED").as_deref(), Some("Never"));

        // As `launch::record` does when the Game ends and its screen pops back here.
        ctx.library.record_play(game.id, 1000, 3720).unwrap();
        ctx.revision += 1;
        act(&mut page, &mut ctx, Action::Right);

        assert_eq!(stat(&page, "PLAYTIME").as_deref(), Some("1h 02m"));
        assert_ne!(stat(&page, "LAST PLAYED").as_deref(), Some("Never"));
    }

    #[test]
    fn an_embedded_game_counts_its_filled_save_state_slots_and_an_external_one_has_none() {
        let (_dir, ctx, game) = world("snes", "snes/Zelda.sfc");
        let paths = StatePaths::new(&ctx.games_root, "snes", Path::new(&game.rel_path));
        touch(&paths.slot(1));
        touch(&paths.slot(3));
        let page = DetailsPage::new(game, &ctx);
        let keys: Vec<_> = page.stats().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["PLAYTIME", "LAST PLAYED", "SAVE STATES", "ADDED", "SIZE"]);
        assert_eq!(stat(&page, "SAVE STATES").as_deref(), Some("2 of 3"));

        let (_dir, ctx, game) = world("gc", "gc/Zelda.iso");
        let page = DetailsPage::new(game, &ctx);
        let keys: Vec<_> = page.stats().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["PLAYTIME", "LAST PLAYED", "ADDED", "SIZE"]);
    }

    #[test]
    fn deleting_the_save_states_from_the_options_menu_updates_the_count() {
        let (_dir, mut ctx, game) = world("snes", "snes/Zelda.sfc");
        let paths = StatePaths::new(&ctx.games_root, "snes", Path::new(&game.rel_path));
        touch(&paths.slot(1));
        let mut page = DetailsPage::new(game, &ctx);
        assert_eq!(stat(&page, "SAVE STATES").as_deref(), Some("1 of 3"));
        let Transition::Push(mut menu) = act(&mut page, &mut ctx, Action::Options) else {
            panic!("Y should open the Options Menu");
        };
        // Past Load save state and Start from power-on to Delete save data, whose first row with
        // anything to delete is Save States.
        for action in [Action::Down, Action::Down, Action::Down, Action::Confirm] {
            menu.handle(Input::Action(action), &mut ctx);
        }
        let Transition::Push(mut confirm) = menu.handle(Input::Action(Action::Confirm), &mut ctx) else {
            panic!("expected a confirmation");
        };
        assert_eq!(confirm.title(), "Delete the Save State?");
        // Delete is focused by default (hold mode): arm, then hold Confirm the full second.
        confirm.tick(&mut ctx);
        ctx.confirm_held = true;
        confirm.tick(&mut ctx);
        ctx.ticks += crate::screens::confirm::HOLD_MS;
        confirm.tick(&mut ctx);
        ctx.confirm_held = false;
        act(&mut page, &mut ctx, Action::Right);
        assert_eq!(stat(&page, "SAVE STATES").as_deref(), Some("0 of 3"));
    }

    #[test]
    fn details_input_never_waits_for_the_size_which_shows_once_measured() {
        let (_dir, mut ctx, game) = world("gc", "gc/Zelda.iso");
        // A measurement that hasn't finished: its sender is still held.
        let (tx, rx) = mpsc::channel();
        let mut page = DetailsPage::with_size(game, &ctx, rx);
        assert!(page.busy());
        assert!(matches!(
            act(&mut page, &mut ctx, Action::Right),
            Transition::None
        ));
        assert_eq!(page.focus, Button::Options, "input is handled while measuring");
        page.tick(&mut ctx);
        assert_eq!(stat(&page, "SIZE").as_deref(), Some("…"));

        tx.send(Some(Measured {
            bytes: 512 * 1024,
            folder: false,
        }))
        .unwrap();
        page.tick(&mut ctx);
        assert!(!page.busy());
        assert_eq!(stat(&page, "SIZE").as_deref(), Some("512 KB"));
    }

    #[test]
    fn a_size_that_cant_be_measured_shows_a_dash() {
        let (_dir, mut ctx, game) = world("gc", "gc/Zelda.iso");
        let (tx, rx) = mpsc::channel();
        let mut page = DetailsPage::with_size(game, &ctx, rx);
        drop(tx);
        page.tick(&mut ctx);
        assert!(!page.busy());
        assert_eq!(stat(&page, "SIZE").as_deref(), Some("—"));
    }

    #[test]
    fn a_folder_game_measures_the_sum_of_its_files() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("wiiu/Xenoblade");
        fs::create_dir_all(game.join("code")).unwrap();
        fs::write(game.join("code/game.rpx"), [0; 300]).unwrap();
        fs::write(game.join("meta.xml"), [0; 20]).unwrap();
        assert_eq!(
            measure(&game),
            Some(Measured {
                bytes: 320,
                folder: true
            })
        );
        assert_eq!(
            measure(&game.join("meta.xml")),
            Some(Measured {
                bytes: 20,
                folder: false
            })
        );
        assert_eq!(measure(&dir.path().join("gone.iso")), None);
    }

    #[test]
    fn sizes_show_three_significant_digits() {
        assert_eq!(size_label(0), "0 B");
        assert_eq!(size_label(1023), "1023 B");
        assert_eq!(size_label(512 * 1024), "512 KB");
        assert_eq!(size_label(1_460_288_880), "1.36 GB");
        assert_eq!(size_label(24_051_816_858), "22.4 GB");
    }

    fn day(year: i32, month: u32, day: u32) -> Local {
        Local {
            year,
            month,
            day,
            hour: 12,
            minute: 0,
        }
    }

    #[test]
    fn dates_show_today_yesterday_or_the_day() {
        let now = day(2026, 10, 21);
        assert_eq!(day_label(day(2026, 10, 21), now), "Today");
        assert_eq!(day_label(day(2026, 10, 20), now), "Yesterday");
        assert_eq!(day_label(day(2026, 10, 3), now), "3 Oct 2026");
        assert_eq!(day_label(day(2025, 12, 31), day(2026, 1, 1)), "Yesterday");
        assert_eq!(date_label(day(2026, 1, 17)), "17 Jan 2026");
    }

    #[test]
    fn opens_in_names_the_standalone_emulator_or_the_core() {
        let by_id = |id| systems::by_id(id).unwrap();
        assert_eq!(opens_in(by_id("gc")).as_deref(), Some("Opens in Dolphin"));
        assert_eq!(
            opens_in(by_id("snes")).as_deref(),
            Some("Opens in Game37 · bsnes core")
        );
    }
}
