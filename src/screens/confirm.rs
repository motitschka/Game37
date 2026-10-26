//! A modal that asks before a destructive action: a question, a line under it, and Cancel and the
//! action as buttons, with Cancel focused. Laid out like a [`Message`](super::message::Message).
//!
//! [`Confirm::hold`] makes a hold-mode dialog instead, for Delete save data (the owner's call:
//! the row that opens it is itself chosen with Confirm, so a double tap must not also delete it).
//! The action button is focused there, but carrying it out needs Confirm held [`HOLD_MS`] rather
//! than tapped; Cancel still acts at once, by Confirm or Back.

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

/// How long Confirm must be held on the action button in hold mode to carry it out.
pub(crate) const HOLD_MS: u64 = 1000;

/// What the action does once chosen.
type OnYes = Box<dyn FnOnce(&mut Ctx)>;

/// Times Confirm held on the action button, hold mode only: since when, and whether a press
/// already held when the dialog opened has been let go yet. Releasing always resets the count, and
/// a press held since before the dialog opened doesn't start one on its own: it only counts once
/// it's released and pressed again, else holding Confirm on the row that opened the dialog would
/// go on to delete a second later. Follows [`crate::screens::controller_test::HoldToExit`]'s
/// approach of timing a held input with `Ctx::ticks`.
#[derive(Debug, Default)]
struct Hold {
    armed: bool,
    since: Option<u64>,
}

impl Hold {
    /// Whether Confirm has now been held long enough to act.
    fn update(&mut self, held: bool, now: u64) -> bool {
        if !held {
            self.armed = true;
            self.since = None;
            return false;
        }
        if !self.armed {
            return false;
        }
        let since = *self.since.get_or_insert(now);
        now.saturating_sub(since) >= HOLD_MS
    }

    /// Fraction of the hold completed so far, 0 to 1, for the button's fill.
    fn progress(&self, now: u64) -> f32 {
        match self.since {
            Some(since) => (now.saturating_sub(since) as f32 / HOLD_MS as f32).min(1.0),
            None => 0.0,
        }
    }
}

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
    /// `Some` in hold mode: the action's button is focused and needs Confirm held, not tapped.
    hold: Option<Hold>,
    /// The hint shown for Confirm while the action's button is focused in hold mode, e.g. "Hold to
    /// delete".
    hold_hint: &'static str,
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
            hold: None,
            hold_hint: "",
        }
    }

    /// Like [`Self::new`], but the action's button is focused and carrying it out needs Confirm
    /// held for [`HOLD_MS`] rather than tapped (see the module docs). `hold_hint` is the Confirm
    /// hint shown while it's focused, e.g. "Hold to delete".
    pub fn hold(
        title: impl Into<String>,
        body: impl Into<String>,
        yes: &'static str,
        hold_hint: &'static str,
        on_yes: impl FnOnce(&mut Ctx) + 'static,
    ) -> Confirm {
        Confirm {
            yes_focused: true,
            hold: Some(Hold::default()),
            hold_hint,
            ..Confirm::new(title, body, yes, on_yes)
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
            // In hold mode the action button needs Confirm held, which `tick` times with
            // `Ctx::confirm_held`; a tap alone does nothing. Cancel still acts at once.
            Input::Action(Action::Confirm) if self.hold.is_some() && self.yes_focused => {}
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

    fn tick(&mut self, ctx: &mut Ctx) -> Transition {
        let Some(hold) = &mut self.hold else {
            return Transition::None;
        };
        // Only the focused action button's hold counts; moving focus away resets it.
        if hold.update(self.yes_focused && ctx.confirm_held, ctx.ticks) {
            return self.choose(true, ctx);
        }
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
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
        let progress = match &self.hold {
            Some(hold) if self.yes_focused => hold.progress(ctx.ticks),
            _ => 0.0,
        };
        for (yes, label) in [(true, self.yes), (false, "Cancel")] {
            let r = Rect::new(right - BUTTON_W, top, BUTTON_W, BUTTON_H);
            if yes == self.yes_focused {
                focus_ring(gfx, r, 14.0);
            }
            gfx.fill_round_rect(r, 14.0, SURFACE2);
            if yes && progress > 0.0 {
                gfx.clip(Some(Rect::new(r.x, r.y, r.w * progress, r.h)));
                gfx.fill_round_rect(r, 14.0, DANGER.alpha(0.35));
                gfx.clip(None);
            }
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
        // Back always cancels, so its hint shows regardless of which button has the focus.
        if self.yes_focused {
            let text = if self.hold.is_some() {
                self.hold_hint
            } else {
                self.yes
            };
            vec![
                Hint::new(&[Action::Confirm], text),
                Hint::new(&[Action::Back], "Cancel"),
            ]
        } else {
            vec![
                Hint::new(&[Action::Confirm], "Cancel"),
                Hint::new(&[Action::Back], "Cancel"),
            ]
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
        // Spec change: Back always cancels, so its hint now shows even with Cancel focused.
        assert_eq!(
            c.hints(),
            [
                Hint::new(&[Action::Confirm], "Cancel"),
                Hint::new(&[Action::Back], "Cancel"),
            ]
        );
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

    fn hold_dialog() -> (Confirm, Rc<Cell<bool>>) {
        let done = Rc::new(Cell::new(false));
        let flag = done.clone();
        (
            Confirm::hold("Q?", "B", "Delete", "Hold to delete", move |_| flag.set(true)),
            done,
        )
    }

    #[test]
    fn delete_is_focused_and_holding_confirm_the_full_second_deletes_once() {
        let ctx = &mut Ctx::empty();
        let (mut c, done) = hold_dialog();
        assert_eq!(c.hints()[0], Hint::new(&[Action::Confirm], "Hold to delete"));
        // Not held yet: this frame only arms the hold.
        assert!(matches!(c.tick(ctx), Transition::None));
        ctx.confirm_held = true;
        assert!(matches!(c.tick(ctx), Transition::None), "the hold just started");
        ctx.ticks += HOLD_MS;
        assert!(matches!(c.tick(ctx), Transition::Pop));
        assert!(done.get());
    }

    #[test]
    fn letting_go_at_900_ms_does_nothing() {
        let ctx = &mut Ctx::empty();
        let (mut c, done) = hold_dialog();
        c.tick(ctx);
        ctx.confirm_held = true;
        c.tick(ctx);
        ctx.ticks += HOLD_MS - 100;
        assert!(matches!(c.tick(ctx), Transition::None));
        ctx.confirm_held = false;
        assert!(matches!(c.tick(ctx), Transition::None));
        assert!(!done.get());
    }

    #[test]
    fn a_confirm_already_held_when_it_opened_only_counts_once_released_and_pressed_again() {
        let ctx = &mut Ctx::empty();
        // Held by the press that chose the row, before this dialog even opened.
        ctx.confirm_held = true;
        let (mut c, done) = hold_dialog();
        assert!(matches!(c.tick(ctx), Transition::None));
        ctx.ticks += HOLD_MS;
        assert!(
            matches!(c.tick(ctx), Transition::None),
            "a press already held when it opened never counts"
        );
        assert!(!done.get());
        ctx.confirm_held = false;
        c.tick(ctx);
        ctx.confirm_held = true;
        c.tick(ctx);
        ctx.ticks += HOLD_MS;
        assert!(
            matches!(c.tick(ctx), Transition::Pop),
            "a fresh press after release counts"
        );
        assert!(done.get());
    }

    #[test]
    fn back_cancels_a_hold_dialog_at_once() {
        let ctx = &mut Ctx::empty();
        let (mut c, done) = hold_dialog();
        assert!(matches!(
            c.handle(Input::Action(Action::Back), ctx),
            Transition::Pop
        ));
        assert!(!done.get());
    }

    #[test]
    fn right_then_left_then_confirm_on_cancel_cancels_a_hold_dialog() {
        let ctx = &mut Ctx::empty();
        let (mut c, done) = hold_dialog();
        c.handle(Input::Action(Action::Right), ctx);
        c.handle(Input::Action(Action::Left), ctx);
        assert_eq!(c.hints()[0], Hint::new(&[Action::Confirm], "Cancel"));
        assert!(matches!(
            c.handle(Input::Action(Action::Confirm), ctx),
            Transition::Pop
        ));
        assert!(!done.get());
    }

    #[test]
    fn a_click_on_delete_acts_at_once_in_hold_mode() {
        let ctx = &mut Ctx::empty();
        let (mut c, done) = hold_dialog();
        c.hits = vec![
            (Rect::new(100.0, 0.0, 50.0, 50.0), true),
            (Rect::new(0.0, 0.0, 50.0, 50.0), false),
        ];
        let click = |x| Input::Pointer(Pointer::Click { x, y: 10.0 });
        assert!(matches!(c.handle(click(110.0), ctx), Transition::Pop));
        assert!(done.get());
    }
}
