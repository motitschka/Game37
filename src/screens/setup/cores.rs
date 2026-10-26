//! First-run Setup step 4, "Download the Cores", after `screen_setup_cores` in
//! `design/generate_mockups.py`: one row per ticked Embedded System with its Core's download
//! state, and a note when a ticked System's Core needs a BIOS. Entering the step queues every
//! missing Core at once in a [`CoreQueue`], one download at a time; leaving it drops the queue,
//! which cancels the running download, and coming back queues what is still missing.

use super::{Button, CONTENT_Y, Go, Model, SETUP_W, Step, system_chip, tick};
use crate::cores::{self, CoreInfo};
use crate::ctx::Ctx;
use crate::screens::core_queue::{CoreQueue, Job, percent, progress_bar};
use crate::screens::home::tilde;
use crate::screens::settings::bios::ASK_FOR;
use crate::systems::System;
use crate::ui::cover::ellipsize;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer};
use crate::ui::nav::Hint;
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, BG, DIM, LINE, MUTED, SUCCESS, SURFACE, SURFACE2, TEXT, WARN};
use crate::ui::{M, Rect, hit};

const ROW_H: f32 = 84.0;
const PITCH: f32 = 96.0;
/// The most rows one column holds; more go into two.
const ONE_COLUMN: usize = 5;
const COL_GAP: f32 = 24.0;
/// The bar's left edge and width in one column.
const BAR_X: f32 = 520.0;
const BAR_W: f32 = 440.0;
/// The bar's left edge in two columns, where it sits under the state.
const BAR_X_TWO: f32 = 350.0;
const NOTE_H: f32 = 64.0;

/// One ticked Embedded System's row.
struct Row {
    system: &'static System,
    /// Its Core's index in the queue: Systems that share a Core share its state.
    core: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Row(usize),
    Button(Button),
}

pub struct Cores {
    rows: Vec<Row>,
    /// Each ticked Embedded System's Core once, built afresh on entering the step.
    queue: CoreQueue,
    /// Nothing while downloads run and none has failed.
    focus: Option<Focus>,
    /// Where the failed rows were drawn, for the mouse.
    hits: Vec<(Rect, usize)>,
}

impl Cores {
    pub fn new() -> Cores {
        Cores {
            rows: Vec::new(),
            queue: CoreQueue::new(Vec::new()),
            focus: None,
            hits: Vec::new(),
        }
    }

    fn failed(&self, row: usize) -> bool {
        self.queue.job(self.rows[row].core) == Job::Failed
    }

    fn failed_rows(&self) -> Vec<usize> {
        (0..self.rows.len()).filter(|&r| self.failed(r)).collect()
    }

    /// Keeps the focus on something that can take it: a failed row, or a button once something
    /// failed or nothing runs. While downloads run and none has failed, nothing has it.
    fn settle(&mut self) {
        let failed = self.failed_rows();
        self.focus = match self.focus {
            Some(Focus::Row(r)) if failed.contains(&r) => Some(Focus::Row(r)),
            Some(Focus::Button(b)) if !failed.is_empty() || !self.queue.running() => Some(Focus::Button(b)),
            _ => match failed.first() {
                Some(&r) => Some(Focus::Row(r)),
                None if self.queue.running() => None,
                None => Some(Focus::Button(Button::Continue)),
            },
        };
    }

    fn move_focus(&mut self, action: Action) {
        let failed = self.failed_rows();
        let at = |r| failed.iter().position(|&f| f == r).unwrap_or(0);
        self.focus = match (self.focus, action) {
            (Some(Focus::Row(r)), Action::Up) if at(r) > 0 => Some(Focus::Row(failed[at(r) - 1])),
            (Some(Focus::Row(r)), Action::Down) => match failed.get(at(r) + 1) {
                Some(&next) => Some(Focus::Row(next)),
                None => Some(Focus::Button(Button::Continue)),
            },
            (Some(Focus::Button(_)), Action::Up) => failed.last().map(|&r| Focus::Row(r)).or(self.focus),
            (Some(Focus::Button(Button::Back)), Action::Right) => Some(Focus::Button(Button::Continue)),
            (Some(Focus::Button(Button::Continue)), Action::Left) => Some(Focus::Button(Button::Back)),
            (focus, _) => focus,
        };
    }

    /// Queues the failed Core on `row` again.
    fn retry(&mut self, row: usize, ctx: &Ctx) {
        self.queue.enqueue(self.rows[row].core);
        self.queue.advance(ctx);
        self.settle();
    }

    fn confirm(&mut self, ctx: &Ctx) -> Go {
        match self.focus {
            Some(Focus::Row(r)) => self.retry(r, ctx),
            Some(Focus::Button(Button::Back)) => return Go::Back,
            Some(Focus::Button(Button::Continue)) => return Go::Next,
            None => {}
        }
        Go::Stay
    }

    /// How many rows each column holds.
    fn per_col(&self) -> usize {
        if self.rows.len() <= ONE_COLUMN {
            self.rows.len()
        } else {
            self.rows.len().div_ceil(2)
        }
    }

    /// Row `i`'s rectangle, down the columns.
    fn rect(&self, i: usize) -> Rect {
        let per_col = self.per_col().max(1);
        let (col, row) = (i / per_col, i % per_col);
        let w = if self.rows.len() <= ONE_COLUMN {
            SETUP_W
        } else {
            (SETUP_W - COL_GAP) / 2.0
        };
        Rect::new(
            M + col as f32 * (w + COL_GAP),
            CONTENT_Y + row as f32 * PITCH,
            w,
            ROW_H,
        )
    }

    fn draw_row(&self, gfx: &mut Gfx<'_>, i: usize, installed: bool) {
        let row = &self.rows[i];
        let r = self.rect(i);
        if self.focus == Some(Focus::Row(i)) {
            gfx.stroke_round_rect(r.inset(-6.0), 20.0, 4.0, ACCENT);
        }
        gfx.fill_round_rect(r, 14.0, SURFACE);
        gfx.stroke_round_rect(r, 14.0, 1.0, LINE);
        system_chip(gfx, r.x + 22.0, r.y + 20.0, 44.0, row.system);
        let name = TextStyle::new(22, Weight::Bold, TEXT);
        gfx.text(r.x + 90.0, r.y + 38.0, row.system.name, name, Align::Left);
        let sub = TextStyle::new(17, Weight::Medium, MUTED);
        gfx.text(
            r.x + 90.0,
            r.y + 64.0,
            &sub_line(self.core_of(i)),
            sub,
            Align::Left,
        );

        let job = self.queue.job(row.core);
        let right = r.x + r.w - 28.0;
        // One column puts the bar beside the state, as the mockup does; two have no room for that,
        // so the bar goes under the state, at the sub-line's height.
        let (bar, text_y) = if r.w == SETUP_W {
            (Rect::new(r.x + BAR_X, r.y + 37.0, BAR_W, 10.0), r.y + 49.0)
        } else {
            let x = r.x + BAR_X_TWO;
            (Rect::new(x, r.y + 54.0, right - x, 10.0), r.y + 38.0)
        };
        let text = state(job, installed);
        let (colour, weight, fill, check) = match job {
            Job::Idle if installed => (MUTED, Weight::SemiBold, None, true),
            Job::Done => (SUCCESS, Weight::SemiBold, Some((1.0, SUCCESS)), true),
            Job::Running(f) => (TEXT, Weight::Bold, Some((f.unwrap_or(0.0), ACCENT)), false),
            Job::Failed => (WARN, Weight::SemiBold, None, false),
            Job::Idle | Job::Waiting => (DIM, Weight::SemiBold, Some((0.0, ACCENT)), false),
        };
        if let Some((f, fill)) = fill {
            progress_bar(gfx, bar, f, SURFACE2, fill);
        }
        let style = TextStyle::new(19, weight, colour);
        let w = gfx.text(right, text_y, &text, style, Align::Right);
        if check {
            tick(gfx, right - w - 30.0, text_y - 14.0, colour);
        }
    }

    fn core_of(&self, i: usize) -> &'static CoreInfo {
        cores::for_system(self.rows[i].system).expect("a row's System is Embedded")
    }

    /// The BIOS note under the rows, when a ticked System's Core needs one.
    fn draw_note(&self, gfx: &mut Gfx<'_>, model: &Model) {
        let Some(text) = bios_note(model) else {
            return;
        };
        let y = CONTENT_Y + self.per_col() as f32 * PITCH + 20.0;
        let r = Rect::new(M, y, SETUP_W, NOTE_H);
        gfx.fill_round_rect(r, 14.0, WARN.alpha(0.1));
        gfx.stroke_round_rect(r, 14.0, 2.0, WARN.alpha(0.55));
        gfx.fill_circle(M + 34.0, y + 32.0, 15.0, WARN);
        let bang = TextStyle::new(19, Weight::ExtraBold, BG);
        gfx.text(M + 34.0, y + 39.0, "!", bang, Align::Centre);
        let style = TextStyle::new(19, Weight::Medium, WARN);
        let text = ellipsize(&text, SETUP_W - 64.0 - 28.0, |s| gfx.measure(s, style));
        gfx.text(M + 64.0, y + 39.0, &text, style, Align::Left);
    }
}

/// A row's sub-line: its Core, and PPSSPP's assets for PSP.
fn sub_line(core: &CoreInfo) -> String {
    if core.system_in_data_dir {
        format!("{} · plus {}'s assets", core.name, core.name)
    } else {
        core.name.to_owned()
    }
}

/// A row's state as the step words it: a running download shows only its percentage.
fn state(job: Job, installed: bool) -> String {
    match job {
        Job::Idle if installed => "Installed".to_owned(),
        Job::Idle => "Not downloaded".to_owned(),
        Job::Waiting => "Waiting".to_owned(),
        Job::Running(Some(f)) => format!("{}%", percent(f)),
        Job::Running(None) => "Downloading".to_owned(),
        Job::Failed => "Download failed · Retry".to_owned(),
        Job::Done => "Downloaded".to_owned(),
    }
}

/// What the BIOS note says, if a ticked System's Core needs a BIOS.
fn bios_note(model: &Model) -> Option<String> {
    let system = model
        .ticked()
        .find(|s| cores::for_system(s).is_some_and(|c| !c.required_bios.is_empty()))?;
    let dir = tilde(&model.games_root.join("bios"), dirs::home_dir().as_deref());
    Some(format!(
        "{} needs a BIOS file: put {ASK_FOR} in {dir}/.",
        system.name
    ))
}

impl Step for Cores {
    fn title(&self) -> &'static str {
        "Download the Cores"
    }

    fn explanation(&self) -> &'static str {
        "Embedded Systems play inside Game37 with a libretro Core from the libretro buildbot."
    }

    fn applies(&self, model: &Model) -> bool {
        model.ticked().any(|s| cores::for_system(s).is_some())
    }

    fn enter(&mut self, model: &Model, ctx: &mut Ctx) {
        let mut queued: Vec<&'static CoreInfo> = Vec::new();
        self.rows.clear();
        for system in model.ticked() {
            let Some(core) = cores::for_system(system) else {
                continue;
            };
            let at = match queued.iter().position(|&c| c == core) {
                Some(at) => at,
                None => {
                    queued.push(core);
                    queued.len() - 1
                }
            };
            self.rows.push(Row { system, core: at });
        }
        self.queue = CoreQueue::new(queued);
        let installed = self.queue.installed(&ctx.data_dir).to_vec();
        for (i, installed) in installed.into_iter().enumerate() {
            if !installed {
                self.queue.enqueue(i);
            }
        }
        self.queue.advance(ctx);
        self.focus = None;
        self.settle();
    }

    fn tick(&mut self, _: &Model, ctx: &mut Ctx) {
        self.queue.tick(ctx);
        self.settle();
    }

    fn leave(&mut self) {
        self.queue = CoreQueue::new(Vec::new());
        self.rows.clear();
        self.hits.clear();
    }

    fn can_continue(&self, _: &Model) -> bool {
        !self.queue.running()
    }

    fn busy(&self) -> bool {
        self.queue.running()
    }

    fn handle(&mut self, input: Input, _: &mut Model, ctx: &mut Ctx) -> Go {
        match input {
            Input::Action(Action::Confirm) => return self.confirm(ctx),
            Input::Action(action) if action.is_direction() => self.move_focus(action),
            Input::Pointer(Pointer::Move { x, y }) => {
                if let Some(r) = hit(&self.hits, x, y) {
                    self.focus = Some(Focus::Row(r));
                }
            }
            Input::Pointer(Pointer::Click { x, y }) => {
                if let Some(r) = hit(&self.hits, x, y) {
                    self.focus = Some(Focus::Row(r));
                    self.retry(r, ctx);
                }
            }
            _ => {}
        }
        Go::Stay
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, model: &Model, ctx: &Ctx) {
        let installed = self.queue.installed(&ctx.data_dir).to_vec();
        self.hits.clear();
        for i in 0..self.rows.len() {
            self.draw_row(gfx, i, installed[self.rows[i].core]);
            if self.failed(i) {
                self.hits.push((self.rect(i), i));
            }
        }
        self.draw_note(gfx, model);
    }

    fn hints(&self, _: &Model) -> Vec<Hint> {
        match self.focus {
            Some(Focus::Row(_)) => vec![Hint::new(&[Action::Confirm], "Retry")],
            _ => Vec::new(),
        }
    }

    fn focused_button(&self) -> Option<Button> {
        match self.focus {
            Some(Focus::Button(b)) => Some(b),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Setup;
    use super::super::tests::act;
    use super::*;
    use crate::cores::testing::manual;
    use crate::cores::{DownloadState, MESEN};
    use crate::systems::SYSTEMS;
    use crate::ui::input::{InputKind, glyph_label};
    use crate::ui::nav::Screen;
    use std::cell::RefCell;
    use std::fs;
    use std::path::Path;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    /// Every download started, in order: a Core's name, or "PPSSPP assets", with the state the
    /// test sets to move it on.
    type Started = Rc<RefCell<Vec<(String, Arc<Mutex<DownloadState>>)>>>;

    /// A Ctx on a temporary data dir whose downloads never run on their own.
    fn world() -> (tempfile::TempDir, Ctx, Started) {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = Ctx::empty();
        ctx.data_dir = dir.path().join("data");
        let started: Started = Rc::default();
        let log = Rc::clone(&started);
        ctx.start_download = Box::new(move |core, _| {
            let (download, state) = manual();
            log.borrow_mut().push((core.name.to_owned(), state));
            download
        });
        let log = Rc::clone(&started);
        ctx.start_assets_download = Box::new(move |_| {
            let (download, state) = manual();
            log.borrow_mut().push(("PPSSPP assets".to_owned(), state));
            download
        });
        (dir, ctx, started)
    }

    /// Writes a dummy Core file where `is_installed` looks.
    fn install(ctx: &Ctx, core: &CoreInfo) {
        let path = cores::installed_path(&ctx.data_dir, core);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"dummy").unwrap();
    }

    fn names(started: &Started) -> Vec<String> {
        started.borrow().iter().map(|(name, _)| name.clone()).collect()
    }

    /// Moves the `n`th download started to `state`.
    fn set(started: &Started, n: usize, state: DownloadState) {
        *started.borrow()[n].1.lock().unwrap() = state;
    }

    fn model(root: &Path, ids: &[&str]) -> Model {
        let mut model = Model::new(root.join("Games"));
        for (i, system) in SYSTEMS.iter().enumerate() {
            model.ticked[i] = ids.contains(&system.id);
        }
        model
    }

    /// The step entered with `ids` ticked.
    fn entered(root: &Path, ids: &[&str], ctx: &mut Ctx) -> (Cores, Model) {
        let model = model(root, ids);
        let mut step = Cores::new();
        step.enter(&model, ctx);
        (step, model)
    }

    /// Each row's state, as drawn.
    fn states(step: &mut Cores, ctx: &Ctx) -> Vec<String> {
        let installed = step.queue.installed(&ctx.data_dir).to_vec();
        let rows = step.rows.iter();
        rows.map(|r| state(step.queue.job(r.core), installed[r.core]))
            .collect()
    }

    /// Setup on the Cores step with `ids`, all Embedded, ticked and their folders made.
    fn on_cores(root: &Path, ids: &[&str], ctx: &mut Ctx) -> Setup {
        let mut setup = Setup::new(root.join("Games"));
        setup.model = model(root, ids);
        act(&mut setup, ctx, Action::Settings);
        act(&mut setup, ctx, Action::Confirm);
        act(&mut setup, ctx, Action::Settings);
        assert_eq!(setup.step(), 4);
        setup
    }

    fn hints(setup: &Setup) -> Vec<&'static str> {
        setup.hints().iter().map(|h| h.text).collect()
    }

    #[test]
    fn the_step_is_passed_over_without_an_embedded_system() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!Cores::new().applies(&model(dir.path(), &["gc", "switch"])));
        assert!(Cores::new().applies(&model(dir.path(), &["gc", "nes"])));

        let mut setup = Setup::new(dir.path().join("Games"));
        setup.model = model(dir.path(), &["gc"]);
        setup.at = 2;
        assert_eq!(setup.neighbour(true), Some(4), "from step 3 on to step 5");
    }

    #[test]
    fn entering_queues_only_the_missing_cores_each_once_one_at_a_time() {
        let (dir, mut ctx, started) = world();
        install(&ctx, &MESEN);
        let (mut step, model) = entered(dir.path(), &["gba", "gbc", "nes", "gb", "gc"], &mut ctx);
        let systems: Vec<_> = step.rows.iter().map(|r| r.system.name).collect();
        assert_eq!(
            systems,
            ["NES", "Game Boy", "Game Boy Color", "Game Boy Advance"],
            "the ticked Embedded Systems in System Order"
        );
        let subs: Vec<_> = (0..step.rows.len()).map(|i| sub_line(step.core_of(i))).collect();
        assert_eq!(subs, ["Mesen", "SameBoy", "SameBoy", "mGBA"]);
        assert_eq!(names(&started), ["SameBoy"], "one at a time");
        assert_eq!(
            states(&mut step, &ctx),
            ["Installed", "Downloading", "Downloading", "Waiting"],
            "Game Boy and Game Boy Color share SameBoy"
        );

        set(
            &started,
            0,
            DownloadState::Running {
                received: 45,
                total: Some(100),
            },
        );
        step.tick(&model, &mut ctx);
        assert_eq!(states(&mut step, &ctx)[1..3], ["45%", "45%"]);
        set(&started, 0, DownloadState::Done(ctx.data_dir.clone()));
        step.tick(&model, &mut ctx);
        assert_eq!(
            names(&started),
            ["SameBoy", "mGBA"],
            "SameBoy once, then the next"
        );
        assert_eq!(
            states(&mut step, &ctx),
            ["Installed", "Downloaded", "Downloaded", "Downloading"]
        );
    }

    #[test]
    fn ppsspp_downloads_its_assets_then_its_core() {
        let (dir, mut ctx, started) = world();
        let (mut step, model) = entered(dir.path(), &["psp"], &mut ctx);
        assert_eq!(sub_line(step.core_of(0)), "PPSSPP · plus PPSSPP's assets");
        assert_eq!(names(&started), ["PPSSPP assets"]);
        set(&started, 0, DownloadState::Done(ctx.data_dir.clone()));
        step.tick(&model, &mut ctx);
        assert_eq!(names(&started), ["PPSSPP assets", "PPSSPP"]);
        assert_eq!(states(&mut step, &ctx), ["50%"]);
        assert!(!step.can_continue(&model));
        set(&started, 1, DownloadState::Done(ctx.data_dir.clone()));
        step.tick(&model, &mut ctx);
        assert_eq!(states(&mut step, &ctx), ["Downloaded"]);
        assert!(step.can_continue(&model));
    }

    #[test]
    fn continue_waits_for_the_downloads_and_a_failure_offers_a_retry_or_continue() {
        let (dir, mut ctx, started) = world();
        let ctx = &mut ctx;
        let mut setup = on_cores(dir.path(), &["nes"], ctx);
        assert_eq!(hints(&setup), ["Back"]);
        assert_eq!(setup.steps[setup.at].focused_button(), None, "nothing is focused");
        act(&mut setup, ctx, Action::Settings);
        act(&mut setup, ctx, Action::Down);
        act(&mut setup, ctx, Action::Confirm);
        assert_eq!(setup.step(), 4, "neither + nor A go on while downloading");

        set(&started, 0, DownloadState::Failed("no network".to_owned()));
        setup.tick(ctx);
        assert_eq!(hints(&setup), ["Retry", "Continue", "Back"]);
        act(&mut setup, ctx, Action::Confirm);
        assert_eq!(names(&started), ["Mesen", "Mesen"], "A on the failed row retries");
        assert_eq!(hints(&setup), ["Back"]);

        set(&started, 1, DownloadState::Failed("no network".to_owned()));
        setup.tick(ctx);
        act(&mut setup, ctx, Action::Down);
        assert_eq!(setup.steps[setup.at].focused_button(), Some(Button::Continue));
        assert_eq!(hints(&setup), ["Continue", "Back"]);
        act(&mut setup, ctx, Action::Confirm);
        assert_eq!(setup.step(), 5, "Continue goes on though the Core failed");
    }

    #[test]
    fn a_finished_step_focuses_continue() {
        let (dir, mut ctx, started) = world();
        let ctx = &mut ctx;
        let mut setup = on_cores(dir.path(), &["nes"], ctx);
        set(&started, 0, DownloadState::Done(ctx.data_dir.clone()));
        setup.tick(ctx);
        assert_eq!(setup.steps[setup.at].focused_button(), Some(Button::Continue));
        assert_eq!(hints(&setup), ["Continue", "Back"]);
    }

    #[test]
    fn setup_is_busy_while_a_core_downloads() {
        let (dir, mut ctx, started) = world();
        let ctx = &mut ctx;
        let mut setup = on_cores(dir.path(), &["nes"], ctx);
        assert!(setup.busy(), "Mesen is downloading");
        set(&started, 0, DownloadState::Done(ctx.data_dir.clone()));
        setup.tick(ctx);
        assert!(!setup.busy());
    }

    #[test]
    fn on_the_keyboard_the_focused_continue_hints_enter_not_ctrl_comma() {
        let (dir, mut ctx, started) = world();
        let ctx = &mut ctx;
        let mut setup = on_cores(dir.path(), &["nes"], ctx);
        set(&started, 0, DownloadState::Done(ctx.data_dir.clone()));
        ctx.input_kind = InputKind::Keyboard;
        setup.tick(ctx);
        assert_eq!(
            setup.hints(),
            [
                Hint::new(&[Action::Confirm], "Continue"),
                Hint::new(&[Action::Back], "Back")
            ]
        );
        let style = ctx.button_style;
        assert_eq!(glyph_label(Action::Confirm, InputKind::Keyboard, style), "Enter");
        act(&mut setup, ctx, Action::Confirm);
        assert_eq!(setup.step(), 5, "Enter on the focused Continue goes on");
    }

    #[test]
    fn leaving_cancels_the_download_and_coming_back_queues_it_again() {
        let (dir, mut ctx, started) = world();
        let ctx = &mut ctx;
        let mut setup = on_cores(dir.path(), &["nes", "snes"], ctx);
        assert_eq!(names(&started), ["Mesen"]);
        act(&mut setup, ctx, Action::Back);
        assert_eq!(setup.step(), 3);
        assert_eq!(
            Arc::strong_count(&started.borrow()[0].1),
            1,
            "the running Download was dropped"
        );

        install(ctx, &MESEN);
        act(&mut setup, ctx, Action::Settings);
        assert_eq!(setup.step(), 4);
        assert_eq!(names(&started), ["Mesen", "bsnes"], "only what is still missing");
    }

    #[test]
    fn the_bios_note_shows_only_with_playstation_ticked() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(bios_note(&model(dir.path(), &["nes", "psp"])), None);
        let bios = dir.path().join("Games/bios");
        let note = format!(
            "PlayStation needs a BIOS file: put {ASK_FOR} in {}/.",
            bios.display()
        );
        assert_eq!(bios_note(&model(dir.path(), &["nes", "ps1"])), Some(note));
    }
}
