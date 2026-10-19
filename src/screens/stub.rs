//! Placeholder for a top-level tab whose real screen comes in a later job.

use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::MUTED;
use crate::ui::{H, W};

pub struct Stub {
    title: &'static str,
}

impl Stub {
    pub fn new(title: &'static str) -> Stub {
        Stub { title }
    }
}

impl Screen for Stub {
    fn title(&self) -> &str {
        self.title
    }

    fn handle(&mut self, _: Input) -> Transition {
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>) {
        gfx.text(
            W / 2.0,
            H / 2.0,
            self.title,
            TextStyle::new(28, Weight::Medium, MUTED),
            Align::Centre,
        );
    }

    fn hints(&self) -> Vec<Hint> {
        vec![Hint::new(&[Action::PrevTab, Action::NextTab], "Switch view")]
    }
}
