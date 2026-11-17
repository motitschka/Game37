//! Settings → Home (`screen_settings_home` in the mockups): the four Shelves in Home's order, each
//! on or off (FR-HOME-2). Confirm switches the focused Shelf on or off, though never the last one
//! on; Left and Right move it up and down the list, the focus going with it. Back returns to the
//! sections, as Left does elsewhere in Settings.

use super::{Row, Section};
use crate::ctx::Ctx;
use crate::screens::home::name;
use crate::ui::input::{Action, Input};
use crate::ui::nav::Transition;

#[derive(Default)]
pub struct Home {
    /// Where the focused Shelf went in the last move, for [`Section::moved_to`].
    moved: Option<usize>,
}

impl Section for Home {
    fn name(&self) -> &'static str {
        "Home"
    }

    fn rows(&mut self, ctx: &Ctx) -> Vec<Row> {
        ctx.home_shelves
            .shelves()
            .into_iter()
            .map(|(shelf, on)| {
                Row::new(name(shelf))
                    .value(if on { "On" } else { "Off" })
                    .verb("On/Off")
                    .moves()
            })
            .collect()
    }

    /// Confirm, or a click on the focused row, switches its Shelf on or off; on the last Shelf on,
    /// nothing changes.
    fn confirm(&mut self, i: usize, ctx: &mut Ctx) -> Transition {
        if let Some(shelves) = ctx.home_shelves.toggled(i) {
            ctx.set_home_shelves(shelves);
        }
        Transition::None
    }

    /// Left moves the focused Shelf one place up, Right one down; at the ends nothing moves.
    fn handle(&mut self, i: usize, input: Input, ctx: &mut Ctx) -> Option<Transition> {
        let by = match input {
            Input::Action(Action::Left) => -1,
            Input::Action(Action::Right) => 1,
            _ => return None,
        };
        if let Some(shelves) = ctx.home_shelves.moved(i, by) {
            ctx.set_home_shelves(shelves);
            self.moved = Some(i.saturating_add_signed(by));
        }
        Some(Transition::None)
    }

    fn moved_to(&mut self) -> Option<usize> {
        self.moved.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::db::{HomeShelves, Shelf};
    use crate::screens::settings::{Focus, Settings};
    use crate::ui::nav::{Hint, Screen};
    use Shelf::{ContinuePlaying, Favorites, MostPlayed, RecentlyAdded};

    fn act(s: &mut Settings, ctx: &mut Ctx, actions: &[Action]) {
        for &action in actions {
            s.handle(Input::Action(action), ctx);
        }
    }

    /// Down past Library, Artwork, BIOS, Cores and Controls to Home, then into its first Shelf.
    fn open(ctx: &mut Ctx) -> Settings {
        let mut s = Settings::new();
        act(&mut s, ctx, &[Action::Down; 5]);
        assert_eq!(s.section(), "Home");
        act(&mut s, ctx, &[Action::Right]);
        assert_eq!((s.focus(), s.row()), (Focus::Rows, 0));
        s
    }

    fn labels(ctx: &Ctx) -> Vec<(&'static str, String)> {
        Home::default()
            .rows(ctx)
            .into_iter()
            .map(|r| (r.label, r.value.unwrap()))
            .collect()
    }

    #[test]
    fn the_rows_name_each_shelf_in_homes_order_and_whether_it_is_on() {
        let mut ctx = Ctx::empty();
        assert_eq!(
            labels(&ctx),
            [
                ("Favorites", "On".to_owned()),
                ("Continue playing", "On".to_owned()),
                ("Recently added", "On".to_owned()),
                ("Most played", "On".to_owned()),
            ]
        );
        ctx.home_shelves = HomeShelves::showing(&[MostPlayed, Favorites]).unwrap();
        assert_eq!(
            labels(&ctx),
            [
                ("Most played", "On".to_owned()),
                ("Favorites", "On".to_owned()),
                ("Continue playing", "Off".to_owned()),
                ("Recently added", "Off".to_owned()),
            ]
        );
    }

    /// Confirm switches the focused Shelf off and on again, and each change is saved: the smoke shot
    /// `d1-home-two-shelves` switches Continue playing and Most played off this way.
    #[test]
    fn confirm_switches_the_focused_shelf_on_or_off_and_saves_it() {
        let ctx = &mut Ctx::empty();
        let mut s = open(ctx);
        act(&mut s, ctx, &[Action::Down, Action::Confirm]);
        let off = HomeShelves::new([
            (Favorites, true),
            (ContinuePlaying, false),
            (RecentlyAdded, true),
            (MostPlayed, true),
        ])
        .unwrap();
        assert_eq!(ctx.home_shelves, off, "Continue playing stays in its place, off");
        assert_eq!(ctx.library.home_shelves().unwrap(), off, "saved");
        assert_eq!(labels(ctx)[1], ("Continue playing", "Off".to_owned()));
        act(&mut s, ctx, &[Action::Confirm]);
        assert_eq!(ctx.home_shelves, HomeShelves::DEFAULT, "on again");
        assert_eq!(
            s.hints(),
            [
                Hint::new(&[Action::Confirm], "On/Off"),
                Hint::new(&[Action::Left, Action::Right], "Move"),
                Hint::new(&[Action::Back], "Back"),
            ]
        );
    }

    #[test]
    fn confirm_on_the_last_shelf_on_changes_nothing() {
        let ctx = &mut Ctx::empty();
        let only = HomeShelves::showing(&[RecentlyAdded]).unwrap();
        ctx.set_home_shelves(only);
        let mut s = open(ctx);
        act(&mut s, ctx, &[Action::Confirm]);
        assert_eq!(ctx.home_shelves, only);
        assert_eq!(ctx.library.home_shelves().unwrap(), only);
        act(&mut s, ctx, &[Action::Down, Action::Confirm]);
        assert_eq!(
            ctx.home_shelves.shown().collect::<Vec<_>>(),
            [RecentlyAdded, Favorites],
            "an off Shelf still switches on"
        );
    }

    #[test]
    fn left_and_right_move_the_focused_shelf_and_the_focus_with_it() {
        let ctx = &mut Ctx::empty();
        let mut s = open(ctx);
        act(&mut s, ctx, &[Action::Left]);
        assert_eq!(
            ctx.home_shelves,
            HomeShelves::DEFAULT,
            "the first Shelf goes no higher"
        );
        assert_eq!(
            (s.focus(), s.row()),
            (Focus::Rows, 0),
            "and Left stays in the rows"
        );
        act(&mut s, ctx, &[Action::Right, Action::Right]);
        assert_eq!(
            ctx.home_shelves.shown().collect::<Vec<_>>(),
            [ContinuePlaying, RecentlyAdded, Favorites, MostPlayed]
        );
        assert_eq!(s.row(), 2, "the focus follows Favorites");
        assert_eq!(ctx.library.home_shelves().unwrap(), ctx.home_shelves, "saved");
        act(&mut s, ctx, &[Action::Right, Action::Right]);
        assert_eq!(
            ctx.home_shelves.shown().collect::<Vec<_>>(),
            [ContinuePlaying, RecentlyAdded, MostPlayed, Favorites]
        );
        assert_eq!(s.row(), 3, "the last Shelf goes no lower");
        act(&mut s, ctx, &[Action::Up, Action::Left]);
        assert_eq!(
            ctx.home_shelves.shown().collect::<Vec<_>>(),
            [ContinuePlaying, MostPlayed, RecentlyAdded, Favorites]
        );
        assert_eq!(s.row(), 1);
    }

    #[test]
    fn back_returns_to_the_sections() {
        let ctx = &mut Ctx::empty();
        let mut s = open(ctx);
        act(&mut s, ctx, &[Action::Down]);
        assert!(matches!(
            s.handle(Input::Action(Action::Back), ctx),
            Transition::None
        ));
        assert_eq!((s.focus(), s.section()), (Focus::Sections, "Home"));
        assert_eq!(ctx.home_shelves, HomeShelves::DEFAULT, "nothing changed");
    }
}
