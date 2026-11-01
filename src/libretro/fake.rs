//! A fake Core: entry points that are plain functions, started without a library, to check what
//! the host does around them. The callbacks it was handed and what it was asked to do are kept in
//! statics, so every test here holds [`serial`] and starts with [`reset`].

use std::ffi::{c_uint, c_void};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::ffi::{
    self, Api, AudioSampleBatchFn, InputStateFn, RetroGameInfo, RetroSystemAvInfo, RetroSystemInfo,
};
use super::tests::serial;
use super::{Button, Claim, Core, CoreDirs, InputState};

/// The callbacks the host handed the fake Core.
static AUDIO_BATCH: Mutex<Option<AudioSampleBatchFn>> = Mutex::new(None);
static INPUT_STATE: Mutex<Option<InputStateFn>> = Mutex::new(None);
/// What `input_state` answered the fake Core.
static ANSWERS: Mutex<Vec<i16>> = Mutex::new(Vec::new());

fn lock<T>(m: &'static Mutex<T>) -> MutexGuard<'static, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Forgets what an earlier test's fake Core was handed and asked.
fn reset() {
    *lock(&AUDIO_BATCH) = None;
    *lock(&INPUT_STATE) = None;
    lock(&ANSWERS).clear();
}

unsafe extern "C" fn set_any<T>(_: T) {}
unsafe extern "C" fn nothing() {}
unsafe extern "C" fn api_version() -> c_uint {
    ffi::RETRO_API_VERSION
}
unsafe extern "C" fn set_audio_sample_batch(f: AudioSampleBatchFn) {
    *lock(&AUDIO_BATCH) = Some(f);
}
unsafe extern "C" fn set_input_state(f: InputStateFn) {
    *lock(&INPUT_STATE) = Some(f);
}
/// The Game is opened by path, so the host reads no file.
unsafe extern "C" fn system_info(info: *mut RetroSystemInfo) {
    // SAFETY: the host passes a valid, writable `retro_system_info`.
    unsafe { (*info).need_fullpath = true };
}
unsafe extern "C" fn av_info(av: *mut RetroSystemAvInfo) {
    // SAFETY: the host passes a valid, writable `retro_system_av_info`.
    let av = unsafe { &mut *av };
    (av.geometry.base_width, av.geometry.base_height) = (320, 240);
    (av.timing.fps, av.timing.sample_rate) = (60.0, 48000.0);
}
unsafe extern "C" fn load_game(_: *const RetroGameInfo) -> bool {
    true
}
unsafe extern "C" fn serialize_size() -> usize {
    0
}
unsafe extern "C" fn serialize(_: *mut c_void, _: usize) -> bool {
    false
}
unsafe extern "C" fn unserialize(_: *const c_void, _: usize) -> bool {
    false
}
unsafe extern "C" fn memory_data(_: c_uint) -> *mut c_void {
    std::ptr::null_mut()
}
unsafe extern "C" fn memory_size(_: c_uint) -> usize {
    0
}

/// `retro_run` of a Core that, like Dolphin, reads the input and sends its audio from a thread of
/// its own: here one it starts and waits for.
unsafe extern "C" fn run_on_another_thread() {
    let (input_state, audio_batch) = (*lock(&INPUT_STATE), *lock(&AUDIO_BATCH));
    let (Some(input_state), Some(audio_batch)) = (input_state, audio_batch) else {
        return;
    };
    let answer = std::thread::spawn(move || {
        let samples = [7i16, -7, 8, -8];
        // SAFETY: the host's callbacks; `samples` holds 2 stereo frames.
        unsafe {
            audio_batch(samples.as_ptr(), 2);
            input_state(0, ffi::RETRO_DEVICE_JOYPAD, 0, Button::A as c_uint)
        }
    })
    .join()
    .unwrap_or(-1);
    lock(&ANSWERS).push(answer);
}

/// A fake Core that does nothing: the base each test changes some entry points of.
fn api() -> Api {
    Api {
        set_environment: set_any,
        set_video_refresh: set_any,
        set_audio_sample: set_any,
        set_audio_sample_batch,
        set_input_poll: set_any,
        set_input_state,
        init: nothing,
        deinit: nothing,
        api_version,
        get_system_info: system_info,
        get_system_av_info: av_info,
        reset: nothing,
        run: nothing,
        serialize_size,
        serialize,
        unserialize,
        load_game,
        unload_game: nothing,
        get_memory_data: memory_data,
        get_memory_size: memory_size,
    }
}

/// Starts a Game on the fake Core `api`, as `Core::load` does once a library is open.
fn start(api: Api) -> Core {
    let dirs = CoreDirs {
        system: "/nonexistent/system".into(),
        save: "/nonexistent/saves".into(),
    };
    let claim = Claim::take().expect("no other Core is loaded");
    Core::start(claim, api, None, Path::new("/nonexistent/game"), dirs, &[], None)
        .unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn a_core_reading_input_and_sending_audio_on_its_own_thread_gets_both_through() {
    let _serial = serial();
    reset();
    let mut core = start(Api {
        run: run_on_another_thread,
        ..api()
    });
    let mut input = InputState::default();
    input.set(0, Button::A, true);
    let audio = core.run_frame(&input).audio.to_vec();
    assert_eq!(*lock(&ANSWERS), [1], "A, as read on the Core's thread");
    assert_eq!(audio, [7, -7, 8, -8], "the audio sent on the Core's thread");
    input.set(0, Button::A, false);
    core.run_frame(&input);
    assert_eq!(*lock(&ANSWERS), [1, 0], "A let go");
}
