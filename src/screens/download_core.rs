//! Downloading a Core the first time an Embedded Game needs it (FR-EMB-2, US-23, FR-CAR-10): a
//! modal progress screen pushed by `launch::confirm_game` when the Game's Core isn't installed
//! yet. Done starts the Game; Failed turns the screen into the usual message modal.

use crate::cores::{self, CoreInfo, Download, DownloadState};
use crate::ctx::{Ctx, Play};
use crate::screens::message::{self, Message, PANEL_W};
use crate::ui::Rect;
use crate::ui::cover::focus_ring;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, LINE, MUTED, SURFACE2, TEXT};

const TITLE: &str = "Downloading the Core";
const FAILED_TITLE: &str = "Couldn't download the Core";
const PAD: f32 = 56.0;
const BAR_W: f32 = PANEL_W - 2.0 * PAD;
const BAR_H: f32 = 24.0;
const BUTTON_W: f32 = 200.0;
const BUTTON_H: f32 = 64.0;

/// A Core's zip body is read in bytes; shown to the player in decimal megabytes.
fn human(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// "1.2 MB of 3.4 MB", or just the received size when the total is unknown.
fn progress_text(state: &DownloadState) -> String {
    match state {
        DownloadState::Running {
            received,
            total: Some(total),
        } => {
            format!("{} of {}", human(*received), human(*total))
        }
        DownloadState::Running {
            received,
            total: None,
        } => human(*received),
        DownloadState::Done(_) | DownloadState::Failed(_) => String::new(),
    }
}

/// Which archive is downloading right now. PPSSPP needs two, assets then Core (FR-EMB-1); every
/// other Core needs only the one.
enum Step {
    Assets(Download),
    Core(Download),
}

pub struct DownloadCore {
    /// What to play once the Core is in, slot and all.
    play: Play,
    core: &'static CoreInfo,
    step: Step,
    /// Whether this screen runs an assets step (always before the Core step): only
    /// [`DownloadCore::new_with_assets`] sets it, so the label counts steps this screen actually
    /// runs rather than going by [`CoreInfo::system_in_data_dir`] alone.
    assets_step: bool,
    progress: DownloadState,
    /// Set once the download fails; from then on this screen draws and behaves like `Message`.
    failed: Option<Message>,
    cancel_hit: Option<Rect>,
}

impl DownloadCore {
    /// Downloads just `core`'s library: every Core except PPSSPP, and PPSSPP once its assets are
    /// already there.
    pub fn new(play: Play, core: &'static CoreInfo, download: Download) -> DownloadCore {
        DownloadCore::with_step(play, core, Step::Core(download), false)
    }

    /// Downloads `core`'s assets first, then its library (FR-EMB-1): PPSSPP only.
    pub fn new_with_assets(play: Play, core: &'static CoreInfo, assets: Download) -> DownloadCore {
        DownloadCore::with_step(play, core, Step::Assets(assets), true)
    }

    fn with_step(play: Play, core: &'static CoreInfo, step: Step, assets_step: bool) -> DownloadCore {
        DownloadCore {
            play,
            core,
            step,
            assets_step,
            progress: DownloadState::Running {
                received: 0,
                total: None,
            },
            failed: None,
            cancel_hit: None,
        }
    }

    /// "Downloading the <Core> Core" or, for a two-step install, with its step count.
    fn label(&self) -> String {
        match &self.step {
            Step::Assets(_) => format!("Downloading the {} assets (1 of 2)", self.core.name),
            Step::Core(_) if self.assets_step => {
                format!("Downloading the {} Core (2 of 2)", self.core.name)
            }
            Step::Core(_) => format!("Downloading the {} Core", self.core.name),
        }
    }
}

impl Screen for DownloadCore {
    fn title(&self) -> &str {
        match &self.failed {
            Some(msg) => msg.title(),
            None => TITLE,
        }
    }

    fn is_modal(&self) -> bool {
        true
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        if let Some(msg) = &mut self.failed {
            return msg.handle(input, ctx);
        }
        match input {
            Input::Action(Action::Back) | Input::Pointer(Pointer::Back) => Transition::Pop,
            Input::Pointer(Pointer::Click { x, y }) if self.cancel_hit.is_some_and(|r| r.contains(x, y)) => {
                Transition::Pop
            }
            _ => Transition::None,
        }
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Transition {
        if self.failed.is_some() {
            return Transition::None;
        }
        let state = match &mut self.step {
            Step::Assets(d) => d.poll(),
            Step::Core(d) => d.poll(),
        };
        match state {
            DownloadState::Done(_) => match &self.step {
                Step::Assets(_) if cores::installed_path(&ctx.data_dir, self.core).is_file() => {
                    // The Core was already installed; only its assets were missing.
                    ctx.play = Some(self.play.clone());
                    Transition::Pop
                }
                Step::Assets(_) => {
                    let download = (ctx.start_download)(self.core, ctx.data_dir.clone());
                    self.step = Step::Core(download);
                    self.progress = DownloadState::Running {
                        received: 0,
                        total: None,
                    };
                    Transition::None
                }
                Step::Core(_) => {
                    ctx.play = Some(self.play.clone());
                    Transition::Pop
                }
            },
            DownloadState::Failed(text) => {
                self.failed = Some(Message::new(FAILED_TITLE, text));
                Transition::None
            }
            running @ DownloadState::Running { .. } => {
                self.progress = running;
                Transition::None
            }
        }
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        if let Some(msg) = &mut self.failed {
            msg.draw(gfx, ctx);
            return;
        }
        message::dim(gfx);
        let title_text = self.label();
        let progress = progress_text(&self.progress);
        let body = TextStyle::new(22, Weight::Regular, MUTED);
        let h = PAD + 34.0 + 32.0 + 24.0 + BAR_H + 32.0 + BUTTON_H + PAD;
        let p = message::panel(gfx, h);
        let x = p.x + PAD;
        let mut y = p.y + PAD + 34.0;
        gfx.text(
            x,
            y,
            &title_text,
            TextStyle::new(34, Weight::Bold, TEXT),
            Align::Left,
        );
        y += 32.0;
        gfx.text(x, y, &progress, body, Align::Left);
        y += 24.0;
        let bar = Rect::new(x, y, BAR_W, BAR_H);
        gfx.fill_round_rect(bar, BAR_H / 2.0, SURFACE2);
        if let DownloadState::Running {
            received,
            total: Some(total),
        } = self.progress
            && total > 0
        {
            let fraction = (received as f64 / total as f64).clamp(0.0, 1.0) as f32;
            let filled = Rect::new(bar.x, bar.y, bar.w * fraction, bar.h);
            gfx.fill_round_rect(filled, BAR_H / 2.0, ACCENT);
        }
        gfx.stroke_round_rect(bar, BAR_H / 2.0, 1.0, LINE);
        let cancel = Rect::new(
            p.x + p.w - PAD - BUTTON_W,
            p.y + p.h - PAD - BUTTON_H,
            BUTTON_W,
            BUTTON_H,
        );
        focus_ring(gfx, cancel, 14.0);
        gfx.fill_round_rect(cancel, 14.0, SURFACE2);
        gfx.text(
            cancel.x + cancel.w / 2.0,
            cancel.y + cancel.h / 2.0 + 8.0,
            "Cancel",
            TextStyle::new(22, Weight::Bold, TEXT),
            Align::Centre,
        );
        self.cancel_hit = Some(cancel);
    }

    fn hints(&self) -> Vec<Hint> {
        match &self.failed {
            Some(msg) => msg.hints(),
            None => vec![Hint::new(&[Action::Back], "Cancel")],
        }
    }

    fn busy(&self) -> bool {
        self.failed.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cores::PPSSPP;
    use crate::cores::SAMEBOY;
    use crate::cores::testing::{assets_zip, core_zip, spawn, spawn_assets, spawn_failing};
    use crate::launch::{confirm_game, confirm_play};
    use crate::library::FoundGame;
    use crate::library::db::{GameRow, SortOrder};
    use std::fs;
    use std::thread;
    use std::time::{Duration, Instant};

    fn wait_for<F: FnMut(&mut DownloadCore) -> Option<T>, T>(screen: &mut DownloadCore, mut f: F) -> T {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(value) = f(screen) {
                return value;
            }
            assert!(Instant::now() < deadline, "timed out waiting for the download");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn game(ctx: &mut Ctx) -> GameRow {
        let found = FoundGame {
            system: "gb",
            rel_path: "gb/Tetris.gb".into(),
            title: "Tetris".into(),
        };
        ctx.library.sync_system("gb", &[found], 0).unwrap();
        ctx.library.system_games("gb", SortOrder::AZ).unwrap().remove(0)
    }

    #[test]
    fn progress_text_shows_received_of_total_or_just_received() {
        assert_eq!(
            progress_text(&DownloadState::Running {
                received: 1_200_000,
                total: Some(3_400_000)
            }),
            "1.2 MB of 3.4 MB"
        );
        assert_eq!(
            progress_text(&DownloadState::Running {
                received: 500_000,
                total: None
            }),
            "0.5 MB"
        );
    }

    #[test]
    fn confirm_with_no_core_pushes_the_download_screen() {
        let mut ctx = Ctx::empty();
        let dir = tempfile::tempdir().unwrap();
        ctx.data_dir = dir.path().to_owned();
        let g = game(&mut ctx);
        ctx.start_download = Box::new(spawn_failing);
        assert!(matches!(confirm_game(&g, &mut ctx), Transition::Push(_)));
        assert!(ctx.play.is_none());
    }

    #[test]
    fn done_pops_and_sets_ctx_play() {
        let mut ctx = Ctx::empty();
        let dir = tempfile::tempdir().unwrap();
        ctx.data_dir = dir.path().to_owned();
        let g = game(&mut ctx);
        let download = spawn(&SAMEBOY, ctx.data_dir.clone(), core_zip(&SAMEBOY), None);
        let mut screen = DownloadCore::new(Play::resume(g.clone()), &SAMEBOY, download);
        wait_for(&mut screen, |s| match s.tick(&mut ctx) {
            Transition::Pop => Some(()),
            Transition::None => None,
            _ => panic!("unexpected transition"),
        });
        assert_eq!(ctx.play.as_ref().map(|p| p.game.id), Some(g.id));
    }

    /// Loading a Save State from the Options Menu (FR-DET-4) before the Core was ever downloaded
    /// still starts from the chosen slot once the download is done.
    #[test]
    fn a_launch_from_a_slot_keeps_its_slot_through_the_core_download() {
        let mut ctx = Ctx::empty();
        let dir = tempfile::tempdir().unwrap();
        ctx.data_dir = dir.path().to_owned();
        let g = game(&mut ctx);
        ctx.start_download = Box::new(|core, data_dir| spawn(core, data_dir, core_zip(core), None));
        let play = Play {
            game: g,
            slot: Some(2),
        };
        let Transition::Push(mut screen) = confirm_play(play.clone(), &mut ctx) else {
            panic!("the Core download should start");
        };
        assert!(ctx.play.is_none(), "nothing plays before the Core is in");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !matches!(screen.tick(&mut ctx), Transition::Pop) {
            assert!(Instant::now() < deadline, "timed out waiting for the download");
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(ctx.play, Some(play), "the Game, from slot 2");
    }

    #[test]
    fn failed_shows_the_message_with_the_cores_text() {
        let mut ctx = Ctx::empty();
        let dir = tempfile::tempdir().unwrap();
        ctx.data_dir = dir.path().to_owned();
        let g = game(&mut ctx);
        let download = spawn_failing(&SAMEBOY, ctx.data_dir.clone());
        let mut screen = DownloadCore::new(Play::resume(g), &SAMEBOY, download);
        wait_for(&mut screen, |s| {
            s.tick(&mut ctx);
            s.failed.is_some().then_some(())
        });
        let msg = screen.failed.unwrap();
        assert_eq!(msg.title, FAILED_TITLE);
        assert_eq!(
            msg.body,
            "Couldn't download the SameBoy Core. Check your internet connection."
        );
    }

    /// A Core-only download (PPSSPP with its assets already present) never says "(2 of 2)": that
    /// counted `system_in_data_dir`, true of PPSSPP regardless of whether an assets step runs.
    #[test]
    fn a_core_only_download_of_ppsspp_never_says_2_of_2() {
        let dir = tempfile::tempdir().unwrap();
        let g = GameRow {
            id: 1,
            system: "psp".into(),
            rel_path: "psp/Game.iso".into(),
            title: "Game".into(),
            date_added: 0,
            last_played: None,
            playtime_secs: 0,
            favorite: false,
        };
        let download = spawn(&PPSSPP, dir.path().to_owned(), core_zip(&PPSSPP), None);
        let screen = DownloadCore::new(Play::resume(g), &PPSSPP, download);
        let label = screen.label();
        assert!(!label.contains("2 of 2"), "{label}");
    }

    /// With PPSSPP's Core already installed and only its assets missing, finishing the assets
    /// step plays straight away instead of downloading the Core again.
    #[test]
    fn assets_finishing_with_the_core_already_installed_skips_the_core_download() {
        let mut ctx = Ctx::empty();
        let dir = tempfile::tempdir().unwrap();
        ctx.data_dir = dir.path().to_owned();
        let g = game(&mut ctx);
        let core_path = crate::cores::installed_path(&ctx.data_dir, &PPSSPP);
        fs::create_dir_all(core_path.parent().unwrap()).unwrap();
        fs::write(&core_path, b"already installed").unwrap();
        ctx.start_download = Box::new(|_, _| panic!("the Core is already installed"));
        let assets = spawn_assets(
            ctx.data_dir.clone(),
            assets_zip(&[("PPSSPP/flash0/font/a.pgf", b"font data")]),
            None,
        );
        let mut screen = DownloadCore::new_with_assets(Play::resume(g.clone()), &PPSSPP, assets);
        wait_for(&mut screen, |s| match s.tick(&mut ctx) {
            Transition::Pop => Some(()),
            Transition::None => None,
            _ => panic!("unexpected transition"),
        });
        assert_eq!(ctx.play.as_ref().map(|p| p.game.id), Some(g.id));
    }

    #[test]
    fn back_cancels_and_pops() {
        let mut ctx = Ctx::empty();
        let dir = tempfile::tempdir().unwrap();
        ctx.data_dir = dir.path().to_owned();
        let g = game(&mut ctx);
        let download = spawn_failing(&SAMEBOY, ctx.data_dir.clone());
        let mut screen = DownloadCore::new(Play::resume(g), &SAMEBOY, download);
        assert!(matches!(
            screen.handle(Input::Action(Action::Back), &mut ctx),
            Transition::Pop
        ));
        assert!(matches!(
            screen.handle(Input::Pointer(Pointer::Back), &mut ctx),
            Transition::Pop
        ));
    }
}
