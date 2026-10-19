//! On top while a Standalone Emulator runs (FR-EXT-2): says how to come back, and swallows all
//! input, since the player quits with the emulator's own controls (FR-EXT-3).

use super::message::{PANEL_W, dim, panel};
use crate::ctx::Ctx;
use crate::emulators::Emulator;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::Input;
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, DIM, MUTED, TEXT};

const PAD: f32 = 56.0;
const LINE_H: f32 = 34.0;

/// What the Playing screen says about `title` running in `emulator`.
pub fn text(title: &str, emulator: &Emulator) -> String {
    let name = emulator.name;
    let mut text =
        format!("{title} is running in {name}. Quit it with {name}'s own controls to come back here.");
    if emulator.stays_open {
        text += &format!("\nWhen you're done, close {name}'s window: Playtime counts until it closes.");
    }
    text
}

pub struct Playing {
    title: String,
    body: String,
    waiting: String,
}

impl Playing {
    pub fn new(title: &str, emulator: &Emulator) -> Playing {
        Playing {
            title: title.to_owned(),
            body: text(title, emulator),
            waiting: format!("Waiting for {}…", emulator.name),
        }
    }
}

impl Screen for Playing {
    fn title(&self) -> &str {
        "Playing"
    }

    fn is_modal(&self) -> bool {
        true
    }

    fn handle(&mut self, _: Input, _: &mut Ctx) -> Transition {
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, _: &Ctx) {
        dim(gfx);
        let body = TextStyle::new(22, Weight::Regular, MUTED);
        let lines = gfx.wrap(&self.body, body, PANEL_W - 2.0 * PAD);
        let h = PAD + 14.0 + 56.0 + 16.0 + lines.len() as f32 * LINE_H + 56.0 + PAD;
        let p = panel(gfx, h);
        let x = p.x + PAD;
        let mut y = p.y + PAD + 14.0;
        gfx.text(
            x,
            y,
            "PLAYING",
            TextStyle::new(15, Weight::Bold, ACCENT),
            Align::Left,
        );
        y += 56.0;
        gfx.text(
            x,
            y,
            &self.title,
            TextStyle::new(40, Weight::ExtraBold, TEXT),
            Align::Left,
        );
        y += 16.0;
        for line in &lines {
            y += LINE_H;
            gfx.text(x, y, line, body, Align::Left);
        }
        gfx.text(
            x,
            y + 56.0,
            &self.waiting,
            TextStyle::new(18, Weight::Medium, DIM),
            Align::Left,
        );
    }

    fn hints(&self) -> Vec<Hint> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulators::{DOLPHIN, SHADPS4};

    #[test]
    fn says_how_to_come_back_and_to_close_emulators_that_stay_open() {
        assert_eq!(
            text("Zelda", &DOLPHIN),
            "Zelda is running in Dolphin. Quit it with Dolphin's own controls to come back here."
        );
        assert_eq!(
            text("Bloodborne", &SHADPS4),
            "Bloodborne is running in shadPS4. Quit it with shadPS4's own controls to come back here.\n\
             When you're done, close shadPS4's window: Playtime counts until it closes."
        );
    }
}
