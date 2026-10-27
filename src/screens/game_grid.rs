//! A titled grid of tall covers for a list of Games: "See all" on Home, and a System's Console
//! Page (FR-CAR-3), after `screen_console_page` in `design/generate_mockups.py`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::choose_exe;
use super::details::DetailsPage;
use super::options::OptionsMenu;
use crate::ctx::Ctx;
use crate::emulators::{Emulator, InstallHint};
use crate::library::db::{DbError, GameRow, Library, SortOrder};
use crate::systems::System;
use crate::ui::chrome;
use crate::ui::cover::{self, COVER_GAP, COVER_H, COVER_W, RING_REACH};
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer, glyph_label};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{BG, Colour, LINE, MUTED, SURFACE, TEXT, WARN};
use crate::ui::{M, Rect, W, hit, scroll};

/// Where a grid's Games come from; called again whenever the library changes.
pub type Source = Box<dyn Fn(&Library) -> Result<Vec<GameRow>, DbError>>;

/// A Console Page's extras, absent on Home's "See all": the colour tint, the sort chip, the
/// empty-state hint pointing at the System's folder (FR-CAR-4), and the System itself, to check
/// for a missing Standalone Emulator (FR-CAR-9).
struct Extra {
    tint: Colour,
    empty_hint: String,
    system: &'static System,
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

/// The missing-emulator notice (FR-CAR-9), after `screen_console_page_missing_emulator` in
/// `design/generate_mockups.py`.
const NOTICE_Y: f32 = 268.0;
const NOTICE_H: f32 = 176.0;
/// Gap between the notice and the grid or empty-state message below it.
const NOTICE_GAP: f32 = 40.0;

/// Which of the notice's buttons has focus: Copy command and Check again on Linux (FR-CAR-9);
/// on Windows, Copy command or Download page (whichever the emulator's `InstallHint` is), then
/// Choose .exe, then Check again (FR-WIN-4, FR-WIN-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoticeFocus {
    Copy,
    Page,
    Choose,
    Check,
}

/// How long "Copied" or "Still not installed" shows on a notice button before reverting to its
/// usual label (US-39).
const FEEDBACK: Duration = Duration::from_secs(2);

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

/// [`step_focus`] for the Windows notice, which has three buttons rather than two (FR-WIN-4,
/// FR-WIN-5): `stops` is their order, first to last, from [`GameGrid::notice_stops`].
fn step_notice_focus(
    focus: Option<NoticeFocus>,
    grid_focus: usize,
    action: Action,
    len: usize,
    cols: usize,
    stops: &[NoticeFocus],
) -> (Option<NoticeFocus>, usize) {
    match focus {
        Some(nf) if action == Action::Right => {
            let i = stops.iter().position(|&s| s == nf).unwrap_or(0);
            (Some(stops[(i + 1).min(stops.len() - 1)]), grid_focus)
        }
        Some(nf) if action == Action::Left => {
            let i = stops.iter().position(|&s| s == nf).unwrap_or(0);
            (Some(stops[i.saturating_sub(1)]), grid_focus)
        }
        Some(_) if action == Action::Down && len > 0 => (None, 0),
        Some(nf) => (Some(nf), grid_focus),
        None if action == Action::Up && grid_focus < cols => (Some(stops[0]), grid_focus),
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
    focus: usize,
    scroll: f32,
    hits: Vec<(Rect, usize)>,
    extra: Option<Extra>,
    /// Where the sort chip was last drawn, for the mouse.
    sort_hit: Option<Rect>,
    /// The global sort order, cached from `reload()` so `draw()` never queries the database
    /// (NFR-3). Meaningless when `extra` is `None`.
    sort_order: SortOrder,
    /// Whether the missing-emulator notice is shown, last computed from `ctx.emulators`
    /// (FR-CAR-9).
    notice_shown: bool,
    /// Which notice button has focus; `None` when the grid has focus, or there's no notice.
    notice_focus: Option<NoticeFocus>,
    /// Where the notice's buttons were last drawn, for the mouse.
    notice_hits: Vec<(Rect, NoticeFocus)>,
    /// Transient feedback on a notice button after it's pressed, and until when (US-39).
    feedback: Option<(NoticeFocus, &'static str, Instant)>,
    /// Whether the notice shows Windows' winget command or download page and Choose .exe, rather
    /// than Linux's Flatpak command (FR-WIN-4, FR-WIN-5): `cfg!(windows)`, except tests turn it
    /// on to drive the Windows notice from Linux.
    windows: bool,
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
            notice_shown: false,
            notice_focus: None,
            notice_hits: Vec::new(),
            feedback: None,
            windows: cfg!(windows),
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
            Some(game) => Transition::Push(Box::new(OptionsMenu::new(game.clone(), &ctx.games_root))),
            None => Transition::None,
        }
    }

    /// Turns this into a Console Page: a `tint` from the System's colour, a sort chip,
    /// `empty_hint` shown in place of the grid when the System has no Games (FR-CAR-4, FR-CAR-5),
    /// and the System itself, so a missing Standalone Emulator can show its notice (FR-CAR-9).
    pub fn sortable(mut self, tint: Colour, empty_hint: String, system: &'static System) -> GameGrid {
        self.extra = Some(Extra {
            tint,
            empty_hint,
            system,
        });
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
        self.scroll = scroll::clamp(self.scroll, self.content_h(), BOTTOM - self.top_y());
        self.reveal_focus();
    }

    fn reload_if_changed(&mut self, ctx: &Ctx) {
        if self.revision != Some(ctx.revision) {
            self.reload(ctx);
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
    /// (cheap) rather than cached across frames (FR-CAR-9). Focus starts on the notice's first
    /// button when it newly appears; when it clears, focus moves to the grid, or to nothing on an
    /// empty page (US-39).
    fn sync_notice(&mut self, ctx: &Ctx) {
        let emulator = self.missing_emulator(ctx);
        let shown = emulator.is_some();
        if shown == self.notice_shown {
            return;
        }
        self.notice_shown = shown;
        self.feedback = None;
        if !shown {
            // The buttons are gone: a pointer over where they were must not focus them.
            self.notice_hits.clear();
        }
        match emulator {
            Some(emulator) => self.notice_focus = Some(self.notice_stops(emulator)[0]),
            None => {
                self.notice_focus = None;
                self.focus = 0;
            }
        }
    }

    /// What gets `emulator` installed, as the notice shows it: the Flatpak command on Linux; on
    /// Windows the `windows: bool` field decides, so tests drive the Windows notice from Linux
    /// (FR-WIN-4, FR-WIN-5).
    fn notice_hint(&self, emulator: &Emulator) -> InstallHint {
        if self.windows {
            emulator.windows_install_hint()
        } else {
            emulator.install_hint()
        }
    }

    /// The notice's buttons, first to last: on Linux always Copy command then Check again; on
    /// Windows, Copy command or Download page (whichever `emulator`'s hint is), then Choose .exe,
    /// then Check again (FR-WIN-4, FR-WIN-5).
    fn notice_stops(&self, emulator: &Emulator) -> Vec<NoticeFocus> {
        if !self.windows {
            return vec![NoticeFocus::Copy, NoticeFocus::Check];
        }
        let first = match self.notice_hint(emulator) {
            InstallHint::Command(_) => NoticeFocus::Copy,
            InstallHint::Page(_) => NoticeFocus::Page,
        };
        vec![first, NoticeFocus::Choose, NoticeFocus::Check]
    }

    /// Re-detects the installed emulators and re-syncs the notice, which clears it when the
    /// emulator is now found (US-39).
    fn check_again(&mut self, ctx: &mut Ctx) {
        ctx.emulators = (ctx.detect_emulators)();
        self.sync_notice(ctx);
        if self.notice_shown {
            self.feedback = Some((NoticeFocus::Check, "Still not installed", Instant::now()));
        }
    }

    /// Presses the notice button that currently has focus (US-39, FR-WIN-4, FR-WIN-5): Copy
    /// command puts the install command on the clipboard through `Ctx`'s seam; Download page asks
    /// `Ctx` to open it; Choose .exe pushes the `.exe` browser, which saves the choice and clears
    /// the notice once it's picked; Check again re-detects the installed emulators.
    fn activate_notice(&mut self, ctx: &mut Ctx) -> Transition {
        let (Some(focus), Some(emulator)) = (self.notice_focus, self.missing_emulator(ctx)) else {
            return Transition::None;
        };
        match focus {
            NoticeFocus::Copy => {
                ctx.clipboard_request = Some(self.notice_hint(emulator).text().to_owned());
                self.feedback = Some((NoticeFocus::Copy, "Copied", Instant::now()));
            }
            NoticeFocus::Page => {
                if let InstallHint::Page(url) = self.notice_hint(emulator) {
                    ctx.open_url_request = Some(url.to_owned());
                }
            }
            NoticeFocus::Choose => {
                let on_choose = Box::new(move |ctx: &mut Ctx, path: PathBuf| {
                    choose_exe::save(ctx, emulator, Some(&path));
                });
                return Transition::Push((ctx.choose_exe)(emulator, None, false, on_choose));
            }
            NoticeFocus::Check => self.check_again(ctx),
        }
        Transition::None
    }

    /// Which notice button, if any, is under a point; `None` while the notice is hidden.
    fn notice_button_at(&self, x: f32, y: f32) -> Option<NoticeFocus> {
        if !self.notice_shown {
            return None;
        }
        hit(&self.notice_hits, x, y)
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
        let bottom_minus_top = BOTTOM - self.top_y();
        let s = scroll::keep_visible(
            self.scroll,
            row * PITCH_Y - RING_REACH,
            COVER_H + 2.0 * RING_REACH + 40.0,
            bottom_minus_top,
        );
        self.scroll = scroll::clamp(s, self.content_h(), bottom_minus_top);
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
    /// `screen_console_missing_emulator` (Linux) or `screen_console_missing_emulator_windows`
    /// (Windows) in `design/generate_mockups.py`. Returns where its buttons were drawn, for the
    /// mouse.
    fn draw_notice(
        &self,
        gfx: &mut Gfx<'_>,
        emulator: &Emulator,
        system: &System,
    ) -> Vec<(Rect, NoticeFocus)> {
        self.draw_notice_frame(gfx, emulator, system);
        if self.windows {
            self.draw_notice_windows(gfx, emulator)
        } else {
            self.draw_notice_linux(gfx, emulator)
        }
    }

    /// The notice's box, warning icon, headline and body line, shared by Linux and Windows.
    fn draw_notice_frame(&self, gfx: &mut Gfx<'_>, emulator: &Emulator, system: &System) {
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
    }

    /// Linux's notice buttons: the Flatpak command box with Copy command, then Check again
    /// (FR-CAR-9). Unchanged since before Windows (mockup 09).
    fn draw_notice_linux(&self, gfx: &mut Gfx<'_>, emulator: &Emulator) -> Vec<(Rect, NoticeFocus)> {
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

        vec![(copy, NoticeFocus::Copy), (check, NoticeFocus::Check)]
    }

    /// Windows' notice buttons, after `screen_console_missing_emulator_windows` (mockup 36), but
    /// 48 px tall rather than the mockup's 64, so they stay inside the 176 px notice box
    /// (FR-WIN-4, FR-WIN-5): a winget command box with Copy command, or a Download page button
    /// when the emulator has no winget package; then Choose .exe; then Check again.
    fn draw_notice_windows(&self, gfx: &mut Gfx<'_>, emulator: &Emulator) -> Vec<(Rect, NoticeFocus)> {
        let cy = NOTICE_Y + 106.0;
        let label_style = TextStyle::new(18, Weight::SemiBold, TEXT);
        let mut x = M + 100.0;
        let mut hits = Vec::new();

        match self.notice_hint(emulator) {
            InstallHint::Command(cmd) => {
                let cmd_box = Rect::new(x, cy, 640.0, 48.0);
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
                x += cmd_box.w + 20.0;

                let copy_label = self.notice_label(NoticeFocus::Copy, "Copy command");
                let copy = Rect::new(x, cy, 206.0, 48.0);
                self.draw_notice_button(gfx, copy, NoticeFocus::Copy, copy_label);
                hits.push((copy, NoticeFocus::Copy));
                x += copy.w + 20.0;
            }
            InstallHint::Page(url) => {
                let label = "Download page";
                let page_w = 28.0 + 34.0 + gfx.measure(label, label_style) + 28.0;
                let page = Rect::new(x, cy, page_w, 48.0);
                self.draw_notice_button(gfx, page, NoticeFocus::Page, label);
                choose_exe::download_icon(gfx, page.x + 28.0, cy + 24.0, MUTED);
                hits.push((page, NoticeFocus::Page));
                x += page.w + 20.0;

                let url_style = TextStyle::new(16, Weight::Medium, MUTED);
                gfx.text(x, cy + 31.0, url, url_style, Align::Left);
                x += gfx.measure(url, url_style) + 40.0;
            }
        }

        let choose_label = "Choose .exe";
        let choose_w = 28.0 + gfx.measure(choose_label, label_style) + 28.0;
        let choose = Rect::new(x, cy, choose_w, 48.0);
        self.draw_notice_button(gfx, choose, NoticeFocus::Choose, choose_label);
        hits.push((choose, NoticeFocus::Choose));
        x += choose.w + 20.0;

        let check_label = self.notice_label(NoticeFocus::Check, "Check again");
        let check_w = 28.0 + gfx.measure("Check again", label_style) + 28.0;
        let check = Rect::new(x, cy, check_w, 48.0);
        self.draw_notice_button(gfx, check, NoticeFocus::Check, check_label);
        hits.push((check, NoticeFocus::Check));

        hits
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
        self.reload_if_changed(ctx);
        self.sync_notice(ctx);
        match input {
            Input::Action(Action::Back) => return Transition::Pop,
            Input::Action(Action::Confirm) if self.notice_focus.is_some() => {
                return self.activate_notice(ctx);
            }
            // Y is Check again while the Windows notice has focus, whichever button that is
            // (FR-WIN-4, FR-WIN-5).
            Input::Action(Action::Options) if self.windows && self.notice_focus.is_some() => {
                self.check_again(ctx);
            }
            Input::Action(Action::Sort) if self.extra.is_some() && self.notice_focus.is_none() => {
                self.cycle_sort(ctx)
            }
            Input::Action(dir) if dir.is_direction() => {
                self.feedback = None;
                if self.notice_shown {
                    let (nf, gf) = if self.windows {
                        let emulator = self
                            .missing_emulator(ctx)
                            .expect("the notice is shown only while an emulator is missing");
                        let stops = self.notice_stops(emulator);
                        step_notice_focus(self.notice_focus, self.focus, dir, self.games.len(), COLS, &stops)
                    } else {
                        step_focus(self.notice_focus, self.focus, dir, self.games.len(), COLS)
                    };
                    self.notice_focus = nf;
                    self.focus = gf;
                } else {
                    self.focus = step(self.focus, self.games.len(), COLS, dir);
                }
                if self.notice_focus.is_none() {
                    self.reveal_focus();
                }
            }
            Input::Pointer(Pointer::Click { x, y }) if self.sort_hit.is_some_and(|r| r.contains(x, y)) => {
                self.cycle_sort(ctx);
            }
            Input::Pointer(Pointer::Click { x, y }) if self.notice_button_at(x, y).is_some() => {
                self.notice_focus = self.notice_button_at(x, y);
                return self.activate_notice(ctx);
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
                let bottom_minus_top = BOTTOM - self.top_y();
                self.scroll =
                    scroll::clamp(self.scroll - dy * WHEEL_STEP, self.content_h(), bottom_minus_top);
            }
            _ => {}
        }
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.reload_if_changed(ctx);
        self.sync_notice(ctx);
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
        self.notice_hits.clear();
        if let Some(emulator) = self.missing_emulator(ctx) {
            let system = self
                .extra
                .as_ref()
                .expect("the notice only shows on a Console Page")
                .system;
            self.notice_hits = self.draw_notice(gfx, emulator, system);
        }
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
        let top = self.top_y();
        let clip_top = top - RING_REACH - 4.0;
        gfx.clip(Some(Rect::new(0.0, clip_top, W, BOTTOM - clip_top)));
        let first_row = ((self.scroll - RING_REACH) / PITCH_Y).floor().max(0.0) as usize;
        let last_row = ((self.scroll + BOTTOM - top) / PITCH_Y).ceil() as usize;
        let caption = TextStyle::new(17, Weight::Medium, MUTED);
        for i in first_row * COLS..((last_row + 1) * COLS).min(self.games.len()) {
            let (row, col) = (i / COLS, i % COLS);
            let r = Rect::new(
                M + col as f32 * PITCH_X,
                top + row as f32 * PITCH_Y - self.scroll,
                COVER_W,
                COVER_H,
            );
            let game = &self.games[i];
            // While a notice button has focus, no cover is drawn focused.
            let art = ctx.artwork.cover(&ctx.library, game);
            cover::draw(
                gfx,
                r,
                game,
                &art,
                true,
                i == self.focus && self.notice_focus.is_none(),
            );
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
        if self.content_h() - self.scroll > BOTTOM - top {
            gfx.fill_round_rect_vgradient(Rect::new(0.0, 900.0, W, BOTTOM - 900.0), 0.0, BG.alpha(0.0), BG);
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
    }

    fn hints(&self) -> Vec<Hint> {
        match self.notice_focus {
            // On Windows, Y is Check again on any of the notice's buttons (FR-WIN-4, FR-WIN-5).
            Some(focus) if self.windows => {
                let a = match focus {
                    NoticeFocus::Copy => "Copy command",
                    NoticeFocus::Page => "Download page",
                    NoticeFocus::Choose => "Choose .exe",
                    NoticeFocus::Check => "Check again",
                };
                let mut hints = vec![Hint::new(&[Action::Confirm], a)];
                if focus != NoticeFocus::Check {
                    hints.push(Hint::new(&[Action::Options], "Check again"));
                }
                hints.push(Hint::new(&[Action::Back], "Back"));
                hints
            }
            Some(NoticeFocus::Copy) => vec![
                Hint::new(&[Action::Confirm], "Copy command"),
                Hint::new(&[Action::Back], "Back"),
            ],
            Some(NoticeFocus::Check) => vec![
                Hint::new(&[Action::Confirm], "Check again"),
                Hint::new(&[Action::Back], "Back"),
            ],
            Some(_) => unreachable!("Page and Choose only occur on Windows"),
            None => {
                let mut hints = Vec::new();
                if !self.games.is_empty() {
                    hints.push(Hint::new(&[Action::Confirm], "Details"));
                    hints.push(Hint::new(&[Action::Options], "Options"));
                }
                if self.extra.is_some() && !self.games.is_empty() {
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

    /// PS2: an External System, with PCSX2 as its Standalone Emulator, which has a winget
    /// package (FR-WIN-5).
    fn ps2() -> &'static System {
        systems::by_id("ps2").unwrap()
    }

    /// GameCube: an External System, with Dolphin as its Standalone Emulator, which has no
    /// winget package: only a download page (FR-WIN-5).
    fn gc() -> &'static System {
        systems::by_id("gc").unwrap()
    }

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
        let mut grid = GameGrid::new("NES", console_source()).sortable(Colour::hex(0), "hint".into(), snes());
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        assert_eq!(grid.sort_order, SortOrder::AZ);

        grid.handle(Input::Action(Action::Sort), &mut ctx);

        assert_eq!(grid.sort_order, SortOrder::LastPlayed);
        assert_eq!(ctx.library.sort_order().unwrap(), SortOrder::LastPlayed);
    }

    #[test]
    fn sort_is_ignored_on_an_empty_console_page() {
        let mut ctx = Ctx::empty();
        let mut grid = GameGrid::new("NES", console_source()).sortable(Colour::hex(0), "hint".into(), snes());
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
        ctx.library.sync_system("ps2", &[found_ps2(0)], 0).unwrap();
        let mut grid =
            GameGrid::new("PlayStation 2", ps2_source()).sortable(Colour::hex(0), "hint".into(), ps2());
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

    fn ps2_source() -> Source {
        Box::new(|library| library.system_games("ps2", SortOrder::AZ))
    }

    fn found_ps2(n: usize) -> FoundGame {
        FoundGame {
            system: "ps2",
            rel_path: format!("ps2/b{n:03}.iso"),
            title: format!("B{n:03}"),
        }
    }

    /// A Console Page for an External System whose Standalone Emulator isn't installed.
    fn missing_pcsx2_page() -> (Ctx, GameGrid) {
        let mut ctx = Ctx::empty();
        ctx.emulators = Emulators::from_installed(Vec::<String>::new());
        let mut grid =
            GameGrid::new("PlayStation 2", ps2_source()).sortable(Colour::hex(0), "hint".into(), ps2());
        // Pins Linux's notice (mockup 09) regardless of the host this test runs on.
        grid.windows = false;
        (ctx, grid)
    }

    fn empty_source() -> Source {
        Box::new(|_| Ok(Vec::new()))
    }

    /// A Windows Console Page (FR-WIN-4, FR-WIN-5) for `system`, whose Standalone Emulator isn't
    /// installed, with the Windows notice turned on so the test drives it from Linux.
    fn missing_windows_page(system: &'static System) -> (Ctx, GameGrid) {
        let mut ctx = Ctx::empty();
        ctx.emulators = Emulators::from_installed(Vec::<String>::new());
        let mut grid =
            GameGrid::new(system.name, empty_source()).sortable(Colour::hex(0), "hint".into(), system);
        grid.windows = true;
        (ctx, grid)
    }

    #[test]
    fn the_notice_shows_for_an_external_system_missing_its_emulator_and_starts_on_copy_command() {
        let (mut ctx, mut grid) = missing_pcsx2_page();
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
        let (mut ctx, mut grid) = missing_pcsx2_page();
        let found = [found_ps2(0), found_ps2(1)];
        ctx.library.sync_system("ps2", &found, 0).unwrap();
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        assert!(grid.notice_shown);
        ctx.detect_emulators = Box::new(|| Emulators::from_installed(["net.pcsx2.PCSX2"]));

        grid.handle(Input::Action(Action::Right), &mut ctx); // focus Check again
        grid.handle(Input::Action(Action::Confirm), &mut ctx);

        assert!(!grid.notice_shown, "PCSX2 is now installed");
        assert_eq!(grid.notice_focus, None, "focus moves to the grid");
        assert_eq!(grid.focus, 0);
    }

    #[test]
    fn check_again_keeps_the_notice_when_still_missing() {
        let (mut ctx, mut grid) = missing_pcsx2_page();
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        ctx.detect_emulators = Box::new(|| Emulators::from_installed(Vec::<String>::new()));

        grid.handle(Input::Action(Action::Right), &mut ctx); // focus Check again
        grid.handle(Input::Action(Action::Confirm), &mut ctx);

        assert!(grid.notice_shown, "PCSX2 is still missing");
        assert_eq!(
            grid.notice_focus,
            Some(NoticeFocus::Check),
            "focus stays on Check again"
        );
    }

    #[test]
    fn a_pointer_over_the_cleared_notices_buttons_focuses_nothing() {
        let (mut ctx, mut grid) = missing_pcsx2_page();
        ctx.library.sync_system("ps2", &[found_ps2(0)], 0).unwrap();
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        let check = Rect::new(100.0, 100.0, 50.0, 50.0);
        grid.notice_hits = vec![
            (Rect::new(0.0, 100.0, 50.0, 50.0), NoticeFocus::Copy),
            (check, NoticeFocus::Check),
        ]; // as last drawn
        ctx.detect_emulators = Box::new(|| Emulators::from_installed(["net.pcsx2.PCSX2"]));

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
        let (mut ctx, mut grid) = missing_pcsx2_page();
        // As `--missing-emulators ps2` does.
        ctx.simulate_emulators(Emulators::from_installed(Vec::<String>::new()));
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load

        grid.handle(Input::Action(Action::Right), &mut ctx); // focus Check again
        grid.handle(Input::Action(Action::Confirm), &mut ctx);

        assert!(grid.notice_shown, "the simulated missing emulator stays missing");
        assert_eq!(ctx.emulators, Emulators::from_installed(Vec::<String>::new()));
    }

    #[test]
    #[cfg(unix)] // The Flatpak command is Linux's; emulators.rs pins the Windows hints.
    fn copy_command_leaves_a_clipboard_request() {
        let (mut ctx, mut grid) = missing_pcsx2_page();
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load, focus on Copy

        grid.handle(Input::Action(Action::Confirm), &mut ctx);

        assert_eq!(
            ctx.clipboard_request.as_deref(),
            Some("flatpak install flathub net.pcsx2.PCSX2")
        );
    }

    #[test]
    fn the_windows_notice_shows_the_winget_command_and_copy_copies_it() {
        let (mut ctx, mut grid) = missing_windows_page(ps2());
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        assert!(grid.notice_shown);
        assert_eq!(
            grid.notice_focus,
            Some(NoticeFocus::Copy),
            "PCSX2 has a winget package"
        );

        grid.handle(Input::Action(Action::Confirm), &mut ctx);

        assert_eq!(
            ctx.clipboard_request.as_deref(),
            Some("winget install -e --id PCSX2Team.PCSX2")
        );
    }

    #[test]
    fn the_windows_notice_s_download_page_button_requests_the_page() {
        let (mut ctx, mut grid) = missing_windows_page(gc());
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        assert_eq!(
            grid.notice_focus,
            Some(NoticeFocus::Page),
            "Dolphin has no winget package"
        );

        grid.handle(Input::Action(Action::Confirm), &mut ctx);

        assert_eq!(
            ctx.open_url_request.as_deref(),
            Some("https://dolphin-emu.org/download/")
        );
    }

    #[test]
    fn choose_exe_pushes_the_browser_and_a_pick_saves_and_clears_the_notice() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("PCSX2/pcsx2-qt.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();
        let config = dir.path().join("config.toml");

        let (mut ctx, mut grid) = missing_windows_page(ps2());
        ctx.config_path = config.clone();
        ctx.detect_emulators = Box::new(move || {
            let chosen = crate::config::load_emulator_exes(&config).unwrap();
            Emulators::locate_all(&chosen, &crate::emulators::Places::default())
        });
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        let asked = choose_exe::tests::picking(&mut ctx, exe.clone());

        grid.handle(Input::Action(Action::Right), &mut ctx); // focus Choose .exe
        assert_eq!(grid.notice_focus, Some(NoticeFocus::Choose));
        let opened = grid.handle(Input::Action(Action::Confirm), &mut ctx);
        choose_exe::tests::answer(opened, &mut ctx, Action::Confirm);

        assert_eq!(*asked.borrow(), vec![("pcsx2", None, false)]);
        // Before the next draw or input, the choice has already saved and re-detected.
        grid.handle(Input::Pointer(Pointer::Move { x: -1.0, y: -1.0 }), &mut ctx);
        assert!(!grid.notice_shown, "PCSX2 is now found at the chosen .exe");
    }

    #[test]
    fn check_again_re_detects_on_windows_too() {
        let (mut ctx, mut grid) = missing_windows_page(ps2());
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load
        ctx.detect_emulators = Box::new(|| Emulators::from_installed(["net.pcsx2.PCSX2"]));

        grid.handle(Input::Action(Action::Right), &mut ctx); // focus Choose .exe
        grid.handle(Input::Action(Action::Right), &mut ctx); // focus Check again
        assert_eq!(grid.notice_focus, Some(NoticeFocus::Check));
        grid.handle(Input::Action(Action::Confirm), &mut ctx);

        assert!(!grid.notice_shown, "PCSX2 is now installed");
    }

    #[test]
    fn y_checks_again_while_the_windows_notice_has_focus() {
        let (mut ctx, mut grid) = missing_windows_page(ps2());
        grid.handle(Input::Action(Action::Back), &mut ctx); // initial load, focus on Copy
        ctx.detect_emulators = Box::new(|| Emulators::from_installed(["net.pcsx2.PCSX2"]));

        grid.handle(Input::Action(Action::Options), &mut ctx);

        assert!(
            !grid.notice_shown,
            "Y checked again and found it, without moving off Copy first"
        );
    }
}
