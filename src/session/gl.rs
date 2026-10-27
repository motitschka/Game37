//! The OpenGL host for Cores that render with OpenGL: a context of the Core's own on Game37's
//! window, next to the SDL renderer's. It shares nothing with the renderer's: the Core's frames
//! are read back and drawn like any other.

use std::ffi::c_int;

use sdl3::sys::video::{
    SDL_GL_CONTEXT_FLAGS, SDL_GL_CONTEXT_MAJOR_VERSION, SDL_GL_CONTEXT_MINOR_VERSION,
    SDL_GL_CONTEXT_PROFILE_COMPATIBILITY, SDL_GL_CONTEXT_PROFILE_CORE, SDL_GL_CONTEXT_PROFILE_MASK,
    SDL_GL_CreateContext, SDL_GL_DestroyContext, SDL_GL_GetAttribute, SDL_GL_GetProcAddress,
    SDL_GL_GetSwapInterval, SDL_GL_MakeCurrent, SDL_GL_SetAttribute, SDL_GL_SetSwapInterval, SDL_GLAttr,
    SDL_GLContext,
};
use sdl3::video::Window;

use crate::libretro::{GlHost, GlRequest, HwGetProcAddressFn};

/// A Core's OpenGL context next to the SDL renderer's, on the same window.
pub struct SdlGl {
    /// Kept alive (it is reference counted) for as long as the Core's context may use it.
    window: Window,
    /// The SDL renderer's context.
    frontend: SDL_GLContext,
    /// The Core's context, null until made.
    core: SDL_GLContext,
}

impl SdlGl {
    /// The host for `window`, whose SDL renderer draws with the OpenGL context `frontend`.
    pub fn new(window: Window, frontend: SDL_GLContext) -> SdlGl {
        SdlGl {
            window,
            frontend,
            core: std::ptr::null_mut(),
        }
    }

    fn make_current(&self, context: SDL_GLContext) {
        // SAFETY: the window is alive (held by `self`) and `context` is a live context made on it.
        if !unsafe { SDL_GL_MakeCurrent(self.window.raw(), context) } {
            eprintln!("game37: can't switch OpenGL contexts: {}", sdl3::get_error());
        }
    }
}

/// Puts back the frontend's swap interval `vsync` read before a Core's context was made, with the
/// frontend's context current.
fn restore_vsync(vsync: Option<c_int>) {
    // SAFETY: a plain setter; the frontend's context is current.
    if let Some(vsync) = vsync
        && !unsafe { SDL_GL_SetSwapInterval(vsync) }
    {
        eprintln!("game37: can't restore vsync: {}", sdl3::get_error());
    }
}

/// The context attributes a Core's context is made with, as `(attribute, value)`.
fn attributes(request: GlRequest) -> [(SDL_GLAttr, c_int); 4] {
    let profile = if request.core_profile {
        SDL_GL_CONTEXT_PROFILE_CORE
    } else {
        SDL_GL_CONTEXT_PROFILE_COMPATIBILITY
    };
    // A compatibility context asks for 2.1 and gets the newest version the driver has.
    let (major, minor) = if request.core_profile {
        (request.major as c_int, request.minor as c_int)
    } else {
        (2, 1)
    };
    [
        (SDL_GL_CONTEXT_PROFILE_MASK, profile.0 as c_int),
        (SDL_GL_CONTEXT_MAJOR_VERSION, major),
        (SDL_GL_CONTEXT_MINOR_VERSION, minor),
        (SDL_GL_CONTEXT_FLAGS, 0),
    ]
}

impl GlHost for SdlGl {
    fn create_context(&mut self, request: GlRequest) -> Result<(), String> {
        if !self.core.is_null() {
            return Err("the Core already has a context".into());
        }
        self.make_current(self.frontend);
        // SDL resets its swap interval to 0 when it makes any context, and on Wayland that one
        // setting is the whole process's: the renderer would lose vsync for good. It's put back
        // once the frontend's context is current again.
        let mut vsync = 0;
        // SAFETY: a plain query with a live out-pointer.
        let vsync = unsafe { SDL_GL_GetSwapInterval(&mut vsync) }.then_some(vsync);
        let wanted = attributes(request);
        let mut saved = wanted;
        // SAFETY: plain attribute calls; each pointer is to a live local.
        let core = unsafe {
            for ((attr, value), (_, old)) in wanted.iter().zip(saved.iter_mut()) {
                SDL_GL_GetAttribute(*attr, old);
                SDL_GL_SetAttribute(*attr, *value);
            }
            let core = SDL_GL_CreateContext(self.window.raw());
            // Later contexts (a new renderer) are made as before.
            for (attr, old) in saved {
                SDL_GL_SetAttribute(attr, old);
            }
            core
        };
        if core.is_null() {
            let e = sdl3::get_error().to_string();
            self.make_current(self.frontend);
            restore_vsync(vsync);
            let kind = if request.core_profile {
                "core"
            } else {
                "compatibility"
            };
            return Err(format!(
                "no OpenGL {}.{} {kind} context: {e}",
                request.major, request.minor
            ));
        }
        self.core = core;
        // Making a context makes it current.
        self.make_current(self.frontend);
        restore_vsync(vsync);
        Ok(())
    }

    fn make_core_current(&mut self) {
        if !self.core.is_null() {
            self.make_current(self.core);
        }
    }

    fn make_frontend_current(&mut self) {
        self.make_current(self.frontend);
    }

    /// SDL's lookup. SDL documents it as main-thread only; it is published for any thread because
    /// on X11 and Wayland it is `glXGetProcAddress`/`eglGetProcAddress` underneath, which are
    /// thread-safe. If a Core ever looks functions up on its own thread on another platform, hand
    /// out the GLX/EGL lookup directly instead. SDL's GL library stays loaded while the window,
    /// held by the host, exists.
    fn loader(&self) -> HwGetProcAddressFn {
        SDL_GL_GetProcAddress
    }

    fn destroy_context(&mut self) {
        self.make_current(self.frontend);
        if !self.core.is_null() {
            // SAFETY: the Core's context was made by `create_context` and isn't current.
            unsafe { SDL_GL_DestroyContext(self.core) };
            self.core = std::ptr::null_mut();
        }
    }
}

impl Drop for SdlGl {
    fn drop(&mut self) {
        self.destroy_context();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(core_profile: bool, major: u32, minor: u32) -> GlRequest {
        GlRequest {
            core_profile,
            major,
            minor,
        }
    }

    #[test]
    fn a_core_profile_context_gets_the_version_asked_for() {
        let attrs = attributes(request(true, 3, 3));
        assert!(attrs.contains(&(
            SDL_GL_CONTEXT_PROFILE_MASK,
            SDL_GL_CONTEXT_PROFILE_CORE.0 as c_int
        )));
        assert!(attrs.contains(&(SDL_GL_CONTEXT_MAJOR_VERSION, 3)));
        assert!(attrs.contains(&(SDL_GL_CONTEXT_MINOR_VERSION, 3)));
    }

    #[test]
    fn a_compatibility_context_asks_for_2_1() {
        let attrs = attributes(request(false, 0, 0));
        assert!(attrs.contains(&(
            SDL_GL_CONTEXT_PROFILE_MASK,
            SDL_GL_CONTEXT_PROFILE_COMPATIBILITY.0 as c_int
        )));
        assert!(attrs.contains(&(SDL_GL_CONTEXT_MAJOR_VERSION, 2)));
        assert!(attrs.contains(&(SDL_GL_CONTEXT_MINOR_VERSION, 1)));
    }
}

#[cfg(test)]
mod real_core_tests {
    use std::collections::HashSet;
    use std::path::Path;

    use sdl3::pixels::Color;
    use sdl3::render::{Canvas, create_renderer};
    use sdl3::sys::render::SDL_FlushRenderer;
    use sdl3::sys::video::SDL_GL_GetCurrentContext;
    use tempfile::TempDir;

    use super::*;
    use crate::libretro::tests::serial;
    use crate::libretro::{Core, CoreDirs, Frame, PixelFormat};
    use crate::session::glx::GlxGl;

    const TESTDATA: &str = "/home/matei/Game37/.testdata";

    /// Which [`GlHost`] a test runs its Core on.
    #[derive(Clone, Copy)]
    enum Host {
        Sdl,
        Glx,
    }

    /// A Core started through an OpenGL host beside the renderer of a hidden offscreen window.
    /// Fields drop in order: the Core first, while its window lives.
    struct OnGl {
        core: Core,
        canvas: Canvas<Window>,
        frontend: SDL_GLContext,
        _dirs: TempDir,
        _video: sdl3::VideoSubsystem,
        _sdl: sdl3::Sdl,
    }

    impl OnGl {
        /// Runs `frames` frames; the renderer's context is current again after every one.
        fn run(&mut self, frames: usize) {
            for _ in 0..frames {
                self.core.run_frame(&Default::default());
                // SAFETY: a plain query.
                assert_eq!(
                    unsafe { SDL_GL_GetCurrentContext() },
                    self.frontend,
                    "the renderer's context"
                );
            }
        }
    }

    /// Runs `game` on `core` for `frames` frames through an OpenGL host beside the renderer of a
    /// hidden offscreen window, with `system` as its system directory; returns the last frame.
    /// The renderer's context is current again after every frame.
    fn run_on_gl(core: &str, game: &str, system: &Path, frames: usize, host: Host) -> Frame {
        let mut gl = start_on_gl(core, game, system, (640, 480), host);
        gl.run(frames);
        gl.core.frame().clone()
    }

    /// Starts `game` on `core` with `system` as its system directory, in a `width × height`
    /// window.
    fn start_on_gl(core: &str, game: &str, system: &Path, (width, height): (u32, u32), host: Host) -> OnGl {
        let data = Path::new(TESTDATA);
        sdl3::hint::set("SDL_VIDEO_DRIVER", "offscreen");
        let sdl = sdl3::init().unwrap();
        let video = sdl.video().unwrap();
        let window = video.window("hw", width, height).hidden().build().unwrap();
        let canvas = create_renderer(window, Some(c"opengl")).unwrap();
        // SAFETY: the renderer is alive; flushing makes its context current.
        let frontend = unsafe {
            SDL_FlushRenderer(canvas.raw());
            SDL_GL_GetCurrentContext()
        };
        assert!(!frontend.is_null(), "{}", sdl3::get_error());
        // Vsync on, as Game37's renderer asks for it.
        // SAFETY: a plain setter; the renderer's context is current.
        assert!(unsafe { SDL_GL_SetSwapInterval(1) }, "{}", sdl3::get_error());
        let window = canvas.window().clone();
        let host: Box<dyn GlHost> = match host {
            Host::Sdl => Box::new(SdlGl::new(window, frontend)),
            Host::Glx => Box::new(GlxGl::open(None, Some((window, frontend))).unwrap()),
        };

        let dirs = TempDir::new().unwrap();
        let core = Core::load(
            &data.join("data/cores").join(core),
            &data.join("games").join(game),
            CoreDirs {
                system: system.to_path_buf(),
                save: dirs.path().join("saves"),
            },
            &[],
            Some(host),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        OnGl {
            core,
            canvas,
            frontend,
            _dirs: dirs,
            _video: video,
            _sdl: sdl,
        }
    }

    /// The window filled with a green texture by the renderer, as RGBA bytes.
    fn fill_green(canvas: &mut Canvas<Window>) -> Vec<u8> {
        let creator = canvas.texture_creator();
        let mut texture = creator
            .create_texture_static(sdl3::pixels::PixelFormat::RGBA32, 4, 4)
            .unwrap();
        texture.update(None, &[0, 200, 0, 255].repeat(16), 16).unwrap();
        canvas.set_draw_color(Color::RGB(200, 0, 0));
        canvas.clear();
        canvas.copy(&texture, None, None).unwrap();
        let surface = canvas.read_pixels(None).unwrap();
        let surface = surface.convert_format(sdl3::pixels::PixelFormat::RGBA32).unwrap();
        surface.with_lock(|pixels| pixels.to_vec())
    }

    /// A frame read back from OpenGL: only those are RGBA.
    fn assert_drawn_with_gl(frame: &Frame) {
        assert_eq!(frame.format, PixelFormat::Rgba32, "the Core drew with OpenGL");
        assert!(frame.width > 0 && frame.height > 0);
        assert_eq!(frame.data.len(), frame.pitch * frame.height as usize);
    }

    /// PPSSPP draws CMFileManager's file list with OpenGL on the GLX host. The file list shows
    /// between frames 600 and 900 (also with PPSSPP's software renderer).
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn hw_ppsspp_draws_cmfilemanager() {
        let _serial = serial();
        let system = TempDir::new().unwrap();
        std::os::unix::fs::symlink(
            Path::new(TESTDATA).join("data/system/PPSSPP"),
            system.path().join("PPSSPP"),
        )
        .unwrap();
        let frame = run_on_gl(
            "ppsspp_libretro.so",
            "psp/CMFileManager/EBOOT.PBP",
            system.path(),
            900,
            Host::Glx,
        );
        assert_drawn_with_gl(&frame);
        let colours: HashSet<&[u8]> = frame.data.chunks(4).map(|p| &p[..3]).collect();
        assert!(colours.len() > 1, "a single colour: {colours:?}");
    }

    /// Mupen64Plus-Next runs N64 NICCC with OpenGL without crashing. The demo shows only black on
    /// this build even with all-software options, so the picture isn't checked.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn hw_mupen64plus_next_runs_n64_niccc() {
        let _serial = serial();
        let system = TempDir::new().unwrap();
        let frame = run_on_gl(
            "mupen64plus_next_libretro.so",
            "n64/N64 NICCC (World).n64",
            system.path(),
            300,
            Host::Sdl,
        );
        assert_drawn_with_gl(&frame);
    }

    /// Making Mupen64Plus-Next's context leaves the renderer's vsync on. SDL resets the swap
    /// interval for every new context, and on Wayland that's the whole process's setting: DS
    /// Games played after an N64 Game ran at the screen's present rate, several times too fast.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn hw_mupen64plus_next_leaves_the_renderers_vsync_on() {
        let _serial = serial();
        let system = TempDir::new().unwrap();
        let mut gl = start_on_gl(
            "mupen64plus_next_libretro.so",
            "n64/N64 NICCC (World).n64",
            system.path(),
            (640, 480),
            Host::Sdl,
        );
        gl.run(10);
        let mut vsync = -2;
        // SAFETY: a plain query with a live out-pointer; the renderer's context is current.
        assert!(
            unsafe { SDL_GL_GetSwapInterval(&mut vsync) },
            "{}",
            sdl3::get_error()
        );
        assert_eq!(vsync, 1, "the renderer's vsync");
    }

    /// Mupen64Plus-Next saves, restores and resets with its own context current: the renderer
    /// still fills its whole window afterwards. With the renderer's context current, GLideN64
    /// changes its viewport and texture state behind its back.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn hw_mupen64plus_next_states_leave_the_renderer_alone() {
        let _serial = serial();
        let system = TempDir::new().unwrap();
        let mut gl = start_on_gl(
            "mupen64plus_next_libretro.so",
            "n64/N64 NICCC (World).n64",
            system.path(),
            (1280, 720),
            Host::Sdl,
        );
        gl.run(300);
        let green = |pixels: &[u8]| pixels.chunks(4).filter(|p| p[..3] == [0, 200, 0]).count();
        assert_eq!(green(&fill_green(&mut gl.canvas)), 1280 * 720, "green before");
        let state = gl.core.serialize().unwrap();
        gl.core.unserialize(&state).unwrap();
        gl.core.reset();
        // SAFETY: a plain query.
        assert_eq!(
            unsafe { SDL_GL_GetCurrentContext() },
            gl.frontend,
            "the renderer's context"
        );
        assert_eq!(
            green(&fill_green(&mut gl.canvas)),
            1280 * 720,
            "the whole window is green"
        );
    }
}
