//! What reaches the Core each frame: pads assigned to ports in connection order (FR-EMB-8), pad
//! buttons mapped by position (FR-EMB-10), the fixed keyboard layout (FR-EMB-9), and the pause
//! trigger (FR-EMB-3).

use sdl3::gamepad::Button as Pad;
use sdl3::keyboard::Scancode;

use crate::libretro::{Button, InputState, MAX_PORTS};

/// Which pad plays on which port. A pad takes the lowest free port when it connects and gives it
/// up when it disconnects; the other pads keep theirs.
#[derive(Debug, Clone, PartialEq)]
pub struct Ports<Id> {
    slots: [Option<Id>; MAX_PORTS],
}

impl<Id: Copy + PartialEq> Default for Ports<Id> {
    fn default() -> Self {
        Ports {
            slots: [None; MAX_PORTS],
        }
    }
}

impl<Id: Copy + PartialEq> Ports<Id> {
    /// Gives `pad` the lowest free port; `None` when all are taken.
    pub fn connect(&mut self, pad: Id) -> Option<usize> {
        if let Some(port) = self.port_of(pad) {
            return Some(port);
        }
        let port = self.slots.iter().position(Option::is_none)?;
        self.slots[port] = Some(pad);
        Some(port)
    }

    pub fn disconnect(&mut self, pad: Id) {
        for slot in &mut self.slots {
            if *slot == Some(pad) {
                *slot = None;
            }
        }
    }

    pub fn port_of(&self, pad: Id) -> Option<usize> {
        self.slots.iter().position(|s| *s == Some(pad))
    }

    /// The pads with a port, and their ports.
    pub fn assigned(&self) -> impl Iterator<Item = (usize, Id)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| Some((i, (*s)?)))
    }
}

/// One pad's state this frame, read from SDL.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PadState {
    /// One bit per pressed SDL gamepad button, at its `SDL_GamepadButton` value.
    pub buttons: u32,
    pub left_x: i16,
    pub left_y: i16,
    pub left_trigger: i16,
    pub right_trigger: i16,
}

impl PadState {
    pub fn press(&mut self, button: Pad) {
        self.buttons |= 1 << button as u32;
    }

    pub fn is_pressed(&self, button: Pad) -> bool {
        self.buttons & 1 << button as u32 != 0
    }
}

/// Half an axis' travel: sticks and triggers past it count as pressed.
const HALF: i16 = i16::MAX / 2;

/// Pad buttons by position on the original controller: the bottom face button is B, the right
/// one A, and so on, whatever the pad prints on them.
const BY_POSITION: [(Pad, Button); 14] = [
    (Pad::South, Button::B),
    (Pad::East, Button::A),
    (Pad::West, Button::Y),
    (Pad::North, Button::X),
    (Pad::Back, Button::Select),
    (Pad::Start, Button::Start),
    (Pad::LeftShoulder, Button::L),
    (Pad::RightShoulder, Button::R),
    (Pad::LeftStick, Button::L3),
    (Pad::RightStick, Button::R3),
    (Pad::DPadUp, Button::Up),
    (Pad::DPadDown, Button::Down),
    (Pad::DPadLeft, Button::Left),
    (Pad::DPadRight, Button::Right),
];

/// The RetroPad buttons a pad holds.
pub fn pad_buttons(pad: &PadState) -> InputState {
    let mut input = InputState::default();
    for (from, to) in BY_POSITION {
        if pad.is_pressed(from) {
            input.set(0, to, true);
        }
    }
    let analog = [
        (pad.left_trigger > HALF, Button::L2),
        (pad.right_trigger > HALF, Button::R2),
        (pad.left_y < -HALF, Button::Up),
        (pad.left_y > HALF, Button::Down),
        (pad.left_x < -HALF, Button::Left),
        (pad.left_x > HALF, Button::Right),
    ];
    for (held, button) in analog {
        if held {
            input.set(0, button, true);
        }
    }
    input
}

/// RetroArch's default keyboard layout for player 1.
const KEYBOARD: [(Scancode, Button); 12] = [
    (Scancode::Up, Button::Up),
    (Scancode::Down, Button::Down),
    (Scancode::Left, Button::Left),
    (Scancode::Right, Button::Right),
    (Scancode::X, Button::A),
    (Scancode::Z, Button::B),
    (Scancode::S, Button::X),
    (Scancode::A, Button::Y),
    (Scancode::Q, Button::L),
    (Scancode::W, Button::R),
    (Scancode::Return, Button::Start),
    (Scancode::RShift, Button::Select),
];

/// The RetroPad buttons the keyboard holds, given which keys are down.
pub fn keyboard_buttons(down: impl Fn(Scancode) -> bool) -> InputState {
    let mut input = InputState::default();
    for (key, button) in KEYBOARD {
        if down(key) {
            input.set(0, button, true);
        }
    }
    input
}

/// Watches for the pause trigger: Guide on any pad, Select+Start together on one pad, or Esc.
/// It fires when the trigger starts, not while it is held, and not for a trigger already held
/// when the game started.
#[derive(Debug, Default)]
pub struct PauseTrigger {
    /// Whether the trigger was held last frame; `None` before the first frame.
    held: Option<bool>,
}

impl PauseTrigger {
    fn update(&mut self, held: bool) -> bool {
        let fired = self.held == Some(false) && held;
        self.held = Some(held);
        fired
    }
}

fn is_chord(pad: &PadState) -> bool {
    pad.is_pressed(Pad::Back) && pad.is_pressed(Pad::Start)
}

/// This frame's Core input from the pads on their ports and the keyboard (on port 1), and whether
/// the pause trigger fired. Select+Start held together on a pad never reaches the Core.
pub fn frame_input(
    pads: &[(usize, PadState)],
    keyboard: InputState,
    esc: bool,
    pause: &mut PauseTrigger,
) -> (InputState, bool) {
    let mut input = InputState::default();
    for (port, pad) in pads {
        let mut buttons = pad_buttons(pad);
        if is_chord(pad) {
            buttons.set(0, Button::Select, false);
            buttons.set(0, Button::Start, false);
        }
        merge(&mut input, *port, &buttons);
    }
    merge(&mut input, 0, &keyboard);
    let held = esc || pads.iter().any(|(_, p)| p.is_pressed(Pad::Guide) || is_chord(p));
    (input, pause.update(held))
}

/// ORs port 0 of `from` into `port` of `into`.
fn merge(into: &mut InputState, port: usize, from: &InputState) {
    for b in 0..16u8 {
        if from.mask(0) & 1 << b != 0 {
            into.set(port, BUTTONS[b as usize], true);
        }
    }
}

const BUTTONS: [Button; 16] = [
    Button::B,
    Button::Y,
    Button::Select,
    Button::Start,
    Button::Up,
    Button::Down,
    Button::Left,
    Button::Right,
    Button::A,
    Button::X,
    Button::L,
    Button::R,
    Button::L2,
    Button::R2,
    Button::L3,
    Button::R3,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_take_the_lowest_free_port_and_keep_it() {
        let mut ports = Ports::default();
        assert_eq!(ports.connect(10), Some(0));
        assert_eq!(ports.connect(11), Some(1));
        assert_eq!(ports.connect(12), Some(2));
        ports.disconnect(11);
        assert_eq!(
            (ports.port_of(10), ports.port_of(12)),
            (Some(0), Some(2)),
            "the others keep theirs"
        );
        assert_eq!(ports.connect(13), Some(1), "a new pad fills the gap");
        assert_eq!(ports.connect(11), Some(3), "the old one comes back last");
        assert_eq!(ports.connect(14), None, "four ports at most");
        assert_eq!(ports.connect(10), Some(0), "connecting twice changes nothing");
        let assigned: Vec<_> = ports.assigned().collect();
        assert_eq!(assigned, [(0, 10), (1, 13), (2, 12), (3, 11)]);
    }

    fn pad(buttons: &[Pad]) -> PadState {
        let mut p = PadState::default();
        for &b in buttons {
            p.press(b);
        }
        p
    }

    #[test]
    fn each_pad_button_maps_by_position() {
        for (from, to) in BY_POSITION {
            let got = pad_buttons(&pad(&[from]));
            let mut want = InputState::default();
            want.set(0, to, true);
            assert_eq!(got, want, "{from:?}");
        }
        assert_eq!(pad_buttons(&pad(&[Pad::South])).mask(0), 1 << Button::B as u16);
        assert_eq!(
            pad_buttons(&pad(&[Pad::Guide])),
            InputState::default(),
            "Guide is Game37's"
        );
    }

    #[test]
    fn triggers_and_the_left_stick_count_past_half_way() {
        let only = |b: Button| {
            let mut s = InputState::default();
            s.set(0, b, true);
            s
        };
        let cases = [
            (
                PadState {
                    left_trigger: 20000,
                    ..Default::default()
                },
                only(Button::L2),
            ),
            (
                PadState {
                    right_trigger: 20000,
                    ..Default::default()
                },
                only(Button::R2),
            ),
            (
                PadState {
                    left_y: -20000,
                    ..Default::default()
                },
                only(Button::Up),
            ),
            (
                PadState {
                    left_y: 20000,
                    ..Default::default()
                },
                only(Button::Down),
            ),
            (
                PadState {
                    left_x: -20000,
                    ..Default::default()
                },
                only(Button::Left),
            ),
            (
                PadState {
                    left_x: 20000,
                    ..Default::default()
                },
                only(Button::Right),
            ),
        ];
        for (state, want) in cases {
            assert_eq!(pad_buttons(&state), want, "{state:?}");
        }
        let light = PadState {
            left_trigger: 10000,
            left_x: 10000,
            left_y: -10000,
            ..Default::default()
        };
        assert_eq!(pad_buttons(&light), InputState::default());
    }

    #[test]
    fn the_keyboard_uses_retroarchs_defaults() {
        for (key, button) in KEYBOARD {
            let mut want = InputState::default();
            want.set(0, button, true);
            assert_eq!(keyboard_buttons(|k| k == key), want, "{key:?}");
        }
        assert!(keyboard_buttons(|k| k == Scancode::X).is_pressed(0, Button::A));
        assert!(keyboard_buttons(|k| k == Scancode::Z).is_pressed(0, Button::B));
        assert!(keyboard_buttons(|k| k == Scancode::RShift).is_pressed(0, Button::Select));
        assert_eq!(keyboard_buttons(|k| k == Scancode::Escape), InputState::default());
    }

    #[test]
    fn pads_play_on_their_ports_and_the_keyboard_joins_port_1() {
        let mut pause = PauseTrigger::default();
        let pads = [(0, pad(&[Pad::South])), (1, pad(&[Pad::East]))];
        let keys = keyboard_buttons(|k| k == Scancode::X);
        let (input, paused) = frame_input(&pads, keys, false, &mut pause);
        assert!(!paused);
        assert!(input.is_pressed(0, Button::B) && input.is_pressed(0, Button::A));
        assert_eq!(input.mask(1), 1 << Button::A as u16);
    }

    #[test]
    fn guide_esc_and_select_start_pause_once_and_select_start_never_reaches_the_core() {
        let none: [(usize, PadState); 0] = [];
        let chord = [(1, pad(&[Pad::Back, Pad::Start, Pad::South]))];
        let mut pause = PauseTrigger::default();
        assert!(!frame_input(&none, InputState::default(), false, &mut pause).1);
        let (input, paused) = frame_input(&chord, InputState::default(), false, &mut pause);
        assert!(paused, "Select+Start");
        assert_eq!(
            input.mask(1),
            1 << Button::B as u16,
            "Select and Start are held back"
        );
        assert!(
            !frame_input(&chord, InputState::default(), false, &mut pause).1,
            "fires once"
        );
        assert!(!frame_input(&none, InputState::default(), false, &mut pause).1);

        assert!(
            frame_input(&none, InputState::default(), true, &mut pause).1,
            "Esc"
        );
        assert!(!frame_input(&none, InputState::default(), false, &mut pause).1);
        let guide = [(3, pad(&[Pad::Guide]))];
        assert!(
            frame_input(&guide, InputState::default(), false, &mut pause).1,
            "Guide"
        );

        let split = [(0, pad(&[Pad::Back])), (1, pad(&[Pad::Start]))];
        let mut pause = PauseTrigger::default();
        frame_input(&none, InputState::default(), false, &mut pause);
        let (input, paused) = frame_input(&split, InputState::default(), false, &mut pause);
        assert!(!paused, "Select and Start on different pads");
        assert!(input.is_pressed(0, Button::Select) && input.is_pressed(1, Button::Start));
    }

    #[test]
    fn a_trigger_held_when_the_game_starts_does_not_pause_it() {
        let mut pause = PauseTrigger::default();
        assert!(!frame_input(&[], InputState::default(), true, &mut pause).1);
        assert!(!frame_input(&[], InputState::default(), false, &mut pause).1);
        assert!(frame_input(&[], InputState::default(), true, &mut pause).1);
    }
}
