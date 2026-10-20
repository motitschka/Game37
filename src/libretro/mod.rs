//! The libretro Core host: loads a Core (`*_libretro.so`) and a Game, runs frames and captures
//! their video and audio. Headless: windows, audio devices and input devices are the caller's.
//!
//! libretro callbacks carry no context, so the running Core's state lives in a thread-local slot
//! (see `callbacks`). Only one [`Core`] exists at a time in the process, and it stays on the thread
//! that loaded it.

mod callbacks;
mod ffi;
mod hw;
#[cfg(test)]
pub(crate) mod tests;

use std::ffi::{CString, c_int};
use std::fmt;
use std::marker::PhantomData;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use libloading::Library;
use libloading::os::unix;

use callbacks::State;
use ffi::{Api, RetroGameGeometry, RetroGameInfo, RetroSystemAvInfo, RetroSystemInfo};
pub use ffi::{HwGetProcAddressFn, ProcAddress};
use hw::Hw;
pub use hw::{GlHost, GlRequest};

/// Ports a Core can read input from.
pub const MAX_PORTS: usize = 4;

/// `RTLD_NOW | RTLD_LOCAL`, plus glibc's `RTLD_NODELETE`: a Core's code stays mapped after it is
/// unloaded. Cores may leave threads behind (bsnes's OpenMP workers in libgomp) that crash the
/// process once the code they run is unmapped. Loading the same Core again reuses the mapping, and
/// libretro.h requires Cores to cope with that.
const DLOPEN_FLAGS: c_int = unix::RTLD_NOW | unix::RTLD_LOCAL | RTLD_NODELETE;
const RTLD_NODELETE: c_int = 0x1000;

/// Set while a [`Core`] exists anywhere in the process.
static LOADED: AtomicBool = AtomicBool::new(false);

/// Directories handed to the Core: where it finds BIOS files, and where it keeps its own saves.
#[derive(Clone, Debug)]
pub struct CoreDirs {
    pub system: PathBuf,
    pub save: PathBuf,
}

#[derive(Debug)]
pub enum CoreError {
    /// Another Core is still loaded; drop it first.
    AlreadyLoaded,
    Open(PathBuf, libloading::Error),
    /// The library lacks this libretro entry point.
    NotACore(PathBuf, &'static str),
    ApiVersion(PathBuf, u32),
    /// The path contains a NUL byte and can't be handed to a Core.
    BadPath(PathBuf),
    ReadGame(PathBuf, std::io::Error),
    LoadGame(PathBuf),
    Serialize,
    Unserialize,
    /// The Core asked for OpenGL rendering, and it couldn't be set up.
    Gl(String),
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::AlreadyLoaded => write!(f, "Another Core is already running"),
            CoreError::Open(path, e) => write!(f, "Can't open the Core {}: {e}", path.display()),
            CoreError::NotACore(path, symbol) => {
                write!(f, "{} is not a libretro Core (no {symbol})", path.display())
            }
            CoreError::ApiVersion(path, v) => {
                write!(f, "{} uses libretro API version {v}, not 1", path.display())
            }
            CoreError::BadPath(path) => write!(f, "Can't pass {} to a Core", path.display()),
            CoreError::ReadGame(path, e) => write!(f, "Can't read {}: {e}", path.display()),
            CoreError::LoadGame(path) => write!(f, "The Core couldn't load {}", path.display()),
            CoreError::Serialize => write!(f, "The Core couldn't save its state"),
            CoreError::Unserialize => write!(f, "The Core couldn't restore the state"),
            CoreError::Gl(e) => write!(f, "Can't start OpenGL for the Core: {e}"),
        }
    }
}

impl std::error::Error for CoreError {}

/// How the Core lays out the pixels of a video frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// libretro's `0RGB1555`, the format until the Core asks for another.
    #[default]
    Xrgb1555,
    Xrgb8888,
    Rgb565,
    /// Bytes R, G, B, A in memory (SDL's `RGBA32`): a frame a Core drew with OpenGL, as read back
    /// from its framebuffer. Never a Core's own format. A is whatever the Core left in its
    /// framebuffer, not the picture's opacity, so it is ignored like the X of the others.
    Rgba32,
}

impl PixelFormat {
    fn from_raw(raw: c_int) -> Option<PixelFormat> {
        match raw {
            ffi::RETRO_PIXEL_FORMAT_0RGB1555 => Some(PixelFormat::Xrgb1555),
            ffi::RETRO_PIXEL_FORMAT_XRGB8888 => Some(PixelFormat::Xrgb8888),
            ffi::RETRO_PIXEL_FORMAT_RGB565 => Some(PixelFormat::Rgb565),
            _ => None,
        }
    }

    pub fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Xrgb1555 | PixelFormat::Rgb565 => 2,
            PixelFormat::Xrgb8888 | PixelFormat::Rgba32 => 4,
        }
    }
}

/// The Core's video geometry and timing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AvInfo {
    pub base_width: u32,
    pub base_height: u32,
    pub max_width: u32,
    pub max_height: u32,
    /// Display aspect ratio; `base_width / base_height` when the Core gives none.
    pub aspect_ratio: f32,
    pub fps: f64,
    pub sample_rate: f64,
}

impl AvInfo {
    fn from_raw(raw: &RetroSystemAvInfo) -> AvInfo {
        let mut info = AvInfo {
            max_width: raw.geometry.max_width,
            max_height: raw.geometry.max_height,
            fps: raw.timing.fps,
            sample_rate: raw.timing.sample_rate,
            ..AvInfo::default()
        };
        info.set_geometry(&raw.geometry);
        info
    }

    /// `SET_GEOMETRY`: the base size and aspect ratio change, the maximum size doesn't.
    fn set_geometry(&mut self, g: &RetroGameGeometry) {
        self.base_width = g.base_width;
        self.base_height = g.base_height;
        self.aspect_ratio = if g.aspect_ratio > 0.0 {
            g.aspect_ratio
        } else {
            g.base_width as f32 / g.base_height.max(1) as f32
        };
    }
}

/// One video frame, rows packed: `pitch` is `width × bytes per pixel`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pitch: usize,
    pub format: PixelFormat,
    pub data: Vec<u8>,
}

/// What one [`Core::run_frame`] produced.
#[derive(Debug)]
pub struct FrameOutput<'a> {
    /// The last frame the Core showed; empty until its first.
    pub video: &'a Frame,
    /// The Core showed no new frame this run, so `video` is the previous one.
    pub duped: bool,
    /// Interleaved stereo samples (left, right, left, …) produced during this run.
    pub audio: &'a [i16],
}

/// The 16 RetroPad buttons, numbered as `RETRO_DEVICE_ID_JOYPAD_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Button {
    B = 0,
    Y,
    Select,
    Start,
    Up,
    Down,
    Left,
    Right,
    A,
    X,
    L,
    R,
    L2,
    R2,
    L3,
    R3,
}

/// A RetroPad's analog sticks, numbered as `RETRO_DEVICE_INDEX_ANALOG_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Stick {
    Left = 0,
    Right,
}

/// Where the pointer (`RETRO_DEVICE_POINTER`) is on the Core's frame: `x` and `y` run from
/// -0x7fff at its left and top edges to 0x7fff at its right and bottom ones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pointer {
    pub x: i16,
    pub y: i16,
    pub pressed: bool,
}

/// What the Core reads: the pressed RetroPad buttons of each port, one bit per [`Button`], each
/// port's analog sticks, and port 0's pointer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputState {
    ports: [u16; MAX_PORTS],
    /// Per port, per [`Stick`]: x and y, from -0x7fff (left, up) to 0x7fff (right, down).
    sticks: [[(i16, i16); 2]; MAX_PORTS],
    /// `None` while the pointer is off the frame.
    pointer: Option<Pointer>,
}

impl InputState {
    /// Panics when `port` is not below [`MAX_PORTS`].
    pub fn set_stick(&mut self, port: usize, stick: Stick, x: i16, y: i16) {
        self.sticks[port][stick as usize] = (x, y);
    }

    /// The stick's x and y; centred for a port beyond [`MAX_PORTS`].
    pub fn stick(&self, port: usize, stick: Stick) -> (i16, i16) {
        self.sticks.get(port).map_or((0, 0), |s| s[stick as usize])
    }

    pub fn set_pointer(&mut self, pointer: Option<Pointer>) {
        self.pointer = pointer;
    }

    pub fn pointer(&self) -> Option<Pointer> {
        self.pointer
    }

    /// Panics when `port` is not below [`MAX_PORTS`].
    pub fn set(&mut self, port: usize, button: Button, pressed: bool) {
        let bit = 1 << button as u16;
        if pressed {
            self.ports[port] |= bit;
        } else {
            self.ports[port] &= !bit;
        }
    }

    pub fn is_pressed(&self, port: usize, button: Button) -> bool {
        self.mask(port) & 1 << button as u16 != 0
    }

    /// The port's buttons as `RETRO_DEVICE_ID_JOYPAD_MASK` reports them; 0 for a port beyond
    /// [`MAX_PORTS`].
    pub fn mask(&self, port: usize) -> u16 {
        self.ports.get(port).copied().unwrap_or(0)
    }
}

/// Holds the process-wide one-Core claim; releasing it is the last thing a Core does.
struct Claim;

impl Claim {
    fn take() -> Option<Claim> {
        LOADED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Claim)
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        LOADED.store(false, Ordering::Release);
    }
}

/// A loaded Core running one Game. Not `Send`: it stays on the thread that loaded it, where its
/// callbacks find their state.
pub struct Core {
    api: Api,
    /// `retro_init` ran and `retro_deinit` hasn't.
    initialised: bool,
    game_loaded: bool,
    video: Frame,
    /// The Core's OpenGL rendering, when it asked for it.
    hw: Option<Hw>,
    duped: bool,
    audio: Vec<i16>,
    game_path: CString,
    /// The Game's contents when the Core takes them from memory; it may read them until unload.
    game_data: Option<Vec<u8>>,
    /// Taken and dropped last in `Drop`, after the Core has shut down.
    lib: Option<Library>,
    _claim: Claim,
    _not_send: PhantomData<*const ()>,
}

fn c_path(path: &Path) -> Result<CString, CoreError> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| CoreError::BadPath(path.to_path_buf()))
}

impl Core {
    /// Loads the Core at `core_path` and starts `game_path` on it. `options` are Core option
    /// values, `(key, value)`, the Core gets ahead of its own defaults. With a `gl` host, a Core may
    /// render with OpenGL (`SET_HW_RENDER`); without one it is refused.
    pub fn load(
        core_path: &Path,
        game_path: &Path,
        dirs: CoreDirs,
        options: &[(&str, &str)],
        gl: Option<Box<dyn GlHost>>,
    ) -> Result<Core, CoreError> {
        let claim = Claim::take().ok_or(CoreError::AlreadyLoaded)?;
        let game_cpath = c_path(game_path)?;
        let mut state = State::new(c_path(&dirs.system)?, c_path(&dirs.save)?);
        state.overrides = callbacks::option_overrides(options);
        state.gl_host = gl;

        // SAFETY: loading a shared library runs its initialisers. A Core is a plain C library and
        // its initialisers only set up its own statics; this is the trust any frontend places in
        // the Core it was given.
        let lib: Library = unsafe { unix::Library::open(Some(core_path), DLOPEN_FLAGS) }
            .map_err(|e| CoreError::Open(core_path.to_path_buf(), e))?
            .into();
        // SAFETY: the library is treated as a libretro Core, and `api` is stored next to `lib` in the
        // `Core`, which unloads `lib` only after its last call through `api`.
        let api = unsafe { Api::load(&lib) }.map_err(|s| CoreError::NotACore(core_path.to_path_buf(), s))?;
        // SAFETY: `retro_api_version` takes no arguments and may be called before `retro_init`.
        let version = unsafe { (api.api_version)() };
        if version != ffi::RETRO_API_VERSION {
            return Err(CoreError::ApiVersion(core_path.to_path_buf(), version));
        }

        callbacks::install(state);
        // SAFETY: the callbacks have the signatures libretro.h declares, never unwind, and find
        // their state in the slot installed above, which stays until `Drop` after `retro_deinit`.
        // libretro.h wants the environment before `retro_init` and the rest before the first
        // `retro_run`; some Cores (Mesen) crash if those come before `retro_init`, so they follow it,
        // as in RetroArch.
        unsafe {
            (api.set_environment)(callbacks::environment);
            (api.init)();
            (api.set_video_refresh)(callbacks::video_refresh);
            (api.set_audio_sample)(callbacks::audio_sample);
            (api.set_audio_sample_batch)(callbacks::audio_sample_batch);
            (api.set_input_poll)(callbacks::input_poll);
            (api.set_input_state)(callbacks::input_state);
        }
        // From here on, `Drop` shuts the Core down on any error.
        let mut core = Core {
            api,
            initialised: true,
            game_loaded: false,
            video: Frame::default(),
            hw: None,
            duped: false,
            audio: Vec::new(),
            game_path: game_cpath,
            game_data: None,
            lib: Some(lib),
            _claim: claim,
            _not_send: PhantomData,
        };

        let mut sys = RetroSystemInfo {
            library_name: std::ptr::null(),
            library_version: std::ptr::null(),
            valid_extensions: std::ptr::null(),
            need_fullpath: false,
            block_extract: false,
        };
        // SAFETY: `sys` is a valid, writable `retro_system_info` for the duration of the call.
        unsafe { (core.api.get_system_info)(&mut sys) };
        if !sys.need_fullpath {
            let data =
                std::fs::read(game_path).map_err(|e| CoreError::ReadGame(game_path.to_path_buf(), e))?;
            core.game_data = Some(data);
        }
        let (data, size) = match &core.game_data {
            Some(d) => (d.as_ptr().cast(), d.len()),
            None => (std::ptr::null(), 0),
        };
        let info = RetroGameInfo {
            path: core.game_path.as_ptr(),
            data,
            size,
            meta: std::ptr::null(),
        };
        // SAFETY: `info` points at the path and data owned by `core`, which keeps both until after
        // `retro_unload_game`; the heap buffers don't move when `core` does.
        if !unsafe { (core.api.load_game)(&info) } {
            return Err(CoreError::LoadGame(game_path.to_path_buf()));
        }
        core.game_loaded = true;

        let mut av = RetroSystemAvInfo::default();
        // SAFETY: `av` is a valid, writable `retro_system_av_info` for the duration of the call.
        unsafe { (core.api.get_system_av_info)(&mut av) };
        let info = AvInfo::from_raw(&av);
        let request = callbacks::with(|s| {
            s.av_info = info;
            s.hw
        })
        .flatten();
        if let Some(request) = request {
            let (w, h) = hw::fbo_size(&info);
            // Should its FBO fail, the Core shuts down with its own context current.
            let hw = Hw::start(request, w, h, || core.shut_down()).map_err(CoreError::Gl)?;
            core.hw = Some(hw);
        }
        Ok(core)
    }

    pub fn av_info(&self) -> AvInfo {
        callbacks::with(|s| s.av_info).expect("a loaded Core's state is installed")
    }

    /// Runs the Core for one frame with `input` held.
    pub fn run_frame(&mut self, input: &InputState) -> FrameOutput<'_> {
        callbacks::with(|s| {
            s.input = *input;
            s.audio.clear();
        });
        // SAFETY: the Core is loaded with a Game and its callbacks' state is installed; a Core
        // rendering with OpenGL runs with its context current, and its frame is read back.
        let run = || unsafe { (self.api.run)() };
        match &mut self.hw {
            Some(hw) => hw.run(run),
            None => run(),
        }
        callbacks::with(|s| {
            self.duped = !s.fresh;
            if s.fresh {
                std::mem::swap(&mut self.video, &mut s.frame);
                s.fresh = false;
            }
            std::mem::swap(&mut self.audio, &mut s.audio);
        });
        FrameOutput {
            video: &self.video,
            duped: self.duped,
            audio: &self.audio,
        }
    }

    /// The input the Core was last given.
    #[cfg(test)]
    pub fn last_input(&self) -> InputState {
        callbacks::with(|s| s.input).unwrap_or_default()
    }

    /// Each input query the Core made, `(port, device, index, id)`, and the last answer to it.
    #[cfg(test)]
    pub fn input_queries(&self) -> std::collections::HashMap<(u32, u32, u32, u32), i16> {
        callbacks::with(|s| s.queries.clone()).unwrap_or_default()
    }

    /// The last video frame the Core produced.
    pub fn frame(&self) -> &Frame {
        &self.video
    }

    /// The Core's state, for [`Core::unserialize`] later.
    pub fn serialize(&mut self) -> Result<Vec<u8>, CoreError> {
        // SAFETY: the Core is loaded with a Game.
        let size = unsafe { (self.api.serialize_size)() };
        if size == 0 {
            return Err(CoreError::Serialize);
        }
        let mut buf = vec![0u8; size];
        // SAFETY: `buf` is writable for the `size` bytes the Core asked for.
        if unsafe { (self.api.serialize)(buf.as_mut_ptr().cast(), size) } {
            Ok(buf)
        } else {
            Err(CoreError::Serialize)
        }
    }

    pub fn unserialize(&mut self, state: &[u8]) -> Result<(), CoreError> {
        // SAFETY: `state` is readable for its whole length during the call.
        if unsafe { (self.api.unserialize)(state.as_ptr().cast(), state.len()) } {
            Ok(())
        } else {
            Err(CoreError::Unserialize)
        }
    }

    pub fn reset(&mut self) {
        // SAFETY: the Core is loaded with a Game.
        unsafe { (self.api.reset)() }
    }

    /// The Game's battery-backed save memory, which the Core owns; `None` if it has none.
    pub fn save_ram(&mut self) -> Option<&mut [u8]> {
        // SAFETY: the Core is loaded with a Game; these calls only report a buffer.
        let (data, size) = unsafe {
            (
                (self.api.get_memory_data)(ffi::RETRO_MEMORY_SAVE_RAM),
                (self.api.get_memory_size)(ffi::RETRO_MEMORY_SAVE_RAM),
            )
        };
        if data.is_null() || size == 0 {
            return None;
        }
        // SAFETY: libretro guarantees the Core's save memory is `size` bytes at `data` and stays
        // there until `retro_unload_game`. The `&mut self` borrow ends before any other call into
        // the Core, so nothing else touches the memory while the slice lives.
        Some(unsafe { std::slice::from_raw_parts_mut(data.cast::<u8>(), size) })
    }
}

impl Core {
    /// Unloads the Game and deinitialises the Core, once.
    fn shut_down(&mut self) {
        if !self.initialised {
            return;
        }
        // SAFETY: `retro_init` ran in `load` and `retro_deinit` hasn't, and `retro_load_game`
        // succeeded when `game_loaded` is set; libretro.h orders unload, then deinit. The library
        // is still loaded.
        unsafe {
            if self.game_loaded {
                (self.api.unload_game)();
            }
            (self.api.deinit)();
        }
        (self.game_loaded, self.initialised) = (false, false);
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        // An OpenGL Core gets `context_destroy` first, and shuts down with its context current.
        match self.hw.take() {
            Some(hw) => hw.stop(|| self.shut_down()),
            None => self.shut_down(),
        }
        // The directory strings the Core was given stay alive until it has shut down.
        callbacks::remove();
        drop(self.lib.take());
    }
}
