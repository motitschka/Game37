//! Real Cores running the free 240p/144p/160p Test Suite Games from the local test data, which is
//! never committed. Run with `cargo test -- --ignored libretro`.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use tempfile::TempDir;

use super::{Core, CoreDirs, CoreError, Frame, InputState, PixelFormat};
use crate::systems;

const TESTDATA: &str = "/home/matei/Game37/.testdata";

struct Case {
    system: &'static str,
    core: &'static str,
    game: &'static str,
    /// Frame sizes the Core may show at this point of the Game.
    sizes: &'static [(u32, u32)],
}

const CASES: [Case; 5] = [
    Case {
        system: "nes",
        core: "mesen_libretro.so",
        game: "240p Test Suite (World).nes",
        sizes: &[(256, 240), (256, 224)],
    },
    Case {
        system: "snes",
        core: "bsnes_libretro.so",
        game: "240p Test Suite (World).sfc",
        sizes: &[
            (256, 224),
            (256, 239),
            (512, 224),
            (512, 448),
            (512, 239),
            (512, 478),
        ],
    },
    Case {
        system: "gb",
        core: "sameboy_libretro.so",
        game: "144p Test Suite (World).gb",
        sizes: &[(160, 144)],
    },
    Case {
        system: "gbc",
        core: "sameboy_libretro.so",
        game: "144p Test Suite (World).gbc",
        sizes: &[(160, 144)],
    },
    Case {
        system: "gba",
        core: "mgba_libretro.so",
        game: "160p Test Suite (World).gba",
        sizes: &[(240, 160)],
    },
];

/// One Core per process at a time: tests that load one take turns.
pub(crate) fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn paths(case: &Case) -> (PathBuf, PathBuf) {
    assert!(
        systems::by_id(case.system).is_some(),
        "{} is a System",
        case.system
    );
    let data = Path::new(TESTDATA);
    (
        data.join("data/cores").join(case.core),
        data.join("games").join(case.system).join(case.game),
    )
}

fn load(case: &Case, dirs: &TempDir) -> Result<Core, CoreError> {
    let (core, game) = paths(case);
    let dirs = CoreDirs {
        system: dirs.path().join("system"),
        save: dirs.path().join("saves"),
    };
    Core::load(&core, &game, dirs, &[], None)
}

fn run(core: &mut Core, frames: usize) -> (Frame, usize) {
    let input = InputState::default();
    let mut samples = 0;
    let mut last = Frame::default();
    for _ in 0..frames {
        samples += core.run_frame(&input).audio.len();
        last = core.frame().clone();
    }
    (last, samples)
}

fn distinct_colours(frame: &Frame) -> usize {
    let bpp = frame.format.bytes_per_pixel();
    let mut colours = HashSet::new();
    for row in frame.data.chunks(frame.pitch) {
        for px in row.chunks(bpp) {
            colours.insert(match frame.format {
                PixelFormat::Xrgb8888 => u32::from_le_bytes(px.try_into().unwrap()) & 0x00FF_FFFF,
                PixelFormat::Xrgb1555 => u32::from(u16::from_le_bytes(px.try_into().unwrap()) & 0x7FFF),
                PixelFormat::Rgb565 => u32::from(u16::from_le_bytes(px.try_into().unwrap())),
                PixelFormat::Rgba32 => u32::from_le_bytes(px.try_into().unwrap()) & 0x00FF_FFFF,
            });
        }
    }
    colours.len()
}

fn hash(frame: &Frame) -> u64 {
    let mut h = DefaultHasher::new();
    frame.hash(&mut h);
    h.finish()
}

#[test]
#[ignore = "needs /home/matei/Game37/.testdata"]
fn libretro_cores_run_test_suites_with_video_and_audio() {
    let _serial = serial();
    for case in &CASES {
        let dirs = TempDir::new().unwrap();
        let mut core = load(case, &dirs).unwrap_or_else(|e| panic!("{}: {e}", case.game));
        let (frame, samples) = run(&mut core, 300);
        let av = core.av_info();

        let size = (frame.width, frame.height);
        assert!(case.sizes.contains(&size), "{}: frame is {size:?}", case.game);
        assert_eq!(
            frame.data.len(),
            frame.pitch * frame.height as usize,
            "{}",
            case.game
        );
        assert!(
            distinct_colours(&frame) > 1,
            "{}: frame 300 is a single colour",
            case.game
        );

        let expected = av.sample_rate * 300.0 / av.fps;
        let got = (samples / 2) as f64;
        assert!(
            (got - expected).abs() <= expected * 0.1,
            "{}: {got} stereo frames of audio, expected about {expected} ({av:?})",
            case.game
        );
    }
}

#[test]
#[ignore = "needs /home/matei/Game37/.testdata"]
fn libretro_save_states_round_trip() {
    let _serial = serial();
    for case in &CASES {
        let dirs = TempDir::new().unwrap();
        let mut core = load(case, &dirs).unwrap_or_else(|e| panic!("{}: {e}", case.game));
        run(&mut core, 100);
        let state = core.serialize().unwrap_or_else(|e| panic!("{}: {e}", case.game));
        let (first, _) = run(&mut core, 30);
        core.unserialize(&state)
            .unwrap_or_else(|e| panic!("{}: {e}", case.game));
        let (second, _) = run(&mut core, 30);
        assert!(first.width > 0, "{}: no frame", case.game);
        assert_eq!(
            hash(&first),
            hash(&second),
            "{}: replay after restoring differs",
            case.game
        );
    }
}

#[test]
#[ignore = "needs /home/matei/Game37/.testdata"]
fn libretro_only_one_core_at_a_time() {
    let _serial = serial();
    for case in &CASES {
        let dirs = TempDir::new().unwrap();
        let first = load(case, &dirs).unwrap_or_else(|e| panic!("{}: {e}", case.game));
        assert!(
            matches!(load(case, &dirs), Err(CoreError::AlreadyLoaded)),
            "{}",
            case.game
        );
        let elsewhere = std::thread::scope(|s| s.spawn(|| load(case, &dirs).err()).join().unwrap());
        assert!(
            matches!(elsewhere, Some(CoreError::AlreadyLoaded)),
            "{}: other thread",
            case.game
        );
        drop(first);

        let mut again = load(case, &dirs).unwrap_or_else(|e| panic!("{} reloaded: {e}", case.game));
        let (frame, _) = run(&mut again, 60);
        assert!(frame.width > 0, "{}: the reloaded Core shows nothing", case.game);
    }
}

/// melonDS DS with its hybrid layout (FR-DS-1) reads the pointer: a press at the centre of the
/// small touch screen reaches it as `RETRO_DEVICE_POINTER` coordinates (FR-DS-3), and R3 swaps
/// the screens (FR-DS-2). Run with `cargo test -- --ignored ds`.
#[test]
#[ignore = "needs /home/matei/Game37/.testdata"]
fn ds_touch_reaches_melonds_ds_and_r3_swaps_the_screens() {
    use super::{Button, Pointer, ffi};
    use crate::cores::MELONDS_DS;

    let _serial = serial();
    let dirs = TempDir::new().unwrap();
    let data = Path::new(TESTDATA);
    let mut options = MELONDS_DS.options.to_vec();
    // hbmenu crashes on melonDS DS's JIT ("undefined THUMB9 instruction") and its SD card
    // driver doesn't fit; without both, it boots.
    options.push(("melonds_homebrew_sdcard", "disabled"));
    options.push(("melonds_jit_enable", "disabled"));
    let mut core = Core::load(
        &data.join("data/cores").join(MELONDS_DS.file),
        &data.join("games/nds/Homebrew Menu (World).nds"),
        CoreDirs {
            system: dirs.path().join("system"),
            save: dirs.path().join("saves"),
        },
        &options,
        None,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let (top, _) = run(&mut core, 120);
    eprintln!("frame {}x{}, {:?}", top.width, top.height, core.av_info());
    assert!(distinct_colours(&top) > 2, "hbmenu shows something");
    // Hybrid, scale 2: the large screen (512×384) on the left, the small one (256×192) below on
    // its right.
    assert_eq!(
        (top.width, top.height),
        (768, 384),
        "one large and one small screen"
    );

    let centre = Pointer {
        x: (640.0 / 768.0 * 2.0 * 32767.0 - 32767.0_f64).round() as i16,
        y: (288.0 / 384.0 * 2.0 * 32767.0 - 32767.0_f64).round() as i16,
        pressed: true,
    };
    let mut input = InputState::default();
    input.set_pointer(Some(centre));
    for _ in 0..10 {
        core.run_frame(&input);
    }
    let queries = core.input_queries();
    let asked = |id| queries.get(&(0, ffi::RETRO_DEVICE_POINTER, 0, id)).copied();
    let mut devices: Vec<_> = queries.keys().map(|k| k.1).collect();
    devices.sort();
    devices.dedup();
    eprintln!("devices queried: {devices:?}");
    assert_eq!(asked(ffi::RETRO_DEVICE_ID_POINTER_X), Some(centre.x));
    assert_eq!(asked(ffi::RETRO_DEVICE_ID_POINTER_Y), Some(centre.y));
    assert_eq!(asked(ffi::RETRO_DEVICE_ID_POINTER_PRESSED), Some(1));

    // R3 for a few frames, then released: the other hybrid layout.
    let before = hash(core.frame());
    let mut r3 = InputState::default();
    r3.set(0, Button::R3, true);
    for _ in 0..5 {
        core.run_frame(&r3);
    }
    let (swapped, _) = run(&mut core, 30);
    eprintln!(
        "after R3: {}x{}, {:?}",
        swapped.width,
        swapped.height,
        core.av_info()
    );
    assert_ne!(hash(&swapped), before, "R3 changed the picture");
}
