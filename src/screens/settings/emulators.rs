//! Settings → Emulators (FR-WIN-4, Windows only), after `screen_settings_emulators` in
//! `design/generate_mockups.py`: one row per Standalone Emulator with its Systems and where it is,
//! or "Not found". Confirm opens the `.exe` browser, starting in the folder of the one it has now,
//! if any; the choice is saved and the rows refresh. Options (Y) or a right-click on a row whose
//! location was chosen (not just found) asks before forgetting it, going back to finding it by
//! itself.

use std::path::PathBuf;

use super::{RW, RX, Row, Section, TOP};
use crate::ctx::Ctx;
use crate::emulators::{self, Emulator, How};
use crate::screens::choose_exe;
use crate::screens::confirm::Confirm;
use crate::screens::setup::folders::trim_start;
use crate::ui::Rect;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::hit;
use crate::ui::input::{Action, Input, Pointer};
use crate::ui::nav::{Hint, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, MUTED, SURFACE, SURFACE2, TEXT, WARN};

const PITCH: f32 = 96.0;
const ROW_H: f32 = 84.0;

/// Every Standalone Emulator, once, in System Order.
pub struct Emulators {
    rows: Vec<&'static Emulator>,
}

impl Default for Emulators {
    fn default() -> Self {
        Emulators {
            rows: emulators::all(),
        }
    }
}

impl Emulators {
    fn layout_rows(&self) -> Vec<(Rect, usize)> {
        (0..self.rows.len())
            .map(|i| (Rect::new(RX, TOP + i as f32 * PITCH, RW, ROW_H), i))
            .collect()
    }

    /// Asks before forgetting row `i`'s chosen `.exe`, if it has one; `None` for a row that has
    /// none to forget (missing, or found by Game37 itself).
    fn forget(&self, i: usize, ctx: &Ctx) -> Option<Transition> {
        let emulator = *self.rows.get(i)?;
        match ctx.emulators.location(emulator) {
            Some(location) if location.how == How::Chosen => {}
            _ => return None,
        }
        let name = emulator.name;
        Some(Transition::Push(Box::new(Confirm::new(
            format!("Forget {name}'s program?"),
            format!("Game37 goes back to finding {name} by itself."),
            "Forget",
            move |ctx: &mut Ctx| choose_exe::save(ctx, emulator, None),
        ))))
    }
}

impl Section for Emulators {
    fn name(&self) -> &'static str {
        "Emulators"
    }

    fn rows(&mut self, ctx: &Ctx) -> Vec<Row> {
        self.rows
            .iter()
            .map(|&emulator| {
                let row = match ctx.emulators.location(emulator) {
                    Some(location) => Row::new(emulator.name).value(location.exe.to_string_lossy()),
                    None => Row::new(emulator.name).value("Not found").warn(),
                }
                .opens();
                match ctx.emulators.location(emulator) {
                    Some(location) if location.how == How::Chosen => {
                        row.extra_hint(Hint::new(&[Action::Options], "Forget"))
                    }
                    _ => row,
                }
            })
            .collect()
    }

    fn confirm(&mut self, i: usize, ctx: &mut Ctx) -> Transition {
        let Some(&emulator) = self.rows.get(i) else {
            return Transition::None;
        };
        let start = ctx
            .emulators
            .location(emulator)
            .and_then(|location| location.exe.parent().map(|p| p.to_path_buf()));
        let on_choose = Box::new(move |ctx: &mut Ctx, exe: PathBuf| {
            choose_exe::save(ctx, emulator, Some(&exe));
        });
        Transition::Push((ctx.choose_exe)(emulator, start, false, on_choose))
    }

    fn handle(&mut self, i: usize, input: Input, ctx: &mut Ctx) -> Option<Transition> {
        match input {
            Input::Action(Action::Options) => self.forget(i, ctx),
            Input::Pointer(Pointer::RightClick { x, y }) => {
                let i = hit(&self.layout_rows(), x, y)?;
                self.forget(i, ctx)
            }
            _ => None,
        }
    }

    fn layout(&self, _rows: &[Row]) -> Vec<(Rect, usize)> {
        self.layout_rows()
    }

    fn draw_rows(&mut self, gfx: &mut Gfx<'_>, _ctx: &Ctx, rows: &[Row], focused: Option<usize>) -> bool {
        for ((row, (r, i)), &emulator) in rows.iter().zip(self.layout_rows()).zip(&self.rows) {
            let on = focused == Some(i);
            if on {
                gfx.fill_round_rect(r, 14.0, SURFACE2);
                gfx.fill_round_rect(Rect::new(r.x, r.y + r.h / 2.0 - 20.0, 5.0, 40.0), 2.0, ACCENT);
            } else {
                gfx.fill_round_rect(r, 14.0, SURFACE.alpha(0.6));
            }
            let base = r.y + r.h / 2.0 - 6.0;
            let weight = if on { Weight::Bold } else { Weight::SemiBold };
            let label = TextStyle::new(22, weight, TEXT);
            gfx.text(r.x + 32.0, base, row.label, label, Align::Left);
            let systems = emulators::systems_of(emulator);
            let sub_style = TextStyle::new(16, Weight::Medium, MUTED);
            gfx.text(r.x + 32.0, base + 30.0, &systems, sub_style, Align::Left);

            let mut right = r.x + r.w - 32.0;
            let arrow = TextStyle::new(26, Weight::SemiBold, MUTED);
            gfx.text(right, r.y + r.h / 2.0 + 9.0, "›", arrow, Align::Right);
            right -= 36.0;
            if let Some(value) = &row.value {
                let style = TextStyle::new(18, Weight::Medium, if row.warn { WARN } else { MUTED });
                let room = right - (r.x + 32.0);
                let value = trim_start(value, room, |s| gfx.measure(s, style));
                gfx.text(right, r.y + r.h / 2.0 + 7.0, &value, style, Align::Right);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use super::*;
    use crate::emulators::{Emulators as InstalledEmulators, Location, Places};
    use crate::screens::choose_exe::tests::{answer, picking};
    use crate::screens::settings::Settings;
    use crate::ui::input::{Action, Input, Pointer};
    use crate::ui::nav::{Screen, Transition};

    fn touch(path: &Path) -> PathBuf {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
        path.to_path_buf()
    }

    /// Cemu's index among `emulators::all()`, i.e. its row in the section.
    fn cemu_index() -> usize {
        emulators::all().iter().position(|e| e.key == "cemu").unwrap()
    }

    /// A Ctx that detects as Windows does: only the `.exe` files chosen in the Config at
    /// `config_path`, re-read every time, plus whatever `located` starts it with.
    fn windows_ctx(config_path: &Path, located: InstalledEmulators) -> Ctx {
        let mut ctx = Ctx::empty();
        ctx.config_path = config_path.to_path_buf();
        ctx.emulators = located;
        let path = config_path.to_path_buf();
        ctx.detect_emulators = Box::new(move || {
            let chosen = crate::config::load_emulator_exes(&path).unwrap();
            InstalledEmulators::locate_all(&chosen, &Places::default())
        });
        ctx
    }

    /// Settings on the Emulators section's first row, built with the test-only flag so it exists
    /// on Linux too; moves to row `i`.
    fn on_row(i: usize, ctx: &mut Ctx) -> Settings {
        let mut s = Settings::new_with_emulators();
        for _ in 0..3 {
            s.handle(Input::Action(Action::Down), ctx);
        }
        s.handle(Input::Action(Action::Right), ctx);
        for _ in 0..i {
            s.handle(Input::Action(Action::Down), ctx);
        }
        assert_eq!(s.section(), "Emulators");
        s
    }

    #[test]
    fn every_emulator_is_listed_with_its_location_or_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let exe = touch(&dir.path().join("Cemu/Cemu.exe"));
        let mut ctx = Ctx::empty();
        ctx.emulators = InstalledEmulators::located(HashMap::from([(
            "cemu",
            Location {
                exe: exe.clone(),
                how: How::Found,
            },
        )]));
        let rows = Emulators::default().rows(&ctx);
        let i = cemu_index();
        assert_eq!(rows[i].value.as_deref(), Some(exe.to_string_lossy().as_ref()));
        assert!(!rows[i].warn);
        let other = (i + 1) % rows.len();
        assert_eq!(rows[other].value.as_deref(), Some("Not found"));
        assert!(rows[other].warn);
    }

    #[test]
    fn confirm_opens_the_browser_at_the_exes_folder_and_a_pick_saves_and_refreshes() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        let old = touch(&dir.path().join("Old/Cemu.exe"));
        let new = touch(&dir.path().join("New/Cemu.exe"));
        let located = InstalledEmulators::located(HashMap::from([(
            "cemu",
            Location {
                exe: old.clone(),
                how: How::Chosen,
            },
        )]));
        let mut ctx = windows_ctx(&config, located);
        let asked = picking(&mut ctx, new.clone());
        let mut s = on_row(cemu_index(), &mut ctx);
        let opened = s.handle(Input::Action(Action::Confirm), &mut ctx);
        assert_eq!(asked.borrow()[0].1, old.parent().map(Path::to_path_buf));
        answer(opened, &mut ctx, Action::Confirm);
        assert_eq!(
            crate::config::load_emulator_exes(&config).unwrap().get("cemu"),
            Some(&new)
        );
        // The row refreshes: it now shows the newly chosen path.
        assert_eq!(
            Emulators::default().rows(&ctx)[cemu_index()].value.as_deref(),
            Some(new.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn y_on_a_chosen_row_asks_and_forget_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        let exe = touch(&dir.path().join("Cemu/Cemu.exe"));
        crate::config::save_emulator_exe(&config, "cemu", Some(&exe)).unwrap();
        let located = InstalledEmulators::located(HashMap::from([(
            "cemu",
            Location {
                exe: exe.clone(),
                how: How::Chosen,
            },
        )]));
        let mut ctx = windows_ctx(&config, located);
        let mut s = on_row(cemu_index(), &mut ctx);
        assert!(s.hints().iter().any(|h| h.text == "Forget"));
        let opened = s.handle(Input::Action(Action::Options), &mut ctx);
        let Transition::Push(mut dialog) = opened else {
            panic!("Options opened no dialog");
        };
        dialog.handle(Input::Action(Action::Right), &mut ctx);
        assert!(matches!(
            dialog.handle(Input::Action(Action::Confirm), &mut ctx),
            Transition::Pop
        ));
        assert_eq!(
            crate::config::load_emulator_exes(&config).unwrap().get("cemu"),
            None
        );
    }

    #[test]
    fn y_on_a_found_but_not_chosen_row_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let exe = touch(&dir.path().join("Cemu/Cemu.exe"));
        let mut ctx = Ctx::empty();
        ctx.emulators = InstalledEmulators::located(HashMap::from([(
            "cemu",
            Location {
                exe: exe.clone(),
                how: How::Found,
            },
        )]));
        let mut s = on_row(cemu_index(), &mut ctx);
        assert!(!s.hints().iter().any(|h| h.text == "Forget"));
        assert!(matches!(
            s.handle(Input::Action(Action::Options), &mut ctx),
            Transition::None
        ));
    }

    #[test]
    fn a_right_click_on_a_chosen_row_asks_too() {
        let dir = tempfile::tempdir().unwrap();
        let exe = touch(&dir.path().join("Cemu/Cemu.exe"));
        let mut ctx = Ctx::empty();
        ctx.emulators = InstalledEmulators::located(HashMap::from([(
            "cemu",
            Location {
                exe: exe.clone(),
                how: How::Chosen,
            },
        )]));
        let i = cemu_index();
        let mut s = on_row(0, &mut ctx);
        let r = Emulators::default().layout_rows()[i].0;
        let got = s.handle(
            Input::Pointer(Pointer::RightClick {
                x: r.x + 10.0,
                y: r.y + 10.0,
            }),
            &mut ctx,
        );
        assert!(matches!(got, Transition::Push(_)));
    }

    #[test]
    fn on_linux_without_the_flag_the_sections_are_unchanged() {
        if cfg!(windows) {
            return;
        }
        let s = Settings::new();
        let names: Vec<_> = s.sections.iter().map(|s| s.name()).collect();
        assert_eq!(
            names,
            [
                "Library", "Artwork", "BIOS", "Cores", "Controls", "Display", "About"
            ]
        );
    }
}
