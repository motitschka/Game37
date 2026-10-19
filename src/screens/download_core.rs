//! Downloading a Core the first time an Embedded Game needs it (FR-EMB-2, US-23, FR-CAR-10): a
//! modal progress screen pushed by `launch::confirm_game` when the Game's Core isn't installed
//! yet. Done starts the Game; Failed turns the screen into the usual message modal.

use crate::cores::{CoreInfo, Download, DownloadState};
use crate::ctx::Ctx;
use crate::library::db::GameRow;
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

pub struct DownloadCore {
    game: GameRow,
    core: &'static CoreInfo,
    download: Download,
    progress: DownloadState,
    /// Set once the download fails; from then on this screen draws and behaves like `Message`.
    failed: Option<Message>,
    cancel_hit: Option<Rect>,
}

impl DownloadCore {
    pub fn new(game: GameRow, core: &'static CoreInfo, download: Download) -> DownloadCore {
        DownloadCore {
            game,
            core,
            download,
            progress: DownloadState::Running {
                received: 0,
                total: None,
            },
            failed: None,
            cancel_hit: None,
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
        match self.download.poll() {
            DownloadState::Done(_) => {
                ctx.play = Some(self.game.clone());
                Transition::Pop
            }
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
        let title_text = format!("Downloading the {} Core", self.core.name);
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
    use crate::cores::SAMEBOY;
    use crate::cores::testing::{core_zip, spawn, spawn_failing};
    use crate::launch::confirm_game;
    use crate::library::FoundGame;
    use crate::library::db::SortOrder;
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
        let mut screen = DownloadCore::new(g.clone(), &SAMEBOY, download);
        wait_for(&mut screen, |s| match s.tick(&mut ctx) {
            Transition::Pop => Some(()),
            Transition::None => None,
            _ => panic!("unexpected transition"),
        });
        assert_eq!(ctx.play.as_ref().map(|g| g.id), Some(g.id));
    }

    #[test]
    fn failed_shows_the_message_with_the_cores_text() {
        let mut ctx = Ctx::empty();
        let dir = tempfile::tempdir().unwrap();
        ctx.data_dir = dir.path().to_owned();
        let g = game(&mut ctx);
        let download = spawn_failing(&SAMEBOY, ctx.data_dir.clone());
        let mut screen = DownloadCore::new(g, &SAMEBOY, download);
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

    #[test]
    fn back_cancels_and_pops() {
        let mut ctx = Ctx::empty();
        let dir = tempfile::tempdir().unwrap();
        ctx.data_dir = dir.path().to_owned();
        let g = game(&mut ctx);
        let download = spawn_failing(&SAMEBOY, ctx.data_dir.clone());
        let mut screen = DownloadCore::new(g, &SAMEBOY, download);
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
