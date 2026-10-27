//! Settings → BIOS (US-28), after `screen_settings_bios` in `design/generate_mockups.py`: a compact
//! table of every System in System Order and its BIOS status, using the same check as launching
//! (FR-EMB-12). The rows have no action: the section can be entered only to move down the table.

use std::path::MAIN_SEPARATOR;

use super::{RW, RX, Row, Section, TOP};
use crate::cores;
use crate::ctx::Ctx;
use crate::emulators;
use crate::screens::home::tilde;
use crate::systems::{Kind, SYSTEMS, System};
use crate::ui::Rect;
use crate::ui::cover::ellipsize;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, BG, LINE, MUTED, SURFACE2, TEXT, WARN};

/// The first row's top, under the column headers.
const TABLE_TOP: f32 = TOP + 28.0;
const PITCH: f32 = 41.0;
/// Left edge of the BIOS column.
const STATUS_X: f32 = RX + 320.0;

/// The BIOS files the Missing text asks for. Beetle PSX HW is the only Core with a `required_bios`
/// list, so, like launching's message (`launch::missing_bios_message`), this names PS1's three
/// region BIOS files rather than every name the check accepts. A test keeps the two the same.
pub(crate) const ASK_FOR: &str = "scph5500.bin, scph5501.bin or scph5502.bin";

/// One System's BIOS status.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    /// The accepted file that was found, named as on disk.
    Found(String),
    /// Its Core needs a BIOS and none is there: what to put where.
    Missing(String),
    NotNeeded,
    /// An External System: its Standalone Emulator's name.
    Handled(&'static str),
}

impl Status {
    fn of(system: &System, ctx: &Ctx) -> Status {
        if system.kind == Kind::External
            && let Some(emulator) = emulators::for_system(system)
        {
            return Status::Handled(emulator.name);
        }
        match cores::for_system(system) {
            Some(core) if !core.required_bios.is_empty() => match cores::found_bios(&ctx.games_root, core) {
                Some(file) => Status::Found(file),
                None => {
                    let dir = tilde(&ctx.games_root.join("bios"), dirs::home_dir().as_deref());
                    Status::Missing(format!("Missing: put {ASK_FOR} in {dir}{MAIN_SEPARATOR}"))
                }
            },
            _ => Status::NotNeeded,
        }
    }

    fn text(&self) -> String {
        match self {
            Status::Found(file) => format!("Found {file}"),
            Status::Missing(text) => text.clone(),
            Status::NotNeeded => "No BIOS needed".to_owned(),
            Status::Handled(emulator) => format!("Handled by {emulator}"),
        }
    }
}

#[derive(Default)]
pub struct Bios {
    /// Every System's status as of the last time the section was shown.
    statuses: Option<Vec<Status>>,
}

impl Bios {
    fn statuses(&mut self, ctx: &Ctx) -> &[Status] {
        self.statuses
            .get_or_insert_with(|| SYSTEMS.iter().map(|s| Status::of(s, ctx)).collect())
    }
}

impl Section for Bios {
    fn name(&self) -> &'static str {
        "BIOS"
    }

    fn rows(&mut self, ctx: &Ctx) -> Vec<Row> {
        let statuses = self.statuses(ctx);
        SYSTEMS
            .iter()
            .zip(statuses)
            .map(|(system, status)| Row::new(system.name).value(status.text()).inert())
            .collect()
    }

    fn shown(&mut self, _ctx: &Ctx) {
        self.statuses = None;
    }

    fn layout(&self, rows: &[Row]) -> Vec<(Rect, usize)> {
        (0..rows.len())
            .map(|i| (Rect::new(RX, TABLE_TOP + i as f32 * PITCH, RW, PITCH), i))
            .collect()
    }

    fn draw_rows(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx, rows: &[Row], focused: Option<usize>) -> bool {
        let header = TextStyle::new(13, Weight::Bold, MUTED);
        gfx.text(RX + 32.0, TOP + 13.0, "SYSTEM", header, Align::Left);
        gfx.text(STATUS_X, TOP + 13.0, "BIOS", header, Align::Left);
        let statuses = self.statuses(ctx).to_vec();
        for ((row, (r, i)), status) in rows.iter().zip(self.layout(rows)).zip(&statuses) {
            let on = focused == Some(i);
            if on {
                gfx.fill_round_rect(r, 10.0, SURFACE2);
                gfx.fill_round_rect(Rect::new(r.x, r.y + r.h / 2.0 - 13.0, 5.0, 26.0), 2.0, ACCENT);
            } else if i > 0 {
                gfx.fill_rect(Rect::new(r.x + 16.0, r.y, r.w - 32.0, 1.0), LINE);
            }
            let base = r.y + r.h / 2.0 + 7.0;
            let weight = if on { Weight::Bold } else { Weight::Medium };
            gfx.text(
                RX + 32.0,
                base,
                row.label,
                TextStyle::new(19, weight, TEXT),
                Align::Left,
            );
            let (x, ink) = match status {
                Status::Missing(_) => {
                    gfx.fill_circle(STATUS_X + 11.0, base - 6.0, 11.0, WARN);
                    let mark = TextStyle::new(15, Weight::ExtraBold, BG);
                    gfx.text(STATUS_X + 11.0, base - 1.0, "!", mark, Align::Centre);
                    (STATUS_X + 32.0, WARN)
                }
                _ => (STATUS_X, MUTED),
            };
            let style = TextStyle::new(18, Weight::Medium, ink);
            let text = ellipsize(&status.text(), RX + RW - 32.0 - x, |s| gfx.measure(s, style));
            gfx.text(x, base, &text, style, Align::Left);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::settings::{Focus, Settings};
    use crate::ui::input::{Action, Input, Pointer};
    use crate::ui::nav::{Hint, Screen, Transition};
    use std::fs;

    /// A Ctx on a temporary Games Root with an empty `bios/` folder.
    fn world() -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = dir.path().join("Games");
        fs::create_dir_all(ctx.games_root.join("bios")).unwrap();
        (dir, ctx)
    }

    fn value(rows: &[Row], id: &str) -> String {
        let i = SYSTEMS.iter().position(|s| s.id == id).unwrap();
        rows[i].value.clone().unwrap()
    }

    /// The `scph….bin` names in `text`, sorted.
    fn bios_names(text: &str) -> Vec<String> {
        let mut names: Vec<_> = text
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '.'))
            .filter(|w| w.starts_with("scph"))
            .map(|w| w.trim_end_matches('.').to_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn there_is_one_row_per_system_in_system_order_and_none_has_an_action() {
        let (_dir, ctx) = world();
        let rows = Bios::default().rows(&ctx);
        let labels: Vec<_> = rows.iter().map(|r| r.label).collect();
        let names: Vec<_> = SYSTEMS.iter().map(|s| s.name).collect();
        assert_eq!((rows.len(), labels), (17, names));
        assert!(rows.iter().all(|r| r.inert));
    }

    #[test]
    fn each_kind_of_status() {
        let (_dir, ctx) = world();
        let rows = Bios::default().rows(&ctx);
        let bios = ctx.games_root.join("bios");
        let dir = tilde(&bios, dirs::home_dir().as_deref());
        let missing = format!("Missing: put {ASK_FOR} in {dir}{MAIN_SEPARATOR}");
        assert_eq!(value(&rows, "ps1"), missing);
        for id in ["nes", "gb", "snes", "n64", "gbc", "gba", "nds", "psp"] {
            assert_eq!(value(&rows, id), "No BIOS needed", "{id}");
        }
        assert_eq!(value(&rows, "gc"), "Handled by Dolphin");
        assert_eq!(value(&rows, "3ds"), "Handled by Azahar");
        assert_eq!(value(&rows, "ps2"), "Handled by PCSX2");
    }

    #[test]
    fn found_names_the_file_actually_there() {
        let (_dir, ctx) = world();
        fs::write(ctx.games_root.join("bios/SCPH5502.BIN"), b"x").unwrap();
        assert_eq!(value(&Bios::default().rows(&ctx), "ps1"), "Found SCPH5502.BIN");
    }

    #[test]
    fn missing_asks_for_the_same_files_as_launching_does() {
        let (_dir, ctx) = world();
        let launch = crate::launch::missing_bios_message(&ctx.games_root);
        let names = bios_names(&value(&Bios::default().rows(&ctx), "ps1"));
        assert_eq!(names, ["scph5500.bin", "scph5501.bin", "scph5502.bin"]);
        assert_eq!(names, bios_names(&launch));
    }

    fn act(s: &mut Settings, ctx: &mut Ctx, actions: &[Action]) -> Transition {
        let mut last = Transition::None;
        for &action in actions {
            last = s.handle(Input::Action(action), ctx);
        }
        last
    }

    #[test]
    fn the_section_rechecks_when_shown_and_its_rows_only_move_the_focus() {
        let (_dir, mut ctx) = world();
        let ctx = &mut ctx;
        let mut s = Settings::new();
        act(&mut s, ctx, &[Action::Down, Action::Down, Action::Right]);
        assert_eq!((s.section(), s.focus()), ("BIOS", Focus::Rows));
        assert_eq!(s.hints(), [Hint::new(&[Action::Back], "Back")]);
        let ps1 = SYSTEMS.iter().position(|s| s.id == "ps1").unwrap();
        let r = Bios::default().layout(&Bios::default().rows(ctx))[ps1].0;
        s.handle(
            Input::Pointer(Pointer::Click {
                x: r.x + 10.0,
                y: r.y + 10.0,
            }),
            ctx,
        );
        assert_eq!(s.row(), ps1, "a click goes by the table's layout");
        assert!(matches!(act(&mut s, ctx, &[Action::Confirm]), Transition::None));
        assert_eq!((s.focus(), s.row()), (Focus::Rows, ps1), "Confirm does nothing");

        fs::write(ctx.games_root.join("bios/scph5501.bin"), b"x").unwrap();
        act(&mut s, ctx, &[Action::Down]);
        assert!(
            value(&s.rows, "ps1").starts_with("Missing"),
            "not re-checked every frame"
        );
        act(
            &mut s,
            ctx,
            &[Action::Left, Action::Down, Action::Up, Action::Right],
        );
        assert_eq!(s.section(), "BIOS");
        assert_eq!(
            value(&s.rows, "ps1"),
            "Found scph5501.bin",
            "re-checked when shown"
        );
        assert_eq!(s.hints(), [Hint::new(&[Action::Back], "Back")]);
    }
}
