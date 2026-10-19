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
fn serial() -> MutexGuard<'static, ()> {
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
    Core::load(&core, &game, dirs)
}

fn run(core: &mut Core, frames: usize) -> (Frame, usize) {
    let input = InputState::default();
    let mut samples = 0;
    let mut last = Frame::default();
    for _ in 0..frames {
        let out = core.run_frame(&input);
        samples += out.audio.len();
        last = out.video.clone();
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
