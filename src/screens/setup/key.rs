//! Step 5, "Add a SteamGridDB key (optional)": typing a key, or Skip, after `screen_setup_key` in
//! `design/generate_mockups.py`. The key lives only in the field's buffer until Finish saves it to
//! `secrets.toml`; it never shows beyond its last four characters, and never reaches an error or
//! `Debug` output.

use super::{Button, CONTENT_Y, Go, Model, Pill, WHITE, pill, pill_width};
use crate::config;
use crate::ctx::Ctx;
use crate::screens::home::tilde;
use crate::ui::gfx::{Align, Gfx, TextStyle};
use crate::ui::input::{Action, Input, InputKind, Pointer, TextInput};
use crate::ui::nav::Hint;
use crate::ui::text::Weight;
use crate::ui::text_entry::{KEYBOARD_W, Outcome, TextEntry, mask};
use crate::ui::theme::{DANGER, LINE, MUTED, SURFACE2, TEXT};
use crate::ui::{M, Rect, W, hit};

/// SteamGridDB's keys are 32 characters; room to spare, so a long paste isn't cut short.
const MAX_KEY_CHARS: usize = 128;
/// What a key with characters no key has gets, without echoing what was typed.
const BAD_KEY: &str = "That isn't a SteamGridDB API key: it has characters keys don't have.";
const FIELD_W: f32 = 900.0;
const FIELD_H: f32 = 72.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Field,
    Skip,
    Button(Button),
}

pub struct Key {
    focus: Focus,
    /// The key being typed. Never shown beyond `mask`, and cleared when the step is left.
    entry: TextEntry,
    /// Whether the on-screen keyboard is open, for a controller's Confirm on the field.
    keyboard_open: bool,
    /// Set just before Skip returns `Go::Next`, so `before_finish` saves nothing.
    skip: bool,
    /// Why the key wasn't saved. Never holds any of the key itself.
    error: Option<String>,
    /// `secrets.toml`'s path, with `~` for home.
    secrets: String,
    /// What the player last used, as of the last frame, for the hints and for whether the
    /// keyboard types into the field in place.
    kind: InputKind,
    /// Where the field and Skip were drawn, for the mouse.
    hits: Vec<(Rect, Focus)>,
}

impl Key {
    pub fn new() -> Key {
        Key {
            focus: Focus::Field,
            entry: TextEntry::new("", MAX_KEY_CHARS),
            keyboard_open: false,
            skip: false,
            error: None,
            secrets: String::new(),
            kind: InputKind::Controller,
            hits: Vec::new(),
        }
    }

    fn move_focus(&mut self, action: Action) {
        self.focus = match (self.focus, action) {
            (Focus::Field, Action::Down) => Focus::Button(Button::Continue),
            (Focus::Button(_) | Focus::Skip, Action::Up) => Focus::Field,
            (Focus::Button(Button::Back), Action::Right) => Focus::Skip,
            (Focus::Skip, Action::Right) => Focus::Button(Button::Continue),
            (Focus::Skip, Action::Left) => Focus::Button(Button::Back),
            (Focus::Button(Button::Continue), Action::Left) => Focus::Skip,
            (focus, _) => focus,
        };
    }

    /// Confirm on the focus. On the field it opens the on-screen keyboard for a controller; the
    /// keyboard types into the field in place.
    fn confirm(&mut self) -> Go {
        match self.focus {
            Focus::Field if self.kind == InputKind::Controller => self.keyboard_open = true,
            Focus::Field => {}
            Focus::Skip => {
                self.skip = true;
                return Go::Next;
            }
            Focus::Button(Button::Back) => return Go::Back,
            Focus::Button(Button::Continue) => return Go::Next,
        }
        Go::Stay
    }

    fn draw_overlay(&mut self, gfx: &mut Gfx<'_>) {
        let label = TextStyle::new(14, Weight::Bold, MUTED);
        gfx.text(M, CONTENT_Y + 20.0, "API KEY", label, Align::Left);
        let field = Rect::new(M, CONTENT_Y + 40.0, KEYBOARD_W, 88.0);
        self.entry
            .draw_field_showing(gfx, field, "", &mask(self.entry.text()));
        self.entry.draw_keyboard(gfx, M, CONTENT_Y + 188.0);
    }

    fn draw_chips(&self, gfx: &mut Gfx<'_>) {
        let label = TextStyle::new(14, Weight::Bold, MUTED);
        gfx.text(M, CONTENT_Y + 20.0, "WHERE TO GET ONE", label, Align::Left);
        let part_style = TextStyle::new(20, Weight::SemiBold, TEXT);
        let arrow_style = TextStyle::new(22, Weight::Medium, MUTED);
        let (top, h) = (CONTENT_Y + 40.0, 48.0);
        let mut x = M;
        for (i, part) in ["steamgriddb.com", "Profile", "Preferences", "API"]
            .iter()
            .enumerate()
        {
            if i > 0 {
                gfx.text(x + 18.0, top + h / 2.0 + 7.0, "→", arrow_style, Align::Centre);
                x += 36.0;
            }
            let w = gfx.measure(part, part_style) + 40.0;
            let r = Rect::new(x, top, w, h);
            gfx.fill_round_rect(r, h / 2.0, SURFACE2);
            gfx.stroke_round_rect(r, h / 2.0, 1.0, LINE);
            gfx.text(x + w / 2.0, top + h / 2.0 + 7.0, part, part_style, Align::Centre);
            x += w;
        }
    }

    fn draw_field(&mut self, gfx: &mut Gfx<'_>) {
        let label = TextStyle::new(14, Weight::Bold, MUTED);
        gfx.text(M, CONTENT_Y + 156.0, "API KEY", label, Align::Left);
        let r = Rect::new(M, CONTENT_Y + 176.0, FIELD_W, FIELD_H);
        if self.focus == Focus::Field {
            gfx.stroke_round_rect(r.inset(-6.0), 20.0, 3.0, WHITE);
        }
        self.entry
            .draw_field_showing(gfx, r, "", &mask(self.entry.text()));
        self.hits.push((r, Focus::Field));
        let (line, ink) = match &self.error {
            Some(error) => (error.clone(), DANGER),
            None => (format!("Saved to {} on this device only", self.secrets), MUTED),
        };
        gfx.text(
            M,
            CONTENT_Y + 288.0,
            &line,
            TextStyle::new(18, Weight::Medium, ink),
            Align::Left,
        );
    }

    /// Skip, left of Back and Finish, which the frame draws at the same widths after this.
    fn draw_skip(&mut self, gfx: &mut Gfx<'_>) {
        let continue_w = pill_width(gfx, "Finish");
        let back_w = pill_width(gfx, "Back");
        let skip_w = pill_width(gfx, "Skip");
        let continue_x = W - M - continue_w;
        let back_x = continue_x - 16.0 - back_w;
        let skip_r = Rect::new(back_x - 16.0 - skip_w, super::BUTTONS_Y, skip_w, super::PILL_H);
        pill(
            gfx,
            skip_r,
            "Skip",
            Pill::Normal {
                focused: self.focus == Focus::Skip,
            },
        );
        self.hits.push((skip_r, Focus::Skip));
    }
}

impl super::Step for Key {
    fn title(&self) -> &'static str {
        "Add a SteamGridDB key (optional)"
    }

    fn explanation(&self) -> &'static str {
        "With a free SteamGridDB API key, Game37 fetches covers and banners for your Games. Without \
         one, Games show their title and System."
    }

    fn enter(&mut self, _model: &Model, ctx: &mut Ctx) {
        self.kind = ctx.input_kind;
        self.focus = Focus::Field;
        self.error = None;
        self.skip = false;
        self.keyboard_open = false;
        self.secrets = tilde(
            &config::secrets_path(&ctx.config_path),
            dirs::home_dir().as_deref(),
        );
    }

    fn tick(&mut self, _model: &Model, ctx: &mut Ctx) {
        self.kind = ctx.input_kind;
    }

    fn leave(&mut self) {
        self.entry = TextEntry::new("", MAX_KEY_CHARS);
        self.keyboard_open = false;
        self.skip = false;
        self.error = None;
    }

    fn can_continue(&self, _model: &Model) -> bool {
        true
    }

    /// Checks a field with text with `parse_api_key`; a bad key shows why and stays. An empty
    /// field, or Skip, finishes with no key: `before_finish` sees `skip` or an empty field either
    /// way, and saves nothing.
    fn before_finish(&mut self, _model: &Model, ctx: &mut Ctx) -> Result<(), String> {
        if self.skip {
            return Ok(());
        }
        match config::parse_api_key(self.entry.text()) {
            Ok(None) => Ok(()),
            Ok(Some(key)) => config::save_api_key(&ctx.config_path, Some(&key))
                .map_err(|e| format!("Can't save the key: {e}")),
            Err(_) => Err(BAD_KEY.to_owned()),
        }
    }

    fn handle(&mut self, input: Input, _model: &mut Model, _ctx: &mut Ctx) -> Go {
        if self.keyboard_open {
            match self.entry.handle(input) {
                Outcome::Submit => self.keyboard_open = false,
                Outcome::Cancel => self.keyboard_open = false,
                Outcome::Changed => self.error = None,
                Outcome::None => {}
            }
            return Go::Stay;
        }
        match input {
            Input::Text(TextInput::Char(_) | TextInput::Backspace) => {
                if self.entry.handle(input) == Outcome::Changed {
                    self.error = None;
                }
            }
            Input::Action(Action::Confirm) => return self.confirm(),
            Input::Action(action) if action.is_direction() => self.move_focus(action),
            Input::Pointer(Pointer::Move { x, y }) => {
                if let Some(focus) = hit(&self.hits, x, y) {
                    self.focus = focus;
                }
            }
            Input::Pointer(Pointer::Click { x, y }) => {
                if let Some(focus) = hit(&self.hits, x, y) {
                    self.focus = focus;
                    return self.confirm();
                }
            }
            _ => {}
        }
        Go::Stay
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, _model: &Model, ctx: &Ctx) {
        self.kind = ctx.input_kind;
        self.hits.clear();
        if self.keyboard_open {
            return self.draw_overlay(gfx);
        }
        self.draw_chips(gfx);
        self.draw_field(gfx);
        self.draw_skip(gfx);
    }

    fn hints(&self, _model: &Model) -> Vec<Hint> {
        if self.keyboard_open {
            return TextEntry::hints(self.kind, "Save");
        }
        match self.kind {
            InputKind::Keyboard if self.types_in_place() => vec![Hint::new(&[Action::Paste], "Paste")],
            InputKind::Keyboard => Vec::new(),
            InputKind::Controller => vec![Hint::new(&[Action::Confirm], "Type")],
        }
    }

    fn focused_button(&self) -> Option<Button> {
        match self.focus {
            Focus::Button(b) => Some(b),
            _ => None,
        }
    }

    fn text_input(&self) -> bool {
        self.keyboard_open
    }

    fn types_in_place(&self) -> bool {
        !self.keyboard_open && self.focus == Focus::Field && self.kind == InputKind::Keyboard
    }
}

#[cfg(test)]
mod tests {
    use super::super::Setup;
    use super::super::tests::act;
    use super::*;
    use crate::ui::controls::{ControlEvent, Controls};
    use crate::ui::nav::{Screen, Transition};
    use sdl3::keyboard::Keycode;

    const SECRET: &str = "0123456789abcdef0123456789ab3f9a";

    /// Setup on step 5 (the key step), nothing ticked but NES, so earlier steps pass quickly. The
    /// Games Root and the data dir are beside `config_path`, so they last as long as it does.
    fn on_key(config_path: &std::path::Path) -> (Setup, Ctx) {
        let dir = config_path.parent().unwrap();
        let mut ctx = Ctx::empty();
        ctx.config_path = config_path.to_path_buf();
        // Mesen is already in the data dir, so the Cores step downloads nothing.
        ctx.data_dir = dir.join("data");
        let mesen = crate::cores::installed_path(&ctx.data_dir, &crate::cores::MESEN);
        std::fs::create_dir_all(mesen.parent().unwrap()).unwrap();
        std::fs::write(mesen, b"dummy").unwrap();
        ctx.start_download = Box::new(|_, _| panic!("Mesen is already installed"));
        let mut setup = Setup::new(dir.join("Games"));
        act(&mut setup, &mut ctx, Action::Confirm); // tick NES
        act(&mut setup, &mut ctx, Action::Settings); // -> folders (emulators skipped)
        act(&mut setup, &mut ctx, Action::Confirm); // create folders
        act(&mut setup, &mut ctx, Action::Settings); // -> Cores, all installed
        act(&mut setup, &mut ctx, Action::Confirm); // the focused Continue goes on
        assert_eq!(setup.step(), 5);
        (setup, ctx)
    }

    fn typed(setup: &mut Setup, ctx: &mut Ctx, text: &str) {
        for ch in text.chars() {
            setup.handle(Input::Text(TextInput::Char(ch)), ctx);
        }
    }

    fn secrets_file(path: &std::path::Path) -> std::path::PathBuf {
        config::secrets_path(path)
    }

    #[test]
    fn the_mask_matches_settings() {
        assert_eq!(mask("abcdefgh"), "••••efgh");
        assert_eq!(mask("abcd"), "abcd");
    }

    #[test]
    fn finish_with_a_good_key_writes_secrets_then_the_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (mut setup, mut ctx) = on_key(&path);
        typed(&mut setup, &mut ctx, SECRET);
        assert!(matches!(
            act(&mut setup, &mut ctx, Action::Settings),
            Transition::None
        ));
        let saved = config::load_api_key(&path).unwrap();
        assert_eq!(saved.as_ref().map(config::ApiKey::expose), Some(SECRET));
        assert!(ctx.setup_finished.is_some());
    }

    #[test]
    fn a_bad_key_writes_neither_file_and_the_error_does_not_echo_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (mut setup, mut ctx) = on_key(&path);
        typed(&mut setup, &mut ctx, "wrongkey-123");
        assert!(matches!(
            act(&mut setup, &mut ctx, Action::Settings),
            Transition::None
        ));
        assert_eq!(setup.step(), 5, "stays");
        assert!(!secrets_file(&path).exists());
        assert!(!path.exists());
        assert_eq!(ctx.setup_finished, None);
        assert_eq!(setup.error.as_deref(), Some(BAD_KEY));
        assert!(!BAD_KEY.contains("wrongkey") && !BAD_KEY.contains("-123"));
    }

    #[test]
    fn skip_writes_the_config_and_no_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (mut setup, mut ctx) = on_key(&path);
        typed(&mut setup, &mut ctx, SECRET);
        // Move focus from the field to Skip and confirm it, ignoring the typed key.
        act(&mut setup, &mut ctx, Action::Down);
        act(&mut setup, &mut ctx, Action::Left);
        act(&mut setup, &mut ctx, Action::Confirm);
        assert!(path.exists(), "the Config is written");
        assert!(!secrets_file(&path).exists(), "no key saved");
        assert!(ctx.setup_finished.is_some());
    }

    #[test]
    fn an_empty_field_finishes_like_skip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (mut setup, mut ctx) = on_key(&path);
        act(&mut setup, &mut ctx, Action::Settings);
        assert!(path.exists());
        assert!(!secrets_file(&path).exists());
    }

    #[test]
    fn paste_works_through_paste_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (mut setup, mut ctx) = on_key(&path);
        let pasted = format!("\t {SECRET} \n");
        for input in crate::app::paste_inputs(&pasted) {
            setup.handle(input, &mut ctx);
        }
        act(&mut setup, &mut ctx, Action::Settings);
        let saved = config::load_api_key(&path).unwrap();
        assert_eq!(saved.as_ref().map(config::ApiKey::expose), Some(SECRET));
    }

    /// One thing the player does: a key or gamepad event, or text the keyboard typed (SDL's text
    /// input event).
    enum Ev<'a> {
        Event(ControlEvent),
        Typed(&'a str),
    }

    fn down(key: Keycode) -> Ev<'static> {
        Ev::Event(ControlEvent::KeyDown {
            key,
            repeat: false,
            ctrl: false,
        })
    }

    fn ctrl(key: Keycode) -> Ev<'static> {
        Ev::Event(ControlEvent::KeyDown {
            key,
            repeat: false,
            ctrl: true,
        })
    }

    /// Feeds `evs` to `setup` through `controls` as `app::feed` does: each in the text mode Setup
    /// asks for by then, with Paste turned into `clipboard`'s characters.
    fn feed(setup: &mut Setup, ctx: &mut Ctx, controls: &mut Controls, evs: &[Ev<'_>], clipboard: &str) {
        for ev in evs {
            let mut inputs = Vec::new();
            controls.text = setup.text_input();
            match ev {
                Ev::Event(event) => controls.handle(0, *event, &mut inputs),
                Ev::Typed(text) => controls.typed(text, &mut inputs),
            }
            for input in inputs {
                match input {
                    Input::Action(Action::Paste) => {
                        for paste in crate::app::paste_inputs(clipboard) {
                            setup.handle(paste, ctx);
                        }
                    }
                    input => {
                        setup.handle(input, ctx);
                    }
                }
            }
        }
    }

    /// Setup on step 5 after a frame in which the player used the keyboard.
    fn on_key_by_keyboard(config_path: &std::path::Path) -> (Setup, Ctx, Controls) {
        let (mut setup, mut ctx) = on_key(config_path);
        ctx.input_kind = InputKind::Keyboard;
        setup.tick(&mut ctx);
        (setup, ctx, Controls::new(InputKind::Keyboard))
    }

    #[test]
    fn the_keyboard_types_and_erases_in_the_field_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (mut setup, mut ctx, mut controls) = on_key_by_keyboard(&path);
        assert!(
            setup.text_input(),
            "the focused field is a text field on the keyboard"
        );
        let qef = [
            down(Keycode::Q),
            Ev::Typed("q"),
            down(Keycode::E),
            Ev::Typed("e"),
            down(Keycode::F),
            Ev::Typed("f"),
        ];
        feed(&mut setup, &mut ctx, &mut controls, &qef, "");
        let erase = [
            down(Keycode::Backspace),
            down(Keycode::Backspace),
            down(Keycode::Backspace),
        ];
        feed(&mut setup, &mut ctx, &mut controls, &erase, "");
        assert_eq!(setup.step(), 5, "Backspace erases, and does not go back");
        let rest = [Ev::Typed(SECRET), ctrl(Keycode::Comma)];
        feed(&mut setup, &mut ctx, &mut controls, &rest, "");
        let saved = config::load_api_key(&path).unwrap();
        assert_eq!(
            saved.as_ref().map(config::ApiKey::expose),
            Some(SECRET),
            "Q, E and F typed, and erased"
        );
    }

    #[test]
    fn ctrl_v_pastes_into_the_field_through_controls() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (mut setup, mut ctx, mut controls) = on_key_by_keyboard(&path);
        let pasted = format!("\t {SECRET} \n");
        let evs = [ctrl(Keycode::V), Ev::Typed("v"), ctrl(Keycode::Comma)];
        feed(&mut setup, &mut ctx, &mut controls, &evs, &pasted);
        let saved = config::load_api_key(&path).unwrap();
        assert_eq!(saved.as_ref().map(config::ApiKey::expose), Some(SECRET));
    }

    #[test]
    fn on_the_keyboard_down_leaves_the_field_and_esc_goes_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (mut setup, mut ctx, mut controls) = on_key_by_keyboard(&path);
        assert_eq!(setup.hints()[0], Hint::new(&[Action::Paste], "Paste"));
        feed(&mut setup, &mut ctx, &mut controls, &[down(Keycode::Down)], "");
        assert!(!setup.text_input(), "on Finish, keys act again");
        assert!(setup.hints().iter().all(|h| h.text != "Paste"));
        feed(&mut setup, &mut ctx, &mut controls, &[down(Keycode::Up)], "");
        assert!(setup.text_input());
        feed(&mut setup, &mut ctx, &mut controls, &[down(Keycode::Escape)], "");
        assert_eq!(setup.step(), 4);
    }

    #[test]
    fn enter_in_the_field_finishes_as_the_hint_says() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (mut setup, mut ctx, mut controls) = on_key_by_keyboard(&path);
        assert_eq!(
            setup.hints(),
            [
                Hint::new(&[Action::Paste], "Paste"),
                Hint::new(&[Action::Start], "Finish"),
                Hint::new(&[Action::Back], "Back"),
            ],
            "as in mockup 30: Ctrl V Paste, Enter Finish, Esc Back"
        );
        let evs = [Ev::Typed(SECRET), down(Keycode::Return)];
        feed(&mut setup, &mut ctx, &mut controls, &evs, "");
        assert!(path.exists(), "the Config is written");
        assert!(ctx.setup_finished.is_some());
        let saved = config::load_api_key(&path).unwrap();
        assert_eq!(saved.as_ref().map(config::ApiKey::expose), Some(SECRET));
    }

    #[test]
    fn on_a_controller_plus_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (mut setup, mut ctx) = on_key(&path);
        let mut controls = Controls::new(InputKind::Controller);
        let (pad, button) = (sdl3::joystick::JoystickId::new(7), sdl3::gamepad::Button::Start);
        let plus = [
            Ev::Event(ControlEvent::PadButtonDown { pad, button }),
            Ev::Event(ControlEvent::PadButtonUp { pad, button }),
        ];
        feed(&mut setup, &mut ctx, &mut controls, &plus, "");
        assert!(path.exists(), "the Config is written");
        assert!(ctx.setup_finished.is_some());
    }

    #[test]
    fn on_a_controller_confirm_on_the_field_opens_the_on_screen_keyboard() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (mut setup, mut ctx) = on_key(&path);
        assert!(
            !setup.text_input(),
            "no text field until the on-screen keyboard opens"
        );
        act(&mut setup, &mut ctx, Action::Confirm);
        assert!(setup.text_input());
        assert_eq!(setup.hints(), TextEntry::hints(InputKind::Controller, "Save"));
    }
}
