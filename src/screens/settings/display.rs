//! Settings → Display: Startup window mode, a cycled choice like Controls' Menu button style.

use super::{Row, Section};
use crate::ctx::Ctx;
use crate::ui::input::{Action, Input};
use crate::ui::nav::Transition;

const MODE: usize = 0;

pub struct Display;

impl Section for Display {
    fn name(&self) -> &'static str {
        "Display"
    }

    fn rows(&mut self, ctx: &Ctx) -> Vec<Row> {
        vec![
            Row::new("Startup window mode")
                .choice(ctx.window_mode.label())
                .sub("Applies the next time Game37 starts"),
        ]
    }

    /// Confirm on Startup window mode cycles it, as Left and Right do in [`Display::handle`].
    fn confirm(&mut self, i: usize, ctx: &mut Ctx) -> Transition {
        if i == MODE {
            ctx.set_window_mode(ctx.window_mode.next());
        }
        Transition::None
    }

    /// Left, Right and Confirm cycle the window mode at once. Confirm is handled here so
    /// Settings' own Confirm dispatch, which would otherwise also call [`Display::confirm`],
    /// never runs for it.
    fn handle(&mut self, i: usize, input: Input, ctx: &mut Ctx) -> Option<Transition> {
        if i != MODE {
            return None;
        }
        match input {
            Input::Action(Action::Right | Action::Confirm) => ctx.set_window_mode(ctx.window_mode.next()),
            Input::Action(Action::Left) => ctx.set_window_mode(ctx.window_mode.prev()),
            _ => return None,
        }
        Some(Transition::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::settings::Settings;
    use crate::ui::gfx::WindowMode;
    use crate::ui::input::{Action, Input};
    use crate::ui::nav::{Hits, Nav};

    /// Settings in a Nav over a root screen, with the focus on Display's first row. One more Down
    /// on Windows, where Emulators sits between BIOS and Cores.
    fn nav() -> Nav {
        let mut nav = Nav::single(Box::new(crate::screens::Stub::new("Home")));
        nav.push(Box::new(Settings::new()));
        let downs = if cfg!(windows) { 6 } else { 5 };
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
    fn cycling_the_window_mode_both_ways_saves_it() {
        let mut nav = nav();
        let ctx = &mut Ctx::empty();
        for style in [
            WindowMode::AlwaysFullscreen,
            WindowMode::AlwaysWindowed,
            WindowMode::RememberLast,
        ] {
            act(&mut nav, ctx, &[Action::Right]);
            assert_eq!(ctx.window_mode, style);
            assert_eq!(ctx.library.window_mode().unwrap(), style);
        }
        for style in [
            WindowMode::AlwaysWindowed,
            WindowMode::AlwaysFullscreen,
            WindowMode::RememberLast,
        ] {
            act(&mut nav, ctx, &[Action::Left]);
            assert_eq!(ctx.window_mode, style);
            assert_eq!(ctx.library.window_mode().unwrap(), style);
        }
        act(&mut nav, ctx, &[Action::Confirm]);
        assert_eq!(ctx.window_mode, WindowMode::AlwaysFullscreen);
        assert_eq!(ctx.library.window_mode().unwrap(), WindowMode::AlwaysFullscreen);
    }
}
