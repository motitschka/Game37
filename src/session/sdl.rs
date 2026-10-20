//! The embedded game session's SDL side: an Embedded System's Game running on its Core, its
//! picture drawn through [`Gfx`] without chrome, its sound played through an SDL audio stream and
//! its input read from the keyboard (FR-EMB-*). The session is the screen on top of [`Nav`] while
//! the Game runs, so ending it pops back to where it started.

use std::fmt::Display;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sdl3::AudioSubsystem;
use sdl3::audio::{AudioFormat, AudioSpec, AudioStreamOwner};
use sdl3::gamepad::{Axis, Gamepad};
use sdl3::keyboard::Scancode;

use super::gl::SdlGl;
use super::input::{self, HeldOver, PadState, PauseTrigger};
use super::saves::{self, Tracker};
use super::states::{self, SLOTS, StatePaths};
use super::timing::{self, AUDIO_MAX, PlayTimer, Scheduler};
use crate::cores;
use crate::ctx::{self, Ctx};
use crate::launch::{self, FAILED_TITLE};
use crate::library::db::GameRow;
use crate::libretro::{Core, CoreDirs, Frame, GlHost};
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
    /// keeps the queue near its target.
    fn balance(&self, base_ratio: f64) {
        let mut queued = self.queued();
        if queued > AUDIO_MAX {
            let _ = self.stream.clear();
            queued = 0.0;
        }
        let ratio = timing::audio_ratio(base_ratio, queued) as f32;
        // SAFETY: the stream is alive.
        unsafe { sdl3::sys::audio::SDL_SetAudioStreamFrequencyRatio(self.stream.raw(), ratio) };
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
    state.left_trigger = gamepad.axis(Axis::TriggerLeft);
    state.right_trigger = gamepad.axis(Axis::TriggerRight);
    state
}

impl Session {
    /// Loads `game` on its System's Core, which may render with OpenGL through `gl`. `refresh` is
    /// the display's refresh rate when frames are synced to it (vsync), else `None`. `Err` is the
    /// message to show.
    pub fn start(
        game: &GameRow,
        ctx: &Ctx,
        gl: Option<Box<dyn GlHost>>,
        audio: Option<&AudioSubsystem>,
        refresh: Option<f64>,
    ) -> Result<Session, String> {
        let system = systems::by_id(&game.system).ok_or_else(|| format!("No System {:?}", game.system))?;
        let core_info =
            cores::for_system(system).ok_or_else(|| format!("{} has no Core yet.", system.name))?;
        let fail = |e: &dyn Display| format!("{} couldn't start {}: {e}.", core_info.name, game.title);
        let root = &ctx.games_root;
        let dirs = CoreDirs {
            system: root.join("bios"),
            save: saves::save_dir(root, system.id),
        };
        for dir in [&dirs.system, &dirs.save] {
            fs::create_dir_all(dir).map_err(|e| fail(&format!("Can't create {}: {e}", dir.display())))?;
        }
        let core_path = cores::installed_path(&ctx.data_dir, core_info);
        let mut core =
            Core::load(&core_path, &root.join(&game.rel_path), dirs, &[], gl).map_err(|e| fail(&e))?;

        let srm = saves::srm_path(root, system.id, Path::new(&game.rel_path));
        let save = core.save_ram().map(|ram| {
            if let Err(e) = saves::load(&srm, ram) {
                eprintln!("game37: can't read {}: {e}", srm.display());
            }
            let tracker = Tracker::new(ram);
            (srm, tracker)
        });
        let states = StatePaths::new(root, system.id, Path::new(&game.rel_path));
        // Continue where the player quit (FR-EMB-6). A state the Core can't read, e.g. after a
        // Core update, starts the Game fresh.
        match fs::read(states.resume()) {
            Ok(state) => {
                if let Err(e) = core.unserialize(&state) {
                    eprintln!("game37: can't resume from {}: {e}", states.resume().display());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => eprintln!("game37: can't read {}: {e}", states.resume().display()),
        }
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
        })
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
    /// occupied port's state and the keys `down` reports held, uploading new frames to `gfx` and
    /// queueing their sound. Returns whether the pause trigger (Esc, Guide or Select+Start)
    /// fired, in which case no frame runs.
    pub fn tick(
        &mut self,
        gfx: &mut impl GameFrames,
        dt: f64,
        pads: &[(usize, PadState)],
        down: impl Fn(Scancode) -> bool,
    ) -> bool {
        let keyboard = input::keyboard_buttons(&down);
        let (mut input, paused) = input::frame_input(pads, keyboard, down(Scancode::Escape), &mut self.pause);
        if paused {
            return true;
        }
        self.held.mask(&mut input);
        self.follow_av_info();
        for _ in 0..self.scheduler.frames(dt) {
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
        if let Some(audio) = &self.audio {
            audio.balance(self.scheduler.base_ratio());
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

    /// Slot `n`'s time and screenshot, `None` when it is empty.
    fn slot_info(&self, n: usize) -> Option<SlotInfo> {
        let modified = fs::metadata(self.states.slot(n))
            .and_then(|m| m.modified())
            .ok()?;
        let thumb = states::read_png(&self.states.thumb(n))
            .map_err(|e| {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("game37: can't read {}: {e}", self.states.thumb(n).display());
                }
            })
            .ok();
        Some(SlotInfo {
            when: when_label(modified),
            thumb,
        })
    }

    /// Writes slot `n`'s Save State and a screenshot of the current frame (FR-EMB-5).
    fn save_slot(&mut self, n: usize) -> Result<SlotInfo, String> {
        let state = self.core.serialize().map_err(|e| e.to_string())?;
        let png = states::encode_png(&states::frame_rgb(self.core.frame()));
        states::write_slot(&self.states.slot(n), &state, &self.states.thumb(n), png)
            .map_err(|e| e.to_string())?;
        self.slot_info(n).ok_or_else(|| "the slot is gone".to_owned())
    }

    fn load_slot(&mut self, n: usize) -> Result<(), String> {
        let state = fs::read(self.states.slot(n)).map_err(|e| e.to_string())?;
        self.core.unserialize(&state).map_err(|e| e.to_string())
    }

    /// Writes the Resume State the next launch continues from (FR-EMB-6).
    fn write_resume(&mut self) {
        store_resume(&self.states.resume(), self.core.serialize());
    }

    /// Writes the save memory if it changed and the Resume State, records the Playtime and
    /// silences the Game. The Core shuts down when the session is dropped.
    fn end(&mut self, ctx: &mut Ctx, now: i64) {
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

/// The OpenGL host a Core renders through when `gfx` draws with OpenGL.
pub fn gl_host(gfx: &Gfx<'_>) -> Option<Box<dyn GlHost>> {
    let (window, context) = gfx.gl_context()?;
    Some(Box::new(SdlGl::new(window, context)))
}

/// Starts `game` and pushes its session, or pushes the message saying why it couldn't start. Its
/// Core may render with OpenGL through `gl` (see [`gl_host`]).
pub fn start(
    game: &GameRow,
    nav: &mut Nav,
    ctx: &Ctx,
    gl: Option<Box<dyn GlHost>>,
    audio: Option<&AudioSubsystem>,
    refresh: Option<f64>,
) {
    match Session::start(game, ctx, gl, audio, refresh) {
        Ok(session) => nav.push(Box::new(session)),
        Err(message) => nav.push(Box::new(Message::new(FAILED_TITLE, message))),
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
    down: impl Fn(Scancode) -> bool,
) {
    let paused = nav.modal().is_some();
    let Some(session) = running(nav) else { return };
    session.set_paused(paused);
    if !paused {
        if session.tick(gfx, dt, pads, down) {
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
        Some(Request::Quit) => end(nav, ctx),
        None if resume => nav.pop(),
        None => {}
    }
}

/// What the pause trigger does: opens the Pause Menu over the session (FR-EMB-3).
pub fn on_pause(nav: &mut Nav) {
    let Some(session) = running(nav) else { return };
    let slots = (1..=SLOTS).map(|n| session.slot_info(n)).collect();
    let menu = PauseMenu::new(&session.title, session.meta(), slots);
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

/// A file time as the Pause Menu shows it, in local time (see [`states::when_label`]).
fn when_label(time: std::time::SystemTime) -> String {
    use sdl3::sys::time::{SDL_DateTime, SDL_GetCurrentTime, SDL_TimeToDateTime};
    let local = |ns: i64| {
        let mut dt = SDL_DateTime::default();
        // SAFETY: only writes to the local passed in.
        unsafe { SDL_TimeToDateTime(ns, &mut dt, true) }.then_some(states::Local {
            year: dt.year,
            month: dt.month as u32,
            day: dt.day as u32,
            hour: dt.hour as u32,
            minute: dt.minute as u32,
        })
    };
    let then = time
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i64);
    let mut now = 0;
    // SAFETY: only writes to the local passed in.
    let now = unsafe { SDL_GetCurrentTime(&mut now) }.then_some(now);
    match (local(then), now.and_then(local)) {
        (Some(then), Some(now)) => states::when_label(then, now),
        _ => String::new(),
    }
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

    /// Save States and the Resume State on bsnes (FR-EMB-5, FR-EMB-6): loading slot 1 replays
    /// the same frames, and a relaunch continues from the frame shown when the player quit.
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

        let mut session = Session::start(&game, &ctx, None, None, None).unwrap_or_else(|e| panic!("{e}"));
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

        run(&mut session, 200);
        session.end(&mut ctx, 0);
        // What the quit-time state shows next. bsnes's first frame after restoring a state into a
        // freshly loaded Core is a stale one, so compare the second.
        let next = run(&mut session, 2);
        drop(session);
        assert!(states.join("240p Test Suite (World).resume.state").exists());
        let mut session = Session::start(&game, &ctx, None, None, None).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(run(&mut session, 2), next, "the relaunch continues where it quit");
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
            tick(nav, ctx, &mut NoPicture, 0.02, &[], down)
        };
        let start_held = |nav: &mut Nav| {
            let session = running(nav).expect("a Game runs");
            session.core.last_input().is_pressed(0, Button::Start)
        };
        let act = |nav: &mut Nav, ctx: &mut Ctx, action: Action| {
            nav.handle(Input::Action(action), &Hits::default(), ctx);
        };

        start(&game, &mut nav, &ctx, None, None, None);
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
        for action in [Action::Down, Action::Down, Action::Down, Action::Confirm] {
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
        start(&game, &mut nav, &ctx, None, None, None);
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

        let mut core =
            Core::load(&core_path, &game_path, dirs.clone(), &[], None).unwrap_or_else(|e| panic!("{e}"));
        let ram = core.save_ram().expect("mGBA's Test Suite Game has save RAM");
        let known: Vec<u8> = (0..ram.len()).map(|i| i as u8).collect();
        ram.copy_from_slice(&known);

        let srm = saves::srm_path(root.path(), "gba", Path::new("160p Test Suite (World).gba"));
        let mut tracker = Tracker::new(core.save_ram().unwrap());
        write_save(&srm, &mut tracker, core.save_ram().unwrap());
        assert!(srm.exists(), "the .srm exists in saves/gba/");
        assert_eq!(srm.parent().unwrap(), saves::save_dir(root.path(), "gba"));
        drop(core);

        let mut core = Core::load(&core_path, &game_path, dirs, &[], None).unwrap_or_else(|e| panic!("{e}"));
        let ram = core.save_ram().expect("mGBA's Test Suite Game has save RAM");
        assert!(saves::load(&srm, ram).unwrap());
        assert_eq!(ram, known.as_slice());
    }
}
