//! A fake Core: entry points that are plain functions, started without a library, to check what
//! the host does around them. The callbacks it was handed and what it was asked to do are kept in
//! statics, so every test here holds [`serial`] and starts with [`reset`].

use std::ffi::{c_uint, c_void};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::ffi::{
    self, Api, AudioSampleBatchFn, EnvironmentFn, InputStateFn, RetroGameInfo, RetroSystemAvInfo,
    RetroSystemInfo,
};
use super::tests::serial;
use super::{Button, Claim, Core, CoreDirs, InputState};

/// The callbacks the host handed the fake Core.
static AUDIO_BATCH: Mutex<Option<AudioSampleBatchFn>> = Mutex::new(None);
static INPUT_STATE: Mutex<Option<InputStateFn>> = Mutex::new(None);
/// What `input_state` answered the fake Core.
static ANSWERS: Mutex<Vec<i16>> = Mutex::new(Vec::new());
/// What the fake Core was asked to do, in order.
static CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn lock<T>(m: &'static Mutex<T>) -> MutexGuard<'static, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Forgets what an earlier test's fake Core was handed and asked.
fn reset() {
    *lock(&AUDIO_BATCH) = None;
    *lock(&INPUT_STATE) = None;
    lock(&ANSWERS).clear();
    lock(&CALLS).clear();
    *lock(&ENVIRONMENT) = None;
    *lock(&QUIRKS_BACK) = None;
}

fn note(call: String) {
    lock(&CALLS).push(call);
}

fn calls() -> Vec<String> {
    lock(&CALLS).clone()
}

unsafe extern "C" fn set_any<T>(_: T) {}
unsafe extern "C" fn nothing() {}
unsafe extern "C" fn set_controller_port_device(port: c_uint, device: c_uint) {
    note(format!("port {port}: device {device}"));
}
unsafe extern "C" fn load_game_noted(_: *const RetroGameInfo) -> bool {
    note("load_game".into());
    true
}
unsafe extern "C" fn run_noted() {
    note("run".into());
}

static ENVIRONMENT: Mutex<Option<EnvironmentFn>> = Mutex::new(None);
/// The quirks the host wrote back to the fake Core.
static QUIRKS_BACK: Mutex<Option<u64>> = Mutex::new(None);

unsafe extern "C" fn set_environment(f: EnvironmentFn) {
    *lock(&ENVIRONMENT) = Some(f);
}
/// `retro_init` of a Core that, like Dolphin and Azahar, can't save or restore its state before
/// its first `retro_run`.
unsafe extern "C" fn init_must_initialize() {
    let Some(environment) = *lock(&ENVIRONMENT) else {
        return;
    };
    let mut quirks = ffi::RETRO_SERIALIZATION_QUIRK_MUST_INITIALIZE;
    // SAFETY: the host's environment callback, given the `uint64_t *` this command takes.
    if unsafe { environment(ffi::ENV_SET_SERIALIZATION_QUIRKS, (&raw mut quirks).cast()) } {
        *lock(&QUIRKS_BACK) = Some(quirks);
    }
}
/// `retro_init` of a Core that greets the player before any Game is loaded.
unsafe extern "C" fn init_with_a_banner() {
    let Some(environment) = *lock(&ENVIRONMENT) else {
        return;
    };
    let message = ffi::RetroMessage {
        msg: c"Core started".as_ptr(),
        frames: 60,
    };
    // SAFETY: the host's environment callback, given the `retro_message *` this command takes.
    unsafe { environment(ffi::ENV_SET_MESSAGE, (&raw const message).cast_mut().cast()) };
}
unsafe extern "C" fn load_game_fails(_: *const RetroGameInfo) -> bool {
    false
}
fn has_run() -> bool {
    calls().iter().any(|c| c == "run")
}
unsafe extern "C" fn serialize_size_once_run() -> usize {
    note("serialize_size".into());
    if has_run() { 4 } else { 0 }
}
unsafe extern "C" fn serialize_once_run(_: *mut c_void, _: usize) -> bool {
    note("serialize".into());
    has_run()
}
unsafe extern "C" fn unserialize_once_run(_: *const c_void, _: usize) -> bool {
    note("unserialize".into());
    has_run()
}
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
        set_environment,
        set_video_refresh: set_any,
        set_audio_sample: set_any,
        set_audio_sample_batch,
        set_input_poll: set_any,
        set_input_state,
        set_controller_port_device,
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

/// Dolphin's CPU thread may send audio after one frame is collected and before the next starts:
/// the next frame plays it rather than dropping it.
#[test]
fn audio_a_core_thread_sends_between_frames_comes_with_the_next_frame() {
    let _serial = serial();
    reset();
    let mut core = start(api());
    assert!(core.run_frame(&InputState::default()).audio.is_empty());
    let audio_batch = lock(&AUDIO_BATCH).expect("the host's audio callback");
    std::thread::spawn(move || {
        let samples = [3i16, -3];
        // SAFETY: the host's callback; `samples` holds 1 stereo frame.
        unsafe { audio_batch(samples.as_ptr(), 1) };
    })
    .join()
    .unwrap();
    assert_eq!(core.run_frame(&InputState::default()).audio, [3, -3]);
    assert!(
        core.run_frame(&InputState::default()).audio.is_empty(),
        "played once"
    );
}

/// A message from before the Game started loading isn't the reason loading it failed.
#[test]
fn a_message_sent_before_loading_is_not_given_as_why_loading_failed() {
    let _serial = serial();
    reset();
    let dirs = CoreDirs {
        system: "/nonexistent/system".into(),
        save: "/nonexistent/saves".into(),
    };
    let claim = Claim::take().expect("no other Core is loaded");
    let api = Api {
        init: init_with_a_banner,
        load_game: load_game_fails,
        ..api()
    };
    let Err(e) = Core::start(claim, api, None, Path::new("/nonexistent/game"), dirs, &[], None) else {
        panic!("the fake Core refuses every Game");
    };
    assert_eq!(e.core_message(), None, "{e}");
}

#[test]
fn every_port_gets_a_retropad_after_the_game_loads_and_before_the_first_frame() {
    let _serial = serial();
    reset();
    let mut core = start(Api {
        load_game: load_game_noted,
        run: run_noted,
        ..api()
    });
    core.run_frame(&InputState::default());
    let joypad = ffi::RETRO_DEVICE_JOYPAD;
    let expected = [
        "load_game".to_owned(),
        format!("port 0: device {joypad}"),
        format!("port 1: device {joypad}"),
        format!("port 2: device {joypad}"),
        format!("port 3: device {joypad}"),
        "run".to_owned(),
    ];
    assert_eq!(calls(), expected);
}

#[test]
fn a_core_that_must_run_first_runs_one_frame_before_its_state_is_restored() {
    let _serial = serial();
    reset();
    let mut core = start(Api {
        init: init_must_initialize,
        run: run_noted,
        unserialize: unserialize_once_run,
        ..api()
    });
    lock(&CALLS).clear();
    assert_eq!(
        *lock(&QUIRKS_BACK),
        Some(ffi::RETRO_SERIALIZATION_QUIRK_MUST_INITIALIZE),
        "the host supports the quirk"
    );
    core.unserialize(b"save").expect("restored after the first frame");
    assert_eq!(
        core.frame(),
        &super::Frame::default(),
        "the first frame isn't shown"
    );
    core.unserialize(b"save").expect("restored again");
    assert_eq!(
        calls(),
        ["run", "unserialize", "unserialize"],
        "one first frame only"
    );
}

#[test]
fn a_core_that_must_run_first_runs_one_frame_before_its_state_is_saved() {
    let _serial = serial();
    reset();
    let mut core = start(Api {
        init: init_must_initialize,
        run: run_noted,
        serialize_size: serialize_size_once_run,
        serialize: serialize_once_run,
        ..api()
    });
    lock(&CALLS).clear();
    assert_eq!(core.serialize().expect("saved after the first frame").len(), 4);
    assert_eq!(calls(), ["run", "serialize_size", "serialize"]);
}

#[test]
fn other_cores_restore_their_state_without_a_first_frame() {
    let _serial = serial();
    reset();
    let mut core = start(Api {
        run: run_noted,
        unserialize: unserialize_once_run,
        ..api()
    });
    lock(&CALLS).clear();
    assert!(
        core.unserialize(b"save").is_err(),
        "this fake Core refuses before a frame"
    );
    assert_eq!(calls(), ["unserialize"], "no frame run for it");
}
