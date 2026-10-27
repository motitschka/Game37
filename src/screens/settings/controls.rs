//! Settings → Controls, after `screen_settings_controls` in `design/generate_mockups.py`: Menu
//! button style, a cycled choice, and Controller test, which pushes [`ControllerTest`].

use super::{Row, Section};
use crate::ctx::Ctx;
use crate::screens::controller_test::ControllerTest;
use crate::ui::input::{Action, Input};
use crate::ui::nav::Transition;

const STYLE: usize = 0;
const TEST: usize = 1;

pub struct Controls;

impl Section for Controls {
    fn name(&self) -> &'static str {
        "Controls"
    }

    fn rows(&mut self, ctx: &Ctx) -> Vec<Row> {
        let style = ctx.button_style;
        vec![
            Row::new("Menu button style")
                .choice(style.label())
                .sub(style.description()),
            Row::new("Controller test").opens(),
        ]
    }

    /// Confirm on Menu button style cycles it, as Left and Right do in [`Controls::handle`]; on
    /// Controller test it pushes [`ControllerTest`].
    fn confirm(&mut self, i: usize, ctx: &mut Ctx) -> Transition {
        match i {
            STYLE => ctx.set_button_style(ctx.button_style.next()),
            TEST => return Transition::Push(Box::new(ControllerTest::new())),
            _ => {}
        }
        Transition::None
    }

    /// Left, Right and Confirm on Menu button style cycle it at once, so the very next press
    /// already uses the new mapping. Confirm is handled here so Settings' own Confirm dispatch,
    /// which would otherwise also call [`Controls::confirm`], never runs for it.
    fn handle(&mut self, i: usize, input: Input, ctx: &mut Ctx) -> Option<Transition> {
        if i != STYLE {
            return None;
        }
        match input {
            Input::Action(Action::Right | Action::Confirm) => ctx.set_button_style(ctx.button_style.next()),
            Input::Action(Action::Left) => ctx.set_button_style(ctx.button_style.prev()),
            _ => return None,
        }
        Some(Transition::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::settings::Settings;
    use crate::ui::input::{Action, ButtonStyle, Input, button_action};
    use crate::ui::nav::{Hint, Hits, Nav};
    use sdl3::gamepad::Button;

    /// Settings in a Nav over a root screen, with the focus on Controls' first row. One more Down
    /// on Windows, where Emulators sits between BIOS and Cores.
    fn nav() -> Nav {
        let mut nav = Nav::single(Box::new(crate::screens::Stub::new("Home")));
        nav.push(Box::new(Settings::new()));
        let downs = if cfg!(windows) { 5 } else { 4 };
        let mut actions = vec![Action::Down; downs];
        actions.push(Action::Right);
        act(&mut nav, &mut Ctx::empty(), &actions);
        nav
    }

    fn act(nav: &mut Nav, ctx: &mut Ctx, actions: &[Action]) {
        for &action in actions {
            nav.handle(Input::Action(action), &Hits::default(), ctx);
        }
    }

    #[test]
    fn cycling_the_button_style_both_ways_saves_it() {
        let mut nav = nav();
        let ctx = &mut Ctx::empty();
        for _ in 0..ButtonStyle::ALL.len() {
            act(&mut nav, ctx, &[Action::Right]);
        }
        assert_eq!(
            ctx.library.button_style().unwrap(),
            ButtonStyle::Nintendo,
            "a full cycle"
        );
        for style in [ButtonStyle::Xbox, ButtonStyle::PlayStation, ButtonStyle::Nintendo] {
            act(&mut nav, ctx, &[Action::Right]);
            assert_eq!(ctx.button_style, style);
            assert_eq!(ctx.library.button_style().unwrap(), style);
        }
        for style in [ButtonStyle::PlayStation, ButtonStyle::Xbox, ButtonStyle::Nintendo] {
            act(&mut nav, ctx, &[Action::Left]);
            assert_eq!(ctx.button_style, style);
            assert_eq!(ctx.library.button_style().unwrap(), style);
        }
        act(&mut nav, ctx, &[Action::Confirm]);
        assert_eq!(ctx.button_style, ButtonStyle::Xbox);
        assert_eq!(ctx.library.button_style().unwrap(), ButtonStyle::Xbox);
    }

    #[test]
    fn after_switching_to_xbox_a_south_button_press_confirms() {
        let mut nav = nav();
        let ctx = &mut Ctx::empty();
        act(&mut nav, ctx, &[Action::Right]);
        assert_eq!(ctx.button_style, ButtonStyle::Xbox);
        assert_eq!(
            button_action(ctx.button_style, Button::South),
            Some(Action::Confirm)
        );
        assert_eq!(button_action(ctx.button_style, Button::East), Some(Action::Back));
    }

    #[test]
    fn the_choice_row_hints_change_and_back() {
        let mut nav = nav();
        let ctx = &mut Ctx::empty();
        act(&mut nav, ctx, &[Action::Right]);
        assert_eq!(
            nav.top().hints(),
            [
                Hint::new(&[Action::Left, Action::Right], "Change"),
                Hint::new(&[Action::Back], "Back"),
            ]
        );
    }

    #[test]
    fn confirm_on_controller_test_pushes_it() {
        let mut nav = nav();
        let ctx = &mut Ctx::empty();
        act(&mut nav, ctx, &[Action::Down, Action::Confirm]);
        assert_eq!(nav.top().title(), "Controller test");
    }
}
