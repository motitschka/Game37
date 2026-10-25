//! A modal that asks before a destructive action: a question, a line under it, and Cancel and the
//! action as buttons, with Cancel focused. Laid out like a [`Message`](super::message::Message).

use crate::ctx::Ctx;
use crate::screens::message::{PANEL_W, dim, panel};
use crate::ui::cover::focus_ring;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{DANGER, MUTED, SURFACE2, TEXT};
use crate::ui::{Rect, hit};

const PAD: f32 = 56.0;
const TITLE_H: f32 = 42.0;
const BUTTON_W: f32 = 200.0;
const BUTTON_H: f32 = 64.0;
const BUTTON_GAP: f32 = 16.0;

/// What the action does once chosen.
type OnYes = Box<dyn FnOnce(&mut Ctx)>;

pub struct Confirm {
    title: String,
    body: String,
    /// The action's button label, e.g. "Reset".
    yes: &'static str,
    on_yes: Option<OnYes>,
    /// Whether the action's button has the focus rather than Cancel.
    yes_focused: bool,
    /// Where the buttons were last drawn, for the mouse: `true` for the action.
    hits: Vec<(Rect, bool)>,
}

impl Confirm {
    pub fn new(
        title: impl Into<String>,
        body: impl Into<String>,
        yes: &'static str,
        on_yes: impl FnOnce(&mut Ctx) + 'static,
    ) -> Confirm {
        Confirm {
            title: title.into(),
            body: body.into(),
            yes,
            on_yes: Some(Box::new(on_yes)),
            yes_focused: false,
            hits: Vec::new(),
        }
    }

    /// Closes the dialog, first carrying out the action if `yes`.
    fn choose(&mut self, yes: bool, ctx: &mut Ctx) -> Transition {
        if yes && let Some(on_yes) = self.on_yes.take() {
            on_yes(ctx);
        }
        Transition::Pop
    }
}

impl Screen for Confirm {
    fn title(&self) -> &str {
        &self.title
    }

    fn is_modal(&self) -> bool {
        true
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        match input {
            Input::Action(Action::Left) => self.yes_focused = false,
            Input::Action(Action::Right) => self.yes_focused = true,
            Input::Action(Action::Confirm) => return self.choose(self.yes_focused, ctx),
            Input::Action(Action::Back) | Input::Pointer(Pointer::Back) => return Transition::Pop,
            Input::Pointer(Pointer::Click { x, y }) => {
                if let Some(yes) = hit(&self.hits, x, y) {
                    return self.choose(yes, ctx);
                }
            }
            _ => {}
        }
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, _: &Ctx) {
        dim(gfx);
        let title = TextStyle::new(34, Weight::Bold, TEXT);
        let body = TextStyle::new(22, Weight::Regular, MUTED);
        let lines = gfx.wrap(&self.title, title, PANEL_W - 2.0 * PAD);
        let h = PAD + lines.len() as f32 * TITLE_H + 24.0 + 34.0 + 48.0 + BUTTON_H + PAD;
        let p = panel(gfx, h);
        let x = p.x + PAD;
        let mut y = p.y + PAD - 8.0;
        for line in &lines {
            y += TITLE_H;
            gfx.text(x, y, line, title, Align::Left);
        }
        gfx.text(x, y + 24.0 + 34.0, &self.body, body, Align::Left);
        self.hits.clear();
        let top = p.y + p.h - PAD - BUTTON_H;
        let mut right = p.x + p.w - PAD;
        for (yes, label) in [(true, self.yes), (false, "Cancel")] {
            let r = Rect::new(right - BUTTON_W, top, BUTTON_W, BUTTON_H);
            if yes == self.yes_focused {
                focus_ring(gfx, r, 14.0);
            }
            gfx.fill_round_rect(r, 14.0, SURFACE2);
            let ink = if yes { DANGER } else { TEXT };
            let style = TextStyle::new(22, Weight::Bold, ink);
            gfx.text(
                r.x + r.w / 2.0,
                r.y + r.h / 2.0 + 8.0,
                label,
                style,
                Align::Centre,
            );
            self.hits.push((r, yes));
            right -= BUTTON_W + BUTTON_GAP;
        }
    }

    fn hints(&self) -> Vec<Hint> {
        if self.yes_focused {
            vec![
                Hint::new(&[Action::Confirm], self.yes),
                Hint::new(&[Action::Back], "Cancel"),
            ]
        } else {
            vec![Hint::new(&[Action::Confirm], "Cancel")]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    fn dialog() -> (Confirm, Rc<Cell<bool>>) {
        let done = Rc::new(Cell::new(false));
        let flag = done.clone();
        (Confirm::new("Q?", "B", "Reset", move |_| flag.set(true)), done)
    }

    #[test]
    fn cancel_is_focused_so_confirm_closes_it_without_acting() {
        let ctx = &mut Ctx::empty();
        let (mut c, done) = dialog();
        assert_eq!(c.hints(), [Hint::new(&[Action::Confirm], "Cancel")]);
        assert!(matches!(
            c.handle(Input::Action(Action::Confirm), ctx),
            Transition::Pop
        ));
        assert!(!done.get());
    }

    #[test]
    fn right_then_confirm_acts_and_left_goes_back_to_cancel() {
        let ctx = &mut Ctx::empty();
        let (mut c, done) = dialog();
        c.handle(Input::Action(Action::Right), ctx);
        assert_eq!(c.hints()[0], Hint::new(&[Action::Confirm], "Reset"));
        c.handle(Input::Action(Action::Left), ctx);
        c.handle(Input::Action(Action::Confirm), ctx);
        assert!(!done.get());
        let (mut c, done) = dialog();
        c.handle(Input::Action(Action::Right), ctx);
        assert!(matches!(
            c.handle(Input::Action(Action::Confirm), ctx),
            Transition::Pop
        ));
        assert!(done.get());
    }

    #[test]
    fn back_closes_it_without_acting_even_with_the_action_focused() {
        let ctx = &mut Ctx::empty();
        let (mut c, done) = dialog();
        c.handle(Input::Action(Action::Right), ctx);
        assert!(matches!(
            c.handle(Input::Action(Action::Back), ctx),
            Transition::Pop
        ));
        assert!(!done.get());
    }

    #[test]
    fn a_click_on_a_button_chooses_it() {
        let ctx = &mut Ctx::empty();
        let (mut c, done) = dialog();
        c.hits = vec![
            (Rect::new(100.0, 0.0, 50.0, 50.0), true),
            (Rect::new(0.0, 0.0, 50.0, 50.0), false),
        ];
        let click = |x| Input::Pointer(Pointer::Click { x, y: 10.0 });
        assert!(matches!(c.handle(click(70.0), ctx), Transition::None));
        assert!(matches!(c.handle(click(10.0), ctx), Transition::Pop));
        assert!(!done.get());
        assert!(matches!(c.handle(click(110.0), ctx), Transition::Pop));
        assert!(done.get());
    }
}
