//! Settings → Library, after `screen_settings_library` in `design/generate_mockups.py`: the Games
//! Root, Rescan library, Hidden games and Reset Playtime and stats.

use std::collections::HashSet;

use super::hidden::HiddenGames;
use super::{Row, Section};
use crate::ctx::{self, Ctx};
use crate::library::db::SortOrder;
use crate::screens::confirm::Confirm;
use crate::screens::home::tilde;
use crate::systems::SYSTEMS;
use crate::ui::nav::Transition;

const RESCAN: usize = 1;
const HIDDEN: usize = 2;
const RESET: usize = 3;

#[derive(Default)]
pub struct Library {
    /// Visible and Hidden Games, as of `ctx.revision` `.0`.
    counts: Option<(u64, usize, usize)>,
    /// What the last rescan found, shown in its row in place of the Game count.
    rescanned: Option<String>,
}

impl Library {
    /// How many Games are visible and how many hidden, re-read only when the library changed.
    fn counts(&mut self, ctx: &Ctx) -> (usize, usize) {
        match self.counts {
            Some((revision, visible, hidden)) if revision == ctx.revision => (visible, hidden),
            _ => {
                let visible = ctx.library.visible_count().unwrap_or(0);
                let hidden = ctx.library.hidden_games().map_or(0, |g| g.len());
                self.counts = Some((ctx.revision, visible, hidden));
                (visible, hidden)
            }
        }
    }
}

impl Section for Library {
    fn name(&self) -> &'static str {
        "Library"
    }

    fn rows(&mut self, ctx: &Ctx) -> Vec<Row> {
        let (visible, hidden) = self.counts(ctx);
        let root = tilde(&ctx.games_root, dirs::home_dir().as_deref());
        let count = self.rescanned.clone().unwrap_or_else(|| games(visible));
        vec![
            Row::new("Games Root location").value(root),
            Row::new("Rescan library").value(count).verb("Rescan"),
            Row::new("Hidden games").value(format!("{hidden} hidden")).opens(),
            Row::new("Reset Playtime and stats")
                .sub("Clears Playtime and last played for every Game. Favorites and saves stay.")
                .danger()
                .verb("Reset"),
        ]
    }

    /// Confirm on Games Root location does nothing yet: changing it comes in a later job.
    fn confirm(&mut self, i: usize, ctx: &mut Ctx) -> Transition {
        match i {
            RESCAN => self.rescanned = Some(rescan(ctx)),
            HIDDEN => return Transition::Push(Box::new(HiddenGames::new(ctx))),
            RESET => return Transition::Push(Box::new(reset())),
            _ => {}
        }
        Transition::None
    }
}

/// "1 Game", "23 Games".
fn games(n: usize) -> String {
    match n {
        1 => "1 Game".to_owned(),
        n => format!("{n} Games"),
    }
}

/// Every visible Game's id.
fn visible_ids(ctx: &Ctx) -> HashSet<i64> {
    SYSTEMS
        .iter()
        .flat_map(|s| ctx.library.system_games(s.id, SortOrder::AZ).unwrap_or_default())
        .map(|g| g.id)
        .collect()
}

/// Scans every System folder now (FR-LIB-3) and says what changed, "2 new Games, 1 gone", or "No
/// changes", from the visible Games before and after.
fn rescan(ctx: &mut Ctx) -> String {
    let before = visible_ids(ctx);
    if let Err(e) = ctx.scan_all(ctx::now()) {
        eprintln!("game37: {e}");
        return "Rescan failed".to_owned();
    }
    let after = visible_ids(ctx);
    let added = after.difference(&before).count();
    let gone = before.difference(&after).count();
    let new = match added {
        1 => "1 new Game".to_owned(),
        n => format!("{n} new Games"),
    };
    match (added, gone) {
        (0, 0) => "No changes".to_owned(),
        (_, 0) => new,
        (0, gone) => format!("{gone} gone"),
        (_, gone) => format!("{new}, {gone} gone"),
    }
}

/// The confirmation before Reset Playtime and stats clears them for every Game.
fn reset() -> Confirm {
    Confirm::new(
        "Reset Playtime and stats for every Game?",
        "This can't be undone.",
        "Reset",
        |ctx| {
            if let Err(e) = ctx.library.reset_stats() {
                eprintln!("game37: {e}");
            }
            ctx.revision += 1;
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::db::Shelf;
    use crate::screens::settings::Settings;
    use crate::ui::input::{Action, Input};
    use crate::ui::nav::{Hint, Hits, Nav, Screen};
    use std::fs;
    use std::path::Path;

    /// A Ctx on a temporary Games Root holding the SNES Games `titles`, synced.
    fn world(titles: &[&str]) -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = dir.path().join("Games");
        fs::create_dir_all(ctx.games_root.join("snes")).unwrap();
        for title in titles {
            add(&ctx.games_root, title);
        }
        ctx.scan_all(0).unwrap();
        (dir, ctx)
    }

    fn add(root: &Path, title: &str) {
        fs::write(root.join("snes").join(format!("{title}.sfc")), b"").unwrap();
    }

    fn id(ctx: &Ctx, title: &str) -> i64 {
        let games = ctx.library.system_games("snes", SortOrder::AZ).unwrap();
        games.iter().find(|g| g.title == title).unwrap().id
    }

    fn rescan_value(lib: &mut Library, ctx: &mut Ctx) -> Option<String> {
        lib.confirm(RESCAN, ctx);
        lib.rows(ctx)[RESCAN].value.clone()
    }

    #[test]
    fn the_rows_show_the_games_root_and_the_counts() {
        let (_dir, ctx) = world(&["A", "B", "C"]);
        ctx.library.set_hidden(id(&ctx, "C"), true).unwrap();
        let mut lib = Library::default();
        let rows = lib.rows(&ctx);
        let labels: Vec<_> = rows.iter().map(|r| r.label).collect();
        assert_eq!(
            labels,
            [
                "Games Root location",
                "Rescan library",
                "Hidden games",
                "Reset Playtime and stats"
            ]
        );
        assert_eq!(rows[RESCAN].value.as_deref(), Some("2 Games"));
        assert_eq!(rows[HIDDEN].value.as_deref(), Some("1 hidden"));
        assert!(rows[HIDDEN].opens && rows[RESET].danger);
        let verbs: Vec<_> = rows.iter().map(|r| r.verb).collect();
        assert_eq!(verbs, ["Open", "Rescan", "Open", "Reset"]);
    }

    #[test]
    fn rescan_says_how_many_games_are_new_and_gone() {
        let (_dir, mut ctx) = world(&["A", "B", "C"]);
        let mut lib = Library::default();
        assert_eq!(rescan_value(&mut lib, &mut ctx).as_deref(), Some("No changes"));
        add(&ctx.games_root, "D");
        add(&ctx.games_root, "E");
        fs::remove_file(ctx.games_root.join("snes/A.sfc")).unwrap();
        let revision = ctx.revision;
        assert_eq!(
            rescan_value(&mut lib, &mut ctx).as_deref(),
            Some("2 new Games, 1 gone")
        );
        assert!(ctx.revision > revision, "screens reload");
        add(&ctx.games_root, "F");
        assert_eq!(rescan_value(&mut lib, &mut ctx).as_deref(), Some("1 new Game"));
        fs::remove_file(ctx.games_root.join("snes/B.sfc")).unwrap();
        fs::remove_file(ctx.games_root.join("snes/C.sfc")).unwrap();
        assert_eq!(rescan_value(&mut lib, &mut ctx).as_deref(), Some("2 gone"));
    }

    /// Settings in a Nav over a root screen, so pushes and pops show.
    fn nav() -> Nav {
        let mut nav = Nav::single(Box::new(crate::screens::Stub::new("Home")));
        nav.push(Box::new(Settings::new()));
        nav
    }

    fn act(nav: &mut Nav, ctx: &mut Ctx, actions: &[Action]) {
        for &action in actions {
            nav.handle(Input::Action(action), &Hits::default(), ctx);
        }
    }

    const TO_HIDDEN: &[Action] = &[Action::Right, Action::Down, Action::Down, Action::Confirm];
    const TO_RESET: &[Action] = &[
        Action::Right,
        Action::Down,
        Action::Down,
        Action::Down,
        Action::Confirm,
    ];

    #[test]
    fn unhiding_a_game_takes_it_off_the_list_and_back_to_its_views() {
        let (_dir, mut ctx) = world(&["A", "B", "C"]);
        let (a, b) = (id(&ctx, "A"), id(&ctx, "B"));
        ctx.library.set_hidden(a, true).unwrap();
        ctx.library.set_hidden(b, true).unwrap();
        let ctx = &mut ctx;
        let mut nav = nav();
        act(&mut nav, ctx, TO_HIDDEN);
        assert_eq!(nav.top().title(), "Hidden games");
        act(&mut nav, ctx, &[Action::Down]);
        let revision = ctx.revision;
        act(&mut nav, ctx, &[Action::Confirm]);
        assert!(ctx.revision > revision, "screens reload");
        let hidden: Vec<_> = ctx.library.hidden_games().unwrap().iter().map(|g| g.id).collect();
        assert_eq!(hidden, [a], "B is unhidden");
        let shown = |ctx: &Ctx, id| {
            let recent = ctx.library.shelf(Shelf::RecentlyAdded, None).unwrap();
            let page = ctx.library.system_games("snes", SortOrder::AZ).unwrap();
            (recent.iter().any(|g| g.id == id), page.iter().any(|g| g.id == id))
        };
        assert_eq!(shown(ctx, b), (true, true));
        assert_eq!(shown(ctx, a), (false, false));
        act(&mut nav, ctx, &[Action::Confirm]);
        assert!(
            ctx.library.hidden_games().unwrap().is_empty(),
            "the focus moved up to A"
        );
        assert_eq!(shown(ctx, a), (true, true));
        act(&mut nav, ctx, &[Action::Confirm]);
        act(&mut nav, ctx, &[Action::Back]);
        assert_eq!(nav.top().title(), "Settings");
        act(&mut nav, ctx, &[Action::Back, Action::Back]);
        assert_eq!((nav.depth(), nav.top().title()), (1, "Home"));
    }

    fn playtimes(ctx: &Ctx) -> Vec<(u64, Option<i64>)> {
        let games = ctx.library.system_games("snes", SortOrder::AZ).unwrap();
        games.iter().map(|g| (g.playtime_secs, g.last_played)).collect()
    }

    #[test]
    fn the_reset_confirmation_defaults_to_cancel_and_reset_clears_playtime() {
        let (_dir, mut ctx) = world(&["A", "B"]);
        ctx.library.record_play(id(&ctx, "A"), 100, 60).unwrap();
        ctx.library.set_favorite(id(&ctx, "B"), true, 5).unwrap();
        let ctx = &mut ctx;
        let played = playtimes(ctx);
        assert_eq!(played[0], (60, Some(100)));
        let mut nav = nav();
        act(&mut nav, ctx, TO_RESET);
        assert_eq!(nav.top().title(), "Reset Playtime and stats for every Game?");
        act(&mut nav, ctx, &[Action::Confirm]);
        assert_eq!(nav.top().title(), "Settings", "Cancel closes it");
        assert_eq!(playtimes(ctx), played, "and changes nothing");
        act(&mut nav, ctx, &[Action::Confirm]);
        act(&mut nav, ctx, &[Action::Right, Action::Back]);
        assert_eq!(nav.top().title(), "Settings", "Back cancels too");
        assert_eq!(playtimes(ctx), played);
        let revision = ctx.revision;
        act(&mut nav, ctx, &[Action::Confirm, Action::Right, Action::Confirm]);
        assert_eq!(nav.top().title(), "Settings");
        assert_eq!(playtimes(ctx), [(0, None), (0, None)]);
        assert!(ctx.revision > revision, "screens reload");
        let favorites = ctx.library.shelf(Shelf::Favorites, None).unwrap();
        assert_eq!(favorites.len(), 1, "Favorites stay");
    }

    #[test]
    fn an_empty_hidden_games_list_says_how_to_hide_one_and_back_returns_to_settings() {
        let (_dir, mut ctx) = world(&["A"]);
        let ctx = &mut ctx;
        let mut hidden = HiddenGames::new(ctx);

        assert_eq!(hidden.hints(), [Hint::new(&[Action::Back], "Back")]);
        assert!(matches!(
            hidden.handle(Input::Action(Action::Confirm), ctx),
            Transition::None
        ));
        let mut nav = nav();
        act(&mut nav, ctx, TO_HIDDEN);
        act(&mut nav, ctx, &[Action::Back]);
        assert_eq!(nav.top().title(), "Settings");
    }
}
