//! The OpenGL host for Cores that render with OpenGL: a context of the Core's own on Game37's
//! window, sharing textures with the SDL renderer's, so the renderer can draw the Core's frames.

use std::ffi::{CStr, c_int};

use sdl3::sys::video::{
    SDL_GL_CONTEXT_FLAGS, SDL_GL_CONTEXT_MAJOR_VERSION, SDL_GL_CONTEXT_MINOR_VERSION,
    SDL_GL_CONTEXT_PROFILE_COMPATIBILITY, SDL_GL_CONTEXT_PROFILE_CORE, SDL_GL_CONTEXT_PROFILE_MASK,
    SDL_GL_CreateContext, SDL_GL_DestroyContext, SDL_GL_GetAttribute, SDL_GL_GetProcAddress,
    SDL_GL_MakeCurrent, SDL_GL_SHARE_WITH_CURRENT_CONTEXT, SDL_GL_SetAttribute, SDL_GLAttr, SDL_GLContext,
};
use sdl3::video::Window;

use crate::libretro::{GlHost, GlRequest};

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

/// The context attributes a Core's context is made with, as `(attribute, value)`.
fn attributes(request: GlRequest) -> [(SDL_GLAttr, c_int); 5] {
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
        (SDL_GL_SHARE_WITH_CURRENT_CONTEXT, 1),
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
        let wanted = attributes(request);
        let mut saved = wanted;
        // SAFETY: plain attribute calls; each pointer is to a live local.
        let core = unsafe {
            for ((attr, value), (_, old)) in wanted.iter().zip(saved.iter_mut()) {
                SDL_GL_GetAttribute(*attr, old);
                SDL_GL_SetAttribute(*attr, *value);
            }
            // Shares with the current context, the renderer's.
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
            let kind = if request.core_profile { "core" } else { "compatibility" };
            return Err(format!(
                "no OpenGL {}.{} {kind} context: {e}",
                request.major, request.minor
            ));
        }
        self.core = core;
        // Making a context makes it current.
        self.make_current(self.frontend);
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

    fn proc_address(&self, name: &CStr) -> crate::libretro::ProcAddress {
        // SAFETY: `name` is NUL-terminated; SDL's GL library is loaded, as a GL window exists.
        unsafe { SDL_GL_GetProcAddress(name.as_ptr()) }
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
        assert!(attrs.contains(&(SDL_GL_SHARE_WITH_CURRENT_CONTEXT, 1)));
        assert!(attrs.contains(&(SDL_GL_CONTEXT_PROFILE_MASK, SDL_GL_CONTEXT_PROFILE_CORE.0 as c_int)));
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

    use sdl3::render::create_renderer;
    use sdl3::sys::render::SDL_FlushRenderer;
    use sdl3::sys::video::SDL_GL_GetCurrentContext;
    use tempfile::TempDir;

    use super::*;
    use crate::libretro::tests::serial;
    use crate::libretro::{Core, CoreDirs, PixelFormat, Video};
    use crate::session::states;

    const TESTDATA: &str = "/home/matei/Game37/.testdata";

    /// Mupen64Plus-Next draws N64 NICCC with OpenGL into the FBO, on a hidden offscreen window.
    #[test]
    #[ignore = "needs /home/matei/Game37/.testdata"]
    fn hw_mupen64plus_next_draws_into_the_fbo() {
        let _serial = serial();
        let data = Path::new(TESTDATA);
        sdl3::hint::set("SDL_VIDEO_DRIVER", "offscreen");
        let sdl = sdl3::init().unwrap();
        let video = sdl.video().unwrap();
        let window = video.window("hw", 640, 480).hidden().build().unwrap();
        let canvas = create_renderer(window, Some(c"opengl")).unwrap();
        // SAFETY: the renderer is alive; flushing makes its context current.
        let frontend = unsafe {
            SDL_FlushRenderer(canvas.raw());
            SDL_GL_GetCurrentContext()
        };
        assert!(!frontend.is_null(), "{}", sdl3::get_error());
        let host = SdlGl::new(canvas.window().clone(), frontend);

        let dirs = TempDir::new().unwrap();
        let mut core = Core::load(
            &data.join("data/cores/mupen64plus_next_libretro.so"),
            &data.join("games/n64/N64 NICCC (World).n64"),
            CoreDirs {
                system: dirs.path().join("system"),
                save: dirs.path().join("saves"),
            },
            Some(Box::new(host)),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let mut hw = None;
        for _ in 0..std::env::var("HWN").map_or(300, |v| v.parse().unwrap()) {
            if let Video::Hardware(frame) = core.run_frame(&Default::default()).video {
                hw = Some(frame);
            }
        }
        let frame = core.snapshot();
        if let Ok(out) = std::env::var("GAME37_HW_PNG") {
            std::fs::write(out, states::encode_png(&states::frame_rgb(&frame)).unwrap()).unwrap();
        }
        let hw = hw.expect("the Core drew with OpenGL");
        eprintln!("DBG {hw:?}");
        assert!(hw.width > 0 && hw.height > 0 && hw.texture != 0, "{hw:?}");
        assert_eq!((frame.width, frame.height), (hw.width, hw.height));
        assert_eq!(frame.format, PixelFormat::Xrgb8888);
        let colours: HashSet<&[u8]> = frame.data.chunks(4).map(|p| &p[..3]).collect();
        assert!(colours.len() > 1, "a single colour: {colours:?}");
        if let Ok(out) = std::env::var("GAME37_HW_PNG") {
            std::fs::write(out, states::encode_png(&states::frame_rgb(&frame)).unwrap()).unwrap();
        }
    }
}
