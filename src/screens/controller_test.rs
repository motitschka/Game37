//! The Controller test (US-34, FR-SET-1): a screen pushed from Settings that draws what Game37
//! receives from a controller, its buttons lit, its sticks pushed and its triggers filled. Controller
//! buttons don't act here, so a controller leaves by holding Back; Esc and the "‹ Settings" click
//! leave at once.

use sdl3::gamepad::Button as Pad;

use crate::ctx::Ctx;
use crate::session::input::{ALL_BUTTONS, PadState};
use crate::ui::cover::ellipsize;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, ButtonStyle, Input, InputKind, button_action, glyph_label};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::theme::{ACCENT, BG, Colour, LINE, MUTED, SURFACE, SURFACE2, TEXT};
use crate::ui::{M, Rect, W};

/// How long Back must be held on a controller to leave.
const HOLD_TO_EXIT_MS: u64 = 1500;

const TOP: f32 = 240.0;
/// Left edge of the column right of the controller list, as on the Settings pages.
const RX: f32 = M + 448.0;
/// Centre x of the drawn pad.
const CX: f32 = (RX + W - M) / 2.0;
const CARD_W: f32 = 400.0;
const CARD_PITCH: f32 = 76.0;
/// How far a stick's cap moves at full deflection.
const STICK_TRAVEL: f32 = 26.0;

/// The face buttons by where they sit on the pad, with their offset from the cluster's centre;
/// [`PadView::face`] and [`PadView::dpad`] follow the same order: top, right, bottom, left.
const FACE: [(Pad, f32, f32); 4] = [
    (Pad::North, 0.0, -50.0),
    (Pad::East, 50.0, 0.0),
    (Pad::South, 0.0, 50.0),
    (Pad::West, -50.0, 0.0),
];
const DPAD: [Pad; 4] = [Pad::DPadUp, Pad::DPadRight, Pad::DPadDown, Pad::DPadLeft];

pub struct ControllerTest {
    hold: HoldToExit,
    /// What the player last used, so the hint names the right way to leave: a controller holds
    /// Back, but on the keyboard Esc leaves at once.
    kind: InputKind,
}

impl Default for ControllerTest {
    fn default() -> ControllerTest {
        ControllerTest {
            hold: HoldToExit::default(),
            kind: InputKind::Controller,
        }
    }
}

impl ControllerTest {
    pub fn new() -> ControllerTest {
        ControllerTest::default()
    }
}

/// Times Back held on a controller: since when it has been held, if it is.
#[derive(Debug, Default)]
struct HoldToExit {
    since: Option<u64>,
}

impl HoldToExit {
    /// Whether Back has now been held long enough to leave. Releasing it starts the count over.
    fn update(&mut self, held: bool, now: u64) -> bool {
        if !held {
            self.since = None;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        now.saturating_sub(since) >= HOLD_TO_EXIT_MS
    }
}

/// Whether the button the menus go back with is held on any connected controller.
fn back_held(ctx: &Ctx) -> bool {
    let back = ALL_BUTTONS
        .into_iter()
        .find(|&b| button_action(ctx.button_style, b) == Some(Action::Back));
    back.is_some_and(|b| ctx.controllers.iter().any(|c| c.pad.is_pressed(b)))
}

/// Which controller the screen shows: the last-used one, else the first connected.
fn shown(ctx: &Ctx) -> Option<usize> {
    ctx.last_controller
        .filter(|&i| i < ctx.controllers.len())
        .or((!ctx.controllers.is_empty()).then_some(0))
}

/// A face button as drawn: its label and whether it is pressed.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Lamp {
    label: &'static str,
    lit: bool,
}

/// A stick as drawn: its position from −1 to 1 on each axis (positive y is down), and whether it
/// is pressed in.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Stick {
    x: f32,
    y: f32,
    pressed: bool,
}

impl Stick {
    fn new(x: i16, y: i16, pressed: bool) -> Stick {
        Stick {
            x: axis(x),
            y: axis(y),
            pressed,
        }
    }

    /// Its position as "x 0.62 · y −0.41", in hundredths, never showing −0.00.
    fn readout(&self) -> String {
        format!("x {} · y {}", hundredths(self.x), hundredths(self.y))
    }

    /// Whether it is off centre by at least what the readout shows.
    fn moved(&self) -> bool {
        (self.x * 100.0).round() != 0.0 || (self.y * 100.0).round() != 0.0
    }
}

fn axis(raw: i16) -> f32 {
    (raw as f32 / i16::MAX as f32).clamp(-1.0, 1.0)
}

/// `v` rounded to two decimals, with a true minus sign; rounded first, so a small negative shows as
/// 0.00.
fn hundredths(v: f32) -> String {
    let n = (v * 100.0).round() as i32;
    let sign = if n < 0 { "−" } else { "" };
    format!("{sign}{}.{:02}", n.abs() / 100, n.abs() % 100)
}

/// A trigger as drawn: how far it is pulled, from 0 to 1, and its label with the percentage once
/// it is pulled ("R2 60%").
#[derive(Debug, Clone, PartialEq)]
struct Trigger {
    amount: f32,
    label: String,
}

impl Trigger {
    fn new(name: &str, raw: i16) -> Trigger {
        let amount = (raw as f32 / i16::MAX as f32).clamp(0.0, 1.0);
        let percent = (amount * 100.0).round();
        let label = if percent > 0.0 {
            format!("{name} {percent}%")
        } else {
            name.to_owned()
        };
        Trigger { amount, label }
    }
}

/// Everything the pad drawing shows for one controller's state this frame.
#[derive(Debug, Clone, PartialEq)]
struct PadView {
    dpad: [bool; 4],
    face: [Lamp; 4],
    l1: bool,
    r1: bool,
    l2: Trigger,
    r2: Trigger,
    left: Stick,
    right: Stick,
    select: bool,
    start: bool,
    guide: bool,
}

impl PadView {
    /// The face buttons are labelled as the menus label them in `style`.
    fn new(pad: &PadState, style: ButtonStyle) -> PadView {
        let label = |b| button_action(style, b).map_or("", |a| glyph_label(a, InputKind::Controller, style));
        PadView {
            dpad: DPAD.map(|b| pad.is_pressed(b)),
            face: FACE.map(|(b, _, _)| Lamp {
                label: label(b),
                lit: pad.is_pressed(b),
            }),
            l1: pad.is_pressed(Pad::LeftShoulder),
            r1: pad.is_pressed(Pad::RightShoulder),
            l2: Trigger::new("L2", pad.left_trigger),
            r2: Trigger::new("R2", pad.right_trigger),
            left: Stick::new(pad.left_x, pad.left_y, pad.is_pressed(Pad::LeftStick)),
            right: Stick::new(pad.right_x, pad.right_y, pad.is_pressed(Pad::RightStick)),
            select: pad.is_pressed(Pad::Back),
            start: pad.is_pressed(Pad::Start),
            guide: pad.is_pressed(Pad::Guide),
        }
    }
}

impl Screen for ControllerTest {
    fn title(&self) -> &str {
        "Controller test"
    }

    fn handle(&mut self, input: Input, _: &mut Ctx) -> Transition {
        match input {
            Input::Action(Action::Back) => Transition::Pop,
            _ => Transition::None,
        }
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Transition {
        self.kind = ctx.input_kind;
        if self.hold.update(back_held(ctx), ctx.ticks) {
            Transition::Pop
        } else {
            Transition::None
        }
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        gfx.text(
            M,
            196.0,
            self.title(),
            TextStyle::new(48, Weight::ExtraBold, TEXT),
            Align::Left,
        );
        gfx.text(
            M,
            TOP + 28.0,
            "Controllers",
            TextStyle::new(20, Weight::Bold, MUTED),
            Align::Left,
        );
        let shown = shown(ctx);
        for (i, controller) in ctx.controllers.iter().enumerate() {
            let y = TOP + 52.0 + i as f32 * CARD_PITCH;
            let style = if shown == Some(i) {
                gfx.fill_round_rect(Rect::new(M - 24.0, y, CARD_W, 64.0), 14.0, SURFACE2);
                gfx.fill_round_rect(Rect::new(M - 24.0, y + 12.0, 5.0, 40.0), 2.0, ACCENT);
                TextStyle::new(20, Weight::Bold, TEXT)
            } else {
                TextStyle::new(20, Weight::Medium, MUTED)
            };
            let name = ellipsize(&controller.name, CARD_W - 48.0, |s| gfx.measure(s, style));
            gfx.text(M, y + 40.0, &name, style, Align::Left);
        }
        gfx.fill_rect(Rect::new(M + CARD_W, TOP, 1.0, 740.0), LINE);
        let Some(shown) = shown else {
            gfx.text(
                CX,
                540.0,
                "Connect a controller to test it.",
                TextStyle::new(28, Weight::Medium, MUTED),
                Align::Centre,
            );
            return;
        };
        draw_pad(gfx, &PadView::new(&ctx.controllers[shown].pad, ctx.button_style));
        gfx.text(
            CX,
            920.0,
            "Menu buttons don't act here.",
            TextStyle::new(20, Weight::Medium, MUTED),
            Align::Centre,
        );
    }

    fn hints(&self) -> Vec<Hint> {
        let text = match self.kind {
            InputKind::Controller => "Hold to exit",
            InputKind::Keyboard => "Back",
        };
        vec![Hint::new(&[Action::Back], text)]
    }

    fn raw_controller(&self) -> bool {
        true
    }
}

/// The pad's body and two grips, outlined as one shape: the outline pass first, then the fill.
fn draw_body(gfx: &mut Gfx<'_>) {
    let body = Rect::new(CX - 390.0, 400.0, 780.0, 300.0);
    for (colour, grow) in [(LINE, 2.0), (SURFACE, 0.0)] {
        for (gx, degrees) in [(CX - 290.0, 20.0), (CX + 290.0, -20.0)] {
            gfx.fill_polygon(&ellipse(gx, 690.0, 110.0 + grow, 160.0 + grow, degrees), colour);
        }
        gfx.fill_round_rect(body.inset(-grow), 150.0 + grow, colour);
    }
}

/// An ellipse's outline as a polygon, rotated clockwise by `degrees` about its centre, as SVG's
/// `rotate` turns it.
fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32, degrees: f32) -> Vec<(f32, f32)> {
    const STEPS: usize = 72;
    let (sin, cos) = degrees.to_radians().sin_cos();
    (0..STEPS)
        .map(|i| {
            let t = i as f32 / STEPS as f32 * std::f32::consts::TAU;
            let (x, y) = (rx * t.cos(), ry * t.sin());
            (cx + x * cos - y * sin, cy + x * sin + y * cos)
        })
        .collect()
}

fn draw_pad(gfx: &mut Gfx<'_>, view: &PadView) {
    draw_body(gfx);
    pad_trigger(gfx, CX - 330.0, 300.0, 170.0, &view.l2);
    pad_trigger(gfx, CX + 160.0, 300.0, 170.0, &view.r2);
    pad_shoulder(gfx, CX - 330.0, 346.0, 170.0, "L1", view.l1);
    pad_shoulder(gfx, CX + 160.0, 346.0, 170.0, "R1", view.r1);
    pad_stick(gfx, CX - 240.0, 490.0, view.left);
    gfx.text(
        CX - 240.0,
        580.0,
        &view.left.readout(),
        TextStyle::new(16, Weight::Medium, MUTED),
        Align::Centre,
    );
    pad_dpad(gfx, CX - 120.0, 640.0, view.dpad);
    let (fx, fy) = (CX + 240.0, 490.0);
    for (&(_, ox, oy), lamp) in FACE.iter().zip(view.face) {
        pad_button(gfx, fx + ox, fy + oy, 22.0, lamp);
    }
    pad_stick(gfx, CX + 120.0, 640.0, view.right);
    let label = |lit| TextStyle::new(14, Weight::SemiBold, if lit { TEXT } else { MUTED });
    let (guide_fill, guide_stroke) = lamp_colours(view.guide);
    gfx.fill_circle(CX, 450.0, 26.0, guide_fill);
    gfx.stroke_circle(CX, 450.0, 26.0, 2.0, guide_stroke);
    gfx.text(CX, 498.0, "Guide", label(view.guide), Align::Centre);
    for (text, sx, lit) in [
        ("Select", CX - 80.0, view.select),
        ("Start", CX + 80.0, view.start),
    ] {
        let (fill, stroke) = lamp_colours(lit);
        let r = Rect::new(sx - 22.0, 516.0, 44.0, 22.0);
        gfx.fill_round_rect(r, 11.0, fill);
        gfx.stroke_round_rect(r, 11.0, 2.0, stroke);
        gfx.text(sx, 560.0, text, label(lit), Align::Centre);
    }
}

/// A pressable part's fill and outline: accent while pressed.
fn lamp_colours(lit: bool) -> (Colour, Colour) {
    if lit { (ACCENT, ACCENT) } else { (SURFACE2, LINE) }
}

fn pad_button(gfx: &mut Gfx<'_>, cx: f32, cy: f32, r: f32, lamp: Lamp) {
    let (fill, stroke) = lamp_colours(lamp.lit);
    gfx.fill_circle(cx, cy, r, fill);
    gfx.stroke_circle(cx, cy, r, 2.0, stroke);
    let ink = if lamp.lit { TEXT } else { MUTED };
    gfx.text(
        cx,
        cy + 6.0,
        lamp.label,
        TextStyle::new(17, Weight::Bold, ink),
        Align::Centre,
    );
}

/// A stick well with its cap pushed to the stick's position; the cap turns accent while the stick is
/// pressed in.
fn pad_stick(gfx: &mut Gfx<'_>, cx: f32, cy: f32, stick: Stick) {
    gfx.fill_circle(cx, cy, 56.0, BG);
    gfx.stroke_circle(cx, cy, 56.0, 2.0, LINE);
    let (x, y) = (cx + stick.x * STICK_TRAVEL, cy + stick.y * STICK_TRAVEL);
    let (fill, stroke) = match (stick.pressed, stick.moved()) {
        (true, _) => (ACCENT, ACCENT),
        (false, true) => (TEXT.alpha(0.9), LINE),
        (false, false) => (SURFACE2, LINE),
    };
    gfx.fill_circle(x, y, 30.0, fill);
    gfx.stroke_circle(x, y, 30.0, 2.0, stroke);
}

fn pad_trigger(gfx: &mut Gfx<'_>, x: f32, y: f32, w: f32, trigger: &Trigger) {
    let pulled = trigger.amount > 0.0;
    let ink = if pulled { TEXT } else { MUTED };
    gfx.text(
        x + w / 2.0,
        y - 12.0,
        &trigger.label,
        TextStyle::new(16, Weight::Bold, ink),
        Align::Centre,
    );
    let bar = Rect::new(x, y, w, 20.0);
    gfx.fill_round_rect(bar, 10.0, SURFACE2);
    gfx.stroke_round_rect(bar, 10.0, 1.0, LINE);
    if pulled {
        let fill_w = w * trigger.amount;
        gfx.fill_round_rect(Rect::new(x, y, fill_w, 20.0), 10.0_f32.min(fill_w / 2.0), ACCENT);
    }
}

fn pad_shoulder(gfx: &mut Gfx<'_>, x: f32, y: f32, w: f32, label: &str, lit: bool) {
    let (fill, stroke) = lamp_colours(lit);
    let r = Rect::new(x, y, w, 40.0);
    gfx.fill_round_rect(r, 20.0, fill);
    gfx.stroke_round_rect(r, 20.0, 2.0, stroke);
    let ink = if lit { TEXT } else { MUTED };
    gfx.text(
        x + w / 2.0,
        y + 27.0,
        label,
        TextStyle::new(17, Weight::Bold, ink),
        Align::Centre,
    );
}

/// The d-pad's cross, each pressed arm filled in accent.
fn pad_dpad(gfx: &mut Gfx<'_>, dx: f32, dy: f32, pressed: [bool; 4]) {
    for r in [
        Rect::new(dx - 18.0, dy - 50.0, 36.0, 100.0),
        Rect::new(dx - 50.0, dy - 18.0, 100.0, 36.0),
    ] {
        gfx.fill_round_rect(r, 6.0, SURFACE2);
        gfx.stroke_round_rect(r, 6.0, 2.0, LINE);
    }
    gfx.fill_rect(Rect::new(dx - 16.0, dy - 16.0, 32.0, 32.0), SURFACE2);
    let arms = [
        Rect::new(dx - 18.0, dy - 50.0, 36.0, 32.0),
        Rect::new(dx + 18.0, dy - 18.0, 32.0, 36.0),
        Rect::new(dx - 18.0, dy + 18.0, 36.0, 32.0),
        Rect::new(dx - 50.0, dy - 18.0, 32.0, 36.0),
    ];
    for (arm, _) in arms.into_iter().zip(pressed).filter(|&(_, lit)| lit) {
        gfx.fill_round_rect(arm, 6.0, ACCENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::Controller;
    use crate::screens::Stub;
    use crate::ui::nav::{Hits, Nav};

    fn ctx_with(pads: &[PadState]) -> Ctx {
        let mut ctx = Ctx::empty();
        ctx.controllers = pads
            .iter()
            .enumerate()
            .map(|(i, &pad)| Controller {
                name: format!("Pad {i}"),
                pad,
            })
            .collect();
        ctx
    }

    fn pressed(button: Pad) -> PadState {
        let mut pad = PadState::default();
        pad.press(button);
        pad
    }

    /// The Controller test pushed over Settings.
    fn nav() -> Nav {
        let mut nav = Nav::single(Box::new(Stub::new("Settings")));
        nav.push(Box::new(ControllerTest::new()));
        nav
    }

    /// Ticks the top screen at `ticks` and returns how many screens are left.
    fn tick_at(nav: &mut Nav, ctx: &mut Ctx, ticks: u64) -> usize {
        ctx.ticks = ticks;
        nav.tick(ctx);
        nav.depth()
    }

    #[test]
    fn pressed_buttons_are_lit_and_face_buttons_labelled_as_the_menus_label_them() {
        let mut pad = pressed(Pad::East);
        pad.press(Pad::DPadLeft);
        pad.press(Pad::LeftShoulder);
        pad.press(Pad::Start);
        let view = PadView::new(&pad, ButtonStyle::Nintendo);
        assert_eq!(
            view.face.map(|l| l.label),
            ["X", "A", "B", "Y"],
            "A on the right, B at the bottom"
        );
        assert_eq!(view.face.map(|l| l.lit), [false, true, false, false]);
        assert_eq!(view.dpad, [false, false, false, true]);
        assert_eq!((view.l1, view.r1), (true, false));
        assert_eq!((view.select, view.start, view.guide), (false, true, false));
        assert!(!view.left.pressed && !view.right.pressed);
        assert!(
            PadView::new(&pressed(Pad::RightStick), ButtonStyle::Nintendo)
                .right
                .pressed
        );
    }

    #[test]
    fn playstation_face_buttons_are_labelled_with_their_glyphs() {
        let view = PadView::new(&PadState::default(), ButtonStyle::PlayStation);
        assert_eq!(
            view.face.map(|l| l.label),
            ["△", "○", "✕", "□"],
            "top (Sort), right (Back), bottom (Confirm), left (Options)"
        );
    }

    #[test]
    fn sticks_are_normalised_and_read_out_in_hundredths() {
        let stick = Stick::new(
            (0.62 * i16::MAX as f32) as i16,
            (-0.41 * i16::MAX as f32) as i16,
            false,
        );
        assert_eq!(stick.readout(), "x 0.62 · y \u{2212}0.41");
        assert!(stick.moved());
        let full = Stick::new(i16::MIN, i16::MAX, false);
        assert_eq!((full.x, full.y), (-1.0, 1.0), "clamped to -1…1");
        assert_eq!(full.readout(), "x \u{2212}1.00 · y 1.00");
        let rest = Stick::new(-100, 50, false);
        assert_eq!(rest.readout(), "x 0.00 · y 0.00", "never -0.00");
        assert!(!rest.moved());
    }

    #[test]
    fn a_pulled_trigger_shows_its_percentage() {
        let r2 = Trigger::new("R2", (0.6 * i16::MAX as f32) as i16);
        assert_eq!(r2.label, "R2 60%");
        assert!((r2.amount - 0.6).abs() < 0.001);
        let l2 = Trigger::new("L2", 0);
        assert_eq!((l2.amount, l2.label.as_str()), (0.0, "L2"));
        assert_eq!(Trigger::new("L2", i16::MAX).label, "L2 100%");
    }

    #[test]
    fn without_a_controller_nothing_is_shown_and_nothing_leaves() {
        let ctx = &mut ctx_with(&[]);
        assert_eq!(shown(ctx), None);
        let mut nav = nav();
        assert_eq!(tick_at(&mut nav, ctx, 0), 2);
        assert_eq!(tick_at(&mut nav, ctx, 5000), 2);
    }

    #[test]
    fn the_last_used_controller_is_shown_else_the_first() {
        let ctx = &mut ctx_with(&[PadState::default(), PadState::default()]);
        assert_eq!(shown(ctx), Some(0));
        ctx.last_controller = Some(1);
        assert_eq!(shown(ctx), Some(1));
        ctx.last_controller = Some(2);
        assert_eq!(shown(ctx), Some(0), "one since disconnected");
    }

    #[test]
    fn holding_back_on_any_controller_for_1500_ms_leaves() {
        let ctx = &mut ctx_with(&[PadState::default(), pressed(Pad::South)]);
        let mut nav = nav();
        assert!(nav.top().raw_controller());
        assert_eq!(tick_at(&mut nav, ctx, 1000), 2);
        assert_eq!(tick_at(&mut nav, ctx, 2499), 2);
        assert_eq!(tick_at(&mut nav, ctx, 2500), 1);
        assert_eq!(nav.top().title(), "Settings");
    }

    #[test]
    fn holding_back_leaves_with_the_xbox_style_too_where_back_is_east_not_south() {
        {
            let ctx = &mut ctx_with(&[pressed(Pad::East)]);
            ctx.button_style = ButtonStyle::Xbox;
            let mut nav = nav();
            assert_eq!(tick_at(&mut nav, ctx, 0), 2);
            assert_eq!(tick_at(&mut nav, ctx, 1500), 1, "East is Back on Xbox");
        }
        let ctx = &mut ctx_with(&[pressed(Pad::South)]);
        ctx.button_style = ButtonStyle::Xbox;
        let mut nav = nav();
        assert_eq!(tick_at(&mut nav, ctx, 0), 2);
        assert_eq!(
            tick_at(&mut nav, ctx, 1500),
            2,
            "South is Confirm on Xbox, not Back"
        );
    }

    #[test]
    fn only_back_counts_as_holding_to_exit() {
        let ctx = &mut ctx_with(&[pressed(Pad::East)]);
        let mut nav = nav();
        assert_eq!(tick_at(&mut nav, ctx, 0), 2);
        assert_eq!(tick_at(&mut nav, ctx, 2000), 2);
    }

    #[test]
    fn releasing_back_starts_the_count_over() {
        let ctx = &mut ctx_with(&[pressed(Pad::South)]);
        let mut nav = nav();
        assert_eq!(tick_at(&mut nav, ctx, 0), 2);
        assert_eq!(tick_at(&mut nav, ctx, 1400), 2);
        ctx.controllers[0].pad = PadState::default();
        assert_eq!(tick_at(&mut nav, ctx, 1450), 2);
        ctx.controllers[0].pad = pressed(Pad::South);
        assert_eq!(tick_at(&mut nav, ctx, 1500), 2);
        assert_eq!(tick_at(&mut nav, ctx, 2999), 2);
        assert_eq!(tick_at(&mut nav, ctx, 3000), 1);
    }

    #[test]
    fn esc_leaves_at_once() {
        let ctx = &mut ctx_with(&[]);
        let mut nav = nav();
        nav.handle(Input::Action(Action::Back), &Hits::default(), ctx);
        assert_eq!((nav.depth(), nav.top().title()), (1, "Settings"));
    }

    #[test]
    fn the_hint_bar_shows_only_hold_to_exit() {
        assert_eq!(
            ControllerTest::new().hints(),
            vec![Hint::new(&[Action::Back], "Hold to exit")]
        );
    }

    #[test]
    fn the_hint_names_hold_to_exit_on_a_controller_and_back_on_the_keyboard() {
        let ctx = &mut ctx_with(&[]);
        let mut nav = nav();
        ctx.input_kind = InputKind::Controller;
        tick_at(&mut nav, ctx, 0);
        assert_eq!(
            nav.top().hints(),
            vec![Hint::new(&[Action::Back], "Hold to exit")]
        );

        ctx.input_kind = InputKind::Keyboard;
        tick_at(&mut nav, ctx, 0);
        assert_eq!(nav.top().hints(), vec![Hint::new(&[Action::Back], "Back")]);
    }
}
