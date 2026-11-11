//! The embedded game session's SDL side: an Embedded System's Game running on its Core, its
//! picture drawn through [`Gfx`] without chrome, its sound played through an SDL audio stream and
//! its input read from the keyboard (FR-EMB-*). The session is the screen on top of [`Nav`] while
//! the Game runs, so ending it pops back to where it started.

use std::ffi::CStr;
use std::fmt::Display;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sdl3::AudioSubsystem;
use sdl3::audio::{AudioFormat, AudioSpec, AudioStreamOwner};
use sdl3::gamepad::{Axis, Button as PadButton, ButtonLabel, Gamepad};
use sdl3::keyboard::Scancode;
use sdl3::sys::video::SDL_GLContext;
use sdl3::video::Window;

use super::gl::SdlGl;
use super::glx::GlxGl;
use super::input::{self, FaceLetters, HeldOver, Mouse, PadState, PauseTrigger, TouchControls};
use super::saves::{self, Tracker};
use super::states::{self, SLOTS, StatePaths};
use super::timing::{self, AUDIO_MAX, PlayTimer, Scheduler};
use crate::cores::{self, CoreInfo, GlKind, WiiController};
use crate::ctx::{self, Ctx, Play, Start};
use crate::launch::{self, FAILED_TITLE};
use crate::library::db::GameRow;
use crate::libretro::{Core, CoreDirs, CoreSetup, Frame, GlHost, PortDevice, Stick};
use crate::screens::message::Message;
use crate::screens::pause::{PauseMenu, Request, SlotInfo};
use crate::systems::{self, System};
use crate::ui::cover::playtime_label;
use crate::ui::gfx::Gfx;
use crate::ui::input::Input;
use crate::ui::nav::{Hint, Nav, Screen, Transition};

/// A Game running on its Core.
pub struct Session {
    core: Core,
    game_id: i64,
    /// The Display Title.
    title: String,
    /// `None` when there's no sound: no audio device, or it failed.
    audio: Option<Audio>,
    scheduler: Scheduler,
    pause: PauseTrigger,
    /// Buttons held when play started or resumed, kept from the Core until let go.
    held: HeldOver,
    /// The display's refresh rate frames sync to, if any (see [`Scheduler::new`]).
    refresh: Option<f64>,
    /// The Core's frame rate and sample rate the scheduler and sound are set up for.
    fps: f64,
    sample_rate: f64,
    /// The Game's save memory file and its change tracker; `None` when it has no save memory.
    save: Option<(PathBuf, Tracker)>,
    system: &'static System,
    states: StatePaths,
    /// The Game's Playtime before this session.
    playtime_before: u64,
    timer: PlayTimer,
    shown: bool,
    /// Whether the Pause Menu is open over it.
    paused: bool,
    /// The controls of the Game's touch screen (DS, 3DS), if it has one: the mouse touches it, R3
    /// and Tab swap screens, a pad button taps with the right-stick cursor, and the mouse cursor
    /// shows.
    touch: Option<TouchControls>,
    /// Whether the left stick also presses the d-pad ([`cores::CoreInfo::stick_is_dpad`]).
    stick_dpad: bool,
    /// Whether a Nintendo-lettered pad's face buttons map by letter ([`input::faces_by_letter`]).
    by_letter: bool,
    /// How fast the Game really ran, logged when the session ends.
    pace: Pace,
    /// Set once, by [`Session::start`]: the message to show the player when they chose a Save
    /// State slot that couldn't be loaded, so the Game started fresh instead (FR-DET-4).
    start_message: Option<String>,
}

/// Counts over a session's unpaused ticks, to log how fast the Game really ran against its own
/// frame rate: a Game that sounds too fast shows up here.
#[derive(Debug, Default)]
struct Pace {
    /// Displayed frames that ran the Core.
    ticks: u64,
    core_frames: u64,
    secs: f64,
    /// Times the sound backlog grew past `AUDIO_MAX` and was dropped.
    dropped: u64,
}

struct Audio {
    stream: AudioStreamOwner,
    /// The Core's sample rate.
    rate: f64,
}

impl Audio {
    fn open(subsystem: &AudioSubsystem, rate: f64) -> Result<Audio, String> {
        let spec = AudioSpec::new(Some(rate.round() as i32), Some(2), Some(AudioFormat::S16LE));
        let stream = subsystem
            .default_playback_device()
            .open_device_stream(Some(&spec))
            .map_err(|e| e.to_string())?;
        stream.resume().map_err(|e| e.to_string())?;
        Ok(Audio { stream, rate })
    }

    /// Plays the Core's sound at its new sample `rate` from now on.
    fn set_rate(&mut self, rate: f64) -> Result<(), String> {
        let spec = AudioSpec::new(Some(rate.round() as i32), Some(2), Some(AudioFormat::S16LE));
        let spec: sdl3::sys::audio::SDL_AudioSpec = (&spec).into();
        // SAFETY: the stream is alive; a null output spec leaves the device side as it is.
        if !unsafe { sdl3::sys::audio::SDL_SetAudioStreamFormat(self.stream.raw(), &spec, std::ptr::null()) }
        {
            return Err(sdl3::get_error().to_string());
        }
        self.rate = rate;
        Ok(())
    }

    /// Seconds of sound waiting to be played.
    fn queued(&self) -> f64 {
        // 2 channels of 2-byte samples.
        self.stream.queued_bytes().unwrap_or(0).max(0) as f64 / (4.0 * self.rate)
    }

    /// Drops a backlog (after a stall) rather than play it late, and sets the playback speed that
    /// keeps the queue near its target. Returns whether it dropped one.
    fn balance(&self, base_ratio: f64) -> bool {
        let mut queued = self.queued();
        let dropped = queued > AUDIO_MAX;
        if dropped {
            let _ = self.stream.clear();
            queued = 0.0;
        }
        let ratio = timing::audio_ratio(base_ratio, queued) as f32;
        // SAFETY: the stream is alive.
        unsafe { sdl3::sys::audio::SDL_SetAudioStreamFrequencyRatio(self.stream.raw(), ratio) };
        dropped
    }
}

/// Where a running Game's new frames go: the [`Gfx`] it is drawn with, or nowhere in tests.
pub trait GameFrames {
    fn upload_game_frame(&mut self, frame: &Frame);
}

impl GameFrames for Gfx<'_> {
    fn upload_game_frame(&mut self, frame: &Frame) {
        Gfx::upload_game_frame(self, frame);
    }
}

/// Lets held arrow keys tilt port 0's left stick fully, ahead of a pad's own stick, on Cores that
/// read it as analog only (FR-UI-1); see [`input::keyboard_stick`].
fn apply_keyboard_stick(
    input: &mut crate::libretro::InputState,
    down: impl Fn(Scancode) -> bool,
    stick_dpad: bool,
) {
    let (kx, ky) = input::keyboard_stick(&down, stick_dpad);
    if (kx, ky) != (0, 0) {
        input.set_stick(0, Stick::Left, kx, ky);
    }
}

/// Reads this frame's [`PadState`] from an open SDL gamepad. The thin SDL adapter: `PadState`
/// itself is pure and tested in `session::input`.
pub fn pad_state(gamepad: &Gamepad) -> PadState {
    let mut state = PadState::default();
    for button in input::ALL_BUTTONS {
        if gamepad.button(button) {
            state.press(button);
        }
    }
    state.left_x = gamepad.axis(Axis::LeftX);
    state.left_y = gamepad.axis(Axis::LeftY);
    state.right_x = gamepad.axis(Axis::RightX);
    state.right_y = gamepad.axis(Axis::RightY);
    state.left_trigger = gamepad.axis(Axis::TriggerLeft);
    state.right_trigger = gamepad.axis(Axis::TriggerRight);
    state.letters = face_letters(gamepad);
    state
}

/// Which letters an open SDL gamepad's face buttons carry: Nintendo when SDL labels its right
/// button A or its bottom one B (SDL itself names the buttons by position).
pub fn face_letters(gamepad: &Gamepad) -> FaceLetters {
    // The sdl3 crate's name for SDL_GetGamepadButtonLabel.
    let label = |button| gamepad.button_label_for_gamepad_type(button);
    if label(PadButton::East) == ButtonLabel::A || label(PadButton::South) == ButtonLabel::B {
        FaceLetters::Nintendo
    } else {
        FaceLetters::Other
    }
}

/// The options `core` runs `system`'s Game at `game` with: for a System with two screens, in the
/// layout chosen in Settings → Display (FR-DS-1, FR-NEW-2).
fn core_options(
    ctx: &Ctx,
    system: &System,
    core: &CoreInfo,
    game: &Path,
) -> Vec<(&'static str, &'static str)> {
    core.options_for(system, game, ctx.screen_layout(system))
}

/// The controller `core` is told sits in each port for `game`, of `system`: on Wii the one chosen
/// for the Game in its Options Menu (FR-NEW-3), which a library error leaves at the default.
fn port_device(ctx: &Ctx, system: &System, core: &CoreInfo, game: &GameRow) -> PortDevice {
    let wii = if system.id == "wii" {
        ctx.library.wii_controller(game.id).unwrap_or_else(|e| {
            eprintln!("game37: can't read {}'s controller: {e}", game.title);
            WiiController::default()
        })
    } else {
        WiiController::default()
    };
    core.port_device(system, wii)
}

/// Why [`Session::start`] failed: the title to show it under, besides [`FAILED_TITLE`] for most
/// failures, and the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartError {
    pub title: &'static str,
    pub message: String,
}

impl StartError {
    /// A failure under the generic [`FAILED_TITLE`].
    fn generic(message: String) -> StartError {
        StartError {
            title: FAILED_TITLE,
            message,
        }
    }
}

impl Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Session {
    /// Loads `play`'s Game on its System's Core, on the OpenGL host its [`cores::GlKind`] needs
    /// (see [`open_gl`]), beside the SDL renderer's own window and context `frontend` (`None` on
    /// another renderer), and puts it in the state [`start_state`] picks. `refresh` is the
    /// display's refresh rate when frames are synced to it (vsync), else `None`. `Err` is the
    /// title to show the failure under and its message (e.g. the BIOS rule's own title for a
    /// missing BIOS, FR-EMB-14).
    pub fn start(
        play: &Play,
        ctx: &Ctx,
        frontend: Option<(Window, SDL_GLContext)>,
        audio: Option<&AudioSubsystem>,
        refresh: Option<f64>,
    ) -> Result<Session, StartError> {
        let game = &play.game;
        let system = systems::by_id(&game.system)
            .ok_or_else(|| StartError::generic(format!("No System {:?}", game.system)))?;
        let core_info = cores::for_system(system)
            .ok_or_else(|| StartError::generic(format!("{} has no Core yet.", system.name)))?;
        // Launching checks this too, but a Core may only log a missing BIOS (LRPS2 does), so the
        // BIOS taken away since then gets the same message (FR-EMB-12).
        if let Some(rule) = core_info.bios
            && rule.found(&ctx.games_root).is_none()
        {
            return Err(StartError {
                title: rule.wording.title,
                message: launch::missing_bios_message(&ctx.games_root, rule),
            });
        }
        let gl = open_gl(core_info, frontend, None, system.name)?;
        let fail = |e: &dyn Display| format!("{} couldn't start {}: {e}.", core_info.name, game.title);
        let root = &ctx.games_root;
        let dirs = CoreDirs {
            system: cores::system_dir(core_info, &ctx.data_dir, root),
            save: saves::save_dir(root, system.id),
        };
        for dir in [&dirs.system, &dirs.save] {
            fs::create_dir_all(dir)
                .map_err(|e| StartError::generic(fail(&format!("Can't create {}: {e}", dir.display()))))?;
        }
        // Before the Core loads, or it makes its own (an unformatted PS2 memory card).
        let rel_path = Path::new(&game.rel_path);
        saves::give_first_save(core_info.saves, root, system.id, rel_path)
            .map_err(|e| StartError::generic(fail(&format!("Can't write its memory card: {e}"))))?;
        let core_path = cores::installed_path(&ctx.data_dir, core_info);
        let game_path = root.join(&game.rel_path);
        let options = core_options(ctx, system, core_info, &game_path);
        let setup = CoreSetup {
            options: &options,
            device: port_device(ctx, system, core_info, game),
            reread_timing: core_info.rereads_timing(),
            address_window: core_info.needs_address_window(),
        };
        // When the Core says why (a missing BIOS, an encrypted dump), that is the reason shown
        // (FR-EMB-14).
        let mut core = Core::load(&core_path, &game_path, dirs, setup, gl).map_err(|e| {
            StartError::generic(match e.core_message() {
                Some(said) => format!("{} couldn't start {}.\n{said}", core_info.name, game.title),
                None => fail(&e),
            })
        })?;

        let srm = saves::srm_path(root, system.id, Path::new(&game.rel_path));
        let save = core.save_ram().map(|ram| {
            if let Err(e) = saves::load(&srm, ram) {
                eprintln!("game37: can't read {}: {e}", srm.display());
            }
            let tracker = Tracker::new(ram);
            (srm, tracker)
        });
        let states = StatePaths::new(root, system.id, Path::new(&game.rel_path));
        // Continue where the player quit (FR-EMB-6), or from the Save State or power-on they chose
        // (FR-DET-4).
        let start_message = load_start_state(&states, play.start, |state| {
            core.unserialize(state).map_err(|e| e.to_string())
        });
        let av = core.av_info();
        let audio = audio.and_then(|a| {
            Audio::open(a, av.sample_rate)
                .map_err(|e| eprintln!("game37: no game sound: {e}"))
                .ok()
        });
        Ok(Session {
            core,
            game_id: game.id,
            title: game.title.clone(),
            audio,
            scheduler: Scheduler::new(av.fps, refresh),
            pause: PauseTrigger::default(),
            held: HeldOver::default(),
            refresh,
            fps: av.fps,
            sample_rate: av.sample_rate,
            save,
            system,
            states,
            playtime_before: game.playtime_secs,
            timer: PlayTimer::start(Instant::now()),
            shown: true,
            paused: false,
            touch: core_info.touch.map(TouchControls::new),
            stick_dpad: core_info.stick_is_dpad(),
            by_letter: input::faces_by_letter(system),
            pace: Pace::default(),
            start_message,
        })
    }

    /// Whether the Game has a touch screen the mouse touches, so the mouse cursor shows.
    pub fn touch_screen(&self) -> bool {
        self.touch.is_some()
    }

    /// The window was hidden or shown: a hidden Game runs no frames and plays no sound, and owes
    /// no frames for the time it was hidden.
    pub fn set_shown(&mut self, shown: bool) {
        if shown == self.shown {
            return;
        }
        self.shown = shown;
        self.timer.set_shown(shown, Instant::now());
        self.sync_audio();
    }

    /// The Pause Menu opened or closed: a paused Game runs no frames, plays no sound and counts
    /// no Playtime. Resuming waits for the pause trigger to be let go before it can fire again,
    /// and keeps the buttons held then (the menu's Confirm) from the Core until they are let go.
    fn set_paused(&mut self, paused: bool) {
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        self.timer.set_paused(paused, Instant::now());
        if !paused {
            self.pause = PauseTrigger::default();
            self.held = HeldOver::default();
            if let Some(touch) = &mut self.touch {
                touch.resume();
            }
        }
        self.sync_audio();
    }

    fn sync_audio(&mut self) {
        let play = self.shown && !self.paused;
        if let Some(audio) = &self.audio {
            let _ = if play {
                audio.stream.resume()
            } else {
                audio.stream.pause()
            };
        }
        self.scheduler.reset();
    }

    /// Runs the Core frames due `dt` seconds after the last tick, with `pads` holding each
    /// occupied port's state, `mouse` touching a touch screen and the keys `down` reports held,
    /// uploading new frames to `gfx` and
    /// queueing their sound. Returns whether the pause trigger (Esc, Guide or Select+Start)
    /// fired, in which case no frame runs.
    pub fn tick(
        &mut self,
        gfx: &mut impl GameFrames,
        dt: f64,
        pads: &[(usize, PadState)],
        mouse: Mouse,
        down: impl Fn(Scancode) -> bool,
    ) -> bool {
        let keyboard = input::keyboard_buttons(&down);
        let esc = down(Scancode::Escape);
        let (mut input, paused) = input::frame_input_with(
            pads,
            keyboard,
            esc,
            &mut self.pause,
            self.stick_dpad,
            self.by_letter,
        );
        if paused {
            return true;
        }
        apply_keyboard_stick(&mut input, &down, self.stick_dpad);
        if let Some(controls) = &mut self.touch
            && let Some((key, value)) = controls.apply(&mut input, down(input::SWAP_SCREENS_KEY))
        {
            self.core.set_option(key, value);
        }
        self.held.mask(&mut input);
        self.follow_av_info();
        if self.touch.is_some() {
            // Where `draw` puts the frame.
            input.set_pointer(input::touch(timing::fit(self.core.av_info().aspect_ratio), mouse));
        }
        let frames = self.scheduler.frames(dt);
        self.pace.ticks += 1;
        self.pace.core_frames += u64::from(frames);
        self.pace.secs += dt.max(0.0);
        for _ in 0..frames {
            let out = self.core.run_frame(&input);
            if !out.duped {
                gfx.upload_game_frame(out.video);
            }
            if let Some(audio) = &self.audio
                && let Err(e) = audio.stream.put_data_i16(out.audio)
            {
                eprintln!("game37: game sound stopped: {e}");
                self.audio = None;
            }
        }
        if let Some(audio) = &self.audio
            && audio.balance(self.scheduler.base_ratio())
        {
            self.pace.dropped += 1;
        }
        if let Some((path, tracker)) = &mut self.save
            && let Some(ram) = self.core.save_ram()
            && tracker.due(Duration::from_secs_f64(dt.max(0.0)), ram)
        {
            write_save(path, tracker, ram);
        }
        false
    }

    /// Follows a Core's `SET_SYSTEM_AV_INFO`: a new frame rate reschedules frames, a new sample
    /// rate plays sound at it. A new geometry needs nothing, as the picture follows frame size.
    fn follow_av_info(&mut self) {
        let av = self.core.av_info();
        if av.fps != self.fps && av.fps > 0.0 {
            self.fps = av.fps;
            self.scheduler = Scheduler::new(av.fps, self.refresh);
        }
        if av.sample_rate != self.sample_rate && av.sample_rate > 0.0 {
            self.sample_rate = av.sample_rate;
            if let Some(audio) = &mut self.audio
                && let Err(e) = audio.set_rate(av.sample_rate)
            {
                eprintln!("game37: game sound stopped: {e}");
                self.audio = None;
            }
        }
    }

    /// While paused: whether Guide or Select+Start was pressed again, which resumes. Esc is left
    /// to the Pause Menu, as its Back.
    fn resume_trigger(&mut self, pads: &[(usize, PadState)], down: impl Fn(Scancode) -> bool) -> bool {
        let esc = down(Scancode::Escape);
        let (_, fired) = input::frame_input(pads, Default::default(), esc, &mut self.pause);
        fired && !esc
    }

    /// The Pause Menu's heading line: "<System> · <Playtime> played".
    fn meta(&self) -> String {
        let secs = self.playtime_before + self.timer.played(Instant::now()).as_secs();
        format!("{} · {} played", self.system.name, playtime_label(secs))
    }

    /// Writes slot `n`'s Save State and a screenshot of the current frame (FR-EMB-5).
    fn save_slot(&mut self, n: usize) -> Result<SlotInfo, String> {
        let state = self.core.serialize().map_err(|e| e.to_string())?;
        let png = states::encode_png(&states::frame_rgb(self.core.frame()));
        states::write_slot(&self.states.slot(n), &state, &self.states.thumb(n), png)
            .map_err(|e| e.to_string())?;
        slot_info(&self.states, n).ok_or_else(|| "the slot is gone".to_owned())
    }

    fn load_slot(&mut self, n: usize) -> Result<(), String> {
        let state = fs::read(self.states.slot(n)).map_err(|e| e.to_string())?;
        self.core.unserialize(&state).map_err(|e| e.to_string())
    }

    /// Resets the Core, as the power button would: the Game restarts, but nothing on disk
    /// changes (FR-EMB-4).
    fn restart(&mut self) {
        self.core.reset();
    }

    /// Writes the Resume State the next launch continues from (FR-EMB-6).
    fn write_resume(&mut self) {
        store_resume(&self.states.resume(), self.core.serialize());
    }

    /// Writes the save memory if it changed and the Resume State, records the Playtime and
    /// silences the Game. The Core shuts down when the session is dropped.
    fn end(&mut self, ctx: &mut Ctx, now: i64) {
        let Pace {
            ticks,
            core_frames,
            secs,
            dropped,
        } = self.pace;
        if secs > 0.0 {
            eprintln!(
                "game37: ran {core_frames} Core frames in {secs:.1} s: {:.2} fps against the Core's {:.2}, \
                 {:.2} displayed frames a second (display {}), sound backlog dropped {dropped} times",
                core_frames as f64 / secs,
                self.fps,
                ticks as f64 / secs,
                self.refresh
                    .map_or("not synced".to_owned(), |r| format!("{r:.2} Hz")),
            );
        }
        self.write_resume();
        if let Some((path, tracker)) = &mut self.save
            && let Some(ram) = self.core.save_ram()
            && tracker.changed(ram)
        {
            write_save(path, tracker, ram);
        }
        launch::record(ctx, self.game_id, now, self.timer.played(Instant::now()));
        self.audio = None;
    }
}

/// Writes the Resume State `state` to `path`. Without a new one, the old one is removed: resuming
/// from it would put an older state on top of newer save memory.
fn store_resume(path: &Path, state: Result<Vec<u8>, impl Display>) {
    let written = match state {
        Ok(state) => saves::write(path, &state).map_err(|e| format!("can't write {}: {e}", path.display())),
        Err(e) => Err(format!("no Resume State: {e}")),
    };
    if let Err(e) = written {
        match fs::remove_file(path) {
            Ok(()) => eprintln!("game37: {e}; removed the old one"),
            Err(r) if r.kind() == std::io::ErrorKind::NotFound => eprintln!("game37: {e}"),
            Err(r) => eprintln!("game37: {e}; can't remove the old one: {r}"),
        }
    }
}

fn write_save(path: &Path, tracker: &mut Tracker, ram: &[u8]) {
    match saves::write(path, ram) {
        Ok(()) => tracker.wrote(ram),
        Err(e) => eprintln!("game37: can't write {}: {e}", path.display()),
    }
}

impl Screen for Session {
    fn title(&self) -> &str {
        &self.title
    }

    fn handle(&mut self, _: Input, _: &mut Ctx) -> Transition {
        Transition::None
    }

    fn draw(&mut self, gfx: &mut Gfx<'_>, _: &Ctx) {
        gfx.draw_game_frame(timing::fit(self.core.av_info().aspect_ratio));
    }

    fn hints(&self) -> Vec<Hint> {
        Vec::new()
    }

    fn chrome(&self) -> bool {
        false
    }

    fn session(&mut self) -> Option<&mut Session> {
        Some(self)
    }
}

/// Opens the OpenGL host `core_info`'s [`GlKind`] needs: beside the SDL renderer's own window and
/// context `frontend` (`None` on another renderer), or GLX's, on the X display `display` (`None`:
/// `$DISPLAY`). `Err` is the failure to show for the System `system_name`, with the real error
/// logged to stderr: also when the Core [needs OpenGL](CoreInfo::needs_gl) and `frontend` is
/// `None`, so the Core is never loaded without the host it needs (issue #43).
fn open_gl(
    core_info: &CoreInfo,
    frontend: Option<(Window, SDL_GLContext)>,
    display: Option<&CStr>,
    system_name: &str,
) -> Result<Option<Box<dyn GlHost>>, StartError> {
    match (core_info.gl(), frontend) {
        (GlKind::Sdl, Some((window, context))) => Ok(Some(Box::new(SdlGl::new(window, context)))),
        (GlKind::Sdl, None) if core_info.needs_gl() => Err(StartError {
            title: NO_GL_TITLE,
            message: format!(
                "{system_name} games need OpenGL, which this computer's graphics driver doesn't provide."
            ),
        }),
        (GlKind::Sdl, None) => Ok(None),
        (GlKind::Glx, frontend) => GlxGl::open(display, frontend)
            .map(|host| Some(Box::new(host) as Box<dyn GlHost>))
            .map_err(|e| {
                eprintln!("game37: can't open the GLX host: {e}");
                StartError::generic(format!(
                    "{system_name} games need X11 (XWayland), which isn't available."
                ))
            }),
    }
}

/// The title of the message shown when a Game's Core needs OpenGL and the renderer has none.
const NO_GL_TITLE: &str = "OpenGL isn't available";

/// The state a session starts from: the Save State in `slot` when the player chose one
/// (FR-DET-4), else the Resume State (FR-EMB-6).
fn start_state(states: &StatePaths, slot: Option<usize>) -> PathBuf {
    slot.map_or_else(|| states.resume(), |n| states.slot(n))
}

/// The title of the message pushed when a chosen Save State slot couldn't be loaded.
const SLOT_FAILED_TITLE: &str = "Couldn't load the save";

/// Reads the start state for `start`, a chosen slot's Save State (FR-DET-4) or the Resume State
/// (FR-EMB-6), from `states`, and hands its bytes to `restore` (`Core::unserialize` in
/// [`Session::start`], a stub in tests). Returns the message to show the player when a slot was
/// chosen and its state couldn't be loaded — missing, unreadable, or refused by `restore` — so
/// starting fresh isn't silent. The Resume State falls back to a fresh start silently either way.
/// From power-on, nothing is read and nothing said.
fn load_start_state(
    states: &StatePaths,
    start: Start,
    restore: impl FnOnce(&[u8]) -> Result<(), String>,
) -> Option<String> {
    if start == Start::PowerOn {
        return None;
    }
    let slot = start.slot();
    let from = start_state(states, slot);
    let failed = match fs::read(&from) {
        Ok(state) => match restore(&state) {
            Ok(()) => false,
            Err(e) => {
                eprintln!("game37: can't start from {}: {e}", from.display());
                true
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) => {
            eprintln!("game37: can't read {}: {e}", from.display());
            true
        }
    };
    failed
        .then(|| {
            slot.map(|n| format!("Slot {n} couldn't be loaded, so the Game started from the beginning."))
        })
        .flatten()
}

/// Starts `play`'s Game and pushes its session, or pushes the message saying why it couldn't
/// start. Its Core's OpenGL host is chosen from `frontend` and the Core itself (see [`open_gl`]).
/// When the player chose a Save State slot that couldn't be loaded, the Game still starts (fresh),
/// but the message saying so is pushed over it, pausing it until dismissed.
pub fn start(
    play: &Play,
    nav: &mut Nav,
    ctx: &Ctx,
    frontend: Option<(Window, SDL_GLContext)>,
    audio: Option<&AudioSubsystem>,
    refresh: Option<f64>,
) {
    match Session::start(play, ctx, frontend, audio, refresh) {
        Ok(mut session) => {
            let slot_message = session.start_message.take();
            nav.push(Box::new(session));
            if let Some(text) = slot_message {
                nav.push(Box::new(Message::new(SLOT_FAILED_TITLE, text)));
            }
        }
        Err(e) => nav.push(Box::new(Message::new(e.title, e.message))),
    }
}

/// The running session, if a Game runs on `nav`, with or without the Pause Menu over it.
pub fn running(nav: &mut Nav) -> Option<&mut Session> {
    nav.base().session()
}

/// Whether a Game runs on `nav` without the Pause Menu: it then takes no menu input, as it reads
/// the keyboard itself.
pub fn playing(nav: &mut Nav) -> bool {
    nav.modal().is_none() && running(nav).is_some()
}

/// Ticks the running session, if any (see [`Session::tick`]), and handles its pause trigger.
pub fn tick(
    nav: &mut Nav,
    ctx: &mut Ctx,
    gfx: &mut impl GameFrames,
    dt: f64,
    pads: &[(usize, PadState)],
    mouse: Mouse,
    down: impl Fn(Scancode) -> bool,
) {
    let paused = nav.modal().is_some();
    let Some(session) = running(nav) else { return };
    session.set_paused(paused);
    if !paused {
        if session.tick(gfx, dt, pads, mouse, down) {
            on_pause(nav);
        }
        return;
    }
    let resume = session.resume_trigger(pads, down);
    let request = nav.top().pause_menu().and_then(PauseMenu::take_request);
    match request {
        Some(Request::Save(n)) => {
            let info = running(nav).expect("found above").save_slot(n);
            let info = info.map_err(|e| eprintln!("game37: can't save to slot {n}: {e}"));
            if let Some(menu) = nav.top().pause_menu() {
                menu.saved(n, info);
            }
        }
        Some(Request::Load(n)) => match running(nav).expect("found above").load_slot(n) {
            Ok(()) => nav.pop(),
            Err(e) => {
                eprintln!("game37: can't load slot {n}: {e}");
                if let Some(menu) = nav.top().pause_menu() {
                    menu.load_failed(n);
                }
            }
        },
        Some(Request::Restart) => {
            running(nav).expect("found above").restart();
            nav.pop();
        }
        Some(Request::Quit) => end(nav, ctx),
        None if resume => nav.pop(),
        None => {}
    }
}

/// What the pause trigger does: opens the Pause Menu over the session (FR-EMB-3).
pub fn on_pause(nav: &mut Nav) {
    let Some(session) = running(nav) else { return };
    let menu = PauseMenu::new(&session.title, session.meta(), slot_infos(&session.states));
    nav.push(Box::new(menu));
}

/// Ends the running session, if any, paused or not: writes its save memory and Resume State,
/// records its Playtime, shuts its Core down and pops back to where it started.
pub fn end(nav: &mut Nav, ctx: &mut Ctx) {
    if let Some(session) = running(nav) {
        session.end(ctx, ctx::now());
        while nav.modal().is_some() {
            nav.pop();
        }
        nav.pop();
    }
}

/// Slot `n`'s time and screenshot from `states`, `None` when it is empty. Read outside a session
/// too, by the Options Menu's slot picker.
pub fn slot_info(states: &StatePaths, n: usize) -> Option<SlotInfo> {
    let modified = fs::metadata(states.slot(n)).and_then(|m| m.modified()).ok()?;
    let thumb = states::read_png(&states.thumb(n))
        .map_err(|e| {
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!("game37: can't read {}: {e}", states.thumb(n).display());
            }
        })
        .ok();
    Some(SlotInfo {
        when: when_label(modified),
        thumb,
    })
}

/// Every slot's [`slot_info`], slot 1 first.
pub fn slot_infos(states: &StatePaths) -> Vec<Option<SlotInfo>> {
    (1..=SLOTS).map(|n| slot_info(states, n)).collect()
}

/// A file time as the Pause Menu shows it, in local time (see [`states::when_label`]).
pub fn when_label(time: std::time::SystemTime) -> String {
    let then = time
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i64);
    match (local_time(then), now_local()) {
        (Some(then), Some(now)) => states::when_label(then, now),
        _ => String::new(),
    }
}

/// A time in nanoseconds since the Unix epoch as a local date and time.
pub fn local_time(ns: i64) -> Option<states::Local> {
    use sdl3::sys::time::{SDL_DateTime, SDL_TimeToDateTime};
    let mut dt = SDL_DateTime::default();
    // SAFETY: only writes to the local passed in.
    unsafe { SDL_TimeToDateTime(ns, &mut dt, true) }.then_some(states::Local {
        year: dt.year,
        month: dt.month as u32,
        day: dt.day as u32,
        hour: dt.hour as u32,
        minute: dt.minute as u32,
    })
}

/// The current local date and time.
pub fn now_local() -> Option<states::Local> {
    use sdl3::sys::time::SDL_GetCurrentTime;
    let mut now = 0;
    // SAFETY: only writes to the local passed in.
    unsafe { SDL_GetCurrentTime(&mut now) }
        .then_some(now)
        .and_then(local_time)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::TempDir;

    use super::*;
    use crate::app::close;
    use crate::libretro::tests::serial;
    use crate::ui::input::Input;

    const TESTDATA: &str = "/home/matei/Game37/.testdata";

    fn frame_hash(session: &Session) -> u64 {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut h = DefaultHasher::new();
        session.core.frame().hash(&mut h);
        h.finish()
    }

    /// Runs `n` frames without input; returns the last one's hash.
    fn run(session: &mut Session, n: usize) -> u64 {
        for _ in 0..n {
            session.core.run_frame(&Default::default());
        }
        frame_hash(session)
    }

    /// Save States and the Resume State on bsnes (FR-EMB-5, FR-EMB-6, FR-DET-4): loading slot 1
    /// replays the same frames, a relaunch continues from the frame shown when the player quit,
    /// and a launch from slot 1 starts from slot 1 instead.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn pause_slots_and_resume_state_round_trip_on_a_real_core() {
        let _serial = serial();
        let root = TempDir::new().unwrap();
        let rel = "snes/240p Test Suite (World).sfc";
        fs::create_dir_all(root.path().join("snes")).unwrap();
        fs::copy(Path::new(TESTDATA).join("games").join(rel), root.path().join(rel)).unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = root.path().to_path_buf();
        ctx.data_dir = Path::new(TESTDATA).join("data");
        let game = GameRow {
            id: 1,
            system: "snes".into(),
            rel_path: rel.into(),
            title: "240p Test Suite".into(),
            date_added: 0,
            last_played: None,
            playtime_secs: 0,
            favorite: false,
        };

        let mut session = Session::start(&Play::resume(game.clone()), &ctx, None, None, None)
            .unwrap_or_else(|e| panic!("{e}"));
        run(&mut session, 100);
        let info = session.save_slot(1).unwrap();
        assert!(info.thumb.is_some_and(|t| t.width > 0), "slot 1 has a screenshot");
        assert!(info.when.starts_with("Today, "), "{}", info.when);
        let states = root.path().join("states/snes");
        assert!(states.join("240p Test Suite (World).slot1.state").exists());
        assert!(states.join("240p Test Suite (World).slot1.png").exists());
        let first = run(&mut session, 30);
        session.load_slot(1).unwrap();
        assert_eq!(
            run(&mut session, 30),
            first,
            "loading slot 1 replays the same frames"
        );
        // What slot 1 shows next, which a launch from slot 1 must show too. bsnes's first frame
        // after restoring a state into a freshly loaded Core is a stale one, so compare the second.
        session.load_slot(1).unwrap();
        let from_slot = run(&mut session, 2);

        // Down moves the Test Suite's menu cursor once its menu has settled, so the Resume State
        // shows another picture.
        run(&mut session, 200);
        let mut down = crate::libretro::InputState::default();
        down.set(0, crate::libretro::Button::Down, true);
        for _ in 0..6 {
            session.core.run_frame(&down);
        }
        run(&mut session, 30);
        session.end(&mut ctx, 0);
        // What the quit-time state shows next.
        let next = run(&mut session, 2);
        drop(session);
        assert_ne!(
            next, from_slot,
            "the Resume State and slot 1 show different frames"
        );
        assert!(states.join("240p Test Suite (World).resume.state").exists());
        let mut session = Session::start(&Play::resume(game.clone()), &ctx, None, None, None)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(run(&mut session, 2), next, "the relaunch continues where it quit");
        drop(session);

        // A launch from slot 1 (FR-DET-4) starts from it instead, and still writes the Resume
        // State on quitting.
        let from = Play {
            game: game.clone(),
            start: Start::Slot(1),
        };
        let mut session = Session::start(&from, &ctx, None, None, None).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(run(&mut session, 2), from_slot, "the launch starts from slot 1");
        session.end(&mut ctx, 0);
        let after = run(&mut session, 2);
        drop(session);
        assert_ne!(after, next, "a new Resume State");
        let mut session = Session::start(&Play::resume(game.clone()), &ctx, None, None, None)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            run(&mut session, 2),
            after,
            "the next launch continues from that play"
        );
    }

    /// Restart game resets the Core, as the power button would, and closes the Pause Menu back
    /// to play (FR-EMB-4): unlike Quit, nothing on disk changes.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn restart_resets_the_core_and_closes_the_pause_menu_on_a_real_core() {
        use crate::screens::Stub;
        use crate::ui::input::Action;
        use crate::ui::nav::Hits;

        let _serial = serial();
        let root = TempDir::new().unwrap();
        let rel = "snes/240p Test Suite (World).sfc";
        fs::create_dir_all(root.path().join("snes")).unwrap();
        fs::copy(Path::new(TESTDATA).join("games").join(rel), root.path().join(rel)).unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = root.path().to_path_buf();
        ctx.data_dir = Path::new(TESTDATA).join("data");
        let game = GameRow {
            id: 1,
            system: "snes".into(),
            rel_path: rel.into(),
            title: "240p Test Suite".into(),
            date_added: 0,
            last_played: None,
            playtime_secs: 0,
            favorite: false,
        };

        let mut nav = Nav::single(Box::new(Stub::new("Home")));
        let none = |_: Scancode| false;
        let esc = |k: Scancode| k == Scancode::Escape;
        let tick = |nav: &mut Nav, ctx: &mut Ctx, down: &dyn Fn(Scancode) -> bool| {
            tick(nav, ctx, &mut NoPicture, 0.02, &[], Mouse::default(), down)
        };
        let act = |nav: &mut Nav, ctx: &mut Ctx, action: Action| {
            nav.handle(Input::Action(action), &Hits::default(), ctx);
        };

        start(&Play::resume(game.clone()), &mut nav, &ctx, None, None, None);
        tick(&mut nav, &mut ctx, &none);
        run(running(&mut nav).unwrap(), 200); // lets the Test Suite's menu settle

        // Moves the Test Suite's on-screen cursor, so the picture after a restart differs from a
        // continued play unless the Core actually reset.
        let mut down = crate::libretro::InputState::default();
        down.set(0, crate::libretro::Button::Down, true);
        for _ in 0..6 {
            running(&mut nav).unwrap().core.run_frame(&down);
        }
        let moved = run(running(&mut nav).unwrap(), 2);

        tick(&mut nav, &mut ctx, &esc);
        tick(&mut nav, &mut ctx, &none);
        assert!(nav.top().pause_menu().is_some(), "Esc pauses");
        for action in [Action::Down, Action::Down, Action::Down, Action::Confirm] {
            act(&mut nav, &mut ctx, action);
        }
        // The Restart confirmation: Right moves focus from Cancel to Restart, Confirm chooses it.
        act(&mut nav, &mut ctx, Action::Right);
        act(&mut nav, &mut ctx, Action::Confirm);
        tick(&mut nav, &mut ctx, &none);

        assert!(nav.top().pause_menu().is_none(), "Restart closes the Pause Menu");
        assert_ne!(
            run(running(&mut nav).unwrap(), 2),
            moved,
            "the Core restarted, rather than continuing from the moved cursor"
        );
    }

    /// A SNES Game in `root` with junk in Save State slot 2, and the Play that starts from it.
    fn junk_slot_2(root: &TempDir) -> (Ctx, Play) {
        let rel = "snes/240p Test Suite (World).sfc";
        fs::create_dir_all(root.path().join("snes")).unwrap();
        fs::copy(Path::new(TESTDATA).join("games").join(rel), root.path().join(rel)).unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = root.path().to_path_buf();
        ctx.data_dir = Path::new(TESTDATA).join("data");
        let game = GameRow {
            id: 1,
            system: "snes".into(),
            rel_path: rel.into(),
            title: "240p Test Suite".into(),
            date_added: 0,
            last_played: None,
            playtime_secs: 0,
            favorite: false,
        };
        let states = StatePaths::new(&ctx.games_root, "snes", Path::new(rel));
        fs::create_dir_all(states.slot(2).parent().unwrap()).unwrap();
        fs::write(states.slot(2), b"not a real save state").unwrap();
        let play = Play {
            game,
            start: Start::Slot(2),
        };
        (ctx, play)
    }

    /// A chosen slot the Core refuses (a junk file, e.g. after a Core update) starts the Game
    /// fresh and tells the player, instead of staying silent (FR-DET-4).
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn a_junk_chosen_slot_starts_fresh_and_tells_the_player() {
        let _serial = serial();
        let root = TempDir::new().unwrap();
        let (ctx, play) = junk_slot_2(&root);
        let mut session = Session::start(&play, &ctx, None, None, None).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            session.start_message.as_deref(),
            Some("Slot 2 couldn't be loaded, so the Game started from the beginning."),
            "the player is told, instead of a silent fresh start"
        );
        run(&mut session, 2);
    }

    /// #47: `start` puts the slot failure up as a Message over the running session, which stays
    /// below it, rather than in place of it.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn start_puts_the_slot_failure_message_over_the_session() {
        let _serial = serial();
        let root = TempDir::new().unwrap();
        let (ctx, play) = junk_slot_2(&root);
        let mut nav = Nav::single(Box::new(crate::screens::Stub::new("Home")));
        start(&play, &mut nav, &ctx, None, None, None);
        assert_eq!(nav.depth(), 3, "Home, the session and the Message");
        assert_eq!(nav.top().title(), SLOT_FAILED_TITLE);
        assert!(nav.top().is_modal(), "the Message is over the session");
        assert!(running(&mut nav).is_some(), "the Game runs below the Message");
        assert_eq!(
            running(&mut nav).unwrap().start_message,
            None,
            "the message was handed to the Message"
        );
    }

    #[test]
    fn a_session_starts_from_the_chosen_slot_else_from_the_resume_state() {
        let states = StatePaths::new(Path::new("/g"), "snes", Path::new("snes/T.sfc"));
        assert_eq!(start_state(&states, Some(2)), states.slot(2));
        assert_eq!(start_state(&states, None), states.resume());
    }

    /// What the Pause Menu and the Options Menu's slot picker show, read without a session.
    #[test]
    fn slot_infos_read_the_filled_slots_with_their_screenshot_and_time() {
        let dir = TempDir::new().unwrap();
        let states = StatePaths::new(dir.path(), "snes", Path::new("snes/T.sfc"));
        assert_eq!(slot_infos(&states), [None, None, None]);
        let thumb = states::Rgb {
            width: 2,
            height: 1,
            data: vec![255, 0, 0, 0, 0, 255],
        };
        let png = states::encode_png(&thumb);
        states::write_slot(&states.slot(2), b"state", &states.thumb(2), png).unwrap();
        let infos = slot_infos(&states);
        assert_eq!(
            (infos.len(), infos[0].is_none(), infos[2].is_none()),
            (SLOTS, true, true)
        );
        let slot2 = infos[1].as_ref().expect("slot 2 is filled");
        assert_eq!(slot2.thumb.as_ref(), Some(&thumb));
        assert!(slot2.when.starts_with("Today, "), "{}", slot2.when);
    }

    #[test]
    fn a_failed_resume_state_removes_the_old_one() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("states/snes/T.resume.state");
        store_resume(&path, Ok::<_, String>(b"new".to_vec()));
        assert_eq!(fs::read(&path).unwrap(), b"new");
        store_resume(&path, Err::<Vec<u8>, _>("the Core can't serialize"));
        assert!(
            !path.exists(),
            "the next launch starts fresh, not from an older state"
        );
        store_resume(&path, Err::<Vec<u8>, _>("again, with nothing to remove"));
        assert!(!path.exists());
    }

    /// A chosen slot that can't be loaded (missing or refused) tells the player; a chosen slot
    /// that loads fine, or the Resume State either way, says nothing (FR-DET-4, FR-EMB-6).
    #[test]
    fn load_start_state_messages_only_on_a_bad_chosen_slot() {
        let dir = TempDir::new().unwrap();
        let states = StatePaths::new(dir.path(), "snes", Path::new("Game.sfc"));

        // Slot 2 was chosen but there's no such file.
        let message = load_start_state(&states, Start::Slot(2), |_| panic!("not reached: missing file"));
        assert_eq!(
            message,
            Some("Slot 2 couldn't be loaded, so the Game started from the beginning.".to_string())
        );

        // Slot 1 was chosen, its file exists, but the Core refuses it.
        fs::create_dir_all(states.slot(1).parent().unwrap()).unwrap();
        fs::write(states.slot(1), b"junk").unwrap();
        let message = load_start_state(&states, Start::Slot(1), |_| Err("bad state".to_string()));
        assert_eq!(
            message,
            Some("Slot 1 couldn't be loaded, so the Game started from the beginning.".to_string())
        );

        // Slot 1 was chosen, its file exists, and the Core accepts it: no message.
        let message = load_start_state(&states, Start::Slot(1), |_| Ok(()));
        assert_eq!(message, None);

        // No slot was chosen (Resume State): missing or refused still says nothing.
        assert_eq!(
            load_start_state(&states, Start::Resume, |_| panic!("not reached")),
            None
        );
        fs::write(states.resume(), b"junk").unwrap();
        let message = load_start_state(&states, Start::Resume, |_| Err("bad state".to_string()));
        assert_eq!(message, None, "the Resume State falls back silently");
    }

    /// Start from power-on (FR-DET-4) reads no state, though a Resume State and a slot are there,
    /// and says nothing; the Resume State still loads on a normal launch, and stays on disk.
    #[test]
    fn power_on_reads_no_state_and_a_resume_still_does() {
        let dir = TempDir::new().unwrap();
        let states = StatePaths::new(dir.path(), "nds", Path::new("nds/Mario.nds"));
        fs::create_dir_all(states.resume().parent().unwrap()).unwrap();
        fs::write(states.resume(), b"frozen").unwrap();
        fs::write(states.slot(1), b"slot").unwrap();
        let message = load_start_state(&states, Start::PowerOn, |state| {
            panic!("power-on read {:?}", String::from_utf8_lossy(state))
        });
        assert_eq!(message, None);
        assert!(states.resume().exists(), "the Resume State stays");

        let mut read = Vec::new();
        let message = load_start_state(&states, Start::Resume, |state| {
            read = state.to_vec();
            Ok(())
        });
        assert_eq!((message, read.as_slice()), (None, &b"frozen"[..]));
    }

    /// Held arrow keys reach port 0's left stick only on a stick-mode System (N64, PSP), as
    /// `Session::tick` wires `apply_keyboard_stick` in: checked on what a real Core receives. bsnes
    /// reads the d-pad; its session then plays as a stick-mode one would.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn held_arrow_keys_reach_port_0s_left_stick_only_on_stick_mode_systems() {
        let _serial = serial();
        let root = TempDir::new().unwrap();
        let rel = "snes/240p Test Suite (World).sfc";
        fs::create_dir_all(root.path().join("snes")).unwrap();
        fs::copy(Path::new(TESTDATA).join("games").join(rel), root.path().join(rel)).unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = root.path().to_path_buf();
        ctx.data_dir = Path::new(TESTDATA).join("data");
        let game = GameRow {
            id: 1,
            system: "snes".into(),
            rel_path: rel.into(),
            title: "240p Test Suite".into(),
            date_added: 0,
            last_played: None,
            playtime_secs: 0,
            favorite: false,
        };
        let mut session =
            Session::start(&Play::resume(game), &ctx, None, None, None).unwrap_or_else(|e| panic!("{e}"));
        let left_held = |k: Scancode| k == Scancode::Left;
        // A tick of 20 ms runs at least one Core frame.
        let stick = |session: &mut Session| {
            assert!(!session.tick(&mut NoPicture, 0.02, &[], Mouse::default(), left_held));
            session.core.last_input().stick(0, Stick::Left)
        };
        assert!(session.stick_dpad);
        assert_eq!(
            stick(&mut session),
            (0, 0),
            "a d-pad System leaves the stick centred: its own d-pad press is enough"
        );
        session.stick_dpad = false;
        assert_eq!(
            stick(&mut session),
            (-0x7fff, 0),
            "a stick-mode System (N64, PSP) gets the held Left on port 0's left stick"
        );
    }

    /// A Nintendo-lettered pad with its east button (A) held.
    fn east_held() -> PadState {
        let mut east = PadState {
            letters: FaceLetters::Nintendo,
            ..Default::default()
        };
        east.press(PadButton::East);
        east
    }

    /// A Nintendo-lettered pad holding its east button (A) maps to the PlayStation symbols for ✕
    /// (RetroPad B) on a PSP Game (FR-EMB-10), checked on a real Core: `Session::tick` wires
    /// `self.by_letter` into `input::frame_input_with`. Setting that call's `by_letter` argument
    /// to `false` makes this fail.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn a_nintendo_lettered_pad_maps_its_east_button_by_letter_on_a_psp_game() {
        let _serial = serial();
        let east = east_held();
        let root = TempDir::new().unwrap();
        let game_dir = root.path().join("psp/CMFileManager");
        fs::create_dir_all(&game_dir).unwrap();
        for entry in fs::read_dir(Path::new(TESTDATA).join("games/psp/CMFileManager")).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), game_dir.join(entry.file_name())).unwrap();
        }
        let data = TempDir::new().unwrap();
        for dir in ["cores", "system"] {
            std::os::unix::fs::symlink(Path::new(TESTDATA).join("data").join(dir), data.path().join(dir))
                .unwrap();
        }
        let mut ctx = Ctx::empty();
        ctx.games_root = root.path().to_path_buf();
        ctx.data_dir = data.path().to_path_buf();
        let game = GameRow {
            id: 1,
            system: "psp".into(),
            rel_path: "psp/CMFileManager/EBOOT.PBP".into(),
            title: "CMFileManager".into(),
            date_added: 0,
            last_played: None,
            playtime_secs: 0,
            favorite: false,
        };
        let (_sdl, canvas) = offscreen_renderer();
        let creator = canvas.texture_creator();
        let mut gfx = Gfx::new(canvas, &creator, crate::ui::text::Fonts::new().unwrap());
        let frontend = gfx.gl_context();
        let mut session = Session::start(&Play::resume(game), &ctx, frontend, None, Some(60.0))
            .unwrap_or_else(|e| panic!("{e}"));
        // Lets PPSSPP finish booting before it's asked to quit, and settles `HeldOver` on a
        // frame with no pads, so it doesn't mistake the button pressed afterwards as held over
        // from before the Game started and mask it out.
        for _ in 0..900 {
            assert!(!session.tick(&mut gfx, 1.0 / 60.0, &[], Mouse::default(), |_| false));
        }
        assert!(!session.tick(&mut gfx, 1.0 / 60.0, &[(0, east)], Mouse::default(), |_| false));
        let mask = session.core.last_input().mask(0);
        gfx.clear(crate::ui::theme::Colour::hex(0x000000));
        session.draw(&mut gfx, &ctx);
        drop(session);
        assert_eq!(
            mask,
            1 << crate::libretro::Button::B as u16,
            "PSP: Nintendo A maps by letter to ✕ (RetroPad B)"
        );
    }

    /// The same Nintendo-lettered pad's east button (A) stays by position, RetroPad A, on an
    /// SNES Game (FR-EMB-10): SNES isn't a PlayStation System, so it maps by position regardless
    /// of `self.by_letter`, and this still passes with that argument forced to `false`.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn a_nintendo_lettered_pad_stays_by_position_on_an_snes_game() {
        let _serial = serial();
        let east = east_held();
        let root = TempDir::new().unwrap();
        let rel = "snes/240p Test Suite (World).sfc";
        fs::create_dir_all(root.path().join("snes")).unwrap();
        fs::copy(Path::new(TESTDATA).join("games").join(rel), root.path().join(rel)).unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = root.path().to_path_buf();
        ctx.data_dir = Path::new(TESTDATA).join("data");
        let game = GameRow {
            id: 1,
            system: "snes".into(),
            rel_path: rel.into(),
            title: "240p Test Suite".into(),
            date_added: 0,
            last_played: None,
            playtime_secs: 0,
            favorite: false,
        };
        let mut session =
            Session::start(&Play::resume(game), &ctx, None, None, None).unwrap_or_else(|e| panic!("{e}"));
        assert!(!session.tick(&mut NoPicture, 1.0 / 60.0, &[], Mouse::default(), |_| false));
        assert!(
            !session.tick(&mut NoPicture, 1.0 / 60.0, &[(0, east)], Mouse::default(), |_| {
                false
            })
        );
        assert_eq!(
            session.core.last_input().mask(0),
            1 << crate::libretro::Button::A as u16,
            "SNES: Nintendo A stays by position: RetroPad A"
        );
    }

    /// New frames go nowhere.
    struct NoPicture;

    impl GameFrames for NoPicture {
        fn upload_game_frame(&mut self, _: &Frame) {}
    }

    /// A whole session driven through the functions `app.rs` uses (FR-EMB-3 to FR-EMB-6, US-4):
    /// start, tick, pause, resume, hide, quit from the Pause Menu, start again, close the window
    /// while paused.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn a_session_counts_playtime_only_while_played_and_resumes_where_it_was_left() {
        use std::thread::sleep;

        use crate::launch::{Launcher, NeverExits};
        use crate::libretro::Button;
        use crate::screens::Stub;
        use crate::ui::input::Action;
        use crate::ui::nav::Hits;

        let _serial = serial();
        let root = TempDir::new().unwrap();
        let rel = "snes/240p Test Suite (World).sfc";
        fs::create_dir_all(root.path().join("snes")).unwrap();
        fs::copy(Path::new(TESTDATA).join("games").join(rel), root.path().join(rel)).unwrap();
        let mut ctx = Ctx::new(
            crate::library::db::Library::open_in_memory().unwrap(),
            root.path().to_path_buf(),
        );
        ctx.data_dir = Path::new(TESTDATA).join("data");
        ctx.scan_all(0).unwrap();
        let games = |ctx: &Ctx| ctx.library.system_games("snes", Default::default()).unwrap();
        let game = games(&ctx).remove(0);
        let mut nav = Nav::single(Box::new(Stub::new("Home")));
        let mut launcher = Launcher::new(Box::new(NeverExits::default()), root.path().join("logs"));
        let resume = root
            .path()
            .join("states/snes/240p Test Suite (World).resume.state");

        let none = |_: Scancode| false;
        let enter = |k: Scancode| k == Scancode::Return;
        let esc = |k: Scancode| k == Scancode::Escape;
        // A tick of 20 ms runs at least one Core frame.
        let tick = |nav: &mut Nav, ctx: &mut Ctx, down: &dyn Fn(Scancode) -> bool| {
            tick(nav, ctx, &mut NoPicture, 0.02, &[], Mouse::default(), down)
        };
        let start_held = |nav: &mut Nav| {
            let session = running(nav).expect("a Game runs");
            session.core.last_input().is_pressed(0, Button::Start)
        };
        let act = |nav: &mut Nav, ctx: &mut Ctx, action: Action| {
            nav.handle(Input::Action(action), &Hits::default(), ctx);
        };

        start(&Play::resume(game.clone()), &mut nav, &ctx, None, None, None);
        tick(&mut nav, &mut ctx, &enter);
        assert!(
            !start_held(&mut nav),
            "the Enter that launched the Game never reaches it"
        );
        tick(&mut nav, &mut ctx, &none);
        tick(&mut nav, &mut ctx, &enter);
        assert!(start_held(&mut nav), "a new press of Enter does");
        sleep(Duration::from_millis(700));
        tick(&mut nav, &mut ctx, &none);

        // Paused for a second: Esc opens the Pause Menu, Enter on Resume closes it.
        tick(&mut nav, &mut ctx, &esc);
        assert!(nav.top().pause_menu().is_some(), "Esc pauses");
        tick(&mut nav, &mut ctx, &none);
        sleep(Duration::from_secs(1));
        act(&mut nav, &mut ctx, Action::Confirm);
        assert!(nav.modal().is_none(), "Resume");
        tick(&mut nav, &mut ctx, &enter);
        assert!(
            !start_held(&mut nav),
            "the Enter that chose Resume never reaches the Game"
        );

        // Hidden for a second.
        running(&mut nav).unwrap().set_shown(false);
        sleep(Duration::from_secs(1));
        running(&mut nav).unwrap().set_shown(true);
        tick(&mut nav, &mut ctx, &none);

        // What the Game shows two frames on from here, which a relaunch must show too. bsnes's
        // first frame after restoring a state into a freshly loaded Core is a stale one.
        let session = running(&mut nav).unwrap();
        let here = session.core.serialize().unwrap();
        let next = run(session, 2);
        session.core.unserialize(&here).unwrap();

        // Quit to library, then close the window: the Game ends once.
        tick(&mut nav, &mut ctx, &esc);
        tick(&mut nav, &mut ctx, &none);
        for action in [
            Action::Down,
            Action::Down,
            Action::Down,
            Action::Down,
            Action::Confirm,
        ] {
            act(&mut nav, &mut ctx, action);
        }
        tick(&mut nav, &mut ctx, &none);
        assert!(running(&mut nav).is_none(), "Quit to library ends the Game");
        close(&mut nav, &mut ctx, &mut launcher);
        let played = games(&ctx)[0].playtime_secs;
        assert_eq!(
            played, 1,
            "about 0.8 s played, without the paused or hidden second, once"
        );
        assert!(resume.exists(), "quitting writes the Resume State");

        // The next launch continues where it quit; closing the window while paused writes a new
        // Resume State and records the Playtime.
        let game = games(&ctx).remove(0);
        start(&Play::resume(game.clone()), &mut nav, &ctx, None, None, None);
        assert_eq!(run(running(&mut nav).unwrap(), 2), next, "the relaunch resumes");
        fs::remove_file(&resume).unwrap();
        tick(&mut nav, &mut ctx, &none);
        tick(&mut nav, &mut ctx, &esc);
        tick(&mut nav, &mut ctx, &none);
        assert!(nav.top().pause_menu().is_some());
        let before = games(&ctx)[0].last_played;
        sleep(Duration::from_secs(1));
        close(&mut nav, &mut ctx, &mut launcher);
        assert!(running(&mut nav).is_none());
        assert!(
            resume.exists(),
            "closing the window while paused writes the Resume State"
        );
        let game = &games(&ctx)[0];
        assert_eq!(game.playtime_secs, played, "the paused second isn't Playtime");
        assert!(
            game.last_played >= before && game.last_played.is_some(),
            "the play is recorded"
        );
    }

    /// In-game Saves are always written (FR-EMB-7): a real Core's save RAM round-trips through
    /// the same save path a session uses, and lands in a `saves/<system>/` temp dir. Of mGBA,
    /// SameBoy, Mesen and bsnes running their 240p/144p/160p Test Suite Game, only mGBA's gba
    /// Game has save RAM.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn in_game_save_round_trips_through_a_real_core() {
        let _serial = serial();
        let data = Path::new(TESTDATA);
        let core_path = data.join("data/cores/mgba_libretro.so");
        let game_path = data.join("games/gba/160p Test Suite (World).gba");
        let root = TempDir::new().unwrap();
        let dirs = CoreDirs {
            system: root.path().join("bios"),
            save: saves::save_dir(root.path(), "gba"),
        };
        fs::create_dir_all(&dirs.system).unwrap();
        fs::create_dir_all(&dirs.save).unwrap();

        let mut core = Core::load(&core_path, &game_path, dirs.clone(), Default::default(), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let ram = core.save_ram().expect("mGBA's Test Suite Game has save RAM");
        let known: Vec<u8> = (0..ram.len()).map(|i| i as u8).collect();
        ram.copy_from_slice(&known);

        let srm = saves::srm_path(root.path(), "gba", Path::new("160p Test Suite (World).gba"));
        let mut tracker = Tracker::new(core.save_ram().unwrap());
        write_save(&srm, &mut tracker, core.save_ram().unwrap());
        assert!(srm.exists(), "the .srm exists in saves/gba/");
        assert_eq!(srm.parent().unwrap(), saves::save_dir(root.path(), "gba"));
        drop(core);

        let mut core = Core::load(&core_path, &game_path, dirs, Default::default(), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let ram = core.save_ram().expect("mGBA's Test Suite Game has save RAM");
        assert!(saves::load(&srm, ram).unwrap());
        assert_eq!(ram, known.as_slice());
    }

    /// A Core that can't load a Game and says why: the player reads its words, not only that the
    /// Game couldn't start (FR-EMB-14).
    #[test]
    fn a_core_that_says_why_it_cannot_start_a_game_is_quoted_to_the_player() {
        let _serial = serial();
        let dir = TempDir::new().unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = dir.path().join("Games");
        ctx.data_dir = dir.path().join("data");
        // The fake Core stands in for the NES's.
        let core_path = cores::installed_path(&ctx.data_dir, &cores::MESEN);
        fs::create_dir_all(core_path.parent().unwrap()).unwrap();
        fs::rename(crate::libretro::tests::fake_core(dir.path()), &core_path).unwrap();
        let game = GameRow {
            id: 1,
            system: "nes".into(),
            rel_path: "nes/Game.nes".into(),
            title: "Game".into(),
            date_added: 0,
            last_played: None,
            playtime_secs: 0,
            favorite: false,
        };
        let play = Play {
            game,
            start: Start::PowerOn,
        };
        match Session::start(&play, &ctx, None, None, None) {
            Err(e) => {
                assert_eq!(e.title, launch::FAILED_TITLE);
                assert_eq!(
                    e.message,
                    format!(
                        "Mesen couldn't start Game.\n{}",
                        crate::libretro::tests::FAKE_MESSAGE
                    )
                )
            }
            Ok(_) => panic!("the fake Core loads no Game"),
        }
    }

    /// A DS Game starts melonDS DS in the layout chosen for DS in Settings → Display, Large and small
    /// until one is chosen (FR-DS-1); 3DS's choice is its own.
    #[test]
    fn a_ds_game_starts_in_the_layout_chosen_in_settings() {
        use crate::cores::ScreenLayout;

        let mut ctx = Ctx::empty();
        let (nds, three_ds) = (systems::by_id("nds").unwrap(), systems::by_id("3ds").unwrap());
        let game = Path::new("/Games/nds/Game.nds");
        let options = |ctx: &Ctx| core_options(ctx, nds, &cores::MELONDS_DS, game);
        assert_eq!(options(&ctx), cores::MELONDS_DS.options);
        for layout in [
            ScreenLayout::SideBySide,
            ScreenLayout::OneScreen,
            ScreenLayout::LargeAndSmall,
        ] {
            ctx.set_screen_layout(nds, layout);
            ctx.set_screen_layout(three_ds, layout.next());
            assert_eq!(
                options(&ctx),
                cores::MELONDS_DS.options_for(nds, game, layout),
                "{layout:?}"
            );
        }
        ctx.set_screen_layout(nds, ScreenLayout::SideBySide);
        assert!(options(&ctx).contains(&("melonds_screen_layout1", "left-right")));
    }

    /// A Wii Game starts Dolphin with the controller chosen for it in its Options Menu: the Wii
    /// Remote and Nunchuk until set, a Classic Controller once set, while another Wii Game keeps
    /// its own; a GameCube Game gets a RetroPad (FR-NEW-3).
    #[test]
    fn a_wii_game_starts_with_the_controller_chosen_for_it() {
        use crate::cores::{DOLPHIN, DOLPHIN_CLASSIC_CONTROLLER, DOLPHIN_WII_REMOTE_AND_NUNCHUK};
        use crate::library::FoundGame;
        use crate::library::db::SortOrder;

        let mut ctx = Ctx::empty();
        let found = |system: &'static str, title: &str| FoundGame {
            system,
            rel_path: format!("{system}/{title}.iso"),
            title: title.to_owned(),
        };
        let mut games = |system: &'static str, titles: &[&str]| {
            let found: Vec<_> = titles.iter().map(|t| found(system, t)).collect();
            ctx.library.sync_system(system, &found, 0).unwrap();
            ctx.library.system_games(system, SortOrder::AZ).unwrap()
        };
        let wii_games = games("wii", &["Kart", "Other"]);
        let gc_game = games("gc", &["Cube"]).remove(0);
        let (kart, other) = (&wii_games[0], &wii_games[1]);
        let (wii, gc) = (systems::by_id("wii").unwrap(), systems::by_id("gc").unwrap());
        assert_eq!(
            port_device(&ctx, wii, &DOLPHIN, kart),
            DOLPHIN_WII_REMOTE_AND_NUNCHUK
        );

        ctx.library
            .set_wii_controller(kart.id, WiiController::ClassicController)
            .unwrap();
        assert_eq!(port_device(&ctx, wii, &DOLPHIN, kart), DOLPHIN_CLASSIC_CONTROLLER);
        assert_eq!(
            port_device(&ctx, wii, &DOLPHIN, other),
            DOLPHIN_WII_REMOTE_AND_NUNCHUK
        );
        assert_eq!(port_device(&ctx, gc, &DOLPHIN, &gc_game), PortDevice::RETROPAD);
    }

    /// A PS2 Game, `ps2/<file>`, on a Games Root.
    fn ps2_game(file: &str) -> GameRow {
        GameRow {
            id: 1,
            system: "ps2".into(),
            rel_path: format!("ps2/{file}"),
            title: "wLaunchELF".into(),
            date_added: 0,
            last_played: None,
            playtime_secs: 0,
            favorite: false,
        }
    }

    /// Without its BIOS a PS2 session doesn't start, and says where the BIOS goes, under the
    /// BIOS rule's own title (not the generic "Couldn't start the game"), before it loads any
    /// Core: LRPS2 itself only logs that it found none (FR-EMB-12, FR-NEW-1).
    #[test]
    fn a_ps2_session_without_its_bios_names_the_bios_folder() {
        let dir = TempDir::new().unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = dir.path().join("Games");
        ctx.data_dir = dir.path().join("data");
        fs::create_dir_all(cores::PS2_BIOS.dir(&ctx.games_root)).unwrap();
        let play = Play::resume(ps2_game("Game.iso"));
        match Session::start(&play, &ctx, None, None, None) {
            Err(e) => {
                assert_eq!(e.title, "PlayStation 2 BIOS missing");
                assert_eq!(
                    e.message,
                    launch::missing_bios_message(&ctx.games_root, &cores::PS2_BIOS)
                )
            }
            Ok(_) => panic!("no PS2 session without a BIOS"),
        }
    }

    /// The opengl renderer of a hidden offscreen window, as `app.rs` makes it. SDL must outlive
    /// the canvas.
    fn offscreen_renderer() -> (sdl3::Sdl, sdl3::render::Canvas<Window>) {
        sdl3::hint::set("SDL_VIDEO_DRIVER", "offscreen");
        let sdl = sdl3::init().unwrap();
        let video = sdl.video().unwrap();
        let window = video.window("hw", 1280, 720).hidden().build().unwrap();
        let mut canvas = sdl3::render::create_renderer(window, Some(c"opengl")).unwrap();
        // As the app's window shows it (`app::canvas`): Game37 draws in logical 1920×1080.
        canvas
            .set_logical_size(
                1920,
                1080,
                sdl3::sys::render::SDL_RendererLogicalPresentation::LETTERBOX,
            )
            .unwrap();
        (sdl, canvas)
    }

    /// A PS2 Games Root holding wLaunchELF, and a data dir with the test data's Cores.
    fn ps2_world() -> (TempDir, TempDir, Ctx, &'static str) {
        let elf = "wLaunchELF (Homebrew).elf";
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("ps2")).unwrap();
        fs::copy(
            Path::new(TESTDATA).join("games/ps2").join(elf),
            root.path().join("ps2").join(elf),
        )
        .unwrap();
        let data = TempDir::new().unwrap();
        std::os::unix::fs::symlink(Path::new(TESTDATA).join("data/cores"), data.path().join("cores"))
            .unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = root.path().to_path_buf();
        ctx.data_dir = data.path().to_path_buf();
        (root, data, ctx, elf)
    }

    /// The distinct colours of what `gfx` shows, read back from the renderer.
    fn shown_colours(gfx: &mut Gfx<'_>, at: &Path) -> std::collections::HashSet<Vec<u8>> {
        use sdl3::surface::Surface;
        gfx.save_bmp(at).unwrap();
        let shot = Surface::load_bmp(at).unwrap();
        let pitch = shot.pitch() as usize;
        let bytes = shot.pixel_format().bytes_per_pixel();
        shot.with_lock(|pixels| {
            pixels
                .chunks(pitch)
                .flat_map(|row| row[..shot.width() as usize * bytes].chunks(bytes))
                .map(<[u8]>::to_vec)
                .collect()
        })
    }

    /// LRPS2 with no BIOS: a PS2 session says where the BIOS goes, and the real Core, made to
    /// load anyway on the SDL OpenGL host with an empty BIOS folder, fails to load the Game
    /// without taking Game37's process down. It says nothing to the player itself, so Game37's
    /// message is the one shown.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn hw_lrps2_without_a_bios_fails_to_start_with_the_ps2_bios_message() {
        use crate::ui::text::Fonts;
        use crate::ui::theme::Colour;

        let _serial = serial();
        let (root, data, ctx, elf) = ps2_world();
        fs::create_dir_all(cores::PS2_BIOS.dir(root.path())).unwrap();
        let (_sdl, canvas) = offscreen_renderer();
        let creator = canvas.texture_creator();
        let mut gfx = Gfx::new(canvas, &creator, Fonts::new().unwrap());

        let play = Play::resume(ps2_game(elf));
        match Session::start(&play, &ctx, gfx.gl_context(), None, Some(60.0)) {
            Err(e) => {
                assert_eq!(e.title, "PlayStation 2 BIOS missing");
                assert_eq!(
                    e.message,
                    launch::missing_bios_message(root.path(), &cores::PS2_BIOS)
                )
            }
            Ok(_) => panic!("no PS2 session without a BIOS"),
        }

        let gl = open_gl(&cores::LRPS2, gfx.gl_context(), None, "PlayStation 2").unwrap();
        assert!(gl.is_some(), "LRPS2 gets the SDL OpenGL host");
        let dirs = CoreDirs {
            system: cores::system_dir(&cores::LRPS2, data.path(), root.path()),
            save: saves::save_dir(root.path(), "ps2"),
        };
        fs::create_dir_all(&dirs.save).unwrap();
        let game = root.path().join("ps2").join(elf);
        let options = core_options(&ctx, systems::by_id("ps2").unwrap(), &cores::LRPS2, &game);
        let core_path = cores::installed_path(data.path(), &cores::LRPS2);
        // Into the address window, as a session loads it: once loaded anywhere else, LRPS2's image
        // stays there, and a later PS2 test in this process may find no room for its maps.
        let setup = CoreSetup {
            options: &options,
            address_window: cores::LRPS2.needs_address_window(),
            ..Default::default()
        };
        match Core::load(&core_path, &game, dirs, setup, gl) {
            Err(e) => eprintln!("LRPS2 without a BIOS: {e}; it said {:?}", e.core_message()),
            Ok(_) => panic!("LRPS2 loaded a Game without a BIOS"),
        }

        // Game37 is still here, and its renderer still draws.
        gfx.clear(Colour::hex(0x204060));
        let colours = shown_colours(&mut gfx, &data.path().join("shot.bmp"));
        assert_eq!(colours.len(), 1, "one fill colour: {colours:?}");
    }

    /// Draws `session`'s Game on a black screen; returns the distinct colours the renderer shows.
    fn shown(
        session: &mut Session,
        gfx: &mut Gfx<'_>,
        ctx: &Ctx,
        at: &Path,
    ) -> std::collections::HashSet<Vec<u8>> {
        gfx.clear(crate::ui::theme::Colour::hex(0x000000));
        session.draw(gfx, ctx);
        shown_colours(gfx, at)
    }

    /// Draws `session`'s Game on a black screen; returns the exact picture, as the bytes of the
    /// `.bmp` file the renderer wrote. Unlike [`shown`]'s set of distinct colours, two pictures
    /// with the same palette but different content (e.g. a moved menu cursor) compare unequal.
    fn shown_exactly(session: &mut Session, gfx: &mut Gfx<'_>, ctx: &Ctx, at: &Path) -> Vec<u8> {
        gfx.clear(crate::ui::theme::Colour::hex(0x000000));
        session.draw(gfx, ctx);
        gfx.save_bmp(at).unwrap();
        fs::read(at).unwrap()
    }

    /// Runs `n` ticks of a 60 Hz display, with no input.
    fn tick_n(session: &mut Session, gfx: &mut Gfx<'_>, n: usize) {
        for _ in 0..n {
            assert!(!session.tick(gfx, 1.0 / 60.0, &[], Mouse::default(), |_| false));
        }
    }

    /// Several colours, not all black: a picture.
    fn assert_a_picture(colours: &std::collections::HashSet<Vec<u8>>, when: &str) {
        assert!(
            colours.len() >= 4 && colours.iter().any(|c| c[..3].iter().any(|&b| b > 0)),
            "{when}: the renderer shows no picture: {colours:?}"
        );
    }

    /// One press of the Down key: held for [`PRESS_TICKS`] ticks, released, then
    /// [`SETTLE_TICKS`] ticks for wLaunchELF's menu to settle, all at 60 Hz.
    fn press_down(session: &mut Session, gfx: &mut Gfx<'_>) {
        for _ in 0..PRESS_TICKS {
            assert!(!session.tick(gfx, 1.0 / 60.0, &[], Mouse::default(), |s| s == Scancode::Down));
        }
        tick_n(session, gfx, SETTLE_TICKS);
    }
    const PRESS_TICKS: usize = 4;
    const SETTLE_TICKS: usize = 30;

    /// Links the owner's PS2 BIOS into the Games Root `root`. Only the BIOS dump(s) are
    /// symlinked, into a temp `bios/pcsx2/bios/`: LRPS2 writes `.nvm`/`.mec` and other folders
    /// beside the dump, under its `pcsx2/` system directory, and those must land in `root`, never
    /// in `.testdata` itself.
    fn link_ps2_bios(root: &Path) {
        let bios_dir = root.join("bios/pcsx2/bios");
        fs::create_dir_all(&bios_dir).unwrap();
        for entry in fs::read_dir(Path::new(TESTDATA).join("bios/pcsx2/bios")).unwrap() {
            let entry = entry.unwrap();
            std::os::unix::fs::symlink(entry.path(), bios_dir.join(entry.file_name())).unwrap();
        }
        assert!(
            cores::PS2_BIOS.found(root).is_some(),
            "a PS2 BIOS dump of 4 to 8 MiB"
        );
    }

    /// The owner's PS2 BIOS (FR-NEW-1): wLaunchELF boots on LRPS2 through a PS2 session beside
    /// the opengl renderer, and its picture shows on the renderer. Save State slot 1 is saved at
    /// the menu's starting position; one Down press from there gives the picture `one_down`, and
    /// a second press moves the highlight on. Loading slot 1, slot 1 in a new session, and the
    /// Resume State (written straight after that load) are each followed by the same one press,
    /// which must give `one_down` again: a load that didn't restore would leave the highlight a
    /// press further on.
    ///
    /// The press is there because, after a load, LRPS2's OpenGL renderer shows nothing new until
    /// the Game draws: `GSState::Defrost` empties the texture cache (`Reset(true)` →
    /// `GSTextureCache::RemoveAll`), and `GSTextureCache::LookupDisplayTarget` only makes a display
    /// target again after a fresh EE→GS transfer. wLaunchELF only redraws on input, so without the
    /// press every frame after a load is a dupe and Game37 keeps showing the pre-load picture (a
    /// Known issue). Frames run with LRPS2's fastmem on (its SIGSEGV handler beside Rust's).
    #[test]
    #[ignore = "needs a PS2 BIOS dump in /home/matei/Game37/.testdata/bios/pcsx2/bios"]
    fn needs_ps2_bios_lrps2_boots_wlaunchelf_on_the_renderer_and_states_round_trip() {
        use crate::ui::text::Fonts;

        let _serial = serial();
        let (root, data, ctx, elf) = ps2_world();
        link_ps2_bios(root.path());
        let (_sdl, canvas) = offscreen_renderer();
        let creator = canvas.texture_creator();
        let mut gfx = Gfx::new(canvas, &creator, Fonts::new().unwrap());
        let shot = data.path().join("shot.bmp");
        let game = ps2_game(elf);

        let mut session = Session::start(
            &Play::resume(game.clone()),
            &ctx,
            gfx.gl_context(),
            None,
            Some(60.0),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        tick_n(&mut session, &mut gfx, 1200);
        let booted = shown_exactly(&mut session, &mut gfx, &ctx, &shot);
        assert_a_picture(&shown(&mut session, &mut gfx, &ctx, &shot), "booted");
        session.save_slot(1).unwrap_or_else(|e| panic!("save: {e}"));

        // The menu starts with no item highlighted; each press highlights the next of its three
        // items, wrapping around: one press highlights the first, a second the second.
        press_down(&mut session, &mut gfx);
        let one_down = shown_exactly(&mut session, &mut gfx, &ctx, &shot);
        assert!(
            one_down != booted,
            "one Down press highlights the menu's first item"
        );
        press_down(&mut session, &mut gfx);
        let moved = shown_exactly(&mut session, &mut gfx, &ctx, &shot);
        assert!(moved != one_down, "a second Down press moves the highlight on");

        // From `moved`, a load that didn't restore would highlight the third item, not the first.
        // The Resume State is written straight after the load, so it too holds the starting
        // position, and the session moves on from there before it ends.
        session.load_slot(1).unwrap_or_else(|e| panic!("load: {e}"));
        session.write_resume();
        let resume = session.states.resume();
        press_down(&mut session, &mut gfx);
        assert!(
            shown_exactly(&mut session, &mut gfx, &ctx, &shot) == one_down,
            "slot 1 brings back the menu's starting position"
        );
        drop(session);
        assert!(
            fs::metadata(&resume).is_ok_and(|m| m.len() > 0),
            "the Resume State"
        );

        let from_slot = Play {
            game,
            start: Start::Slot(1),
        };
        let mut session = Session::start(&from_slot, &ctx, gfx.gl_context(), None, Some(60.0))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(session.start_message, None, "slot 1 restored in a new session");
        press_down(&mut session, &mut gfx);
        assert!(
            shown_exactly(&mut session, &mut gfx, &ctx, &shot) == one_down,
            "slot 1 in a new session brings back the menu's starting position"
        );
        // The new session has moved on to the first item: the Resume State must take it back.
        let state = fs::read(&resume).unwrap();
        session
            .core
            .unserialize(&state)
            .unwrap_or_else(|e| panic!("resume: {e}"));
        press_down(&mut session, &mut gfx);
        assert!(
            shown_exactly(&mut session, &mut gfx, &ctx, &shot) == one_down,
            "the Resume State brings back the menu's starting position"
        );
    }

    /// Set in the child process [`needs_ps2_bios_lrps2_boots_after_azahar_dolphin_and_mupen64plus_next`]
    /// runs itself in.
    const PS2_AFTER_OTHERS_CHILD: &str = "GAME37_TEST_PS2_AFTER_OTHER_CORES";

    /// Kills a child process (by its handle, so by pid) that is still running when the test ends.
    struct KillOnDrop(std::process::Child);

    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            if let Ok(None) = self.0.try_wait() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

    /// The owner's PS2 BIOS: a PS2 Game started after Azahar, Dolphin on GameCube and
    /// Mupen64Plus-Next have each played a Game in the same process boots and shows its picture.
    /// Those Cores stay mapped (`RTLD_NODELETE`) beside a 1 GB reservation Mupen64Plus-Next never
    /// releases and the holes their maps leave, and LRPS2's library would go into one of those
    /// holes, with no room near it for the main and code maps its recompilers must reach: it then
    /// says it "could not place" them and aborts the process (every time, in 8 runs, without the
    /// address window). An abort ends every test of the process, so the Games play in a child
    /// process, this test run again, and the test fails when the child doesn't exit cleanly or
    /// LRPS2 says it couldn't place its maps.
    #[test]
    #[ignore = "needs a PS2 BIOS dump in /home/matei/Game37/.testdata/bios/pcsx2/bios"]
    fn needs_ps2_bios_lrps2_boots_after_azahar_dolphin_and_mupen64plus_next() {
        let _serial = serial();
        if std::env::var_os(PS2_AFTER_OTHERS_CHILD).is_some() {
            return ps2_after_other_cores();
        }
        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("child.log");
        let log = fs::File::create(&log_path).unwrap();
        let name = "needs_ps2_bios_lrps2_boots_after_azahar_dolphin_and_mupen64plus_next";
        let path = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", &path, "--nocapture", "--test-threads=1"])
            .env(PS2_AFTER_OTHERS_CHILD, "1")
            // The child's temp dirs go in this test's, so an abort can't leave them behind.
            .env("TMPDIR", dir.path())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let mut child = KillOnDrop(child);
        let deadline = Instant::now() + Duration::from_secs(600);
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "the child still runs after 10 minutes");
            std::thread::sleep(Duration::from_millis(100));
        };
        let out = fs::read_to_string(&log_path).unwrap_or_default();
        let tail: Vec<_> = out
            .lines()
            .rev()
            .take(30)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let tail = tail.join("\n");
        assert!(
            !out.contains("Could not place"),
            "LRPS2 couldn't place its maps:\n{tail}"
        );
        assert!(
            !out.contains("outside its address window") && !out.contains("no address window"),
            "LRPS2 was loaded without its address window:\n{tail}"
        );
        assert!(status.success(), "the child process ended with {status}:\n{tail}");
        assert!(out.contains("1 passed"), "the child ran the test:\n{tail}");
    }

    /// Plays ftpd on Azahar, FCE Ultra GX on Dolphin (GameCube) and N64 NICCC on
    /// Mupen64Plus-Next for 300 frames each, then boots wLaunchELF on LRPS2 and checks its picture.
    fn ps2_after_other_cores() {
        use crate::ui::text::Fonts;

        let (_sdl, canvas) = offscreen_renderer();
        let creator = canvas.texture_creator();
        let mut gfx = Gfx::new(canvas, &creator, Fonts::new().unwrap());
        let n64_world = || {
            let file = "N64 NICCC (World).n64";
            let root = TempDir::new().unwrap();
            fs::create_dir_all(root.path().join("n64")).unwrap();
            fs::copy(
                Path::new(TESTDATA).join("games/n64").join(file),
                root.path().join("n64").join(file),
            )
            .unwrap();
            let data = TempDir::new().unwrap();
            std::os::unix::fs::symlink(Path::new(TESTDATA).join("data/cores"), data.path().join("cores"))
                .unwrap();
            let mut ctx = Ctx::empty();
            ctx.games_root = root.path().to_path_buf();
            ctx.data_dir = data.path().to_path_buf();
            let game = GameRow {
                system: "n64".into(),
                rel_path: format!("n64/{file}"),
                title: "N64 NICCC".into(),
                ..ps2_game(file)
            };
            (root, data, ctx, game)
        };
        for (root, data, ctx, game) in [three_ds_world(), dolphin_world("gc"), n64_world()] {
            let mut session = Session::start(
                &Play::resume(game.clone()),
                &ctx,
                gfx.gl_context(),
                None,
                Some(60.0),
            )
            .unwrap_or_else(|e| panic!("{}: {e}", game.system));
            tick_n(&mut session, &mut gfx, 300);
            drop(session);
            drop((root, data));
        }

        let (root, data, ctx, elf) = ps2_world();
        link_ps2_bios(root.path());
        let mut session = Session::start(
            &Play::resume(ps2_game(elf)),
            &ctx,
            gfx.gl_context(),
            None,
            Some(60.0),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        tick_n(&mut session, &mut gfx, 1200);
        assert_a_picture(
            &shown(&mut session, &mut gfx, &ctx, &data.path().join("shot.bmp")),
            "booted after the other Cores",
        );
    }

    /// A 3DS Games Root holding ftpd, and a data dir with the test data's Cores.
    fn three_ds_world() -> (TempDir, TempDir, Ctx, GameRow) {
        let file = "ftpd (Homebrew).3dsx";
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("3ds")).unwrap();
        fs::copy(
            Path::new(TESTDATA).join("games/3ds").join(file),
            root.path().join("3ds").join(file),
        )
        .unwrap();
        let data = TempDir::new().unwrap();
        std::os::unix::fs::symlink(Path::new(TESTDATA).join("data/cores"), data.path().join("cores"))
            .unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = root.path().to_path_buf();
        ctx.data_dir = data.path().to_path_buf();
        let game = GameRow {
            id: 1,
            system: "3ds".into(),
            rel_path: format!("3ds/{file}"),
            title: "ftpd".into(),
            date_added: 0,
            last_played: None,
            playtime_secs: 0,
            favorite: false,
        };
        (root, data, ctx, game)
    }

    /// Azahar's 1920×960 frame with the large/small layout at proportion 3
    /// (`cores::THREE_DS_LAYOUTS.large_and_small`): the
    /// large screen's inside, `(x, y, w, h)`, top left, and the small one's, bottom right, each
    /// a few pixels in from its edges.
    const AZAHAR_LARGE: (usize, usize, usize, usize) = (8, 8, 1500, 895);
    const AZAHAR_SMALL: (usize, usize, usize, usize) = (1524, 640, 388, 287);

    /// The RGB bytes of `area` of an `Xrgb8888` or `Rgba32` frame, sampled on a 96 × 72 grid, so
    /// that a large and a small screen compare.
    fn sampled(frame: &Frame, (x, y, w, h): (usize, usize, usize, usize)) -> Vec<u8> {
        let bpp = frame.format.bytes_per_pixel();
        let mut out = Vec::with_capacity(96 * 72 * 3);
        for row in 0..72 {
            let line = &frame.data[(y + row * h / 72) * frame.pitch..];
            for col in 0..96 {
                out.extend_from_slice(&line[(x + col * w / 96) * bpp..][..3]);
            }
        }
        out
    }

    /// The mean difference per colour byte of two samples.
    fn difference(a: &[u8], b: &[u8]) -> f64 {
        let sum: u64 = a.iter().zip(b).map(|(x, y)| u64::from(x.abs_diff(*y))).sum();
        sum as f64 / a.len() as f64
    }

    /// Whether the swap put what the small screen showed in `before` on the large one now: the large
    /// screen looks more like the old small one than like the old large one.
    fn swapped(before: &Frame, now: &Frame) -> bool {
        let now_large = sampled(now, AZAHAR_LARGE);
        let (to_small, to_large) = (
            difference(&now_large, &sampled(before, AZAHAR_SMALL)),
            difference(&now_large, &sampled(before, AZAHAR_LARGE)),
        );
        eprintln!(
            "the large screen differs from the old small one by {to_small}, the old large one by {to_large}"
        );
        to_small < to_large
    }

    /// 3DS with Azahar (FR-NEW-2): ftpd boots through a 3DS session on the SDL OpenGL host and its
    /// picture shows on the renderer; one large and one small screen in a 2:1 frame. The mouse on
    /// the small screen reaches Azahar as `RETRO_DEVICE_POINTER`; a pad's L3 reaches it as R3, the
    /// tap of its right-stick cursor; a pad's R3 swaps the screens and Tab swaps them back,
    /// through `citra_swap_screen`, and Azahar never sees L3, its HOME button, pressed. Save State
    /// slot 1 saves and loads, and slot 1 and the Resume State restore in the next session (after
    /// its first frame, for Azahar's `MUST_INITIALIZE`).
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn hw_azahar_boots_ftpd_on_the_renderer_touches_swaps_and_states_round_trip() {
        // RETRO_DEVICE_POINTER, and its ids for x, y and the press.
        const POINTER: u32 = 6;
        const X: u32 = 0;
        const Y: u32 = 1;
        const PRESSED: u32 = 2;
        // RETRO_DEVICE_JOYPAD, and its ids for L3 and R3.
        const JOYPAD: u32 = 1;
        const L3: u32 = 14;
        const R3: u32 = 15;
        use crate::ui::text::Fonts;

        let _serial = serial();
        let (_root, data, ctx, game) = three_ds_world();
        let (_sdl, canvas) = offscreen_renderer();
        let creator = canvas.texture_creator();
        let mut gfx = Gfx::new(canvas, &creator, Fonts::new().unwrap());
        let shot = data.path().join("shot.bmp");
        let mut session = Session::start(
            &Play::resume(game.clone()),
            &ctx,
            gfx.gl_context(),
            None,
            Some(60.0),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(session.touch_screen(), "the mouse cursor shows");
        tick_n(&mut session, &mut gfx, 600);
        assert_a_picture(&shown(&mut session, &mut gfx, &ctx, &shot), "booted");
        let frame = session.core.frame();
        assert_eq!((frame.width, frame.height), (1920, 960), "large and small at 3");

        // The mouse pressed at the small screen's centre, in Game37's logical coordinates.
        let picture = timing::fit(session.core.av_info().aspect_ratio);
        let (x, y, w, h) = AZAHAR_SMALL;
        let at = (
            picture.x + (x + w / 2) as f32 * picture.w / 1920.0,
            picture.y + (y + h / 2) as f32 * picture.h / 960.0,
        );
        let mouse = Mouse {
            at: Some(at),
            left: true,
        };
        let pointer = input::touch(picture, mouse).expect("on the picture");
        for _ in 0..10 {
            assert!(!session.tick(&mut gfx, 1.0 / 60.0, &[], mouse, |_| false));
        }
        let queries = session.core.input_queries();
        let asked = |id| queries.get(&(0, POINTER, 0, id)).copied();
        assert_eq!(asked(PRESSED), Some(1), "the press");
        assert_eq!(asked(X), Some(pointer.x), "x");
        assert_eq!(asked(Y), Some(pointer.y), "y");
        tick_n(&mut session, &mut gfx, 30);

        // Azahar asked for L3 and R3; L3 is never pressed, whatever the player holds.
        let joypad = |session: &Session, id| session.core.input_queries().get(&(0, JOYPAD, 0, id)).copied();
        assert_eq!(joypad(&session, R3), Some(0), "Azahar reads R3, its touch tap");
        assert_eq!(joypad(&session, L3), Some(0), "and L3, its swap and HOME");
        let mut l3 = PadState::default();
        l3.press(sdl3::gamepad::Button::LeftStick);
        for _ in 0..3 {
            session.tick(&mut gfx, 1.0 / 60.0, &[(0, l3)], Mouse::default(), |_| false);
        }
        assert_eq!(joypad(&session, R3), Some(1), "a pad's L3 taps, as R3");
        assert_eq!(joypad(&session, L3), Some(0), "and is not L3");
        tick_n(&mut session, &mut gfx, 30);

        // R3 on a pad, then Tab: each swaps which screen is large.
        let before = session.core.frame().clone();
        let mut r3 = PadState::default();
        r3.press(sdl3::gamepad::Button::RightStick);
        for _ in 0..5 {
            session.tick(&mut gfx, 1.0 / 60.0, &[(0, r3)], Mouse::default(), |_| false);
            assert_eq!(
                (joypad(&session, R3), joypad(&session, L3)),
                (Some(0), Some(0)),
                "R3 reaches Azahar as neither"
            );
        }
        tick_n(&mut session, &mut gfx, 30);
        let after_r3 = session.core.frame().clone();
        assert!(
            swapped(&before, &after_r3),
            "R3 put the touch screen on the large one"
        );
        let tab = |key| key == input::SWAP_SCREENS_KEY;
        for _ in 0..5 {
            session.tick(&mut gfx, 1.0 / 60.0, &[], Mouse::default(), tab);
            assert_eq!(
                (joypad(&session, R3), joypad(&session, L3)),
                (Some(0), Some(0)),
                "Tab reaches Azahar as neither"
            );
        }
        tick_n(&mut session, &mut gfx, 30);
        assert!(
            swapped(&after_r3, session.core.frame()),
            "Tab put the top screen back on the large one"
        );

        session.save_slot(1).unwrap_or_else(|e| panic!("save: {e}"));
        tick_n(&mut session, &mut gfx, 60);
        session.load_slot(1).unwrap_or_else(|e| panic!("load: {e}"));
        tick_n(&mut session, &mut gfx, 60);
        assert_a_picture(&shown(&mut session, &mut gfx, &ctx, &shot), "loaded");
        session.write_resume();
        let resume = session.states.resume();
        drop(session);
        assert!(
            fs::metadata(&resume).is_ok_and(|m| m.len() > 0),
            "the Resume State"
        );

        let from_slot = Play {
            game,
            start: Start::Slot(1),
        };
        let mut session = Session::start(&from_slot, &ctx, gfx.gl_context(), None, Some(60.0))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(session.start_message, None, "slot 1 restored in a new session");
        tick_n(&mut session, &mut gfx, 60);
        assert_a_picture(&shown(&mut session, &mut gfx, &ctx, &shot), "from slot 1");
        // The new session has moved on to the first item: the Resume State must take it back.
        let state = fs::read(&resume).unwrap();
        session
            .core
            .unserialize(&state)
            .unwrap_or_else(|e| panic!("resume: {e}"));
        tick_n(&mut session, &mut gfx, 60);
        assert_a_picture(&shown(&mut session, &mut gfx, &ctx, &shot), "resumed");
    }

    /// 3DS swaps its screens in Side by side and One screen too (FR-DS-2, FR-NEW-2), not only
    /// Large and small: Side by side (720×240, the top screen's 400 width beside the touch
    /// screen's 320) puts the touch screen on the left; One screen (either screen at its own
    /// size) shows the other one, through `citra_swap_screen`, checked on the real Core's
    /// picture, and Tab swaps each back.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn hw_azahar_swaps_the_screens_in_side_by_side_and_one_screen_layouts() {
        use crate::cores::ScreenLayout;
        use crate::ui::text::Fonts;

        let _serial = serial();
        let press_r3 = |session: &mut Session, gfx: &mut Gfx<'_>| {
            let mut r3 = PadState::default();
            r3.press(sdl3::gamepad::Button::RightStick);
            for _ in 0..5 {
                session.tick(gfx, 1.0 / 60.0, &[(0, r3)], Mouse::default(), |_| false);
            }
            tick_n(session, gfx, 30);
        };
        let press_tab = |session: &mut Session, gfx: &mut Gfx<'_>| {
            let tab = |key| key == input::SWAP_SCREENS_KEY;
            for _ in 0..5 {
                session.tick(gfx, 1.0 / 60.0, &[], Mouse::default(), tab);
            }
            tick_n(session, gfx, 30);
        };

        // Side by side: R3 puts the touch screen (the old right half) on the left; Tab swaps it
        // back.
        {
            let (_root, _data, mut ctx, game) = three_ds_world();
            ctx.set_screen_layout(systems::by_id("3ds").unwrap(), ScreenLayout::SideBySide);
            let (_sdl, canvas) = offscreen_renderer();
            let creator = canvas.texture_creator();
            let mut gfx = Gfx::new(canvas, &creator, Fonts::new().unwrap());
            let mut session = Session::start(&Play::resume(game), &ctx, gfx.gl_context(), None, Some(60.0))
                .unwrap_or_else(|e| panic!("{e}"));
            tick_n(&mut session, &mut gfx, 600);
            let before = session.core.frame().clone();
            assert_eq!(
                (before.width, before.height),
                (720, 240),
                "the top and touch screens side by side"
            );
            let right_before = sampled(&before, (400, 0, 320, 240));

            press_r3(&mut session, &mut gfx);
            let after = session.core.frame().clone();
            assert_eq!((after.width, after.height), (720, 240), "still side by side");
            let (left_after, left_before) = (
                sampled(&after, (0, 0, 400, 240)),
                sampled(&before, (0, 0, 400, 240)),
            );
            assert!(
                difference(&left_after, &right_before) < difference(&left_after, &left_before),
                "R3 put the touch screen on the left"
            );

            press_tab(&mut session, &mut gfx);
            let back = sampled(session.core.frame(), (0, 0, 400, 240));
            assert!(
                difference(&back, &left_before) < difference(&back, &right_before),
                "Tab put the top screen back on the left"
            );
        }

        // One screen: R3 shows the other screen, at its own size (400×240 for the top, 320×240
        // for the touch screen); Tab shows the first one again.
        {
            let (_root, _data, mut ctx, game) = three_ds_world();
            ctx.set_screen_layout(systems::by_id("3ds").unwrap(), ScreenLayout::OneScreen);
            let (_sdl, canvas) = offscreen_renderer();
            let creator = canvas.texture_creator();
            let mut gfx = Gfx::new(canvas, &creator, Fonts::new().unwrap());
            let mut session = Session::start(&Play::resume(game), &ctx, gfx.gl_context(), None, Some(60.0))
                .unwrap_or_else(|e| panic!("{e}"));
            tick_n(&mut session, &mut gfx, 600);
            let before = session.core.frame().clone();
            assert_eq!((before.width, before.height), (400, 240), "the top screen alone");

            press_r3(&mut session, &mut gfx);
            let after = session.core.frame().clone();
            assert_eq!(
                (after.width, after.height),
                (320, 240),
                "R3 shows the touch screen, at its own size"
            );

            press_tab(&mut session, &mut gfx);
            let back = session.core.frame().clone();
            assert_eq!(
                (back.width, back.height),
                (400, 240),
                "Tab shows the top screen again"
            );
        }
    }

    /// An encrypted 3DS dump (FR-NEW-2, FR-EMB-14): Azahar refuses it, and the message the player
    /// sees quotes Azahar saying it must be decrypted. The dump is a made-up NCCH header whose
    /// "no crypto" flag is clear, as on a dump straight off a cartridge.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn hw_azahar_refuses_an_encrypted_dump_and_says_it_must_be_decrypted() {
        use crate::ui::text::Fonts;

        let _serial = serial();
        let (root, _data, ctx, mut game) = three_ds_world();
        let mut ncch = vec![0u8; 0x4000];
        ncch[0x100..0x104].copy_from_slice(b"NCCH");
        // The content size, in 0x200-byte media units, and the extended header's size.
        ncch[0x104..0x108].copy_from_slice(&0x20u32.to_le_bytes());
        ncch[0x180..0x184].copy_from_slice(&0x400u32.to_le_bytes());
        // An executable (content type 0x02); flags[7] without 0x04, so encrypted.
        ncch[0x18D] = 0x02;
        fs::write(root.path().join("3ds/Encrypted.cxi"), &ncch).unwrap();
        game.rel_path = "3ds/Encrypted.cxi".into();
        game.title = "Encrypted".into();
        let (_sdl, canvas) = offscreen_renderer();
        let creator = canvas.texture_creator();
        let gfx = Gfx::new(canvas, &creator, Fonts::new().unwrap());

        match Session::start(&Play::resume(game), &ctx, gfx.gl_context(), None, Some(60.0)) {
            Err(e) => {
                eprintln!("{e}");
                assert_eq!(e.title, launch::FAILED_TITLE);
                assert!(
                    e.message.starts_with("Azahar couldn't start Encrypted."),
                    "{}",
                    e.message
                );
                assert!(e.message.contains("must be decrypted"), "{}", e.message);
            }
            Ok(_) => panic!("Azahar started an encrypted dump"),
        }
    }

    /// A Games Root holding FCE Ultra GX for `system` (GameCube or Wii), and a data dir with the
    /// test data's Cores and Dolphin's system files, where [`cores::DOLPHIN_ASSETS`] unpack them.
    /// Only `Sys` is linked: Dolphin writes into its `User/` folder in the Games Root's saves.
    /// Dolphin keeps the system directory of its first load in a process, so in a test run after
    /// another Dolphin test, `Sys` is that test's, by then deleted; FCE Ultra GX boots without it.
    fn dolphin_world(system: &str) -> (TempDir, TempDir, Ctx, GameRow) {
        let file = "FCE Ultra GX (Homebrew).dol";
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join(system)).unwrap();
        fs::copy(
            Path::new(TESTDATA).join("games").join(system).join(file),
            root.path().join(system).join(file),
        )
        .unwrap();
        let data = TempDir::new().unwrap();
        std::os::unix::fs::symlink(Path::new(TESTDATA).join("data/cores"), data.path().join("cores"))
            .unwrap();
        let marker = cores::assets_marker(data.path(), &cores::DOLPHIN_ASSETS);
        fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(Path::new(TESTDATA).join("data/system/dolphin-emu/Sys"), &marker).unwrap();
        assert!(cores::is_installed(data.path(), &cores::DOLPHIN));
        let mut ctx = Ctx::empty();
        ctx.games_root = root.path().to_path_buf();
        ctx.data_dir = data.path().to_path_buf();
        let game = GameRow {
            id: 1,
            system: system.into(),
            rel_path: format!("{system}/{file}"),
            title: "FCE Ultra GX".into(),
            date_added: 0,
            last_played: None,
            playtime_secs: 0,
            favorite: false,
        };
        (root, data, ctx, game)
    }

    /// Runs `n` frames with `input` on `session`'s Core directly, as `tick` would; returns the
    /// audio samples they brought.
    fn frames_with(session: &mut Session, input: &crate::libretro::InputState, n: usize) -> Vec<i16> {
        let mut audio = Vec::new();
        for _ in 0..n {
            audio.extend_from_slice(session.core.run_frame(input).audio);
        }
        audio
    }

    /// Holds `pad` on port 0 for 10 ticks, then lets go for 50, so FCE Ultra GX's menu settles.
    fn press(session: &mut Session, gfx: &mut Gfx<'_>, pad: PadState) {
        for _ in 0..10 {
            assert!(!session.tick(gfx, 1.0 / 60.0, &[(0, pad)], Mouse::default(), |_| false));
        }
        tick_n(session, gfx, 50);
    }

    /// Two seconds of Dolphin's sound arrive, stereo at its sample rate. Dolphin sends it from its
    /// CPU thread, not from `retro_run`.
    fn assert_sound_at_the_cores_rate(session: &mut Session) {
        let frames = (2.0 * session.core.av_info().fps).round();
        let samples = frames_with(session, &Default::default(), frames as usize).len() as f64;
        let av = session.core.av_info();
        let expected = frames / av.fps * 2.0 * av.sample_rate;
        assert!(
            (samples / expected - 1.0).abs() < 0.1,
            "{samples} samples in {frames} frames, not about {expected} ({av:?})"
        );
    }

    /// Fails if anything under `dir` was made or changed since `since`.
    fn assert_untouched(dir: &Path, since: std::time::SystemTime) {
        let meta = fs::symlink_metadata(dir).unwrap();
        assert!(meta.modified().unwrap() < since, "{} changed", dir.display());
        if meta.is_dir() {
            for entry in fs::read_dir(dir).unwrap() {
                assert_untouched(&entry.unwrap().path(), since);
            }
        }
    }

    /// Dolphin wrote into its `User/` folder in the Games Root's saves for `system`, and nothing
    /// into the test data it was given.
    fn assert_dolphin_wrote_only_its_own_folders(root: &Path, system: &str, since: std::time::SystemTime) {
        assert!(
            saves::save_dir(root, system).join("User").is_dir(),
            "Dolphin's User folder in the Games Root"
        );
        assert_untouched(&Path::new(TESTDATA).join("data/system/dolphin-emu"), since);
        assert_untouched(&Path::new(TESTDATA).join("games").join(system), since);
    }

    /// Save State slot 1 keeps the picture `session` shows now (FR-EMB-5, FR-EMB-6): `mover`
    /// moves FCE Ultra GX's menu cursor, so the picture changes; loading the slot brings back the
    /// exact saved picture, not the moved one, and so do slot 1 and the Resume State in a new
    /// session, Dolphin's second load in the process.
    fn assert_dolphin_states_round_trip(
        mut session: Session,
        gfx: &mut Gfx<'_>,
        ctx: &Ctx,
        shot: &Path,
        game: &GameRow,
        mover: PadState,
    ) {
        let saved = shown_exactly(&mut session, gfx, ctx, shot);
        session.save_slot(1).unwrap_or_else(|e| panic!("save: {e}"));
        press(&mut session, gfx, mover);
        let moved = shown_exactly(&mut session, gfx, ctx, shot);
        assert!(
            moved != saved,
            "the cursor moved the picture away from the saved one"
        );

        session.load_slot(1).unwrap_or_else(|e| panic!("load: {e}"));
        tick_n(&mut session, gfx, 30);
        assert!(
            shown_exactly(&mut session, gfx, ctx, shot) == saved,
            "the slot brings back the picture from save time, not the moved one"
        );
        session.write_resume();
        let resume = session.states.resume();
        drop(session);
        assert!(
            fs::metadata(&resume).is_ok_and(|m| m.len() > 0),
            "the Resume State"
        );

        let from_slot = Play {
            game: game.clone(),
            start: Start::Slot(1),
        };
        let mut session = Session::start(&from_slot, ctx, gfx.gl_context(), None, Some(60.0))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(session.start_message, None, "slot 1 restored in a new session");
        tick_n(&mut session, gfx, 30);
        assert!(
            shown_exactly(&mut session, gfx, ctx, shot) == saved,
            "a new session also restores the saved picture"
        );
        press(&mut session, gfx, mover);
        assert!(
            shown_exactly(&mut session, gfx, ctx, shot) != saved,
            "moved again"
        );
        // The new session has moved on to the first item: the Resume State must take it back.
        let state = fs::read(&resume).unwrap();
        session
            .core
            .unserialize(&state)
            .unwrap_or_else(|e| panic!("resume: {e}"));
        tick_n(&mut session, gfx, 30);
        assert!(
            shown_exactly(&mut session, gfx, ctx, shot) == saved,
            "the Resume State also restores the saved picture"
        );
    }

    /// GameCube with Dolphin (FR-NEW-3, FR-NEW-4): FCE Ultra GX, a `.dol`, boots through a
    /// GameCube session beside the opengl renderer and its menu shows on the renderer, 4:3 at
    /// twice the GameCube's 640×528. Its sound arrives, the d-pad moves its menu, and Save
    /// States and the Resume State round-trip.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn hw_dolphin_plays_fce_ultra_gx_on_gamecube_with_sound_input_and_states() {
        use crate::ui::text::Fonts;

        let _serial = serial();
        let since = std::time::SystemTime::now();
        let (root, data, ctx, game) = dolphin_world("gc");
        let (_sdl, canvas) = offscreen_renderer();
        let creator = canvas.texture_creator();
        let mut gfx = Gfx::new(canvas, &creator, Fonts::new().unwrap());
        let shot = data.path().join("shot.bmp");
        let mut session = Session::start(
            &Play::resume(game.clone()),
            &ctx,
            gfx.gl_context(),
            None,
            Some(60.0),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        tick_n(&mut session, &mut gfx, 300);
        assert_a_picture(&shown(&mut session, &mut gfx, &ctx, &shot), "booted");
        let frame = session.core.frame();
        assert_eq!((frame.width, frame.height), (1280, 1056), "twice 640×528");
        assert_eq!(session.core.av_info().aspect_ratio, 4.0 / 3.0);
        assert_sound_at_the_cores_rate(&mut session);

        let mut down = PadState::default();
        down.press(sdl3::gamepad::Button::DPadDown);
        assert_dolphin_states_round_trip(session, &mut gfx, &ctx, &shot, &game, down);
        assert_dolphin_wrote_only_its_own_folders(root.path(), "gc", since);
    }

    /// Wii with Dolphin (FR-NEW-3, FR-NEW-4): FCE Ultra GX, a `.dol`, boots through a Wii session
    /// beside the opengl renderer and its menu shows on the renderer, 16:9. The controller is a
    /// Wii Remote with a Nunchuk: Dolphin asks for the RetroPad buttons only that mapping uses
    /// (L, R and L2: -, + and shaking the Nunchuk), A on the Wii Remote opens the folder list,
    /// and the left stick, the Nunchuk's, moves its cursor (on a Wii Remote alone it would tilt
    /// the remote and leave the cursor). Its sound arrives, and Save States and the Resume State
    /// round-trip.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn hw_dolphin_plays_fce_ultra_gx_on_wii_with_the_wii_remote_and_nunchuk_sound_and_states() {
        use crate::libretro::Button;
        use crate::ui::text::Fonts;

        let _serial = serial();
        let since = std::time::SystemTime::now();
        let (root, data, ctx, game) = dolphin_world("wii");
        let wii = systems::by_id("wii").unwrap();
        assert_eq!(
            port_device(&ctx, wii, &cores::DOLPHIN, &game),
            cores::DOLPHIN_WII_REMOTE_AND_NUNCHUK
        );
        let (_sdl, canvas) = offscreen_renderer();
        let creator = canvas.texture_creator();
        let mut gfx = Gfx::new(canvas, &creator, Fonts::new().unwrap());
        let shot = data.path().join("shot.bmp");
        let mut session = Session::start(
            &Play::resume(game.clone()),
            &ctx,
            gfx.gl_context(),
            None,
            Some(60.0),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        tick_n(&mut session, &mut gfx, 300);
        let booted = shown_exactly(&mut session, &mut gfx, &ctx, &shot);
        assert_a_picture(&shown(&mut session, &mut gfx, &ctx, &shot), "booted");
        assert_eq!(session.core.av_info().aspect_ratio, 16.0 / 9.0);
        let queries = session.core.input_queries();
        for button in [Button::L, Button::R, Button::L2] {
            let id = (0, crate::libretro::PortDevice::RETROPAD.id(), 0, button as u32);
            assert!(queries.contains_key(&id), "{button:?}: {queries:?}");
        }
        assert_sound_at_the_cores_rate(&mut session);

        // A on the Wii Remote: "Up One Level", to a list the cursor can move in.
        let mut a = PadState::default();
        a.press(sdl3::gamepad::Button::East);
        press(&mut session, &mut gfx, a);
        assert!(
            shown_exactly(&mut session, &mut gfx, &ctx, &shot) != booted,
            "A opened a list"
        );

        let nunchuk_down = PadState {
            left_y: i16::MAX,
            ..Default::default()
        };
        assert_dolphin_states_round_trip(session, &mut gfx, &ctx, &shot, &game, nunchuk_down);
        assert_dolphin_wrote_only_its_own_folders(root.path(), "wii", since);
    }

    /// Wii with Dolphin and a Game set to the Classic Controller (FR-NEW-3): Game37 tells Dolphin
    /// its Classic Controller (Pro) device, and Dolphin then asks for what that mapping reads: the
    /// RetroPad's L, R, L2 and R2 as buttons (L, R, ZL and ZR), but neither the mouse nor L3, which
    /// only its Wii Remote mappings use, nor the analog L2 and R2 its plain Classic Controller reads
    /// for L and R; those queries are what pin the device. A then opens FCE Ultra GX's folder list (as
    /// it would with the Nunchuk too): the Game takes input.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn hw_dolphin_plays_fce_ultra_gx_on_wii_with_the_classic_controller() {
        use crate::library::FoundGame;
        use crate::library::db::SortOrder;
        use crate::libretro::Button;
        use crate::ui::text::Fonts;

        let _serial = serial();
        let (_root, data, mut ctx, game) = dolphin_world("wii");
        let found = FoundGame {
            system: "wii",
            rel_path: game.rel_path.clone(),
            title: game.title.clone(),
        };
        ctx.library.sync_system("wii", &[found], 0).unwrap();
        let game = ctx.library.system_games("wii", SortOrder::AZ).unwrap().remove(0);
        ctx.library
            .set_wii_controller(game.id, WiiController::ClassicController)
            .unwrap();
        let wii = systems::by_id("wii").unwrap();
        assert_eq!(
            port_device(&ctx, wii, &cores::DOLPHIN, &game),
            cores::DOLPHIN_CLASSIC_CONTROLLER
        );
        let (_sdl, canvas) = offscreen_renderer();
        let creator = canvas.texture_creator();
        let mut gfx = Gfx::new(canvas, &creator, Fonts::new().unwrap());
        let shot = data.path().join("shot.bmp");
        let mut session = Session::start(&Play::resume(game), &ctx, gfx.gl_context(), None, Some(60.0))
            .unwrap_or_else(|e| panic!("{e}"));
        tick_n(&mut session, &mut gfx, 300);
        let booted = shown_exactly(&mut session, &mut gfx, &ctx, &shot);
        assert_a_picture(&shown(&mut session, &mut gfx, &ctx, &shot), "booted");
        let queries = session.core.input_queries();
        let retropad = crate::libretro::PortDevice::RETROPAD.id();
        for button in [Button::L, Button::R, Button::L2, Button::R2] {
            assert!(
                queries.contains_key(&(0, retropad, 0, button as u32)),
                "{button:?}: {queries:?}"
            );
        }
        assert!(
            !queries.contains_key(&(0, retropad, 0, Button::L3 as u32)),
            "L3: {queries:?}"
        );
        // RETRO_DEVICE_MOUSE, RETRO_DEVICE_ANALOG and RETRO_DEVICE_INDEX_ANALOG_BUTTON.
        let (mouse, analog, index_button) = (2, 5, 2);
        assert!(
            !queries
                .keys()
                .any(|&(_, d, i, _)| d == mouse || (d == analog && i == index_button)),
            "{queries:?}"
        );

        let mut a = PadState::default();
        a.press(sdl3::gamepad::Button::East);
        press(&mut session, &mut gfx, a);
        assert!(
            shown_exactly(&mut session, &mut gfx, &ctx, &shot) != booted,
            "A opened a list"
        );
    }

    /// Dolphin, one Core for two Systems, loads again and again in one process, as when the
    /// player quits one Game and starts another: GameCube, then Wii, then GameCube again, each
    /// showing its Game on the renderer in its own shape, with frames run at the Game's own rate
    /// (FCE Ultra GX: 50 Hz on GameCube, 59.94 on Wii), as its sound comes. At load, Dolphin
    /// gives the rate of the Game before, so this needs [`CoreInfo::rereads_timing`].
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn hw_dolphin_loads_again_in_one_process_gamecube_then_wii_then_gamecube() {
        use crate::ui::text::Fonts;

        let _serial = serial();
        let (_sdl, canvas) = offscreen_renderer();
        let creator = canvas.texture_creator();
        let mut gfx = Gfx::new(canvas, &creator, Fonts::new().unwrap());
        // FCE Ultra GX's own rate on each System, as measured with Dolphin as the first Core in
        // a process (so no previous Game's rate could leak in): used here, not
        // `session.core.av_info().fps`, since that is read from the same stale place as
        // `session.fps` and would still agree with it when `rereads_timing` is broken.
        for (system, aspect, fps) in [
            ("gc", 4.0 / 3.0, 50.0),
            ("wii", 16.0 / 9.0, 59.94),
            ("gc", 4.0 / 3.0, 50.0),
        ] {
            let (_root, data, ctx, game) = dolphin_world(system);
            let shot = data.path().join("shot.bmp");
            let mut session = Session::start(&Play::resume(game), &ctx, gfx.gl_context(), None, Some(60.0))
                .unwrap_or_else(|e| panic!("{system}: {e}"));
            tick_n(&mut session, &mut gfx, 300);
            assert_a_picture(&shown(&mut session, &mut gfx, &ctx, &shot), system);
            assert_eq!(session.core.av_info().aspect_ratio, aspect, "{system}");
            assert_sound_at_the_cores_rate(&mut session);
            tick_n(&mut session, &mut gfx, 1);
            assert!(
                (session.fps - fps).abs() < 0.1,
                "{system}: frames at the Game's own rate, not the previous Game's: {} vs {fps}",
                session.fps
            );
        }
    }

    /// SDL's own label lookup, on virtual gamepads that report the vendor and product of real
    /// controllers: a Switch Pro Controller is Nintendo-lettered, an Xbox or PlayStation pad is not,
    /// and the Switch pad's A (right) reaches a PS2 Core as ✕ (RetroPad B) but an SNES Core as A.
    #[test]
    fn sdl_labels_a_switch_pro_controller_nintendo_lettered_and_xbox_and_playstation_pads_not() {
        use sdl3::sys::joystick::{
            SDL_AttachVirtualJoystick, SDL_DetachVirtualJoystick, SDL_JOYSTICK_TYPE_GAMEPAD,
            SDL_VirtualJoystickDesc,
        };

        let _serial = serial();
        let sdl = sdl3::init().unwrap();
        let gamepads = sdl.gamepad().unwrap();
        let open = |vendor: u16, product: u16| {
            let desc = SDL_VirtualJoystickDesc {
                r#type: SDL_JOYSTICK_TYPE_GAMEPAD.0 as u16,
                vendor_id: vendor,
                product_id: product,
                naxes: 6,
                nbuttons: 15,
                name: c"Game37 test pad".as_ptr(),
                ..SDL_VirtualJoystickDesc::new()
            };
            // SAFETY: `desc` is a valid descriptor, read during the call.
            let id = unsafe { SDL_AttachVirtualJoystick(&desc) };
            assert_ne!(id.0, 0, "{}", sdl3::get_error());
            let pad = gamepads.open(id.into()).unwrap();
            (id, pad)
        };
        let pads = [
            (
                open(0x057e, 0x2009),
                FaceLetters::Nintendo,
                "Switch Pro Controller",
            ),
            (open(0x045e, 0x02ea), FaceLetters::Other, "Xbox One controller"),
            (open(0x054c, 0x0ce6), FaceLetters::Other, "DualSense"),
        ];
        for ((_, pad), want, name) in &pads {
            assert_eq!(face_letters(pad), *want, "{name}: {:?}", pad.r#type());
            assert_eq!(pad_state(pad).letters, *want, "{name}");
        }

        let ((_, switch), _, _) = &pads[0];
        let mut a = pad_state(switch);
        a.press(PadButton::East);
        let on = |system: &str| {
            let by_letter = input::faces_by_letter(systems::by_id(system).unwrap());
            let (input, _) = input::frame_input_with(
                &[(0, a)],
                Default::default(),
                false,
                &mut PauseTrigger::default(),
                true,
                by_letter,
            );
            input.mask(0)
        };
        assert_eq!(on("ps2"), 1 << crate::libretro::Button::B as u16, "A is ✕ on PS2");
        assert_eq!(
            on("snes"),
            1 << crate::libretro::Button::A as u16,
            "A is A on SNES"
        );

        for ((id, pad), _, _) in pads {
            drop(pad);
            // SAFETY: `id` is a virtual joystick this test attached, now closed.
            assert!(unsafe { SDL_DetachVirtualJoystick(id) });
        }
    }

    #[test]
    fn sdl_cores_get_no_host_without_a_frontend_context() {
        assert!(open_gl(&cores::MESEN, None, None, "NES").unwrap().is_none());
    }

    /// On the software renderer (no frontend context), the Systems whose Cores draw in software
    /// still play, and those whose Cores need OpenGL are refused with a message saying so
    /// (issue #43). PSP is left out: its GLX host doesn't come from the renderer.
    #[test]
    fn without_a_frontend_context_only_cores_that_need_opengl_are_refused() {
        for id in ["nes", "snes", "gb", "gbc", "gba", "nds", "ps1"] {
            let system = systems::by_id(id).unwrap();
            let core = cores::for_system(system).unwrap();
            match open_gl(core, None, None, system.name) {
                Ok(host) => assert!(host.is_none(), "{id}"),
                Err(e) => panic!("{id} refused: {e}"),
            }
        }
        for id in ["n64", "3ds", "gc", "wii", "ps2"] {
            let system = systems::by_id(id).unwrap();
            let core = cores::for_system(system).unwrap();
            match open_gl(core, None, None, system.name) {
                Err(e) => {
                    assert_eq!(e.title, "OpenGL isn't available", "{id}");
                    assert_eq!(
                        e.message,
                        format!(
                            "{} games need OpenGL, which this computer's graphics driver doesn't provide.",
                            system.name
                        )
                    );
                }
                Ok(_) => panic!("{id} was given a Core without OpenGL"),
            }
        }
    }

    /// A Game whose Core needs OpenGL, started without a frontend context, gets the OpenGL message
    /// before its Core is loaded: there is no Core installed here, so loading it would fail with
    /// the generic title instead (issue #43).
    #[test]
    fn a_game_whose_core_needs_opengl_does_not_start_without_a_frontend_context() {
        let _serial = serial();
        let dir = TempDir::new().unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = dir.path().join("Games");
        ctx.data_dir = dir.path().join("data");
        let play = Play {
            game: GameRow {
                id: 1,
                system: "n64".into(),
                rel_path: "n64/Game.n64".into(),
                title: "Game".into(),
                date_added: 0,
                last_played: None,
                playtime_secs: 0,
                favorite: false,
            },
            start: Start::PowerOn,
        };
        match Session::start(&play, &ctx, None, None, None) {
            Err(e) => {
                assert_eq!(e.title, "OpenGL isn't available");
                assert_eq!(
                    e.message,
                    "Nintendo 64 games need OpenGL, which this computer's graphics driver doesn't provide."
                );
            }
            Ok(_) => panic!("expected an error"),
        }
    }

    #[test]
    fn a_glx_core_with_no_x_display_fails_with_the_system_name_in_the_message() {
        match open_gl(&cores::PPSSPP, None, Some(c"/nonexistent/game37-test:0"), "PSP") {
            Err(e) => assert_eq!(e.message, "PSP games need X11 (XWayland), which isn't available."),
            Ok(_) => panic!("expected an error"),
        }
    }

    /// The app's path for an OpenGL Core: a PSP session started beside the `opengl` renderer of
    /// an offscreen window, ticked and drawn through `Gfx` as `app.rs` does for 900 frames, shows
    /// CMFileManager's picture on the renderer. PPSSPP leaves its framebuffer's alpha at 0, so a
    /// frame drawn with its alpha would show nothing.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn hw_ppsspp_session_shows_cmfilemanager_on_the_renderer() {
        use sdl3::render::create_renderer;
        use sdl3::surface::Surface;

        use crate::ui::text::Fonts;
        use crate::ui::theme::Colour;

        let _serial = serial();
        let root = TempDir::new().unwrap();
        let game_dir = root.path().join("psp/CMFileManager");
        fs::create_dir_all(&game_dir).unwrap();
        for entry in fs::read_dir(Path::new(TESTDATA).join("games/psp/CMFileManager")).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), game_dir.join(entry.file_name())).unwrap();
        }
        let data = TempDir::new().unwrap();
        for dir in ["cores", "system"] {
            std::os::unix::fs::symlink(Path::new(TESTDATA).join("data").join(dir), data.path().join(dir))
                .unwrap();
        }
        let mut ctx = Ctx::empty();
        ctx.games_root = root.path().to_path_buf();
        ctx.data_dir = data.path().to_path_buf();
        let game = GameRow {
            id: 1,
            system: "psp".into(),
            rel_path: "psp/CMFileManager/EBOOT.PBP".into(),
            title: "CMFileManager".into(),
            date_added: 0,
            last_played: None,
            playtime_secs: 0,
            favorite: false,
        };

        sdl3::hint::set("SDL_VIDEO_DRIVER", "offscreen");
        let sdl = sdl3::init().unwrap();
        let video = sdl.video().unwrap();
        let window = video.window("hw", 1280, 720).hidden().build().unwrap();
        let canvas = create_renderer(window, Some(c"opengl")).unwrap();
        let creator = canvas.texture_creator();
        let mut gfx = Gfx::new(canvas, &creator, Fonts::new().unwrap());
        let frontend = gfx.gl_context();
        assert!(frontend.is_some(), "the opengl renderer's context");
        let mut session = Session::start(&Play::resume(game.clone()), &ctx, frontend, None, Some(60.0))
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..900 {
            assert!(!session.tick(&mut gfx, 1.0 / 60.0, &[], Mouse::default(), |_| false));
        }
        gfx.clear(Colour::hex(0x000000));
        session.draw(&mut gfx, &ctx);
        let shot = data.path().join("shot.bmp");
        gfx.save_bmp(&shot).unwrap();
        drop(session);

        let shot = Surface::load_bmp(&shot).unwrap();
        let pitch = shot.pitch() as usize;
        let bytes = shot.pixel_format().bytes_per_pixel();
        let colours: std::collections::HashSet<Vec<u8>> = shot.with_lock(|pixels| {
            pixels
                .chunks(pitch)
                .flat_map(|row| row[..shot.width() as usize * bytes].chunks(bytes))
                .map(<[u8]>::to_vec)
                .collect()
        });
        assert!(
            colours.len() > 1,
            "the renderer shows a single colour: {colours:?}"
        );
    }
}
