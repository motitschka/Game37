//! The callbacks a Core calls into, and the thread-local state they work on.
//!
//! libretro callbacks carry no context pointer, so the loaded Core's [`State`] sits in a
//! thread-local slot. A callback that finds no state (or finds it already borrowed) does nothing
//! and answers a safe default, and no panic leaves a callback: each one is caught and turned into
//! that default.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, Ordering};

use super::ffi::{
    self, ProcAddress, RetroGameGeometry, RetroHwRenderCallback, RetroSystemAvInfo, RetroVariable,
};
use super::hw::{GlHost, GlRequest, HwRequest};
use super::{AvInfo, Frame, InputState, PixelFormat};

/// Everything the callbacks of the loaded Core read and write.
pub(super) struct State {
    pub pixel_format: PixelFormat,
    /// Handed to the Core as `const char *`; kept alive here until the Core is unloaded.
    pub system_dir: CString,
    pub save_dir: CString,
    pub av_info: AvInfo,
    /// The Core asked `GET_INPUT_BITMASKS`, so it may query `RETRO_DEVICE_ID_JOYPAD_MASK`.
    pub bitmasks: bool,
    pub input: InputState,
    /// The last frame the Core sent, when `fresh`.
    pub frame: Frame,
    /// A frame arrived since the host last took one.
    pub fresh: bool,
    /// Interleaved stereo samples since the host last cleared them.
    pub audio: Vec<i16>,
    /// Each Core option's default value, by key, from `SET_VARIABLES`. Like `overrides`, handed
    /// out as `const char *`, so kept until the Core is unloaded.
    pub options: HashMap<CString, CString>,
    /// The host's values for Core options, by key, answered ahead of the defaults.
    pub overrides: HashMap<CString, CString>,
    /// The frontend's OpenGL side; without it, hardware rendering is refused.
    pub gl_host: Option<Box<dyn GlHost>>,
    /// What the Core asked for in `SET_HW_RENDER`, once accepted.
    pub hw: Option<HwRequest>,
    /// The Core drew its last frame into the FBO at this size; it is read back into `frame` after
    /// `retro_run`.
    pub hw_size: Option<(u32, u32)>,
}

impl State {
    pub fn new(system_dir: CString, save_dir: CString) -> State {
        State {
            pixel_format: PixelFormat::default(),
            system_dir,
            save_dir,
            av_info: AvInfo::default(),
            bitmasks: false,
            input: InputState::default(),
            frame: Frame::default(),
            fresh: false,
            audio: Vec::new(),
            options: HashMap::new(),
            overrides: HashMap::new(),
            gl_host: None,
            hw: None,
            hw_size: None,
        }
    }
}

thread_local! {
    static SLOT: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub(super) fn install(state: State) {
    SLOT.with(|slot| *slot.borrow_mut() = Some(state));
}

pub(super) fn remove() {
    SLOT.with(|slot| slot.borrow_mut().take());
}

/// Runs `f` on this thread's state; `None` when there is none or it is already in use.
pub(super) fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    SLOT.with(|slot| slot.try_borrow_mut().ok()?.as_mut().map(f))
}

/// The body of every callback: `f` on the state, or `default` when there is none or `f` panics.
fn guarded<R>(default: R, f: impl FnOnce(&mut State) -> R) -> R {
    panic::catch_unwind(AssertUnwindSafe(|| with(f)))
        .ok()
        .flatten()
        .unwrap_or(default)
}

/// `retro_environment_t`.
///
/// # Safety
/// `data` is what libretro.h specifies for `cmd`: null, or valid for that command's type.
pub(super) unsafe extern "C" fn environment(cmd: c_uint, data: *mut c_void) -> bool {
    // SAFETY: the Core passes `data` as libretro.h specifies for `cmd` (this function's contract).
    guarded(false, |s| unsafe { dispatch(s, cmd, data) })
}

/// `retro_video_refresh_t`. A null `data` repeats the last frame (`GET_CAN_DUPE`).
///
/// # Safety
/// A non-null `data` points at `height` rows `pitch` bytes apart, each `width` pixels long in the
/// pixel format the Core set.
pub(super) unsafe extern "C" fn video_refresh(
    data: *const c_void,
    width: c_uint,
    height: c_uint,
    pitch: usize,
) {
    guarded((), |s| {
        if data.is_null() {
            return;
        }
        if data == ffi::RETRO_HW_FRAME_BUFFER_VALID {
            if s.hw.is_some() {
                s.hw_size = Some((width, height));
                s.fresh = true;
            }
            return;
        }
        // SAFETY: this function's contract, which libretro.h places on the Core.
        if unsafe { copy_frame(&mut s.frame, data.cast(), width, height, pitch, s.pixel_format) } {
            s.hw_size = None;
            s.fresh = true;
        }
    })
}

pub(super) extern "C" fn audio_sample(left: i16, right: i16) {
    guarded((), |s| s.audio.extend_from_slice(&[left, right]))
}

/// `retro_audio_sample_batch_t`.
///
/// # Safety
/// `data` points at `frames` interleaved stereo frames, `2 × frames` samples.
pub(super) unsafe extern "C" fn audio_sample_batch(data: *const i16, frames: usize) -> usize {
    guarded(0, |s| {
        if data.is_null() {
            return 0;
        }
        // SAFETY: this function's contract, which libretro.h places on the Core.
        let samples = unsafe { std::slice::from_raw_parts(data, frames * 2) };
        s.audio.extend_from_slice(samples);
        frames
    })
}

/// The FBO the Core draws into, 0 when there is none. Not in the thread-local [`State`]: a Core may
/// ask for it from a thread of its own (PPSSPP records its draws on its emulator thread).
static FBO: AtomicU32 = AtomicU32::new(0);

/// Hands out `fbo` as the FBO the Core draws into; 0 when it has none.
pub(super) fn set_fbo(fbo: u32) {
    FBO.store(fbo, Ordering::Relaxed);
}

/// `retro_hw_get_current_framebuffer_t`: the FBO the Core draws into, on any thread.
pub(super) extern "C" fn get_current_framebuffer() -> usize {
    FBO.load(Ordering::Relaxed) as usize
}

/// `retro_hw_get_proc_address_t`: an OpenGL function, looked up by the frontend.
///
/// # Safety
/// `sym` is null or a NUL-terminated string.
pub(super) unsafe extern "C" fn get_proc_address(sym: *const c_char) -> ProcAddress {
    guarded(None, |s| {
        if sym.is_null() {
            return None;
        }
        // SAFETY: non-null and NUL-terminated by this function's contract.
        let name = unsafe { CStr::from_ptr(sym) };
        s.gl_host.as_ref()?.proc_address(name)
    })
}

/// The host sets the input before each `retro_run`, so there is nothing to poll.
pub(super) extern "C" fn input_poll() {}

pub(super) extern "C" fn input_state(port: c_uint, device: c_uint, _index: c_uint, id: c_uint) -> i16 {
    guarded(0, |s| input_answer(&s.input, s.bitmasks, port, device, id))
}

/// Answers `cmd` for the Core, returning whether it is supported.
///
/// # Safety
/// `data` is null or valid for the type libretro.h gives `cmd`.
pub(super) unsafe fn dispatch(s: &mut State, cmd: c_uint, data: *mut c_void) -> bool {
    match cmd {
        ffi::ENV_SET_PIXEL_FORMAT => {
            // SAFETY: for this command `data` is a `const enum retro_pixel_format *` (an int).
            let Some(&raw) = (unsafe { data.cast::<c_int>().as_ref() }) else {
                return false;
            };
            match PixelFormat::from_raw(raw) {
                Some(format) => {
                    s.pixel_format = format;
                    true
                }
                None => false,
            }
        }
        // SAFETY (all `write_out` arms): the command's `data` is a pointer to the written type.
        ffi::ENV_GET_SYSTEM_DIRECTORY => unsafe { write_out(data, s.system_dir.as_ptr()) },
        ffi::ENV_GET_SAVE_DIRECTORY => unsafe { write_out(data, s.save_dir.as_ptr()) },
        ffi::ENV_GET_CAN_DUPE => unsafe { write_out(data, true) },
        ffi::ENV_GET_AUDIO_VIDEO_ENABLE => unsafe {
            write_out(data, ffi::RETRO_AV_ENABLE_VIDEO | ffi::RETRO_AV_ENABLE_AUDIO)
        },
        ffi::ENV_SET_INPUT_DESCRIPTORS
        | ffi::ENV_SET_CONTROLLER_INFO
        | ffi::ENV_SET_MEMORY_MAPS
        | ffi::ENV_SET_SUBSYSTEM_INFO
        | ffi::ENV_SET_SUPPORT_ACHIEVEMENTS => true,
        ffi::ENV_GET_INPUT_BITMASKS => {
            s.bitmasks = true;
            true
        }
        ffi::ENV_SET_GEOMETRY => {
            // SAFETY: for this command `data` is a `const struct retro_game_geometry *`.
            let Some(g) = (unsafe { data.cast::<RetroGameGeometry>().as_ref() }) else {
                return false;
            };
            s.av_info.set_geometry(g);
            true
        }
        ffi::ENV_SET_HW_RENDER => {
            // SAFETY: for this command `data` is a `struct retro_hw_render_callback *`.
            let Some(cb) = (unsafe { data.cast::<RetroHwRenderCallback>().as_mut() }) else {
                return false;
            };
            set_hw_render(s, cb)
        }
        ffi::ENV_GET_PREFERRED_HW_RENDER if s.gl_host.is_some() => unsafe {
            // SAFETY: for this command `data` is an `enum retro_hw_context_type *`.
            write_out(data, ffi::RETRO_HW_CONTEXT_OPENGL_CORE)
        },
        // The Core's context is its own already: nothing else draws with it.
        ffi::ENV_SET_HW_SHARED_CONTEXT => s.gl_host.is_some(),
        ffi::ENV_SET_SYSTEM_AV_INFO => {
            // SAFETY: for this command `data` is a `const struct retro_system_av_info *`.
            let Some(av) = (unsafe { data.cast::<RetroSystemAvInfo>().as_ref() }) else {
                return false;
            };
            s.av_info = AvInfo::from_raw(av);
            true
        }
        ffi::ENV_GET_LOG_INTERFACE => unsafe {
            write_out(
                data,
                ffi::RetroLogCallback {
                    log: ffi::game37_core_log,
                },
            )
        },
        // Cores run with their options' defaults: some (Mupen64Plus-Next) have no defaults of
        // their own and crash without them.
        ffi::ENV_SET_VARIABLES => {
            // SAFETY: for this command `data` is an array of `struct retro_variable` ended by
            // one with a null key, each value a NUL-terminated string.
            unsafe { set_variables(s, data.cast()) };
            true
        }
        ffi::ENV_GET_VARIABLE => {
            // SAFETY: for this command `data` is a `struct retro_variable *` with a
            // NUL-terminated key.
            let Some(var) = (unsafe { data.cast::<RetroVariable>().as_mut() }) else {
                return false;
            };
            if var.key.is_null() {
                return false;
            }
            // SAFETY: non-null and NUL-terminated, as above.
            let key = unsafe { CStr::from_ptr(var.key) };
            match s.overrides.get(key).or_else(|| s.options.get(key)) {
                Some(value) => {
                    var.value = value.as_ptr();
                    true
                }
                None => false,
            }
        }
        // Options never change while a Game runs.
        ffi::ENV_GET_VARIABLE_UPDATE => unsafe { write_out(data, false) },
        _ => false,
    }
}

/// `SET_VARIABLES`: records each option's default, replacing those recorded before.
///
/// # Safety
/// `vars` is null or points at `struct retro_variable`s ended by one with a null key, each
/// non-null value NUL-terminated.
unsafe fn set_variables(s: &mut State, mut vars: *const RetroVariable) {
    s.options.clear();
    // SAFETY: each element up to the terminator is valid, by this function's contract.
    while let Some(var) = unsafe { vars.as_ref() } {
        if var.key.is_null() {
            break;
        }
        if !var.value.is_null() {
            // SAFETY: non-null and NUL-terminated, by this function's contract.
            let (key, value) = unsafe { (CStr::from_ptr(var.key), CStr::from_ptr(var.value)) };
            if let Some(default) = option_default(value.to_bytes()) {
                s.options.insert(key.to_owned(), default);
            }
        }
        // SAFETY: the array goes on until the terminator.
        vars = unsafe { vars.add(1) };
    }
}

/// Core option values given by the host, `(key, value)`, as the C strings handed to the Core.
///
/// # Panics
/// On a NUL in a key or value: they are Game37's own constants.
pub(super) fn option_overrides(options: &[(&str, &str)]) -> HashMap<CString, CString> {
    let c = |s: &str| CString::new(s).expect("Core option keys and values have no NUL");
    options.iter().map(|&(key, value)| (c(key), c(value))).collect()
}

/// The default in a `SET_VARIABLES` value, `"Description; first|second|..."`: the first choice.
pub(super) fn option_default(value: &[u8]) -> Option<CString> {
    let at = value.windows(2).position(|w| w == b"; ")?;
    let choices = &value[at + 2..];
    let first = choices.split(|&b| b == b'|').next()?;
    CString::new(first).ok()
}

/// `SET_HW_RENDER`: accepts OpenGL, compatibility or a 3.x/4.x core profile, when there is a host
/// to make the context, and hands the Core the frontend's functions. GLES, Vulkan and the rest
/// are refused, so the Core falls back to what it has.
fn set_hw_render(s: &mut State, cb: &mut RetroHwRenderCallback) -> bool {
    if s.gl_host.is_none() {
        return false;
    }
    let gl = match cb.context_type {
        ffi::RETRO_HW_CONTEXT_OPENGL => GlRequest {
            core_profile: false,
            major: 0,
            minor: 0,
        },
        ffi::RETRO_HW_CONTEXT_OPENGL_CORE if matches!(cb.version_major, 3 | 4) => GlRequest {
            core_profile: true,
            major: cb.version_major,
            minor: cb.version_minor,
        },
        _ => return false,
    };
    s.hw = Some(HwRequest {
        gl,
        depth: cb.depth,
        stencil: cb.stencil,
        bottom_left_origin: cb.bottom_left_origin,
        context_reset: cb.context_reset,
        context_destroy: cb.context_destroy,
    });
    cb.get_current_framebuffer = Some(get_current_framebuffer);
    cb.get_proc_address = Some(get_proc_address);
    true
}

/// Writes `value` through `data`; false when `data` is null.
///
/// # Safety
/// `data` is null or valid and aligned for writing a `T`.
unsafe fn write_out<T>(data: *mut c_void, value: T) -> bool {
    let out = data.cast::<T>();
    if out.is_null() {
        return false;
    }
    // SAFETY: non-null, and valid for a `T` by this function's contract.
    unsafe { out.write(value) };
    true
}

/// What `input_state` returns for `RETRO_DEVICE_JOYPAD` queries; 0 for any other device.
pub(super) fn input_answer(
    input: &InputState,
    bitmasks: bool,
    port: c_uint,
    device: c_uint,
    id: c_uint,
) -> i16 {
    if device & ffi::RETRO_DEVICE_MASK != ffi::RETRO_DEVICE_JOYPAD {
        return 0;
    }
    let mask = input.mask(port as usize);
    match id {
        // R3 is bit 15, the sign bit of the returned i16.
        ffi::RETRO_DEVICE_ID_JOYPAD_MASK if bitmasks => mask as i16,
        0..=15 => i16::from(mask >> id & 1 == 1),
        _ => 0,
    }
}

/// Copies a frame into `dst`, packing its rows. False, leaving `dst` alone, when `pitch` is
/// shorter than a row.
///
/// # Safety
/// `src` points at `height` rows `pitch` bytes apart, each at least `width` pixels of `format`.
pub(super) unsafe fn copy_frame(
    dst: &mut Frame,
    src: *const u8,
    width: c_uint,
    height: c_uint,
    pitch: usize,
    format: PixelFormat,
) -> bool {
    let row = width as usize * format.bytes_per_pixel();
    if pitch < row {
        return false;
    }
    dst.data.clear();
    dst.data.reserve(row * height as usize);
    for y in 0..height as usize {
        // SAFETY: row `y` starts `y × pitch` bytes into the frame and holds at least `row` bytes,
        // by this function's contract.
        dst.data
            .extend_from_slice(unsafe { std::slice::from_raw_parts(src.add(y * pitch), row) });
    }
    dst.width = width;
    dst.height = height;
    dst.pitch = row;
    dst.format = format;
    true
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;
    use crate::libretro::{Button, MAX_PORTS};

    fn state() -> State {
        State::new(CString::new("/sys").unwrap(), CString::new("/save").unwrap())
    }

    fn env<T>(s: &mut State, cmd: c_uint, value: &mut T) -> bool {
        // SAFETY: `value` is a valid, exclusive `T` for the call.
        unsafe { dispatch(s, cmd, (value as *mut T).cast()) }
    }

    fn geometry(w: u32, h: u32, aspect: f32) -> RetroGameGeometry {
        RetroGameGeometry {
            base_width: w,
            base_height: h,
            max_width: 2 * w,
            max_height: 2 * h,
            aspect_ratio: aspect,
        }
    }

    #[test]
    fn pixel_formats_are_accepted_and_recorded() {
        let mut s = state();
        assert_eq!(s.pixel_format, PixelFormat::Xrgb1555);
        for (raw, format) in [
            (1, PixelFormat::Xrgb8888),
            (2, PixelFormat::Rgb565),
            (0, PixelFormat::Xrgb1555),
        ] {
            assert!(env(&mut s, ffi::ENV_SET_PIXEL_FORMAT, &mut { raw as c_int }));
            assert_eq!(s.pixel_format, format);
        }
    }

    #[test]
    fn unknown_pixel_format_is_refused_and_keeps_the_current_one() {
        let mut s = state();
        assert!(env(&mut s, ffi::ENV_SET_PIXEL_FORMAT, &mut 2 as &mut c_int));
        assert!(!env(&mut s, ffi::ENV_SET_PIXEL_FORMAT, &mut 3 as &mut c_int));
        assert!(!env(&mut s, ffi::ENV_SET_PIXEL_FORMAT, &mut { c_int::MAX }));
        assert_eq!(s.pixel_format, PixelFormat::Rgb565);
    }

    #[test]
    fn bytes_per_pixel() {
        assert_eq!(PixelFormat::Xrgb1555.bytes_per_pixel(), 2);
        assert_eq!(PixelFormat::Rgb565.bytes_per_pixel(), 2);
        assert_eq!(PixelFormat::Xrgb8888.bytes_per_pixel(), 4);
    }

    #[test]
    fn directories_are_handed_out_as_the_kept_strings() {
        let mut s = state();
        let mut out: *const std::ffi::c_char = ptr::null();
        assert!(env(&mut s, ffi::ENV_GET_SYSTEM_DIRECTORY, &mut out));
        assert_eq!(out, s.system_dir.as_ptr());
        assert!(env(&mut s, ffi::ENV_GET_SAVE_DIRECTORY, &mut out));
        assert_eq!(out, s.save_dir.as_ptr());
    }

    #[test]
    fn can_dupe_and_audio_video_enable() {
        let mut s = state();
        let mut dupe = false;
        assert!(env(&mut s, ffi::ENV_GET_CAN_DUPE, &mut dupe));
        assert!(dupe);
        let mut av: c_int = 0;
        assert!(env(&mut s, ffi::ENV_GET_AUDIO_VIDEO_ENABLE, &mut av));
        assert_eq!(av, 0b11);
    }

    #[test]
    fn descriptions_are_accepted_and_ignored() {
        let mut s = state();
        for cmd in [
            ffi::ENV_SET_INPUT_DESCRIPTORS,
            ffi::ENV_SET_CONTROLLER_INFO,
            ffi::ENV_SET_MEMORY_MAPS,
            ffi::ENV_SET_SUBSYSTEM_INFO,
            ffi::ENV_SET_SUPPORT_ACHIEVEMENTS,
        ] {
            // SAFETY: these commands' data is never read.
            assert!(unsafe { dispatch(&mut s, cmd, ptr::null_mut()) }, "cmd {cmd}");
        }
    }

    #[test]
    fn input_bitmasks_are_offered_and_remembered() {
        let mut s = state();
        // SAFETY: GET_INPUT_BITMASKS ignores its data.
        assert!(unsafe { dispatch(&mut s, ffi::ENV_GET_INPUT_BITMASKS, ptr::null_mut()) });
        assert!(s.bitmasks);
    }

    #[test]
    fn unknown_commands_are_unsupported() {
        let mut s = state();
        let mut dummy = [0usize; 4];
        for cmd in [
            52,                    // GET_CORE_OPTIONS_VERSION: Cores fall back to SET_VARIABLES
            ffi::ENV_GET_VARIABLE, // with a null key
            9999,
        ] {
            assert!(!env(&mut s, cmd, &mut dummy), "cmd {cmd}");
        }
        assert_eq!(dummy, [0; 4]);
    }

    #[test]
    fn every_core_gets_the_log_callback() {
        let mut s = state();
        let mut out: Option<ffi::LogPrintfFn> = None;
        assert!(env(&mut s, ffi::ENV_GET_LOG_INTERFACE, &mut out));
        assert_eq!(
            out.map(|f| f as usize),
            Some(ffi::game37_core_log as ffi::LogPrintfFn as usize)
        );
    }

    /// `SET_VARIABLES`' array: each `(key, value)`, then the terminator. Keeps the strings alive.
    fn variables(vars: &[(&str, &str)]) -> (Vec<CString>, Vec<RetroVariable>) {
        let strings: Vec<CString> = vars
            .iter()
            .flat_map(|&(k, v)| [CString::new(k).unwrap(), CString::new(v).unwrap()])
            .collect();
        let mut array: Vec<RetroVariable> = strings
            .chunks(2)
            .map(|kv| RetroVariable {
                key: kv[0].as_ptr(),
                value: kv[1].as_ptr(),
            })
            .collect();
        array.push(RetroVariable {
            key: ptr::null(),
            value: ptr::null(),
        });
        (strings, array)
    }

    /// `GET_VARIABLE` for `key`: the value, `None` when refused.
    fn get_variable(s: &mut State, key: &str) -> Option<String> {
        let key = CString::new(key).unwrap();
        let mut var = RetroVariable {
            key: key.as_ptr(),
            value: ptr::null(),
        };
        // SAFETY: on success the value points into `s`, which outlives this read.
        env(s, ffi::ENV_GET_VARIABLE, &mut var)
            .then(|| unsafe { CStr::from_ptr(var.value) }.to_str().unwrap().to_owned())
    }

    #[test]
    fn options_are_answered_with_their_defaults() {
        let mut s = state();
        let (_strings, mut array) = variables(&[
            ("mupen64plus-rdp-plugin", "RDP Plugin; gliden64|angrylion"),
            (
                "ppsspp_internal_resolution",
                "Internal Resolution; 480x272|960x544",
            ),
            ("broken", "no separator"),
        ]);
        // SAFETY: `array` is ended by a null key; its strings live in `_strings`.
        assert!(unsafe { dispatch(&mut s, ffi::ENV_SET_VARIABLES, array.as_mut_ptr().cast()) });
        assert_eq!(
            get_variable(&mut s, "mupen64plus-rdp-plugin").as_deref(),
            Some("gliden64")
        );
        assert_eq!(
            get_variable(&mut s, "ppsspp_internal_resolution").as_deref(),
            Some("480x272")
        );
        assert_eq!(get_variable(&mut s, "broken"), None);
        assert_eq!(get_variable(&mut s, "unknown"), None);
        let mut updated = true;
        assert!(env(&mut s, ffi::ENV_GET_VARIABLE_UPDATE, &mut updated));
        assert!(!updated, "options never change while a Game runs");
    }

    #[test]
    fn the_hosts_option_values_come_before_the_defaults() {
        let mut s = state();
        s.overrides = option_overrides(&[("melonds_screen_layout", "Left/Right"), ("only_ours", "on")]);
        let (_strings, mut array) = variables(&[("melonds_screen_layout", "Layout; Top/Bottom|Left/Right")]);
        // SAFETY: `array` is ended by a null key; its strings live in `_strings`.
        assert!(unsafe { dispatch(&mut s, ffi::ENV_SET_VARIABLES, array.as_mut_ptr().cast()) });
        assert_eq!(
            get_variable(&mut s, "melonds_screen_layout").as_deref(),
            Some("Left/Right")
        );
        assert_eq!(get_variable(&mut s, "only_ours").as_deref(), Some("on"));
    }

    #[test]
    fn an_option_default_is_the_first_choice() {
        let default = |v: &str| option_default(v.as_bytes()).map(|c| c.into_string().unwrap());
        assert_eq!(
            default("Renderer; hardware|software").as_deref(),
            Some("hardware")
        );
        assert_eq!(default("Speed; 100").as_deref(), Some("100"));
        assert_eq!(
            default("Layout; Top/Bottom; odd|x").as_deref(),
            Some("Top/Bottom; odd")
        );
        assert_eq!(default("no choices"), None);
    }

    /// A host whose context is never made: dispatch only checks there is one.
    struct NoGl;

    impl GlHost for NoGl {
        fn create_context(&mut self, _: GlRequest) -> Result<(), String> {
            Err("unused".into())
        }
        fn make_core_current(&mut self) {}
        fn make_frontend_current(&mut self) {}
        fn proc_address(&self, _: &CStr) -> ProcAddress {
            None
        }
        fn destroy_context(&mut self) {}
    }

    fn with_gl() -> State {
        let mut s = state();
        s.gl_host = Some(Box::new(NoGl));
        s
    }

    fn hw_render(context_type: c_int, major: c_uint, minor: c_uint) -> RetroHwRenderCallback {
        RetroHwRenderCallback {
            context_type,
            context_reset: None,
            get_current_framebuffer: None,
            get_proc_address: None,
            depth: true,
            stencil: false,
            bottom_left_origin: true,
            version_major: major,
            version_minor: minor,
            cache_context: false,
            context_destroy: None,
            debug_context: false,
        }
    }

    #[test]
    fn opengl_and_opengl_core_3_and_4_are_accepted_with_a_host() {
        for (context_type, major, minor, gl) in [
            (ffi::RETRO_HW_CONTEXT_OPENGL, 0, 0, (false, 0, 0)),
            (ffi::RETRO_HW_CONTEXT_OPENGL_CORE, 3, 3, (true, 3, 3)),
            (ffi::RETRO_HW_CONTEXT_OPENGL_CORE, 4, 5, (true, 4, 5)),
        ] {
            let mut s = with_gl();
            let mut cb = hw_render(context_type, major, minor);
            assert!(
                env(&mut s, ffi::ENV_SET_HW_RENDER, &mut cb),
                "{context_type} {major}.{minor}"
            );
            let request = s.hw.expect("recorded");
            let (core_profile, major, minor) = gl;
            assert_eq!(
                request.gl,
                GlRequest {
                    core_profile,
                    major,
                    minor
                }
            );
            assert!(request.depth && !request.stencil && request.bottom_left_origin);
            assert_eq!(
                cb.get_current_framebuffer.map(|f| f as usize),
                Some(get_current_framebuffer as ffi::HwGetCurrentFramebufferFn as usize)
            );
            assert_eq!(
                cb.get_proc_address.map(|f| f as usize),
                Some(get_proc_address as ffi::HwGetProcAddressFn as usize)
            );
        }
    }

    #[test]
    fn gles_vulkan_and_old_core_profiles_are_refused() {
        for (context_type, major) in [
            (2, 0),                                 // GLES2
            (4, 3),                                 // GLES3
            (5, 3),                                 // GLES_VERSION
            (6, 1),                                 // Vulkan
            (ffi::RETRO_HW_CONTEXT_OPENGL_CORE, 2), // a core profile before 3.0
        ] {
            let mut s = with_gl();
            let mut cb = hw_render(context_type, major, 0);
            assert!(
                !env(&mut s, ffi::ENV_SET_HW_RENDER, &mut cb),
                "{context_type} {major}"
            );
            assert!(s.hw.is_none() && cb.get_current_framebuffer.is_none());
        }
    }

    #[test]
    fn the_framebuffer_is_handed_out_on_any_thread() {
        set_fbo(7);
        assert_eq!(get_current_framebuffer(), 7);
        let elsewhere = std::thread::spawn(|| get_current_framebuffer()).join().unwrap();
        set_fbo(0);
        assert_eq!(elsewhere, 7, "the FBO, asked for from a thread of the Core's");
        assert_eq!(get_current_framebuffer(), 0);
    }

    #[test]
    fn without_a_host_there_is_no_hardware_rendering() {
        let mut s = state();
        let mut cb = hw_render(ffi::RETRO_HW_CONTEXT_OPENGL_CORE, 3, 3);
        assert!(!env(&mut s, ffi::ENV_SET_HW_RENDER, &mut cb));
        assert!(s.hw.is_none());
        let mut preferred: c_int = -1;
        assert!(!env(&mut s, ffi::ENV_GET_PREFERRED_HW_RENDER, &mut preferred));
        assert_eq!(preferred, -1);
        // SAFETY: SET_HW_SHARED_CONTEXT ignores its data.
        assert!(!unsafe { dispatch(&mut s, ffi::ENV_SET_HW_SHARED_CONTEXT, ptr::null_mut()) });
    }

    #[test]
    fn with_a_host_opengl_core_is_preferred() {
        let mut s = with_gl();
        let mut preferred: c_int = -1;
        assert!(env(&mut s, ffi::ENV_GET_PREFERRED_HW_RENDER, &mut preferred));
        assert_eq!(preferred, ffi::RETRO_HW_CONTEXT_OPENGL_CORE);
        // SAFETY: SET_HW_SHARED_CONTEXT ignores its data.
        assert!(unsafe { dispatch(&mut s, ffi::ENV_SET_HW_SHARED_CONTEXT, ptr::null_mut()) });
    }

    #[test]
    fn a_frame_drawn_with_opengl_is_marked_for_readback() {
        install(with_gl());
        // SAFETY: a null frame is a dupe; the HW marker is never dereferenced.
        unsafe { video_refresh(ffi::RETRO_HW_FRAME_BUFFER_VALID, 320, 240, 0) };
        assert_eq!(
            with(|s| (s.fresh, s.hw_size)),
            Some((false, None)),
            "no OpenGL accepted yet"
        );
        with(|s| s.hw = Some(hw_request()));
        // SAFETY: as above.
        unsafe { video_refresh(ffi::RETRO_HW_FRAME_BUFFER_VALID, 320, 240, 0) };
        assert_eq!(with(|s| (s.fresh, s.hw_size)), Some((true, Some((320, 240)))));
        remove();
    }

    fn hw_request() -> HwRequest {
        let mut s = with_gl();
        let mut cb = hw_render(ffi::RETRO_HW_CONTEXT_OPENGL_CORE, 3, 3);
        assert!(env(&mut s, ffi::ENV_SET_HW_RENDER, &mut cb));
        s.hw.unwrap()
    }

    #[test]
    fn null_data_is_refused_where_data_is_needed() {
        let mut s = state();
        for cmd in [
            ffi::ENV_SET_PIXEL_FORMAT,
            ffi::ENV_GET_SYSTEM_DIRECTORY,
            ffi::ENV_GET_SAVE_DIRECTORY,
            ffi::ENV_GET_CAN_DUPE,
            ffi::ENV_SET_GEOMETRY,
            ffi::ENV_SET_SYSTEM_AV_INFO,
        ] {
            // SAFETY: null is what is being tested; it is never dereferenced.
            assert!(!unsafe { dispatch(&mut s, cmd, ptr::null_mut()) }, "cmd {cmd}");
        }
    }

    #[test]
    fn av_info_takes_the_cores_aspect_ratio() {
        let raw = RetroSystemAvInfo {
            geometry: geometry(256, 224, 4.0 / 3.0),
            timing: ffi::RetroSystemTiming {
                fps: 60.1,
                sample_rate: 32040.0,
            },
        };
        let info = AvInfo::from_raw(&raw);
        assert_eq!(
            info,
            AvInfo {
                base_width: 256,
                base_height: 224,
                max_width: 512,
                max_height: 448,
                aspect_ratio: 4.0 / 3.0,
                fps: 60.1,
                sample_rate: 32040.0
            }
        );
    }

    #[test]
    fn av_info_aspect_ratio_falls_back_to_width_over_height() {
        for aspect in [0.0, -1.0] {
            let raw = RetroSystemAvInfo {
                geometry: geometry(160, 144, aspect),
                ..Default::default()
            };
            assert_eq!(AvInfo::from_raw(&raw).aspect_ratio, 160.0 / 144.0);
        }
    }

    #[test]
    fn set_geometry_changes_base_size_and_aspect_but_not_max() {
        let mut s = state();
        s.av_info = AvInfo::from_raw(&RetroSystemAvInfo {
            geometry: geometry(256, 224, 4.0 / 3.0),
            timing: ffi::RetroSystemTiming {
                fps: 60.0,
                sample_rate: 48000.0,
            },
        });
        let mut g = RetroGameGeometry {
            base_width: 512,
            base_height: 448,
            max_width: 1,
            max_height: 1,
            aspect_ratio: 0.0,
        };
        assert!(env(&mut s, ffi::ENV_SET_GEOMETRY, &mut g));
        assert_eq!((s.av_info.base_width, s.av_info.base_height), (512, 448));
        assert_eq!((s.av_info.max_width, s.av_info.max_height), (512, 448));
        assert_eq!(s.av_info.aspect_ratio, 512.0 / 448.0);
        assert_eq!(s.av_info.fps, 60.0);
    }

    #[test]
    fn set_system_av_info_replaces_everything() {
        let mut s = state();
        let mut av = RetroSystemAvInfo {
            geometry: geometry(240, 160, 1.5),
            timing: ffi::RetroSystemTiming {
                fps: 59.73,
                sample_rate: 32768.0,
            },
        };
        assert!(env(&mut s, ffi::ENV_SET_SYSTEM_AV_INFO, &mut av));
        assert_eq!(s.av_info, AvInfo::from_raw(&av));
        assert_eq!(s.av_info.max_width, 480);
    }

    #[test]
    fn input_state_sets_and_clears_bits() {
        let mut input = InputState::default();
        input.set(0, Button::B, true);
        input.set(0, Button::A, true);
        input.set(3, Button::R3, true);
        assert_eq!(input.mask(0), 1 << 0 | 1 << 8);
        assert_eq!(input.mask(3), 0x8000);
        input.set(0, Button::B, false);
        assert_eq!(input.mask(0), 1 << 8);
        assert!(input.is_pressed(0, Button::A) && !input.is_pressed(0, Button::B));
        assert_eq!(input.mask(MAX_PORTS), 0);
        assert!(!input.is_pressed(MAX_PORTS, Button::A));
    }

    #[test]
    fn buttons_are_numbered_as_libretro() {
        assert_eq!(Button::B as u8, 0);
        assert_eq!(Button::Start as u8, 3);
        assert_eq!(Button::Up as u8, 4);
        assert_eq!(Button::A as u8, 8);
        assert_eq!(Button::R as u8, 11);
        assert_eq!(Button::R3 as u8, 15);
    }

    #[test]
    fn joypad_queries_answer_per_button() {
        let mut input = InputState::default();
        input.set(1, Button::Start, true);
        let joypad = ffi::RETRO_DEVICE_JOYPAD;
        assert_eq!(input_answer(&input, false, 1, joypad, Button::Start as c_uint), 1);
        assert_eq!(input_answer(&input, false, 1, joypad, Button::A as c_uint), 0);
        assert_eq!(input_answer(&input, false, 0, joypad, Button::Start as c_uint), 0);
        assert_eq!(input_answer(&input, false, 9, joypad, Button::Start as c_uint), 0);
        // A joypad subclass is still a joypad.
        let subclass = (1 + 1) << ffi::RETRO_DEVICE_TYPE_SHIFT | joypad;
        assert_eq!(
            input_answer(&input, false, 1, subclass, Button::Start as c_uint),
            1
        );
        // Other devices (here the keyboard, 3) read nothing.
        assert_eq!(input_answer(&input, false, 1, 3, Button::Start as c_uint), 0);
        assert_eq!(input_answer(&input, false, 1, joypad, 16), 0);
    }

    #[test]
    fn joypad_mask_is_answered_only_after_the_core_asked() {
        let mut input = InputState::default();
        input.set(0, Button::Y, true);
        input.set(0, Button::R3, true);
        let (joypad, mask) = (ffi::RETRO_DEVICE_JOYPAD, ffi::RETRO_DEVICE_ID_JOYPAD_MASK);
        assert_eq!(input_answer(&input, false, 0, joypad, mask), 0);
        assert_eq!(input_answer(&input, true, 0, joypad, mask) as u16, 0x8002);
    }

    #[test]
    fn frames_are_copied_with_rows_packed() {
        // 2×2 RGB565 frame with 2 bytes of padding per row.
        let src: [u8; 12] = [1, 2, 3, 4, 0xEE, 0xEE, 5, 6, 7, 8, 0xEE, 0xEE];
        let mut frame = Frame::default();
        // SAFETY: `src` holds 2 rows 6 bytes apart, each 2 RGB565 pixels.
        assert!(unsafe { copy_frame(&mut frame, src.as_ptr(), 2, 2, 6, PixelFormat::Rgb565) });
        assert_eq!(
            frame,
            Frame {
                width: 2,
                height: 2,
                pitch: 4,
                format: PixelFormat::Rgb565,
                data: vec![1, 2, 3, 4, 5, 6, 7, 8]
            }
        );
    }

    #[test]
    fn a_frame_with_too_short_a_pitch_is_dropped() {
        let src = [7u8; 16];
        let mut frame = Frame::default();
        // SAFETY: rejected before any read.
        assert!(!unsafe { copy_frame(&mut frame, src.as_ptr(), 2, 2, 7, PixelFormat::Xrgb8888) });
        assert_eq!(frame, Frame::default());
    }

    #[test]
    fn callbacks_fill_the_installed_state() {
        install(state());
        let src = [9u8; 8];
        // SAFETY: `src` is one row of 2 XRGB8888 pixels, and a null frame is a dupe.
        unsafe {
            video_refresh(ptr::null(), 2, 1, 8);
            assert_eq!(with(|s| s.fresh), Some(false));
            assert!(environment(
                ffi::ENV_SET_PIXEL_FORMAT,
                (&mut 1 as *mut c_int).cast()
            ));
            video_refresh(src.as_ptr().cast(), 2, 1, 8);
        }
        audio_sample(1, -1);
        let batch = [2i16, -2, 3, -3];
        // SAFETY: `batch` holds 2 stereo frames.
        assert_eq!(unsafe { audio_sample_batch(batch.as_ptr(), 2) }, 2);
        with(|s| {
            s.input.set(0, Button::Down, true);
            assert!(s.fresh);
            assert_eq!(s.frame.data, src);
            assert_eq!(s.frame.format, PixelFormat::Xrgb8888);
            assert_eq!(s.audio, [1, -1, 2, -2, 3, -3]);
        });
        assert_eq!(
            input_state(0, ffi::RETRO_DEVICE_JOYPAD, 0, Button::Down as c_uint),
            1
        );
        remove();
    }

    #[test]
    fn callbacks_without_state_answer_defaults() {
        remove();
        let mut dupe = false;
        // SAFETY: `dupe` is a valid bool; the batch pointer is never read without state.
        unsafe {
            assert!(!environment(
                ffi::ENV_GET_CAN_DUPE,
                (&mut dupe as *mut bool).cast()
            ));
            assert_eq!(audio_sample_batch([0i16; 2].as_ptr(), 1), 0);
        }
        assert!(!dupe);
        assert_eq!(input_state(0, ffi::RETRO_DEVICE_JOYPAD, 0, 0), 0);
        audio_sample(0, 0);
    }

    #[test]
    fn a_callback_reentering_busy_state_answers_the_default() {
        install(state());
        let mut dupe = false;
        let answer = with(|_| {
            // SAFETY: `dupe` is a valid bool.
            unsafe { environment(ffi::ENV_GET_CAN_DUPE, (&mut dupe as *mut bool).cast()) }
        });
        assert_eq!(answer, Some(false));
        remove();
    }

    #[test]
    fn a_panic_inside_a_callback_becomes_the_default() {
        install(state());
        assert_eq!(guarded(7, |_| panic!("boom")), 7);
        // The state survives the caught panic.
        assert_eq!(with(|s| s.bitmasks), Some(false));
        remove();
    }
}
