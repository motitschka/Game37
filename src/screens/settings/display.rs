//! Settings → Display (`screen_settings_display` in the mockups): Startup window mode and Motion,
//! then the Screens group with each two-screen System's layout (FR-DS-1, FR-NEW-2), all cycled
//! choices like Controls' Menu button style.

use super::{Row, Section};
use crate::cores::{self, ScreenLayouts};
use crate::ctx::Ctx;
use crate::systems::{SYSTEMS, System};
use crate::ui::input::{Action, Input};
use crate::ui::nav::Transition;

const MODE: usize = 0;
const MOTION: usize = 1;
/// The rows before the Screens group.
const LEADING: usize = 2;

/// The Systems with two screens, in System Order: one layout row each, after Startup window mode.
fn two_screen_systems() -> impl Iterator<Item = (&'static System, &'static ScreenLayouts)> {
    SYSTEMS
        .iter()
        .filter_map(|s| cores::screen_layouts(s).map(|layouts| (s, layouts)))
}

pub struct Display;

impl Section for Display {
    fn name(&self) -> &'static str {
        "Display"
    }

    fn rows(&mut self, ctx: &Ctx) -> Vec<Row> {
        let mode = Row::new("Startup window mode")
            .choice(ctx.window_mode.label())
            .sub("Applies the next time Game37 starts");
        let motion = Row::new("Motion")
            .choice(if ctx.animate { "Full" } else { "Reduced" })
            .sub("Reduced turns off fades, slides and gliding");
        let layouts = two_screen_systems().enumerate().map(|(i, (system, layouts))| {
            let layout = ctx.screen_layout(system);
            let row = Row::new(layouts.label).choice(layout.label()).sub(layout.sub());
            if i == 0 { row.group("Screens") } else { row }
        });
        [mode, motion].into_iter().chain(layouts).collect()
    }

    /// Confirm on a row cycles its choice, as Left and Right do in [`Display::handle`].
    fn confirm(&mut self, i: usize, ctx: &mut Ctx) -> Transition {
        cycle(i, true, ctx);
        Transition::None
    }

    /// Left, Right and Confirm cycle the focused row's choice at once. Confirm is handled here so
    /// Settings' own Confirm dispatch, which would otherwise also call [`Display::confirm`],
    /// never runs for it.
    fn handle(&mut self, i: usize, input: Input, ctx: &mut Ctx) -> Option<Transition> {
        let forward = match input {
            Input::Action(Action::Right | Action::Confirm) => true,
            Input::Action(Action::Left) => false,
            _ => return None,
        };
        cycle(i, forward, ctx).then_some(Transition::None)
    }
}

/// Moves row `i`'s choice to the next one, or the previous one, and saves it. False if there is no
/// row `i`.
fn cycle(i: usize, forward: bool, ctx: &mut Ctx) -> bool {
    if i == MODE {
        let mode = if forward {
            ctx.window_mode.next()
        } else {
            ctx.window_mode.prev()
        };
        ctx.set_window_mode(mode);
        return true;
    }
    if i == MOTION {
        // Two choices: either direction flips it.
        ctx.set_reduced_motion(ctx.animate);
        return true;
    }
    let Some((system, _)) = two_screen_systems().nth(i - LEADING) else {
        return false;
    };
    let layout = ctx.screen_layout(system);
    ctx.set_screen_layout(system, if forward { layout.next() } else { layout.prev() });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cores::ScreenLayout;
    use crate::screens::settings::Settings;
    use crate::ui::gfx::WindowMode;
    use crate::ui::input::{Action, Input};
    use crate::ui::nav::{Hits, Nav};

    /// Settings in a Nav over a root screen, with the focus on Display's first row.
    fn nav() -> Nav {
        let mut nav = Nav::single(Box::new(crate::screens::Stub::new("Home")));
        nav.push(Box::new(Settings::new()));
        act(
            &mut nav,
            &mut Ctx::empty(),
            &[
                Action::Down,
                Action::Down,
                Action::Down,
                Action::Down,
                Action::Down,
                Action::Right,
            ],
        );
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

    #[test]
    fn the_screens_group_has_a_layout_row_for_ds_then_3ds_large_and_small_until_changed() {
        let rows = Display.rows(&Ctx::empty());
        let labels: Vec<_> = rows
            .iter()
            .map(|r| (r.label, r.value.as_deref(), r.group))
            .collect();
        assert_eq!(
            labels,
            [
                ("Startup window mode", Some("Remember last"), None),
                ("Motion", Some("Reduced"), None),
                ("DS layout", Some("Large and small"), Some("Screens")),
                ("3DS layout", Some("Large and small"), None),
            ]
        );
        assert!(rows.iter().all(|r| r.choice));
        assert_eq!(rows[2].sub, Some(ScreenLayout::LargeAndSmall.sub()));
    }

    /// Down to the DS layout row and Right, Left or Confirm cycle and save DS's layout, which the
    /// row then shows, and leave 3DS's alone; one more Down does the same for 3DS.
    #[test]
    fn cycling_a_layout_row_saves_that_systems_layout() {
        let mut nav = nav();
        let ctx = &mut Ctx::empty();
        let saved = |ctx: &Ctx, id| ctx.library.screen_layout(id).unwrap();
        act(&mut nav, ctx, &[Action::Down, Action::Down, Action::Right]);
        assert_eq!(saved(ctx, "nds"), ScreenLayout::SideBySide);
        assert_eq!(saved(ctx, "3ds"), ScreenLayout::LargeAndSmall);
        assert_eq!(Display.rows(ctx)[2].value.as_deref(), Some("Side by side"));
        assert_eq!(Display.rows(ctx)[2].sub, Some(ScreenLayout::SideBySide.sub()));
        act(&mut nav, ctx, &[Action::Confirm]);
        assert_eq!(saved(ctx, "nds"), ScreenLayout::OneScreen);
        act(&mut nav, ctx, &[Action::Right]);
        assert_eq!(saved(ctx, "nds"), ScreenLayout::LargeAndSmall, "wraps around");
        act(&mut nav, ctx, &[Action::Left]);
        assert_eq!(saved(ctx, "nds"), ScreenLayout::OneScreen);
        act(&mut nav, ctx, &[Action::Down, Action::Left]);
        assert_eq!(saved(ctx, "3ds"), ScreenLayout::OneScreen);
        assert_eq!(saved(ctx, "nds"), ScreenLayout::OneScreen);
        assert_eq!(ctx.window_mode, WindowMode::RememberLast, "untouched");
    }

    /// The Motion row flips between Full and Reduced with Right, Left or Confirm, turns
    /// `Ctx::animate` on or off, and is saved.
    #[test]
    fn the_motion_row_turns_animation_on_and_off_and_is_saved() {
        let mut nav = nav();
        let ctx = &mut Ctx::empty();
        ctx.animate = true;
        assert_eq!(Display.rows(ctx)[1].value.as_deref(), Some("Full"));
        act(&mut nav, ctx, &[Action::Down, Action::Right]);
        assert!(!ctx.animate);
        assert!(ctx.library.reduced_motion().unwrap());
        assert_eq!(Display.rows(ctx)[1].value.as_deref(), Some("Reduced"));
        act(&mut nav, ctx, &[Action::Left]);
        assert!(ctx.animate);
        assert!(!ctx.library.reduced_motion().unwrap());
        act(&mut nav, ctx, &[Action::Confirm]);
        assert!(!ctx.animate, "Confirm flips it too");
        assert_eq!(ctx.window_mode, WindowMode::RememberLast, "untouched");
    }
}
