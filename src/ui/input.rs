//! Maps controller, keyboard and mouse input to UI [`Input`]s (FR-UI-1, FR-UI-2), and times held
//! direction repeat (FR-CAR-7).

use sdl3::gamepad::Button;
use sdl3::keyboard::Keycode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
    Options,
    Sort,
    PrevTab,
    NextTab,
}

impl Action {
    pub const ALL: [Action; 10] = [
        Action::Up,
        Action::Down,
        Action::Left,
        Action::Right,
        Action::Confirm,
        Action::Back,
        Action::Options,
        Action::Sort,
        Action::PrevTab,
        Action::NextTab,
    ];

    pub fn is_direction(self) -> bool {
        matches!(self, Action::Up | Action::Down | Action::Left | Action::Right)
    }

    /// The lowercase name used by `--actions` in screenshot mode.
    pub fn name(self) -> &'static str {
        match self {
            Action::Up => "up",
            Action::Down => "down",
            Action::Left => "left",
            Action::Right => "right",
            Action::Confirm => "confirm",
            Action::Back => "back",
            Action::Options => "options",
            Action::Sort => "sort",
            Action::PrevTab => "prevtab",
            Action::NextTab => "nexttab",
        }
    }

    pub fn from_name(name: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.name() == name)
    }
}

/// Mouse input, in logical 1920×1080 coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pointer {
    Move {
        x: f32,
        y: f32,
    },
    Click {
        x: f32,
        y: f32,
    },
    RightClick {
        x: f32,
        y: f32,
    },
    /// Positive `dy` scrolls up.
    Wheel {
        dy: f32,
    },
    /// The mouse's back button.
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    Action(Action),
    Pointer(Pointer),
}

/// What the player last used, which decides the glyphs the UI shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Controller,
    /// Keyboard or mouse.
    Keyboard,
}

/// Which face button confirms. Only Nintendo style exists so far; a Settings switch to
/// Xbox/PlayStation style comes later (FR-UI-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonStyle {
    /// Positional: the right face button confirms, the bottom one goes back.
    #[default]
    Nintendo,
}

pub fn button_action(style: ButtonStyle, button: Button) -> Option<Action> {
    let action = match (style, button) {
        (ButtonStyle::Nintendo, Button::East) => Action::Confirm,
        (ButtonStyle::Nintendo, Button::South) => Action::Back,
        (ButtonStyle::Nintendo, Button::West) => Action::Options,
        (ButtonStyle::Nintendo, Button::North) => Action::Sort,
        (_, Button::LeftShoulder) => Action::PrevTab,
        (_, Button::RightShoulder) => Action::NextTab,
        (_, Button::DPadUp) => Action::Up,
        (_, Button::DPadDown) => Action::Down,
        (_, Button::DPadLeft) => Action::Left,
        (_, Button::DPadRight) => Action::Right,
        _ => return None,
    };
    Some(action)
}

pub fn key_action(key: Keycode) -> Option<Action> {
    let action = match key {
        Keycode::Up => Action::Up,
        Keycode::Down => Action::Down,
        Keycode::Left => Action::Left,
        Keycode::Right => Action::Right,
        Keycode::Return | Keycode::KpEnter | Keycode::Space => Action::Confirm,
        Keycode::Escape | Keycode::Backspace => Action::Back,
        Keycode::Q => Action::PrevTab,
        Keycode::E | Keycode::Tab => Action::NextTab,
        Keycode::F => Action::Options,
        Keycode::S => Action::Sort,
        _ => return None,
    };
    Some(action)
}

/// Fraction of full deflection the left stick must pass to count as a direction.
pub const STICK_DEADZONE: f32 = 0.5;

/// The direction the left stick points, from its raw axis values (positive y is down).
pub fn stick_direction(x: i16, y: i16) -> Option<Action> {
    let (fx, fy) = (x as f32 / i16::MAX as f32, y as f32 / i16::MAX as f32);
    if fx.abs().max(fy.abs()) < STICK_DEADZONE {
        return None;
    }
    Some(if fx.abs() >= fy.abs() {
        if fx > 0.0 { Action::Right } else { Action::Left }
    } else if fy > 0.0 {
        Action::Down
    } else {
        Action::Up
    })
}

/// The glyph label for an action, following the last-used input kind.
pub fn glyph_label(action: Action, kind: InputKind, style: ButtonStyle) -> &'static str {
    match (kind, style, action) {
        (_, _, Action::Up) => "↑",
        (_, _, Action::Down) => "↓",
        (_, _, Action::Left) => "←",
        (_, _, Action::Right) => "→",
        (InputKind::Controller, ButtonStyle::Nintendo, Action::Confirm) => "A",
        (InputKind::Controller, ButtonStyle::Nintendo, Action::Back) => "B",
        (InputKind::Controller, ButtonStyle::Nintendo, Action::Options) => "Y",
        (InputKind::Controller, ButtonStyle::Nintendo, Action::Sort) => "X",
        (InputKind::Controller, _, Action::PrevTab) => "L1",
        (InputKind::Controller, _, Action::NextTab) => "R1",
        (InputKind::Keyboard, _, Action::Confirm) => "Enter",
        (InputKind::Keyboard, _, Action::Back) => "Esc",
        (InputKind::Keyboard, _, Action::Options) => "F",
        (InputKind::Keyboard, _, Action::Sort) => "S",
        (InputKind::Keyboard, _, Action::PrevTab) => "Q",
        (InputKind::Keyboard, _, Action::NextTab) => "E",
    }
}

/// Delay before a held direction first repeats.
pub const REPEAT_DELAY_MS: u64 = 350;
/// Repeat interval right after the first repeat.
pub const REPEAT_SLOW_MS: u64 = 110;
/// Repeat interval once the direction has been held for [`REPEAT_RAMP_END_MS`].
pub const REPEAT_FAST_MS: u64 = 35;
pub const REPEAT_RAMP_END_MS: u64 = 1500;

/// Held-direction repeat timing, driven by millisecond timestamps. The press itself is the first
/// step; `poll` yields the repeats.
#[derive(Debug, Default)]
pub struct Repeat {
    held: Option<Held>,
}

#[derive(Debug)]
struct Held {
    dir: Action,
    since: u64,
    next: u64,
}

impl Repeat {
    /// A direction went down at `now`; it replaces any direction held before.
    pub fn press(&mut self, dir: Action, now: u64) {
        self.held = Some(Held {
            dir,
            since: now,
            next: now + REPEAT_DELAY_MS,
        });
    }

    /// A direction came up. Releasing a direction other than the held one changes nothing.
    pub fn release(&mut self, dir: Action) {
        if self.held.as_ref().is_some_and(|h| h.dir == dir) {
            self.held = None;
        }
    }

    pub fn release_all(&mut self) {
        self.held = None;
    }

    /// The repeat due at `now`, if any. Yields at most one per call.
    pub fn poll(&mut self, now: u64) -> Option<Action> {
        let held = self.held.as_mut()?;
        if now < held.next {
            return None;
        }
        held.next = now + interval(now - held.since);
        Some(held.dir)
    }
}

/// The repeat interval after a direction has been held for `held_ms`: 110 ms at the first repeat,
/// ramping linearly to 35 ms at 1.5 s.
pub fn interval(held_ms: u64) -> u64 {
    let span = (REPEAT_RAMP_END_MS - REPEAT_DELAY_MS) as f32;
    let t = (held_ms.saturating_sub(REPEAT_DELAY_MS) as f32 / span).min(1.0);
    let slow = REPEAT_SLOW_MS as f32;
    (slow + (REPEAT_FAST_MS as f32 - slow) * t).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Times of the repeats while holding from 0 to `until`, polling every millisecond.
    fn repeats(until: u64) -> Vec<u64> {
        let mut r = Repeat::default();
        r.press(Action::Right, 0);
        (0..=until).filter(|&t| r.poll(t).is_some()).collect()
    }

    #[test]
    fn first_repeat_comes_at_350_ms() {
        let mut r = Repeat::default();
        r.press(Action::Left, 1000);
        assert_eq!(r.poll(1000), None);
        assert_eq!(r.poll(1349), None);
        assert_eq!(r.poll(1350), Some(Action::Left));
    }

    #[test]
    fn second_repeat_follows_after_110_ms() {
        assert_eq!(&repeats(460)[..], &[350, 460]);
    }

    #[test]
    fn repeat_ramps_to_35_ms_by_1500_ms() {
        let times = repeats(3000);
        let gaps: Vec<u64> = times.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(gaps.windows(2).all(|g| g[1] <= g[0]), "gaps never grow: {gaps:?}");
        let late: Vec<u64> = times
            .windows(2)
            .filter(|w| w[0] >= 1500)
            .map(|w| w[1] - w[0])
            .collect();
        assert!(!late.is_empty());
        assert!(late.iter().all(|&g| g == 35), "{late:?}");
        assert_eq!(interval(1500), 35);
        assert_eq!(interval(350), 110);
    }

    #[test]
    fn release_resets() {
        let mut r = Repeat::default();
        r.press(Action::Up, 0);
        assert_eq!(r.poll(400), Some(Action::Up));
        r.release(Action::Up);
        assert_eq!(r.poll(2000), None);
        r.press(Action::Up, 2000);
        assert_eq!(r.poll(2349), None, "the delay starts over");
        assert_eq!(r.poll(2350), Some(Action::Up));
    }

    #[test]
    fn releasing_another_direction_keeps_the_held_one() {
        let mut r = Repeat::default();
        r.press(Action::Up, 0);
        r.press(Action::Down, 10);
        r.release(Action::Up);
        assert_eq!(r.poll(360), Some(Action::Down));
    }

    #[test]
    fn nintendo_style_confirms_on_the_right_face_button() {
        let s = ButtonStyle::Nintendo;
        assert_eq!(button_action(s, Button::East), Some(Action::Confirm));
        assert_eq!(button_action(s, Button::South), Some(Action::Back));
        assert_eq!(button_action(s, Button::West), Some(Action::Options));
        assert_eq!(button_action(s, Button::North), Some(Action::Sort));
        assert_eq!(button_action(s, Button::LeftShoulder), Some(Action::PrevTab));
        assert_eq!(button_action(s, Button::RightShoulder), Some(Action::NextTab));
        assert_eq!(button_action(s, Button::DPadLeft), Some(Action::Left));
        assert_eq!(button_action(s, Button::Guide), None);
    }

    #[test]
    fn keyboard_mapping() {
        let cases = [
            (Keycode::Up, Action::Up),
            (Keycode::Right, Action::Right),
            (Keycode::Return, Action::Confirm),
            (Keycode::Space, Action::Confirm),
            (Keycode::Escape, Action::Back),
            (Keycode::Backspace, Action::Back),
            (Keycode::Q, Action::PrevTab),
            (Keycode::E, Action::NextTab),
            (Keycode::Tab, Action::NextTab),
            (Keycode::F, Action::Options),
            (Keycode::S, Action::Sort),
        ];
        for (key, action) in cases {
            assert_eq!(key_action(key), Some(action), "{key:?}");
        }
        assert_eq!(key_action(Keycode::F11), None);
    }

    #[test]
    fn stick_needs_half_deflection() {
        assert_eq!(stick_direction(16000, 0), None);
        assert_eq!(stick_direction(20000, 0), Some(Action::Right));
        assert_eq!(stick_direction(-20000, 5000), Some(Action::Left));
        assert_eq!(stick_direction(3000, -30000), Some(Action::Up));
        assert_eq!(stick_direction(0, 32767), Some(Action::Down));
    }

    #[test]
    fn hint_labels_follow_the_input_kind() {
        let s = ButtonStyle::Nintendo;
        let pad = |a| glyph_label(a, InputKind::Controller, s);
        let keys = |a| glyph_label(a, InputKind::Keyboard, s);
        assert_eq!(
            [
                Action::Confirm,
                Action::Back,
                Action::Sort,
                Action::Options,
                Action::PrevTab,
                Action::NextTab
            ]
            .map(pad),
            ["A", "B", "X", "Y", "L1", "R1"]
        );
        assert_eq!(
            [
                Action::Confirm,
                Action::Back,
                Action::Sort,
                Action::Options,
                Action::PrevTab,
                Action::NextTab
            ]
            .map(keys),
            ["Enter", "Esc", "S", "F", "Q", "E"]
        );
    }

    #[test]
    fn action_names_round_trip() {
        for a in Action::ALL {
            assert_eq!(Action::from_name(a.name()), Some(a));
        }
        assert_eq!(Action::from_name("jump"), None);
    }
}
