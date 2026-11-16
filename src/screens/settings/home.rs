//! Settings → Home (`screen_settings_home` in the mockups): which Shelf each of Home's four slots
//! holds, or Off (FR-HOME-2), each a cycled choice like Display's.

use super::{Row, Section};
use crate::ctx::Ctx;
use crate::library::db::{HomeShelves, Shelf};
use crate::screens::home::name;
use crate::ui::input::{Action, Input};
use crate::ui::nav::Transition;

const SLOTS: [&str; 4] = ["Shelf 1", "Shelf 2", "Shelf 3", "Shelf 4"];

pub struct Home;

impl Section for Home {
    fn name(&self) -> &'static str {
        "Home"
    }

    fn rows(&mut self, ctx: &Ctx) -> Vec<Row> {
        SLOTS
            .into_iter()
            .zip(ctx.home_shelves.slots())
            .map(|(label, shelf)| Row::new(label).choice(shelf.map_or("Off", name)))
            .collect()
    }

    /// Confirm on a row steps its choice, as Right does in [`Home::handle`].
    fn confirm(&mut self, i: usize, ctx: &mut Ctx) -> Transition {
        cycle(i, true, ctx);
        Transition::None
    }

    /// Left, Right and Confirm step the focused slot at once. Confirm is handled here so Settings'
    /// own Confirm dispatch, which would otherwise also call [`Home::confirm`], never runs for it.
    fn handle(&mut self, i: usize, input: Input, ctx: &mut Ctx) -> Option<Transition> {
        let forward = match input {
            Input::Action(Action::Right | Action::Confirm) => true,
            Input::Action(Action::Left) => false,
            _ => return None,
        };
        cycle(i, forward, ctx).then_some(Transition::None)
    }
}

/// Steps slot `i` and saves the result. False if there is no slot `i`.
fn cycle(i: usize, forward: bool, ctx: &mut Ctx) -> bool {
    if i >= SLOTS.len() {
        return false;
    }
    ctx.set_home_shelves(step(ctx.home_shelves, i, forward));
    true
}

/// Slot `slot` after one step forward or back through every Shelf in [`Shelf::ALL`] order, then
/// Off, wrapping round. Stepping onto a Shelf another slot holds swaps the two: that slot gets
/// this one's previous choice, Off included. Off is stepped over on the only slot holding a Shelf.
fn step(shelves: HomeShelves, slot: usize, forward: bool) -> HomeShelves {
    let choices: Vec<Option<Shelf>> = Shelf::ALL.into_iter().map(Some).chain([None]).collect();
    let mut slots = shelves.slots();
    let was = slots[slot];
    let only = was.is_some() && slots.iter().flatten().count() == 1;
    let mut at = choices
        .iter()
        .position(|&c| c == was)
        .expect("every choice is listed");
    loop {
        let by = if forward { 1 } else { choices.len() - 1 };
        at = (at + by) % choices.len();
        if !(only && choices[at].is_none()) {
            break;
        }
    }
    let now = choices[at];
    if let Some(other) = slots.iter().position(|&s| now.is_some() && s == now) {
        slots[other] = was;
    }
    slots[slot] = now;
    HomeShelves::new(slots).expect("stepping keeps each Shelf in one slot and one slot on")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::settings::Settings;
    use crate::ui::nav::{Hint, Screen};
    use Shelf::{ContinuePlaying, Favorites, MostPlayed, RecentlyAdded};

    fn shelves(slots: [Option<Shelf>; 4]) -> HomeShelves {
        HomeShelves::new(slots).unwrap()
    }

    #[test]
    fn stepping_onto_a_shelf_another_slot_holds_swaps_the_two_slots() {
        let start = HomeShelves::DEFAULT;
        assert_eq!(
            step(start, 0, true),
            shelves([
                Some(ContinuePlaying),
                Some(Favorites),
                Some(RecentlyAdded),
                Some(MostPlayed)
            ]),
            "Shelf 2 gets Favorites"
        );
        assert_eq!(
            step(start, 3, false),
            shelves([
                Some(Favorites),
                Some(ContinuePlaying),
                Some(MostPlayed),
                Some(RecentlyAdded)
            ]),
            "back from Most played to Recently added, which Shelf 3 gives up"
        );
        let with_off = shelves([None, Some(Favorites), Some(MostPlayed), None]);
        assert_eq!(
            step(with_off, 0, true),
            shelves([Some(Favorites), None, Some(MostPlayed), None]),
            "the slot Favorites leaves gets this one's Off"
        );
        assert_eq!(
            step(start, 1, true),
            shelves([
                Some(Favorites),
                Some(RecentlyAdded),
                Some(ContinuePlaying),
                Some(MostPlayed)
            ])
        );
    }

    #[test]
    fn the_cycle_runs_through_every_shelf_then_off_and_wraps() {
        let mut s = HomeShelves::DEFAULT;
        let mut seen = Vec::new();
        for _ in 0..5 {
            s = step(s, 3, true);
            seen.push(s.slots()[3]);
        }
        assert_eq!(
            seen,
            [
                None,
                Some(Favorites),
                Some(ContinuePlaying),
                Some(RecentlyAdded),
                Some(MostPlayed)
            ]
        );
        assert_eq!(
            step(HomeShelves::DEFAULT, 0, false).slots()[0],
            None,
            "back from Favorites to Off"
        );
    }

    #[test]
    fn off_is_stepped_over_on_the_only_slot_holding_a_shelf() {
        let one = shelves([None, None, Some(MostPlayed), None]);
        assert_eq!(step(one, 2, true), shelves([None, None, Some(Favorites), None]));
        let one = shelves([None, Some(Favorites), None, None]);
        assert_eq!(step(one, 1, false), shelves([None, Some(MostPlayed), None, None]));
        assert_eq!(
            step(one, 0, false),
            shelves([Some(MostPlayed), Some(Favorites), None, None]),
            "an Off slot still steps onto a Shelf"
        );
    }

    fn act(s: &mut Settings, ctx: &mut Ctx, actions: &[Action]) {
        for &action in actions {
            s.handle(Input::Action(action), ctx);
        }
    }

    /// Down past Library, Artwork, BIOS, Cores and Controls to Home, then into Shelf 1.
    fn open(ctx: &mut Ctx) -> Settings {
        let mut s = Settings::new();
        act(&mut s, ctx, &[Action::Down; 5]);
        assert_eq!(s.section(), "Home");
        act(&mut s, ctx, &[Action::Right]);
        s
    }

    #[test]
    fn the_rows_name_each_slots_shelf_or_off() {
        let mut ctx = Ctx::empty();
        let labels = |ctx: &Ctx| -> Vec<_> {
            Home.rows(ctx)
                .into_iter()
                .map(|r| (r.label, r.value.unwrap(), r.choice))
                .collect()
        };
        assert_eq!(
            labels(&ctx),
            [
                ("Shelf 1", "Favorites".to_owned(), true),
                ("Shelf 2", "Continue playing".to_owned(), true),
                ("Shelf 3", "Recently added".to_owned(), true),
                ("Shelf 4", "Most played".to_owned(), true),
            ]
        );
        ctx.home_shelves = shelves([None, Some(MostPlayed), None, None]);
        assert_eq!(labels(&ctx)[0].1, "Off");
        assert_eq!(labels(&ctx)[1].1, "Most played");
    }

    /// Left, Right and Confirm step the focused slot, and each change is saved: the smoke shot
    /// `d1-home-two-shelves` leaves Recently added then Most played this way.
    #[test]
    fn left_right_and_confirm_step_the_focused_slot_and_save_it() {
        let ctx = &mut Ctx::empty();
        let mut s = open(ctx);
        act(&mut s, ctx, &[Action::Left; 3]);
        assert_eq!(
            ctx.home_shelves,
            shelves([Some(RecentlyAdded), Some(ContinuePlaying), Some(MostPlayed), None])
        );
        act(
            &mut s,
            ctx,
            &[Action::Down, Action::Left, Action::Left, Action::Left],
        );
        let two = shelves([Some(RecentlyAdded), Some(MostPlayed), None, None]);
        assert_eq!(ctx.home_shelves, two);
        assert_eq!(ctx.library.home_shelves().unwrap(), two, "saved");
        act(&mut s, ctx, &[Action::Right]);
        assert_eq!(ctx.home_shelves, shelves([Some(RecentlyAdded), None, None, None]));
        act(&mut s, ctx, &[Action::Confirm]);
        assert_eq!(
            ctx.home_shelves,
            shelves([Some(RecentlyAdded), Some(Favorites), None, None])
        );
        assert_eq!(s.hints()[0], Hint::new(&[Action::Left, Action::Right], "Change"));
    }
}
