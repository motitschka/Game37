//! A small modal with a title, body text and one OK button, over the screen that opened it.

use crate::ctx::Ctx;
use crate::ui::cover::focus_ring;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, Pointer};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{Colour, LINE, MUTED, SURFACE, SURFACE2, TEXT};
use crate::ui::{H, Rect, W};

pub const PANEL_W: f32 = 960.0;
const PAD: f32 = 56.0;
const LINE_H: f32 = 34.0;
const BUTTON_W: f32 = 200.0;
const BUTTON_H: f32 = 64.0;

/// Dims everything drawn so far, top bar included, as `dim` does in `design/generate_mockups.py`.
pub fn dim(gfx: &mut Gfx<'_>) {
    gfx.fill_rect(Rect::new(0.0, 0.0, W, H), Colour::hex(0x000000).alpha(0.62));
}

/// A centred panel `h` tall; returns its rect.
pub fn panel(gfx: &mut Gfx<'_>, h: f32) -> Rect {
    let r = Rect::new((W - PANEL_W) / 2.0, (H - h) / 2.0, PANEL_W, h);
    gfx.fill_round_rect(r, 20.0, SURFACE);
    gfx.stroke_round_rect(r, 20.0, 1.0, LINE);
    r
}

pub struct Message {
    pub title: String,
    pub body: String,
    /// Where OK was last drawn, for the mouse.
    ok_hit: Option<Rect>,
}

impl Message {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Message {
        Message {
            title: title.into(),
            body: body.into(),
            ok_hit: None,
        }
    }
}

impl Screen for Message {
    fn title(&self) -> &str {
        &self.title
    }

    fn is_modal(&self) -> bool {
        true
    }

    fn handle(&mut self, input: Input, _: &mut Ctx) -> Transition {
        match input {
            Input::Action(Action::Confirm | Action::Back) | Input::Pointer(Pointer::Back) => Transition::Pop,
            Input::Pointer(Pointer::Click { x, y }) if self.ok_hit.is_some_and(|r| r.contains(x, y)) => {
                Transition::Pop
            }
            _ => Transition::None,
        }
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, _: &Ctx) {
        dim(gfx);
        let body = TextStyle::new(22, Weight::Regular, MUTED);
        let lines = gfx.wrap(&self.body, body, PANEL_W - 2.0 * PAD);
        let h = PAD + 34.0 + 24.0 + lines.len() as f32 * LINE_H + 48.0 + BUTTON_H + PAD;
        let p = panel(gfx, h);
        let x = p.x + PAD;
        let mut y = p.y + PAD + 34.0;
        gfx.text(
            x,
            y,
            &self.title,
            TextStyle::new(34, Weight::Bold, TEXT),
            Align::Left,
        );
        y += 24.0;
        for line in &lines {
            y += LINE_H;
            gfx.text(x, y, line, body, Align::Left);
        }
        let ok = Rect::new(
            p.x + p.w - PAD - BUTTON_W,
            p.y + p.h - PAD - BUTTON_H,
            BUTTON_W,
            BUTTON_H,
        );
        focus_ring(gfx, ok, 14.0);
        gfx.fill_round_rect(ok, 14.0, SURFACE2);
        gfx.text(
            ok.x + ok.w / 2.0,
            ok.y + ok.h / 2.0 + 8.0,
            "OK",
            TextStyle::new(22, Weight::Bold, TEXT),
            Align::Centre,
        );
        self.ok_hit = Some(ok);
    }

    fn hints(&self) -> Vec<Hint> {
        vec![Hint::new(&[Action::Confirm], "OK")]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_back_or_a_click_on_ok_closes_it_and_nothing_else_does() {
        let ctx = &mut Ctx::empty();
        let mut m = Message::new("T", "B");
        for input in [Input::Action(Action::Confirm), Input::Action(Action::Back)] {
            assert!(matches!(m.handle(input, ctx), Transition::Pop));
        }
        let click = Input::Pointer(Pointer::Click { x: 10.0, y: 10.0 });
        assert!(matches!(m.handle(click, ctx), Transition::None), "not drawn yet");
        m.ok_hit = Some(Rect::new(0.0, 0.0, 20.0, 20.0));
        assert!(matches!(m.handle(click, ctx), Transition::Pop));
        assert!(matches!(
            m.handle(Input::Action(Action::NextTab), ctx),
            Transition::None
        ));
    }
}
