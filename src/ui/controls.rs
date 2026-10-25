//! Turns key and gamepad events into UI [`Input`]s, apart from SDL's event loop so it can be tested
//! with plain timestamps (FR-UI-1, FR-CAR-7).
//!
//! Directions are tracked per source (the keyboard, and each gamepad's d-pad and left stick). The
//! most recently pressed direction still held drives the repeat; releasing it falls back to the
//! next most recent one.

use std::collections::HashMap;

use sdl3::gamepad::Button;
use sdl3::joystick::JoystickId;
use sdl3::keyboard::Keycode;

use super::input::{
    Action, ButtonStyle, Input, InputKind, Repeat, TextInput, button_action, ctrl_key_action, key_action,
    stick_direction, text_key,
};

/// Below this fraction of full deflection, an engaged stick direction releases. It engages at
/// [`super::input::STICK_DEADZONE`].
pub const STICK_RELEASE: f32 = 0.4;
/// How far the other axis must exceed an engaged stick direction to take over.
pub const STICK_SWITCH_MARGIN: f32 = 0.2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickAxis {
    X,
    Y,
}

/// The key and gamepad events the UI reacts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControlEvent {
    /// `repeat` marks the operating system's own key repeat; `ctrl`, a Ctrl key held with it.
    KeyDown {
        key: Keycode,
        repeat: bool,
        ctrl: bool,
    },
    KeyUp {
        key: Keycode,
    },
    PadButtonDown {
        pad: JoystickId,
        button: Button,
    },
    PadButtonUp {
        pad: JoystickId,
        button: Button,
    },
    /// The left stick moved on one axis; positive y is down.
    StickMotion {
        pad: JoystickId,
        axis: StickAxis,
        value: i16,
    },
    PadRemoved {
        pad: JoystickId,
    },
    FocusLost,
}

/// Where a held direction comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Keyboard,
    DPad(JoystickId),
    Stick(JoystickId),
}

impl Source {
    fn pad(self) -> Option<JoystickId> {
        match self {
            Source::Keyboard => None,
            Source::DPad(pad) | Source::Stick(pad) => Some(pad),
        }
    }
}

/// One gamepad's left stick: its raw axes and the direction it holds.
#[derive(Debug, Default)]
struct Stick {
    x: i16,
    y: i16,
    dir: Option<Action>,
}

impl Stick {
    /// Records an axis value and returns the direction the stick now holds. A held direction stays
    /// until it falls below [`STICK_RELEASE`] or the other axis exceeds it by [`STICK_SWITCH_MARGIN`].
    fn moved(&mut self, axis: StickAxis, value: i16) -> Option<Action> {
        match axis {
            StickAxis::X => self.x = value,
            StickAxis::Y => self.y = value,
        }
        let (fx, fy) = (self.x as f32 / i16::MAX as f32, self.y as f32 / i16::MAX as f32);
        if let Some(dir) = self.dir
            && stick_holds(dir, fx, fy)
        {
            return Some(dir);
        }
        self.dir = stick_direction(self.x, self.y);
        self.dir
    }

    /// Records an axis value without taking a direction, while raw controller input is on.
    fn track(&mut self, axis: StickAxis, value: i16) {
        match axis {
            StickAxis::X => self.x = value,
            StickAxis::Y => self.y = value,
        }
    }
}

/// Whether the stick at (`fx`, `fy`) still holds `dir`.
fn stick_holds(dir: Action, fx: f32, fy: f32) -> bool {
    let (along, across) = match dir {
        Action::Right => (fx, fy),
        Action::Left => (-fx, fy),
        Action::Down => (fy, fx),
        _ => (-fy, fx),
    };
    along >= STICK_RELEASE && across.abs() <= along + STICK_SWITCH_MARGIN
}

/// Key and gamepad input state: the last input kind, held directions and their repeat.
#[derive(Debug)]
pub struct Controls {
    pub kind: InputKind,
    pub style: ButtonStyle,
    /// Whether a text field is active: keys then type rather than act (see [`text_key`]).
    pub text: bool,
    /// Whether the top screen wants raw controller input (see [`super::nav::Screen::raw_controller`]):
    /// while on, pad buttons and stick motion are tracked but never become menu actions.
    pub raw: bool,
    /// Whether the last key went down with Ctrl held: text typed after it is dropped, so Ctrl+F
    /// or Ctrl+comma never types an "f" or a ",", whatever the system's input method sends.
    ctrl: bool,
    /// Held directions, oldest press first; the last one drives the repeat.
    held: Vec<(Source, Action)>,
    sticks: HashMap<JoystickId, Stick>,
    repeat: Repeat,
    /// `raw` as last synced by [`Self::sync_raw`], to notice when it turns on.
    synced_raw: bool,
}

impl Controls {
    pub fn new(kind: InputKind) -> Controls {
        Controls {
            kind,
            style: ButtonStyle::default(),
            text: false,
            raw: false,
            ctrl: false,
            held: Vec::new(),
            sticks: HashMap::new(),
            repeat: Repeat::default(),
            synced_raw: false,
        }
    }

    /// Drops pad-sourced held directions when `raw` turns on, so a direction held as the screen
    /// opens never repeats; stick directions are cleared the same way, through [`Self::follow`].
    fn sync_raw(&mut self, at: u64) {
        if self.raw == self.synced_raw {
            return;
        }
        self.synced_raw = self.raw;
        if self.raw {
            let driver = self.held.last().copied();
            self.held.retain(|(source, _)| source.pad().is_none());
            self.follow(driver, at);
            for stick in self.sticks.values_mut() {
                stick.dir = None;
            }
        }
    }

    /// Handles an event that happened at `at` milliseconds, pushing the inputs it makes to `out`.
    pub fn handle(&mut self, at: u64, event: ControlEvent, out: &mut Vec<Input>) {
        self.sync_raw(at);
        if let ControlEvent::KeyDown {
            key,
            repeat: false,
            ctrl: true,
        } = event
            && let Some(action) = ctrl_key_action(key, self.text)
        {
            self.kind = InputKind::Keyboard;
            self.ctrl = true;
            self.press(Source::Keyboard, action, at, out);
            return;
        }
        match event {
            // Held Backspace erases at the operating system's key repeat.
            ControlEvent::KeyDown {
                key,
                repeat: true,
                ctrl,
            } if self.text => {
                self.ctrl = ctrl;
                if !ctrl && let Some(erase @ Input::Text(_)) = text_key(key) {
                    out.push(erase);
                }
            }
            ControlEvent::KeyDown {
                repeat: true, ctrl, ..
            } => self.ctrl = ctrl,
            ControlEvent::KeyDown {
                key,
                repeat: false,
                ctrl,
            } if self.text => {
                self.kind = InputKind::Keyboard;
                self.ctrl = ctrl;
                match text_key(key) {
                    Some(Input::Action(action)) => self.press(Source::Keyboard, action, at, out),
                    Some(input) => out.push(input),
                    None => {}
                }
            }
            ControlEvent::KeyDown {
                key,
                repeat: false,
                ctrl,
            } => {
                self.kind = InputKind::Keyboard;
                self.ctrl = ctrl;
                if let Some(action) = key_action(key) {
                    self.press(Source::Keyboard, action, at, out);
                }
            }
            ControlEvent::KeyUp { key } => {
                if let Some(action) = key_action(key) {
                    self.release(Source::Keyboard, action, at);
                }
            }
            ControlEvent::PadButtonDown { pad, button } => {
                self.kind = InputKind::Controller;
                if self.raw {
                    return;
                }
                if let Some(action) = self.button_action(button) {
                    self.press(Source::DPad(pad), action, at, out);
                }
            }
            ControlEvent::PadButtonUp { pad, button } => {
                if self.raw {
                    return;
                }
                if let Some(action) = self.button_action(button) {
                    self.release(Source::DPad(pad), action, at);
                }
            }
            ControlEvent::StickMotion { pad, axis, value } => {
                let stick = self.sticks.entry(pad).or_default();
                if self.raw {
                    stick.track(axis, value);
                    return;
                }
                let old = stick.dir;
                let new = stick.moved(axis, value);
                if new != old {
                    if let Some(old) = old {
                        self.release(Source::Stick(pad), old, at);
                    }
                    if let Some(new) = new {
                        self.kind = InputKind::Controller;
                        self.press(Source::Stick(pad), new, at, out);
                    }
                }
            }
            ControlEvent::PadRemoved { pad } => {
                self.sticks.remove(&pad);
                let driver = self.held.last().copied();
                self.held.retain(|(source, _)| source.pad() != Some(pad));
                self.follow(driver, at);
            }
            ControlEvent::FocusLost => {
                self.held.clear();
                self.repeat.release_all();
                for stick in self.sticks.values_mut() {
                    stick.dir = None;
                }
            }
        }
    }

    /// Text the keyboard typed (SDL's text input event), as one input per character; nothing unless
    /// a text field is active, or right after a key went down with Ctrl.
    pub fn typed(&mut self, text: &str, out: &mut Vec<Input>) {
        if self.text && !self.ctrl {
            self.kind = InputKind::Keyboard;
            out.extend(text.chars().map(|c| Input::Text(TextInput::Char(c))));
        }
    }

    /// The repeat of the held direction due at `now`, if any.
    pub fn poll(&mut self, now: u64) -> Option<Input> {
        self.sync_raw(now);
        self.repeat.poll(now).map(Input::Action)
    }

    /// What a gamepad button does: Start opens Settings, except in a text field, which it finishes.
    fn button_action(&self, button: Button) -> Option<Action> {
        match button_action(self.style, button)? {
            Action::Start if !self.text => Some(Action::Settings),
            action => Some(action),
        }
    }

    fn press(&mut self, source: Source, action: Action, at: u64, out: &mut Vec<Input>) {
        out.push(Input::Action(action));
        if action.is_direction() {
            self.held.retain(|&held| held != (source, action));
            self.held.push((source, action));
            self.repeat.press(action, at);
        }
    }

    fn release(&mut self, source: Source, action: Action, at: u64) {
        let driver = self.held.last().copied();
        self.held.retain(|&held| held != (source, action));
        self.follow(driver, at);
    }

    /// Hands the repeat to the most recent direction still held, after `driver` drove it. A
    /// different direction starts with a fresh delay and no extra step; the same direction held on
    /// another source keeps its timing.
    fn follow(&mut self, driver: Option<(Source, Action)>, at: u64) {
        let next = self.held.last().copied();
        if next == driver {
            return;
        }
        match next {
            None => self.repeat.release_all(),
            Some((_, dir)) if Some(dir) == driver.map(|(_, d)| d) => {}
            Some((_, dir)) => self.repeat.press(dir, at),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAD: JoystickId = JoystickId::new(7);
    const OTHER_PAD: JoystickId = JoystickId::new(8);

    /// An axis value at `f` of full deflection.
    fn axis(f: f32) -> i16 {
        (f * i16::MAX as f32).round() as i16
    }

    fn key_down(key: Keycode) -> ControlEvent {
        ControlEvent::KeyDown {
            key,
            repeat: false,
            ctrl: false,
        }
    }

    fn key_up(key: Keycode) -> ControlEvent {
        ControlEvent::KeyUp { key }
    }

    fn stick(axis_: StickAxis, f: f32) -> ControlEvent {
        ControlEvent::StickMotion {
            pad: PAD,
            axis: axis_,
            value: axis(f),
        }
    }

    /// Feeds timed events, polling the repeat every millisecond from the first event to `until`,
    /// and returns every step with its time.
    fn run(events: &[(u64, ControlEvent)], until: u64) -> Vec<(u64, Action)> {
        let mut controls = Controls::new(InputKind::Keyboard);
        let mut steps = Vec::new();
        let mut out = Vec::new();
        let start = events.first().map_or(0, |e| e.0);
        let mut next = events.iter().peekable();
        for now in start..=until {
            while let Some(&&(at, event)) = next.peek()
                && at == now
            {
                controls.handle(at, event, &mut out);
                next.next();
            }
            out.extend(controls.poll(now));
            for input in out.drain(..) {
                if let Input::Action(action) = input {
                    steps.push((now, action));
                }
            }
        }
        steps
    }

    #[test]
    fn diagonal_stick_noise_steps_once_until_the_repeat_delay() {
        let mut events = vec![(0, stick(StickAxis::X, 0.7)), (1, stick(StickAxis::Y, 0.7))];
        for t in 2..340 {
            let wobble = if t % 2 == 0 { 0.03 } else { -0.03 };
            let axis_ = if t % 4 < 2 { StickAxis::X } else { StickAxis::Y };
            events.push((t, stick(axis_, 0.7 + wobble)));
        }
        let steps = run(&events, 349);
        assert_eq!(steps, [(0, Action::Right)]);
        assert_eq!(run(&events, 350).last(), Some(&(350, Action::Right)));
    }

    #[test]
    fn stick_hovering_near_half_deflection_steps_once() {
        let events: Vec<_> = (0..300)
            .map(|t| (t, stick(StickAxis::X, if t % 2 == 0 { 0.52 } else { 0.46 })))
            .collect();
        assert_eq!(run(&events, 340), [(0, Action::Right)]);
    }

    #[test]
    fn stick_releases_below_0_4_not_at_0_45() {
        let held = [(0, stick(StickAxis::X, 0.8)), (100, stick(StickAxis::X, 0.45))];
        assert_eq!(run(&held, 350), [(0, Action::Right), (350, Action::Right)]);

        let released = [(0, stick(StickAxis::X, 0.8)), (100, stick(StickAxis::X, 0.39))];
        assert_eq!(run(&released, 2000), [(0, Action::Right)]);
    }

    #[test]
    fn stick_switches_when_the_other_axis_leads_by_the_margin() {
        let events = [
            (0, stick(StickAxis::X, 0.6)),
            (10, stick(StickAxis::Y, 0.75)),
            (20, stick(StickAxis::Y, 0.85)),
        ];
        assert_eq!(run(&events, 369), [(0, Action::Right), (20, Action::Down)]);
    }

    #[test]
    fn stick_flicked_across_releases_then_presses_the_other_way() {
        let events = [(0, stick(StickAxis::X, 0.8)), (100, stick(StickAxis::X, -0.8))];
        assert_eq!(run(&events, 449), [(0, Action::Right), (100, Action::Left)]);
    }

    #[test]
    fn in_a_text_field_qefs_and_space_type_text_and_act_on_nothing() {
        let mut controls = Controls::new(InputKind::Controller);
        controls.text = true;
        let mut out = Vec::new();
        for key in [Keycode::Q, Keycode::E, Keycode::F, Keycode::S, Keycode::Space] {
            controls.handle(0, key_down(key), &mut out);
            controls.handle(1, key_up(key), &mut out);
        }
        assert_eq!(out, [], "no tab switch, Options, Sort or Confirm");
        assert_eq!(controls.kind, InputKind::Keyboard);
        controls.typed("qefs ", &mut out);
        let typed: String = out
            .iter()
            .map(|i| match i {
                Input::Text(TextInput::Char(c)) => *c,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(typed, "qefs ");

        out.clear();
        controls.handle(2, key_down(Keycode::Backspace), &mut out);
        let repeat = ControlEvent::KeyDown {
            key: Keycode::Backspace,
            repeat: true,
            ctrl: false,
        };
        controls.handle(3, repeat, &mut out);
        controls.handle(4, key_down(Keycode::Return), &mut out);
        let erase = Input::Text(TextInput::Backspace);
        assert_eq!(
            out,
            [erase, erase, Input::Action(Action::Start)],
            "held Backspace repeats"
        );

        controls.text = false;
        out.clear();
        controls.typed("q", &mut out);
        controls.handle(5, key_down(Keycode::Q), &mut out);
        assert_eq!(
            out,
            [Input::Action(Action::PrevTab)],
            "outside a text field keys act again"
        );
    }

    fn ctrl_down(key: Keycode) -> ControlEvent {
        ControlEvent::KeyDown {
            key,
            repeat: false,
            ctrl: true,
        }
    }

    fn start(controls: &mut Controls) -> Vec<Input> {
        let mut out = Vec::new();
        let (pad, button) = (PAD, Button::Start);
        controls.handle(0, ControlEvent::PadButtonDown { pad, button }, &mut out);
        controls.handle(1, ControlEvent::PadButtonUp { pad, button }, &mut out);
        out
    }

    #[test]
    fn ctrl_f_searches_ctrl_comma_opens_settings_and_plain_f_is_options() {
        let mut controls = Controls::new(InputKind::Controller);
        let mut out = Vec::new();
        controls.handle(0, ctrl_down(Keycode::F), &mut out);
        assert_eq!(controls.kind, InputKind::Keyboard);
        controls.handle(1, key_down(Keycode::F), &mut out);
        controls.handle(2, ctrl_down(Keycode::Comma), &mut out);
        controls.handle(3, ctrl_down(Keycode::Q), &mut out);
        assert_eq!(
            out,
            [Action::Search, Action::Options, Action::Settings, Action::PrevTab].map(Input::Action)
        );
    }

    #[test]
    fn the_style_is_read_per_press() {
        let mut controls = Controls::new(InputKind::Controller);
        let mut out = Vec::new();
        let press = |controls: &mut Controls, button, out: &mut Vec<Input>| {
            controls.handle(0, ControlEvent::PadButtonDown { pad: PAD, button }, out);
            controls.handle(1, ControlEvent::PadButtonUp { pad: PAD, button }, out);
        };
        press(&mut controls, Button::East, &mut out);
        controls.style = ButtonStyle::PlayStation;
        press(&mut controls, Button::East, &mut out);
        press(&mut controls, Button::South, &mut out);
        assert_eq!(
            out,
            [Action::Confirm, Action::Back, Action::Confirm].map(Input::Action)
        );
    }

    #[test]
    fn start_opens_settings_outside_a_text_field_and_finishes_one_inside() {
        let mut controls = Controls::new(InputKind::Keyboard);
        assert_eq!(start(&mut controls), [Input::Action(Action::Settings)]);
        assert_eq!(controls.kind, InputKind::Controller);
        controls.text = true;
        assert_eq!(start(&mut controls), [Input::Action(Action::Start)]);
    }

    #[test]
    fn in_a_text_field_ctrl_f_and_ctrl_comma_type_nothing() {
        let mut controls = Controls::new(InputKind::Keyboard);
        controls.text = true;
        let mut out = Vec::new();
        controls.handle(0, ctrl_down(Keycode::F), &mut out);
        controls.typed("f", &mut out);
        controls.handle(1, key_up(Keycode::F), &mut out);
        controls.handle(2, ctrl_down(Keycode::Comma), &mut out);
        controls.typed(",", &mut out);
        assert_eq!(
            out,
            [Input::Action(Action::Search), Input::Action(Action::Settings)],
            "no f or , typed"
        );
        out.clear();
        controls.handle(3, key_down(Keycode::F), &mut out);
        controls.typed("f", &mut out);
        assert_eq!(
            out,
            [Input::Text(TextInput::Char('f'))],
            "without Ctrl, F types again"
        );
    }

    #[test]
    fn ctrl_v_pastes_in_a_text_field_and_types_nothing_outside_one() {
        let mut controls = Controls::new(InputKind::Keyboard);
        let mut out = Vec::new();
        controls.handle(0, ctrl_down(Keycode::V), &mut out);
        controls.typed("v", &mut out);
        assert_eq!(out, [], "outside a text field Ctrl+V does nothing");
        controls.text = true;
        controls.handle(1, ctrl_down(Keycode::V), &mut out);
        controls.typed("v", &mut out);
        assert_eq!(
            out,
            [Input::Action(Action::Paste)],
            "Ctrl+V pastes and types no v"
        );
    }

    #[test]
    fn releasing_a_tapped_direction_resumes_the_held_one_after_the_delay() {
        let events = [
            (0, key_down(Keycode::Right)),
            (100, key_down(Keycode::Up)),
            (200, key_up(Keycode::Up)),
        ];
        assert_eq!(
            run(&events, 550),
            [(0, Action::Right), (100, Action::Up), (550, Action::Right)]
        );
    }

    #[test]
    fn same_direction_on_keyboard_and_pad_keeps_repeating_while_one_is_held() {
        let pad_down = ControlEvent::PadButtonDown {
            pad: PAD,
            button: Button::DPadRight,
        };
        let pad_up = ControlEvent::PadButtonUp {
            pad: PAD,
            button: Button::DPadRight,
        };
        let events = [(0, key_down(Keycode::Right)), (10, pad_down), (200, pad_up)];
        let steps = run(&events, 470);
        assert_eq!(
            steps,
            [
                (0, Action::Right),
                (10, Action::Right),
                (360, Action::Right),
                (470, Action::Right)
            ]
        );

        let events = [
            (0, key_down(Keycode::Right)),
            (10, pad_down),
            (200, key_up(Keycode::Right)),
        ];
        assert_eq!(run(&events, 360).last(), Some(&(360, Action::Right)));
    }

    #[test]
    fn unplugging_a_pad_leaves_a_keyboard_repeat_alone() {
        let events = [
            (0, key_down(Keycode::Right)),
            (5, stick(StickAxis::X, 0.0)),
            (100, ControlEvent::PadRemoved { pad: PAD }),
        ];
        assert_eq!(run(&events, 350), [(0, Action::Right), (350, Action::Right)]);
    }

    #[test]
    fn unplugging_a_pad_releases_only_its_directions() {
        let other_down = ControlEvent::PadButtonDown {
            pad: OTHER_PAD,
            button: Button::DPadLeft,
        };
        let events = [
            (0, key_down(Keycode::Right)),
            (50, other_down),
            (60, stick(StickAxis::Y, 0.9)),
            (100, ControlEvent::PadRemoved { pad: PAD }),
        ];
        assert_eq!(
            run(&events, 450),
            [
                (0, Action::Right),
                (50, Action::Left),
                (60, Action::Down),
                (450, Action::Left)
            ]
        );
    }

    #[test]
    fn focus_loss_clears_every_held_direction() {
        let events = [
            (0, key_down(Keycode::Right)),
            (10, stick(StickAxis::Y, 0.9)),
            (
                20,
                ControlEvent::PadButtonDown {
                    pad: OTHER_PAD,
                    button: Button::DPadUp,
                },
            ),
            (100, ControlEvent::FocusLost),
            (200, key_up(Keycode::Right)),
            (300, stick(StickAxis::Y, 0.0)),
        ];
        assert_eq!(
            run(&events, 3000),
            [(0, Action::Right), (10, Action::Down), (20, Action::Up)]
        );
    }

    #[test]
    fn the_os_key_repeat_makes_no_step() {
        let events = [
            (0, key_down(Keycode::Right)),
            (
                30,
                ControlEvent::KeyDown {
                    key: Keycode::Right,
                    repeat: true,
                    ctrl: false,
                },
            ),
            (
                60,
                ControlEvent::KeyDown {
                    key: Keycode::Right,
                    repeat: true,
                    ctrl: false,
                },
            ),
        ];
        assert_eq!(run(&events, 349), [(0, Action::Right)]);
        assert_eq!(run(&events, 350).last(), Some(&(350, Action::Right)));
    }

    #[test]
    fn input_kind_follows_the_last_device() {
        let mut controls = Controls::new(InputKind::Keyboard);
        let mut out = Vec::new();
        controls.handle(0, stick(StickAxis::X, 0.9), &mut out);
        assert_eq!(controls.kind, InputKind::Controller);
        controls.handle(1, key_down(Keycode::Return), &mut out);
        assert_eq!(controls.kind, InputKind::Keyboard);
        assert_eq!(
            out,
            [Input::Action(Action::Right), Input::Action(Action::Confirm)]
        );
    }

    #[test]
    fn raw_controller_swallows_pad_input_while_keyboard_still_acts() {
        let mut controls = Controls::new(InputKind::Keyboard);
        controls.raw = true;
        let mut out = Vec::new();
        let (pad, button) = (PAD, Button::South);
        controls.handle(0, ControlEvent::PadButtonDown { pad, button }, &mut out);
        controls.handle(10, ControlEvent::PadButtonUp { pad, button }, &mut out);
        controls.handle(20, stick(StickAxis::X, 0.9), &mut out);
        assert_eq!(out, [], "no pad input while raw");

        controls.handle(30, key_down(Keycode::Escape), &mut out);
        assert_eq!(out, [Input::Action(Action::Back)], "Esc still acts");
        out.clear();
        controls.handle(40, key_down(Keycode::Right), &mut out);
        assert_eq!(out, [Input::Action(Action::Right)], "arrows still act");
    }

    #[test]
    fn a_direction_held_when_raw_turns_on_does_not_repeat() {
        let mut controls = Controls::new(InputKind::Controller);
        let mut out = Vec::new();
        controls.handle(
            0,
            ControlEvent::PadButtonDown {
                pad: PAD,
                button: Button::DPadRight,
            },
            &mut out,
        );
        assert_eq!(out, [Input::Action(Action::Right)]);
        controls.raw = true;
        out.clear();
        for now in 1..400 {
            out.extend(controls.poll(now));
        }
        assert_eq!(out, [], "the held direction never repeats once raw is on");
    }

    #[test]
    fn b_pressed_raw_and_released_after_raw_goes_off_fires_nothing_below() {
        let mut controls = Controls::new(InputKind::Controller);
        controls.raw = true;
        let mut out = Vec::new();
        let (pad, button) = (PAD, Button::South);
        controls.handle(0, ControlEvent::PadButtonDown { pad, button }, &mut out);
        controls.raw = false;
        controls.handle(10, ControlEvent::PadButtonUp { pad, button }, &mut out);
        assert_eq!(out, [], "no Back leaks to the screen below");
    }

    #[test]
    fn a_raw_pad_press_still_sets_the_input_kind_to_controller() {
        let mut controls = Controls::new(InputKind::Keyboard);
        controls.raw = true;
        let mut out = Vec::new();
        controls.handle(
            0,
            ControlEvent::PadButtonDown {
                pad: PAD,
                button: Button::South,
            },
            &mut out,
        );
        assert_eq!(controls.kind, InputKind::Controller);
        assert_eq!(out, []);
    }
}
