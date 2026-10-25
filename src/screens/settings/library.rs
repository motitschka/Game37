//! Settings → Library, after `screen_settings_library` in `design/generate_mockups.py`: the Games
//! Root, Rescan library, Hidden games and Reset Playtime and stats.

use super::{Row, Section};
use crate::ctx::Ctx;
use crate::screens::home::tilde;

#[derive(Default)]
pub struct Library {
    /// Visible and Hidden Games, as of `ctx.revision` `.0`.
    counts: Option<(u64, usize, usize)>,
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
        vec![
            Row::new("Games Root location").value(root),
            Row::new("Rescan library").value(games(visible)).verb("Rescan"),
            Row::new("Hidden games").value(format!("{hidden} hidden")).opens(),
            Row::new("Reset Playtime and stats")
                .sub("Clears Playtime and last played for every Game. Favorites and saves stay.")
                .danger()
                .verb("Reset"),
        ]
    }
}

/// "1 Game", "23 Games".
fn games(n: usize) -> String {
    match n {
        1 => "1 Game".to_owned(),
        n => format!("{n} Games"),
    }
}
