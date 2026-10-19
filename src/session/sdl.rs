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
use sdl3::keyboard::Scancode;

use super::input::{self, PauseTrigger};
use super::saves::{self, Tracker};
use super::timing::{self, AUDIO_MAX, Scheduler};
use crate::cores;
use crate::ctx::{self, Ctx};
use crate::launch::{self, FAILED_TITLE};
use crate::library::db::GameRow;
use crate::libretro::{Core, CoreDirs};
use crate::screens::message::Message;
use crate::systems;
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
    /// The Game's save memory file and its change tracker; `None` when it has no save memory.
    save: Option<(PathBuf, Tracker)>,
    started: Instant,
    shown: bool,
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

impl Session {
    /// Loads `game` on its System's Core. `refresh` is the display's refresh rate when frames are
    /// synced to it (vsync), else `None`. `Err` is the message to show.
    pub fn start(
        game: &GameRow,
        ctx: &Ctx,
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
        let mut core = Core::load(&core_path, &root.join(&game.rel_path), dirs).map_err(|e| fail(&e))?;

        let srm = saves::srm_path(root, system.id, Path::new(&game.rel_path));
        let save = core.save_ram().map(|ram| {
            if let Err(e) = saves::load(&srm, ram) {
                eprintln!("game37: can't read {}: {e}", srm.display());
            }
            let tracker = Tracker::new(ram);
            (srm, tracker)
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
            save,
            started: Instant::now(),
            shown: true,
        })
    }

    /// The window was hidden or shown: a hidden Game runs no frames and plays no sound, and owes
    /// no frames for the time it was hidden.
    pub fn set_shown(&mut self, shown: bool) {
        if shown == self.shown {
            return;
        }
        self.shown = shown;
        if let Some(audio) = &self.audio {
            let _ = if shown {
                audio.stream.resume()
            } else {
                audio.stream.pause()
            };
        }
        self.scheduler.reset();
    }

    /// Runs the Core frames due `dt` seconds after the last tick, with the keys `down` reports
    /// held, uploading new frames to `gfx` and queueing their sound. Returns whether the pause
    /// trigger (Esc) fired, in which case no frame runs.
    pub fn tick(&mut self, gfx: &mut Gfx<'_>, dt: f64, down: impl Fn(Scancode) -> bool) -> bool {
        let keyboard = input::keyboard_buttons(&down);
        let (input, paused) = input::frame_input(&[], keyboard, down(Scancode::Escape), &mut self.pause);
        if paused {
            return true;
        }
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

    /// Writes the save memory if it changed, records the Playtime and silences the Game. The
    /// Core shuts down when the session is dropped.
    fn end(&mut self, ctx: &mut Ctx, now: i64) {
        if let Some((path, tracker)) = &mut self.save
            && let Some(ram) = self.core.save_ram()
            && tracker.changed(ram)
        {
            write_save(path, tracker, ram);
        }
        launch::record(ctx, self.game_id, now, self.started.elapsed());
        self.audio = None;
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

/// Starts `game` and pushes its session, or pushes the message saying why it couldn't start.
pub fn start(game: &GameRow, nav: &mut Nav, ctx: &Ctx, audio: Option<&AudioSubsystem>, refresh: Option<f64>) {
    match Session::start(game, ctx, audio, refresh) {
        Ok(session) => nav.push(Box::new(session)),
        Err(message) => nav.push(Box::new(Message::new(FAILED_TITLE, message))),
    }
}

/// The session on top of `nav`, if a Game is running there.
pub fn running(nav: &mut Nav) -> Option<&mut Session> {
    nav.top().session()
}

/// Ticks the running session, if any (see [`Session::tick`]), and handles its pause trigger.
pub fn tick(nav: &mut Nav, ctx: &mut Ctx, gfx: &mut Gfx<'_>, dt: f64, down: impl Fn(Scancode) -> bool) {
    if let Some(session) = running(nav)
        && session.tick(gfx, dt, down)
    {
        on_pause(nav, ctx);
    }
}

/// What the pause trigger does. The Pause Menu replaces this; for now it ends the session.
pub fn on_pause(nav: &mut Nav, ctx: &mut Ctx) {
    end(nav, ctx);
}

/// Ends the running session, if any: writes its save memory, records its Playtime, shuts its
/// Core down and pops back to where it started.
pub fn end(nav: &mut Nav, ctx: &mut Ctx) {
    if let Some(session) = running(nav) {
        session.end(ctx, ctx::now());
        nav.pop();
    }
}
