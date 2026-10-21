//! A one-line text field with an on-screen keyboard (FR-UI-1): typed into on a physical keyboard
//! (its text arrives as [`Input::Text`]), or key by key with a controller or the mouse. Screens
//! embed it, hand it their input and act on the [`Outcome`]; renaming a Game uses it, and so will
//! search.
//!
//! Controller: the d-pad moves over the keys, A types the focused one, Y erases, X types a space,
//! + finishes and B cancels. Keyboard: type, Enter finishes, Esc cancels. Mouse: click the keys.

use super::gfx::{Align, Gfx, TextStyle};
use super::input::{Action, Input, InputKind, Pointer, TextInput};
use super::nav::Hint;
use super::text::Weight;
use super::theme::{ACCENT, LINE, MUTED, SURFACE2, TEXT};
use super::{Rect, hit};

/// What one input did to the entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing the screen needs to know about: focus moved, or a key changed the layout.
    None,
    /// The text changed.
    Changed,
    /// Done: the screen takes [`TextEntry::text`].
    Submit,
    /// Cancelled: the screen drops the entry.
    Cancel,
}

/// One key of the on-screen keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Shift,
    Backspace,
    /// Switches between letters and symbols.
    Layer,
    Space,
    Done,
}

/// The two key layouts the [`Key::Layer`] key switches between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Letters,
    Symbols,
}

/// Shift on the letter keys: off, for the next letter only, or locked on until pressed again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    Off,
    Once,
    Lock,
}

type Row = &'static [(Key, u8)];

const fn c(ch: char) -> (Key, u8) {
    (Key::Char(ch), 2)
}

/// Rows of keys with their widths in half keys; every row is 20 half keys wide.
const DIGITS: Row = &[
    c('1'),
    c('2'),
    c('3'),
    c('4'),
    c('5'),
    c('6'),
    c('7'),
    c('8'),
    c('9'),
    c('0'),
];
const BOTTOM: Row = &[(Key::Layer, 4), c('-'), (Key::Space, 8), c(':'), (Key::Done, 4)];
const LETTERS: [Row; 5] = [
    DIGITS,
    &[
        c('q'),
        c('w'),
        c('e'),
        c('r'),
        c('t'),
        c('y'),
        c('u'),
        c('i'),
        c('o'),
        c('p'),
    ],
    &[
        c('a'),
        c('s'),
        c('d'),
        c('f'),
        c('g'),
        c('h'),
        c('j'),
        c('k'),
        c('l'),
        c('\''),
    ],
    &[
        (Key::Shift, 3),
        c('z'),
        c('x'),
        c('c'),
        c('v'),
        c('b'),
        c('n'),
        c('m'),
        (Key::Backspace, 3),
    ],
    BOTTOM,
];
const SYMBOLS: [Row; 5] = [
    DIGITS,
    &[
        c('!'),
        c('?'),
        c('&'),
        c('.'),
        c(','),
        c('('),
        c(')'),
        c('/'),
        c('+'),
        c('#'),
    ],
    &[
        c('@'),
        c('%'),
        c('*'),
        c('='),
        c('_'),
        c('"'),
        c(';'),
        c('~'),
        c('['),
        c(']'),
    ],
    &[
        c('é'),
        c('è'),
        c('à'),
        c('ü'),
        c('ö'),
        c('ñ'),
        c('ç'),
        c('ß'),
        (Key::Backspace, 4),
    ],
    BOTTOM,
];

/// Horizontal pitch of half a key; keys are separated by [`GAP`].
const HALF: f32 = 57.0;
const GAP: f32 = 10.0;
const KEY_H: f32 = 76.0;
const ROW_PITCH: f32 = KEY_H + GAP;
/// Size of the on-screen keyboard as [`TextEntry::draw_keyboard`] draws it.
pub const KEYBOARD_W: f32 = 20.0 * HALF - GAP;
pub const KEYBOARD_H: f32 = 5.0 * ROW_PITCH - GAP;

pub struct TextEntry {
    text: String,
    /// The most characters (Unicode scalar values) typing can make the text.
    max_chars: usize,
    layer: Layer,
    shift: Shift,
    /// The focused key: row, and index in the row.
    focus: (usize, usize),
    /// Where the last frame drew each key.
    key_hits: Vec<(Rect, (usize, usize))>,
}

impl TextEntry {
    /// An entry holding `text`, which typing can lengthen up to `max_chars` characters. Focus
    /// starts on the first letter.
    pub fn new(text: &str, max_chars: usize) -> TextEntry {
        TextEntry {
            text: text.to_owned(),
            max_chars,
            layer: Layer::Letters,
            shift: Shift::Off,
            focus: (1, 0),
            key_hits: Vec::new(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn layer(&self) -> Layer {
        self.layer
    }

    pub fn shift(&self) -> Shift {
        self.shift
    }

    /// The key with focus.
    pub fn focused(&self) -> Key {
        self.rows()[self.focus.0][self.focus.1].0
    }

    fn rows(&self) -> &'static [Row; 5] {
        match self.layer {
            Layer::Letters => &LETTERS,
            Layer::Symbols => &SYMBOLS,
        }
    }

    pub fn handle(&mut self, input: Input) -> Outcome {
        match input {
            Input::Text(TextInput::Char(ch)) => self.insert(ch),
            Input::Text(TextInput::Backspace) | Input::Action(Action::Options) => self.erase(),
            Input::Action(Action::Sort) => self.insert(' '),
            Input::Action(Action::Confirm) => self.press(self.focused()),
            Input::Action(Action::Start) => Outcome::Submit,
            Input::Action(Action::Back) | Input::Pointer(Pointer::Back) => Outcome::Cancel,
            Input::Action(dir @ (Action::Up | Action::Down | Action::Left | Action::Right)) => {
                self.move_focus(dir);
                Outcome::None
            }
            Input::Pointer(Pointer::Move { x, y }) => {
                if let Some(at) = hit(&self.key_hits, x, y) {
                    self.focus = at;
                }
                Outcome::None
            }
            Input::Pointer(Pointer::Click { x, y }) => match hit(&self.key_hits, x, y) {
                Some(at) => {
                    self.focus = at;
                    self.press(self.focused())
                }
                None => Outcome::None,
            },
            _ => Outcome::None,
        }
    }

    /// Appends `ch` unless the text is full or it is a control character.
    fn insert(&mut self, ch: char) -> Outcome {
        if ch.is_control() || self.text.chars().count() >= self.max_chars {
            return Outcome::None;
        }
        self.text.push(ch);
        Outcome::Changed
    }

    fn erase(&mut self) -> Outcome {
        match self.text.pop() {
            Some(_) => Outcome::Changed,
            None => Outcome::None,
        }
    }

    fn press(&mut self, key: Key) -> Outcome {
        match key {
            Key::Char(ch) => {
                let shifted = self.shift != Shift::Off && ch.is_lowercase();
                if self.shift == Shift::Once {
                    self.shift = Shift::Off;
                }
                if !shifted {
                    return self.insert(ch);
                }
                // An uppercase form may be several characters; it goes in whole or not at all.
                let upper: String = ch.to_uppercase().collect();
                if self.text.chars().count() + upper.chars().count() > self.max_chars {
                    return Outcome::None;
                }
                self.text.push_str(&upper);
                Outcome::Changed
            }
            Key::Shift => {
                self.shift = match self.shift {
                    Shift::Off => Shift::Once,
                    Shift::Once => Shift::Lock,
                    Shift::Lock => Shift::Off,
                };
                Outcome::None
            }
            Key::Backspace => self.erase(),
            Key::Layer => {
                self.layer = match self.layer {
                    Layer::Letters => Layer::Symbols,
                    Layer::Symbols => Layer::Letters,
                };
                let row = self.rows()[self.focus.0];
                self.focus.1 = self.focus.1.min(row.len() - 1);
                Outcome::None
            }
            Key::Space => self.insert(' '),
            Key::Done => Outcome::Submit,
        }
    }

    /// Left and right wrap around the row; up and down go to the key below the focused key's
    /// centre, stopping at the top and bottom rows.
    fn move_focus(&mut self, dir: Action) {
        let rows = self.rows();
        let (r, i) = self.focus;
        let len = rows[r].len();
        // Up and down aim at the centre of the key being left.
        let to_row = |to: usize| (to, key_at(rows[to], centre(rows[r], i)));
        self.focus = match dir {
            Action::Left => (r, (i + len - 1) % len),
            Action::Right => (r, (i + 1) % len),
            Action::Up if r > 0 => to_row(r - 1),
            Action::Down if r + 1 < rows.len() => to_row(r + 1),
            _ => (r, i),
        };
    }

    /// The hint bar for an embedding screen, with `submit` naming what finishing does. On the
    /// keyboard only finishing and cancelling are listed: the other keys type.
    pub fn hints(kind: InputKind, submit: &'static str) -> Vec<Hint> {
        match kind {
            InputKind::Keyboard => vec![
                Hint::new(&[Action::Start], submit),
                Hint::new(&[Action::Back], "Cancel"),
            ],
            InputKind::Controller => vec![
                Hint::new(&[Action::Confirm], "Type"),
                Hint::new(&[Action::Options], "Backspace"),
                Hint::new(&[Action::Sort], "Space"),
                Hint::new(&[Action::Back], "Cancel"),
                Hint::new(&[Action::Start], submit),
            ],
        }
    }

    /// Draws the field into `r`: the text and a caret after it, showing the end when it is too
    /// long. An empty field shows `placeholder`.
    pub fn draw_field(&mut self, gfx: &mut Gfx<'_>, r: Rect, placeholder: &str) {
        gfx.fill_round_rect(r, 14.0, SURFACE2);
        gfx.stroke_round_rect(r, 14.0, 2.0, ACCENT);
        let style = TextStyle::new(32, Weight::SemiBold, TEXT);
        let (x, base) = (r.x + 28.0, r.y + r.h / 2.0 + 12.0);
        let room = r.w - 64.0;
        if self.text.is_empty() {
            let dim = TextStyle::new(32, Weight::Medium, MUTED);
            gfx.text(x + 12.0, base, placeholder, dim, Align::Left);
        }
        let mut shown = self.text.as_str();
        let mut prefix = "";
        while gfx.measure(shown, style) > room {
            let mut chars = shown.chars();
            chars.next();
            shown = chars.as_str();
            prefix = "…";
        }
        let shown = format!("{prefix}{shown}");
        let w = gfx.text(x, base, &shown, style, Align::Left);
        gfx.fill_rect(Rect::new(x + w + 3.0, r.y + 18.0, 3.0, r.h - 36.0), ACCENT);
    }

    /// Draws the on-screen keyboard with its top left at (`x`, `y`), [`KEYBOARD_W`] by
    /// [`KEYBOARD_H`], and remembers where each key went for the mouse.
    pub fn draw_keyboard(&mut self, gfx: &mut Gfx<'_>, x: f32, y: f32) {
        self.key_hits.clear();
        for (r, row) in self.rows().iter().enumerate() {
            let mut start = 0;
            for (i, &(key, halves)) in row.iter().enumerate() {
                let rect = Rect::new(
                    x + start as f32 * HALF,
                    y + r as f32 * ROW_PITCH,
                    halves as f32 * HALF - GAP,
                    KEY_H,
                );
                start += halves;
                self.draw_key(gfx, rect, key, self.focus == (r, i));
                self.key_hits.push((rect, (r, i)));
            }
        }
    }

    fn draw_key(&self, gfx: &mut Gfx<'_>, r: Rect, key: Key, focused: bool) {
        let special = !matches!(key, Key::Char(_) | Key::Space);
        let fill = match (focused, special) {
            (true, _) => ACCENT,
            (false, true) => LINE,
            (false, false) => SURFACE2,
        };
        gfx.fill_round_rect(r, 12.0, fill);
        let shifted = self.shift != Shift::Off;
        let upper;
        let label = match key {
            Key::Char(ch) if shifted && ch.is_lowercase() => {
                upper = ch.to_uppercase().collect::<String>();
                upper.as_str()
            }
            Key::Char(ch) => {
                upper = ch.to_string();
                upper.as_str()
            }
            Key::Shift if self.shift == Shift::Lock => "Caps lock",
            Key::Shift => "Shift",
            Key::Backspace => "Backspace",
            Key::Layer if self.layer == Layer::Letters => "?123",
            Key::Layer => "ABC",
            Key::Space => "Space",
            Key::Done => "Done",
        };
        let ink = match key {
            _ if focused => TEXT,
            Key::Shift if shifted => ACCENT,
            Key::Space => MUTED,
            _ if special => MUTED,
            _ => TEXT,
        };
        let style = match key {
            Key::Char(_) => TextStyle::new(28, Weight::SemiBold, ink),
            _ => TextStyle::new(20, Weight::SemiBold, ink),
        };
        let base = r.y + r.h / 2.0 + if matches!(key, Key::Char(_)) { 10.0 } else { 7.0 };
        gfx.text(r.x + r.w / 2.0, base, label, style, Align::Centre);
    }
}

/// The centre of key `i` of `row`, in half keys from the left.
fn centre(row: Row, i: usize) -> f32 {
    let start: u8 = row[..i].iter().map(|k| k.1).sum();
    start as f32 + row[i].1 as f32 / 2.0
}

/// The index of the key of `row` under `at` half keys from the left.
fn key_at(row: Row, at: f32) -> usize {
    let mut end = 0.0;
    for (i, k) in row.iter().enumerate() {
        end += k.1 as f32;
        if at < end {
            return i;
        }
    }
    row.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(entry: &mut TextEntry, action: Action) -> Outcome {
        entry.handle(Input::Action(action))
    }

    fn type_text(entry: &mut TextEntry, text: &str) {
        for ch in text.chars() {
            entry.handle(Input::Text(TextInput::Char(ch)));
        }
    }

    /// Moves focus onto `key` in the current layer with d-pad steps, as a player would: down or
    /// up to its row, then right along it.
    fn go_to(entry: &mut TextEntry, key: Key) {
        let rows = entry.rows();
        let (r, i) = (0..rows.len())
            .find_map(|r| rows[r].iter().position(|k| k.0 == key).map(|i| (r, i)))
            .unwrap_or_else(|| panic!("{key:?} isn't on the {:?} keys", entry.layer));
        while entry.focus.0 < r {
            act(entry, Action::Down);
        }
        while entry.focus.0 > r {
            act(entry, Action::Up);
        }
        while entry.focus.1 != i {
            act(entry, Action::Right);
        }
        assert_eq!(entry.focused(), key);
    }

    fn press(entry: &mut TextEntry, key: Key) -> Outcome {
        go_to(entry, key);
        act(entry, Action::Confirm)
    }

    #[test]
    fn every_row_is_twenty_half_keys_wide() {
        for row in LETTERS.iter().chain(&SYMBOLS) {
            assert_eq!(row.iter().map(|k| k.1 as u32).sum::<u32>(), 20, "{row:?}");
        }
    }

    #[test]
    fn typing_appends_and_backspace_erases() {
        let mut entry = TextEntry::new("Zel", 100);
        assert_eq!(entry.handle(Input::Text(TextInput::Char('d'))), Outcome::Changed);
        type_text(&mut entry, "a: ok");
        assert_eq!(entry.text(), "Zelda: ok");
        assert_eq!(entry.handle(Input::Text(TextInput::Backspace)), Outcome::Changed);
        assert_eq!(act(&mut entry, Action::Options), Outcome::Changed, "Y erases too");
        assert_eq!(entry.text(), "Zelda: ");
        assert_eq!(act(&mut entry, Action::Sort), Outcome::Changed, "X types a space");
        assert_eq!(entry.text(), "Zelda:  ");

        let mut empty = TextEntry::new("", 100);
        assert_eq!(
            act(&mut empty, Action::Options),
            Outcome::None,
            "nothing to erase"
        );
        type_text(&mut empty, "a\tb\n");
        assert_eq!(empty.text(), "ab", "control characters are dropped");
    }

    #[test]
    fn unicode_is_typed_and_erased_a_character_at_a_time() {
        let mut entry = TextEntry::new("Pok", 100);
        type_text(&mut entry, "émon ポケモン 🎮");
        assert_eq!(entry.text(), "Pokémon ポケモン 🎮");
        entry.handle(Input::Text(TextInput::Backspace));
        entry.handle(Input::Text(TextInput::Backspace));
        entry.handle(Input::Text(TextInput::Backspace));
        assert_eq!(entry.text(), "Pokémon ポケモ");
    }

    #[test]
    fn typing_stops_at_the_length_cap_counted_in_characters() {
        let mut entry = TextEntry::new("ab", 5);
        type_text(&mut entry, "éé");
        assert_eq!(entry.handle(Input::Text(TextInput::Char('ü'))), Outcome::Changed);
        assert_eq!(entry.handle(Input::Text(TextInput::Char('x'))), Outcome::None);
        assert_eq!(act(&mut entry, Action::Sort), Outcome::None);
        assert_eq!(press(&mut entry, Key::Char('q')), Outcome::None);
        assert_eq!(entry.text(), "abééü");
        entry.handle(Input::Text(TextInput::Backspace));
        assert_eq!(press(&mut entry, Key::Char('q')), Outcome::Changed);
        assert_eq!(entry.text(), "abééq");

        let long = TextEntry::new("a longer title", 5);
        assert_eq!(long.text(), "a longer title", "text given at the start isn't cut");
    }

    #[test]
    fn the_dpad_moves_over_the_keys_and_a_types_the_focused_one() {
        let mut entry = TextEntry::new("", 100);
        assert_eq!(
            entry.focused(),
            Key::Char('q'),
            "focus starts on the first letter"
        );
        act(&mut entry, Action::Up);
        assert_eq!(entry.focused(), Key::Char('1'));
        act(&mut entry, Action::Up);
        assert_eq!(entry.focused(), Key::Char('1'), "stops at the top");
        act(&mut entry, Action::Left);
        assert_eq!(entry.focused(), Key::Char('0'), "left wraps around the row");
        act(&mut entry, Action::Right);
        assert_eq!(entry.focused(), Key::Char('1'), "and right wraps back");
        for (dir, key) in [
            (Action::Down, Key::Char('q')),
            (Action::Down, Key::Char('a')),
            (Action::Down, Key::Shift),
            (Action::Down, Key::Layer),
            (Action::Down, Key::Layer),
            (Action::Right, Key::Char('-')),
            (Action::Right, Key::Space),
            (Action::Up, Key::Char('v')),
            (Action::Right, Key::Char('b')),
            (Action::Up, Key::Char('j')),
            (Action::Right, Key::Char('k')),
            (Action::Right, Key::Char('l')),
            (Action::Right, Key::Char('\'')),
            (Action::Down, Key::Backspace),
            (Action::Down, Key::Done),
        ] {
            act(&mut entry, dir);
            assert_eq!(entry.focused(), key, "after {dir:?}");
        }
        for key in [
            Key::Char('m'),
            Key::Char('k'),
            Key::Char('8'),
            Key::Space,
            Key::Char(':'),
        ] {
            assert_eq!(press(&mut entry, key), Outcome::Changed);
        }
        assert_eq!(entry.text(), "mk8 :");
        assert_eq!(press(&mut entry, Key::Backspace), Outcome::Changed);
        assert_eq!(entry.text(), "mk8 ");
        assert_eq!(press(&mut entry, Key::Done), Outcome::Submit);
        assert_eq!(act(&mut entry, Action::Start), Outcome::Submit, "+ finishes too");
        assert_eq!(act(&mut entry, Action::Back), Outcome::Cancel);
        assert_eq!(entry.handle(Input::Pointer(Pointer::Back)), Outcome::Cancel);
    }

    #[test]
    fn shift_capitalises_the_next_letter_twice_locks_and_thrice_turns_off() {
        let mut entry = TextEntry::new("", 100);
        assert_eq!(press(&mut entry, Key::Shift), Outcome::None);
        assert_eq!(entry.shift(), Shift::Once);
        press(&mut entry, Key::Char('m'));
        press(&mut entry, Key::Char('k'));
        assert_eq!(entry.text(), "Mk", "only the next letter");
        assert_eq!(entry.shift(), Shift::Off);
        press(&mut entry, Key::Shift);
        press(&mut entry, Key::Shift);
        assert_eq!(entry.shift(), Shift::Lock);
        press(&mut entry, Key::Char('d'));
        press(&mut entry, Key::Char('s'));
        press(&mut entry, Key::Char('9'));
        assert_eq!(entry.text(), "MkDS9", "digits stay digits");
        press(&mut entry, Key::Shift);
        assert_eq!(entry.shift(), Shift::Off);
        press(&mut entry, Key::Char('x'));
        type_text(&mut entry, "y");
        assert_eq!(entry.text(), "MkDS9xy", "shift never changes typed text");
    }

    #[test]
    fn the_layer_key_switches_to_symbols_and_back() {
        let mut entry = TextEntry::new("", 100);
        assert_eq!(press(&mut entry, Key::Layer), Outcome::None);
        assert_eq!(entry.layer(), Layer::Symbols);
        assert_eq!(entry.focused(), Key::Layer, "focus stays on the layer key");
        for key in ['é', '&', '!', '.'] {
            press(&mut entry, Key::Char(key));
        }
        assert_eq!(entry.text(), "é&!.");
        go_to(&mut entry, Key::Char('ß'));
        act(&mut entry, Action::Right);
        assert_eq!(entry.focused(), Key::Backspace);
        press(&mut entry, Key::Layer);
        assert_eq!(entry.layer(), Layer::Letters);
        press(&mut entry, Key::Char('p'));
        assert_eq!(entry.text(), "é&!.p");
    }

    #[test]
    fn the_mouse_focuses_and_clicks_keys() {
        let mut entry = TextEntry::new("", 100);
        entry.key_hits = vec![
            (Rect::new(0.0, 0.0, 50.0, 50.0), (1, 2)),
            (Rect::new(60.0, 0.0, 50.0, 50.0), (4, 4)),
        ];
        entry.handle(Input::Pointer(Pointer::Move { x: 10.0, y: 10.0 }));
        assert_eq!(entry.focused(), Key::Char('e'), "hover focuses");
        let click = |x| Input::Pointer(Pointer::Click { x, y: 10.0 });
        assert_eq!(entry.handle(click(10.0)), Outcome::Changed);
        assert_eq!(entry.text(), "e");
        assert_eq!(entry.handle(click(55.0)), Outcome::None, "between keys");
        assert_eq!(entry.handle(click(70.0)), Outcome::Submit, "Done");
    }

    #[test]
    fn the_hints_follow_the_input_kind() {
        let texts = |kind| {
            TextEntry::hints(kind, "Save")
                .iter()
                .map(|h| h.text)
                .collect::<Vec<_>>()
        };
        assert_eq!(texts(InputKind::Keyboard), ["Save", "Cancel"]);
        assert_eq!(
            texts(InputKind::Controller),
            ["Type", "Backspace", "Space", "Cancel", "Save"]
        );
    }
}
