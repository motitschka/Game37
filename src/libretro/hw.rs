//! libretro OpenGL hardware rendering (ADR 0003): a Core that asks for it with `SET_HW_RENDER`
//! draws into a framebuffer object of the host's, in an OpenGL context of its own. Each frame it
//! draws is read back into a [`Frame`] (`Rgba32`), so the frontend shows it like any other: the
//! Core's context shares nothing with the frontend's.
//!
//! Windows and contexts are the frontend's, behind [`GlHost`]; this module makes the few OpenGL
//! calls the libretro protocol needs (the FBO, reading a frame back) through function pointers
//! the host looks up.

use std::ffi::{CStr, c_void};
use std::fmt;

use super::callbacks;
use super::ffi::{HwContextResetFn, HwGetProcAddressFn, ProcAddress};
use super::{AvInfo, Frame, PixelFormat};

/// The OpenGL context a Core asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlRequest {
    /// A core profile context (`RETRO_HW_CONTEXT_OPENGL_CORE`); else a compatibility one.
    pub core_profile: bool,
    /// The version of a core profile context; 0.0 for a compatibility one, which gets the
    /// newest version the driver has.
    pub major: u32,
    pub minor: u32,
}

impl fmt::Display for GlRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.core_profile {
            write!(f, "OpenGL {}.{} core", self.major, self.minor)
        } else {
            write!(f, "OpenGL compatibility")
        }
    }
}

/// The frontend's side of OpenGL rendering: the Core's context and the frontend's own. Without
/// one, Cores are refused hardware rendering.
pub trait GlHost {
    /// Creates the Core's context as `request` asks; it needn't share objects with the frontend's.
    fn create_context(&mut self, request: GlRequest) -> Result<(), String>;
    /// Makes the Core's context current.
    fn make_core_current(&mut self);
    /// Makes the frontend's own context current again.
    fn make_frontend_current(&mut self);
    /// The platform's OpenGL loader (`SDL_GL_GetProcAddress`, `glXGetProcAddressARB`): callable
    /// from any thread, for as long as the host lives.
    fn loader(&self) -> HwGetProcAddressFn;
    /// The address of the OpenGL function `name`, `None` if there is none.
    fn proc_address(&self, name: &CStr) -> ProcAddress {
        // SAFETY: `name` is NUL-terminated, all the loader asks.
        unsafe { self.loader()(name.as_ptr()) }
    }
    /// Destroys the Core's context, leaving the frontend's current.
    fn destroy_context(&mut self);
}

/// What a Core asked for in `SET_HW_RENDER`.
#[derive(Clone, Copy, Debug)]
pub(super) struct HwRequest {
    pub gl: GlRequest,
    pub depth: bool,
    pub stencil: bool,
    pub bottom_left_origin: bool,
    pub context_reset: Option<HwContextResetFn>,
    pub context_destroy: Option<HwContextResetFn>,
}

type GLenum = u32;
type GLuint = u32;
type GLint = i32;
type GLsizei = i32;

const GL_TEXTURE_2D: GLenum = 0x0DE1;
const GL_RGBA: GLenum = 0x1908;
const GL_RGBA8: GLint = 0x8058;
const GL_UNSIGNED_BYTE: GLenum = 0x1401;
const GL_TEXTURE_MAG_FILTER: GLenum = 0x2800;
const GL_TEXTURE_MIN_FILTER: GLenum = 0x2801;
const GL_TEXTURE_WRAP_S: GLenum = 0x2802;
const GL_TEXTURE_WRAP_T: GLenum = 0x2803;
const GL_LINEAR: GLint = 0x2601;
const GL_CLAMP_TO_EDGE: GLint = 0x812F;
const GL_FRAMEBUFFER: GLenum = 0x8D40;
const GL_READ_FRAMEBUFFER: GLenum = 0x8CA8;
const GL_READ_FRAMEBUFFER_BINDING: GLenum = 0x8CAA;
const GL_FRAMEBUFFER_COMPLETE: GLenum = 0x8CD5;
const GL_COLOR_ATTACHMENT0: GLenum = 0x8CE0;
const GL_DEPTH_ATTACHMENT: GLenum = 0x8D00;
const GL_DEPTH_STENCIL_ATTACHMENT: GLenum = 0x821A;
const GL_RENDERBUFFER: GLenum = 0x8D41;
const GL_DEPTH_COMPONENT24: GLenum = 0x81A6;
const GL_DEPTH24_STENCIL8: GLenum = 0x88F0;
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;
const GL_PIXEL_PACK_BUFFER: GLenum = 0x88EB;
const GL_PIXEL_PACK_BUFFER_BINDING: GLenum = 0x88ED;
const GL_PACK_ROW_LENGTH: GLenum = 0x0D02;
const GL_PACK_SKIP_ROWS: GLenum = 0x0D03;
const GL_PACK_SKIP_PIXELS: GLenum = 0x0D04;
const GL_PACK_ALIGNMENT: GLenum = 0x0D05;

macro_rules! gl_functions {
    ($($field:ident = $name:literal: fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        /// The OpenGL functions the host itself calls.
        struct Gl {
            $($field: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
        }

        impl Gl {
            /// Looks up every function, or names the first one missing.
            ///
            /// # Safety
            /// `lookup` returns OpenGL's own function for each name, or `None`.
            unsafe fn load(lookup: impl Fn(&CStr) -> ProcAddress) -> Result<Gl, String> {
                Ok(Gl {
                    $($field: {
                        let f = lookup($name).ok_or_else(|| format!("no {}", $name.to_string_lossy()))?;
                        // SAFETY: `lookup` returned OpenGL's `$name`, whose C signature this
                        // field's type spells out (the caller's contract).
                        unsafe {
                            std::mem::transmute::<unsafe extern "C" fn(), unsafe extern "C" fn($($arg),*) $(-> $ret)?>(f)
                        }
                    },)*
                })
            }
        }
    };
}

gl_functions! {
    gen_textures = c"glGenTextures": fn(GLsizei, *mut GLuint);
    delete_textures = c"glDeleteTextures": fn(GLsizei, *const GLuint);
    bind_texture = c"glBindTexture": fn(GLenum, GLuint);
    tex_image_2d = c"glTexImage2D":
        fn(GLenum, GLint, GLint, GLsizei, GLsizei, GLint, GLenum, GLenum, *const c_void);
    tex_parameter_i = c"glTexParameteri": fn(GLenum, GLenum, GLint);
    gen_framebuffers = c"glGenFramebuffers": fn(GLsizei, *mut GLuint);
    delete_framebuffers = c"glDeleteFramebuffers": fn(GLsizei, *const GLuint);
    bind_framebuffer = c"glBindFramebuffer": fn(GLenum, GLuint);
    framebuffer_texture_2d = c"glFramebufferTexture2D": fn(GLenum, GLenum, GLenum, GLuint, GLint);
    check_framebuffer_status = c"glCheckFramebufferStatus": fn(GLenum) -> GLenum;
    gen_renderbuffers = c"glGenRenderbuffers": fn(GLsizei, *mut GLuint);
    delete_renderbuffers = c"glDeleteRenderbuffers": fn(GLsizei, *const GLuint);
    bind_renderbuffer = c"glBindRenderbuffer": fn(GLenum, GLuint);
    renderbuffer_storage = c"glRenderbufferStorage": fn(GLenum, GLenum, GLsizei, GLsizei);
    framebuffer_renderbuffer = c"glFramebufferRenderbuffer": fn(GLenum, GLenum, GLenum, GLuint);
    clear_color = c"glClearColor": fn(f32, f32, f32, f32);
    clear = c"glClear": fn(u32);
    get_integer_v = c"glGetIntegerv": fn(GLenum, *mut GLint);
    bind_buffer = c"glBindBuffer": fn(GLenum, GLuint);
    pixel_store_i = c"glPixelStorei": fn(GLenum, GLint);
    read_pixels = c"glReadPixels": fn(GLint, GLint, GLsizei, GLsizei, GLenum, GLenum, *mut c_void);
}

/// Runs `f` on the installed host; `None` without one.
fn host<R>(f: impl FnOnce(&mut (dyn GlHost + 'static)) -> R) -> Option<R> {
    callbacks::with(|s| s.gl_host.as_mut().map(|h| f(h.as_mut()))).flatten()
}

/// A Core's running OpenGL rendering: its context (the host's), and the FBO it draws into.
pub(super) struct Hw {
    gl: Gl,
    request: HwRequest,
    fbo: Fbo,
    /// The size the FBO was last made, or tried, at.
    asked: (u32, u32),
}

/// The size of FBO a Core with geometry `av` draws into: its maximum size, else its base size.
pub(super) fn fbo_size(av: &AvInfo) -> (u32, u32) {
    let (w, h) = if av.max_width > 0 && av.max_height > 0 {
        (av.max_width, av.max_height)
    } else {
        (av.base_width, av.base_height)
    };
    (w.max(1), h.max(1))
}

/// The size an FBO made at `made` grows to once the Core's geometry is `av`, as RetroArch remakes
/// it when `SET_SYSTEM_AV_INFO` raises the maximum size; `None` while the picture fits. A
/// `SET_GEOMETRY` larger than the maximum size is clamped instead, unless the Core gave none.
fn grown(made: (u32, u32), av: &AvInfo) -> Option<(u32, u32)> {
    let (w, h) = fbo_size(av);
    (w > made.0 || h > made.1).then(|| (w.max(made.0), h.max(made.1)))
}

impl Hw {
    /// Creates the Core's context and a `width × height` FBO, and hands both to the Core
    /// (`context_reset`). Leaves the frontend's context current. Should the FBO fail once the
    /// context is made, the Core shuts down (`shut_down`) with its context current first.
    pub fn start(
        request: HwRequest,
        width: u32,
        height: u32,
        shut_down: impl FnOnce(),
    ) -> Result<Hw, String> {
        let loader =
            host(|h| h.create_context(request.gl).map(|()| h.loader())).ok_or("no OpenGL host")??;
        callbacks::set_loader(Some(loader));
        host(|h| h.make_core_current());
        let size = (width.max(1), height.max(1));
        // SAFETY: the platform's loader returns OpenGL's own function for each name, or `None`,
        // and asks only for a NUL-terminated name.
        let made = unsafe { Gl::load(|name| loader(name.as_ptr())) }.and_then(|gl| {
            let fbo = Fbo::new(&gl, &request, size)?;
            Ok(Hw {
                gl,
                request,
                fbo,
                asked: size,
            })
        });
        let hw = match made {
            Ok(hw) => hw,
            Err(e) => {
                shut_down();
                callbacks::set_loader(None);
                host(|h| h.destroy_context());
                return Err(e);
            }
        };
        callbacks::set_fbo(hw.fbo.fbo);
        eprintln!(
            "game37: the Core draws with {} into a {}×{} framebuffer",
            request.gl, size.0, size.1
        );
        if let Some(reset) = request.context_reset {
            // SAFETY: the Core's context is current with its FBO bound, as libretro.h promises
            // `context_reset`; the callbacks' state is not borrowed, so the Core may call back.
            unsafe { reset() };
        }
        host(|h| h.make_frontend_current());
        Ok(hw)
    }

    /// Runs `f` (a `retro_run`) with the Core's context current, then reads the frame it drew into
    /// the FBO, if it drew one, into the callbacks' frame. First the FBO grows if the Core's
    /// maximum size did.
    pub fn run(&mut self, f: impl FnOnce()) {
        host(|h| h.make_core_current());
        if let Some(size) = callbacks::with(|s| grown(self.asked, &s.av_info)).flatten() {
            self.resize(size);
        }
        f();
        callbacks::with(|s| {
            if let Some((width, height)) = s.hw_size.take() {
                self.read(&mut s.frame, width, height);
            }
        });
        host(|h| h.make_frontend_current());
    }

    /// Remakes the FBO at `size` with the Core's context current, between `context_destroy` and
    /// `context_reset` as RetroArch does. Should it fail, the Core keeps drawing into the old one.
    fn resize(&mut self, size: (u32, u32)) {
        self.asked = size;
        if let Some(destroy) = self.request.context_destroy {
            // SAFETY: the Core's context is current, as libretro.h promises `context_destroy`; the
            // callbacks' state is not borrowed.
            unsafe { destroy() };
        }
        match Fbo::new(&self.gl, &self.request, size) {
            Ok(fbo) => {
                std::mem::replace(&mut self.fbo, fbo).delete(&self.gl);
                eprintln!("game37: the Core's framebuffer grew to {}×{}", size.0, size.1);
            }
            Err(e) => eprintln!(
                "game37: can't grow the Core's framebuffer to {}×{}: {e}",
                size.0, size.1
            ),
        }
        // SAFETY: the Core's context is current and the FBO is its own, live.
        unsafe { (self.gl.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo.fbo) };
        callbacks::set_fbo(self.fbo.fbo);
        if let Some(reset) = self.request.context_reset {
            // SAFETY: as in `start`: the Core's context is current with its FBO bound.
            unsafe { reset() };
        }
    }

    /// Runs `f` (the Core's shutdown: unload and deinit) with the Core's context current, after
    /// `context_destroy`, then frees the FBO and the context.
    pub fn stop(mut self, f: impl FnOnce()) {
        host(|h| h.make_core_current());
        if let Some(destroy) = self.request.context_destroy {
            // SAFETY: the Core's context is current, as libretro.h promises `context_destroy`.
            unsafe { destroy() };
        }
        f();
        self.fbo.delete(&self.gl);
        callbacks::set_fbo(0);
        callbacks::set_loader(None);
        host(|h| h.destroy_context());
    }

    /// Reads the `width × height` picture (at most the FBO's size) into `frame`, top row first,
    /// with the Core's context current.
    fn read(&self, frame: &mut Frame, width: u32, height: u32) {
        let (w, h) = (width.min(self.fbo.width), height.min(self.fbo.height));
        let pitch = w as usize * 4;
        frame.data.resize(pitch * h as usize, 0);
        let gl = &self.gl;
        // SAFETY: the Core's context is current. `frame.data` holds `w × h` RGBA pixels, which is
        // what `glReadPixels` writes with no pack buffer bound and the pack state set here: rows
        // packed (no row length), 4-byte aligned, none skipped. The Core's bindings and pack state
        // are put back as they were.
        unsafe {
            let (mut read_fbo, mut pack_buffer) = (0, 0);
            (gl.get_integer_v)(GL_READ_FRAMEBUFFER_BINDING, &mut read_fbo);
            (gl.get_integer_v)(GL_PIXEL_PACK_BUFFER_BINDING, &mut pack_buffer);
            // Each pack parameter with the value it reads with; swapped for the Core's below.
            let mut pack = [
                (GL_PACK_ROW_LENGTH, 0),
                (GL_PACK_SKIP_ROWS, 0),
                (GL_PACK_SKIP_PIXELS, 0),
                (GL_PACK_ALIGNMENT, 4),
            ];
            for (name, value) in &mut pack {
                let ours = *value;
                (gl.get_integer_v)(*name, value);
                (gl.pixel_store_i)(*name, ours);
            }
            (gl.bind_framebuffer)(GL_READ_FRAMEBUFFER, self.fbo.fbo);
            (gl.bind_buffer)(GL_PIXEL_PACK_BUFFER, 0);
            (gl.read_pixels)(
                0,
                0,
                w as GLsizei,
                h as GLsizei,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                frame.data.as_mut_ptr().cast(),
            );
            for (name, core) in pack {
                (gl.pixel_store_i)(name, core);
            }
            (gl.bind_buffer)(GL_PIXEL_PACK_BUFFER, pack_buffer as GLuint);
            (gl.bind_framebuffer)(GL_READ_FRAMEBUFFER, read_fbo as GLuint);
        }
        if self.request.bottom_left_origin {
            flip_rows(&mut frame.data, pitch);
        }
        frame.width = w;
        frame.height = h;
        frame.pitch = pitch;
        frame.format = PixelFormat::Rgba32;
    }
}

/// The FBO a Core draws into, with its attachments.
struct Fbo {
    fbo: GLuint,
    texture: GLuint,
    /// The depth (and stencil) renderbuffer, 0 when the Core asked for neither.
    depth: GLuint,
    width: u32,
    height: u32,
}

impl Fbo {
    /// Makes a `width × height` FBO as `request` asks, with the Core's context current, leaving it
    /// bound.
    fn new(gl: &Gl, request: &HwRequest, (width, height): (u32, u32)) -> Result<Fbo, String> {
        let (w, h) = (width as GLsizei, height as GLsizei);
        let mut fbo = Fbo {
            fbo: 0,
            texture: 0,
            depth: 0,
            width,
            height,
        };
        // SAFETY: the Core's context is current and these are its OpenGL functions; every pointer
        // passed points at a live local or field, or is null where OpenGL allows it.
        unsafe {
            (gl.gen_textures)(1, &mut fbo.texture);
            (gl.bind_texture)(GL_TEXTURE_2D, fbo.texture);
            (gl.tex_image_2d)(
                GL_TEXTURE_2D,
                0,
                GL_RGBA8,
                w,
                h,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                std::ptr::null(),
            );
            for (param, value) in [
                (GL_TEXTURE_MIN_FILTER, GL_LINEAR),
                (GL_TEXTURE_MAG_FILTER, GL_LINEAR),
                (GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE),
                (GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE),
            ] {
                (gl.tex_parameter_i)(GL_TEXTURE_2D, param, value);
            }
            (gl.bind_texture)(GL_TEXTURE_2D, 0);
            (gl.gen_framebuffers)(1, &mut fbo.fbo);
            (gl.bind_framebuffer)(GL_FRAMEBUFFER, fbo.fbo);
            (gl.framebuffer_texture_2d)(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                fbo.texture,
                0,
            );
            if request.depth || request.stencil {
                let (format, attachment) = if request.stencil {
                    (GL_DEPTH24_STENCIL8, GL_DEPTH_STENCIL_ATTACHMENT)
                } else {
                    (GL_DEPTH_COMPONENT24, GL_DEPTH_ATTACHMENT)
                };
                (gl.gen_renderbuffers)(1, &mut fbo.depth);
                (gl.bind_renderbuffer)(GL_RENDERBUFFER, fbo.depth);
                (gl.renderbuffer_storage)(GL_RENDERBUFFER, format, w, h);
                (gl.framebuffer_renderbuffer)(GL_FRAMEBUFFER, attachment, GL_RENDERBUFFER, fbo.depth);
                (gl.bind_renderbuffer)(GL_RENDERBUFFER, 0);
            }
            let status = (gl.check_framebuffer_status)(GL_FRAMEBUFFER);
            if status != GL_FRAMEBUFFER_COMPLETE {
                fbo.delete(gl);
                return Err(format!("the framebuffer is incomplete ({status:#x})"));
            }
            (gl.clear_color)(0.0, 0.0, 0.0, 1.0);
            (gl.clear)(GL_COLOR_BUFFER_BIT);
        }
        Ok(fbo)
    }

    /// Deletes the FBO and its attachments, with the Core's context current.
    fn delete(&mut self, gl: &Gl) {
        // SAFETY: the Core's context is current; deleting the name 0 is ignored.
        unsafe {
            (gl.bind_framebuffer)(GL_FRAMEBUFFER, 0);
            (gl.delete_framebuffers)(1, &self.fbo);
            (gl.delete_renderbuffers)(1, &self.depth);
            (gl.delete_textures)(1, &self.texture);
        }
        (self.fbo, self.depth, self.texture) = (0, 0, 0);
    }
}

/// Reverses the order of `data`'s rows of `row` bytes each: OpenGL reads a picture bottom row
/// first, and a [`Frame`] is top row first.
fn flip_rows(data: &mut [u8], row: usize) {
    if row == 0 {
        return;
    }
    let rows = data.len() / row;
    for top in 0..rows / 2 {
        let (upper, lower) = data.split_at_mut((rows - 1 - top) * row);
        upper[top * row..(top + 1) * row].swap_with_slice(&mut lower[..row]);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    #[test]
    fn rows_are_flipped_top_to_bottom() {
        let mut odd = vec![1, 1, 2, 2, 3, 3];
        flip_rows(&mut odd, 2);
        assert_eq!(odd, [3, 3, 2, 2, 1, 1]);
        let mut even = vec![1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4];
        flip_rows(&mut even, 3);
        assert_eq!(even, [4, 4, 4, 3, 3, 3, 2, 2, 2, 1, 1, 1]);
        let mut one = vec![5, 6, 7, 8];
        flip_rows(&mut one, 4);
        assert_eq!(one, [5, 6, 7, 8]);
        let mut empty: Vec<u8> = Vec::new();
        flip_rows(&mut empty, 0);
        assert!(empty.is_empty());
    }

    fn av(base: (u32, u32), max: (u32, u32)) -> AvInfo {
        AvInfo {
            base_width: base.0,
            base_height: base.1,
            max_width: max.0,
            max_height: max.1,
            ..AvInfo::default()
        }
    }

    #[test]
    fn the_framebuffer_grows_with_the_cores_maximum_size_only() {
        let made = (480, 272);
        assert_eq!(grown(made, &av((480, 272), (480, 272))), None);
        assert_eq!(
            grown(made, &av((480, 272), (960, 544))),
            Some((960, 544)),
            "4× the pixels"
        );
        assert_eq!(grown(made, &av((240, 136), (240, 136))), None, "it never shrinks");
        assert_eq!(
            grown(made, &av((480, 272), (512, 240))),
            Some((512, 272)),
            "one side grows"
        );
        assert_eq!(
            grown(made, &av((960, 544), (480, 272))),
            None,
            "a base size past the maximum is clamped"
        );
        assert_eq!(
            grown(made, &av((640, 480), (0, 0))),
            Some((640, 480)),
            "without a maximum size, the base size"
        );
        assert_eq!(fbo_size(&av((0, 0), (0, 0))), (1, 1));
    }

    /// A host whose context is made but has no OpenGL functions; it notes each call.
    struct NoFunctions(Rc<RefCell<Vec<&'static str>>>);

    unsafe extern "C" fn no_functions(_: *const std::ffi::c_char) -> ProcAddress {
        None
    }

    impl GlHost for NoFunctions {
        fn create_context(&mut self, _: GlRequest) -> Result<(), String> {
            self.0.borrow_mut().push("create");
            Ok(())
        }
        fn make_core_current(&mut self) {
            self.0.borrow_mut().push("core current");
        }
        fn make_frontend_current(&mut self) {
            self.0.borrow_mut().push("frontend current");
        }
        fn loader(&self) -> HwGetProcAddressFn {
            no_functions
        }
        fn destroy_context(&mut self) {
            self.0.borrow_mut().push("destroy");
        }
    }

    #[test]
    fn a_core_whose_framebuffer_fails_shuts_down_with_its_context_current() {
        let _serial = crate::libretro::tests::serial();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut state = callbacks::State::new(c"/sys".into(), c"/save".into());
        state.gl_host = Some(Box::new(NoFunctions(calls.clone())));
        callbacks::install(state);
        let request = HwRequest {
            gl: GlRequest {
                core_profile: true,
                major: 3,
                minor: 3,
            },
            depth: false,
            stencil: false,
            bottom_left_origin: true,
            context_reset: None,
            context_destroy: None,
        };
        let started = Hw::start(request, 480, 272, || calls.borrow_mut().push("shut down"));
        callbacks::remove();
        assert_eq!(started.err().as_deref(), Some("no glGenTextures"));
        assert_eq!(
            *calls.borrow(),
            ["create", "core current", "shut down", "destroy"]
        );
    }

    /// A host whose context is made, with one OpenGL function (`glGenTextures`) resolvable; the
    /// rest, including the next one `Gl::load` asks for, missing.
    struct OneFunction;

    unsafe extern "C" fn stub_fn() {}

    unsafe extern "C" fn one_function(sym: *const std::ffi::c_char) -> ProcAddress {
        // SAFETY: the loader's contract (`HwGetProcAddressFn`) promises a NUL-terminated name.
        let name = unsafe { CStr::from_ptr(sym) };
        if name.to_bytes() == b"glGenTextures" {
            Some(stub_fn)
        } else {
            None
        }
    }

    impl GlHost for OneFunction {
        fn create_context(&mut self, _: GlRequest) -> Result<(), String> {
            Ok(())
        }
        fn make_core_current(&mut self) {}
        fn make_frontend_current(&mut self) {}
        fn loader(&self) -> HwGetProcAddressFn {
            one_function
        }
        fn destroy_context(&mut self) {}
    }

    #[test]
    fn the_loader_is_cleared_after_a_failed_start() {
        let _serial = crate::libretro::tests::serial();
        // Other tests in this process leave the loader set; start from a known state.
        callbacks::set_loader(None);
        let mut state = callbacks::State::new(c"/sys".into(), c"/save".into());
        state.gl_host = Some(Box::new(OneFunction));
        callbacks::install(state);
        let request = HwRequest {
            gl: GlRequest {
                core_profile: true,
                major: 3,
                minor: 3,
            },
            depth: false,
            stencil: false,
            bottom_left_origin: true,
            context_reset: None,
            context_destroy: None,
        };
        // SAFETY: a NUL-terminated name.
        assert!(
            unsafe { callbacks::get_proc_address(c"glGenTextures".as_ptr()) }.is_none(),
            "no loader installed before start"
        );
        let resolved_while_running = Rc::new(RefCell::new(false));
        let resolved = resolved_while_running.clone();
        let started = Hw::start(request, 480, 272, move || {
            // SAFETY: as above; this runs before `set_loader(None)`, so the loader is still live.
            let addr = unsafe { callbacks::get_proc_address(c"glGenTextures".as_ptr()) };
            *resolved.borrow_mut() = addr.is_some();
        });
        callbacks::remove();
        assert!(started.is_err());
        assert!(
            *resolved_while_running.borrow(),
            "glGenTextures resolved while the Core was shutting down"
        );
        // SAFETY: as above.
        assert!(
            unsafe { callbacks::get_proc_address(c"glGenTextures".as_ptr()) }.is_none(),
            "the loader is cleared after the failed start"
        );
    }

    static FBO_BUILD_COUNT: AtomicU32 = AtomicU32::new(0);
    /// The 1-based FBO build (counted by calls to `glCheckFramebufferStatus`) that comes back
    /// incomplete; 0 means every build succeeds.
    static FBO_FAIL_AT: AtomicU32 = AtomicU32::new(0);
    static CONTEXT_RESET_CALLS: AtomicU32 = AtomicU32::new(0);
    static CONTEXT_DESTROY_CALLS: AtomicU32 = AtomicU32::new(0);

    unsafe extern "C" fn stub_gen(n: GLsizei, out: *mut GLuint) {
        if n > 0 && !out.is_null() {
            // SAFETY: `Fbo::new` passes a pointer to one live `GLuint`.
            unsafe { *out = 1 };
        }
    }
    unsafe extern "C" fn stub_delete(_: GLsizei, _: *const GLuint) {}
    unsafe extern "C" fn stub_bind(_: GLenum, _: GLuint) {}
    unsafe extern "C" fn stub_tex_image_2d(
        _: GLenum,
        _: GLint,
        _: GLint,
        _: GLsizei,
        _: GLsizei,
        _: GLint,
        _: GLenum,
        _: GLenum,
        _: *const c_void,
    ) {
    }
    unsafe extern "C" fn stub_tex_parameter_i(_: GLenum, _: GLenum, _: GLint) {}
    unsafe extern "C" fn stub_framebuffer_texture_2d(_: GLenum, _: GLenum, _: GLenum, _: GLuint, _: GLint) {}
    unsafe extern "C" fn stub_check_framebuffer_status(_: GLenum) -> GLenum {
        let build = FBO_BUILD_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
        if build == FBO_FAIL_AT.load(Ordering::SeqCst) {
            0
        } else {
            GL_FRAMEBUFFER_COMPLETE
        }
    }
    unsafe extern "C" fn stub_renderbuffer_storage(_: GLenum, _: GLenum, _: GLsizei, _: GLsizei) {}
    unsafe extern "C" fn stub_framebuffer_renderbuffer(_: GLenum, _: GLenum, _: GLenum, _: GLuint) {}
    unsafe extern "C" fn stub_clear_color(_: f32, _: f32, _: f32, _: f32) {}
    unsafe extern "C" fn stub_clear(_: u32) {}
    unsafe extern "C" fn stub_get_integer_v(_: GLenum, _: *mut GLint) {}
    unsafe extern "C" fn stub_pixel_store_i(_: GLenum, _: GLint) {}
    unsafe extern "C" fn stub_read_pixels(
        _: GLint,
        _: GLint,
        _: GLsizei,
        _: GLsizei,
        _: GLenum,
        _: GLenum,
        _: *mut c_void,
    ) {
    }

    unsafe extern "C" fn count_context_reset() {
        CONTEXT_RESET_CALLS.fetch_add(1, Ordering::SeqCst);
    }
    unsafe extern "C" fn count_context_destroy() {
        CONTEXT_DESTROY_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    /// Every OpenGL function `Fbo::new` calls, resolved to a stub: real enough to build and tear
    /// an FBO down without a GPU, standing in for the platform's loader.
    unsafe extern "C" fn fake_gl_loader(sym: *const std::ffi::c_char) -> ProcAddress {
        // SAFETY: `sym` is NUL-terminated, the loader's contract.
        let name = unsafe { CStr::from_ptr(sym) };
        // SAFETY: each stub's signature below matches the OpenGL function of its name; `Gl::load`
        // transmutes it back to that signature before ever calling it.
        unsafe {
            match name.to_bytes() {
                b"glGenTextures" | b"glGenFramebuffers" | b"glGenRenderbuffers" => {
                    Some(std::mem::transmute::<
                        unsafe extern "C" fn(GLsizei, *mut GLuint),
                        unsafe extern "C" fn(),
                    >(stub_gen))
                }
                b"glDeleteTextures" | b"glDeleteFramebuffers" | b"glDeleteRenderbuffers" => {
                    Some(std::mem::transmute::<
                        unsafe extern "C" fn(GLsizei, *const GLuint),
                        unsafe extern "C" fn(),
                    >(stub_delete))
                }
                b"glBindTexture" | b"glBindFramebuffer" | b"glBindRenderbuffer" | b"glBindBuffer" => {
                    Some(std::mem::transmute::<
                        unsafe extern "C" fn(GLenum, GLuint),
                        unsafe extern "C" fn(),
                    >(stub_bind))
                }
                b"glTexImage2D" => Some(std::mem::transmute::<
                    unsafe extern "C" fn(
                        GLenum,
                        GLint,
                        GLint,
                        GLsizei,
                        GLsizei,
                        GLint,
                        GLenum,
                        GLenum,
                        *const c_void,
                    ) -> (),
                    unsafe extern "C" fn(),
                >(stub_tex_image_2d)),
                b"glTexParameteri" => Some(std::mem::transmute::<
                    unsafe extern "C" fn(GLenum, GLenum, GLint),
                    unsafe extern "C" fn(),
                >(stub_tex_parameter_i)),
                b"glFramebufferTexture2D" => Some(std::mem::transmute::<
                    unsafe extern "C" fn(GLenum, GLenum, GLenum, GLuint, GLint),
                    unsafe extern "C" fn(),
                >(stub_framebuffer_texture_2d)),
                b"glCheckFramebufferStatus" => Some(std::mem::transmute::<
                    unsafe extern "C" fn(GLenum) -> GLenum,
                    unsafe extern "C" fn(),
                >(stub_check_framebuffer_status)),
                b"glRenderbufferStorage" => Some(std::mem::transmute::<
                    unsafe extern "C" fn(GLenum, GLenum, GLsizei, GLsizei),
                    unsafe extern "C" fn(),
                >(stub_renderbuffer_storage)),
                b"glFramebufferRenderbuffer" => Some(std::mem::transmute::<
                    unsafe extern "C" fn(GLenum, GLenum, GLenum, GLuint),
                    unsafe extern "C" fn(),
                >(stub_framebuffer_renderbuffer)),
                b"glClearColor" => Some(std::mem::transmute::<
                    unsafe extern "C" fn(f32, f32, f32, f32),
                    unsafe extern "C" fn(),
                >(stub_clear_color)),
                b"glClear" => Some(std::mem::transmute::<
                    unsafe extern "C" fn(u32),
                    unsafe extern "C" fn(),
                >(stub_clear)),
                b"glGetIntegerv" => Some(std::mem::transmute::<
                    unsafe extern "C" fn(GLenum, *mut GLint),
                    unsafe extern "C" fn(),
                >(stub_get_integer_v)),
                b"glPixelStorei" => Some(std::mem::transmute::<
                    unsafe extern "C" fn(GLenum, GLint),
                    unsafe extern "C" fn(),
                >(stub_pixel_store_i)),
                b"glReadPixels" => Some(std::mem::transmute::<
                    unsafe extern "C" fn(GLint, GLint, GLsizei, GLsizei, GLenum, GLenum, *mut c_void),
                    unsafe extern "C" fn(),
                >(stub_read_pixels)),
                _ => None,
            }
        }
    }

    /// A host whose context resolves every OpenGL function `Fbo::new` needs, through
    /// [`fake_gl_loader`].
    struct FakeGlHost;

    impl GlHost for FakeGlHost {
        fn create_context(&mut self, _: GlRequest) -> Result<(), String> {
            Ok(())
        }
        fn make_core_current(&mut self) {}
        fn make_frontend_current(&mut self) {}
        fn loader(&self) -> HwGetProcAddressFn {
            fake_gl_loader
        }
        fn destroy_context(&mut self) {}
    }

    #[test]
    fn resize_destroys_and_resets_the_context_once_even_when_the_new_fbo_fails() {
        let _serial = crate::libretro::tests::serial();
        FBO_BUILD_COUNT.store(0, Ordering::SeqCst);
        FBO_FAIL_AT.store(2, Ordering::SeqCst);
        CONTEXT_RESET_CALLS.store(0, Ordering::SeqCst);
        CONTEXT_DESTROY_CALLS.store(0, Ordering::SeqCst);
        let mut state = callbacks::State::new(c"/sys".into(), c"/save".into());
        state.gl_host = Some(Box::new(FakeGlHost));
        callbacks::install(state);
        let request = HwRequest {
            gl: GlRequest {
                core_profile: true,
                major: 3,
                minor: 3,
            },
            depth: false,
            stencil: false,
            bottom_left_origin: true,
            context_reset: Some(count_context_reset),
            context_destroy: Some(count_context_destroy),
        };
        let mut hw = Hw::start(request, 480, 272, || panic!("the first FBO build succeeds"))
            .expect("the first FBO build succeeds");
        assert_eq!(
            CONTEXT_RESET_CALLS.load(Ordering::SeqCst),
            1,
            "start resets the context once"
        );
        let before = callbacks::get_current_framebuffer();
        hw.resize((960, 544));
        assert_eq!(
            FBO_BUILD_COUNT.load(Ordering::SeqCst),
            2,
            "resize tried to build a new FBO"
        );
        assert_eq!(
            (hw.fbo.width, hw.fbo.height),
            (480, 272),
            "the failed resize keeps the old FBO"
        );
        assert_eq!(
            callbacks::get_current_framebuffer(),
            before,
            "the Core keeps drawing into it"
        );
        assert_eq!(
            CONTEXT_DESTROY_CALLS.load(Ordering::SeqCst),
            1,
            "resize destroys the context exactly once"
        );
        assert_eq!(
            CONTEXT_RESET_CALLS.load(Ordering::SeqCst),
            2,
            "resize resets the context exactly once, even though the new FBO failed"
        );
        // Stopping clears the process-wide FBO and loader, so no later test inherits them.
        hw.stop(|| {});
        callbacks::remove();
    }

    #[test]
    fn requests_name_their_profile() {
        let core = GlRequest {
            core_profile: true,
            major: 3,
            minor: 3,
        };
        assert_eq!(core.to_string(), "OpenGL 3.3 core");
        let compatibility = GlRequest {
            core_profile: false,
            major: 0,
            minor: 0,
        };
        assert_eq!(compatibility.to_string(), "OpenGL compatibility");
    }
}
