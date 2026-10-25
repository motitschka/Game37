//! The window, the event loop and screenshot mode.

use std::collections::HashMap;
use std::ffi::CStr;
use std::path::Path;
use std::time::{Duration, Instant};

use sdl3::event::{Event, WindowEvent};
use sdl3::gamepad::{Axis, Button, Gamepad};
use sdl3::joystick::JoystickId;
use sdl3::keyboard::{Keycode, Mod, Scancode};
use sdl3::mouse::MouseButton;
use sdl3::render::{WindowCanvas, create_renderer};
use sdl3::sys::render::SDL_RendererLogicalPresentation;
use sdl3::{GamepadSubsystem, VideoSubsystem};

use crate::artwork::Artwork;
use crate::cli::{Options, Screenshot, ShotScreen};
use crate::config::{self, Config};
use crate::ctx::{self, Controller, Ctx};
use crate::emulators::Emulators;
use crate::launch::{Launcher, NeverExits, Processes};
use crate::library::db::Library;
use crate::library::watch::LibraryWatcher;
use crate::screens;
use crate::session::input::{self, Mouse, Ports};
use crate::session::sdl;
use crate::ui::chrome::{self, TopBar};
use crate::ui::controls::{ControlEvent, Controls, StickAxis};
use crate::ui::gfx::{Gfx, WindowMode};
use crate::ui::hit;
use crate::ui::input::{Action, ButtonStyle, Input, InputKind, Pointer, TextInput};
use crate::ui::nav::{Flow, Hits, Nav};
use crate::ui::text::Fonts;
use crate::ui::theme::Colour;

pub fn run(options: Options) -> Result<(), String> {
    let path = options.config.unwrap_or_else(config::config_path);
    let loaded = config::load(&path);
    let data_dir = options.data_dir.unwrap_or_else(config::data_dir);
    let log_dir = data_dir.join("logs");
    let mut ctx = Ctx::empty();
    ctx.data_dir = data_dir;
    ctx.config_path = path;
    let has_library = match &loaded {
        Ok(Some(config)) => {
            open_library(&mut ctx, config)?;
            true
        }
        _ => false,
    };
    match options.screenshot {
        Some(shot) => {
            ctx.simulate_emulators(shot.missing_emulators.clone());
            if has_library {
                start_shot_library(&mut ctx, shot.artwork)?;
            }
            // Setup's folders go under the data dir, never in the real ~/Games.
            let mut nav = screens::start(loaded, &ctx.data_dir.join("Games"), &ctx);
            if shot.screen == Some(ShotScreen::ControllerTest) {
                open_controller_test(&mut nav, &mut ctx);
            }
            // Never starts a process: a launch shows the Playing screen for good.
            let launcher = Launcher::new(Box::new(NeverExits::default()), log_dir);
            screenshot(nav, ctx, launcher, shot)
        }
        None => {
            ctx.animate = true;
            let watcher = has_library.then(|| start_live_library(&mut ctx));
            let setup_root = dirs::home_dir().expect("no home directory").join("Games");
            let nav = screens::start(loaded, &setup_root, &ctx);
            let launcher = Launcher::new(Box::new(Processes), log_dir);
            // First-run Setup, without a library, always starts windowed.
            let fullscreen = has_library && {
                let last = ctx.library.last_fullscreen().unwrap_or_else(|e| {
                    eprintln!("game37: can't read the last window state: {e}");
                    false
                });
                starts_fullscreen(ctx.window_mode, last)
            };
            interactive(nav, ctx, watcher, launcher, fullscreen)
        }
    }
}

/// What starting with a Config does in both modes, at launch or when First-run Setup finishes
/// (FR-CFG-1): the library in the data dir, for the Config's Games Root, and the menu settings saved
/// in it.
fn open_library(ctx: &mut Ctx, config: &Config) -> Result<(), String> {
    ctx.library = Library::open(&ctx.data_dir.join("library.db")).map_err(|e| e.to_string())?;
    ctx.games_root = config.games_root.clone();
    ctx.button_style = ctx.library.button_style().unwrap_or_else(|e| {
        eprintln!("game37: can't read the button style: {e}");
        ButtonStyle::default()
    });
    ctx.window_mode = ctx.library.window_mode().unwrap_or_else(|e| {
        eprintln!("game37: can't read the window mode: {e}");
        WindowMode::default()
    });
    Ok(())
}

/// Screenshot mode's start with a library: every System scanned in the foreground, so the
/// screenshot shows every Game, and Artwork only with `--artwork`. The emulators stay simulated.
fn start_shot_library(ctx: &mut Ctx, with_artwork: bool) -> Result<(), String> {
    ctx.scan_all(ctx::now()).map_err(|e| e.to_string())?;
    if with_artwork {
        ctx.artwork = artwork(&ctx.config_path, &ctx.data_dir);
    }
    Ok(())
}

/// The interactive start with a library: the emulators detected, Artwork with the key in
/// `secrets.toml`, and the watcher, which scans every System in the background and then follows the
/// Games Root.
fn start_live_library(ctx: &mut Ctx) -> LibraryWatcher {
    ctx.emulators = Emulators::detect();
    ctx.artwork = artwork(&ctx.config_path, &ctx.data_dir);
    LibraryWatcher::start(ctx.games_root.clone())
}

/// Carries out First-run Setup's Finish (FR-CFG-4) as if Game37 had started with `config`: opens
/// its library, starts it as the mode does with `start`, and replaces Setup with Home and Consoles.
fn finish_setup<T>(
    nav: &mut Nav,
    ctx: &mut Ctx,
    config: Config,
    start: impl FnOnce(&mut Ctx) -> Result<T, String>,
) -> Result<T, String> {
    nav.leave_all(ctx);
    open_library(ctx, &config)?;
    let started = start(ctx)?;
    let root = config.games_root.clone();
    *nav = screens::start(Ok(Some(config)), &root, ctx);
    Ok(started)
}

/// `--screen controller-test`: the Controller test over Settings, with two fake controllers
/// connected, as in the mockup. The first, shown one has A and L1 held, its left stick pushed up and
/// right, and R2 pulled to 60%; the second is idle.
fn open_controller_test(nav: &mut Nav, ctx: &mut Ctx) {
    nav.push(Box::new(screens::Stub::new("Settings")));
    nav.push(Box::new(screens::ControllerTest::new()));
    let full = i16::MAX as f32;
    let mut pad = input::PadState {
        left_x: (0.62 * full) as i16,
        left_y: (-0.41 * full) as i16,
        right_trigger: (0.6 * full) as i16,
        ..Default::default()
    };
    pad.press(Button::East);
    pad.press(Button::LeftShoulder);
    ctx.controllers = vec![
        Controller {
            name: "Xbox Wireless Controller".to_owned(),
            pad,
        },
        Controller {
            name: "8BitDo Pro 2".to_owned(),
            pad: input::PadState::default(),
        },
    ];
    ctx.last_controller = Some(0);
}

/// Artwork cached under `data_dir`, fetched with the key in `secrets.toml` next to the Config at
/// `config_path`. Without a key (or with an unreadable secrets.toml) it is the cache only.
fn artwork(config_path: &Path, data_dir: &Path) -> Artwork {
    let key = config::load_api_key(config_path).unwrap_or_else(|e| {
        eprintln!("game37: {e}");
        None
    });
    Artwork::start(data_dir.join("artwork"), key)
}

/// Whether the window opens fullscreen (FR-UI-3): Remember last goes by how it was when Game37
/// last quit or F11 last toggled it, the Always modes ignore that.
fn starts_fullscreen(mode: WindowMode, last_fullscreen: bool) -> bool {
    match mode {
        WindowMode::RememberLast => last_fullscreen,
        WindowMode::AlwaysFullscreen => true,
        WindowMode::AlwaysWindowed => false,
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
    mut fullscreen: bool,
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
    if fullscreen {
        gfx.set_fullscreen(true);
    }
    let mut events = sdl.event_pump().map_err(|e| e.to_string())?;

    let mut input = InputState::new(gamepads);
    let mut hits = Hits::default();
    let mut feeds = Vec::new();
    // Whether SDL sends text input events, which follows `input.controls.text`.
    let mut text_on = false;
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
            feeds.extend(input.event(event, &mut gfx));
        }
        // F11 toggled it, or the window manager did: saved for Remember last (US-33). Read back
        // from the window, which may take a frame or two to follow the request.
        if gfx.is_fullscreen() != fullscreen {
            fullscreen = !fullscreen;
            ctx.remember_fullscreen(fullscreen);
        }
        ctx.controllers = input.controllers();
        ctx.last_controller = input.last_controller();
        ctx.ticks = sdl3::timer::ticks();
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
        // Lets a Core download (FR-EMB-2) progress every frame, not only on input.
        if nav.tick(&mut ctx) == Flow::Quit {
            break 'frames;
        }
        // Artwork that finished loading in the background (NFR-3).
        ctx.artwork.poll(&ctx.library);
        if let Some(play) = ctx.play.take() {
            let refresh = if vsync { gfx.refresh_rate() } else { None };
            sdl::start(&play, &mut nav, &ctx, gfx.gl_context(), audio.as_ref(), refresh);
        }
        feeds.push(Feed::Repeat(sdl3::timer::ticks()));
        let flow = feed(
            &mut input.controls,
            feeds.drain(..),
            &mut nav,
            &hits,
            &mut ctx,
            || video.clipboard().clipboard_text().unwrap_or_default(),
            |nav, ctx| {
                if let Some(game) = ctx.launch.take() {
                    gfx.minimize();
                    if !launcher.start(&game, nav, ctx) {
                        gfx.restore();
                    }
                }
                if let Some(play) = ctx.play.take() {
                    // Frames sync to the display only with vsync.
                    let refresh = if vsync { gfx.refresh_rate() } else { None };
                    sdl::start(&play, nav, ctx, gfx.gl_context(), audio.as_ref(), refresh);
                }
            },
        );
        if flow == Flow::Quit {
            break 'frames;
        }
        if input.controls.text != text_on {
            text_on = input.controls.text;
            gfx.set_text_input(text_on);
        }
        copy_clipboard_request(&mut ctx, &video);
        if let Some(path) = ctx.show_file_request.take() {
            crate::file_manager::show(&path);
        }
        if let Some(config) = ctx.setup_finished.take() {
            let started = finish_setup(&mut nav, &mut ctx, config, |ctx| Ok(start_live_library(ctx)))?;
            watcher = Some(started);
        }
        if let Some(session) = sdl::running(&mut nav) {
            session.set_shown(shown);
        }
        if !shown {
            continue;
        }
        let keys = events.keyboard_state();
        let pads = input.pad_states();
        let state = events.mouse_state();
        let mouse = Mouse {
            at: Some(gfx.to_logical(state.x(), state.y())),
            left: state.left(),
        };
        sdl::tick(&mut nav, &mut ctx, &mut gfx, dt, &pads, mouse, |k| {
            keys.is_scancode_pressed(k)
        });
        // The mouse cursor hides while a Game plays, unless the mouse touches its screen.
        let cursor = !sdl::playing(&mut nav) || sdl::running(&mut nav).is_some_and(|s| s.touch_screen());
        if cursor != sdl.mouse().is_cursor_showing() {
            sdl.mouse().show_cursor(cursor);
        }
        ctx.input_kind = input.controls.kind;
        hits = draw(&mut gfx, &mut nav, &ctx, &input.view(), &clock());
        gfx.present();
        if !vsync && let Some(left) = FRAME.checked_sub(frame_start.elapsed()) {
            std::thread::sleep(left);
        }
    }
    ctx.remember_fullscreen(gfx.is_fullscreen());
    close(&mut nav, &mut ctx, &mut launcher);
    Ok(())
}

/// What closing the window does once the loop has stopped.
pub(crate) fn close(nav: &mut Nav, ctx: &mut Ctx, launcher: &mut Launcher) {
    // A running Game ends first, saving it, paused or not.
    sdl::end(nav, ctx);
    // Quitting while an emulator runs leaves it running, with Playtime counted up to now.
    launcher.quit(nav, ctx, ctx::now());
    nav.leave_all(ctx);
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
        pointer: None,
    };
    let clock = clock();
    let mut hits = draw_settled(&mut gfx, &mut nav, &ctx, &view, &clock);
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
                Input::Text(_) => view.kind = InputKind::Keyboard,
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
            // Screenshot mode never opens the file manager.
            ctx.show_file_request = None;
            if let Some(config) = ctx.setup_finished.take() {
                finish_setup(&mut nav, &mut ctx, config, |ctx| {
                    start_shot_library(ctx, shot.artwork)
                })?;
            }
            if let Some(game) = ctx.launch.take() {
                launcher.start(&game, &mut nav, &ctx);
            }
            if let Some(play) = ctx.play.take() {
                sdl::start(
                    &play,
                    &mut nav,
                    &ctx,
                    gfx.gl_context(),
                    audio.as_ref(),
                    Some(SHOT_REFRESH),
                );
            }
        }
        for _ in 0..step.frames() {
            sdl::tick(
                &mut nav,
                &mut ctx,
                &mut gfx,
                1.0 / SHOT_REFRESH,
                &[],
                Mouse::default(),
                |k| esc && k == Scancode::Escape,
            );
            // A Core download (FR-EMB-2) runs on its own thread; block until it settles so the
            // screenshot is deterministic, with a cap in case it never does.
            let deadline = Instant::now() + Duration::from_secs(60);
            while nav.top().busy() && Instant::now() < deadline {
                if nav.tick(&mut ctx) == Flow::Quit {
                    break 'steps;
                }
                if let Some(play) = ctx.play.take() {
                    sdl::start(
                        &play,
                        &mut nav,
                        &ctx,
                        gfx.gl_context(),
                        audio.as_ref(),
                        Some(SHOT_REFRESH),
                    );
                }
            }
            ctx.input_kind = view.kind;
            hits = draw_settled(&mut gfx, &mut nav, &ctx, &view, &clock);
            gfx.present();
        }
    }
    for frame in 1..=shot.frames {
        sdl::tick(
            &mut nav,
            &mut ctx,
            &mut gfx,
            1.0 / SHOT_REFRESH,
            &[],
            Mouse::default(),
            |_| false,
        );
        draw_settled(&mut gfx, &mut nav, &ctx, &view, &clock);
        if frame < shot.frames {
            gfx.present();
        }
    }
    let saved = gfx.save_bmp(&shot.out);
    sdl::end(&mut nav, &mut ctx);
    saved
}

/// Draws a screenshot frame. Artwork the frame asks for (`--artwork`) loads in the background:
/// wait for it and draw the frame again once it has arrived, so the screenshot is deterministic,
/// with a cap in case it never does. Without `--artwork` nothing is ever loading.
fn draw_settled(gfx: &mut Gfx<'_>, nav: &mut Nav, ctx: &Ctx, view: &View, clock: &str) -> Hits {
    let mut hits = draw(gfx, nav, ctx, view, clock);
    let mut drawn = Instant::now();
    let deadline = drawn + Duration::from_secs(60);
    while ctx.artwork.busy() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
        ctx.artwork.poll(&ctx.library);
        // Drawing again may ask for more, which the loop then waits for too. It also draws a frame's
        // worth of time apart while waiting, as the interactive loop would: Artwork drops queued work
        // that isn't asked for again.
        if !ctx.artwork.busy() || drawn.elapsed() >= FRAME {
            hits = draw(gfx, nav, ctx, view, clock);
            drawn = Instant::now();
        }
    }
    hits
}

/// The display refresh rate screenshot mode pretends to have: each step and frame is 1/60 s.
const SHOT_REFRESH: f64 = 60.0;

/// Around a screen without chrome, such as a running Game's bars.
const BLACK: Colour = Colour::hex(0x000000);

/// What the chrome needs to know about input.
struct View {
    kind: InputKind,
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
        let hints = chrome::hintbar(gfx, &modal.hints(), view.kind, ctx.button_style, view.pointer);
        return Hits {
            hints,
            ..Hits::default()
        };
    }
    chrome::background(gfx);
    nav.base().draw(gfx, ctx);
    let labels = nav.tab_labels();
    let library = nav.has_library();
    let pill = nav.base().search_pill();
    let search = library && pill;
    let shows_clock = nav.base().clock();
    let bar = TopBar {
        // The Search screen's field takes the tabs' place too.
        tabs: labels.as_deref().filter(|_| pill).map(|l| (l, nav.active_tab())),
        back: nav.back_title(),
        clock: shows_clock.then_some(clock),
        search,
        settings: library,
        kind: view.kind,
        style: ctx.button_style,
        pointer: view.pointer,
    };
    let top = chrome::topbar(gfx, &bar);
    // A modal dims everything below it, top bar included.
    if let Some(modal) = nav.modal() {
        modal.draw(gfx, ctx);
    }
    let hints = chrome::hintbar(gfx, &nav.top().hints(), view.kind, ctx.button_style, view.pointer);
    Hits {
        tabs: top.tabs,
        back: top.back,
        hints,
        search: top.search,
        settings: top.settings,
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

/// One SDL event as the menus get it: a key or pad event or typed text, which [`Controls`] reads
/// only once the events before it are handled, or an input of its own (the mouse).
#[derive(Debug, Clone, PartialEq)]
enum Feed {
    Quit,
    /// What happened, at what time in milliseconds.
    Control(u64, ControlEvent),
    Typed(String),
    Input(Input),
    /// The held direction's repeat, if due at this time.
    Repeat(u64),
}

/// Whether the keyboard types rather than acts: only while the top screen has a text field
/// active, and never while a Game runs.
fn wants_text(nav: &mut Nav) -> bool {
    !sdl::playing(nav) && nav.top().text_input()
}

/// Handles a frame's `feeds` in order. Each is read in the text mode the top screen wants once the
/// ones before it are handled: Enter on Rename then F in the same frame types (or drops) the F,
/// never acts on it. Likewise each takes the button style as it is by then, so switching it flips
/// Confirm and Back from the next press (US-32). `started` runs after each handled input, to start
/// what it asked to launch or play. Leaves `controls.text` as the top screen wants it.
fn feed(
    controls: &mut Controls,
    feeds: impl IntoIterator<Item = Feed>,
    nav: &mut Nav,
    hits: &Hits,
    ctx: &mut Ctx,
    mut clipboard: impl FnMut() -> String,
    mut started: impl FnMut(&mut Nav, &mut Ctx),
) -> Flow {
    let mut inputs = Vec::new();
    for feed in feeds {
        controls.text = wants_text(nav);
        controls.raw = nav.top().raw_controller();
        controls.style = ctx.button_style;
        match feed {
            Feed::Quit => return Flow::Quit,
            Feed::Control(at, event) => controls.handle(at, event, &mut inputs),
            Feed::Typed(text) => controls.typed(&text, &mut inputs),
            Feed::Input(input) => inputs.push(input),
            Feed::Repeat(at) => inputs.extend(controls.poll(at)),
        }
        for input in inputs.drain(..) {
            // A running Game takes no menu input: it reads the keyboard itself.
            if sdl::playing(nav) {
                continue;
            }
            // A Paste hint click is resolved here, before `Nav::handle`, which would otherwise
            // turn it into a plain `Action::Paste` with no clipboard text attached; Ctrl+V while
            // typing arrives as that same action (see `ctrl_key_action`).
            let pastes = match input {
                Input::Action(Action::Paste) => true,
                Input::Pointer(Pointer::Click { x, y }) => hit(&hits.hints, x, y) == Some(Action::Paste),
                _ => false,
            };
            if pastes {
                for paste in paste_inputs(&clipboard()) {
                    if nav.handle(paste, hits, ctx) == Flow::Quit {
                        return Flow::Quit;
                    }
                }
                started(nav, ctx);
                continue;
            }
            if nav.handle(input, hits, ctx) == Flow::Quit {
                return Flow::Quit;
            }
            started(nav, ctx);
        }
    }
    controls.text = wants_text(nav);
    Flow::Continue
}

/// Clipboard text turned into one [`Input::Text`] per printable character (FR-UI-1): newlines,
/// tabs and other control characters are dropped, an empty clipboard becomes no `Input`s.
pub(crate) fn paste_inputs(text: &str) -> Vec<Input> {
    text.chars()
        .filter(|c| !c.is_control())
        .map(|c| Input::Text(TextInput::Char(c)))
        .collect()
}

/// Turns SDL events into [`Feed`]s: keys and gamepads for [`Controls`], the mouse directly.
struct InputState {
    subsystem: GamepadSubsystem,
    pads: HashMap<JoystickId, Gamepad>,
    /// The connected pads and their names, in connection order.
    connected: Vec<(JoystickId, String)>,
    /// The pad that last had a button pressed or a stick or trigger pushed past half way.
    last_used: Option<JoystickId>,
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
            connected: Vec::new(),
            last_used: None,
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

    /// Every connected pad's name and state this frame, in connection order.
    fn controllers(&self) -> Vec<Controller> {
        self.connected
            .iter()
            .filter_map(|(id, name)| {
                let pad = sdl::pad_state(self.pads.get(id)?);
                Some(Controller {
                    name: name.clone(),
                    pad,
                })
            })
            .collect()
    }

    /// Where the last-used pad is in [`InputState::controllers`].
    fn last_controller(&self) -> Option<usize> {
        let last = self.last_used?;
        self.connected.iter().position(|(id, _)| *id == last)
    }

    fn view(&self) -> View {
        View {
            kind: self.controls.kind,
            pointer: self.pointer,
        }
    }

    fn event(&mut self, event: Event, gfx: &mut Gfx<'_>) -> Option<Feed> {
        // Any stick or trigger pushed past half way, as SDL reports each axis on its own.
        if let Event::GamepadAxisMotion { which, value, .. } = &event
            && value.unsigned_abs() > i16::MAX as u16 / 2
        {
            self.last_used = Some(*which);
        }
        let control = match event {
            Event::Quit { .. } => return Some(Feed::Quit),
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
                return None;
            }
            Event::KeyDown {
                keycode: Some(key),
                repeat,
                keymod,
                ..
            } => ControlEvent::KeyDown {
                key,
                repeat,
                ctrl: keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD),
            },
            Event::KeyUp {
                keycode: Some(key), ..
            } => ControlEvent::KeyUp { key },
            Event::TextInput { text, .. } => return Some(Feed::Typed(text)),

            Event::GamepadAdded { which, .. } => {
                if let Ok(pad) = self.subsystem.open(which) {
                    let name = pad.name().unwrap_or_else(|| "Controller".to_owned());
                    self.connected.retain(|(id, _)| *id != which);
                    self.connected.push((which, name));
                    self.pads.insert(which, pad);
                    self.ports.connect(which);
                }
                return None;
            }
            Event::GamepadRemoved { which, .. } => {
                self.pads.remove(&which);
                self.connected.retain(|(id, _)| *id != which);
                self.ports.disconnect(which);
                ControlEvent::PadRemoved { pad: which }
            }
            Event::GamepadButtonDown { which, button, .. } => {
                self.last_used = Some(which);
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
                return Some(Feed::Input(Input::Pointer(Pointer::Move { x, y })));
            }
            Event::MouseButtonUp { mouse_btn, x, y, .. } => {
                let (x, y) = gfx.to_logical(x, y);
                self.controls.kind = InputKind::Keyboard;
                let pointer = match mouse_btn {
                    MouseButton::Left => Pointer::Click { x, y },
                    MouseButton::Right => Pointer::RightClick { x, y },
                    MouseButton::X1 => Pointer::Back,
                    _ => return None,
                };
                return Some(Feed::Input(Input::Pointer(pointer)));
            }
            Event::MouseWheel { y, .. } => {
                self.controls.kind = InputKind::Keyboard;
                return Some(Feed::Input(Input::Pointer(Pointer::Wheel { dy: y })));
            }
            _ => return None,
        };
        Some(Feed::Control(sdl3::timer::ticks(), control))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finishing_setup_opens_the_library_for_the_new_games_root_and_starts_on_home() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Games");
        std::fs::create_dir_all(root.join("nes")).unwrap();
        std::fs::write(root.join("nes/Dummy.nes"), b"").unwrap();
        let mut ctx = Ctx::empty();
        ctx.data_dir = dir.path().join("data");
        let mut nav = screens::start(Ok(None), &root, &ctx);
        assert_eq!(nav.top().title(), "First-run Setup");

        let config = Config {
            games_root: root.clone(),
        };
        finish_setup(&mut nav, &mut ctx, config, |ctx| start_shot_library(ctx, false)).unwrap();
        assert_eq!(ctx.games_root, root);
        assert!(
            ctx.data_dir.join("library.db").is_file(),
            "the library is the data dir's"
        );
        let games = ctx.library.system_games("nes", Default::default()).unwrap();
        let titles: Vec<_> = games.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, ["Dummy"]);
        assert_eq!(nav.tab_labels(), Some(vec!["Home", "Consoles"]));
        assert_eq!(nav.top().title(), "Home");
        assert!(nav.has_library(), "with Search and Settings");
    }

    /// Enter on Rename, then F in the same frame (FR-UI-1): the rename field is open by the time
    /// F is read, so F types (dropped here, as SDL's text input wasn't on yet) rather than acts as
    /// Options, which the field takes as Backspace.
    #[test]
    fn a_key_after_the_one_that_opens_a_text_field_is_read_as_typing_in_the_same_frame() {
        use crate::library::FoundGame;
        use crate::screens::Stub;
        use crate::screens::options::OptionsMenu;

        let dir = tempfile::tempdir().unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = dir.path().join("Games");
        let found = [FoundGame {
            system: "snes",
            rel_path: "snes/G0.sfc".into(),
            title: "G0".into(),
        }];
        ctx.library.sync_system("snes", &found, 0).unwrap();
        let game = ctx
            .library
            .system_games("snes", Default::default())
            .unwrap()
            .remove(0);
        let mut nav = Nav::single(Box::new(Stub::new("Home")));
        nav.push(Box::new(OptionsMenu::new(game.clone(), &ctx.games_root)));
        let mut controls = Controls::new(InputKind::Keyboard);
        let press = |key| {
            [
                Feed::Control(
                    0,
                    ControlEvent::KeyDown {
                        key,
                        repeat: false,
                        ctrl: false,
                    },
                ),
                Feed::Control(0, ControlEvent::KeyUp { key }),
            ]
        };
        let mut frame = |nav: &mut Nav, ctx: &mut Ctx, keys: &[Keycode]| {
            let feeds = keys.iter().flat_map(|&k| press(k));
            assert_eq!(
                feed(
                    &mut controls,
                    feeds,
                    nav,
                    &Hits::default(),
                    ctx,
                    String::new,
                    |_, _| {}
                ),
                Flow::Continue
            );
        };

        frame(
            &mut nav,
            &mut ctx,
            &[Keycode::Down, Keycode::Down, Keycode::Return, Keycode::F],
        );
        assert!(nav.top().text_input(), "the rename field is open");
        frame(&mut nav, &mut ctx, &[Keycode::Return]);
        assert_eq!(nav.depth(), 1, "the second Enter saved and closed the menu");
        assert_eq!(
            ctx.library.game(game.id).unwrap().unwrap().title,
            "G0",
            "F erased nothing"
        );
    }

    #[test]
    fn remember_last_starts_as_last_time_and_the_always_modes_ignore_it() {
        let cases = [
            (WindowMode::RememberLast, false, false),
            (WindowMode::RememberLast, true, true),
            (WindowMode::AlwaysFullscreen, false, true),
            (WindowMode::AlwaysFullscreen, true, true),
            (WindowMode::AlwaysWindowed, false, false),
            (WindowMode::AlwaysWindowed, true, false),
        ];
        for (mode, last, fullscreen) in cases {
            assert_eq!(
                starts_fullscreen(mode, last),
                fullscreen,
                "{mode:?}, last fullscreen {last}"
            );
        }
    }

    /// Stands in for the Settings row: records the actions it gets, and switches to the next button
    /// style on Options, which keeps its button in every style.
    struct StyleRow(std::rc::Rc<std::cell::RefCell<Vec<Action>>>);

    impl crate::ui::nav::Screen for StyleRow {
        fn title(&self) -> &str {
            "Settings"
        }
        fn handle(&mut self, input: Input, ctx: &mut Ctx) -> crate::ui::nav::Transition {
            if let Input::Action(action) = input {
                self.0.borrow_mut().push(action);
                if action == Action::Options {
                    ctx.set_button_style(ctx.button_style.next());
                }
            }
            crate::ui::nav::Transition::None
        }
        fn draw(&mut self, _: &mut Gfx<'_>, _: &Ctx) {}
        fn hints(&self) -> Vec<crate::ui::nav::Hint> {
            Vec::new()
        }
    }

    /// US-32: "Switching the setting flips confirm and back immediately", even between two presses
    /// in the same frame.
    #[test]
    fn switching_the_button_style_flips_confirm_and_back_from_the_next_press() {
        let log = std::rc::Rc::default();
        let mut nav = Nav::single(Box::new(StyleRow(std::rc::Rc::clone(&log))));
        let mut ctx = Ctx::empty();
        let mut controls = Controls::new(InputKind::Controller);
        let pad = JoystickId::new(1);
        let feeds = [Button::East, Button::West, Button::East, Button::South]
            .into_iter()
            .flat_map(|button| {
                [
                    Feed::Control(0, ControlEvent::PadButtonDown { pad, button }),
                    Feed::Control(0, ControlEvent::PadButtonUp { pad, button }),
                ]
            });
        let flow = feed(
            &mut controls,
            feeds,
            &mut nav,
            &Hits::default(),
            &mut ctx,
            String::new,
            |_, _| {},
        );
        assert_eq!(flow, Flow::Continue);
        assert_eq!(
            *log.borrow(),
            [Action::Confirm, Action::Options, Action::Back, Action::Confirm],
            "East confirms in Nintendo style, then goes back in Xbox style, where South confirms"
        );
        assert_eq!(ctx.library.button_style().unwrap(), ButtonStyle::Xbox, "saved");
    }

    /// `Ports` keyed by the SDL joystick ids `InputState` gets from `GamepadAdded` and
    /// `GamepadRemoved` (FR-EMB-8): player order follows connection order, the other pads keep
    /// their ports when one leaves, and a reconnecting id can land on a different port than before.
    #[test]
    fn joystick_ids_keep_their_ports_and_a_reconnecting_one_takes_the_lowest_free_port() {
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

    #[test]
    fn paste_inputs_keeps_printable_characters_and_drops_control_ones() {
        assert_eq!(
            paste_inputs("hé\tl\nlo"),
            ['h', 'é', 'l', 'l', 'o'].map(|c| Input::Text(TextInput::Char(c)))
        );
        assert_eq!(paste_inputs(""), [], "an empty clipboard pastes nothing");
    }

    /// A screen that records every `Input` it is handed, for the Paste tests below.
    struct Probe(std::rc::Rc<std::cell::RefCell<Vec<Input>>>);

    impl crate::ui::nav::Screen for Probe {
        fn title(&self) -> &str {
            "Probe"
        }
        fn handle(&mut self, input: Input, _: &mut Ctx) -> crate::ui::nav::Transition {
            self.0.borrow_mut().push(input);
            crate::ui::nav::Transition::None
        }
        fn draw(&mut self, _: &mut Gfx<'_>, _: &Ctx) {}
        fn hints(&self) -> Vec<crate::ui::nav::Hint> {
            Vec::new()
        }
        fn text_input(&self) -> bool {
            true
        }
    }

    /// Ctrl+V arrives as a plain `Action::Paste`: `feed` reads the clipboard through the closure
    /// it is given and turns it into the typed characters, never handing the bare action on.
    #[test]
    fn feed_turns_the_paste_action_into_the_clipboards_characters() {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut nav = Nav::single(Box::new(Probe(log.clone())));
        let mut ctx = Ctx::empty();
        let mut controls = Controls::new(InputKind::Keyboard);
        let flow = feed(
            &mut controls,
            [Feed::Input(Input::Action(Action::Paste))],
            &mut nav,
            &Hits::default(),
            &mut ctx,
            || "hé\n\tllo".to_owned(),
            |_, _| {},
        );
        assert_eq!(flow, Flow::Continue);
        assert_eq!(*log.borrow(), paste_inputs("héllo"));
    }

    /// A click on the Paste hint is resolved the same way, before `Nav::handle` turns it into an
    /// action of its own.
    #[test]
    fn feed_turns_a_click_on_the_paste_hint_into_the_clipboards_characters() {
        use crate::ui::Rect;

        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut nav = Nav::single(Box::new(Probe(log.clone())));
        let mut ctx = Ctx::empty();
        let mut controls = Controls::new(InputKind::Keyboard);
        let hits = Hits {
            hints: vec![(Rect::new(0.0, 0.0, 20.0, 20.0), Action::Paste)],
            ..Hits::default()
        };
        let flow = feed(
            &mut controls,
            [Feed::Input(Input::Pointer(Pointer::Click { x: 5.0, y: 5.0 }))],
            &mut nav,
            &hits,
            &mut ctx,
            || "ab".to_owned(),
            |_, _| {},
        );
        assert_eq!(flow, Flow::Continue);
        assert_eq!(*log.borrow(), paste_inputs("ab"));
    }
}
