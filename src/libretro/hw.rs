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
use super::ffi::{HwContextResetFn, ProcAddress};
use super::{Frame, PixelFormat};

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
    /// The address of the OpenGL function `name`, `None` if there is none.
    fn proc_address(&self, name: &CStr) -> ProcAddress;
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
    fbo: GLuint,
    texture: GLuint,
    /// The depth (and stencil) renderbuffer, 0 when the Core asked for neither.
    depth: GLuint,
    width: u32,
    height: u32,
}

impl Hw {
    /// Creates the Core's context and a `width × height` FBO, and hands both to the Core
    /// (`context_reset`). Leaves the frontend's context current.
    pub fn start(request: HwRequest, width: u32, height: u32) -> Result<Hw, String> {
        host(|h| h.create_context(request.gl)).ok_or("no OpenGL host")??;
        host(|h| h.make_core_current());
        // SAFETY: the host looks functions up with the platform's GL loader.
        let made = unsafe { Gl::load(|name| host(|h| h.proc_address(name)).flatten()) }
            .and_then(|gl| Hw::with_fbo(gl, request, width.max(1), height.max(1)));
        let hw = match made {
            Ok(hw) => hw,
            Err(e) => {
                host(|h| h.destroy_context());
                return Err(e);
            }
        };
        callbacks::with(|s| s.fbo = hw.fbo);
        eprintln!(
            "game37: the Core draws with {} into a {width}×{height} framebuffer",
            request.gl
        );
        if let Some(reset) = request.context_reset {
            // SAFETY: the Core's context is current with its FBO bound, as libretro.h promises
            // `context_reset`; the callbacks' state is not borrowed, so the Core may call back.
            unsafe { reset() };
        }
        host(|h| h.make_frontend_current());
        Ok(hw)
    }

    /// Makes the FBO with the Core's context current, leaving it bound.
    fn with_fbo(gl: Gl, request: HwRequest, width: u32, height: u32) -> Result<Hw, String> {
        let (w, h) = (width as GLsizei, height as GLsizei);
        let mut hw = Hw {
            gl,
            request,
            fbo: 0,
            texture: 0,
            depth: 0,
            width,
            height,
        };
        let gl = &hw.gl;
        // SAFETY: the Core's context is current and these are its OpenGL functions; every pointer
        // passed points at a live local or field, or is null where OpenGL allows it.
        unsafe {
            (gl.gen_textures)(1, &mut hw.texture);
            (gl.bind_texture)(GL_TEXTURE_2D, hw.texture);
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
            (gl.gen_framebuffers)(1, &mut hw.fbo);
            (gl.bind_framebuffer)(GL_FRAMEBUFFER, hw.fbo);
            (gl.framebuffer_texture_2d)(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, hw.texture, 0);
            if request.depth || request.stencil {
                let (format, attachment) = if request.stencil {
                    (GL_DEPTH24_STENCIL8, GL_DEPTH_STENCIL_ATTACHMENT)
                } else {
                    (GL_DEPTH_COMPONENT24, GL_DEPTH_ATTACHMENT)
                };
                (gl.gen_renderbuffers)(1, &mut hw.depth);
                (gl.bind_renderbuffer)(GL_RENDERBUFFER, hw.depth);
                (gl.renderbuffer_storage)(GL_RENDERBUFFER, format, w, h);
                (gl.framebuffer_renderbuffer)(GL_FRAMEBUFFER, attachment, GL_RENDERBUFFER, hw.depth);
                (gl.bind_renderbuffer)(GL_RENDERBUFFER, 0);
            }
            let status = (gl.check_framebuffer_status)(GL_FRAMEBUFFER);
            if status != GL_FRAMEBUFFER_COMPLETE {
                hw.delete();
                return Err(format!("the framebuffer is incomplete ({status:#x})"));
            }
            (gl.clear_color)(0.0, 0.0, 0.0, 1.0);
            (gl.clear)(GL_COLOR_BUFFER_BIT);
        }
        Ok(hw)
    }

    /// Deletes the FBO and its attachments, with the Core's context current.
    fn delete(&mut self) {
        let gl = &self.gl;
        // SAFETY: the Core's context is current; deleting the name 0 is ignored.
        unsafe {
            (gl.bind_framebuffer)(GL_FRAMEBUFFER, 0);
            (gl.delete_framebuffers)(1, &self.fbo);
            (gl.delete_renderbuffers)(1, &self.depth);
            (gl.delete_textures)(1, &self.texture);
        }
        (self.fbo, self.depth, self.texture) = (0, 0, 0);
    }

    /// Runs `f` (a `retro_run`) with the Core's context current, then reads the frame it drew into
    /// the FBO, if it drew one, into the callbacks' frame.
    pub fn run(&self, f: impl FnOnce()) {
        host(|h| h.make_core_current());
        f();
        callbacks::with(|s| {
            if let Some((width, height)) = s.hw_size.take() {
                self.read(&mut s.frame, width, height);
            }
        });
        host(|h| h.make_frontend_current());
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
        self.delete();
        host(|h| h.destroy_context());
    }

    /// Reads the `width × height` picture (at most the FBO's size) into `frame`, top row first,
    /// with the Core's context current.
    fn read(&self, frame: &mut Frame, width: u32, height: u32) {
        let (w, h) = (width.min(self.width), height.min(self.height));
        let pitch = w as usize * 4;
        frame.data.resize(pitch * h as usize, 0);
        let gl = &self.gl;
        // SAFETY: the Core's context is current. `frame.data` holds `w × h` RGBA pixels, which is
        // what `glReadPixels` writes with no pack buffer bound, rows packed and 4-byte aligned. The
        // Core's bindings and pack state are put back as they were.
        unsafe {
            let (mut read_fbo, mut pack_buffer, mut row_length, mut alignment) = (0, 0, 0, 0);
            (gl.get_integer_v)(GL_READ_FRAMEBUFFER_BINDING, &mut read_fbo);
            (gl.get_integer_v)(GL_PIXEL_PACK_BUFFER_BINDING, &mut pack_buffer);
            (gl.get_integer_v)(GL_PACK_ROW_LENGTH, &mut row_length);
            (gl.get_integer_v)(GL_PACK_ALIGNMENT, &mut alignment);
            (gl.bind_framebuffer)(GL_READ_FRAMEBUFFER, self.fbo);
            (gl.bind_buffer)(GL_PIXEL_PACK_BUFFER, 0);
            (gl.pixel_store_i)(GL_PACK_ROW_LENGTH, 0);
            (gl.pixel_store_i)(GL_PACK_ALIGNMENT, 4);
            (gl.read_pixels)(
                0,
                0,
                w as GLsizei,
                h as GLsizei,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                frame.data.as_mut_ptr().cast(),
            );
            (gl.pixel_store_i)(GL_PACK_ALIGNMENT, alignment);
            (gl.pixel_store_i)(GL_PACK_ROW_LENGTH, row_length);
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
