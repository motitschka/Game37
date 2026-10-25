//! Shown instead of First-run Setup when the Config exists but can't be read.

use crate::config::ConfigError;
use crate::ctx::Ctx;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{MUTED, TEXT};
use crate::ui::{M, W};

pub struct NoConfig {
    heading: &'static str,
    body: String,
}

impl NoConfig {
    pub fn unreadable(error: &ConfigError) -> NoConfig {
        NoConfig {
            heading: "Your Config couldn't be read",
            body: error.to_string(),
        }
    }
}

impl Screen for NoConfig {
    fn title(&self) -> &str {
        "First-run Setup"
    }

    fn handle(&mut self, input: Input, _: &mut Ctx) -> Transition {
        match input {
            Input::Action(Action::Back) | Input::Pointer(Pointer::Back) => Transition::Quit,
            _ => Transition::None,
        }
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, _: &Ctx) {
        let body = TextStyle::new(22, Weight::Regular, MUTED);
        let lines = gfx.wrap(&self.body, body, W - 4.0 * M);
        let line_height = 34.0;
        let top = 540.0 - (lines.len() as f32 * line_height) / 2.0;
        gfx.text(
            W / 2.0,
            top - 30.0,
            self.heading,
            TextStyle::new(40, Weight::Bold, TEXT),
            Align::Centre,
        );
        for (i, line) in lines.iter().enumerate() {
            gfx.text(
                W / 2.0,
                top + 30.0 + i as f32 * line_height,
                line,
                body,
                Align::Centre,
            );
        }
    }

    fn hints(&self) -> Vec<Hint> {
        vec![Hint::new(&[Action::Back], "Quit")]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_quits() {
        let error = ConfigError::BadApiKey(std::path::PathBuf::from("/secrets.toml"));
        let mut screen = NoConfig::unreadable(&error);
        assert!(matches!(
            screen.handle(Input::Action(Action::Back), &mut Ctx::empty()),
            Transition::Quit
        ));
        assert!(matches!(
            screen.handle(Input::Pointer(Pointer::Back), &mut Ctx::empty()),
            Transition::Quit
        ));
        assert!(matches!(
            screen.handle(Input::Action(Action::Confirm), &mut Ctx::empty()),
            Transition::None
        ));
    }
}
