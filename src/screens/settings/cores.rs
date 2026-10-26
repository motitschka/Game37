//! Settings → Cores (FR-SET-1), after `screen_settings_cores` in `design/generate_mockups.py`: Update
//! all, then one row per Core with its download state. Confirm downloads a Core, or redownloads it if
//! it is installed; PPSSPP downloads its assets first, then the Core, as `DownloadCore` does. One
//! download runs at a time and the rest wait in a [`CoreQueue`], which lives as long as Settings:
//! closing Settings drops the running `Download`, which cancels it and removes its temp files.

use super::{RW, RX, Row, Section, TOP};
use crate::cores::{self, CoreInfo};
use crate::ctx::Ctx;
use crate::screens::core_queue::{self, CoreQueue, Job, progress_bar};
use crate::systems::SYSTEMS;
use crate::ui::Rect;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::nav::Transition;
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, DIM, LINE, MUTED, SUCCESS, SURFACE, SURFACE2, TEXT, WARN};

const UPDATE_ALL_H: f32 = 64.0;
/// The first Core row's top, under Update all.
const CORES_TOP: f32 = TOP + 78.0;
const PITCH: f32 = 78.0;
const ROW_H: f32 = 70.0;
const BAR_W: f32 = 240.0;

/// One Core's row.
struct Entry {
    core: &'static CoreInfo,
    /// Its Systems' names, in System Order.
    systems: String,
}

pub struct Cores {
    /// Each Core once, ordered by its first System.
    entries: Vec<Entry>,
    /// The entries' downloads, by the same index. It rechecks which are installed when the
    /// section is shown, and when a download ends.
    queue: CoreQueue,
}

impl Default for Cores {
    fn default() -> Self {
        let mut entries: Vec<Entry> = Vec::new();
        for core in SYSTEMS.iter().filter_map(cores::for_system) {
            if entries.iter().all(|e| e.core != core) {
                let names: Vec<_> = SYSTEMS
                    .iter()
                    .filter(|s| cores::for_system(s) == Some(core))
                    .map(|s| s.name)
                    .collect();
                entries.push(Entry {
                    core,
                    systems: names.join(", "),
                });
            }
        }
        let queue = CoreQueue::new(entries.iter().map(|e| e.core).collect());
        Cores { entries, queue }
    }
}

impl Cores {
    fn installed(&mut self, ctx: &Ctx) -> &[bool] {
        self.queue.installed(&ctx.data_dir)
    }

    /// The installed Cores Update all would queue: those not already waiting or downloading.
    fn update_targets(&mut self, ctx: &Ctx) -> Vec<usize> {
        let installed = self.installed(ctx).to_vec();
        (0..self.entries.len())
            .filter(|&i| installed[i] && !self.queue.busy(i))
            .collect()
    }
}

impl Section for Cores {
    fn name(&self) -> &'static str {
        "Cores"
    }

    fn rows(&mut self, ctx: &Ctx) -> Vec<Row> {
        let installed = self.installed(ctx).to_vec();
        let count = installed.iter().filter(|&&on| on).count();
        let value = match count {
            0 => "No Cores installed yet".to_owned(),
            1 => "Redownloads the 1 installed Core".to_owned(),
            n => format!("Redownloads the {n} installed Cores"),
        };
        let mut update = Row::new("Update all").value(value).verb("Update all");
        if self.update_targets(ctx).is_empty() {
            update = update.inert();
        }
        let mut rows = vec![update];
        for (i, entry) in self.entries.iter().enumerate() {
            let job = self.queue.job(i);
            let verb = match job {
                Job::Failed => "Retry",
                _ if installed[i] => "Redownload",
                _ => "Download",
            };
            let row = Row::new(entry.core.name)
                .value(core_queue::status(job, installed[i]))
                .verb(verb);
            rows.push(if self.queue.busy(i) { row.inert() } else { row });
        }
        rows
    }

    fn confirm(&mut self, i: usize, ctx: &mut Ctx) -> Transition {
        if i == 0 {
            for target in self.update_targets(ctx) {
                self.queue.enqueue(target);
            }
        } else if i <= self.entries.len() {
            self.queue.enqueue(i - 1);
        }
        self.queue.advance(ctx);
        Transition::None
    }

    fn shown(&mut self, _ctx: &Ctx) {
        self.queue.recheck();
    }

    fn tick(&mut self, ctx: &mut Ctx) {
        self.queue.tick(ctx);
    }

    fn layout(&self, rows: &[Row]) -> Vec<(Rect, usize)> {
        (0..rows.len())
            .map(|i| match i {
                0 => (Rect::new(RX, TOP, RW, UPDATE_ALL_H), 0),
                _ => (Rect::new(RX, CORES_TOP + (i - 1) as f32 * PITCH, RW, ROW_H), i),
            })
            .collect()
    }

    fn draw_rows(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx, rows: &[Row], focused: Option<usize>) -> bool {
        let installed = self.installed(ctx).to_vec();
        for (row, (r, i)) in rows.iter().zip(self.layout(rows)) {
            let on = focused == Some(i);
            if on {
                gfx.fill_round_rect(r, 14.0, SURFACE2);
                gfx.fill_round_rect(Rect::new(r.x, r.y + r.h / 2.0 - 20.0, 5.0, 40.0), 2.0, ACCENT);
            } else {
                gfx.fill_round_rect(r, 14.0, SURFACE.alpha(0.6));
            }
            let entry = i.checked_sub(1).and_then(|e| self.entries.get(e));
            let base = r.y + r.h / 2.0 + if entry.is_some() { -6.0 } else { 8.0 };
            let weight = if on { Weight::Bold } else { Weight::SemiBold };
            gfx.text(
                RX + 32.0,
                base,
                row.label,
                TextStyle::new(22, weight, TEXT),
                Align::Left,
            );
            let right = RX + RW - 32.0;
            let value_y = r.y + r.h / 2.0 + 7.0;
            let value = row.value.as_deref().unwrap_or_default();
            let Some(entry) = entry else {
                let style = TextStyle::new(18, Weight::Medium, MUTED);
                gfx.text(right, value_y, value, style, Align::Right);
                continue;
            };
            let sub = TextStyle::new(16, Weight::Medium, MUTED);
            gfx.text(RX + 32.0, base + 30.0, &entry.systems, sub, Align::Left);
            let (style, x) = match self.queue.job(i - 1) {
                Job::Running(f) => {
                    let bar = Rect::new(right - BAR_W, r.y + r.h / 2.0 - 4.0, BAR_W, 8.0);
                    progress_bar(gfx, bar, f.unwrap_or(0.0), LINE, ACCENT);
                    (TextStyle::new(18, Weight::Medium, TEXT), bar.x - 20.0)
                }
                Job::Failed => (TextStyle::new(18, Weight::SemiBold, WARN), right),
                Job::Done => (TextStyle::new(18, Weight::Medium, SUCCESS), right),
                Job::Idle if !installed[i - 1] => (TextStyle::new(18, Weight::Medium, DIM), right),
                Job::Idle | Job::Waiting => (TextStyle::new(18, Weight::Medium, MUTED), right),
            };
            gfx.text(x, value_y, value, style, Align::Right);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cores::DownloadState;
    use crate::cores::testing::{core_zip, manual, spawn, spawn_failing};
    use crate::cores::{BEETLE_PSX_HW, MESEN, MGBA, MUPEN64PLUS_NEXT, PPSSPP};
    use crate::screens::settings::{Focus, Settings};
    use crate::ui::input::{Action, Input, Pointer};
    use crate::ui::nav::{Hint, Screen};
    use std::cell::RefCell;
    use std::fs;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

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

    /// Writes a dummy Core file where `is_installed` looks, and PPSSPP's assets folder for PPSSPP.
    fn install(ctx: &Ctx, core: &CoreInfo) {
        let path = cores::installed_path(&ctx.data_dir, core);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"dummy").unwrap();
        if core.system_in_data_dir {
            fs::create_dir_all(cores::assets_dir(&ctx.data_dir)).unwrap();
        }
    }

    fn names(started: &Started) -> Vec<String> {
        started.borrow().iter().map(|(name, _)| name.clone()).collect()
    }

    /// Moves the `n`th download started to `state`.
    fn set(started: &Started, n: usize, state: DownloadState) {
        *started.borrow()[n].1.lock().unwrap() = state;
    }

    fn running(received: u64, total: u64) -> DownloadState {
        DownloadState::Running {
            received,
            total: Some(total),
        }
    }

    /// The row of `core`, counting Update all as row 0.
    fn row_of(core: &CoreInfo) -> usize {
        1 + Cores::default()
            .entries
            .iter()
            .position(|e| e.core == core)
            .unwrap()
    }

    fn value(cores: &mut Cores, ctx: &Ctx, core: &CoreInfo) -> String {
        cores.rows(ctx)[row_of(core)].value.clone().unwrap()
    }

    #[test]
    fn update_all_then_each_core_once_ordered_by_its_first_system_with_its_systems() {
        let (_dir, ctx, _) = world();
        let mut cores = Cores::default();
        let labels: Vec<_> = cores.rows(&ctx).iter().map(|r| r.label).collect();
        let expected = [
            "Update all",
            "Mesen",
            "SameBoy",
            "bsnes",
            "Mupen64Plus-Next",
            "mGBA",
            "melonDS DS",
            "Beetle PSX HW",
            "PPSSPP",
        ];
        assert_eq!(labels, expected);
        let systems: Vec<_> = cores.entries.iter().map(|e| e.systems.as_str()).collect();
        assert_eq!(systems[1], "Game Boy, Game Boy Color");
        for (entry, systems) in cores.entries.iter().zip(&systems) {
            let names: Vec<_> = SYSTEMS
                .iter()
                .filter(|s| cores::for_system(s) == Some(entry.core))
                .map(|s| s.name)
                .collect();
            assert_eq!(*systems, names.join(", "), "{}", entry.core.name);
        }
    }

    #[test]
    fn each_state_from_not_downloaded_through_progress_failure_retry_and_done() {
        let (_dir, mut ctx, started) = world();
        install(&ctx, &MESEN);
        let mut cores = Cores::default();
        assert_eq!(value(&mut cores, &ctx, &MESEN), "Installed");
        assert_eq!(value(&mut cores, &ctx, &MGBA), "Not downloaded");

        cores.confirm(row_of(&MUPEN64PLUS_NEXT), &mut ctx);
        assert_eq!(names(&started), ["Mupen64Plus-Next"]);
        assert_eq!(value(&mut cores, &ctx, &MUPEN64PLUS_NEXT), "Downloading");
        set(&started, 0, running(45, 100));
        cores.tick(&mut ctx);
        assert_eq!(value(&mut cores, &ctx, &MUPEN64PLUS_NEXT), "Downloading 45%");

        set(&started, 0, DownloadState::Failed("no network".to_owned()));
        cores.tick(&mut ctx);
        assert_eq!(
            value(&mut cores, &ctx, &MUPEN64PLUS_NEXT),
            "Download failed · Retry"
        );
        assert_eq!(cores.rows(&ctx)[row_of(&MUPEN64PLUS_NEXT)].verb, "Retry");

        cores.confirm(row_of(&MUPEN64PLUS_NEXT), &mut ctx);
        assert_eq!(
            names(&started),
            ["Mupen64Plus-Next", "Mupen64Plus-Next"],
            "a retry"
        );
        install(&ctx, &MUPEN64PLUS_NEXT);
        set(&started, 1, DownloadState::Done(ctx.data_dir.clone()));
        cores.tick(&mut ctx);
        assert_eq!(value(&mut cores, &ctx, &MUPEN64PLUS_NEXT), "Downloaded");
        let rows = cores.rows(&ctx);
        assert_eq!(rows[row_of(&MUPEN64PLUS_NEXT)].verb, "Redownload");
        assert_eq!(
            rows[0].value.as_deref(),
            Some("Redownloads the 2 installed Cores")
        );
    }

    #[test]
    fn one_download_at_a_time_and_a_confirm_meanwhile_queues() {
        let (_dir, mut ctx, started) = world();
        let mut cores = Cores::default();
        cores.confirm(row_of(&MGBA), &mut ctx);
        cores.confirm(row_of(&MESEN), &mut ctx);
        cores.confirm(row_of(&MESEN), &mut ctx);
        cores.tick(&mut ctx);
        assert_eq!(names(&started), ["mGBA"], "one at a time");
        assert_eq!(value(&mut cores, &ctx, &MESEN), "Waiting");
        let rows = cores.rows(&ctx);
        assert!(
            rows[row_of(&MESEN)].inert && rows[row_of(&MGBA)].inert,
            "already on its way"
        );

        set(&started, 0, DownloadState::Failed("no network".to_owned()));
        cores.tick(&mut ctx);
        assert_eq!(names(&started), ["mGBA", "Mesen"], "the next starts, once");
        cores.tick(&mut ctx);
        assert_eq!(names(&started).len(), 2);
    }

    #[test]
    fn tick_covered_starts_a_queued_core_while_something_covers_settings() {
        let (_dir, mut ctx, started) = world();
        let mut cores = Cores::default();
        cores.confirm(row_of(&MGBA), &mut ctx);
        cores.confirm(row_of(&MESEN), &mut ctx);
        let mut s = Settings::with_sections(vec![Box::new(cores)]);
        s.tick_covered(&mut ctx);
        assert_eq!(names(&started), ["mGBA"], "nothing new yet, still downloading");

        set(&started, 0, DownloadState::Failed("no network".to_owned()));
        s.tick_covered(&mut ctx);
        assert_eq!(
            names(&started),
            ["mGBA", "Mesen"],
            "the queued Core starts while something covers Settings"
        );
    }

    #[test]
    fn update_all_queues_only_the_installed_cores_and_ppsspps_assets_before_its_core() {
        let (_dir, mut ctx, started) = world();
        install(&ctx, &MESEN);
        install(&ctx, &BEETLE_PSX_HW);
        install(&ctx, &PPSSPP);
        let mut cores = Cores::default();
        let rows = cores.rows(&ctx);
        assert_eq!(
            rows[0].value.as_deref(),
            Some("Redownloads the 3 installed Cores")
        );
        assert_eq!((rows[0].verb, rows[0].inert), ("Update all", false));

        cores.confirm(0, &mut ctx);
        assert_eq!(value(&mut cores, &ctx, &BEETLE_PSX_HW), "Waiting");
        assert_eq!(value(&mut cores, &ctx, &MGBA), "Not downloaded");
        for n in 0..4 {
            set(&started, n, DownloadState::Done(ctx.data_dir.clone()));
            cores.tick(&mut ctx);
        }
        let expected = ["Mesen", "Beetle PSX HW", "PPSSPP assets", "PPSSPP"];
        assert_eq!(names(&started), expected);
        assert_eq!(value(&mut cores, &ctx, &PPSSPP), "Downloaded");
        assert_eq!(value(&mut cores, &ctx, &MGBA), "Not downloaded");
    }

    #[test]
    fn ppsspp_downloads_its_assets_then_its_core_on_one_bar() {
        let (_dir, mut ctx, started) = world();
        let mut cores = Cores::default();
        cores.confirm(row_of(&PPSSPP), &mut ctx);
        assert_eq!(names(&started), ["PPSSPP assets"]);
        set(&started, 0, running(50, 100));
        cores.tick(&mut ctx);
        assert_eq!(value(&mut cores, &ctx, &PPSSPP), "Downloading 25%");
        set(&started, 0, DownloadState::Done(ctx.data_dir.clone()));
        cores.tick(&mut ctx);
        assert_eq!(names(&started), ["PPSSPP assets", "PPSSPP"]);
        assert_eq!(value(&mut cores, &ctx, &PPSSPP), "Downloading 50%");
        set(&started, 1, running(50, 100));
        cores.tick(&mut ctx);
        assert_eq!(value(&mut cores, &ctx, &PPSSPP), "Downloading 75%");
    }

    #[test]
    fn update_all_with_nothing_installed_has_no_action() {
        let (_dir, mut ctx, started) = world();
        let mut cores = Cores::default();
        let rows = cores.rows(&ctx);
        assert_eq!(rows[0].value.as_deref(), Some("No Cores installed yet"));
        assert!(rows[0].inert);
        cores.confirm(0, &mut ctx);
        assert!(names(&started).is_empty());
    }

    fn wait_for(cores: &mut Cores, ctx: &mut Ctx, core: &CoreInfo, want: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while value(cores, ctx, core) != want {
            assert!(Instant::now() < deadline, "timed out waiting for {want}");
            thread::sleep(Duration::from_millis(5));
            cores.tick(ctx);
        }
    }

    #[test]
    fn a_real_download_installs_the_core_and_a_failing_one_offers_a_retry() {
        let (_dir, mut ctx, _) = world();
        ctx.start_download = Box::new(|core, data_dir| spawn(core, data_dir, core_zip(core), None));
        let mut cores = Cores::default();
        cores.confirm(row_of(&MGBA), &mut ctx);
        wait_for(&mut cores, &mut ctx, &MGBA, "Downloaded");
        assert!(cores::is_installed(&ctx.data_dir, &MGBA));

        ctx.start_download = Box::new(spawn_failing);
        cores.confirm(row_of(&MESEN), &mut ctx);
        wait_for(&mut cores, &mut ctx, &MESEN, "Download failed · Retry");
        assert!(!cores::is_installed(&ctx.data_dir, &MESEN));
    }

    fn act(s: &mut Settings, ctx: &mut Ctx, actions: &[Action]) {
        for &action in actions {
            s.handle(Input::Action(action), ctx);
        }
    }

    #[test]
    fn the_hints_follow_the_focus_and_a_click_goes_by_the_sections_layout() {
        let (_dir, mut ctx, started) = world();
        let ctx = &mut ctx;
        install(ctx, &MESEN);
        let mut s = Settings::new();
        act(
            &mut s,
            ctx,
            &[Action::Down, Action::Down, Action::Down, Action::Right],
        );
        assert_eq!((s.section(), s.focus(), s.row()), ("Cores", Focus::Rows, 0));
        let back = Hint::new(&[Action::Back], "Back");
        let a = |verb| vec![Hint::new(&[Action::Confirm], verb), back.clone()];
        assert_eq!(s.hints(), a("Update all"));
        act(&mut s, ctx, &[Action::Down]);
        assert_eq!(s.hints(), a("Redownload"));
        act(&mut s, ctx, &[Action::Down]);
        assert_eq!(s.hints(), a("Download"));

        let r = Cores::default().layout(&s.rows)[row_of(&BEETLE_PSX_HW)].0;
        let click = Input::Pointer(Pointer::Click {
            x: r.x + 10.0,
            y: r.y + 10.0,
        });
        s.handle(click, ctx);
        assert_eq!(s.row(), row_of(&BEETLE_PSX_HW));
        s.handle(click, ctx);
        assert_eq!(
            names(&started),
            ["Beetle PSX HW"],
            "a click on the focused row confirms"
        );
        set(&started, 0, DownloadState::Failed("no network".to_owned()));
        s.tick(ctx);
        act(&mut s, ctx, &[Action::Up, Action::Down]);
        assert_eq!(s.hints(), a("Retry"));
    }
}
