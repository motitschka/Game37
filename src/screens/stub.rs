//! Placeholder for a top-level tab or an entry screen whose real screen comes in a later job.

use crate::ctx::Ctx;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input};
use crate::ui::nav::{Entry, Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::MUTED;
use crate::ui::{H, W};

pub struct Stub {
    title: &'static str,
    entry: Option<Entry>,
}

impl Stub {
    pub fn new(title: &'static str) -> Stub {
        Stub { title, entry: None }
    }

    /// The placeholder for the Search or the Settings screen.
    pub fn for_entry(title: &'static str, entry: Entry) -> Stub {
        Stub {
            title,
            entry: Some(entry),
        }
    }
}

impl Screen for Stub {
    fn title(&self) -> &str {
        self.title
    }

    fn handle(&mut self, input: Input, _: &mut Ctx) -> Transition {
        match input {
            Input::Action(Action::Back) => Transition::Pop,
            _ => Transition::None,
        }
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, _: &Ctx) {
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

    fn entry(&self) -> Option<Entry> {
        self.entry
    }

    fn search_pill(&self) -> bool {
        self.entry != Some(Entry::Search)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::nav::{Hits, Nav};

    #[test]
    fn back_leaves_a_pushed_placeholder() {
        let mut nav = Nav::single(Box::new(Stub::new("Home")));
        nav.push(Box::new(Stub::for_entry("Settings", Entry::Settings)));
        let ctx = &mut Ctx::empty();
        nav.handle(Input::Action(Action::Back), &Hits::default(), ctx);
        assert_eq!((nav.depth(), nav.top().title()), (1, "Home"));
        nav.handle(Input::Action(Action::Back), &Hits::default(), ctx);
        assert_eq!(nav.depth(), 1, "a root placeholder stays");
    }
}
