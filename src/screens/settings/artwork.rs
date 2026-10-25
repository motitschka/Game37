//! Settings → Artwork, after `screen_settings_api_key` in `design/generate_mockups.py`: the
//! SteamGridDB API key (US-20), set, changed or removed without a restart. The key itself is only
//! ever in the entry's field and in [`ApiKey`]: a row shows at most its last four characters.

use super::{Row, Section};
use crate::artwork::Artwork as Service;
use crate::config::{self, ApiKey};
use crate::ctx::Ctx;
use crate::screens::confirm::Confirm;
use crate::ui::nav::Transition;

const REMOVE: usize = 1;

pub struct Artwork;

impl Section for Artwork {
    fn name(&self) -> &'static str {
        "Artwork"
    }

    fn rows(&mut self, ctx: &Ctx) -> Vec<Row> {
        let key =
            Row::new("SteamGridDB API key").sub("Get a key at steamgriddb.com → Profile → Preferences → API");
        let Some(set) = ctx.artwork.key() else {
            return vec![key.value("Not set").verb("Set")];
        };
        let key = if ctx.artwork.key_rejected() {
            key.value("Rejected by SteamGridDB").warn()
        } else {
            key.value(ends_in(set))
        };
        vec![key.verb("Change"), Row::new("Remove key").danger().verb("Remove")]
    }

    fn confirm(&mut self, i: usize, ctx: &mut Ctx) -> Transition {
        match i {
            REMOVE if ctx.artwork.key().is_some() => Transition::Push(Box::new(remove())),
            _ => Transition::None,
        }
    }
}

/// "Set · ends in 3f9a": the key's last four characters, or none of it if it is no longer than
/// that.
fn ends_in(key: &ApiKey) -> String {
    let key = key.expose();
    match key.char_indices().rev().nth(4) {
        Some((i, ch)) => format!("Set · ends in {}", &key[i + ch.len_utf8()..]),
        None => "Set".to_owned(),
    }
}

/// Restarts Artwork with `key`, or with none (the cache only), so the change shows without a
/// restart.
fn restart(ctx: &mut Ctx, key: Option<ApiKey>) {
    ctx.artwork = Service::start(ctx.data_dir.join("artwork"), key);
}

/// The confirmation before Remove key deletes the key from `secrets.toml` and stops fetching.
fn remove() -> Confirm {
    Confirm::new(
        "Remove the SteamGridDB API key?",
        "Covers already fetched stay.",
        "Remove",
        |ctx| match config::save_api_key(&ctx.config_path, None) {
            Ok(()) => restart(ctx, None),
            Err(e) => eprintln!("game37: {e}"),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::settings::Settings;
    use crate::ui::input::{Action, Input};
    use crate::ui::nav::{Hits, Nav};

    const KEY: usize = 0;
    const SECRET: &str = "0123456789abcdef0123456789ab3f9a";

    /// A Ctx whose Config and data folder are in a temporary directory, with `key` saved and
    /// Artwork started with it.
    fn world(key: Option<&str>) -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = Ctx::empty();
        ctx.config_path = dir.path().join("config.toml");
        ctx.data_dir = dir.path().join("data");
        let key = key.map(ApiKey::new);
        config::save_api_key(&ctx.config_path, key.as_ref()).unwrap();
        restart(&mut ctx, key);
        (dir, ctx)
    }

    /// Settings in a Nav over a root screen, with the focus on the Artwork section.
    fn nav(ctx: &mut Ctx) -> Nav {
        let mut nav = Nav::single(Box::new(crate::screens::Stub::new("Home")));
        nav.push(Box::new(Settings::new()));
        act(&mut nav, ctx, &[Action::Down]);
        nav
    }

    fn act(nav: &mut Nav, ctx: &mut Ctx, actions: &[Action]) {
        for &action in actions {
            nav.handle(Input::Action(action), &Hits::default(), ctx);
        }
    }

    fn labels(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|r| r.label).collect()
    }

    #[test]
    fn with_no_key_there_is_only_the_key_row_saying_not_set() {
        let (_dir, ctx) = world(None);
        let rows = Artwork.rows(&ctx);
        assert_eq!(labels(&rows), ["SteamGridDB API key"]);
        assert_eq!(rows[KEY].value.as_deref(), Some("Not set"));
        assert_eq!(
            rows[KEY].sub,
            Some("Get a key at steamgriddb.com → Profile → Preferences → API")
        );
        assert!(!rows[KEY].warn);
    }

    #[test]
    fn with_a_key_the_row_shows_only_its_last_four_characters_and_remove_key_shows() {
        let (_dir, ctx) = world(Some(SECRET));
        let rows = Artwork.rows(&ctx);
        assert_eq!(labels(&rows), ["SteamGridDB API key", "Remove key"]);
        assert_eq!(rows[KEY].value.as_deref(), Some("Set · ends in 3f9a"));
        assert!(rows[REMOVE].danger && !rows[KEY].danger);
        let shown = format!("{rows:?}");
        assert!(!shown.contains(&SECRET[..28]), "{shown}");
        assert_eq!(
            ends_in(&ApiKey::new("abcd")),
            "Set",
            "a key that short shows none of it"
        );
        assert_eq!(ends_in(&ApiKey::new("abcde")), "Set · ends in bcde");
    }

    #[test]
    fn a_key_steamgriddb_rejected_says_so_in_the_warning_colour() {
        use crate::artwork::tests::{Fake, settle, unpaced};
        use crate::artwork::{Art, FetchError};
        use crate::library::FoundGame;
        use crate::library::db::SortOrder;
        let (dir, mut ctx) = world(Some(SECRET));
        let found = FoundGame {
            system: "snes",
            rel_path: "snes/Alpha.sfc".into(),
            title: "Alpha".into(),
        };
        ctx.library.sync_system("snes", &[found], 0).unwrap();
        let game = ctx.library.system_games("snes", SortOrder::AZ).unwrap().remove(0);
        let fake = Fake::new();
        fake.route("https://", Err(FetchError::Status(401)));
        let key = Some(ApiKey::new(SECRET));
        ctx.artwork = unpaced(Service::new(dir.path().join("artwork"), key, fake.fetch()));
        let ask = || ctx.artwork.cover(&ctx.library, &game);
        settle(&ctx.artwork, &ctx.library, ask, |a| matches!(a, Art::Loading));
        let rows = Artwork.rows(&ctx);
        assert_eq!(rows[KEY].value.as_deref(), Some("Rejected by SteamGridDB"));
        assert!(rows[KEY].warn);
        assert_eq!(
            labels(&rows),
            ["SteamGridDB API key", "Remove key"],
            "it can still be removed"
        );
    }

    #[test]
    fn remove_key_asks_first_with_cancel_focused() {
        let (_dir, mut ctx) = world(Some(SECRET));
        let ctx = &mut ctx;
        let mut nav = nav(ctx);
        act(&mut nav, ctx, &[Action::Right, Action::Down, Action::Confirm]);
        assert_eq!(nav.top().title(), "Remove the SteamGridDB API key?");
        act(&mut nav, ctx, &[Action::Confirm]);
        assert_eq!(nav.top().title(), "Settings", "Cancel closes it");
        let kept = config::load_api_key(&ctx.config_path).unwrap();
        assert_eq!(kept.as_ref().map(ApiKey::expose), Some(SECRET));
        assert_eq!(ctx.artwork.key().map(ApiKey::expose), Some(SECRET));
    }

    #[test]
    fn remove_key_deletes_the_key_and_turns_fetching_off() {
        let (_dir, mut ctx) = world(Some(SECRET));
        let ctx = &mut ctx;
        let mut nav = nav(ctx);
        act(&mut nav, ctx, &[Action::Right, Action::Down, Action::Confirm]);
        act(&mut nav, ctx, &[Action::Right, Action::Confirm]);
        assert_eq!(nav.top().title(), "Settings");
        assert_eq!(config::load_api_key(&ctx.config_path).unwrap(), None);
        assert!(ctx.artwork.key().is_none(), "Artwork runs on the cache only");
        assert_eq!(labels(&Artwork.rows(ctx)), ["SteamGridDB API key"]);
    }
}
