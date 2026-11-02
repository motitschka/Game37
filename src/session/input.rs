//! What reaches the Core each frame: pads assigned to ports in connection order (FR-EMB-8), pad
//! buttons mapped by position, or by letter for a Nintendo-lettered pad on a PlayStation System
//! (FR-EMB-10), the fixed keyboard layout (FR-EMB-9), and the pause trigger (FR-EMB-3).

use sdl3::gamepad::Button as Pad;
use sdl3::keyboard::Scancode;

use crate::cores::{Swap, TouchScreen};
use crate::libretro::{Button, InputState, MAX_PORTS, Pointer, Stick};
use crate::systems::{Maker, System};
use crate::ui::Rect;

/// Which pad plays on which port. A pad takes the lowest free port when it connects and gives it
/// up when it disconnects; the other pads keep theirs. A pad that connects while every port is
/// taken waits, and the longest-waiting one takes the next port freed.
#[derive(Debug, Clone, PartialEq)]
pub struct Ports<Id> {
    slots: [Option<Id>; MAX_PORTS],
    /// Connected pads without a port, longest-connected first.
    waiting: Vec<Id>,
}

impl<Id: Copy + PartialEq> Default for Ports<Id> {
    fn default() -> Self {
        Ports {
            slots: [None; MAX_PORTS],
            waiting: Vec::new(),
        }
    }
}

impl<Id: Copy + PartialEq> Ports<Id> {
    /// Gives `pad` the lowest free port; `None` when all are taken.
    pub fn connect(&mut self, pad: Id) -> Option<usize> {
        if let Some(port) = self.port_of(pad) {
            return Some(port);
        }
        let Some(port) = self.slots.iter().position(Option::is_none) else {
            if !self.waiting.contains(&pad) {
                self.waiting.push(pad);
            }
            return None;
        };
        self.slots[port] = Some(pad);
        Some(port)
    }

    /// Frees `pad`'s port, which the longest-waiting pad takes.
    pub fn disconnect(&mut self, pad: Id) {
        self.waiting.retain(|w| *w != pad);
        for slot in &mut self.slots {
            if *slot == Some(pad) {
                *slot = None;
                if !self.waiting.is_empty() {
                    *slot = Some(self.waiting.remove(0));
                }
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

/// The letters a pad's face buttons carry, as SDL reports them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FaceLetters {
    /// Xbox letters (A at the bottom), PlayStation symbols, or unknown.
    #[default]
    Other,
    /// Nintendo letters: A on the right, B at the bottom, X on top, Y on the left (Switch Pro
    /// Controller, Joy-Con pair).
    Nintendo,
}

/// One pad's state this frame, read from SDL.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PadState {
    /// One bit per pressed SDL gamepad button, at its `SDL_GamepadButton` value.
    pub buttons: u32,
    /// Which letters its face buttons carry.
    pub letters: FaceLetters,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
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

/// Every button `PadState` reads from SDL: the positional ones (see [`BY_POSITION`]) plus Guide,
/// which only the pause trigger reads. The thin SDL adapter in `session::sdl` presses these from
/// an open `Gamepad` each frame.
pub const ALL_BUTTONS: [Pad; 15] = [
    Pad::South,
    Pad::East,
    Pad::West,
    Pad::North,
    Pad::Back,
    Pad::Start,
    Pad::LeftShoulder,
    Pad::RightShoulder,
    Pad::LeftStick,
    Pad::RightStick,
    Pad::DPadUp,
    Pad::DPadDown,
    Pad::DPadLeft,
    Pad::DPadRight,
    Pad::Guide,
];

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

/// A Nintendo-lettered pad's face buttons by their letters, on a PlayStation System: A (right) is
/// ✕, B (bottom) ○, X (top) □ and Y (left) △, which PlayStation Cores read as RetroPad B, A, Y and
/// X. The letters land where an Xbox pad's are, so its A confirms in PlayStation Games as theirs does.
const NINTENDO_BY_LETTER: [(Pad, Button); 4] = [
    (Pad::East, Button::B),
    (Pad::South, Button::A),
    (Pad::North, Button::Y),
    (Pad::West, Button::X),
];

/// Whether a System's Games read a Nintendo-lettered pad's face buttons by their letters
/// ([`NINTENDO_BY_LETTER`]) rather than by position: the PlayStation Systems. Every other pad, and
/// every pad on the other Systems, maps by position.
///
/// This also covers PS3 and PS4 by `maker`, though neither has a Core in `cores::for_system` and
/// so never reaches this function in Game37; that is fine, not a gap to close.
pub fn faces_by_letter(system: &System) -> bool {
    system.maker == Maker::Sony
}

/// The RetroPad buttons a pad holds, the left stick counting as the d-pad, every button by position.
pub fn pad_buttons(pad: &PadState) -> InputState {
    pad_buttons_with(pad, true, false)
}

/// The RetroPad buttons a pad holds. With `stick_dpad`, the left stick past half way also presses
/// the d-pad, for Systems without an analog stick; Cores of Systems with one (N64, PSP) read the
/// stick as analog only, so it doesn't press the d-pad as well. With `by_letter`
/// ([`faces_by_letter`]), a Nintendo-lettered pad's face buttons map by their letters.
pub fn pad_buttons_with(pad: &PadState, stick_dpad: bool, by_letter: bool) -> InputState {
    let letters = by_letter && pad.letters == FaceLetters::Nintendo;
    let mut input = InputState::default();
    for (from, by_position) in BY_POSITION {
        let to = match NINTENDO_BY_LETTER.iter().find(|(face, _)| *face == from) {
            Some(&(_, by_letter)) if letters => by_letter,
            _ => by_position,
        };
        if pad.is_pressed(from) {
            input.set(0, to, true);
        }
    }
    let analog = [
        (pad.left_trigger > HALF, Button::L2),
        (pad.right_trigger > HALF, Button::R2),
        (stick_dpad && pad.left_y < -HALF, Button::Up),
        (stick_dpad && pad.left_y > HALF, Button::Down),
        (stick_dpad && pad.left_x < -HALF, Button::Left),
        (stick_dpad && pad.left_x > HALF, Button::Right),
    ];
    for (held, button) in analog {
        if held {
            input.set(0, button, true);
        }
    }
    input
}

/// A stick axis as the Core reads it: SDL's -32768 to 32767 clamped to libretro's -0x7fff to
/// 0x7fff, with no deadzone (the Core keeps its own, as melonDS DS does for its cursor).
fn axis(value: i16) -> i16 {
    value.max(-i16::MAX)
}

/// Swaps the DS and 3DS screens from the keyboard, as R3 does on a pad: Tab, which none of
/// RetroArch's player 1 keys in [`KEYBOARD`] use.
pub const SWAP_SCREENS_KEY: Scancode = Scancode::Tab;

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

/// The RetroPad buttons the keyboard holds, given which keys are down. [`SWAP_SCREENS_KEY`] holds
/// none: [`TouchControls`] reads it.
pub fn keyboard_buttons(down: impl Fn(Scancode) -> bool) -> InputState {
    let mut input = InputState::default();
    for (key, button) in KEYBOARD {
        if down(key) {
            input.set(0, button, true);
        }
    }
    input
}

/// Game37's controls for a touch screen (DS, 3DS; FR-DS-2, FR-DS-3, FR-NEW-2), the same on both:
/// R3 on any pad, or Tab, swaps the screens, and L3 taps with the right-stick cursor, reaching the
/// Core as the button its cursor taps on, with the Core's other button moves ([`TouchScreen`]).
#[derive(Debug, Clone, Copy)]
pub struct TouchControls {
    touch: TouchScreen,
    /// Whether the swap control was held at the last frame.
    held: bool,
    /// For a Core that swaps through an option: whether it has `values[1]` now.
    swapped: bool,
}

impl TouchControls {
    pub fn new(touch: TouchScreen) -> TouchControls {
        TouchControls {
            touch,
            held: false,
            swapped: false,
        }
    }

    /// Marks the swap control as held over a pause: one still down when play resumes must be let
    /// go before it swaps again, as `HeldOver` does for the rest of the pad.
    pub fn resume(&mut self) {
        self.held = true;
    }

    /// Takes the touch controls out of a frame's `input` and `tab`, whether Tab is down, and puts
    /// in what the Core takes for them. On each pad, every button of the touch screen's `moves` is
    /// let go and the button it moves to held instead while it is; R3 is let go, and with
    /// [`Swap::Button`] that button held while the swap control is. With [`Swap::Option`], returns
    /// the option and its new value once per press of the swap control, however long it is held,
    /// for the Core to change.
    pub fn apply(&mut self, input: &mut InputState, tab: bool) -> Option<(&'static str, &'static str)> {
        let mut held = false;
        let pad = *input;
        for port in 0..MAX_PORTS {
            // Tab is port 0's, as the rest of the keyboard is.
            let swap = pad.is_pressed(port, Button::R3) || (port == 0 && tab);
            held |= swap;
            input.set(port, Button::R3, false);
            for &(from, _) in self.touch.moves {
                input.set(port, from, false);
            }
            for &(from, to) in self.touch.moves {
                if pad.is_pressed(port, from) {
                    input.set(port, to, true);
                }
            }
            if let Swap::Button(button) = self.touch.swap
                && swap
            {
                input.set(port, button, true);
            }
        }
        let pressed = held && !self.held;
        self.held = held;
        match self.touch.swap {
            Swap::Option { key, values } if pressed => {
                self.swapped = !self.swapped;
                Some((key, values[usize::from(self.swapped)]))
            }
            _ => None,
        }
    }
}

/// The left stick the arrow keys drive, for Systems whose Core reads the stick as analog rather
/// than the d-pad (`!stick_dpad`, see [`pad_buttons_with`]): N64 and PSP. Full deflection
/// (±0x7fff) while an arrow key is held, centred otherwise; `stick_dpad` always centres it, since
/// those Systems have no analog stick and [`keyboard_buttons`]'s own d-pad press is enough
/// (FR-EMB-9). The d-pad press from [`keyboard_buttons`] stays in effect either way, so menus
/// inside games still work.
pub fn keyboard_stick(down: impl Fn(Scancode) -> bool, stick_dpad: bool) -> (i16, i16) {
    if stick_dpad {
        return (0, 0);
    }
    let axis = |negative: bool, positive: bool| match (negative, positive) {
        (true, false) => -0x7fff,
        (false, true) => 0x7fff,
        _ => 0,
    };
    (
        axis(down(Scancode::Left), down(Scancode::Right)),
        axis(down(Scancode::Up), down(Scancode::Down)),
    )
}

/// The mouse this frame: where it is, in logical 1920×1080 coordinates, and whether its left
/// button is held.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Mouse {
    pub at: Option<(f32, f32)>,
    pub left: bool,
}

/// The pointer the Core reads for `mouse` over the Game's `picture` (where its frame is drawn):
/// the left button touches. `None` when the mouse is off the picture.
pub fn touch(picture: Rect, mouse: Mouse) -> Option<Pointer> {
    let (x, y) = mouse.at?;
    let (fx, fy) = ((x - picture.x) / picture.w, (y - picture.y) / picture.h);
    if !(0.0..=1.0).contains(&fx) || !(0.0..=1.0).contains(&fy) {
        return None;
    }
    let scale = |f: f32| ((f * 2.0 - 1.0) * f32::from(i16::MAX)).round() as i16;
    Some(Pointer {
        x: scale(fx),
        y: scale(fy),
        pressed: mouse.left,
    })
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

/// This frame's Core input from the pads on their ports (buttons and sticks) and the keyboard (on
/// port 1), and whether
/// the pause trigger fired. Select+Start held together on a pad never reaches the Core.
pub fn frame_input(
    pads: &[(usize, PadState)],
    keyboard: InputState,
    esc: bool,
    pause: &mut PauseTrigger,
) -> (InputState, bool) {
    frame_input_with(pads, keyboard, esc, pause, true, false)
}

/// [`frame_input`], with the left stick pressing the d-pad only if `stick_dpad`, and a
/// Nintendo-lettered pad's face buttons by letter if `by_letter` (see [`pad_buttons_with`]).
pub fn frame_input_with(
    pads: &[(usize, PadState)],
    keyboard: InputState,
    esc: bool,
    pause: &mut PauseTrigger,
    stick_dpad: bool,
    by_letter: bool,
) -> (InputState, bool) {
    let mut input = InputState::default();
    for (port, pad) in pads {
        let mut buttons = pad_buttons_with(pad, stick_dpad, by_letter);
        if is_chord(pad) {
            buttons.set(0, Button::Select, false);
            buttons.set(0, Button::Start, false);
        }
        merge(&mut input, *port, &buttons);
        input.set_stick(*port, Stick::Left, axis(pad.left_x), axis(pad.left_y));
        input.set_stick(*port, Stick::Right, axis(pad.right_x), axis(pad.right_y));
    }
    merge(&mut input, 0, &keyboard);
    let held = esc || pads.iter().any(|(_, p)| p.is_pressed(Pad::Guide) || is_chord(p));
    (input, pause.update(held))
}

/// Core buttons already held when play starts or resumes: the Enter that launched the Game or
/// chose Resume, the A that loaded a slot. The first frame after records what is held on each
/// port, and each of those stays masked from the Core until it is released, so a menu button
/// never reaches the Game.
#[derive(Debug, Default)]
pub struct HeldOver {
    /// Per port; `None` until the first frame records it.
    held: Option<[u16; MAX_PORTS]>,
}

impl HeldOver {
    /// Clears from `input` the buttons still held over, first forgetting those let go.
    pub fn mask(&mut self, input: &mut InputState) {
        let held = self
            .held
            .get_or_insert_with(|| std::array::from_fn(|port| input.mask(port)));
        for (port, held) in held.iter_mut().enumerate() {
            *held &= input.mask(port);
            for (b, button) in BUTTONS.iter().enumerate() {
                if *held & 1 << b != 0 {
                    input.set(port, *button, false);
                }
            }
        }
    }
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

    #[test]
    fn a_fifth_pad_waits_and_takes_the_next_freed_port() {
        let mut ports = Ports::default();
        for pad in 10..14 {
            ports.connect(pad);
        }
        assert_eq!(ports.connect(14), None, "four ports at most");
        assert_eq!(ports.connect(15), None);
        assert_eq!(ports.connect(14), None, "connecting twice changes nothing");
        ports.disconnect(12);
        assert_eq!(
            ports.port_of(14),
            Some(2),
            "the longest-waiting pad takes the freed port"
        );
        ports.disconnect(15);
        ports.disconnect(11);
        assert_eq!(ports.port_of(11), None);
        assert_eq!(
            ports.port_of(15),
            None,
            "a pad that left while waiting is forgotten"
        );
        assert_eq!(
            ports.connect(16),
            Some(1),
            "so the freed port stays free for the next"
        );
        let assigned: Vec<_> = ports.assigned().collect();
        assert_eq!(assigned, [(0, 10), (1, 16), (2, 14), (3, 13)]);
    }

    #[test]
    fn buttons_held_when_play_starts_reach_the_core_only_after_release() {
        let only = |port, b: Button| {
            let mut s = InputState::default();
            s.set(port, b, true);
            s
        };
        let mut held = HeldOver::default();
        let mut input = only(0, Button::Start);
        input.set(1, Button::A, true);
        held.mask(&mut input);
        assert_eq!(
            input,
            InputState::default(),
            "Enter on Resume and A on a slot are held back"
        );

        let mut input = only(0, Button::Start);
        input.set(0, Button::X, true);
        input.set(1, Button::A, true);
        held.mask(&mut input);
        assert_eq!(
            input,
            only(0, Button::X),
            "a new press goes through, the held ones don't"
        );

        let mut input = only(1, Button::A);
        held.mask(&mut input);
        assert_eq!(input, InputState::default(), "Start let go, A still held");
        let mut none = InputState::default();
        held.mask(&mut none);
        let mut input = only(0, Button::Start);
        input.set(1, Button::A, true);
        let want = input;
        held.mask(&mut input);
        assert_eq!(input, want, "pressed again after release, they reach the Core");
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

    /// Port 0's buttons when a pad with `letters` holds `held` on `system`, as a session there reads
    /// them.
    fn on(system: &str, letters: FaceLetters, held: &[Pad]) -> u16 {
        let system = crate::systems::by_id(system).unwrap();
        let pad = PadState { letters, ..pad(held) };
        let mut pause = PauseTrigger::default();
        let by_letter = faces_by_letter(system);
        let (input, _) = frame_input_with(
            &[(0, pad)],
            InputState::default(),
            false,
            &mut pause,
            true,
            by_letter,
        );
        input.mask(0)
    }

    fn bit(button: Button) -> u16 {
        1 << button as u16
    }

    #[test]
    fn a_nintendo_lettered_pad_maps_by_letter_on_playstation_systems() {
        // PlayStation Cores read RetroPad B as ✕, A as ○, Y as □ and X as △.
        let by_letter = [
            (Pad::East, Button::B, "A is ✕"),
            (Pad::South, Button::A, "B is ○"),
            (Pad::North, Button::Y, "X is □"),
            (Pad::West, Button::X, "Y is △"),
        ];
        for system in ["ps1", "ps2", "psp"] {
            for (face, want, what) in by_letter {
                assert_eq!(
                    on(system, FaceLetters::Nintendo, &[face]),
                    bit(want),
                    "{system}: {what}"
                );
            }
        }
    }

    #[test]
    fn a_nintendo_lettered_pad_maps_by_position_on_nintendo_systems() {
        for system in ["snes", "gc", "n64", "3ds"] {
            for (face, want) in &BY_POSITION[..4] {
                assert_eq!(
                    on(system, FaceLetters::Nintendo, &[*face]),
                    bit(*want),
                    "{system}: {face:?}"
                );
            }
        }
        assert_eq!(
            on("snes", FaceLetters::Nintendo, &[Pad::East]),
            bit(Button::A),
            "A is still the right button"
        );
    }

    #[test]
    fn xbox_and_playstation_pads_map_by_position_on_playstation_systems() {
        // SDL reports neither as Nintendo-lettered: their bottom button (A, ✕) is already ✕.
        for (face, want) in &BY_POSITION[..4] {
            assert_eq!(on("ps2", FaceLetters::Other, &[*face]), bit(*want), "{face:?}");
        }
        assert_eq!(
            on("ps2", FaceLetters::Other, &[Pad::South]),
            bit(Button::B),
            "A or ✕ is ✕"
        );
    }

    #[test]
    fn two_kinds_of_pad_on_two_ports_each_map_their_own_way() {
        let nintendo = PadState {
            letters: FaceLetters::Nintendo,
            ..pad(&[Pad::East])
        };
        let xbox = pad(&[Pad::East]);
        let mut pause = PauseTrigger::default();
        let ps2 = faces_by_letter(crate::systems::by_id("ps2").unwrap());
        let pads = [(0, nintendo), (1, xbox)];
        let (input, _) = frame_input_with(&pads, InputState::default(), false, &mut pause, true, ps2);
        assert_eq!(input.mask(0), bit(Button::B), "the Switch pad's A is ✕");
        assert_eq!(input.mask(1), bit(Button::A), "the Xbox pad's B is ○");
    }

    #[test]
    fn a_nintendo_lettered_pad_keeps_its_other_buttons_and_the_pause_chord_on_playstation() {
        for (from, want) in &BY_POSITION[4..] {
            assert_eq!(on("ps2", FaceLetters::Nintendo, &[*from]), bit(*want), "{from:?}");
        }
        let chord = PadState {
            letters: FaceLetters::Nintendo,
            ..pad(&[Pad::Back, Pad::Start])
        };
        let mut pause = PauseTrigger::default();
        frame_input_with(&[], InputState::default(), false, &mut pause, true, true);
        let (input, paused) = frame_input_with(
            &[(0, chord)],
            InputState::default(),
            false,
            &mut pause,
            true,
            true,
        );
        assert!(paused, "Select+Start still pauses");
        assert_eq!(input.mask(0), 0);
    }

    #[test]
    fn only_the_playstation_systems_read_faces_by_letter() {
        let by_letter: Vec<_> = crate::systems::SYSTEMS
            .iter()
            .filter(|s| faces_by_letter(s))
            .map(|s| s.id)
            .collect();
        assert_eq!(by_letter, ["ps1", "ps2", "psp", "ps3", "ps4"]);
    }

    #[test]
    fn an_analog_system_reads_the_left_stick_as_analog_only() {
        let pad = PadState {
            left_x: 20000,
            left_y: -20000,
            ..PadState::default()
        };
        assert_eq!(
            pad_buttons_with(&pad, false, false).mask(0),
            0,
            "no d-pad from the stick"
        );
        let mut pause = PauseTrigger::default();
        let (input, _) = frame_input_with(
            &[(0, pad)],
            InputState::default(),
            false,
            &mut pause,
            false,
            false,
        );
        assert_eq!(input.mask(0), 0);
        assert_eq!(
            input.stick(0, Stick::Left),
            (20000, -20000),
            "still read as analog"
        );
        let (input, _) = frame_input(&[(0, pad)], InputState::default(), false, &mut pause);
        assert!(input.is_pressed(0, Button::Right) && input.is_pressed(0, Button::Up));
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
    fn arrow_keys_drive_the_left_stick_at_full_deflection_unless_stick_dpad() {
        assert_eq!(keyboard_stick(|k| k == Scancode::Left, false), (-0x7fff, 0));
        assert_eq!(keyboard_stick(|k| k == Scancode::Down, false), (0, 0x7fff));
        assert_eq!(
            keyboard_stick(|k| k == Scancode::Left || k == Scancode::Right, false),
            (0, 0),
            "cancel out"
        );
        assert_eq!(keyboard_stick(|_| false, false), (0, 0));
        assert_eq!(
            keyboard_stick(|k| k == Scancode::Left, true),
            (0, 0),
            "stick_dpad: no analog from the keyboard"
        );

        // The keyboard's own d-pad press is unaffected, so menus inside games still work.
        let left = keyboard_buttons(|k| k == Scancode::Left);
        assert!(left.is_pressed(0, Button::Left));
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

    #[test]
    fn tab_is_the_swap_key_and_clashes_with_no_other_key() {
        assert_eq!(SWAP_SCREENS_KEY, Scancode::Tab);
        assert_eq!(
            keyboard_buttons(|k| k == SWAP_SCREENS_KEY),
            InputState::default(),
            "Tab holds no button of its own"
        );
        assert!(KEYBOARD.iter().all(|(key, _)| *key != SWAP_SCREENS_KEY));
        assert_ne!(SWAP_SCREENS_KEY, Scancode::Escape, "Esc pauses");
    }

    /// `buttons` held on `port`.
    fn holding(port: usize, buttons: &[Button]) -> InputState {
        let mut input = InputState::default();
        for &button in buttons {
            input.set(port, button, true);
        }
        input
    }

    /// The touch controls' frame: what reaches the Core and the option change, for `input`.
    fn touch_frame(
        controls: &mut TouchControls,
        mut input: InputState,
        tab: bool,
    ) -> (InputState, Option<(&'static str, &'static str)>) {
        let change = controls.apply(&mut input, tab);
        (input, change)
    }

    /// 3DS on Azahar (FR-NEW-2): a pad's L3 taps with the right-stick cursor, reaching Azahar as
    /// R3; R3 and Tab swap the screens through `citra_swap_screen`, once per press, and reach
    /// Azahar as neither R3 nor L3, which is its HOME button.
    #[test]
    fn on_3ds_l3_taps_as_r3_and_r3_or_tab_flip_the_swap_option_once_per_press() {
        let mut controls = TouchControls::new(crate::cores::AZAHAR.touch.unwrap());
        let swap = |value| Some(("citra_swap_screen", value));

        let (input, change) = touch_frame(&mut controls, holding(1, &[Button::L3, Button::A]), false);
        assert_eq!(input, holding(1, &[Button::R3, Button::A]), "L3 taps, as R3");
        assert_eq!(change, None);

        let (input, change) = touch_frame(&mut controls, holding(2, &[Button::R3, Button::B]), false);
        assert_eq!(input, holding(2, &[Button::B]), "R3 never reaches Azahar");
        assert_eq!(change, swap("Bottom"), "the press swaps");
        for _ in 0..3 {
            let (input, change) = touch_frame(&mut controls, holding(2, &[Button::R3]), false);
            assert_eq!(input, InputState::default());
            assert_eq!(change, None, "a held R3 is one swap");
        }
        let (_, change) = touch_frame(&mut controls, InputState::default(), false);
        assert_eq!(change, None, "letting go swaps nothing");

        let (input, change) = touch_frame(&mut controls, InputState::default(), true);
        assert_eq!(input, InputState::default(), "Tab reaches Azahar as no button");
        assert_eq!(change, swap("Top"), "Tab swaps back");
        let (_, change) = touch_frame(&mut controls, holding(0, &[Button::R3]), true);
        assert_eq!(change, None, "still held, now on R3 as well");
        touch_frame(&mut controls, InputState::default(), false);

        let (input, change) = touch_frame(&mut controls, holding(0, &[Button::L3, Button::R3]), false);
        assert_eq!(input, holding(0, &[Button::R3]), "the tap, not the swap");
        assert_eq!(change, swap("Bottom"));
    }

    /// A swap control held over a pause (R3, or Tab) must be let go before it swaps again on
    /// resume, as `HeldOver` holds back the rest of the pad: `resume` marks it held so the frame
    /// that resumes play, with the control still down, swaps nothing.
    #[test]
    fn a_swap_control_held_over_a_pause_swaps_nothing_until_let_go() {
        let swap = |value| Some(("citra_swap_screen", value));

        for tab in [false, true] {
            let mut controls = TouchControls::new(crate::cores::AZAHAR.touch.unwrap());
            let held = || {
                if tab {
                    InputState::default()
                } else {
                    holding(0, &[Button::R3])
                }
            };

            // Resuming with the control still down from the Pause Menu swaps nothing.
            controls.resume();
            let (_, change) = touch_frame(&mut controls, held(), tab);
            assert_eq!(change, None, "still held from before the pause");
            let (_, change) = touch_frame(&mut controls, held(), tab);
            assert_eq!(change, None, "still held, a frame later");

            // Letting go, then pressing again, swaps once.
            let (_, change) = touch_frame(&mut controls, InputState::default(), false);
            assert_eq!(change, None, "let go");
            let (_, change) = touch_frame(&mut controls, held(), tab);
            assert_eq!(change, swap("Bottom"), "a new press swaps");
        }
    }

    /// DS on melonDS DS (FR-DS-2, FR-DS-3): R3 and Tab hold R3, its Next Screen Layout, while
    /// held; no option changes.
    #[test]
    fn on_ds_r3_and_tab_hold_r3_its_next_screen_layout() {
        let mut controls = TouchControls::new(crate::cores::MELONDS_DS.touch.unwrap());
        for _ in 0..2 {
            let (input, change) = touch_frame(&mut controls, holding(1, &[Button::R3, Button::A]), false);
            assert_eq!(
                (input, change),
                (holding(1, &[Button::R3, Button::A]), None),
                "R3"
            );
        }
        let (input, change) = touch_frame(&mut controls, InputState::default(), true);
        assert_eq!((input, change), (holding(0, &[Button::R3]), None), "Tab");
    }

    /// DS on melonDS DS (FR-DS-3): a pad's L3 taps with the right-stick cursor, reaching melonDS
    /// DS as R2, its Touch Joystick, and never as L3, its microphone; a pad's R2 reaches it as L3,
    /// the microphone, and never taps; both together are both. On every port, beside other
    /// buttons, which reach it as they are.
    #[test]
    fn on_ds_l3_taps_as_r2_and_r2_is_the_microphone_as_l3() {
        let mut controls = TouchControls::new(crate::cores::MELONDS_DS.touch.unwrap());
        for port in [0, 1, 3] {
            let frame =
                |controls: &mut TouchControls, buttons| touch_frame(controls, holding(port, buttons), false);
            assert_eq!(
                frame(&mut controls, &[Button::L3]),
                (holding(port, &[Button::R2]), None),
                "port {port}: L3 taps, as R2 only"
            );
            assert_eq!(
                frame(&mut controls, &[Button::R2, Button::B]),
                (holding(port, &[Button::L3, Button::B]), None),
                "port {port}: R2 is the microphone, as L3 only"
            );
            assert_eq!(
                frame(&mut controls, &[Button::L3, Button::R2]),
                (holding(port, &[Button::R2, Button::L3]), None),
                "port {port}: both are both"
            );
            assert_eq!(
                frame(&mut controls, &[Button::L2, Button::L]),
                (holding(port, &[Button::L2, Button::L]), None),
                "port {port}: L2 and L as they are"
            );
        }
        let mut two = holding(0, &[Button::L3]);
        two.set(2, Button::R2, true);
        let mut expected = holding(0, &[Button::R2]);
        expected.set(2, Button::L3, true);
        assert_eq!(
            touch_frame(&mut controls, two, false),
            (expected, None),
            "each pad's own"
        );
    }

    #[test]
    fn both_sticks_reach_the_core_on_the_pads_port_without_a_deadzone() {
        let mut pause = PauseTrigger::default();
        let pad = PadState {
            left_x: -32768,
            left_y: 1,
            right_x: 32767,
            right_y: -200,
            ..Default::default()
        };
        let (input, _) = frame_input(&[(2, pad)], InputState::default(), false, &mut pause);
        assert_eq!(
            input.stick(2, Stick::Left),
            (-0x7fff, 1),
            "clamped to libretro's range"
        );
        assert_eq!(
            input.stick(2, Stick::Right),
            (0x7fff, -200),
            "the right stick apart"
        );
        assert_eq!(input.stick(0, Stick::Left), (0, 0), "other ports centred");
        assert_eq!(input.stick(0, Stick::Right), (0, 0));
    }

    /// A 4:3 picture pillarboxed in the logical screen, as `timing::fit` places it.
    const PICTURE: Rect = Rect::new(240.0, 0.0, 1440.0, 1080.0);

    fn at(x: f32, y: f32, left: bool) -> Mouse {
        Mouse {
            at: Some((x, y)),
            left,
        }
    }

    #[test]
    fn the_mouse_touches_the_picture_from_corner_to_corner() {
        let p = |x, y, pressed| Some(Pointer { x, y, pressed });
        assert_eq!(touch(PICTURE, at(960.0, 540.0, true)), p(0, 0, true), "centre");
        assert_eq!(touch(PICTURE, at(240.0, 0.0, false)), p(-0x7fff, -0x7fff, false));
        assert_eq!(touch(PICTURE, at(1680.0, 1080.0, true)), p(0x7fff, 0x7fff, true));
        assert_eq!(touch(PICTURE, at(1680.0, 0.0, false)), p(0x7fff, -0x7fff, false));
        assert_eq!(touch(PICTURE, at(600.0, 810.0, false)), p(-0x4000, 0x4000, false));
    }

    #[test]
    fn the_mouse_off_the_picture_touches_nothing() {
        assert_eq!(touch(PICTURE, at(100.0, 540.0, true)), None, "in the black bar");
        assert_eq!(touch(PICTURE, at(1700.0, 540.0, true)), None, "the other bar");
        assert_eq!(touch(PICTURE, at(960.0, -5.0, true)), None, "above the picture");
        assert_eq!(touch(PICTURE, at(960.0, 1081.0, true)), None);
        assert_eq!(touch(PICTURE, Mouse::default()), None, "no mouse");
        let letterboxed = Rect::new(0.0, 60.0, 1920.0, 960.0);
        assert_eq!(touch(letterboxed, at(960.0, 30.0, true)), None);
        let corner = Pointer {
            x: -0x7fff,
            y: -0x7fff,
            pressed: true,
        };
        assert_eq!(touch(letterboxed, at(0.0, 60.0, true)), Some(corner));
    }
}
