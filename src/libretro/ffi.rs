//! The subset of `libretro.h` (libretro-common) that the Core host uses, written by hand.

use std::ffi::{c_char, c_int, c_uint, c_void};

use libloading::Library;

pub const RETRO_API_VERSION: c_uint = 1;

pub const RETRO_DEVICE_TYPE_SHIFT: c_uint = 8;
pub const RETRO_DEVICE_MASK: c_uint = (1 << RETRO_DEVICE_TYPE_SHIFT) - 1;
pub const RETRO_DEVICE_JOYPAD: c_uint = 1;
pub const RETRO_DEVICE_ID_JOYPAD_MASK: c_uint = 256;

pub const RETRO_MEMORY_SAVE_RAM: c_uint = 0;

pub const RETRO_PIXEL_FORMAT_0RGB1555: c_int = 0;
pub const RETRO_PIXEL_FORMAT_XRGB8888: c_int = 1;
pub const RETRO_PIXEL_FORMAT_RGB565: c_int = 2;

pub const RETRO_AV_ENABLE_VIDEO: c_int = 1 << 0;
pub const RETRO_AV_ENABLE_AUDIO: c_int = 1 << 1;

const EXPERIMENTAL: c_uint = 0x10000;
pub const ENV_GET_CAN_DUPE: c_uint = 3;
pub const ENV_GET_SYSTEM_DIRECTORY: c_uint = 9;
pub const ENV_SET_PIXEL_FORMAT: c_uint = 10;
pub const ENV_SET_INPUT_DESCRIPTORS: c_uint = 11;
pub const ENV_SET_HW_RENDER: c_uint = 14;
pub const ENV_GET_VARIABLE: c_uint = 15;
pub const ENV_SET_VARIABLES: c_uint = 16;
pub const ENV_GET_VARIABLE_UPDATE: c_uint = 17;
pub const ENV_GET_LOG_INTERFACE: c_uint = 27;
pub const ENV_GET_SAVE_DIRECTORY: c_uint = 31;
pub const ENV_SET_SYSTEM_AV_INFO: c_uint = 32;
pub const ENV_SET_SUBSYSTEM_INFO: c_uint = 34;
pub const ENV_SET_CONTROLLER_INFO: c_uint = 35;
pub const ENV_SET_MEMORY_MAPS: c_uint = 36 | EXPERIMENTAL;
pub const ENV_SET_GEOMETRY: c_uint = 37;
pub const ENV_SET_SUPPORT_ACHIEVEMENTS: c_uint = 42 | EXPERIMENTAL;
pub const ENV_SET_HW_SHARED_CONTEXT: c_uint = 44 | EXPERIMENTAL;
pub const ENV_GET_AUDIO_VIDEO_ENABLE: c_uint = 47 | EXPERIMENTAL;
pub const ENV_GET_INPUT_BITMASKS: c_uint = 51 | EXPERIMENTAL;
pub const ENV_GET_PREFERRED_HW_RENDER: c_uint = 56;

/// `enum retro_hw_context_type`: the two the host accepts.
pub const RETRO_HW_CONTEXT_OPENGL: c_int = 1;
pub const RETRO_HW_CONTEXT_OPENGL_CORE: c_int = 3;

/// The `data` a Core passes to `retro_video_refresh_t` when it drew the frame into the frontend's
/// framebuffer: `((void *)-1)`.
pub const RETRO_HW_FRAME_BUFFER_VALID: *const c_void = usize::MAX as *const c_void;

/// `struct retro_variable`: a Core option's key and value. In `SET_VARIABLES` the value is
/// `"Description; first|second|..."`, the first choice being the default.
#[repr(C)]
pub struct RetroVariable {
    pub key: *const c_char,
    pub value: *const c_char,
}

#[repr(C)]
pub struct RetroSystemInfo {
    pub library_name: *const c_char,
    pub library_version: *const c_char,
    pub valid_extensions: *const c_char,
    pub need_fullpath: bool,
    pub block_extract: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RetroGameGeometry {
    pub base_width: c_uint,
    pub base_height: c_uint,
    pub max_width: c_uint,
    pub max_height: c_uint,
    pub aspect_ratio: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RetroSystemTiming {
    pub fps: f64,
    pub sample_rate: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RetroSystemAvInfo {
    pub geometry: RetroGameGeometry,
    pub timing: RetroSystemTiming,
}

#[repr(C)]
pub struct RetroGameInfo {
    pub path: *const c_char,
    pub data: *const c_void,
    pub size: usize,
    pub meta: *const c_char,
}

/// `retro_log_printf_t`.
pub type LogPrintfFn = unsafe extern "C" fn(level: c_int, fmt: *const c_char, ...);

/// `struct retro_log_callback`.
#[repr(C)]
pub struct RetroLogCallback {
    pub log: LogPrintfFn,
}

unsafe extern "C" {
    /// The log callback every Core gets, in `log.c`.
    pub fn game37_core_log(level: c_int, fmt: *const c_char, ...);
}

pub type ProcAddress = Option<unsafe extern "C" fn()>;
pub type HwContextResetFn = unsafe extern "C" fn();
pub type HwGetCurrentFramebufferFn = unsafe extern "C" fn() -> usize;
pub type HwGetProcAddressFn = unsafe extern "C" fn(sym: *const c_char) -> ProcAddress;

/// `struct retro_hw_render_callback`: the Core fills in what it wants, the frontend its two
/// functions.
#[repr(C)]
pub struct RetroHwRenderCallback {
    pub context_type: c_int,
    pub context_reset: Option<HwContextResetFn>,
    pub get_current_framebuffer: Option<HwGetCurrentFramebufferFn>,
    pub get_proc_address: Option<HwGetProcAddressFn>,
    pub depth: bool,
    pub stencil: bool,
    pub bottom_left_origin: bool,
    pub version_major: c_uint,
    pub version_minor: c_uint,
    pub cache_context: bool,
    pub context_destroy: Option<HwContextResetFn>,
    pub debug_context: bool,
}

pub type EnvironmentFn = unsafe extern "C" fn(cmd: c_uint, data: *mut c_void) -> bool;
pub type VideoRefreshFn =
    unsafe extern "C" fn(data: *const c_void, width: c_uint, height: c_uint, pitch: usize);
pub type AudioSampleFn = unsafe extern "C" fn(left: i16, right: i16);
pub type AudioSampleBatchFn = unsafe extern "C" fn(data: *const i16, frames: usize) -> usize;
pub type InputPollFn = unsafe extern "C" fn();
pub type InputStateFn = unsafe extern "C" fn(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16;

/// The Core's entry points, copied out of the library. Valid only while that library stays loaded.
pub struct Api {
    pub set_environment: unsafe extern "C" fn(EnvironmentFn),
    pub set_video_refresh: unsafe extern "C" fn(VideoRefreshFn),
    pub set_audio_sample: unsafe extern "C" fn(AudioSampleFn),
    pub set_audio_sample_batch: unsafe extern "C" fn(AudioSampleBatchFn),
    pub set_input_poll: unsafe extern "C" fn(InputPollFn),
    pub set_input_state: unsafe extern "C" fn(InputStateFn),
    pub init: unsafe extern "C" fn(),
    pub deinit: unsafe extern "C" fn(),
    pub api_version: unsafe extern "C" fn() -> c_uint,
    pub get_system_info: unsafe extern "C" fn(*mut RetroSystemInfo),
    pub get_system_av_info: unsafe extern "C" fn(*mut RetroSystemAvInfo),
    pub reset: unsafe extern "C" fn(),
    pub run: unsafe extern "C" fn(),
    pub serialize_size: unsafe extern "C" fn() -> usize,
    pub serialize: unsafe extern "C" fn(*mut c_void, usize) -> bool,
    pub unserialize: unsafe extern "C" fn(*const c_void, usize) -> bool,
    pub load_game: unsafe extern "C" fn(*const RetroGameInfo) -> bool,
    pub unload_game: unsafe extern "C" fn(),
    pub get_memory_data: unsafe extern "C" fn(c_uint) -> *mut c_void,
    pub get_memory_size: unsafe extern "C" fn(c_uint) -> usize,
}

impl Api {
    /// Looks up every entry point, or names the first one the library lacks.
    ///
    /// # Safety
    /// `lib` must be a libretro Core, so that each symbol has the signature `libretro.h` gives it.
    pub unsafe fn load(lib: &Library) -> Result<Api, &'static str> {
        macro_rules! sym {
            ($name:literal) => {
                // SAFETY: the caller guarantees `lib` is a libretro Core, whose `$name` has the
                // type of the field it fills. The pointer is copied out; `Api`'s contract keeps the
                // library loaded while it is used.
                *unsafe { lib.get($name) }.map_err(|_| $name)?
            };
        }
        Ok(Api {
            set_environment: sym!("retro_set_environment"),
            set_video_refresh: sym!("retro_set_video_refresh"),
            set_audio_sample: sym!("retro_set_audio_sample"),
            set_audio_sample_batch: sym!("retro_set_audio_sample_batch"),
            set_input_poll: sym!("retro_set_input_poll"),
            set_input_state: sym!("retro_set_input_state"),
            init: sym!("retro_init"),
            deinit: sym!("retro_deinit"),
            api_version: sym!("retro_api_version"),
            get_system_info: sym!("retro_get_system_info"),
            get_system_av_info: sym!("retro_get_system_av_info"),
            reset: sym!("retro_reset"),
            run: sym!("retro_run"),
            serialize_size: sym!("retro_serialize_size"),
            serialize: sym!("retro_serialize"),
            unserialize: sym!("retro_unserialize"),
            load_game: sym!("retro_load_game"),
            unload_game: sym!("retro_unload_game"),
            get_memory_data: sym!("retro_get_memory_data"),
            get_memory_size: sym!("retro_get_memory_size"),
        })
    }
}
