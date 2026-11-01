//! libretro Cores: which one plays each Embedded System, and downloading one the first time its
//! System is played (FR-EMB-2, US-23).
//!
//! Cores are never committed (ADR 0004); they are fetched from the libretro build server into
//! `~/.local/share/game37/cores/` on first use and kept there for later launches.

use std::fmt;
use std::fs;
use std::io;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use tempfile::NamedTempFile;

use crate::libretro::{Button, PortDevice};
use crate::systems::System;

/// A libretro Core: its display name, the `.so` file the build server publishes, and what it
/// needs to run well (FR-EMB-1, FR-EMB-12, FR-DS-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreInfo {
    pub name: &'static str,
    pub file: &'static str,
    /// `GET_VARIABLE` answers this Core needs ahead of its own defaults, e.g. to turn on hardware
    /// rendering or to lay out Nintendo DS's two screens (FR-DS-1).
    pub options: &'static [(&'static str, &'static str)],
    /// The BIOS the Core needs before it can start a Game (FR-EMB-12), or `None` when it needs none.
    pub bios: Option<&'static BiosRule>,
    /// Files the Core needs beside its library, downloaded with it, or `None` when it needs none.
    /// A Core with assets has its system directory under the data dir (see [`system_dir`]).
    pub assets: Option<&'static CoreAssets>,
    /// How it keeps a Game's In-game Saves.
    pub saves: InGameSaves,
    /// The touch screen of a two-screen System (DS, 3DS), or `None` for a System without one.
    pub touch: Option<TouchScreen>,
}

/// A Core's touch screen, one of two screens it shows in one frame (FR-DS-1–3, FR-NEW-2): the
/// mouse touches it through `RETRO_DEVICE_POINTER` over the whole frame, the mouse cursor shows,
/// Game37's swap control (a pad's R3, Tab) swaps which screen is large, and the right stick moves
/// a cursor the Core draws on it, which taps on `tap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchScreen {
    /// How Game37's swap control reaches the Core.
    pub swap: Swap,
    /// The RetroPad button the Core's right-stick cursor taps on: R2 on melonDS DS (its "Touch
    /// Joystick"; the DS has no R2 of its own), R3 on Azahar (its "Touch Screen Touch").
    pub tap: Button,
    /// The pad button that reaches the Core as `tap`: R2 itself on DS, L3 on 3DS, whose R2 is ZR
    /// and whose R3 is Game37's swap. On DS L3 stays L3, melonDS DS's microphone.
    pub tap_with: Button,
}

/// How Game37's swap control reaches a Core (FR-DS-2, FR-NEW-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Swap {
    /// The Core swaps on this RetroPad button, held while the swap control is: melonDS DS's
    /// "Next Screen Layout", R3, which cycles its two layouts.
    Button(Button),
    /// The Core swaps when the option `key` changes during play, from `values[0]`, the value the
    /// Game starts with, to `values[1]` and back, once per press of the swap control: Azahar's
    /// `citra_swap_screen`, its prominent screen. Its own swap button, L3, is also the 3DS HOME
    /// button, so it never sees L3 pressed.
    Option {
        key: &'static str,
        values: [&'static str; 2],
    },
}

/// How a Core keeps a Game's In-game Saves, all under the save directory it is given,
/// `<Games Root>/saves/<system>/` (FR-EMB-7, FR-EMB-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InGameSaves {
    /// It hands over its save memory, which Game37 keeps in the Game's `.srm`
    /// ([`crate::session::saves::srm_path`]).
    SaveMemory,
    /// It writes them itself, into files every Game of the System shares (PPSSPP's `SAVEDATA`
    /// folder, a System's NAND): no Game has a save file of its own to delete.
    Shared,
    /// It writes them itself, one file per Game: `<save dir>/<Game file stem>.<extension>`.
    PerGame { extension: &'static str },
}

/// Where a Core's BIOS goes and which files count as one (FR-EMB-12), with how Game37 words it for
/// the Core's System.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BiosRule {
    /// The folder the BIOS goes in, under `<Games Root>/bios/`; empty for that folder itself.
    pub folder: &'static str,
    pub files: BiosFiles,
    pub wording: BiosWording,
}

/// Which files in a [`BiosRule`]'s folder count as the BIOS. Only regular files count (a symlink
/// counts by what it points at), so a directory or a dangling symlink never does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiosFiles {
    /// Any one of these names (lowercase), matched case-insensitively, as a non-empty file.
    Named(&'static [&'static str]),
    /// Any file from `min` to `max` bytes, whatever its name: the Core itself checks that it is
    /// a real BIOS.
    Sized { min: u64, max: u64 },
}

/// What Game37 tells the player about a [`BiosRule`], in its System's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BiosWording {
    /// The missing-BIOS message's title.
    pub title: &'static str,
    /// The missing-BIOS message shown on Play; `{dir}` stands for the folder.
    pub message: &'static str,
    /// What Settings → BIOS and Setup ask the player to put in the folder.
    pub ask_for: &'static str,
}

/// PS1's BIOS (Beetle PSX HW): one of the region BIOS files directly in `<Games Root>/bios/`. The
/// wording names the three region files rather than every name the check accepts.
pub static PS1_BIOS: BiosRule = BiosRule {
    folder: "",
    files: BiosFiles::Named(&[
        "scph5500.bin",
        "scph5501.bin",
        "scph5502.bin",
        "scph5503.bin",
        "scph7003.bin",
        "scph5552.bin",
    ]),
    wording: BiosWording {
        title: "PlayStation BIOS missing",
        message: "PlayStation games need a BIOS file. Put scph5501.bin (North America), scph5502.bin \
                  (Europe) or scph5500.bin (Japan) in {dir}.",
        ask_for: "scph5500.bin, scph5501.bin or scph5502.bin",
    },
};

/// PS2's BIOS (LRPS2, FR-NEW-1): the player's own dump, any file name, in
/// `<Games Root>/bios/pcsx2/bios/`, where LRPS2 looks for it under its system directory. The size
/// bounds are LRPS2's own (`MIN_BIOS_SIZE`, `MAX_BIOS_SIZE` in its `BiosTools.cpp`: 4 and 8 MiB,
/// both allowed); it then checks the file's contents itself, and only logs when none passes.
pub static PS2_BIOS: BiosRule = BiosRule {
    folder: "pcsx2/bios",
    files: BiosFiles::Sized {
        min: 4 * 1024 * 1024,
        max: 8 * 1024 * 1024,
    },
    wording: BiosWording {
        title: "PlayStation 2 BIOS missing",
        message: "PlayStation 2 games need a BIOS file dumped from your own PlayStation 2. Put it, \
                  with any file name, in {dir}.",
        ask_for: "your own BIOS dump, any file name,",
    },
};

impl BiosRule {
    /// The folder this BIOS goes in: `<games_root>/bios/<folder>`.
    pub fn dir(&self, games_root: &Path) -> PathBuf {
        let bios = games_root.join("bios");
        if self.folder.is_empty() {
            bios
        } else {
            bios.join(self.folder)
        }
    }

    /// The file in [`BiosRule::dir`] that satisfies this rule, named as it is on disk, or `None`.
    /// With several, a named rule takes the one earliest in its list and a sized rule the first by
    /// name.
    pub fn found(&self, games_root: &Path) -> Option<String> {
        let entries = fs::read_dir(self.dir(games_root)).ok()?;
        let files = entries.filter_map(|e| e.ok()).filter_map(|entry| {
            let metadata = fs::metadata(entry.path()).ok().filter(|m| m.is_file())?;
            Some((entry.file_name().to_string_lossy().into_owned(), metadata.len()))
        });
        match self.files {
            BiosFiles::Named(names) => files
                .filter(|(name, len)| *len > 0 && names.contains(&name.to_lowercase().as_str()))
                .map(|(name, _)| name)
                .min_by_key(|name| names.iter().position(|n| *n == name.to_lowercase())),
            BiosFiles::Sized { min, max } => files
                .filter(|(_, len)| (min..=max).contains(len))
                .map(|(name, _)| name)
                .min(),
        }
    }
}

/// Files a Core needs in its system directory beside its library, from a zip on the libretro build
/// server that holds one top folder (FR-EMB-1, FR-NEW-3). That folder unpacks to
/// `<data dir>/system/<folder>`, the Core's system directory being `<data dir>/system`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreAssets {
    /// How messages name them, e.g. "PPSSPP assets".
    pub name: &'static str,
    pub url: &'static str,
    /// The zip's top folder. Entries outside it are ignored.
    pub folder: &'static str,
    /// The folder, inside the unpacked one, whose presence says the assets are installed; empty for
    /// the unpacked folder itself.
    pub marker: &'static str,
}

/// PPSSPP's assets (M4 decision): the `PPSSPP/` folder, about 9 MB.
pub static PPSSPP_ASSETS: CoreAssets = CoreAssets {
    name: "PPSSPP assets",
    url: "https://buildbot.libretro.com/assets/system/PPSSPP.zip",
    folder: "PPSSPP",
    marker: "",
};

/// Dolphin's system files (FR-NEW-3): the `dolphin-emu/` folder, about 15 MB unpacked, which
/// Dolphin needs as `<system dir>/dolphin-emu/Sys`.
pub static DOLPHIN_ASSETS: CoreAssets = CoreAssets {
    name: "Dolphin system files",
    url: "https://buildbot.libretro.com/assets/system/Dolphin.zip",
    folder: "dolphin-emu",
    marker: "Sys",
};

pub static MESEN: CoreInfo = CoreInfo {
    name: "Mesen",
    file: "mesen_libretro.so",
    options: &[],
    bios: None,
    assets: None,
    saves: InGameSaves::SaveMemory,
    touch: None,
};
pub static BSNES: CoreInfo = CoreInfo {
    name: "bsnes",
    file: "bsnes_libretro.so",
    options: &[],
    bios: None,
    assets: None,
    saves: InGameSaves::SaveMemory,
    touch: None,
};
pub static SAMEBOY: CoreInfo = CoreInfo {
    name: "SameBoy",
    file: "sameboy_libretro.so",
    options: &[],
    bios: None,
    assets: None,
    saves: InGameSaves::SaveMemory,
    touch: None,
};
pub static MGBA: CoreInfo = CoreInfo {
    name: "mGBA",
    file: "mgba_libretro.so",
    options: &[],
    bios: None,
    assets: None,
    saves: InGameSaves::SaveMemory,
    touch: None,
};
pub static MUPEN64PLUS_NEXT: CoreInfo = CoreInfo {
    name: "Mupen64Plus-Next",
    file: "mupen64plus_next_libretro.so",
    options: &[],
    bios: None,
    assets: None,
    saves: InGameSaves::SaveMemory,
    touch: None,
};

/// Crops a European (PAL) N64 Game to its picture: such Games draw about 240 lines into the
/// 288-line PAL frame, which leaves black bands around it. In the Game's own lines and pixels, as
/// measured on Mario Kart 64 (Europe); the Core allows 0 to 50 for each side. NTSC Games fill
/// their frame, so they keep the Core's defaults.
const N64_PAL_CROP: &[(&str, &str)] = &[
    ("mupen64plus-EnableOverscan", "Enabled"),
    ("mupen64plus-OverscanTop", "24"),
    ("mupen64plus-OverscanBottom", "27"),
    ("mupen64plus-OverscanLeft", "4"),
    ("mupen64plus-OverscanRight", "4"),
];

/// Mupen64Plus-Next's options for the N64 Game at `game`: the PAL crop for a European one.
fn n64_game_options(game: &Path) -> &'static [(&'static str, &'static str)] {
    let mut header = [0u8; 64];
    let read = fs::File::open(game).and_then(|mut f| f.read_exact(&mut header));
    if read.is_ok() && n64_is_pal(&header) {
        N64_PAL_CROP
    } else {
        &[]
    }
}

/// Whether an N64 Game file's header names a PAL country, as Mupen64Plus decides it. The country
/// code is byte 0x3E of the big-endian header; the first 4 bytes tell the file's byte order. An
/// unknown order counts as NTSC.
fn n64_is_pal(header: &[u8; 64]) -> bool {
    let country = match header[..4] {
        // .z64, big-endian.
        [0x80, 0x37, 0x12, 0x40] => header[0x3E],
        // .v64, each 16-bit half swapped.
        [0x37, 0x80, 0x40, 0x12] => header[0x3F],
        // .n64, each 32-bit word reversed.
        [0x40, 0x12, 0x37, 0x80] => header[0x3D],
        _ => return false,
    };
    matches!(country, b'D' | b'F' | b'I' | b'P' | b'S' | b'U' | b'X' | b'Y')
}
/// Beetle PSX HW only calls `SET_HW_RENDER` when `GET_VARIABLE` answers its own `renderer`
/// option, so this is also needed for OpenGL hardware rendering to have any effect (see
/// `docs/research/m4-cores.md`).
pub static BEETLE_PSX_HW: CoreInfo = CoreInfo {
    name: "Beetle PSX HW",
    file: "mednafen_psx_hw_libretro.so",
    options: &[("beetle_psx_hw_renderer", "hardware_gl")],
    bios: Some(&PS1_BIOS),
    assets: None,
    saves: InGameSaves::SaveMemory,
    touch: None,
};
/// Options chosen for FR-DS-1–3: two hybrid layouts (one large screen, one small), the touch
/// pointer and joystick cursor both live, and a visible cursor for aiming it.
pub static MELONDS_DS: CoreInfo = CoreInfo {
    name: "melonDS DS",
    file: "melondsds_libretro.so",
    options: &[
        ("melonds_number_of_screen_layouts", "2"),
        ("melonds_screen_layout1", "hybrid-top"),
        ("melonds_screen_layout2", "hybrid-bottom"),
        // The large screen three times the small one: the picture is 4 × 256 by 3 × 192, exactly
        // 16:9, so on a 16:9 display the large screen fills the height and the small one the rest
        // of the width (FR-DS-1).
        ("melonds_hybrid_ratio", "3"),
        ("melonds_hybrid_small_screen", "one"),
        ("melonds_touch_mode", "auto"),
        ("melonds_show_cursor", "always"),
        // The JIT crashed the DS homebrew test Game here; the interpreter is fast enough on a
        // desktop CPU.
        ("melonds_jit_enable", "disabled"),
    ],
    bios: None,
    assets: None,
    saves: InGameSaves::SaveMemory,
    touch: Some(TouchScreen {
        swap: Swap::Button(Button::R3),
        tap: Button::R2,
        tap_with: Button::R2,
    }),
};
pub static PPSSPP: CoreInfo = CoreInfo {
    name: "PPSSPP",
    file: "ppsspp_libretro.so",
    options: &[],
    bios: None,
    assets: Some(&PPSSPP_ASSETS),
    saves: InGameSaves::Shared,
    touch: None,
};
/// LRPS2 (FR-NEW-1), with the values its own option list offers: OpenGL, as Game37 has no Vulkan
/// host; twice the PS2's resolution; a memory card per Game, `<save dir>/<file stem>.ps2`; and the
/// BIOS intro skipped.
pub static LRPS2: CoreInfo = CoreInfo {
    name: "LRPS2",
    file: "pcsx2_libretro.so",
    options: &[
        ("pcsx2_renderer", "OpenGL"),
        ("pcsx2_upscale_multiplier", "2x"),
        ("pcsx2_shared_memory_cards", "disabled"),
        ("pcsx2_fastboot", "enabled"),
    ],
    bios: Some(&PS2_BIOS),
    assets: None,
    saves: InGameSaves::PerGame { extension: "ps2" },
    touch: None,
};
/// Azahar (FR-NEW-2), with the values its own option list offers: OpenGL, never `Auto` or
/// Vulkan (Vulkan crashes on AMD, Azahar issue #2742); saves under `<save dir>/Azahar/`; the right
/// stick both as the C-stick and as a touch cursor Azahar draws on the touch screen, as on DS
/// (FR-NEW-2). The screens are laid out by the 3DS layout chosen in Settings
/// ([`screen_layouts`]). Its Save States break on every Core update: a slot that can't load then
/// says so.
pub static AZAHAR: CoreInfo = CoreInfo {
    name: "Azahar",
    file: "azahar_libretro.so",
    options: &[
        ("citra_graphics_api", "OpenGL"),
        ("citra_use_libretro_save_path", "LibRetro Default"),
        ("citra_analog_function", "c_stick_and_touchscreen"),
        // The top screen prominent, as the swap option starts (see `touch`).
        ("citra_swap_screen", "Top"),
    ],
    bios: None,
    assets: None,
    saves: InGameSaves::Shared,
    touch: Some(TouchScreen {
        swap: Swap::Option {
            key: "citra_swap_screen",
            values: ["Top", "Bottom"],
        },
        tap: Button::R3,
        tap_with: Button::L3,
    }),
};
/// Dolphin (FR-NEW-3), for GameCube and Wii, with the values its own option list offers: its one
/// hardware renderer, which Game37 hosts with OpenGL; twice the original resolution; shaders
/// compiled in the background with Dolphin's ubershaders drawing meanwhile, so a new effect
/// doesn't stutter. Its CPU thread stays on its own (dual core): Game37 takes the audio and input
/// it sends from there. Its on-screen messages are off: Dolphin draws them into the Game's
/// picture (at every start, for seconds, which OpenGL driver it runs on). Its frame rate is read
/// again after every frame ([`CoreInfo::rereads_timing`]). Dolphin's system files come with it
/// ([`DOLPHIN_ASSETS`]); its own files, such as GameCube memory cards and the Wii's NAND, go in a
/// `User/` folder in the System's save folder. On Wii the controller is a Wii Remote with a
/// Nunchuk ([`CoreInfo::port_device`]).
pub static DOLPHIN: CoreInfo = CoreInfo {
    name: "Dolphin",
    file: "dolphin_libretro.so",
    options: &[
        ("dolphin_renderer", "Hardware"),
        ("dolphin_efb_scale", "2"),
        ("dolphin_shader_compilation_mode", "2"),
        ("dolphin_main_cpu_thread", "enabled"),
        ("dolphin_osd_enabled", "disabled"),
    ],
    bios: None,
    assets: Some(&DOLPHIN_ASSETS),
    saves: InGameSaves::Shared,
    touch: None,
};

/// Dolphin's Wii Remote with a Nunchuk (`RETRO_DEVICE_WIIMOTE_NC` in its `Input.cpp`): what most
/// Wii Games play with. The left stick is the Nunchuk's stick and the right stick points the
/// Wii Remote at the screen; the face buttons are A, B, C and Z. Dolphin's plain RetroPad on Wii
/// is a Wii Remote alone, whose left stick tilts it.
pub const DOLPHIN_WII_REMOTE_AND_NUNCHUK: PortDevice = PortDevice::retropad_subclass(2);

/// How a two-screen System shows its two screens (FR-DS-1, FR-NEW-2), chosen per System in
/// Settings → Display. R3 swaps the screens in each (FR-DS-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenLayout {
    /// One screen large, the other small beside it.
    #[default]
    LargeAndSmall,
    /// Both screens next to each other at the same size.
    SideBySide,
    /// The top screen alone, filling the display; R3 shows the touch screen instead.
    OneScreen,
}

impl ScreenLayout {
    pub const ALL: [ScreenLayout; 3] = [
        ScreenLayout::LargeAndSmall,
        ScreenLayout::SideBySide,
        ScreenLayout::OneScreen,
    ];

    /// The name the library's `settings` table stores it under.
    pub fn id(self) -> &'static str {
        match self {
            ScreenLayout::LargeAndSmall => "large_and_small",
            ScreenLayout::SideBySide => "side_by_side",
            ScreenLayout::OneScreen => "one_screen",
        }
    }

    pub fn from_id(id: &str) -> Option<ScreenLayout> {
        ScreenLayout::ALL.into_iter().find(|l| l.id() == id)
    }

    /// The label its Settings row shows.
    pub fn label(self) -> &'static str {
        match self {
            ScreenLayout::LargeAndSmall => "Large and small",
            ScreenLayout::SideBySide => "Side by side",
            ScreenLayout::OneScreen => "One screen",
        }
    }

    /// The sub-line its Settings row shows: what the layout looks like, and the swap.
    pub fn sub(self) -> &'static str {
        match self {
            ScreenLayout::LargeAndSmall => "One screen large, the other small · R3 or Tab swaps them",
            ScreenLayout::SideBySide => "Both screens at the same size · R3 or Tab swaps them",
            ScreenLayout::OneScreen => "The top screen alone · R3 or Tab shows the touch screen",
        }
    }

    /// The next layout in [`ScreenLayout::ALL`], wrapping around.
    pub fn next(self) -> ScreenLayout {
        let i = ScreenLayout::ALL.iter().position(|&l| l == self).unwrap_or(0);
        ScreenLayout::ALL[(i + 1) % ScreenLayout::ALL.len()]
    }

    /// The previous layout in [`ScreenLayout::ALL`], wrapping around.
    pub fn prev(self) -> ScreenLayout {
        let n = ScreenLayout::ALL.len();
        let i = ScreenLayout::ALL.iter().position(|&l| l == self).unwrap_or(0);
        ScreenLayout::ALL[(i + n - 1) % n]
    }
}

/// The Core options that lay out a two-screen System's screens in each [`ScreenLayout`]. They
/// replace the same keys in its Core's own `options`, and are added after them otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenLayouts {
    /// Its Settings row, e.g. "DS layout".
    pub label: &'static str,
    pub large_and_small: &'static [(&'static str, &'static str)],
    pub side_by_side: &'static [(&'static str, &'static str)],
    pub one_screen: &'static [(&'static str, &'static str)],
}

impl ScreenLayouts {
    pub fn options(&self, layout: ScreenLayout) -> &'static [(&'static str, &'static str)] {
        match layout {
            ScreenLayout::LargeAndSmall => self.large_and_small,
            ScreenLayout::SideBySide => self.side_by_side,
            ScreenLayout::OneScreen => self.one_screen,
        }
    }
}

/// DS's layouts on melonDS DS, with its option values. Each has two layouts, so R3, its "next
/// layout" binding, swaps the screens. Large and small is [`MELONDS_DS`]'s own (FR-DS-1).
static DS_LAYOUTS: ScreenLayouts = ScreenLayouts {
    label: "DS layout",
    large_and_small: &[
        ("melonds_number_of_screen_layouts", "2"),
        ("melonds_screen_layout1", "hybrid-top"),
        ("melonds_screen_layout2", "hybrid-bottom"),
        ("melonds_hybrid_ratio", "3"),
        ("melonds_hybrid_small_screen", "one"),
    ],
    side_by_side: &[
        ("melonds_number_of_screen_layouts", "2"),
        ("melonds_screen_layout1", "left-right"),
        ("melonds_screen_layout2", "right-left"),
    ],
    one_screen: &[
        ("melonds_number_of_screen_layouts", "2"),
        ("melonds_screen_layout1", "top"),
        ("melonds_screen_layout2", "bottom"),
    ],
};

/// 3DS's layouts on its Core (Azahar), with its option values (FR-NEW-2). Large and small has the
/// large screen three times the small one, as on DS; Azahar's own default is four.
static THREE_DS_LAYOUTS: ScreenLayouts = ScreenLayouts {
    label: "3DS layout",
    large_and_small: &[
        ("citra_layout_option", "large_screen"),
        ("citra_large_screen_proportion", "3.00"),
    ],
    side_by_side: &[("citra_layout_option", "side_by_side")],
    one_screen: &[("citra_layout_option", "single_screen")],
};

/// The screen layouts of a System with two screens, or `None` for one with a single screen.
pub fn screen_layouts(system: &System) -> Option<&'static ScreenLayouts> {
    match system.id {
        "nds" => Some(&DS_LAYOUTS),
        "3ds" => Some(&THREE_DS_LAYOUTS),
        _ => None,
    }
}

/// Which OpenGL host a Core's context must be made on (see [`CoreInfo::gl`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlKind {
    /// GLX, on the X display: PPSSPP calls `glewInit()`, which fails on EGL.
    Glx,
    /// Beside the SDL renderer's own context, on its window.
    Sdl,
}

impl CoreInfo {
    /// The option values the Core runs `system`'s Game at `game` with: `options`, then those this
    /// Core reads from the Game's file, then, for a System with two screens, those of its `layout`
    /// in place of the same keys.
    pub fn options_for(
        &self,
        system: &System,
        game: &Path,
        layout: ScreenLayout,
    ) -> Vec<(&'static str, &'static str)> {
        let own = if std::ptr::eq(self, &MUPEN64PLUS_NEXT) {
            n64_game_options(game)
        } else {
            &[]
        };
        let mut options = [self.options, own].concat();
        let screens = screen_layouts(system).map_or(&[][..], |layouts| layouts.options(layout));
        for &(key, value) in screens {
            match options.iter_mut().find(|(k, _)| *k == key) {
                Some(option) => option.1 = value,
                None => options.push((key, value)),
            }
        }
        options
    }

    /// Which OpenGL host this Core's context must be made on.
    pub fn gl(&self) -> GlKind {
        if std::ptr::eq(self, &PPSSPP) {
            GlKind::Glx
        } else {
            GlKind::Sdl
        }
    }

    /// Whether the left stick also presses the d-pad: for Systems without an analog stick. N64,
    /// GameCube, Wii (the Nunchuk's), PSP, PS2 and 3DS have one, so their Cores read it as analog
    /// only.
    pub fn stick_is_dpad(&self) -> bool {
        ![&MUPEN64PLUS_NEXT, &PPSSPP, &LRPS2, &AZAHAR, &DOLPHIN]
            .iter()
            .any(|core| std::ptr::eq(self, *core))
    }

    /// Whether Game37 reads this Core's frame rate and sample rate again after every frame
    /// ([`crate::libretro::CoreSetup::reread_timing`]): Dolphin's follow the Game's video mode but
    /// are never announced, and at load they are those of the Game it ran before in the process
    /// (a Wii Game after a 50 Hz GameCube one would run at 50 frames a second).
    pub fn rereads_timing(&self) -> bool {
        std::ptr::eq(self, &DOLPHIN)
    }

    /// The controller this Core is told sits in each port for `system`'s Games: a RetroPad, but on
    /// Wii Dolphin's Wii Remote with a Nunchuk.
    pub fn port_device(&self, system: &System) -> PortDevice {
        if std::ptr::eq(self, &DOLPHIN) && system.id == "wii" {
            DOLPHIN_WII_REMOTE_AND_NUNCHUK
        } else {
            PortDevice::RETROPAD
        }
    }
}

/// The Core for a System, or `None` for an External System, which never uses a Core, or for an
/// Embedded System with no Core yet.
pub fn for_system(system: &System) -> Option<&'static CoreInfo> {
    match system.id {
        "nes" => Some(&MESEN),
        "snes" => Some(&BSNES),
        "gb" | "gbc" => Some(&SAMEBOY),
        "gba" => Some(&MGBA),
        "n64" => Some(&MUPEN64PLUS_NEXT),
        "ps1" => Some(&BEETLE_PSX_HW),
        "nds" => Some(&MELONDS_DS),
        "gc" | "wii" => Some(&DOLPHIN),
        "3ds" => Some(&AZAHAR),
        "ps2" => Some(&LRPS2),
        "psp" => Some(&PPSSPP),
        _ => None,
    }
}

/// Where a Core is, or would be, stored: `<data_dir>/cores/<file>`.
pub fn installed_path(data_dir: &Path, core: &CoreInfo) -> PathBuf {
    data_dir.join("cores").join(core.file)
}

/// A Core's own system directory (ADR 0005): `<data dir>/system` for a Core with assets, which are
/// replaceable data, `<Games Root>/bios` for every other Core's BIOS files (FR-LIB-9).
pub fn system_dir(core: &CoreInfo, data_dir: &Path, games_root: &Path) -> PathBuf {
    if core.assets.is_some() {
        data_dir.join("system")
    } else {
        games_root.join("bios")
    }
}

/// Where `assets` unpack to: `<data dir>/system/<folder>`, in their Core's system directory.
pub fn assets_dir(data_dir: &Path, assets: &CoreAssets) -> PathBuf {
    data_dir.join("system").join(assets.folder)
}

/// The folder whose presence says `assets` are installed: their [`CoreAssets::marker`].
pub fn assets_marker(data_dir: &Path, assets: &CoreAssets) -> PathBuf {
    let dir = assets_dir(data_dir, assets);
    if assets.marker.is_empty() {
        dir
    } else {
        dir.join(assets.marker)
    }
}

/// Whether `core`'s assets are installed. True when it needs none.
pub fn has_assets(data_dir: &Path, core: &CoreInfo) -> bool {
    core.assets
        .is_none_or(|assets| assets_marker(data_dir, assets).is_dir())
}

/// A Core is installed once its library is there and its assets, if it has any, are too.
pub fn is_installed(data_dir: &Path, core: &CoreInfo) -> bool {
    installed_path(data_dir, core).is_file() && has_assets(data_dir, core)
}

/// Whether `core`'s BIOS is in place (see [`BiosRule::found`]). True when the Core needs none.
pub fn has_required_bios(games_root: &Path, core: &CoreInfo) -> bool {
    core.bios.is_none_or(|rule| rule.found(games_root).is_some())
}

/// The file that satisfies `core`'s BIOS rule, named as it is on disk (see [`BiosRule::found`]).
/// `None` when there is none, or the Core needs none.
pub fn found_bios(games_root: &Path, core: &CoreInfo) -> Option<String> {
    core.bios.and_then(|rule| rule.found(games_root))
}

/// The libretro build server's URL for a Core's nightly zip.
pub fn url(core: &CoreInfo) -> String {
    format!(
        "https://buildbot.libretro.com/nightly/linux/x86_64/latest/{}.zip",
        core.file
    )
}

/// How far a [`Download`] has got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadState {
    Running { received: u64, total: Option<u64> },
    Done(PathBuf),
    Failed(String),
}

/// Why fetching the Core's zip failed, before any bytes of its body were read.
enum FetchError {
    /// Couldn't connect at all: DNS, refused, timed out.
    Connect,
    /// Connected, but the server didn't answer 200.
    Status(u16),
}

type FetchResult = Result<(Box<dyn Read + Send>, Option<u64>), FetchError>;

/// Performs the HTTP GET for a Core's zip. Injectable so tests run offline; production uses
/// [`ureq_fetch`].
type Fetcher = Arc<dyn Fn(&str) -> FetchResult + Send + Sync>;

/// A cap on a Core's zip size (FR-EMB-2 is for small libretro Cores, not arbitrary downloads).
const MAX_DOWNLOAD_BYTES: u64 = 100 * 1024 * 1024;

/// A cap on a Core's unzipped size, so a zip bomb can't fill memory or the disk. The largest real
/// Core, Azahar's, is about 47 MB; most are under 25 MB.
const MAX_CORE_BYTES: u64 = 64 * 1024 * 1024;

/// The size caps a download enforces. A parameter so tests can exercise them with tiny inputs.
#[derive(Debug, Clone, Copy)]
struct Limits {
    /// The zip as downloaded.
    download: u64,
    /// The Core unzipped from it, declared or actual.
    core: u64,
}

const LIMITS: Limits = Limits {
    download: MAX_DOWNLOAD_BYTES,
    core: MAX_CORE_BYTES,
};

/// A cap on an assets zip as downloaded.
const MAX_ASSETS_DOWNLOAD_BYTES: u64 = 100 * 1024 * 1024;

/// A cap on assets unpacked, so a zip bomb can't fill memory or the disk. The real assets are
/// 15 MB at most.
const MAX_ASSETS_BYTES: u64 = 64 * 1024 * 1024;

/// A Core download running on a background thread. `poll` never blocks; call it every frame.
/// Dropping a running `Download` cancels it and returns at once: its thread stops at its next step,
/// however long the network keeps it waiting, and leaves no partial files behind.
pub struct Download {
    state: Arc<Mutex<DownloadState>>,
    cancel: Arc<AtomicBool>,
}

impl Download {
    /// Starts downloading `core` into `data_dir` on a background thread.
    pub fn start(core: &'static CoreInfo, data_dir: PathBuf) -> Download {
        Download::spawn_limited(core, data_dir, Arc::new(ureq_fetch), LIMITS)
    }

    #[cfg(test)]
    fn spawn(core: &'static CoreInfo, data_dir: PathBuf, fetch: Fetcher) -> Download {
        Download::spawn_limited(core, data_dir, fetch, LIMITS)
    }

    fn spawn_limited(core: &'static CoreInfo, data_dir: PathBuf, fetch: Fetcher, limits: Limits) -> Download {
        let state = Arc::new(Mutex::new(DownloadState::Running {
            received: 0,
            total: None,
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let state_in_thread = Arc::clone(&state);
        let cancel_in_thread = Arc::clone(&cancel);
        // Detached: `Drop` never joins it.
        thread::spawn(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                run(
                    core,
                    &data_dir,
                    fetch.as_ref(),
                    limits,
                    &state_in_thread,
                    &cancel_in_thread,
                )
            }));
            let final_state = match result {
                Ok(Ok(path)) => DownloadState::Done(path),
                Ok(Err(message)) => DownloadState::Failed(message),
                // A bug, not a network problem: still never leave the screen waiting forever.
                Err(_) => DownloadState::Failed(format!("Couldn't download the {} Core.", core.name)),
            };
            *lock(&state_in_thread) = final_state;
        });
        Download { state, cancel }
    }

    /// Never blocks: reports the last known state.
    pub fn poll(&mut self) -> DownloadState {
        lock(&self.state).clone()
    }

    /// Starts downloading a Core's `assets` into [`assets_dir`] on a background thread (FR-EMB-1,
    /// FR-NEW-3).
    pub fn start_assets(assets: &'static CoreAssets, data_dir: PathBuf) -> Download {
        Download::spawn_assets_limited(
            assets,
            data_dir,
            Arc::new(ureq_fetch),
            MAX_ASSETS_DOWNLOAD_BYTES,
            MAX_ASSETS_BYTES,
        )
    }

    #[cfg(test)]
    fn spawn_assets(assets: &'static CoreAssets, data_dir: PathBuf, fetch: Fetcher) -> Download {
        Download::spawn_assets_limited(
            assets,
            data_dir,
            fetch,
            MAX_ASSETS_DOWNLOAD_BYTES,
            MAX_ASSETS_BYTES,
        )
    }

    fn spawn_assets_limited(
        assets: &'static CoreAssets,
        data_dir: PathBuf,
        fetch: Fetcher,
        download_cap: u64,
        assets_cap: u64,
    ) -> Download {
        let state = Arc::new(Mutex::new(DownloadState::Running {
            received: 0,
            total: None,
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let state_in_thread = Arc::clone(&state);
        let cancel_in_thread = Arc::clone(&cancel);
        thread::spawn(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                run_assets(
                    assets,
                    &data_dir,
                    fetch.as_ref(),
                    download_cap,
                    assets_cap,
                    &state_in_thread,
                    &cancel_in_thread,
                )
            }));
            let final_state = match result {
                Ok(Ok(path)) => DownloadState::Done(path),
                Ok(Err(message)) => DownloadState::Failed(message),
                Err(_) => DownloadState::Failed(format!("Couldn't download the {}.", assets.name)),
            };
            *lock(&state_in_thread) = final_state;
        });
        Download { state, cancel }
    }
}

/// The state is only ever replaced whole, so a poisoned lock still holds a meaningful value.
fn lock(state: &Mutex<DownloadState>) -> MutexGuard<'_, DownloadState> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Drop for Download {
    /// Never joins: the thread may be blocked in DNS, connect or a read for as long as its timeouts
    /// allow, and the UI must not freeze meanwhile. It sees `cancel` at its next step and removes its
    /// own temp files.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// The ELF magic a valid Linux shared library starts with.
const ELF_MAGIC: &[u8] = b"\x7fELF";

/// Read and extraction buffer size.
const CHUNK: usize = 64 * 1024;

/// The error a cancelled download ends with. Nobody sees it: the `Download` is already dropped.
const CANCELLED: &str = "cancelled";

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err(CANCELLED.to_string())
    } else {
        Ok(())
    }
}

fn run(
    core: &CoreInfo,
    data_dir: &Path,
    fetch: &(dyn Fn(&str) -> FetchResult + Send + Sync),
    limits: Limits,
    state: &Mutex<DownloadState>,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let cores_dir = data_dir.join("cores");
    fs::create_dir_all(&cores_dir).map_err(|e| save_failed(core, &e))?;
    check_cancel(cancel)?;

    let (mut reader, total) = fetch(&url(core)).map_err(|e| match e {
        FetchError::Connect => connect_failed(core),
        FetchError::Status(status) => bad_status(core, status),
    })?;
    check_cancel(cancel)?;

    // Stream to a temp file of this download's own, reporting progress as we go. Its name is
    // unique, so a retry of the same Core while this thread is still winding down never shares
    // it, and dropping it removes it on every path out of here.
    let mut zip_file = temp_file(&cores_dir, core, ".zip.part").map_err(|e| save_failed(core, &e))?;
    let mut buf = vec![0u8; CHUNK];
    let mut received: u64 = 0;
    loop {
        check_cancel(cancel)?;
        let n = reader.read(&mut buf).map_err(|_| incomplete(core))?;
        if n == 0 {
            break;
        }
        received += n as u64;
        if received > limits.download {
            return Err(incomplete(core));
        }
        zip_file.write_all(&buf[..n]).map_err(|e| save_failed(core, &e))?;
        *lock(state) = DownloadState::Running { received, total };
    }
    drop(reader);
    if let Some(total) = total
        && received < total
    {
        return Err(incomplete(core));
    }

    // Open the zip and take the one entry we trust: `by_name` matches the name exactly.
    let mut archive = zip::ZipArchive::new(zip_file.as_file()).map_err(|_| incomplete(core))?;
    let entry = archive.by_name(core.file).map_err(|_| invalid_library(core))?;
    let declared = entry.size();
    let content = read_core(core, entry, declared, limits.core, cancel)?;
    drop(archive);
    drop(zip_file);

    // Verify, make executable, and install with an atomic rename.
    if !content.starts_with(ELF_MAGIC) {
        return Err(invalid_library(core));
    }
    let mut library = temp_file(&cores_dir, core, ".tmp").map_err(|e| save_failed(core, &e))?;
    library.write_all(&content).map_err(|e| save_failed(core, &e))?;
    library
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o755))
        .map_err(|e| save_failed(core, &e))?;
    check_cancel(cancel)?;
    let final_path = installed_path(data_dir, core);
    library
        .persist(&final_path)
        .map_err(|e| save_failed(core, &e.error))?;
    Ok(final_path)
}

/// Reads a Core out of its zip `entry`, a chunk at a time so a cancel is seen promptly. An entry
/// over `limit`, by its `declared` size or by what it actually unpacks to, isn't a Core: it is
/// rejected without reading more than `limit + 1` bytes.
fn read_core(
    core: &CoreInfo,
    entry: impl Read,
    declared: u64,
    limit: u64,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    if declared > limit {
        return Err(invalid_library(core));
    }
    let mut capped = entry.take(limit + 1);
    let mut content = Vec::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        check_cancel(cancel)?;
        let n = capped.read(&mut buf).map_err(|_| incomplete(core))?;
        if n == 0 {
            break;
        }
        content.extend_from_slice(&buf[..n]);
    }
    if content.len() as u64 > limit {
        return Err(invalid_library(core));
    }
    Ok(content)
}

/// A uniquely named hidden temp file in `cores/`, removed when dropped unless persisted.
fn temp_file(cores_dir: &Path, core: &CoreInfo, suffix: &str) -> io::Result<NamedTempFile> {
    tempfile::Builder::new()
        .prefix(&format!(".{}.", core.file))
        .suffix(suffix)
        .tempfile_in(cores_dir)
}

/// Downloads `assets`' zip and unpacks the entries under its top folder into [`assets_dir`], the
/// same safe way as a Core: a unique temp file and a temp extraction directory, cleaned up on every
/// path out, with no entry escaping its folder via `..` or an absolute path, and both the download
/// and the unpacked size capped. A zip without the assets' marker folder isn't valid, so an install
/// never succeeds without making them count as installed.
fn run_assets(
    assets: &CoreAssets,
    data_dir: &Path,
    fetch: &(dyn Fn(&str) -> FetchResult + Send + Sync),
    download_cap: u64,
    assets_cap: u64,
    state: &Mutex<DownloadState>,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let system_dir = data_dir.join("system");
    fs::create_dir_all(&system_dir).map_err(|e| assets_save_failed(assets, &e))?;
    check_cancel(cancel)?;

    let (mut reader, total) = fetch(assets.url).map_err(|e| match e {
        FetchError::Connect => assets_connect_failed(assets),
        FetchError::Status(status) => assets_bad_status(assets, status),
    })?;
    check_cancel(cancel)?;

    let prefix = format!(".{}-assets", assets.folder.to_lowercase());
    let mut zip_file = tempfile::Builder::new()
        .prefix(&format!("{prefix}."))
        .suffix(".zip.part")
        .tempfile_in(&system_dir)
        .map_err(|e| assets_save_failed(assets, &e))?;
    let mut buf = vec![0u8; CHUNK];
    let mut received: u64 = 0;
    loop {
        check_cancel(cancel)?;
        let n = reader.read(&mut buf).map_err(|_| assets_incomplete(assets))?;
        if n == 0 {
            break;
        }
        received += n as u64;
        if received > download_cap {
            return Err(assets_incomplete(assets));
        }
        zip_file
            .write_all(&buf[..n])
            .map_err(|e| assets_save_failed(assets, &e))?;
        *lock(state) = DownloadState::Running { received, total };
    }
    drop(reader);
    if let Some(total) = total
        && received < total
    {
        return Err(assets_incomplete(assets));
    }

    let mut archive = zip::ZipArchive::new(zip_file.as_file()).map_err(|_| assets_incomplete(assets))?;
    let extract_dir = tempfile::Builder::new()
        .prefix(&format!("{prefix}-extract."))
        .tempdir_in(&system_dir)
        .map_err(|e| assets_save_failed(assets, &e))?;
    let mut extracted: u64 = 0;
    let mut files_written: u64 = 0;
    for i in 0..archive.len() {
        check_cancel(cancel)?;
        let mut entry = archive.by_index(i).map_err(|_| assets_invalid(assets))?;
        // Only entries under the top folder, and never one that escapes the extraction dir.
        let Ok(name) = entry.name() else { continue };
        let Some(rel) = safe_asset_path(assets.folder, &name) else {
            continue;
        };
        let dest = extract_dir.path().join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&dest).map_err(|e| assets_save_failed(assets, &e))?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| assets_save_failed(assets, &e))?;
        }
        extracted += entry.size();
        if extracted > assets_cap {
            return Err(assets_invalid(assets));
        }
        let mut out = fs::File::create(&dest).map_err(|e| assets_save_failed(assets, &e))?;
        let mut capped = (&mut entry).take(assets_cap + 1);
        let mut written: u64 = 0;
        loop {
            check_cancel(cancel)?;
            let n = capped.read(&mut buf).map_err(|_| assets_incomplete(assets))?;
            if n == 0 {
                break;
            }
            written += n as u64;
            out.write_all(&buf[..n])
                .map_err(|e| assets_save_failed(assets, &e))?;
        }
        if written > assets_cap {
            return Err(assets_invalid(assets));
        }
        files_written += 1;
    }
    drop(archive);
    drop(zip_file);
    check_cancel(cancel)?;
    if files_written == 0 || !extract_dir.path().join(assets.marker).is_dir() {
        return Err(assets_invalid(assets));
    }

    let final_dir = assets_dir(data_dir, assets);
    if final_dir.exists() {
        fs::remove_dir_all(&final_dir).map_err(|e| assets_save_failed(assets, &e))?;
    }
    fs::rename(extract_dir.path(), &final_dir).map_err(|e| assets_save_failed(assets, &e))?;
    // Already moved to its final place: disarm the temp dir's own cleanup.
    let _ = extract_dir.keep();
    Ok(final_dir)
}

/// The path under the extraction directory for a zip entry named `name`, or `None` when it is
/// outside `folder/` or would escape the extraction directory (a `..` component or an absolute
/// path).
fn safe_asset_path(folder: &str, name: &str) -> Option<PathBuf> {
    let rel = name.strip_prefix(folder)?.strip_prefix('/')?;
    if rel.is_empty() {
        return None;
    }
    let mut out = PathBuf::new();
    for component in Path::new(rel).components() {
        match component {
            std::path::Component::Normal(part) => out.push(part),
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn assets_connect_failed(assets: &CoreAssets) -> String {
    format!(
        "Couldn't download the {}. Check your internet connection.",
        assets.name
    )
}

fn assets_bad_status(assets: &CoreAssets, status: u16) -> String {
    format!(
        "The libretro build server couldn't provide the {} (HTTP {status}).",
        assets.name
    )
}

fn assets_incomplete(assets: &CoreAssets) -> String {
    format!("The {} download was incomplete.", assets.name)
}

fn assets_invalid(assets: &CoreAssets) -> String {
    format!("The downloaded {} aren't valid.", assets.name)
}

fn assets_save_failed(assets: &CoreAssets, error: &io::Error) -> String {
    format!("Couldn't save the {}: {error}.", assets.name)
}

fn connect_failed(core: &CoreInfo) -> String {
    format!(
        "Couldn't download the {} Core. Check your internet connection.",
        core.name
    )
}

fn bad_status(core: &CoreInfo, status: u16) -> String {
    format!(
        "The libretro build server couldn't provide the {} Core (HTTP {status}).",
        core.name
    )
}

fn incomplete(core: &CoreInfo) -> String {
    format!("The {} Core download was incomplete.", core.name)
}

fn invalid_library(core: &CoreInfo) -> String {
    format!("The downloaded {} Core isn't a valid Linux library.", core.name)
}

fn save_failed(core: &CoreInfo, error: &io::Error) -> String {
    format!("Couldn't save the {} Core: {error}.", core.name)
}

/// The production fetcher: `ureq` over rustls.
///
/// Each step up to the response headers has its own 30 s timeout. The body gets 10 minutes in
/// total, as ureq 3 has no idle timeout between reads: enough for the largest Core on a slow but
/// working connection. The user can cancel at any point regardless.
///
/// Redirects are not followed: the build server answers its zip URLs directly, and NFR-5 allows
/// no other host. A redirect shows up as its HTTP status.
fn ureq_fetch(url: &str) -> FetchResult {
    let step = Some(Duration::from_secs(30));
    let config = ureq::Agent::config_builder()
        .timeout_resolve(step)
        .timeout_connect(step)
        .timeout_send_request(step)
        .timeout_recv_response(step)
        .timeout_recv_body(Some(Duration::from_secs(10 * 60)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build();
    let agent: ureq::Agent = config.into();
    let response = agent.get(url).call().map_err(|_| FetchError::Connect)?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(FetchError::Status(status));
    }
    let total = response.body().content_length();
    let reader = response.into_body().into_reader();
    Ok((Box::new(reader), total))
}

impl fmt::Debug for Download {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Download").finish_non_exhaustive()
    }
}

/// Offline [`Download`]s for other modules' tests (the download screen's, in particular): never
/// touches the network.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::io::Cursor;

    /// A `Download` that completes with `bytes` as the fetched zip's body.
    pub fn spawn(core: &'static CoreInfo, data_dir: PathBuf, bytes: Vec<u8>, total: Option<u64>) -> Download {
        let fetch: Fetcher = Arc::new(move |_url: &str| {
            Ok((
                Box::new(Cursor::new(bytes.clone())) as Box<dyn Read + Send>,
                total,
            ))
        });
        Download::spawn(core, data_dir, fetch)
    }

    /// A `Download` with no thread behind it: it reports whatever the test puts in the returned
    /// state, so a test can hold it running for as long as it likes.
    pub fn manual() -> (Download, Arc<Mutex<DownloadState>>) {
        let state = Arc::new(Mutex::new(DownloadState::Running {
            received: 0,
            total: None,
        }));
        let download = Download {
            state: Arc::clone(&state),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        (download, state)
    }

    /// A `Download` that fails to connect as soon as it is polled.
    pub fn spawn_failing(core: &'static CoreInfo, data_dir: PathBuf) -> Download {
        Download::spawn(core, data_dir, Arc::new(|_url: &str| Err(FetchError::Connect)))
    }

    /// A valid zip for `core`: its file, holding a tiny fake ELF library.
    pub fn core_zip(core: &CoreInfo) -> Vec<u8> {
        let mut content = ELF_MAGIC.to_vec();
        content.extend_from_slice(b"fake core");
        let mut buf = Vec::new();
        let mut writer = zip::ZipWriter::new(Cursor::new(&mut buf));
        let options =
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer.start_file(core.file, options).unwrap();
        writer.write_all(&content).unwrap();
        writer.finish().unwrap();
        buf
    }

    /// A `Download` of PPSSPP's assets that completes with `bytes` as the fetched zip's body.
    pub fn spawn_assets(data_dir: PathBuf, bytes: Vec<u8>, total: Option<u64>) -> Download {
        spawn_assets_of(&PPSSPP_ASSETS, data_dir, bytes, total)
    }

    /// [`spawn_assets`] for any Core's `assets`.
    pub fn spawn_assets_of(
        assets: &'static CoreAssets,
        data_dir: PathBuf,
        bytes: Vec<u8>,
        total: Option<u64>,
    ) -> Download {
        let fetch: Fetcher = Arc::new(move |_url: &str| {
            Ok((
                Box::new(Cursor::new(bytes.clone())) as Box<dyn Read + Send>,
                total,
            ))
        });
        Download::spawn_assets(assets, data_dir, fetch)
    }

    /// A `Download` of PPSSPP's assets that fails to connect as soon as it is polled.
    pub fn spawn_assets_failing(data_dir: PathBuf) -> Download {
        Download::spawn_assets(
            &PPSSPP_ASSETS,
            data_dir,
            Arc::new(|_url: &str| Err(FetchError::Connect)),
        )
    }

    /// A zip holding exactly `entries` (`(name, content)`), for assets tests to add entries
    /// outside the top folder or that try to escape it.
    pub fn assets_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut writer = zip::ZipWriter::new(Cursor::new(&mut buf));
        let options =
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, content) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(content).unwrap();
        }
        writer.finish().unwrap();
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::{SYSTEMS, by_id};
    use std::io::Cursor;
    use std::sync::atomic::AtomicU64;
    use std::sync::{Barrier, mpsc};
    use std::time::Instant;

    fn zip_with(entry_name: &str, content: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut buf));
            let options =
                zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            writer.start_file(entry_name, options).unwrap();
            writer.write_all(content).unwrap();
            writer.finish().unwrap();
        }
        buf
    }

    fn core_content() -> Vec<u8> {
        let mut content = ELF_MAGIC.to_vec();
        content.extend_from_slice(b"rest of a fake core");
        content
    }

    fn core_zip() -> Vec<u8> {
        zip_with(SAMEBOY.file, &core_content())
    }

    fn fixed_reader_fetcher(bytes: Vec<u8>) -> Fetcher {
        let total = bytes.len() as u64;
        Arc::new(move |_url: &str| {
            Ok((
                Box::new(Cursor::new(bytes.clone())) as Box<dyn Read + Send>,
                Some(total),
            ))
        })
    }

    fn truncated_total_fetcher(bytes: Vec<u8>, claimed_total: u64) -> Fetcher {
        Arc::new(move |_url: &str| {
            Ok((
                Box::new(Cursor::new(bytes.clone())) as Box<dyn Read + Send>,
                Some(claimed_total),
            ))
        })
    }

    fn failing_fetcher() -> Fetcher {
        Arc::new(|_url: &str| Err(FetchError::Connect))
    }

    fn wait_for<F: FnMut() -> Option<T>, T>(mut f: F) -> T {
        for _ in 0..2000 {
            if let Some(value) = f() {
                return value;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for condition");
    }

    fn wait_for_failure(download: &mut Download) -> String {
        wait_for(|| match download.poll() {
            DownloadState::Failed(m) => Some(m),
            DownloadState::Done(_) => panic!("expected a failure"),
            DownloadState::Running { .. } => None,
        })
    }

    fn wait_for_done(download: &mut Download) -> PathBuf {
        wait_for(|| match download.poll() {
            DownloadState::Done(path) => Some(path),
            DownloadState::Failed(e) => panic!("download failed: {e}"),
            DownloadState::Running { .. } => None,
        })
    }

    /// Everything in `cores/` except an installed SameBoy: any temp file a download left behind.
    fn leftovers(data_dir: &Path) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(data_dir.join("cores")) else {
            return Vec::new();
        };
        entries
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap() != SAMEBOY.file)
            .collect()
    }

    /// `n` zero bytes, counting how many have been read so far into `consumed`.
    struct Zeros {
        remaining: u64,
        consumed: Arc<AtomicU64>,
    }
    impl Read for Zeros {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = (buf.len() as u64).min(self.remaining) as usize;
            buf[..n].fill(0);
            self.remaining -= n as u64;
            self.consumed.fetch_add(n as u64, Ordering::Relaxed);
            Ok(n)
        }
    }

    #[test]
    fn table_has_a_core_for_each_embedded_system_of_this_milestone() {
        assert_eq!(for_system(by_id("nes").unwrap()), Some(&MESEN));
        assert_eq!(for_system(by_id("snes").unwrap()), Some(&BSNES));
        assert_eq!(for_system(by_id("gb").unwrap()), Some(&SAMEBOY));
        assert_eq!(for_system(by_id("gbc").unwrap()), Some(&SAMEBOY));
        assert_eq!(for_system(by_id("gba").unwrap()), Some(&MGBA));
        assert_eq!(for_system(by_id("n64").unwrap()), Some(&MUPEN64PLUS_NEXT));
        assert_eq!(for_system(by_id("ps1").unwrap()), Some(&BEETLE_PSX_HW));
        assert_eq!(for_system(by_id("nds").unwrap()), Some(&MELONDS_DS));
        assert_eq!(for_system(by_id("3ds").unwrap()), Some(&AZAHAR));
        assert_eq!(for_system(by_id("ps2").unwrap()), Some(&LRPS2));
        assert_eq!(for_system(by_id("psp").unwrap()), Some(&PPSSPP));
        for system in SYSTEMS
            .iter()
            .filter(|s| s.kind == crate::systems::Kind::External)
        {
            assert_eq!(for_system(system), None, "{}", system.id);
        }
    }

    #[test]
    fn only_n64_gamecube_wii_psp_ps2_and_3ds_read_the_left_stick_as_analog_only() {
        assert!(!MUPEN64PLUS_NEXT.stick_is_dpad());
        assert!(!DOLPHIN.stick_is_dpad());
        assert!(!PPSSPP.stick_is_dpad());
        assert!(!LRPS2.stick_is_dpad());
        assert!(!AZAHAR.stick_is_dpad());
        for core in [&MESEN, &BSNES, &SAMEBOY, &MGBA, &BEETLE_PSX_HW, &MELONDS_DS] {
            assert!(core.stick_is_dpad(), "{}", core.name);
        }
    }

    /// FR-NEW-3: on Wii the controller stands in for a Wii Remote, with a Nunchuk; everywhere
    /// else it is a RetroPad. Only Dolphin needs its timing reread.
    #[test]
    fn dolphin_gets_a_wii_remote_and_nunchuk_on_wii_and_its_timing_reread() {
        for system in &crate::systems::SYSTEMS {
            let Some(core) = for_system(system) else { continue };
            let expected = if system.id == "wii" {
                DOLPHIN_WII_REMOTE_AND_NUNCHUK
            } else {
                PortDevice::RETROPAD
            };
            assert_eq!(core.port_device(system), expected, "{}", system.id);
            assert_eq!(core.rereads_timing(), core.name == "Dolphin", "{}", system.id);
        }
        assert_eq!(
            DOLPHIN_WII_REMOTE_AND_NUNCHUK.id(),
            (3 << 8) | 1,
            "RETRO_DEVICE_WIIMOTE_NC"
        );
        assert!(std::ptr::eq(
            for_system(crate::systems::by_id("gc").unwrap()).unwrap(),
            &DOLPHIN
        ));
    }

    /// A big-endian (.z64) N64 Game file header with `country` at 0x3E.
    fn n64_header(country: u8) -> [u8; 64] {
        let mut header = [0u8; 64];
        header[..4].copy_from_slice(&[0x80, 0x37, 0x12, 0x40]);
        header[0x3E] = country;
        header
    }

    /// `header` in .v64 order: each 16-bit half swapped.
    fn v64(mut header: [u8; 64]) -> [u8; 64] {
        header.chunks_mut(2).for_each(|half| half.reverse());
        header
    }

    /// `header` in .n64 order: each 32-bit word reversed.
    fn n64(mut header: [u8; 64]) -> [u8; 64] {
        header.chunks_mut(4).for_each(|word| word.reverse());
        header
    }

    #[test]
    fn n64_game_headers_name_pal_in_every_byte_order() {
        for country in *b"DFIPSUXY" {
            for header in [
                n64_header(country),
                v64(n64_header(country)),
                n64(n64_header(country)),
            ] {
                assert!(n64_is_pal(&header), "{}: {:02x?}", country as char, &header[..4]);
            }
        }
        // USA, Japan, and a country code from nowhere.
        for country in [b'E', b'J', 0] {
            for header in [
                n64_header(country),
                v64(n64_header(country)),
                n64(n64_header(country)),
            ] {
                assert!(!n64_is_pal(&header), "{}: {:02x?}", country as char, &header[..4]);
            }
        }
        let mut garbage = [b'P'; 64];
        garbage[..4].copy_from_slice(b"GAME");
        assert!(!n64_is_pal(&garbage), "an unknown byte order counts as NTSC");
    }

    #[test]
    fn mupen64plus_next_crops_only_pal_n64_games() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, bytes: &[u8]| {
            let path = dir.path().join(name);
            fs::write(&path, bytes).unwrap();
            path
        };
        let pal = [
            write("pal.z64", &n64_header(b'P')),
            write("pal.v64", &v64(n64_header(b'D'))),
            write("pal.n64", &n64(n64_header(b'U'))),
        ];
        let n64_system = by_id("n64").unwrap();
        let options_for = |game| MUPEN64PLUS_NEXT.options_for(n64_system, game, ScreenLayout::default());
        for game in &pal {
            assert_eq!(options_for(game), N64_PAL_CROP, "{}", game.display());
        }
        let ntsc = [
            write("ntsc.z64", &n64_header(b'E')),
            write("ntsc.n64", &n64(n64_header(b'J'))),
            write("short.z64", &n64_header(b'P')[..63]),
            write("garbage.z64", &[b'P'; 64]),
            dir.path().join("missing.z64"),
        ];
        for game in &ntsc {
            assert_eq!(options_for(game), [], "{}", game.display());
        }
        // Every other Core, DS's in its default layout too, runs with its own options alone.
        for system in SYSTEMS.iter().filter(|s| s.id != "n64" && s.id != "3ds") {
            if let Some(core) = for_system(system) {
                let options = core.options_for(system, &pal[0], ScreenLayout::default());
                assert_eq!(options, core.options, "{}", core.name);
            }
        }
        let three_ds = crate::systems::by_id("3ds").unwrap();
        assert_eq!(
            AZAHAR.options_for(three_ds, &pal[0], ScreenLayout::default()),
            [AZAHAR.options, THREE_DS_LAYOUTS.large_and_small].concat(),
            "Azahar's own options, then the 3DS layout's"
        );
    }

    #[test]
    fn screen_layouts_cycle_both_ways_and_round_trip_their_ids() {
        use ScreenLayout::*;
        assert_eq!(ScreenLayout::default(), LargeAndSmall);
        assert_eq!(
            [LargeAndSmall.next(), SideBySide.next(), OneScreen.next()],
            [SideBySide, OneScreen, LargeAndSmall]
        );
        assert_eq!(
            [LargeAndSmall.prev(), SideBySide.prev(), OneScreen.prev()],
            [OneScreen, LargeAndSmall, SideBySide]
        );
        for layout in ScreenLayout::ALL {
            assert_eq!(ScreenLayout::from_id(layout.id()), Some(layout));
        }
        assert_eq!(ScreenLayout::from_id("top-bottom"), None);
    }

    #[test]
    fn only_ds_and_3ds_have_screen_layouts_each_named_after_its_system() {
        let with: Vec<_> = SYSTEMS
            .iter()
            .filter(|s| screen_layouts(s).is_some())
            .map(|s| s.id)
            .collect();
        assert_eq!(with, ["nds", "3ds"]);
        for system in SYSTEMS.iter().filter(|s| screen_layouts(s).is_some()) {
            assert_eq!(
                screen_layouts(system).unwrap().label,
                format!("{} layout", system.short)
            );
        }
    }

    /// The options melonDS DS runs a DS Game with in `layout`.
    fn ds_options(layout: ScreenLayout) -> Vec<(&'static str, &'static str)> {
        MELONDS_DS.options_for(by_id("nds").unwrap(), Path::new("/Games/nds/Game.nds"), layout)
    }

    /// Each choice's layouts are melonDS DS's own values (as its `SET_VARIABLES` offers them), two
    /// each so R3 swaps the screens; only the layout keys change, in place.
    #[test]
    fn each_ds_layout_gives_melonds_ds_its_two_layouts() {
        let with = |layout1, layout2| -> Vec<_> {
            MELONDS_DS
                .options
                .iter()
                .map(|&(key, value)| match key {
                    "melonds_screen_layout1" => (key, layout1),
                    "melonds_screen_layout2" => (key, layout2),
                    _ => (key, value),
                })
                .collect()
        };
        assert_eq!(
            ds_options(ScreenLayout::LargeAndSmall),
            MELONDS_DS.options,
            "FR-DS-1"
        );
        assert_eq!(
            ds_options(ScreenLayout::SideBySide),
            with("left-right", "right-left")
        );
        assert_eq!(ds_options(ScreenLayout::OneScreen), with("top", "bottom"));
        for layout in ScreenLayout::ALL {
            assert!(ds_options(layout).contains(&("melonds_number_of_screen_layouts", "2")));
        }
    }

    /// 3DS's layouts as Azahar's options, whatever Core options they join.
    #[test]
    fn each_3ds_layout_gives_azahars_layout_options() {
        let layouts = screen_layouts(by_id("3ds").unwrap()).unwrap();
        assert_eq!(
            layouts.options(ScreenLayout::LargeAndSmall),
            [
                ("citra_layout_option", "large_screen"),
                ("citra_large_screen_proportion", "3.00"),
            ]
        );
        assert_eq!(
            layouts.options(ScreenLayout::SideBySide),
            [("citra_layout_option", "side_by_side")]
        );
        assert_eq!(
            layouts.options(ScreenLayout::OneScreen),
            [("citra_layout_option", "single_screen")]
        );
        // On a Core whose own options set a layout, the choice replaces it in place.
        let core = CoreInfo {
            options: &[
                ("citra_layout_option", "default"),
                ("citra_graphics_api", "OpenGL"),
            ],
            ..MELONDS_DS
        };
        let game = Path::new("/Games/3ds/Game.3dsx");
        assert_eq!(
            core.options_for(by_id("3ds").unwrap(), game, ScreenLayout::OneScreen),
            [
                ("citra_layout_option", "single_screen"),
                ("citra_graphics_api", "OpenGL")
            ]
        );
        assert_eq!(
            core.options_for(by_id("3ds").unwrap(), game, ScreenLayout::LargeAndSmall),
            [
                ("citra_layout_option", "large_screen"),
                ("citra_graphics_api", "OpenGL"),
                ("citra_large_screen_proportion", "3.00"),
            ]
        );
    }

    #[test]
    fn only_ppsspp_needs_the_glx_host() {
        assert_eq!(PPSSPP.gl(), GlKind::Glx);
        for core in [
            &MESEN,
            &BSNES,
            &SAMEBOY,
            &MGBA,
            &MUPEN64PLUS_NEXT,
            &BEETLE_PSX_HW,
            &MELONDS_DS,
            &LRPS2,
            &AZAHAR,
        ] {
            assert_eq!(core.gl(), GlKind::Sdl, "{}", core.name);
        }
    }

    #[test]
    fn beetle_psx_hw_options_and_required_bios_match_the_research_doc() {
        assert_eq!(
            BEETLE_PSX_HW.options,
            &[("beetle_psx_hw_renderer", "hardware_gl")]
        );
        assert_eq!(BEETLE_PSX_HW.bios, Some(&PS1_BIOS));
        assert_eq!(PS1_BIOS.folder, "", "directly in bios/");
        assert_eq!(
            PS1_BIOS.files,
            BiosFiles::Named(&[
                "scph5500.bin",
                "scph5501.bin",
                "scph5502.bin",
                "scph5503.bin",
                "scph7003.bin",
                "scph5552.bin",
            ])
        );
    }

    #[test]
    fn lrps2_renders_with_opengl_keeps_a_memory_card_per_game_and_needs_the_ps2_bios() {
        assert_eq!(
            LRPS2.options,
            &[
                ("pcsx2_renderer", "OpenGL"),
                ("pcsx2_upscale_multiplier", "2x"),
                ("pcsx2_shared_memory_cards", "disabled"),
                ("pcsx2_fastboot", "enabled"),
            ]
        );
        assert_eq!(LRPS2.file, "pcsx2_libretro.so");
        assert_eq!(LRPS2.bios, Some(&PS2_BIOS));
        assert_eq!(LRPS2.saves, InGameSaves::PerGame { extension: "ps2" });
        let (data_dir, root) = (Path::new("/data"), Path::new("/Games"));
        assert_eq!(
            PS2_BIOS.dir(root),
            system_dir(&LRPS2, data_dir, root).join("pcsx2/bios"),
            "where LRPS2 looks under its system directory"
        );
    }

    /// The option values are the ones Azahar's own list offers (read from its `SET_VARIABLES`).
    #[test]
    fn azahar_renders_with_opengl_shares_its_saves_and_lays_out_large_and_small_at_3() {
        assert_eq!(
            AZAHAR.options,
            &[
                ("citra_graphics_api", "OpenGL"),
                ("citra_use_libretro_save_path", "LibRetro Default"),
                ("citra_analog_function", "c_stick_and_touchscreen"),
                ("citra_swap_screen", "Top"),
            ]
        );
        assert_eq!(
            THREE_DS_LAYOUTS.large_and_small,
            &[
                ("citra_layout_option", "large_screen"),
                ("citra_large_screen_proportion", "3.00"),
            ]
        );
        assert_eq!(AZAHAR.file, "azahar_libretro.so");
        assert_eq!((AZAHAR.bios, AZAHAR.assets), (None, None));
        assert_eq!(
            AZAHAR.saves,
            InGameSaves::Shared,
            "files under <save dir>/Azahar/"
        );
    }

    #[test]
    fn ds_and_3ds_have_a_touch_screen_whose_swap_and_tap_their_cores_take_differently() {
        assert_eq!(
            MELONDS_DS.touch,
            Some(TouchScreen {
                swap: Swap::Button(Button::R3),
                tap: Button::R2,
                tap_with: Button::R2,
            }),
            "melonDS DS's Next Screen Layout and Touch Joystick"
        );
        let Some(TouchScreen {
            swap: Swap::Option { key, values },
            tap: Button::R3,
            tap_with: Button::L3,
        }) = AZAHAR.touch
        else {
            panic!(
                "Azahar swaps through an option and taps on R3: {:?}",
                AZAHAR.touch
            );
        };
        assert_eq!((key, values), ("citra_swap_screen", ["Top", "Bottom"]));
        assert!(
            AZAHAR.options.contains(&(key, values[0])),
            "the Game starts with the screens unswapped"
        );
        for core in [
            &MESEN,
            &BSNES,
            &SAMEBOY,
            &MGBA,
            &MUPEN64PLUS_NEXT,
            &BEETLE_PSX_HW,
            &PPSSPP,
            &LRPS2,
        ] {
            assert_eq!(core.touch, None, "{}", core.name);
        }
    }

    #[test]
    fn melonds_ds_options_match_fr_ds_1_to_3() {
        assert_eq!(
            MELONDS_DS.options,
            &[
                ("melonds_number_of_screen_layouts", "2"),
                ("melonds_screen_layout1", "hybrid-top"),
                ("melonds_screen_layout2", "hybrid-bottom"),
                ("melonds_hybrid_ratio", "3"),
                ("melonds_hybrid_small_screen", "one"),
                ("melonds_touch_mode", "auto"),
                ("melonds_show_cursor", "always"),
                ("melonds_jit_enable", "disabled"),
            ]
        );
        assert!(MELONDS_DS.bios.is_none(), "DS's BIOS is optional (FreeBIOS)");
    }

    #[test]
    fn only_ppsspp_has_assets_and_uses_the_data_dirs_system_directory() {
        for core in [
            &MESEN,
            &BSNES,
            &SAMEBOY,
            &MGBA,
            &MUPEN64PLUS_NEXT,
            &BEETLE_PSX_HW,
            &MELONDS_DS,
            &LRPS2,
            &AZAHAR,
        ] {
            assert!(core.assets.is_none(), "{}", core.name);
        }
        assert_eq!(PPSSPP.assets, Some(&PPSSPP_ASSETS));
        assert_eq!(
            PPSSPP_ASSETS.url,
            "https://buildbot.libretro.com/assets/system/PPSSPP.zip"
        );
    }

    #[test]
    fn ppsspps_system_dir_is_under_the_data_dir_everyone_elses_is_the_bios_folder() {
        let data_dir = Path::new("/data");
        let root = Path::new("/Games");
        assert_eq!(system_dir(&PPSSPP, data_dir, root), data_dir.join("system"));
        assert_eq!(system_dir(&BEETLE_PSX_HW, data_dir, root), root.join("bios"));
    }

    #[test]
    fn has_required_bios_is_true_with_no_requirement_or_any_one_match_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(has_required_bios(root, &MELONDS_DS), "no requirement");
        assert!(!has_required_bios(root, &BEETLE_PSX_HW), "nothing there yet");

        fs::create_dir_all(root.join("bios")).unwrap();
        fs::write(root.join("bios/SCPH5502.BIN"), b"not really a bios").unwrap();
        assert!(
            has_required_bios(root, &BEETLE_PSX_HW),
            "any one, any case, is enough"
        );

        fs::write(root.join("bios/unrelated.bin"), b"").unwrap();
        assert!(has_required_bios(root, &BEETLE_PSX_HW));
    }

    #[test]
    fn has_required_bios_ignores_a_directory_or_an_empty_file_with_the_right_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("bios/scph5502.bin")).unwrap();
        assert!(!has_required_bios(root, &BEETLE_PSX_HW), "a directory");

        fs::remove_dir(root.join("bios/scph5502.bin")).unwrap();
        fs::write(root.join("bios/scph5502.bin"), b"").unwrap();
        assert!(!has_required_bios(root, &BEETLE_PSX_HW), "a zero-byte file");

        fs::write(root.join("bios/scph5502.bin"), b"x").unwrap();
        assert!(has_required_bios(root, &BEETLE_PSX_HW), "a non-empty file");
    }

    #[test]
    fn found_bios_names_the_file_on_disk_and_prefers_the_earliest_accepted_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(found_bios(root, &BEETLE_PSX_HW), None, "no bios folder");
        assert_eq!(found_bios(root, &MELONDS_DS), None, "needs none");
        fs::create_dir_all(root.join("bios")).unwrap();
        fs::write(root.join("bios/scph5501.bin"), b"").unwrap();
        fs::write(root.join("bios/other.bin"), b"x").unwrap();
        assert_eq!(
            found_bios(root, &BEETLE_PSX_HW),
            None,
            "an empty file doesn't count"
        );
        fs::write(root.join("bios/SCPH5552.BIN"), b"x").unwrap();
        assert_eq!(found_bios(root, &BEETLE_PSX_HW).as_deref(), Some("SCPH5552.BIN"));
        fs::write(root.join("bios/scph5501.bin"), b"x").unwrap();
        assert_eq!(found_bios(root, &BEETLE_PSX_HW).as_deref(), Some("scph5501.bin"));
    }

    /// A sparse file of `len` bytes at `path`, so a multi-MB BIOS stand-in costs no disk.
    fn file_of(path: &Path, len: u64) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::File::create(path).unwrap().set_len(len).unwrap();
    }

    const MIB: u64 = 1024 * 1024;

    #[test]
    fn the_ps2_bios_is_any_file_of_4_to_8_mib_in_bios_pcsx2_bios() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(PS2_BIOS.dir(root), root.join("bios/pcsx2/bios"));
        assert_eq!(PS2_BIOS.found(root), None, "no folder");

        file_of(&root.join("bios/ps2-bios.bin"), 4 * MIB);
        assert_eq!(PS2_BIOS.found(root), None, "not in its sub-folder");
        file_of(&root.join("bios/pcsx2/bios/small.bin"), 4 * MIB - 1);
        file_of(&root.join("bios/pcsx2/bios/large.bin"), 8 * MIB + 1);
        fs::create_dir_all(root.join("bios/pcsx2/bios/folder.bin")).unwrap();
        assert_eq!(PS2_BIOS.found(root), None, "too small, too large, a folder");

        file_of(&root.join("bios/pcsx2/bios/My Dump.BIN"), 4 * MIB);
        assert_eq!(PS2_BIOS.found(root).as_deref(), Some("My Dump.BIN"), "any name");
        file_of(&root.join("bios/pcsx2/bios/b.rom"), 8 * MIB);
        assert_eq!(
            PS2_BIOS.found(root).as_deref(),
            Some("My Dump.BIN"),
            "with several, the first by name"
        );
        fs::remove_file(root.join("bios/pcsx2/bios/My Dump.BIN")).unwrap();
        assert_eq!(
            PS2_BIOS.found(root).as_deref(),
            Some("b.rom"),
            "8 MiB is in range"
        );
    }

    #[test]
    fn the_ps1_bios_folder_is_bios_itself() {
        let root = Path::new("/Games");
        assert_eq!(PS1_BIOS.dir(root), root.join("bios"));
        assert_eq!(PS1_BIOS.dir(root).as_os_str(), "/Games/bios", "no trailing slash");
    }

    #[test]
    fn ppsspp_is_installed_only_once_its_library_and_its_assets_are_both_there() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        assert!(!is_installed(&data_dir, &PPSSPP));

        touch(&installed_path(&data_dir, &PPSSPP));
        assert!(!is_installed(&data_dir, &PPSSPP), "library only");

        fs::create_dir_all(assets_dir(&data_dir, &PPSSPP_ASSETS)).unwrap();
        assert!(is_installed(&data_dir, &PPSSPP));
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"").unwrap();
    }

    #[test]
    fn url_points_at_the_nightly_zip() {
        assert_eq!(
            url(&MESEN),
            "https://buildbot.libretro.com/nightly/linux/x86_64/latest/mesen_libretro.so.zip"
        );
    }

    fn wait_for_assets<F: FnMut() -> Option<T>, T>(mut f: F) -> T {
        for _ in 0..2000 {
            if let Some(value) = f() {
                return value;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for condition");
    }

    #[test]
    fn assets_install_unpacks_only_entries_under_ppsspp() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = testing::assets_zip(&[
            ("PPSSPP/flash0/font/a.pgf", b"font data"),
            ("PPSSPP/README.txt", b"readme"),
            ("not-ppsspp/other.txt", b"should be ignored"),
        ]);
        let mut download = testing::spawn_assets(data_dir.clone(), zip, None);

        let path = wait_for_assets(|| match download.poll() {
            DownloadState::Done(path) => Some(path),
            DownloadState::Failed(e) => panic!("assets install failed: {e}"),
            DownloadState::Running { .. } => None,
        });

        assert_eq!(path, assets_dir(&data_dir, &PPSSPP_ASSETS));
        assert_eq!(fs::read(path.join("flash0/font/a.pgf")).unwrap(), b"font data");
        assert_eq!(fs::read(path.join("README.txt")).unwrap(), b"readme");
        assert!(!path.join("other.txt").exists());
        assert!(!data_dir.join("not-ppsspp").exists());
    }

    #[test]
    fn assets_install_refuses_a_path_traversal_entry() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = testing::assets_zip(&[
            ("PPSSPP/good.txt", b"fine"),
            ("PPSSPP/../../evil.txt", b"escape attempt"),
        ]);
        let mut download = testing::spawn_assets(data_dir.clone(), zip, None);

        let path = wait_for_assets(|| match download.poll() {
            DownloadState::Done(path) => Some(path),
            DownloadState::Failed(e) => panic!("assets install failed: {e}"),
            DownloadState::Running { .. } => None,
        });

        assert_eq!(fs::read(path.join("good.txt")).unwrap(), b"fine");
        assert!(!dir.path().join("evil.txt").exists());
        assert!(!data_dir.join("evil.txt").exists());
    }

    #[test]
    fn assets_install_refuses_a_zip_with_no_ppsspp_entries() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = testing::assets_zip(&[("not-ppsspp/other.txt", b"should be ignored")]);
        let mut download = testing::spawn_assets(data_dir.clone(), zip, None);

        let message = wait_for_assets(|| match download.poll() {
            DownloadState::Failed(m) => Some(m),
            DownloadState::Done(_) => panic!("expected a failure"),
            DownloadState::Running { .. } => None,
        });

        assert_eq!(message, "The downloaded PPSSPP assets aren't valid.");
        assert!(!assets_dir(&data_dir, &PPSSPP_ASSETS).exists());
    }

    #[test]
    fn a_failed_assets_download_leaves_no_temp_files_behind() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let mut download = testing::spawn_assets_failing(data_dir.clone());

        let message = wait_for_assets(|| match download.poll() {
            DownloadState::Failed(m) => Some(m),
            DownloadState::Done(_) => panic!("expected a failure"),
            DownloadState::Running { .. } => None,
        });

        assert_eq!(
            message,
            "Couldn't download the PPSSPP assets. Check your internet connection."
        );
        assert!(!assets_dir(&data_dir, &PPSSPP_ASSETS).exists());
        let leftover = fs::read_dir(data_dir.join("system"))
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(leftover, 0, "no temp files left in system/");
    }

    fn wait_for_assets_result(download: &mut Download) -> Result<PathBuf, String> {
        wait_for_assets(|| match download.poll() {
            DownloadState::Done(path) => Some(Ok(path)),
            DownloadState::Failed(m) => Some(Err(m)),
            DownloadState::Running { .. } => None,
        })
    }

    #[test]
    fn dolphins_system_files_unpack_to_their_own_folder_and_nothing_escapes_it() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let zip = testing::assets_zip(&[
            ("dolphin-emu/license.txt", b"license"),
            ("dolphin-emu/Sys/GC/font.bin", b"font"),
            ("dolphin-emu/../evil.txt", b"escape attempt"),
            ("dolphin-emu//abs.txt", b"absolute"),
            (
                "dolphin-emu-extra/near.txt",
                b"a folder that only starts the same",
            ),
            ("PPSSPP/other.txt", b"another Core's assets"),
        ]);
        let mut download = testing::spawn_assets_of(&DOLPHIN_ASSETS, data_dir.clone(), zip, None);

        let path = wait_for_assets_result(&mut download).unwrap();

        assert_eq!(path, data_dir.join("system/dolphin-emu"));
        assert_eq!(fs::read(path.join("license.txt")).unwrap(), b"license");
        assert_eq!(fs::read(path.join("Sys/GC/font.bin")).unwrap(), b"font");
        assert!(assets_marker(&data_dir, &DOLPHIN_ASSETS).is_dir());
        let mut system: Vec<_> = fs::read_dir(data_dir.join("system"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        system.sort();
        assert_eq!(system, ["dolphin-emu"], "nothing else in system/, no temp files");
        let mut unpacked: Vec<_> = fs::read_dir(&path)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        unpacked.sort();
        assert_eq!(unpacked, ["Sys", "license.txt"]);
        assert!(!dir.path().join("evil.txt").exists() && !data_dir.join("evil.txt").exists());
        assert!(!Path::new("/abs.txt").exists());
    }

    #[test]
    fn assets_without_their_marker_folder_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = testing::assets_zip(&[("dolphin-emu/license.txt", b"but no Sys")]);
        let mut download = testing::spawn_assets_of(&DOLPHIN_ASSETS, data_dir.clone(), zip, None);

        let message = wait_for_assets_result(&mut download).unwrap_err();

        assert_eq!(message, "The downloaded Dolphin system files aren't valid.");
        assert!(!assets_dir(&data_dir, &DOLPHIN_ASSETS).exists());
        assert_eq!(fs::read_dir(data_dir.join("system")).unwrap().count(), 0);
    }

    /// A Core with both new kinds of requirement, as LRPS2's BIOS and Dolphin's system files.
    static NEEDS_BOTH: CoreInfo = CoreInfo {
        name: "Test",
        file: "test_libretro.so",
        options: &[],
        bios: Some(&PS2_BIOS),
        assets: Some(&DOLPHIN_ASSETS),
        saves: InGameSaves::SaveMemory,
        touch: None,
    };

    #[test]
    fn a_core_with_a_folder_bios_and_marked_assets_checks_both() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let root = dir.path().join("Games");
        assert_eq!(system_dir(&NEEDS_BOTH, &data_dir, &root), data_dir.join("system"));

        touch(&installed_path(&data_dir, &NEEDS_BOTH));
        fs::create_dir_all(assets_dir(&data_dir, &DOLPHIN_ASSETS)).unwrap();
        assert!(
            !is_installed(&data_dir, &NEEDS_BOTH),
            "the folder without its Sys"
        );
        fs::create_dir_all(data_dir.join("system/dolphin-emu/Sys")).unwrap();
        assert!(is_installed(&data_dir, &NEEDS_BOTH));

        assert!(!has_required_bios(&root, &NEEDS_BOTH));
        file_of(&root.join("bios/pcsx2/bios/scph39001.bin"), 4 * MIB);
        assert!(has_required_bios(&root, &NEEDS_BOTH));
        assert_eq!(found_bios(&root, &NEEDS_BOTH).as_deref(), Some("scph39001.bin"));
    }

    #[test]
    fn successful_install_is_executable_with_no_temp_files_left() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(core_zip()));

        let path = wait_for_done(&mut download);

        assert_eq!(path, installed_path(&data_dir, &SAMEBOY));
        assert!(is_installed(&data_dir, &SAMEBOY));
        assert_eq!(fs::read(&path).unwrap(), core_content());
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        assert_eq!(leftovers(&data_dir), Vec::<PathBuf>::new());
    }

    #[test]
    fn progress_is_reported_with_total_from_the_length() {
        let dir = tempfile::tempdir().unwrap();
        let zip = core_zip();
        let total = zip.len() as u64;
        let half = zip.len() / 2;
        let (first, second) = (zip[..half].to_vec(), zip[half..].to_vec());

        struct StepReader(mpsc::Receiver<Vec<u8>>);
        impl Read for StepReader {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                match self.0.recv() {
                    Ok(chunk) => {
                        let n = chunk.len().min(buf.len());
                        buf[..n].copy_from_slice(&chunk[..n]);
                        Ok(n)
                    }
                    Err(_) => Ok(0),
                }
            }
        }

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        // `Fetcher` is `Fn`, not `FnOnce`, but `Download::spawn` only ever calls it once, so a
        // `Mutex` just to make the one-shot `rx` reusable across calls is enough here.
        let rx = Mutex::new(Some(rx));
        let fetch: Fetcher = Arc::new(move |_url: &str| {
            let rx = rx.lock().unwrap().take().expect("fetcher called more than once");
            Ok((Box::new(StepReader(rx)) as Box<dyn Read + Send>, Some(total)))
        });

        let mut download = Download::spawn(&SAMEBOY, dir.path().to_owned(), fetch);
        tx.send(first.clone()).unwrap();

        let received = wait_for(|| match download.poll() {
            DownloadState::Running { received, total: t } if received > 0 => {
                assert_eq!(t, Some(total));
                Some(received)
            }
            DownloadState::Failed(e) => panic!("download failed: {e}"),
            _ => None,
        });
        assert_eq!(received, first.len() as u64);

        tx.send(second).unwrap();
        drop(tx);
        wait_for_done(&mut download);
    }

    #[test]
    fn missing_entry_is_the_invalid_library_error() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = zip_with("other_libretro.so", ELF_MAGIC);
        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(zip));

        assert_eq!(wait_for_failure(&mut download), invalid_library(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
    }

    #[test]
    fn a_path_traversal_entry_is_the_invalid_library_error() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = zip_with(&format!("../{}", SAMEBOY.file), ELF_MAGIC);
        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(zip));

        assert_eq!(wait_for_failure(&mut download), invalid_library(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
    }

    #[test]
    fn a_non_elf_entry_is_the_invalid_library_error() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = zip_with(SAMEBOY.file, b"not an elf file");
        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(zip));

        assert_eq!(wait_for_failure(&mut download), invalid_library(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
    }

    #[test]
    fn a_truncated_stream_is_the_incomplete_error() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = core_zip();
        let claimed_total = zip.len() as u64 + 1000;
        let mut download = Download::spawn(
            &SAMEBOY,
            data_dir.clone(),
            truncated_total_fetcher(zip, claimed_total),
        );

        assert_eq!(wait_for_failure(&mut download), incomplete(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
        assert_eq!(leftovers(&data_dir), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_fetch_error_is_the_connection_message() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let mut download = Download::spawn(&SAMEBOY, data_dir.clone(), failing_fetcher());

        assert_eq!(wait_for_failure(&mut download), connect_failed(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
    }

    #[test]
    fn an_http_error_says_the_build_server_couldnt_provide_the_core() {
        let dir = tempfile::tempdir().unwrap();
        let fetch: Fetcher = Arc::new(|_url: &str| Err(FetchError::Status(404)));
        let mut download = Download::spawn(&SAMEBOY, dir.path().to_owned(), fetch);

        assert_eq!(
            wait_for_failure(&mut download),
            "The libretro build server couldn't provide the SameBoy Core (HTTP 404)."
        );
    }

    #[test]
    fn a_local_disk_error_says_the_core_couldnt_be_saved() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the data directory should be: `cores/` can't be created under it.
        let data_dir = dir.path().join("not-a-dir");
        fs::write(&data_dir, b"").unwrap();
        let mut download = Download::spawn(&SAMEBOY, data_dir, fixed_reader_fetcher(core_zip()));

        let message = wait_for_failure(&mut download);
        assert!(
            message.starts_with("Couldn't save the SameBoy Core: "),
            "{message}"
        );
    }

    #[test]
    fn a_download_over_the_cap_stops_reading_and_fails() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let limits = Limits {
            download: 100 * 1024,
            core: MAX_CORE_BYTES,
        };
        let consumed = Arc::new(AtomicU64::new(0));
        let consumed_in_fetch = Arc::clone(&consumed);
        let fetch: Fetcher = Arc::new(move |_url: &str| {
            let zeros = Zeros {
                remaining: 10 * 1024 * 1024,
                consumed: Arc::clone(&consumed_in_fetch),
            };
            Ok((Box::new(zeros) as Box<dyn Read + Send>, None))
        });
        let mut download = Download::spawn_limited(&SAMEBOY, data_dir.clone(), fetch, limits);

        assert_eq!(wait_for_failure(&mut download), incomplete(&SAMEBOY));
        let consumed = consumed.load(Ordering::Relaxed);
        assert!(
            consumed <= limits.download + CHUNK as u64,
            "read {consumed} bytes past a {} byte cap",
            limits.download
        );
        assert!(!is_installed(&data_dir, &SAMEBOY));
        assert_eq!(leftovers(&data_dir), Vec::<PathBuf>::new());
    }

    #[test]
    fn the_real_caps_are_100_mb_downloaded_and_64_mb_unzipped() {
        assert_eq!(LIMITS.download, 100 * 1024 * 1024);
        assert_eq!(LIMITS.core, 64 * 1024 * 1024);
    }

    #[test]
    fn an_entry_declared_over_the_core_cap_is_the_invalid_library_error() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let limits = Limits {
            download: MAX_DOWNLOAD_BYTES,
            core: 1024,
        };
        // A valid-looking library, just bigger than the cap.
        let mut content = ELF_MAGIC.to_vec();
        content.resize(2048, 0);
        let zip = zip_with(SAMEBOY.file, &content);
        let mut download =
            Download::spawn_limited(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(zip), limits);

        assert_eq!(wait_for_failure(&mut download), invalid_library(&SAMEBOY));
        assert!(!is_installed(&data_dir, &SAMEBOY));
        assert_eq!(leftovers(&data_dir), Vec::<PathBuf>::new());
    }

    #[test]
    fn an_entry_unpacking_past_its_declared_size_stops_at_the_cap() {
        // A zip bomb lying about its size: declares 0 bytes, unpacks to far more than the cap.
        let consumed = Arc::new(AtomicU64::new(0));
        let bomb = Zeros {
            remaining: 10 * 1024 * 1024,
            consumed: Arc::clone(&consumed),
        };
        let result = read_core(&SAMEBOY, bomb, 0, 1024, &AtomicBool::new(false));

        // Compare lengths, not contents: a failure shouldn't print megabytes of zeros.
        assert_eq!(
            result.map(|content| content.len()),
            Err(invalid_library(&SAMEBOY))
        );
        assert_eq!(consumed.load(Ordering::Relaxed), 1025);
    }

    #[test]
    fn a_core_at_the_cap_is_read_whole() {
        let content = vec![7u8; 1024];
        let result = read_core(
            &SAMEBOY,
            Cursor::new(content.clone()),
            1024,
            1024,
            &AtomicBool::new(false),
        );
        assert_eq!(result, Ok(content));
    }

    #[test]
    fn extraction_stops_when_cancelled() {
        let result = read_core(
            &SAMEBOY,
            Cursor::new(vec![0u8; 16]),
            16,
            1024,
            &AtomicBool::new(true),
        );
        assert_eq!(result, Err(CANCELLED.to_string()));
    }

    #[test]
    fn a_panicking_worker_is_a_failure_not_running_forever() {
        let dir = tempfile::tempdir().unwrap();
        let fetch: Fetcher = Arc::new(|_url: &str| panic!("a bug in the worker"));
        let mut download = Download::spawn(&SAMEBOY, dir.path().to_owned(), fetch);

        assert_eq!(
            wait_for_failure(&mut download),
            "Couldn't download the SameBoy Core."
        );
    }

    /// Serves one chunk of zeros, but only once released; until then it blocks, like a read on a
    /// stalled connection. Tells `entered` when the worker is blocked in it.
    struct BlockedReader {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        served: bool,
    }
    impl Read for BlockedReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.served {
                return Ok(0);
            }
            let _ = self.entered.send(());
            let _ = self.release.recv();
            self.served = true;
            let n = buf.len().min(16);
            buf[..n].fill(0);
            Ok(n)
        }
    }

    #[test]
    fn dropping_while_blocked_returns_at_once_and_the_worker_cleans_up_after_itself() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let reader = Mutex::new(Some(BlockedReader {
            entered: entered_tx,
            release: release_rx,
            served: false,
        }));
        let fetch: Fetcher = Arc::new(move |_url: &str| {
            let reader = reader
                .lock()
                .unwrap()
                .take()
                .expect("fetcher called more than once");
            Ok((Box::new(reader) as Box<dyn Read + Send>, None))
        });
        // The worker thread owns the fetcher until it exits.
        let worker_alive = Arc::downgrade(&fetch);

        let download = Download::spawn(&SAMEBOY, data_dir.clone(), fetch);
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();

        // Release the reader well after the drop should have returned, so a drop that waits for the
        // worker shows up as slow instead of hanging the test.
        let releaser = thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            release_tx.send(()).unwrap();
        });
        let started = Instant::now();
        drop(download);
        let elapsed = started.elapsed();
        assert!(elapsed < Duration::from_millis(100), "drop took {elapsed:?}");

        // A retry while the old worker is still blocked installs fine, and the old worker's
        // cleanup doesn't touch it.
        let mut retry = Download::spawn(&SAMEBOY, data_dir.clone(), fixed_reader_fetcher(core_zip()));
        wait_for_done(&mut retry);

        releaser.join().unwrap();
        wait_for(|| worker_alive.upgrade().is_none().then_some(()));
        assert_eq!(leftovers(&data_dir), Vec::<PathBuf>::new());
        assert_eq!(
            fs::read(installed_path(&data_dir, &SAMEBOY)).unwrap(),
            core_content()
        );
    }

    #[test]
    fn two_concurrent_downloads_of_the_same_core_both_install_it_whole() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let zip = core_zip();
        // Both workers are past the fetch before either reads a byte, so their files overlap.
        let barrier = Arc::new(Barrier::new(2));
        let fetch: Fetcher = Arc::new(move |_url: &str| {
            barrier.wait();
            Ok((Box::new(Cursor::new(zip.clone())) as Box<dyn Read + Send>, None))
        });

        let mut first = Download::spawn(&SAMEBOY, data_dir.clone(), Arc::clone(&fetch));
        let mut second = Download::spawn(&SAMEBOY, data_dir.clone(), fetch);
        let first_path = wait_for_done(&mut first);
        let second_path = wait_for_done(&mut second);

        assert_eq!(first_path, second_path);
        assert_eq!(fs::read(&first_path).unwrap(), core_content());
        assert_eq!(leftovers(&data_dir), Vec::<PathBuf>::new());
    }

    /// The only real network access in this suite. Run once with `cargo test -- --ignored cores`.
    #[test]
    #[ignore]
    fn real_download_of_sameboy() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_owned();
        let mut download = Download::start(&SAMEBOY, data_dir.clone());

        let path = wait_for_done(&mut download);

        assert!(is_installed(&data_dir, &SAMEBOY));
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(ELF_MAGIC));
        assert_eq!(leftovers(&data_dir), Vec::<PathBuf>::new());
    }
}
