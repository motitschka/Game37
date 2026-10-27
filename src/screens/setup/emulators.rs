//! Step 2, "Install the emulators": the Standalone Emulator each ticked External System needs and
//! whether it is installed, with the command that installs the missing ones, after
//! `screen_setup_emulators` and `command_box` in `design/generate_mockups.py`. It is passed over
//! when no External System is ticked. Linux's: Windows has [`super::find_emulators`].

use std::time::{Duration, Instant};

use super::{Button, CONTENT_Y, Go, Model, SETUP_W, Step, system_chip, tick};
use crate::ctx::Ctx;
use crate::emulators::{self, Emulator};
use crate::systems::System;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer};
use crate::ui::nav::Hint;
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, Colour, DIM, LINE, MUTED, SUCCESS, SURFACE, SURFACE2, TEXT, WARN};
use crate::ui::{M, Rect, W, hit};

/// How long "Copied" shows on the Copy button, as on the missing-emulator notice (US-39).
const FEEDBACK: Duration = Duration::from_secs(2);
/// Between rows, and between the two columns of them.
const GAP: f32 = 16.0;
const BOX_H: f32 = 48.0;
/// The spacing of the command's lines once it wraps.
const LINE_H: f32 = 28.0;
const COPY_W: f32 = 130.0;
const CHECK_W: f32 = 168.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Copy,
    Check,
    Button(Button),
}

pub struct Install {
    focus: Focus,
    /// The command that installs the missing emulators, as last checked; `None` when none are.
    command: Option<String>,
    /// When Copy was last pressed, for its "Copied" feedback.
    copied: Option<Instant>,
    /// Where Copy and Check again were drawn, for the mouse.
    hits: Vec<(Rect, Focus)>,
}

impl Install {
    pub fn new() -> Install {
        Install {
            focus: Focus::Copy,
            command: None,
            copied: None,
            hits: Vec::new(),
        }
    }

    /// Re-detects the installed emulators, which `app.rs` doesn't do without a Config.
    fn check(&mut self, model: &Model, ctx: &mut Ctx) {
        ctx.emulators = (ctx.detect_emulators)();
        self.command = install_command(model, &ctx.emulators);
        if self.command.is_none() && matches!(self.focus, Focus::Copy | Focus::Check) {
            self.focus = Focus::Button(Button::Continue);
        }
    }

    fn move_focus(&mut self, action: Action) {
        self.focus = match (self.focus, action) {
            (Focus::Copy, Action::Right) => Focus::Check,
            (Focus::Check, Action::Left) => Focus::Copy,
            (Focus::Copy | Focus::Check, Action::Down) => Focus::Button(Button::Continue),
            (Focus::Button(Button::Back), Action::Right) => Focus::Button(Button::Continue),
            (Focus::Button(Button::Continue), Action::Left) => Focus::Button(Button::Back),
            (Focus::Button(_), Action::Up) if self.command.is_some() => Focus::Copy,
            (focus, _) => focus,
        };
    }

    fn confirm(&mut self, model: &Model, ctx: &mut Ctx) -> Go {
        match self.focus {
            Focus::Copy => {
                if let Some(command) = &self.command {
                    ctx.clipboard_request = Some(command.clone());
                    self.copied = Some(Instant::now());
                }
            }
            Focus::Check => self.check(model, ctx),
            Focus::Button(Button::Back) => return Go::Back,
            Focus::Button(Button::Continue) => return Go::Next,
        }
        Go::Stay
    }

    /// The command box at `y`, as [`command_box`] draws it. Returns its height.
    fn draw_command(&mut self, gfx: &mut Gfx<'_>, y: f32, width: f32, command: &str) -> f32 {
        let copied = self.copied.is_some_and(|at| at.elapsed() < FEEDBACK);
        let focus = (self.focus == Focus::Copy, self.focus == Focus::Check);
        let (box_h, copy, check) = command_box(gfx, y, width, command, copied, focus);
        self.hits = vec![(copy, Focus::Copy), (check, Focus::Check)];
        box_h
    }
}

/// The command box at `y`, its text wrapped to leave room for Copy and Check again beside it
/// within `width`, after `command_box`; `focus` is whether Copy and whether Check again has the
/// focus. Returns its height and where Copy and Check again were drawn.
pub(super) fn command_box(
    gfx: &mut Gfx<'_>,
    y: f32,
    width: f32,
    command: &str,
    copied: bool,
    focus: (bool, bool),
) -> (f32, Rect, Rect) {
    let mono = TextStyle::new(18, Weight::Medium, TEXT);
    let lines = gfx.wrap(
        command,
        mono,
        width - 42.0 - 24.0 - 20.0 - COPY_W - 20.0 - CHECK_W,
    );
    let text_w = lines
        .iter()
        .map(|line| gfx.measure(line, mono))
        .fold(0.0, f32::max);
    let box_w = 42.0 + text_w + 24.0;
    let box_h = BOX_H + (lines.len() - 1) as f32 * LINE_H;
    gfx.fill_round_rect(
        Rect::new(M, y, box_w, box_h),
        10.0,
        Colour::hex(0x000000).alpha(0.4),
    );
    let prompt = TextStyle::new(18, Weight::SemiBold, DIM);
    gfx.text(M + 20.0, y + 31.0, "$", prompt, Align::Left);
    for (i, line) in lines.iter().enumerate() {
        gfx.text(M + 42.0, y + 31.0 + i as f32 * LINE_H, line, mono, Align::Left);
    }

    let focus_ring = |gfx: &mut Gfx<'_>, r: Rect, on: bool| {
        if on {
            gfx.stroke_round_rect(r.inset(-5.0), 15.0, 3.0, ACCENT);
        }
    };
    let label = TextStyle::new(18, Weight::SemiBold, TEXT);
    let copy = Rect::new(M + box_w + 20.0, y, COPY_W, BOX_H);
    focus_ring(gfx, copy, focus.0);
    gfx.fill_round_rect(copy, 10.0, SURFACE2);
    // Two overlapping sheets.
    gfx.stroke_round_rect(Rect::new(copy.x + 22.0, y + 13.0, 14.0, 17.0), 3.0, 2.0, TEXT);
    let front = Rect::new(copy.x + 27.0, y + 18.0, 14.0, 17.0);
    gfx.fill_round_rect(front, 3.0, SURFACE2);
    gfx.stroke_round_rect(front, 3.0, 2.0, TEXT);
    let copy_label = if copied { "Copied" } else { "Copy" };
    gfx.text(copy.x + 56.0, y + 31.0, copy_label, label, Align::Left);

    let check = Rect::new(copy.x + COPY_W + 20.0, y, CHECK_W, BOX_H);
    focus_ring(gfx, check, focus.1);
    gfx.stroke_round_rect(check, 10.0, 2.0, LINE);
    refresh_icon(gfx, check.x + 28.0, y + 22.7);
    gfx.text(check.x + 52.0, y + 31.0, "Check again", label, Align::Left);
    (box_h, copy, check)
}

/// The ticked External Systems with their Standalone Emulators, in System Order.
fn external(model: &Model) -> impl Iterator<Item = (&'static System, &'static Emulator)> + '_ {
    model
        .ticked()
        .filter_map(|system| Some((system, emulators::for_system(system)?)))
}

/// The command that installs the ticked Systems' missing emulators, each once (GameCube and Wii
/// share Dolphin) and in System Order; `None` when none are missing. Windows has its own step,
/// [`super::find_emulators`].
fn install_command(model: &Model, installed: &emulators::Emulators) -> Option<String> {
    let missing: Vec<&Emulator> = external(model)
        .filter_map(|(s, _)| installed.missing(s))
        .collect();
    emulators::install_all(&missing)
}

/// Where the rows go for `n` ticked External Systems: the mockup's rows for up to three, shorter
/// ones for four, and two columns of the shorter ones, down the columns, for more, so all eight
/// leave room for the command below.
struct Layout {
    per_col: usize,
    cols: usize,
    row_w: f32,
    row_h: f32,
    /// Where the Flatpak id starts, from the row's left edge.
    id_x: f32,
}

impl Layout {
    fn new(n: usize) -> Layout {
        match n {
            0..=4 => Layout {
                per_col: n,
                cols: 1,
                row_w: SETUP_W,
                row_h: if n <= 3 { 96.0 } else { 80.0 },
                id_x: 440.0,
            },
            n => Layout {
                per_col: n.div_ceil(2),
                cols: 2,
                row_w: (W - 2.0 * M - GAP) / 2.0,
                row_h: 80.0,
                id_x: 300.0,
            },
        }
    }

    fn row(&self, i: usize) -> Rect {
        let (col, row) = (i / self.per_col, i % self.per_col);
        let x = M + col as f32 * (self.row_w + GAP);
        Rect::new(
            x,
            CONTENT_Y + row as f32 * (self.row_h + GAP),
            self.row_w,
            self.row_h,
        )
    }

    fn width(&self) -> f32 {
        self.cols as f32 * self.row_w + (self.cols - 1) as f32 * GAP
    }

    fn bottom(&self) -> f32 {
        CONTENT_Y + self.per_col as f32 * (self.row_h + GAP) - GAP
    }
}

fn draw_row(gfx: &mut Gfx<'_>, r: Rect, id_x: f32, system: &System, emulator: &Emulator, installed: bool) {
    gfx.fill_round_rect(r, 14.0, SURFACE);
    gfx.stroke_round_rect(r, 14.0, 1.0, LINE);
    let mid = r.y + r.h / 2.0;
    system_chip(gfx, r.x + 24.0, mid - 26.0, 52.0, system);
    let name = TextStyle::new(24, Weight::Bold, TEXT);
    gfx.text(r.x + 100.0, mid - 4.0, system.name, name, Align::Left);
    let muted = TextStyle::new(18, Weight::Medium, MUTED);
    gfx.text(r.x + 100.0, mid + 26.0, emulator.name, muted, Align::Left);
    gfx.text(r.x + id_x, mid + 8.0, emulator.flatpak_id, muted, Align::Left);
    let right = r.x + r.w - 28.0;
    let (state, colour) = if installed {
        ("Installed", SUCCESS)
    } else {
        ("Not installed", WARN)
    };
    let w = gfx.text(
        right,
        mid + 8.0,
        state,
        TextStyle::new(20, Weight::SemiBold, colour),
        Align::Right,
    );
    if installed {
        tick(gfx, right - w - 32.0, mid - 6.0, SUCCESS);
    } else {
        gfx.fill_circle(right - w - 18.0, mid + 1.0, 6.0, WARN);
    }
}

/// Check again's circular arrow, about 18 px across, around (`cx`, `cy`), after `command_box`.
fn refresh_icon(gfx: &mut Gfx<'_>, cx: f32, cy: f32) {
    // A 310° arc, open at the top right, the arrow head at its upper end.
    let point = |deg: f32| {
        let a = deg.to_radians();
        (cx + 9.0 * a.cos(), cy + 9.0 * a.sin())
    };
    let steps = 24;
    let mut last = point(-48.0);
    for i in 1..=steps {
        let next = point(-48.0 - 310.0 * i as f32 / steps as f32);
        gfx.line(last.0, last.1, next.0, next.1, 2.4, TEXT);
        gfx.fill_circle(next.0, next.1, 1.2, TEXT);
        last = next;
    }
    let head = [(cx + 2.0, cy - 11.7), (cx + 7.0, cy - 6.7), (cx + 1.0, cy - 3.7)];
    gfx.line(head[0].0, head[0].1, head[1].0, head[1].1, 2.4, TEXT);
    gfx.line(head[1].0, head[1].1, head[2].0, head[2].1, 2.4, TEXT);
    for (x, y) in head {
        gfx.fill_circle(x, y, 1.2, TEXT);
    }
}

impl Step for Install {
    fn title(&self) -> &'static str {
        "Install the emulators"
    }

    fn explanation(&self) -> &'static str {
        "These Systems run in their own emulator, installed with Flatpak."
    }

    fn applies(&self, model: &Model) -> bool {
        external(model).next().is_some()
    }

    fn enter(&mut self, model: &Model, ctx: &mut Ctx) {
        self.copied = None;
        self.check(model, ctx);
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

    fn draw(&mut self, gfx: &mut Gfx<'_>, model: &Model, ctx: &Ctx) {
        let layout = Layout::new(external(model).count());
        for (i, (system, emulator)) in external(model).enumerate() {
            let installed = ctx.emulators.missing(system).is_none();
            draw_row(gfx, layout.row(i), layout.id_x, system, emulator, installed);
        }

        let y = layout.bottom() + 60.0;
        self.hits.clear();
        let Some(command) = self.command.clone() else {
            tick(gfx, M, y - 14.0, SUCCESS);
            let done = TextStyle::new(20, Weight::SemiBold, TEXT);
            gfx.text(M + 32.0, y, "Everything is installed.", done, Align::Left);
            return;
        };
        let label = TextStyle::new(14, Weight::Bold, MUTED);
        gfx.text(M, y, "INSTALL THE MISSING ONES", label, Align::Left);
        let box_h = self.draw_command(gfx, y + 20.0, layout.width(), &command);
        let note = "You can continue now and install them later. Missing emulators show on the Carousel.";
        let muted = TextStyle::new(18, Weight::Medium, MUTED);
        gfx.text(M, y + 20.0 + box_h + 56.0, note, muted, Align::Left);
    }

    fn hints(&self, _: &Model) -> Vec<Hint> {
        let a = match self.focus {
            Focus::Copy => "Copy",
            Focus::Check => "Check again",
            Focus::Button(Button::Back) => "Back",
            Focus::Button(Button::Continue) => "Continue",
        };
        let mut hints = vec![Hint::new(&[Action::Confirm], a)];
        if self.command.is_some() {
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
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use super::super::Setup;
    use super::super::tests::act;
    use super::*;
    use crate::emulators::Emulators;
    use crate::systems::SYSTEMS;
    use crate::ui::nav::{Screen, Transition};

    /// Setup on step 1 with `ids` ticked, and Linux's step 2 on any OS.
    fn ticking(ids: &[&str]) -> Setup {
        let mut setup = Setup::new(PathBuf::from("/g"));
        setup.steps[1] = Box::new(Install::new());
        for (i, system) in SYSTEMS.iter().enumerate() {
            setup.model.ticked[i] = ids.contains(&system.id);
        }
        setup
    }

    /// A Ctx whose detector finds `installed` and counts how often it was asked, never running
    /// `flatpak`.
    fn detecting(installed: &'static [&'static str]) -> (Ctx, Rc<Cell<usize>>) {
        let mut ctx = Ctx::empty();
        let calls = Rc::new(Cell::new(0));
        let counter = Rc::clone(&calls);
        ctx.detect_emulators = Box::new(move || {
            counter.set(counter.get() + 1);
            Emulators::from_installed(installed.iter().copied())
        });
        (ctx, calls)
    }

    #[test]
    fn the_step_is_passed_over_both_ways_without_an_external_system() {
        let (mut ctx, calls) = detecting(&[]);
        let mut setup = ticking(&["nes", "psp"]);
        act(&mut setup, &mut ctx, Action::Settings);
        assert_eq!(setup.step(), 3, "on to Make your folders");
        act(&mut setup, &mut ctx, Action::Back);
        assert_eq!(setup.step(), 1);
        assert_eq!(calls.get(), 0, "never shown, so never detected");

        let mut setup = ticking(&["nes", "gc"]);
        act(&mut setup, &mut ctx, Action::Settings);
        assert_eq!(setup.step(), 2, "GameCube needs Dolphin");
    }

    #[test]
    #[cfg(unix)] // The Flatpak command is Linux's; emulators.rs pins the Windows hints.
    fn the_command_lists_each_missing_emulator_once_in_system_order() {
        let setup = ticking(&["nes", "gc", "wii", "3ds", "switch", "ps2"]);
        let installed = Emulators::from_installed(["org.azahar_emu.Azahar"]);
        assert_eq!(
            install_command(&setup.model, &installed).as_deref(),
            Some(
                "flatpak install flathub org.DolphinEmu.dolphin-emu io.github.ryubing.Ryujinx net.pcsx2.PCSX2"
            )
        );
    }

    #[test]
    fn with_nothing_missing_there_is_no_command() {
        let setup = ticking(&["gc", "ps2"]);
        let installed = Emulators::from_installed(["org.DolphinEmu.dolphin-emu", "net.pcsx2.PCSX2"]);
        assert_eq!(install_command(&setup.model, &installed), None);

        let (mut ctx, _) = detecting(&["org.DolphinEmu.dolphin-emu", "net.pcsx2.PCSX2"]);
        let mut setup = ticking(&["gc", "ps2"]);
        act(&mut setup, &mut ctx, Action::Settings);
        assert!(!setup.hints().iter().any(|h| h.text == "Check again"));
        assert!(setup.hints().iter().any(|h| h.text == "Continue"));
        act(&mut setup, &mut ctx, Action::Confirm);
        assert_eq!(ctx.clipboard_request, None, "there is no Copy");
        assert_eq!(setup.step(), 3, "A is on Continue");
    }

    #[test]
    #[cfg(unix)] // The Flatpak command is Linux's; emulators.rs pins the Windows hints.
    fn copy_asks_for_the_command_on_the_clipboard() {
        let (mut ctx, _) = detecting(&["org.DolphinEmu.dolphin-emu"]);
        let mut setup = ticking(&["gc", "switch", "ps2"]);
        act(&mut setup, &mut ctx, Action::Settings);
        assert!(
            setup.hints().iter().any(|h| h.text == "Copy"),
            "Copy has focus first"
        );
        act(&mut setup, &mut ctx, Action::Confirm);
        assert_eq!(
            ctx.clipboard_request.as_deref(),
            Some("flatpak install flathub io.github.ryubing.Ryujinx net.pcsx2.PCSX2")
        );
        assert_eq!(setup.step(), 2);
    }

    #[test]
    fn entering_the_step_and_check_again_detect_the_emulators() {
        let (mut ctx, calls) = detecting(&["net.pcsx2.PCSX2"]);
        let mut setup = ticking(&["ps2", "ps3"]);
        act(&mut setup, &mut ctx, Action::Settings);
        assert_eq!(calls.get(), 1);
        assert_eq!(ctx.emulators, Emulators::from_installed(["net.pcsx2.PCSX2"]));
        act(&mut setup, &mut ctx, Action::Options);
        assert_eq!(calls.get(), 2, "Y");
        act(&mut setup, &mut ctx, Action::Right);
        act(&mut setup, &mut ctx, Action::Confirm);
        assert_eq!(calls.get(), 3, "A on Check again");
        assert_eq!(ctx.clipboard_request, None);
        act(&mut setup, &mut ctx, Action::Back);
        act(&mut setup, &mut ctx, Action::Settings);
        assert_eq!(calls.get(), 4, "entering it again");
    }

    #[test]
    fn continue_works_with_emulators_missing() {
        let (mut ctx, _) = detecting(&[]);
        let mut setup = ticking(&["wiiu"]);
        act(&mut setup, &mut ctx, Action::Settings);
        assert!(matches!(
            act(&mut setup, &mut ctx, Action::Settings),
            Transition::None
        ));
        assert_eq!(setup.step(), 3);

        act(&mut setup, &mut ctx, Action::Back);
        assert_eq!(setup.step(), 2);
        act(&mut setup, &mut ctx, Action::Down);
        act(&mut setup, &mut ctx, Action::Confirm);
        assert_eq!(setup.step(), 3, "A on the Continue button");
    }
}
