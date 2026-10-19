//! The window, the event loop and screenshot mode.

use std::collections::HashMap;
use std::ffi::CStr;
use std::time::{Duration, Instant};

use sdl3::event::{Event, WindowEvent};
use sdl3::gamepad::{Axis, Gamepad};
use sdl3::joystick::JoystickId;
use sdl3::keyboard::{Keycode, Scancode};
use sdl3::mouse::MouseButton;
use sdl3::render::{WindowCanvas, create_renderer};
use sdl3::sys::render::SDL_RendererLogicalPresentation;
use sdl3::{GamepadSubsystem, VideoSubsystem};

use crate::cli::{Options, Screenshot};
use crate::config;
use crate::ctx::{self, Ctx};
use crate::emulators::Emulators;
use crate::launch::{Launcher, NeverExits, Processes};
use crate::library::db::Library;
use crate::library::watch::LibraryWatcher;
use crate::screens;
use crate::session::input::{self, Ports};
use crate::session::sdl;
use crate::ui::chrome::{self, TopBar};
use crate::ui::controls::{ControlEvent, Controls, StickAxis};
use crate::ui::gfx::Gfx;
use crate::ui::input::{Action, ButtonStyle, Input, InputKind, Pointer};
use crate::ui::nav::{Flow, Hits, Nav};
use crate::ui::text::Fonts;
use crate::ui::theme::Colour;

pub fn run(options: Options) -> Result<(), String> {
    let path = options.config.unwrap_or_else(config::config_path);
    let loaded = config::load(&path);
    let data_dir = options.data_dir.unwrap_or_else(config::data_dir);
    let log_dir = data_dir.join("logs");
    let (mut ctx, has_library) = match &loaded {
        Ok(Some(config)) => {
            let library = Library::open(&data_dir.join("library.db")).map_err(|e| e.to_string())?;
            (Ctx::new(library, config.games_root.clone()), true)
        }
        _ => (Ctx::empty(), false),
    };
    ctx.data_dir = data_dir;
    match options.screenshot {
        Some(shot) => {
            // Scan in the foreground, so the screenshot shows every Game.
            if has_library {
                ctx.scan_all(ctx::now()).map_err(|e| e.to_string())?;
            }
            ctx.simulate_emulators(shot.missing_emulators.clone());
            let nav = screens::start(loaded, &path, &ctx);
            // Never starts a process: a launch shows the Playing screen for good.
            let launcher = Launcher::new(Box::new(NeverExits::default()), log_dir);
            screenshot(nav, ctx, launcher, shot)
        }
        None => {
            ctx.animate = true;
            if has_library {
                ctx.emulators = Emulators::detect();
            }
            let nav = screens::start(loaded, &path, &ctx);
            let watcher = has_library.then(|| LibraryWatcher::start(ctx.games_root.clone()));
            let launcher = Launcher::new(Box::new(Processes), log_dir);
            interactive(nav, ctx, watcher, launcher)
        }
    }
}

/// A window with an SDL_Renderer on the named backend, presenting logical 1920×1080 letterboxed.
fn canvas(video: &VideoSubsystem, w: u32, h: u32, driver: &CStr) -> Result<WindowCanvas, String> {
    let window = video
        .window("Game37", w, h)
        .resizable()
        .position_centered()
        .build()
        .map_err(|e| format!("Can't open a window: {e}"))?;
    let mut canvas = create_renderer(window, Some(driver))
        .map_err(|e| format!("Can't start the {} renderer: {e}", driver.to_string_lossy()))?;
    canvas
        .set_logical_size(1920, 1080, SDL_RendererLogicalPresentation::LETTERBOX)
        .map_err(|e| e.to_string())?;
    Ok(canvas)
}

/// How long a frame takes at 60 fps, the cap when vsync isn't available.
const FRAME: Duration = Duration::from_nanos(1_000_000_000 / 60);
/// The longest the loop sleeps while the window is minimized or hidden.
const HIDDEN_WAIT: Duration = Duration::from_millis(100);

fn interactive(
    mut nav: Nav,
    mut ctx: Ctx,
    mut watcher: Option<LibraryWatcher>,
    mut launcher: Launcher,
) -> Result<(), String> {
    let sdl = sdl3::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
    let gamepads = sdl.gamepad().map_err(|e| e.to_string())?;
    let audio = sdl.audio().map_err(|e| eprintln!("game37: no sound: {e}")).ok();
    let fonts = Fonts::new()?;
    // OpenGL, because later milestones host libretro OpenGL cores in this renderer.
    let canvas = canvas(&video, 1600, 900, c"opengl")?;
    // SAFETY: the renderer is alive.
    let vsync = unsafe { sdl3::sys::render::SDL_SetRenderVSync(canvas.raw(), 1) };
    if !vsync {
        eprintln!("game37: no vsync ({}); capping at 60 fps", sdl3::get_error());
    }
    let creator = canvas.texture_creator();
    let mut gfx = Gfx::new(canvas, &creator, fonts);
    let mut events = sdl.event_pump().map_err(|e| e.to_string())?;

    let mut input = InputState::new(gamepads);
    let mut hits = Hits::default();
    let mut inputs = Vec::new();
    let mut sync_failing = false;
    let mut last_frame = Instant::now();
    'frames: loop {
        let frame_start = Instant::now();
        let dt = frame_start.duration_since(last_frame).as_secs_f64();
        last_frame = frame_start;
        // Nothing is drawn while the window can't be seen: sleep until an event, or long enough
        // for the watcher to keep up.
        let shown = gfx.is_shown();
        let woken_by = if shown {
            None
        } else {
            events.wait_event_timeout(HIDDEN_WAIT)
        };
        for event in woken_by.into_iter().chain(events.poll_iter()) {
            if input.event(event, &mut gfx, &mut inputs) == Flow::Quit {
                break 'frames;
            }
        }
        if let Some(watcher) = &mut watcher {
            match watcher.poll(&mut ctx.library, ctx::now()) {
                Ok(changed) => {
                    sync_failing = false;
                    if changed {
                        ctx.revision += 1;
                    }
                }
                // The watcher retries the failed sync every frame: log it and reload the views
                // (some Systems may have synced before the failure) once, not every frame.
                Err(e) if !sync_failing => {
                    sync_failing = true;
                    ctx.revision += 1;
                    eprintln!("game37: {e}");
                }
                Err(_) => {}
            }
        }
        // The emulator exited: come back in front (FR-EXT-2).
        if launcher.poll(&mut nav, &mut ctx, ctx::now()).is_some() {
            gfx.restore();
        }
        inputs.extend(input.controls.poll(sdl3::timer::ticks()));
        for i in inputs.drain(..) {
            // A running Game takes no menu input: it reads the keyboard itself.
            if sdl::playing(&mut nav) {
                continue;
            }
            if nav.handle(i, &hits, &mut ctx) == Flow::Quit {
                break 'frames;
            }
            if let Some(game) = ctx.launch.take() {
                gfx.minimize();
                if !launcher.start(&game, &mut nav, &ctx) {
                    gfx.restore();
                }
            }
            if let Some(game) = ctx.play.take() {
                // Frames sync to the display only with vsync.
                let refresh = if vsync { gfx.refresh_rate() } else { None };
                sdl::start(&game, &mut nav, &ctx, audio.as_ref(), refresh);
            }
        }
        copy_clipboard_request(&mut ctx, &video);
        if let Some(session) = sdl::running(&mut nav) {
            session.set_shown(shown);
        }
        if !shown {
            continue;
        }
        let keys = events.keyboard_state();
        let pads = input.pad_states();
        sdl::tick(&mut nav, &mut ctx, &mut gfx, dt, &pads, |k| {
            keys.is_scancode_pressed(k)
        });
        ctx.input_kind = input.controls.kind;
        ctx.button_style = input.controls.style;
        hits = draw(&mut gfx, &mut nav, &ctx, &input.view(), &clock());
        gfx.present();
        if !vsync && let Some(left) = FRAME.checked_sub(frame_start.elapsed()) {
            std::thread::sleep(left);
        }
    }
    // Closing the window ends a running Game first, saving it.
    sdl::end(&mut nav, &mut ctx);
    // Quitting while an emulator runs leaves it running, with Playtime counted up to now.
    launcher.quit(&mut nav, &mut ctx, ctx::now());
    nav.leave_all(&ctx);
    Ok(())
}

fn screenshot(mut nav: Nav, mut ctx: Ctx, mut launcher: Launcher, shot: Screenshot) -> Result<(), String> {
    sdl3::hint::set("SDL_VIDEO_DRIVER", "offscreen");
    sdl3::hint::set("SDL_AUDIO_DRIVER", "dummy");
    let sdl = sdl3::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
    let audio = sdl.audio().map_err(|e| eprintln!("game37: no sound: {e}")).ok();
    let fonts = Fonts::new()?;
    let canvas = canvas(&video, 1920, 1080, c"opengl").or_else(|e| {
        eprintln!("game37: {e}; using the software renderer for the screenshot");
        canvas(&video, 1920, 1080, c"software")
    })?;
    let creator = canvas.texture_creator();
    let mut gfx = Gfx::new(canvas, &creator, fonts);

    let mut view = View {
        kind: InputKind::Controller,
        style: ButtonStyle::default(),
        pointer: None,
    };
    let clock = clock();
    let mut hits = draw(&mut gfx, &mut nav, &ctx, &view, &clock);
    gfx.present();
    'steps: for step in shot.steps {
        let mut esc = false;
        for i in step.inputs() {
            // A running Game takes no menu input; `back` stands for Esc.
            if sdl::playing(&mut nav) {
                esc |= i == Input::Action(Action::Back);
                continue;
            }
            match i {
                Input::Action(_) => view.kind = InputKind::Controller,
                Input::Pointer(p) => {
                    view.kind = InputKind::Keyboard;
                    if let Pointer::Move { x, y } = p {
                        view.pointer = Some((x, y));
                    }
                }
            }
            if nav.handle(i, &hits, &mut ctx) == Flow::Quit {
                break 'steps;
            }
            copy_clipboard_request(&mut ctx, &video);
            if let Some(game) = ctx.launch.take() {
                launcher.start(&game, &mut nav, &ctx);
            }
            if let Some(game) = ctx.play.take() {
                sdl::start(&game, &mut nav, &ctx, audio.as_ref(), Some(SHOT_REFRESH));
            }
        }
        for _ in 0..step.frames() {
            sdl::tick(&mut nav, &mut ctx, &mut gfx, 1.0 / SHOT_REFRESH, &[], |k| {
                esc && k == Scancode::Escape
            });
            ctx.input_kind = view.kind;
            hits = draw(&mut gfx, &mut nav, &ctx, &view, &clock);
            gfx.present();
        }
    }
    for frame in 1..=shot.frames {
        sdl::tick(&mut nav, &mut ctx, &mut gfx, 1.0 / SHOT_REFRESH, &[], |_| false);
        draw(&mut gfx, &mut nav, &ctx, &view, &clock);
        if frame < shot.frames {
            gfx.present();
        }
    }
    let saved = gfx.save_bmp(&shot.out);
    sdl::end(&mut nav, &mut ctx);
    saved
}

/// The display refresh rate screenshot mode pretends to have: each step and frame is 1/60 s.
const SHOT_REFRESH: f64 = 60.0;

/// Around a screen without chrome, such as a running Game's bars.
const BLACK: Colour = Colour::hex(0x000000);

/// What the chrome needs to know about input.
struct View {
    kind: InputKind,
    style: ButtonStyle,
    pointer: Option<(f32, f32)>,
}

/// Draws a frame (without presenting it) and returns where its clickable chrome went.
fn draw(gfx: &mut Gfx<'_>, nav: &mut Nav, ctx: &Ctx, view: &View, clock: &str) -> Hits {
    if !nav.base().chrome() {
        gfx.clear(BLACK);
        nav.base().draw(gfx, ctx);
        let Some(modal) = nav.modal() else {
            return Hits::default();
        };
        // The Pause Menu over a running Game.
        modal.draw(gfx, ctx);
        let hints = chrome::hintbar(gfx, &modal.hints(), view.kind, view.style, view.pointer);
        return Hits {
            hints,
            ..Hits::default()
        };
    }
    chrome::background(gfx);
    nav.base().draw(gfx, ctx);
    let labels = nav.tab_labels();
    let bar = TopBar {
        tabs: labels.as_deref().map(|l| (l, nav.active_tab())),
        back: nav.back_title(),
        clock,
        kind: view.kind,
        style: view.style,
        pointer: view.pointer,
    };
    let top = chrome::topbar(gfx, &bar);
    // A modal dims everything below it, top bar included.
    if let Some(modal) = nav.modal() {
        modal.draw(gfx, ctx);
    }
    let hints = chrome::hintbar(gfx, &nav.top().hints(), view.kind, view.style, view.pointer);
    Hits {
        tabs: top.tabs,
        back: top.back,
        hints,
    }
}

/// Carries out a screen's clipboard request, if it left one (FR-CAR-9): the screen layer has no
/// SDL handle, so `Ctx::clipboard_request` is the seam. A failure is logged, not fatal.
fn copy_clipboard_request(ctx: &mut Ctx, video: &VideoSubsystem) {
    if let Some(text) = ctx.clipboard_request.take()
        && let Err(e) = video.clipboard().set_clipboard_text(&text)
    {
        eprintln!("game37: can't copy to the clipboard: {e}");
    }
}

/// Local time as HH:MM.
fn clock() -> String {
    use sdl3::sys::time::{SDL_DateTime, SDL_GetCurrentTime, SDL_TimeToDateTime};
    let mut now = 0;
    let mut dt = SDL_DateTime::default();
    // SAFETY: both calls only write to the locals passed in.
    let ok = unsafe { SDL_GetCurrentTime(&mut now) && SDL_TimeToDateTime(now, &mut dt, true) };
    if ok {
        format!("{:02}:{:02}", dt.hour, dt.minute)
    } else {
        String::new()
    }
}

/// Turns SDL events into UI inputs: keys and gamepads through [`Controls`], the mouse directly.
struct InputState {
    subsystem: GamepadSubsystem,
    pads: HashMap<JoystickId, Gamepad>,
    /// Which pad plays on which port, kept for the app's whole life (FR-EMB-8): player order
    /// follows connection order, and persists across Games.
    ports: Ports<JoystickId>,
    controls: Controls,
    pointer: Option<(f32, f32)>,
}

impl InputState {
    fn new(subsystem: GamepadSubsystem) -> InputState {
        let has_pad = subsystem.gamepads().is_ok_and(|pads| !pads.is_empty());
        let kind = if has_pad {
            InputKind::Controller
        } else {
            InputKind::Keyboard
        };
        InputState {
            subsystem,
            pads: HashMap::new(),
            ports: Ports::default(),
            controls: Controls::new(kind),
            pointer: None,
        }
    }

    /// This frame's pad state per occupied port, for a running Game (see
    /// [`crate::session::sdl::pad_state`]).
    fn pad_states(&self) -> Vec<(usize, input::PadState)> {
        self.ports
            .assigned()
            .filter_map(|(port, id)| self.pads.get(&id).map(|g| (port, sdl::pad_state(g))))
            .collect()
    }

    fn view(&self) -> View {
        View {
            kind: self.controls.kind,
            style: self.controls.style,
            pointer: self.pointer,
        }
    }

    fn event(&mut self, event: Event, gfx: &mut Gfx<'_>, out: &mut Vec<Input>) -> Flow {
        let control = match event {
            Event::Quit { .. } => return Flow::Quit,
            Event::Window {
                win_event: WindowEvent::FocusLost,
                ..
            } => ControlEvent::FocusLost,

            Event::KeyDown {
                keycode: Some(Keycode::F11),
                repeat: false,
                ..
            } => {
                gfx.toggle_fullscreen();
                return Flow::Continue;
            }
            Event::KeyDown {
                keycode: Some(key),
                repeat,
                ..
            } => ControlEvent::KeyDown { key, repeat },
            Event::KeyUp {
                keycode: Some(key), ..
            } => ControlEvent::KeyUp { key },

            Event::GamepadAdded { which, .. } => {
                if let Ok(pad) = self.subsystem.open(which) {
                    self.pads.insert(which, pad);
                    self.ports.connect(which);
                }
                return Flow::Continue;
            }
            Event::GamepadRemoved { which, .. } => {
                self.pads.remove(&which);
                self.ports.disconnect(which);
                ControlEvent::PadRemoved { pad: which }
            }
            Event::GamepadButtonDown { which, button, .. } => {
                ControlEvent::PadButtonDown { pad: which, button }
            }
            Event::GamepadButtonUp { which, button, .. } => ControlEvent::PadButtonUp { pad: which, button },
            Event::GamepadAxisMotion {
                which,
                axis: axis @ (Axis::LeftX | Axis::LeftY),
                value,
                ..
            } => ControlEvent::StickMotion {
                pad: which,
                axis: if axis == Axis::LeftX {
                    StickAxis::X
                } else {
                    StickAxis::Y
                },
                value,
            },

            Event::MouseMotion { x, y, .. } => {
                let (x, y) = gfx.to_logical(x, y);
                self.controls.kind = InputKind::Keyboard;
                self.pointer = Some((x, y));
                out.push(Input::Pointer(Pointer::Move { x, y }));
                return Flow::Continue;
            }
            Event::MouseButtonUp { mouse_btn, x, y, .. } => {
                let (x, y) = gfx.to_logical(x, y);
                self.controls.kind = InputKind::Keyboard;
                let pointer = match mouse_btn {
                    MouseButton::Left => Pointer::Click { x, y },
                    MouseButton::Right => Pointer::RightClick { x, y },
                    MouseButton::X1 => Pointer::Back,
                    _ => return Flow::Continue,
                };
                out.push(Input::Pointer(pointer));
                return Flow::Continue;
            }
            Event::MouseWheel { y, .. } => {
                self.controls.kind = InputKind::Keyboard;
                out.push(Input::Pointer(Pointer::Wheel { dy: y }));
                return Flow::Continue;
            }
            _ => return Flow::Continue,
        };
        self.controls.handle(sdl3::timer::ticks(), control, out);
        Flow::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `InputState` feeds `GamepadAdded`/`GamepadRemoved` ids straight into its `Ports` (FR-EMB-8):
    /// player order follows connection order, the other pads keep their ports when one leaves, and
    /// a reconnecting id can land on a different port than before.
    #[test]
    fn ports_persist_across_added_and_removed_events_even_when_ids_reconnect_reordered() {
        let mut ports: Ports<JoystickId> = Ports::default();
        let (a, b, c) = (JoystickId::new(1), JoystickId::new(2), JoystickId::new(3));
        assert_eq!(ports.connect(a), Some(0), "player 1 by connection order");
        assert_eq!(ports.connect(b), Some(1), "player 2");
        ports.disconnect(a);
        assert_eq!(ports.connect(c), Some(0), "fills the port a left");
        assert_eq!(
            ports.connect(a),
            Some(2),
            "a reconnects after b and c, on a different port than before"
        );
        assert_eq!(ports.port_of(b), Some(1), "b kept its port throughout");
    }
}
