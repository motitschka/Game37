//! The window, the event loop and screenshot mode.

use std::collections::HashMap;
use std::ffi::CStr;

use sdl3::event::{Event, WindowEvent};
use sdl3::gamepad::{Axis, Gamepad};
use sdl3::joystick::JoystickId;
use sdl3::keyboard::Keycode;
use sdl3::mouse::MouseButton;
use sdl3::render::{WindowCanvas, create_renderer};
use sdl3::sys::render::SDL_RendererLogicalPresentation;
use sdl3::{GamepadSubsystem, VideoSubsystem};

use crate::cli::{Options, Screenshot};
use crate::config;
use crate::ctx::{self, Ctx};
use crate::library::db::Library;
use crate::screens;
use crate::ui::chrome::{self, TopBar};
use crate::ui::gfx::Gfx;
use crate::ui::input::{
    Action, ButtonStyle, Input, InputKind, Pointer, Repeat, button_action, key_action, stick_direction,
};
use crate::ui::nav::{Flow, Hits, Nav};
use crate::ui::text::Fonts;

pub fn run(options: Options) -> Result<(), String> {
    let path = options.config.unwrap_or_else(config::config_path);
    let loaded = config::load(&path);
    let ctx = match &loaded {
        Ok(Some(config)) => {
            let data_dir = options.data_dir.unwrap_or_else(config::data_dir);
            let library = Library::open(&data_dir.join("library.db")).map_err(|e| e.to_string())?;
            let mut ctx = Ctx::new(library, config.games_root.clone());
            ctx.scan_all(ctx::now()).map_err(|e| e.to_string())?;
            ctx
        }
        _ => Ctx::empty(),
    };
    let nav = screens::start(loaded, &path, &ctx);
    match options.screenshot {
        Some(shot) => screenshot(nav, ctx, shot),
        None => interactive(nav, Ctx { animate: true, ..ctx }),
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

fn interactive(mut nav: Nav, mut ctx: Ctx) -> Result<(), String> {
    let sdl = sdl3::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
    let gamepads = sdl.gamepad().map_err(|e| e.to_string())?;
    let fonts = Fonts::new()?;
    // OpenGL, because later milestones host libretro OpenGL cores in this renderer.
    let canvas = canvas(&video, 1600, 900, c"opengl")?;
    // SAFETY: the renderer is alive; vsync is best effort.
    unsafe { sdl3::sys::render::SDL_SetRenderVSync(canvas.raw(), 1) };
    let creator = canvas.texture_creator();
    let mut gfx = Gfx::new(canvas, &creator, fonts);
    let mut events = sdl.event_pump().map_err(|e| e.to_string())?;

    let mut input = InputState::new(gamepads);
    let mut hits = Hits::default();
    let mut inputs = Vec::new();
    loop {
        for event in events.poll_iter() {
            if input.event(event, &mut gfx, &mut inputs) == Flow::Quit {
                return Ok(());
            }
        }
        if let Some(dir) = input.repeat.poll(sdl3::timer::ticks()) {
            inputs.push(Input::Action(dir));
        }
        for i in inputs.drain(..) {
            if nav.handle(i, &hits, &mut ctx) == Flow::Quit {
                return Ok(());
            }
        }
        hits = draw(&mut gfx, &mut nav, &ctx, &input.view(), &clock());
        gfx.present();
    }
}

fn screenshot(mut nav: Nav, mut ctx: Ctx, shot: Screenshot) -> Result<(), String> {
    sdl3::hint::set("SDL_VIDEO_DRIVER", "offscreen");
    let sdl = sdl3::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
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
    for step in shot.steps {
        for i in step.inputs() {
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
                break;
            }
        }
        hits = draw(&mut gfx, &mut nav, &ctx, &view, &clock);
        gfx.present();
    }
    draw(&mut gfx, &mut nav, &ctx, &view, &clock);
    gfx.save_bmp(&shot.out)
}

/// What the chrome needs to know about input.
struct View {
    kind: InputKind,
    style: ButtonStyle,
    pointer: Option<(f32, f32)>,
}

/// Draws a frame (without presenting it) and returns where its clickable chrome went.
fn draw(gfx: &mut Gfx<'_>, nav: &mut Nav, ctx: &Ctx, view: &View, clock: &str) -> Hits {
    chrome::background(gfx);
    nav.top().draw(gfx, ctx);
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
    let hints = chrome::hintbar(gfx, &nav.top().hints(), view.kind, view.style, view.pointer);
    Hits {
        tabs: top.tabs,
        back: top.back,
        hints,
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

/// Turns SDL events into UI inputs, tracking held directions, sticks and the last input kind.
struct InputState {
    subsystem: GamepadSubsystem,
    pads: HashMap<JoystickId, Gamepad>,
    /// Left stick axes and the direction they last pointed, per pad.
    sticks: HashMap<JoystickId, (i16, i16, Option<Action>)>,
    repeat: Repeat,
    kind: InputKind,
    style: ButtonStyle,
    pointer: Option<(f32, f32)>,
}

impl InputState {
    fn new(subsystem: GamepadSubsystem) -> InputState {
        let has_pad = subsystem.gamepads().is_ok_and(|pads| !pads.is_empty());
        InputState {
            subsystem,
            pads: HashMap::new(),
            sticks: HashMap::new(),
            repeat: Repeat::default(),
            kind: if has_pad {
                InputKind::Controller
            } else {
                InputKind::Keyboard
            },
            style: ButtonStyle::default(),
            pointer: None,
        }
    }

    fn view(&self) -> View {
        View {
            kind: self.kind,
            style: self.style,
            pointer: self.pointer,
        }
    }

    fn press(&mut self, action: Action, out: &mut Vec<Input>) {
        if action.is_direction() {
            self.repeat.press(action, sdl3::timer::ticks());
        }
        out.push(Input::Action(action));
    }

    fn event(&mut self, event: Event, gfx: &mut Gfx<'_>, out: &mut Vec<Input>) -> Flow {
        match event {
            Event::Quit { .. } => return Flow::Quit,
            Event::Window {
                win_event: WindowEvent::FocusLost,
                ..
            } => self.repeat.release_all(),

            Event::KeyDown {
                keycode: Some(Keycode::F11),
                repeat: false,
                ..
            } => gfx.toggle_fullscreen(),
            Event::KeyDown {
                keycode: Some(key),
                repeat: false,
                ..
            } => {
                self.kind = InputKind::Keyboard;
                if let Some(action) = key_action(key) {
                    self.press(action, out);
                }
            }
            Event::KeyUp {
                keycode: Some(key), ..
            } => {
                if let Some(action) = key_action(key) {
                    self.repeat.release(action);
                }
            }

            Event::GamepadAdded { which, .. } => {
                if let Ok(pad) = self.subsystem.open(which) {
                    self.pads.insert(which, pad);
                }
            }
            Event::GamepadRemoved { which, .. } => {
                self.pads.remove(&which);
                self.sticks.remove(&which);
                self.repeat.release_all();
            }
            Event::GamepadButtonDown { button, .. } => {
                self.kind = InputKind::Controller;
                if let Some(action) = button_action(self.style, button) {
                    self.press(action, out);
                }
            }
            Event::GamepadButtonUp { button, .. } => {
                if let Some(action) = button_action(self.style, button) {
                    self.repeat.release(action);
                }
            }
            Event::GamepadAxisMotion {
                which,
                axis: axis @ (Axis::LeftX | Axis::LeftY),
                value,
                ..
            } => {
                let stick = self.sticks.entry(which).or_insert((0, 0, None));
                match axis {
                    Axis::LeftX => stick.0 = value,
                    _ => stick.1 = value,
                }
                let (old, new) = (stick.2, stick_direction(stick.0, stick.1));
                if new != old {
                    stick.2 = new;
                    if let Some(old) = old {
                        self.repeat.release(old);
                    }
                    if let Some(new) = new {
                        self.kind = InputKind::Controller;
                        self.press(new, out);
                    }
                }
            }

            Event::MouseMotion { x, y, .. } => {
                let (x, y) = gfx.to_logical(x, y);
                self.kind = InputKind::Keyboard;
                self.pointer = Some((x, y));
                out.push(Input::Pointer(Pointer::Move { x, y }));
            }
            Event::MouseButtonUp { mouse_btn, x, y, .. } => {
                let (x, y) = gfx.to_logical(x, y);
                self.kind = InputKind::Keyboard;
                let pointer = match mouse_btn {
                    MouseButton::Left => Pointer::Click { x, y },
                    MouseButton::Right => Pointer::RightClick { x, y },
                    MouseButton::X1 => Pointer::Back,
                    _ => return Flow::Continue,
                };
                out.push(Input::Pointer(pointer));
            }
            Event::MouseWheel { y, .. } => {
                self.kind = InputKind::Keyboard;
                out.push(Input::Pointer(Pointer::Wheel { dy: y }));
            }
            _ => {}
        }
        Flow::Continue
    }
}
