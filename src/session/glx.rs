//! The GLX host for Cores whose OpenGL loader needs GLX (PPSSPP calls `glewInit()`, which fails on
//! EGL): a context of the Core's own on the X display (XWayland on a Wayland desktop), current on a
//! small pbuffer, whatever video driver SDL uses. The Core draws into its FBO and its frames are
//! read back, so no window is needed.
//!
//! libX11 and libGL are loaded when a host is opened, so Game37 runs without them.

use std::ffi::{CStr, c_char, c_int, c_uchar, c_ulong, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use libloading::Library;
use sdl3::sys::video::{SDL_GL_MakeCurrent, SDL_GLContext};
use sdl3::video::Window;

use crate::libretro::{GlHost, GlRequest, ProcAddress};

type Display = c_void;
type GlxFbConfig = *mut c_void;
type GlxContext = *mut c_void;
type GlxDrawable = c_ulong;

/// Xlib's `XErrorEvent`, of which only the layout matters here.
#[repr(C)]
struct XErrorEvent {
    kind: c_int,
    display: *mut Display,
    resource_id: c_ulong,
    serial: c_ulong,
    error_code: c_uchar,
    request_code: c_uchar,
    minor_code: c_uchar,
}

type XErrorHandler = Option<unsafe extern "C" fn(*mut Display, *mut XErrorEvent) -> c_int>;
type CreateContextAttribs =
    unsafe extern "C" fn(*mut Display, GlxFbConfig, GlxContext, c_int, *const c_int) -> GlxContext;

const GLX_DOUBLEBUFFER: c_int = 5;
const GLX_RED_SIZE: c_int = 8;
const GLX_GREEN_SIZE: c_int = 9;
const GLX_BLUE_SIZE: c_int = 10;
const GLX_ALPHA_SIZE: c_int = 11;
const GLX_DRAWABLE_TYPE: c_int = 0x8010;
const GLX_RENDER_TYPE: c_int = 0x8011;
const GLX_RGBA_BIT: c_int = 0x1;
const GLX_PBUFFER_BIT: c_int = 0x4;
const GLX_PBUFFER_HEIGHT: c_int = 0x8040;
const GLX_PBUFFER_WIDTH: c_int = 0x8041;
const GLX_CONTEXT_MAJOR_VERSION_ARB: c_int = 0x2091;
const GLX_CONTEXT_MINOR_VERSION_ARB: c_int = 0x2092;
const GLX_CONTEXT_PROFILE_MASK_ARB: c_int = 0x9126;
const GLX_CONTEXT_CORE_PROFILE_BIT_ARB: c_int = 0x1;
const GLX_CONTEXT_COMPATIBILITY_PROFILE_BIT_ARB: c_int = 0x2;

/// The libX11 and GLX functions the host calls.
struct Glx {
    open_display: unsafe extern "C" fn(*const c_char) -> *mut Display,
    close_display: unsafe extern "C" fn(*mut Display) -> c_int,
    default_screen: unsafe extern "C" fn(*mut Display) -> c_int,
    free: unsafe extern "C" fn(*mut c_void) -> c_int,
    sync: unsafe extern "C" fn(*mut Display, c_int) -> c_int,
    set_error_handler: unsafe extern "C" fn(XErrorHandler) -> XErrorHandler,
    choose_fb_config: unsafe extern "C" fn(*mut Display, c_int, *const c_int, *mut c_int) -> *mut GlxFbConfig,
    create_pbuffer: unsafe extern "C" fn(*mut Display, GlxFbConfig, *const c_int) -> GlxDrawable,
    destroy_pbuffer: unsafe extern "C" fn(*mut Display, GlxDrawable),
    make_context_current: unsafe extern "C" fn(*mut Display, GlxDrawable, GlxDrawable, GlxContext) -> c_int,
    destroy_context: unsafe extern "C" fn(*mut Display, GlxContext),
    get_current_context: unsafe extern "C" fn() -> GlxContext,
    get_proc_address: unsafe extern "C" fn(*const c_uchar) -> ProcAddress,
    /// Kept loaded for as long as the functions above may be called.
    _libs: [Library; 2],
}

impl Glx {
    fn load() -> Result<Glx, String> {
        // SAFETY: libX11 and libGL run no initialisation code with preconditions when loaded.
        let (x11, gl) = unsafe { (Library::new("libX11.so.6"), Library::new("libGL.so.1")) };
        let (x11, gl) = (x11.map_err(|e| e.to_string())?, gl.map_err(|e| e.to_string())?);
        macro_rules! sym {
            ($lib:expr, $name:literal) => {
                // SAFETY: the field this initialises spells out the C signature of `$name`, as
                // Xlib.h and glx.h declare it.
                unsafe { symbol(&$lib, $name)? }
            };
        }
        Ok(Glx {
            open_display: sym!(x11, b"XOpenDisplay\0"),
            close_display: sym!(x11, b"XCloseDisplay\0"),
            default_screen: sym!(x11, b"XDefaultScreen\0"),
            free: sym!(x11, b"XFree\0"),
            sync: sym!(x11, b"XSync\0"),
            set_error_handler: sym!(x11, b"XSetErrorHandler\0"),
            choose_fb_config: sym!(gl, b"glXChooseFBConfig\0"),
            create_pbuffer: sym!(gl, b"glXCreatePbuffer\0"),
            destroy_pbuffer: sym!(gl, b"glXDestroyPbuffer\0"),
            make_context_current: sym!(gl, b"glXMakeContextCurrent\0"),
            destroy_context: sym!(gl, b"glXDestroyContext\0"),
            get_current_context: sym!(gl, b"glXGetCurrentContext\0"),
            get_proc_address: sym!(gl, b"glXGetProcAddressARB\0"),
            _libs: [x11, gl],
        })
    }
}

/// The function `name` (NUL-terminated) of `lib`.
///
/// # Safety
/// `T` is the function pointer type of `name`'s C signature.
unsafe fn symbol<T: Copy>(lib: &Library, name: &[u8]) -> Result<T, String> {
    // SAFETY: the caller's contract.
    unsafe { lib.get::<T>(name) }
        .map(|f| *f)
        .map_err(|e| e.to_string())
}

/// Set by [`record_x_error`] while a context is being made.
static X_ERROR: AtomicBool = AtomicBool::new(false);

/// An X error handler that only notes the error: Xlib's own exits the process, and a driver
/// refusing a context version answers with an X error.
unsafe extern "C" fn record_x_error(_: *mut Display, _: *mut XErrorEvent) -> c_int {
    X_ERROR.store(true, Ordering::Relaxed);
    0
}

/// A Core's OpenGL context on the X display, beside the SDL renderer's (if it draws with OpenGL),
/// which is made current again after every switch back to the frontend.
pub struct GlxGl {
    glx: Glx,
    display: *mut Display,
    config: GlxFbConfig,
    /// The pbuffer the Core's context is current on.
    pbuffer: GlxDrawable,
    /// The Core's context, null until made.
    core: GlxContext,
    /// The SDL renderer's window and OpenGL context, `None` on another renderer.
    frontend: Option<(Window, SDL_GLContext)>,
}

impl GlxGl {
    /// Opens the X display `display` (`None`: `$DISPLAY`) for a Core's context, beside the SDL
    /// renderer's `frontend` window and context. `Err` when there is no X display or no GLX on it.
    pub fn open(display: Option<&CStr>, frontend: Option<(Window, SDL_GLContext)>) -> Result<GlxGl, String> {
        let glx = Glx::load()?;
        // SAFETY: the name is NUL-terminated, or null for `$DISPLAY`.
        let dpy = unsafe { (glx.open_display)(display.map_or(ptr::null(), CStr::as_ptr)) };
        if dpy.is_null() {
            return Err("can't open the X display".into());
        }
        let attributes = [
            GLX_DRAWABLE_TYPE,
            GLX_PBUFFER_BIT,
            GLX_RENDER_TYPE,
            GLX_RGBA_BIT,
            GLX_RED_SIZE,
            8,
            GLX_GREEN_SIZE,
            8,
            GLX_BLUE_SIZE,
            8,
            GLX_ALPHA_SIZE,
            8,
            GLX_DOUBLEBUFFER,
            0,
            0,
        ];
        let pbuffer_size = [GLX_PBUFFER_WIDTH, 16, GLX_PBUFFER_HEIGHT, 16, 0];
        // SAFETY: `dpy` is open; the attribute lists are 0-terminated and `count` is a live local.
        // The configs array Xlib returns is freed once its first config is copied out.
        let (config, pbuffer) = unsafe {
            let mut count = 0;
            let configs =
                (glx.choose_fb_config)(dpy, (glx.default_screen)(dpy), attributes.as_ptr(), &mut count);
            let config = if configs.is_null() || count < 1 {
                ptr::null_mut()
            } else {
                *configs
            };
            if !configs.is_null() {
                (glx.free)(configs.cast());
            }
            let pbuffer = if config.is_null() {
                0
            } else {
                (glx.create_pbuffer)(dpy, config, pbuffer_size.as_ptr())
            };
            (config, pbuffer)
        };
        if pbuffer == 0 {
            // SAFETY: `dpy` is open and nothing made on it is left.
            unsafe { (glx.close_display)(dpy) };
            return Err("the X display has no GLX pbuffer".into());
        }
        Ok(GlxGl {
            glx,
            display: dpy,
            config,
            pbuffer,
            core: ptr::null_mut(),
            frontend,
        })
    }

    /// Releases the SDL renderer's context: one thread can't have an EGL and a GLX context
    /// current at once, and SDL must not believe its context is still current.
    fn release_frontend(&self) {
        if let Some((window, _)) = &self.frontend {
            // SAFETY: the window is alive (held by `self`); a null context releases the current one.
            unsafe { SDL_GL_MakeCurrent(window.raw(), ptr::null_mut()) };
        }
    }
}

/// The `glXCreateContextAttribsARB` attributes for `request`, 0-terminated. A compatibility
/// context asks for 2.1 and gets the newest version the driver has.
fn context_attributes(request: GlRequest) -> [c_int; 7] {
    let (profile, major, minor) = if request.core_profile {
        (
            GLX_CONTEXT_CORE_PROFILE_BIT_ARB,
            request.major as c_int,
            request.minor as c_int,
        )
    } else {
        (GLX_CONTEXT_COMPATIBILITY_PROFILE_BIT_ARB, 2, 1)
    };
    [
        GLX_CONTEXT_MAJOR_VERSION_ARB,
        major,
        GLX_CONTEXT_MINOR_VERSION_ARB,
        minor,
        GLX_CONTEXT_PROFILE_MASK_ARB,
        profile,
        0,
    ]
}

impl GlHost for GlxGl {
    fn create_context(&mut self, request: GlRequest) -> Result<(), String> {
        if !self.core.is_null() {
            return Err("the Core already has a context".into());
        }
        let create = self.proc_address(c"glXCreateContextAttribsARB");
        let Some(create) = create else {
            return Err("no glXCreateContextAttribsARB".into());
        };
        // SAFETY: `glXGetProcAddressARB` returned GLX's `glXCreateContextAttribsARB`, whose
        // signature is `CreateContextAttribs`.
        let create = unsafe { std::mem::transmute::<unsafe extern "C" fn(), CreateContextAttribs>(create) };
        let attributes = context_attributes(request);
        // SAFETY: the display and config are live; the attribute list is 0-terminated. X errors
        // are caught (Xlib's default handler would exit) until the request has been answered,
        // then the previous handler is put back.
        let core = unsafe {
            X_ERROR.store(false, Ordering::Relaxed);
            let previous = (self.glx.set_error_handler)(Some(record_x_error));
            let core = create(self.display, self.config, ptr::null_mut(), 1, attributes.as_ptr());
            (self.glx.sync)(self.display, 0);
            (self.glx.set_error_handler)(previous);
            core
        };
        if core.is_null() || X_ERROR.load(Ordering::Relaxed) {
            if !core.is_null() {
                // SAFETY: the context was just made and was never current.
                unsafe { (self.glx.destroy_context)(self.display, core) };
            }
            return Err(format!("no {request} context on GLX"));
        }
        self.core = core;
        Ok(())
    }

    fn make_core_current(&mut self) {
        if self.core.is_null() {
            return;
        }
        self.release_frontend();
        // SAFETY: the display, pbuffer and context are live, and made with the same config.
        if unsafe { (self.glx.make_context_current)(self.display, self.pbuffer, self.pbuffer, self.core) }
            == 0
        {
            eprintln!("game37: can't make the Core's GLX context current");
        }
    }

    fn make_frontend_current(&mut self) {
        // Releasing GLX while another API's context is current is an X error (BadAccess, from
        // libglvnd), so only the Core's own context is released.
        // SAFETY: the display is open; no drawable and no context releases the current one.
        unsafe {
            if !self.core.is_null() && (self.glx.get_current_context)() == self.core {
                (self.glx.make_context_current)(self.display, 0, 0, ptr::null_mut());
            }
        }
        if let Some((window, context)) = &self.frontend {
            // SAFETY: the window is alive (held by `self`) and `context` is a live context on it.
            if !unsafe { SDL_GL_MakeCurrent(window.raw(), *context) } {
                eprintln!("game37: can't switch OpenGL contexts: {}", sdl3::get_error());
            }
        }
    }

    fn proc_address(&self, name: &CStr) -> ProcAddress {
        // SAFETY: `name` is NUL-terminated.
        unsafe { (self.glx.get_proc_address)(name.as_ptr().cast()) }
    }

    fn destroy_context(&mut self) {
        self.make_frontend_current();
        if !self.core.is_null() {
            // SAFETY: the Core's context was made by `create_context` and isn't current.
            unsafe { (self.glx.destroy_context)(self.display, self.core) };
            self.core = ptr::null_mut();
        }
    }
}

impl Drop for GlxGl {
    fn drop(&mut self) {
        self.destroy_context();
        // SAFETY: the pbuffer was made on the display, and nothing is current on it any more.
        unsafe {
            (self.glx.destroy_pbuffer)(self.display, self.pbuffer);
            (self.glx.close_display)(self.display);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_core_profile_context_gets_the_version_asked_for() {
        let attributes = context_attributes(GlRequest {
            core_profile: true,
            major: 3,
            minor: 1,
        });
        assert_eq!(
            attributes,
            [
                GLX_CONTEXT_MAJOR_VERSION_ARB,
                3,
                GLX_CONTEXT_MINOR_VERSION_ARB,
                1,
                GLX_CONTEXT_PROFILE_MASK_ARB,
                GLX_CONTEXT_CORE_PROFILE_BIT_ARB,
                0
            ]
        );
    }

    #[test]
    fn a_compatibility_context_asks_for_2_1() {
        let attributes = context_attributes(GlRequest {
            core_profile: false,
            major: 0,
            minor: 0,
        });
        assert_eq!(
            &attributes[..4],
            [GLX_CONTEXT_MAJOR_VERSION_ARB, 2, GLX_CONTEXT_MINOR_VERSION_ARB, 1]
        );
        assert_eq!(attributes[5], GLX_CONTEXT_COMPATIBILITY_PROFILE_BIT_ARB);
    }

    #[test]
    fn no_x_display_is_an_error() {
        assert!(GlxGl::open(Some(c"/nonexistent/game37-test:0"), None).is_err());
    }
}
