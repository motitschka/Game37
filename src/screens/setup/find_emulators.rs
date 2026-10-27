//! Step 2 on Windows, "Set up the emulators" (FR-WIN-4, FR-WIN-5), after
//! `screen_setup_emulators_windows` in `design/generate_mockups.py`: the Standalone Emulator of
//! each ticked External System, once, either found, with where, or not found, with Choose .exe,
//! which opens the `.exe` browser. Below them one winget line installs the missing ones that have
//! a winget package, and a button per missing one without opens its download page.
//!
//! There is no Config yet, and its existence means Setup is done, so the chosen `.exe` files stay
//! in the [`Model`] and Finish writes them; detection here counts them as chosen meanwhile.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::emulators::command_box;
use super::{Button, CONTENT_Y, Go, Model, SETUP_W, Step, tick};
use crate::ctx::Ctx;
use crate::emulators::{self, Emulator, WindowsInstall};
use crate::screens::choose_exe::download_icon;
use crate::screens::setup::folders::trim_start;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer};
use crate::ui::nav::{Hint, Screen};
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, LINE, MUTED, SUCCESS, SURFACE, SURFACE2, TEXT, WARN};
use crate::ui::{M, Rect, W, hit};

/// How long "Copied" shows on the Copy button.
const FEEDBACK: Duration = Duration::from_secs(2);
/// The first row's top, a little above the Linux step's.
const TOP: f32 = CONTENT_Y - 12.0;
/// Between the columns, when there are two.
const COL_GAP: f32 = 16.0;
const CHOOSE_W: f32 = 210.0;
const CHOOSE_H: f32 = 52.0;
const PILL_H: f32 = 64.0;
/// Between the download page buttons, down and across.
const PILL_GAP_Y: f32 = 16.0;
const PILL_GAP_X: f32 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    /// The Choose .exe button of row `i`.
    Choose(usize),
    Copy,
    Check,
    /// The download page button of the `i`th missing emulator without a winget package.
    Page(usize),
    Button(Button),
}

pub struct FindEmulators {
    focus: Focus,
    /// The ticked Systems' emulators, each once, in System Order, as of entering the step.
    rows: Vec<&'static Emulator>,
    /// Which of `rows` is missing, as last checked.
    missing: Vec<bool>,
    /// The winget line for the missing ones, as last checked; `None` when none has a package.
    command: Option<String>,
    /// The missing ones without a winget package, and their download pages.
    pages: Vec<(&'static Emulator, &'static str)>,
    /// The choices as of the last check, so a new one re-checks.
    seen: Vec<(&'static str, PathBuf)>,
    /// When Copy was last pressed, for its "Copied" feedback.
    copied: Option<Instant>,
    hits: Vec<(Rect, Focus)>,
    /// The browser Choose .exe opened, for Setup to push.
    browser: Option<Box<dyn Screen>>,
}

/// The ticked External Systems' emulators, each once, in System Order.
fn ticked_emulators(model: &Model) -> Vec<&'static Emulator> {
    let mut rows: Vec<&'static Emulator> = Vec::new();
    for emulator in model.ticked().filter_map(emulators::for_system) {
        if !rows.contains(&emulator) {
            rows.push(emulator);
        }
    }
    rows
}

/// `chosen` as a sorted list, to compare.
fn snapshot(model: &Model) -> Vec<(&'static str, PathBuf)> {
    let mut chosen: Vec<_> = model
        .chosen
        .borrow()
        .iter()
        .map(|(&k, p)| (k, p.clone()))
        .collect();
    chosen.sort();
    chosen
}

/// A download page as the mockup shows it: no scheme, no trailing slash.
fn short_url(url: &str) -> &str {
    let url = url.trim_start_matches("https://").trim_start_matches("http://");
    url.trim_end_matches('/')
}

/// Where the rows go: one column of the mockup's rows for up to four, else two columns of
/// shorter ones, down the columns, so seven leave room for the winget line and the pages below.
struct Layout {
    per_col: usize,
    row_w: f32,
    row_h: f32,
    pitch: f32,
}

impl Layout {
    fn new(n: usize) -> Layout {
        if n <= 4 {
            Layout {
                per_col: n.max(1),
                row_w: SETUP_W,
                row_h: 96.0,
                pitch: 108.0,
            }
        } else {
            Layout {
                per_col: n.div_ceil(2),
                row_w: (W - 2.0 * M - COL_GAP) / 2.0,
                row_h: 84.0,
                pitch: 96.0,
            }
        }
    }

    fn row(&self, i: usize) -> Rect {
        let (col, row) = (i / self.per_col, i % self.per_col);
        let x = M + col as f32 * (self.row_w + COL_GAP);
        Rect::new(x, TOP + row as f32 * self.pitch, self.row_w, self.row_h)
    }

    fn width(&self, n: usize) -> f32 {
        if n <= 4 { SETUP_W } else { W - 2.0 * M }
    }

    fn bottom(&self) -> f32 {
        TOP + (self.per_col - 1) as f32 * self.pitch + self.row_h
    }

    /// Where row `i`'s Choose .exe button is.
    fn choose(&self, i: usize) -> Rect {
        let r = self.row(i);
        Rect::new(
            r.x + r.w - 28.0 - CHOOSE_W,
            r.y + r.h / 2.0 - CHOOSE_H / 2.0,
            CHOOSE_W,
            CHOOSE_H,
        )
    }
}

impl FindEmulators {
    pub fn new() -> FindEmulators {
        FindEmulators {
            focus: Focus::Button(Button::Continue),
            rows: Vec::new(),
            missing: Vec::new(),
            command: None,
            pages: Vec::new(),
            seen: Vec::new(),
            copied: None,
            hits: Vec::new(),
            browser: None,
        }
    }

    /// Re-detects the emulators, counting the `.exe` files chosen so far, and moves the focus off
    /// anything that went: onto the first Choose .exe left, or on.
    fn check(&mut self, model: &Model, ctx: &mut Ctx) {
        self.seen = snapshot(model);
        ctx.emulators = (ctx.detect_emulators)().with_chosen(&model.chosen.borrow());
        self.missing = self.rows.iter().map(|e| !ctx.emulators.has(e)).collect();
        let missing: Vec<&Emulator> = self.missing_rows().map(|i| self.rows[i]).collect();
        self.command = emulators::winget_install_all(&missing);
        self.pages = missing
            .iter()
            .filter_map(|e| match e.windows_install {
                WindowsInstall::Page(url) => Some((*e, url)),
                WindowsInstall::Winget(_) => None,
            })
            .collect();
        if !self.stops().contains(&self.focus) && !self.focus_on_side_stop() {
            self.focus = self.stops()[0];
        }
    }

    fn missing_rows(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.rows.len()).filter(|&i| self.missing[i])
    }

    /// What Up and Down go through, in order: each missing row's Choose .exe, Copy, each
    /// download page, then Continue. Check again and Back are beside Copy and Continue.
    fn stops(&self) -> Vec<Focus> {
        let mut stops: Vec<Focus> = self.missing_rows().map(Focus::Choose).collect();
        if self.command.is_some() {
            stops.push(Focus::Copy);
        }
        stops.extend((0..self.pages.len()).map(Focus::Page));
        stops.push(Focus::Button(Button::Continue));
        stops
    }

    /// Whether the focus is on Check again or Back, which are still there when their neighbour
    /// in [`Self::stops`] is.
    fn focus_on_side_stop(&self) -> bool {
        match self.focus {
            Focus::Check => self.command.is_some(),
            Focus::Button(Button::Back) => true,
            _ => false,
        }
    }

    fn move_focus(&mut self, action: Action) {
        let stops = self.stops();
        let along = match self.focus {
            Focus::Check => Focus::Copy,
            Focus::Button(Button::Back) => Focus::Button(Button::Continue),
            focus => focus,
        };
        let at = stops.iter().position(|&s| s == along).unwrap_or(0);
        self.focus = match (self.focus, action) {
            (Focus::Copy, Action::Right) => Focus::Check,
            (Focus::Check, Action::Left) => Focus::Copy,
            (Focus::Page(i), Action::Right) if i + 1 < self.pages.len() => Focus::Page(i + 1),
            (Focus::Page(i), Action::Left) if i > 0 => Focus::Page(i - 1),
            (Focus::Button(Button::Continue), Action::Left) => Focus::Button(Button::Back),
            (Focus::Button(Button::Back), Action::Right) => Focus::Button(Button::Continue),
            (_, Action::Up) if at > 0 => stops[at - 1],
            (_, Action::Down) if at + 1 < stops.len() => stops[at + 1],
            (focus, _) => focus,
        };
    }

    fn confirm(&mut self, model: &Model, ctx: &mut Ctx) -> Go {
        match self.focus {
            Focus::Choose(i) => {
                let emulator = self.rows[i];
                let chosen = model.chosen.clone();
                let start = chosen
                    .borrow()
                    .get(emulator.key)
                    .and_then(|p| p.parent().map(PathBuf::from));
                let on_choose = Box::new(move |_: &mut Ctx, exe: PathBuf| {
                    chosen.borrow_mut().insert(emulator.key, exe);
                });
                self.browser = Some((ctx.choose_exe)(emulator, start, true, on_choose));
            }
            Focus::Copy => {
                if let Some(command) = &self.command {
                    ctx.clipboard_request = Some(command.clone());
                    self.copied = Some(Instant::now());
                }
            }
            Focus::Check => self.check(model, ctx),
            Focus::Page(i) => {
                if let Some((_, url)) = self.pages.get(i) {
                    ctx.open_url_request = Some((*url).to_owned());
                }
            }
            Focus::Button(Button::Back) => return Go::Back,
            Focus::Button(Button::Continue) => return Go::Next,
        }
        Go::Stay
    }

    fn draw_row(&mut self, gfx: &mut Gfx<'_>, layout: &Layout, i: usize, ctx: &Ctx) {
        let r = layout.row(i);
        let emulator = self.rows[i];
        let focused = self.focus == Focus::Choose(i);
        gfx.fill_round_rect(r, 14.0, if focused { SURFACE2 } else { SURFACE });
        gfx.stroke_round_rect(r, 14.0, 1.0, LINE);
        if focused {
            gfx.fill_round_rect(Rect::new(r.x, r.y + r.h / 2.0 - 20.0, 5.0, 40.0), 2.0, ACCENT);
        }
        let (top, under) = (r.y + r.h / 2.0 - 8.0, r.y + r.h / 2.0 + 22.0);
        let name = TextStyle::new(24, Weight::Bold, TEXT);
        let name_w = gfx.text(r.x + 28.0, top, emulator.name, name, Align::Left);
        let systems = TextStyle::new(17, Weight::Medium, MUTED);
        gfx.text(
            r.x + 28.0,
            under,
            &emulators::systems_of(emulator),
            systems,
            Align::Left,
        );
        let right = r.x + r.w - 28.0;
        if !self.missing[i] {
            let found = TextStyle::new(18, Weight::SemiBold, SUCCESS);
            let w = gfx.text(right, top, "Found", found, Align::Right);
            tick(gfx, right - w - 30.0, top - 14.0, SUCCESS);
            if let Some(location) = ctx.emulators.location(emulator) {
                let path = TextStyle::new(16, Weight::Medium, MUTED);
                let room = r.w - 28.0 - name_w - 80.0;
                let shown = trim_start(&location.exe.to_string_lossy(), room, |s| gfx.measure(s, path));
                gfx.text(right, under, &shown, path, Align::Right);
            }
            return;
        }
        let b = layout.choose(i);
        if focused {
            gfx.stroke_round_rect(b.inset(-5.0), 15.0, 3.0, super::WHITE);
            gfx.fill_round_rect(b, 10.0, ACCENT);
        } else {
            gfx.fill_round_rect(b, 10.0, SURFACE2);
            gfx.stroke_round_rect(b, 10.0, 2.0, LINE);
        }
        let label = TextStyle::new(19, Weight::Bold, TEXT);
        gfx.text(
            b.x + b.w / 2.0,
            b.y + b.h / 2.0 + 7.0,
            "Choose .exe",
            label,
            Align::Centre,
        );
        let warn = TextStyle::new(18, Weight::SemiBold, WARN);
        gfx.text(b.x - 16.0, top, "Not found", warn, Align::Right);
        self.hits.push((b, Focus::Choose(i)));
    }

    /// The download page buttons from `y`, each with its page beside it, as many to a line as fit
    /// in `width`.
    fn draw_pages(&mut self, gfx: &mut Gfx<'_>, y: f32, width: f32) {
        let label = TextStyle::new(22, Weight::Bold, TEXT);
        let url_style = TextStyle::new(18, Weight::Medium, MUTED);
        let (mut x, mut y) = (M, y);
        for (i, (emulator, url)) in self.pages.clone().into_iter().enumerate() {
            let text = format!("{}: download page", emulator.name);
            let url = short_url(url);
            let pill_w = 28.0 + 26.0 + gfx.measure(&text, label) + 28.0;
            let item_w = pill_w + 20.0 + gfx.measure(url, url_style);
            if x > M && x + item_w > M + width {
                (x, y) = (M, y + PILL_H + PILL_GAP_Y);
            }
            let r = Rect::new(x, y, pill_w, PILL_H);
            let focused = self.focus == Focus::Page(i);
            if focused {
                gfx.stroke_round_rect(r.inset(-6.0), PILL_H / 2.0 + 6.0, 3.0, super::WHITE);
            }
            gfx.fill_round_rect(r, PILL_H / 2.0, SURFACE.alpha(0.85));
            gfx.stroke_round_rect(r, PILL_H / 2.0, 2.0, LINE);
            download_icon(gfx, x + 36.0, y + PILL_H / 2.0, MUTED);
            gfx.text(x + 54.0, y + 40.0, &text, label, Align::Left);
            gfx.text(x + pill_w + 20.0, y + 40.0, url, url_style, Align::Left);
            self.hits.push((r, Focus::Page(i)));
            x += item_w + PILL_GAP_X;
        }
    }
}

impl Step for FindEmulators {
    fn title(&self) -> &'static str {
        "Set up the emulators"
    }

    fn explanation(&self) -> &'static str {
        "These Systems run in their own emulator. Game37 looks for each one; pick its program if it \
         isn't found."
    }

    fn applies(&self, model: &Model) -> bool {
        !ticked_emulators(model).is_empty()
    }

    fn enter(&mut self, model: &Model, ctx: &mut Ctx) {
        self.rows = ticked_emulators(model);
        self.copied = None;
        self.check(model, ctx);
        self.focus = self.stops()[0];
    }

    /// A choice made in the browser re-checks once Setup is back on top.
    fn tick(&mut self, model: &Model, ctx: &mut Ctx) {
        if snapshot(model) != self.seen {
            self.check(model, ctx);
        }
    }

    fn can_continue(&self, _: &Model) -> bool {
        true
    }

    fn handle(&mut self, input: Input, model: &mut Model, ctx: &mut Ctx) -> Go {
        match input {
            Input::Action(Action::Options) => self.check(model, ctx),
            Input::Action(Action::Confirm) => return self.confirm(model, ctx),
            Input::Action(action) if action.is_direction() => self.move_focus(action),
            Input::Pointer(Pointer::Move { x, y }) => {
                if let Some(focus) = hit(&self.hits, x, y) {
                    self.focus = focus;
                }
            }
            Input::Pointer(Pointer::Click { x, y }) => {
                if let Some(focus) = hit(&self.hits, x, y) {
                    self.focus = focus;
                    return self.confirm(model, ctx);
                }
            }
            _ => {}
        }
        Go::Stay
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, _model: &Model, ctx: &Ctx) {
        self.hits.clear();
        let layout = Layout::new(self.rows.len());
        for i in 0..self.rows.len() {
            self.draw_row(gfx, &layout, i, ctx);
        }
        let width = layout.width(self.rows.len());
        let y = layout.bottom() + 36.0;
        if self.command.is_none() && self.pages.is_empty() {
            tick(gfx, M, y - 14.0, SUCCESS);
            let done = TextStyle::new(20, Weight::SemiBold, TEXT);
            gfx.text(M + 32.0, y, "Every emulator is found.", done, Align::Left);
            return;
        }
        let mut pages_y = y - 14.0;
        if let Some(command) = self.command.clone() {
            let label = TextStyle::new(14, Weight::Bold, MUTED);
            gfx.text(
                M,
                y,
                "PASTE INTO POWERSHELL OR TERMINAL FOR THE REST",
                label,
                Align::Left,
            );
            let copied = self.copied.is_some_and(|at| at.elapsed() < FEEDBACK);
            let focus = (self.focus == Focus::Copy, self.focus == Focus::Check);
            let (box_h, copy, check) = command_box(gfx, y + 20.0, width, &command, copied, focus);
            self.hits.push((copy, Focus::Copy));
            self.hits.push((check, Focus::Check));
            pages_y = y + 20.0 + box_h + 32.0;
        }
        self.draw_pages(gfx, pages_y, width);
    }

    fn hints(&self, _: &Model) -> Vec<Hint> {
        let a = match self.focus {
            Focus::Choose(_) => "Choose .exe",
            Focus::Copy => "Copy",
            Focus::Check => "Check again",
            Focus::Page(_) => "Open page",
            Focus::Button(Button::Back) => "Back",
            Focus::Button(Button::Continue) => "Continue",
        };
        let mut hints = vec![Hint::new(&[Action::Confirm], a)];
        if self.missing.contains(&true) {
            hints.push(Hint::new(&[Action::Options], "Check again"));
        }
        hints
    }

    fn focused_button(&self) -> Option<Button> {
        match self.focus {
            Focus::Button(b) => Some(b),
            _ => None,
        }
    }

    fn take_screen(&mut self) -> Option<Box<dyn Screen>> {
        self.browser.take()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use super::super::Setup;
    use super::super::tests::act;
    use super::*;
    use crate::config;
    use crate::emulators::{CEMU, Emulators, How, Places};
    use crate::screens::choose_exe::tests::{answer, picking};
    use crate::systems::SYSTEMS;
    use crate::ui::nav::Transition;

    /// Setup on step 1 with `ids` ticked, Games Root `root`, and Windows' step 2 on any OS.
    fn ticking(root: &Path, ids: &[&str]) -> Setup {
        let mut setup = Setup::new(root.to_path_buf());
        setup.steps[1] = Box::new(FindEmulators::new());
        for (i, system) in SYSTEMS.iter().enumerate() {
            setup.model.ticked[i] = ids.contains(&system.id);
        }
        setup
    }

    /// A Ctx with its Config at `config`, detecting as Windows does with nothing installed: only
    /// the `.exe` files the Config holds.
    fn windows_ctx(config: &Path) -> Ctx {
        let mut ctx = Ctx::empty();
        ctx.config_path = config.to_path_buf();
        let path = config.to_path_buf();
        ctx.detect_emulators = Box::new(move || {
            let chosen = config::load_emulator_exes(&path).unwrap();
            Emulators::locate_all(&chosen, &Places::default())
        });
        ctx
    }

    fn touch(path: &Path) -> PathBuf {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
        path.to_path_buf()
    }

    fn hint(setup: &Setup, text: &str) -> bool {
        setup.hints().iter().any(|h| h.text == text)
    }

    #[test]
    fn a_chosen_exe_stays_in_memory_until_finish_writes_it_after_the_config() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        let exe = touch(&dir.path().join("Cemu/Cemu.exe"));
        let mut ctx = windows_ctx(&config);
        let asked = picking(&mut ctx, exe.clone());
        let mut setup = ticking(&dir.path().join("Games"), &["wiiu"]);
        act(&mut setup, &mut ctx, Action::Settings);
        assert_eq!(setup.step(), 2);
        assert!(hint(&setup, "Choose .exe"), "Cemu's Choose .exe has the focus");

        let opened = act(&mut setup, &mut ctx, Action::Confirm);
        answer(opened, &mut ctx, Action::Confirm);
        assert_eq!(*asked.borrow(), vec![("cemu", None, true)]);
        setup.tick(&mut ctx);
        let location = ctx.emulators.location(&CEMU).expect("Cemu counts as chosen");
        assert_eq!((&location.exe, location.how), (&exe, How::Chosen));
        assert!(!hint(&setup, "Check again"), "nothing is missing now");
        assert!(!config.exists(), "no Config, so Setup is not done");

        act(&mut setup, &mut ctx, Action::Settings);
        act(&mut setup, &mut ctx, Action::Confirm);
        act(&mut setup, &mut ctx, Action::Settings);
        assert_eq!(setup.step(), 5, "the Cores step is passed over");
        assert!(!config.exists());
        act(&mut setup, &mut ctx, Action::Settings);
        assert!(ctx.setup_finished.is_some(), "{:?}", setup.error);
        assert_eq!(
            config::load_emulator_exes(&config).unwrap(),
            HashMap::from([("cemu".to_owned(), exe)])
        );
        assert!(config::load(&config).unwrap().is_some());
    }

    #[test]
    fn a_cancelled_setup_or_browser_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        let exe = touch(&dir.path().join("Dolphin/Dolphin.exe"));
        let mut ctx = windows_ctx(&config);
        picking(&mut ctx, exe);
        let mut setup = ticking(&dir.path().join("Games"), &["gc", "wii"]);
        act(&mut setup, &mut ctx, Action::Settings);

        let opened = act(&mut setup, &mut ctx, Action::Confirm);
        answer(opened, &mut ctx, Action::Back);
        setup.tick(&mut ctx);
        assert!(hint(&setup, "Check again"), "Dolphin is still missing");

        let opened = act(&mut setup, &mut ctx, Action::Confirm);
        answer(opened, &mut ctx, Action::Confirm);
        setup.tick(&mut ctx);
        assert!(!hint(&setup, "Check again"));
        act(&mut setup, &mut ctx, Action::Back);
        assert_eq!(setup.step(), 1);
        assert!(matches!(
            act(&mut setup, &mut ctx, Action::Back),
            Transition::Quit
        ));
        assert!(
            !config.exists(),
            "quitting Setup writes neither the Config nor the choice"
        );
    }

    #[test]
    fn the_winget_line_has_the_missing_packages_and_each_page_only_one_a_page_button() {
        let mut ctx = Ctx::empty();
        ctx.detect_emulators = Box::new(|| Emulators::from_installed(["net.pcsx2.PCSX2"]));
        let dir = tempfile::tempdir().unwrap();
        let mut setup = ticking(dir.path(), &["gc", "wii", "3ds", "wiiu", "ps2", "ps3"]);
        act(&mut setup, &mut ctx, Action::Settings);
        // Dolphin, Azahar, Cemu, PCSX2 (found, so passed over) and RPCS3, then Copy.
        for _ in 0..4 {
            act(&mut setup, &mut ctx, Action::Down);
        }
        assert!(hint(&setup, "Copy"));
        act(&mut setup, &mut ctx, Action::Confirm);
        assert_eq!(
            ctx.clipboard_request.as_deref(),
            Some("winget install -e --id AzaharEmu.Azahar; winget install -e --id Cemu.Cemu")
        );

        act(&mut setup, &mut ctx, Action::Down);
        assert!(hint(&setup, "Open page"));
        act(&mut setup, &mut ctx, Action::Confirm);
        assert_eq!(
            ctx.open_url_request.as_deref(),
            Some("https://dolphin-emu.org/download/")
        );
        act(&mut setup, &mut ctx, Action::Right);
        act(&mut setup, &mut ctx, Action::Confirm);
        assert_eq!(
            ctx.open_url_request.as_deref(),
            Some("https://rpcs3.net/download")
        );
        act(&mut setup, &mut ctx, Action::Right);
        act(&mut setup, &mut ctx, Action::Down);
        assert!(
            hint(&setup, "Continue"),
            "the pages end there; Down goes to Continue"
        );
        assert_eq!(setup.step(), 2);
    }

    #[test]
    fn with_everything_found_there_is_nothing_to_choose_copy_or_open() {
        let mut ctx = Ctx::empty();
        ctx.detect_emulators = Box::new(|| Emulators::from_installed(["org.DolphinEmu.dolphin-emu"]));
        let dir = tempfile::tempdir().unwrap();
        let mut setup = ticking(dir.path(), &["gc"]);
        act(&mut setup, &mut ctx, Action::Settings);
        assert!(hint(&setup, "Continue"));
        assert!(!hint(&setup, "Check again"));
        act(&mut setup, &mut ctx, Action::Confirm);
        assert_eq!(setup.step(), 3);
    }

    #[test]
    fn a_download_page_shows_without_its_scheme_and_slash() {
        assert_eq!(
            short_url("https://dolphin-emu.org/download/"),
            "dolphin-emu.org/download"
        );
        assert_eq!(short_url("https://ryujinx.app/"), "ryujinx.app");
    }
}
