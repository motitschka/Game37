//! What the Windows screens that set up a Standalone Emulator share (FR-WIN-4, FR-WIN-5): saving
//! the `.exe` the player chose, and the download page icon.

use std::path::Path;

use crate::config;
use crate::ctx::Ctx;
use crate::emulators::Emulator;
use crate::ui::gfx::Gfx;
use crate::ui::theme::Colour;

/// Keeps (`Some`) or forgets (`None`) the `.exe` chosen for `emulator` in the Config, then
/// re-detects the emulators, so a notice for it clears or a Settings row shows where it is now.
/// Only for a Config that exists: First-run Setup keeps its choices until Finish.
pub fn save(ctx: &mut Ctx, emulator: &Emulator, exe: Option<&Path>) {
    if let Err(e) = config::save_emulator_exe(&ctx.config_path, emulator.key, exe) {
        eprintln!("game37: can't save {}'s program: {e}", emulator.name);
    }
    ctx.emulators = (ctx.detect_emulators)();
}

/// A download arrow over a bar, about 14 by 16, centred on `x` with its middle at `cy`: the
/// mockups' "⤓", drawn because not every font has it.
pub fn download_icon(gfx: &mut Gfx<'_>, x: f32, cy: f32, colour: Colour) {
    let (top, tip) = (cy - 8.0, cy + 3.0);
    gfx.line(x, top, x, tip, 2.0, colour);
    gfx.line(x - 5.0, tip - 5.0, x, tip, 2.0, colour);
    gfx.line(x + 5.0, tip - 5.0, x, tip, 2.0, colour);
    gfx.line(x - 7.0, cy + 8.0, x + 7.0, cy + 8.0, 2.0, colour);
}

#[cfg(test)]
pub mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use crate::ctx::{Ctx, OnChoose};
    use crate::emulators::Emulator;
    use crate::ui::gfx::Gfx;
    use crate::ui::input::{Action, Input};
    use crate::ui::nav::{Hint, Screen, Transition};

    /// What a Choose .exe asked for: the emulator's key, the start folder and whether in Setup.
    pub type Asked = Rc<RefCell<Vec<(&'static str, Option<PathBuf>, bool)>>>;

    /// Swaps `ctx`'s `.exe` browser for one that picks `exe` on Confirm and cancels on Back, and
    /// logs what each was asked for.
    pub fn picking(ctx: &mut Ctx, exe: PathBuf) -> Asked {
        let asked: Asked = Rc::default();
        let log = asked.clone();
        ctx.choose_exe = Box::new(move |emulator: &'static Emulator, start, in_setup, on_choose| {
            log.borrow_mut().push((emulator.key, start, in_setup));
            Box::new(Picks {
                exe: exe.clone(),
                on_choose: Some(on_choose),
            })
        });
        asked
    }

    struct Picks {
        exe: PathBuf,
        on_choose: Option<OnChoose>,
    }

    impl Screen for Picks {
        fn title(&self) -> &str {
            "Picks"
        }

        fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
            match input {
                Input::Action(Action::Confirm) => {
                    if let Some(on_choose) = self.on_choose.take() {
                        on_choose(ctx, self.exe.clone());
                    }
                    Transition::Pop
                }
                Input::Action(Action::Back) => Transition::Pop,
                _ => Transition::None,
            }
        }

        fn draw(&mut self, _: &mut Gfx<'_>, _: &Ctx) {}

        fn hints(&self) -> Vec<Hint> {
            Vec::new()
        }
    }

    /// Plays the browser `transition` pushed: Confirm picks, Back cancels.
    pub fn answer(transition: Transition, ctx: &mut Ctx, action: Action) {
        let Transition::Push(mut browser) = transition else {
            panic!("Choose .exe opened no browser");
        };
        assert!(matches!(
            browser.handle(Input::Action(action), ctx),
            Transition::Pop
        ));
    }
}
