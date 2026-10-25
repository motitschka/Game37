//! Settings → Artwork, after `screen_settings_api_key` in `design/generate_mockups.py`: the
//! SteamGridDB API key (US-20), set, changed or removed without a restart. The key itself is only
//! ever in the entry's field and in [`ApiKey`]: a row shows at most its last four characters.

use super::{Row, Section};
use crate::artwork::Artwork as Service;
use crate::config::{self, ApiKey};
use crate::ctx::Ctx;
use crate::screens::confirm::Confirm;
use crate::screens::home::tilde;
use crate::screens::message::dim;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Input, InputKind};
use crate::ui::nav::{Hint, Screen, Transition};
use crate::ui::text::Weight;
use crate::ui::text_entry::{KEYBOARD_H, KEYBOARD_W, Outcome, TextEntry};
use crate::ui::theme::{DANGER, LINE, MUTED, SURFACE, TEXT};
use crate::ui::{Rect, W};

const KEY: usize = 0;
const REMOVE: usize = 1;

/// The key entry card, placed like the Rename card in `options.rs`.
const CARD_W: f32 = 1240.0;
const CARD_H: f32 = 768.0;
const CARD_X: f32 = (W - CARD_W) / 2.0;
const CARD_Y: f32 = 120.0;
/// SteamGridDB's keys are 32 characters; room to spare, so a long paste isn't cut short.
const MAX_KEY_CHARS: usize = 128;
/// What a key with characters no key has gets, without echoing what was typed.
const BAD_KEY: &str = "That isn't a SteamGridDB API key: it has characters keys don't have.";

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
            KEY => Transition::Push(Box::new(KeyEntry::new(ctx))),
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

/// `text` as the key field shows it: every character but the last four as a dot. Four or fewer
/// show as typed, so the last four typed are always visible.
fn mask(text: &str) -> String {
    let hidden = text.chars().count().saturating_sub(4);
    text.chars()
        .enumerate()
        .map(|(i, ch)| if i < hidden { '•' } else { ch })
        .collect()
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

/// Typing a new key, over Settings: the field starts empty, shows the key masked, and saving
/// checks it, writes it to `secrets.toml` and restarts Artwork with it.
struct KeyEntry {
    entry: TextEntry,
    /// Why the last save didn't happen. Never holds any of the key.
    error: Option<String>,
    /// `secrets.toml`'s path, with `~` for home.
    secrets: String,
    /// What the player last used, as of the last frame, for the hints.
    kind: InputKind,
}

impl KeyEntry {
    fn new(ctx: &Ctx) -> KeyEntry {
        let secrets = config::secrets_path(&ctx.config_path);
        KeyEntry {
            entry: TextEntry::new("", MAX_KEY_CHARS),
            error: None,
            secrets: tilde(&secrets, dirs::home_dir().as_deref()),
            kind: ctx.input_kind,
        }
    }

    /// Saves the typed key. An empty field changes nothing; a bad key stays open with the reason.
    fn save(&mut self, ctx: &mut Ctx) -> Transition {
        let key = match config::parse_api_key(self.entry.text()) {
            Ok(Some(key)) => key,
            Ok(None) => return self.close(),
            Err(_) => {
                self.error = Some(BAD_KEY.to_owned());
                return Transition::None;
            }
        };
        if let Err(e) = config::save_api_key(&ctx.config_path, Some(&key)) {
            self.error = Some(format!("Can't save the key: {e}"));
            return Transition::None;
        }
        restart(ctx, Some(key));
        self.close()
    }

    /// Drops what was typed and closes.
    fn close(&mut self) -> Transition {
        self.entry = TextEntry::new("", MAX_KEY_CHARS);
        Transition::Pop
    }
}

impl Screen for KeyEntry {
    fn title(&self) -> &str {
        "SteamGridDB API key"
    }

    fn is_modal(&self) -> bool {
        true
    }

    fn text_input(&self) -> bool {
        true
    }

    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Transition {
        match self.entry.handle(input) {
            Outcome::Submit => self.save(ctx),
            Outcome::Cancel => self.close(),
            Outcome::Changed => {
                self.error = None;
                Transition::None
            }
            Outcome::None => Transition::None,
        }
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, ctx: &Ctx) {
        self.kind = ctx.input_kind;
        dim(gfx);
        let card = Rect::new(CARD_X, CARD_Y, CARD_W, CARD_H);
        gfx.fill_round_rect(card, 20.0, SURFACE);
        gfx.stroke_round_rect(card, 20.0, 1.0, LINE);
        let x = CARD_X + (CARD_W - KEYBOARD_W) / 2.0;
        let heading = TextStyle::new(28, Weight::ExtraBold, TEXT);
        gfx.text(x, CARD_Y + 72.0, self.title(), heading, Align::Left);
        let field = Rect::new(x, CARD_Y + 136.0, KEYBOARD_W, 88.0);
        self.entry
            .draw_field_showing(gfx, field, "", &mask(self.entry.text()));
        let (line, ink) = match &self.error {
            Some(error) => (error.clone(), DANGER),
            None => (format!("Saved to {} on this device only", self.secrets), MUTED),
        };
        gfx.text(
            x,
            CARD_Y + 256.0,
            &line,
            TextStyle::new(16, Weight::Medium, ink),
            Align::Left,
        );
        let top = CARD_Y + CARD_H - 48.0 - KEYBOARD_H;
        self.entry.draw_keyboard(gfx, x, top);
    }

    fn hints(&self) -> Vec<Hint> {
        TextEntry::hints(self.kind, "Save")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::settings::Settings;
    use crate::ui::input::{Action, Input, TextInput};
    use crate::ui::nav::{Hits, Nav};

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

    fn typed(entry: &mut KeyEntry, ctx: &mut Ctx, text: &str) {
        for ch in text.chars() {
            entry.handle(Input::Text(TextInput::Char(ch)), ctx);
        }
    }

    fn secrets(ctx: &Ctx) -> String {
        std::fs::read_to_string(config::secrets_path(&ctx.config_path)).unwrap_or_default()
    }

    #[test]
    fn the_field_shows_all_but_the_last_four_characters_as_dots() {
        assert_eq!(mask("abcdefgh"), "••••efgh");
        assert_eq!(mask("abcde"), "•bcde");
        assert_eq!(mask("abcd"), "abcd", "four or fewer show as typed");
        assert_eq!(mask(""), "");
    }

    #[test]
    fn confirm_on_the_key_row_opens_an_empty_entry() {
        let (_dir, mut ctx) = world(Some(SECRET));
        let ctx = &mut ctx;
        let mut nav = nav(ctx);
        act(&mut nav, ctx, &[Action::Right, Action::Confirm]);
        assert_eq!(nav.top().title(), "SteamGridDB API key");
        assert!(nav.top().is_modal() && nav.top().text_input());
        let entry = KeyEntry::new(ctx);
        assert_eq!(entry.entry.text(), "", "not prefilled");
    }

    #[test]
    fn saving_writes_secrets_and_restarts_artwork_with_the_key() {
        let (_dir, mut ctx) = world(None);
        let ctx = &mut ctx;
        assert!(ctx.artwork.key().is_none());
        let mut entry = KeyEntry::new(ctx);
        typed(&mut entry, ctx, &format!(" {SECRET} "));
        assert!(matches!(
            entry.handle(Input::Action(Action::Start), ctx),
            Transition::Pop
        ));
        let saved = config::load_api_key(&ctx.config_path).unwrap();
        assert_eq!(saved.as_ref().map(ApiKey::expose), Some(SECRET));
        assert_eq!(
            ctx.artwork.key().map(ApiKey::expose),
            Some(SECRET),
            "Artwork was replaced"
        );
        assert_eq!(entry.entry.text(), "", "the field is cleared");
        assert_eq!(
            Artwork.rows(ctx)[KEY].value.as_deref(),
            Some("Set · ends in 3f9a")
        );
    }

    #[test]
    fn a_bad_key_is_not_saved_and_its_error_does_not_echo_it() {
        let (_dir, mut ctx) = world(Some(SECRET));
        let ctx = &mut ctx;
        let before = secrets(ctx);
        let mut entry = KeyEntry::new(ctx);
        typed(&mut entry, ctx, "wrongkey-123");
        assert!(matches!(
            entry.handle(Input::Action(Action::Start), ctx),
            Transition::None
        ));
        let error = entry.error.clone().unwrap();
        assert_eq!(error, BAD_KEY);
        assert!(!error.contains("wrongkey") && !error.contains("-123"), "{error}");
        assert_eq!(secrets(ctx), before);
        assert_eq!(ctx.artwork.key().map(ApiKey::expose), Some(SECRET));
        entry.handle(Input::Text(TextInput::Backspace), ctx);
        assert_eq!(entry.error, None, "typing clears the error");
    }

    #[test]
    fn an_empty_field_or_cancel_changes_nothing() {
        let (_dir, mut ctx) = world(Some(SECRET));
        let ctx = &mut ctx;
        // Off, so a restart with any key would show.
        ctx.artwork = Service::off();
        let before = secrets(ctx);
        let mut entry = KeyEntry::new(ctx);
        typed(&mut entry, ctx, "   ");
        assert!(matches!(
            entry.handle(Input::Action(Action::Start), ctx),
            Transition::Pop
        ));
        let mut entry = KeyEntry::new(ctx);
        typed(&mut entry, ctx, "abcdef0123456789");
        assert!(matches!(
            entry.handle(Input::Action(Action::Back), ctx),
            Transition::Pop
        ));
        assert_eq!(entry.entry.text(), "", "the field is cleared");
        assert_eq!(secrets(ctx), before);
        assert!(ctx.artwork.key().is_none());
    }
}
