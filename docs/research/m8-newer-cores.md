# Cores for PS2, 3DS, GameCube and Wii (research for milestone M8)

Gathered 2026-11-01 from the Cores' repositories, the libretro docs and core-info files, and the
buildbot, plus a read of Game37's own frontend. "UNVERIFIED" marks what nobody has run yet.

## The Cores

| System | Core | Buildbot file (Linux x86_64) | Renders with | BIOS / system files |
|---|---|---|---|---|
| PS2 | LRPS2 ("ParaLLEl PS2", libretro's hard fork of PCSX2) | `pcsx2_libretro.so` (13 MB) | OpenGL core 3.3 (`pcsx2_renderer=OpenGL`); Vulkan and paraLLEl-GS need a Vulkan host Game37 lacks | A real PS2 BIOS, any file name, 4–8 MB, in `<system>/pcsx2/bios/`. No free one exists. |
| 3DS | Azahar (the upstream emulator's own core) | `azahar_libretro.so` (45 MB) | OpenGL core 4.3 (`citra_graphics_api=OpenGL`; Vulkan crashes on AMD, issue #2742) | None for homebrew and decrypted Games; encrypted dumps and `.cia` are refused |
| GameCube, Wii | Dolphin (libretro/dolphin, synced with upstream) | `dolphin_libretro.so` (16 MB) | OpenGL core 3.3 | Dolphin's `Sys` folder at `<system>/dolphin-emu/Sys`, from the buildbot asset `assets/system/Dolphin.zip` (3 MB) |

All three answer Game37's existing `GET_PREFERRED_HW_RENDER` (OpenGL core) and `SET_HW_SHARED_CONTEXT`,
so no Vulkan work is needed. Alternatives looked at and not chosen: PCEE2 and Play! (PS2; PCEE2's GL
path grabs the frontend's context outside the API and defaults to Vulkan, Play! links GLEW and ignores
the libretro folders), Citra and Panda3DS (3DS; Azahar is Citra's maintained successor).

## What each needs from Game37

**LRPS2**
- Reads `RETRO_DEVICE_ID_JOYPAD_MASK` every frame without asking `GET_INPUT_BITMASKS` first; Game37
  answers the mask only after that question, so every button would read as released.
- Draws only on the frontend thread, inside `retro_run` and the calls that drain its GS ring
  (serialize, unserialize, reset, unload): Game37 already makes the Core's context current there.
- In-game saves are memory cards it writes itself; with `pcsx2_shared_memory_cards=disabled` each Game
  gets `<save dir>/<file stem>.ps2`. No `SAVE_RAM`.
- Save states about 50 MB, fixed size. Fastmem installs a SIGSEGV/SIGBUS filter (UNVERIFIED alongside
  Rust's handlers; `pcsx2_fastmem=disabled` is the fallback).
- Content: `elf iso ciso cue gz chd cso zso m3u`, full path, no archives. `.elf` still needs a BIOS.
- 640×448 (NTSC) or 640×512 (PAL) times the upscale; 60/1.001 or 50 fps; 48000 Hz.

**Azahar**
- Layouts: top-bottom 5:6, side by side 3:1, large/small with a proportion (default 4, a 2:1 frame),
  single screen. Changed live through `GET_VARIABLE_UPDATE`. Swaps screens with `citra_swap_screen` or
  its own L3 hotkey. Touch through `RETRO_DEVICE_POINTER` over the whole frame.
- Reports 60 fps but runs at about 59.826; 32728 Hz.
- `retro_serialize` works only right after `retro_serialize_size` (Game37's `Core::serialize` already
  calls them in that order); states need one `retro_run` first (`MUST_INITIALIZE`) and break on every
  Core update (the header carries the Core's revision).
- Saves are files under `<save dir>/Azahar/` (sdmc, nand). No `SAVE_RAM`.
- Content: `3ds 3dsx elf axf cci cxi app`; decrypted only.

**Dolphin**
- In its default dual-core mode it calls `audio_sample_batch` and `input_state` from its own CPU
  thread. Game37's callbacks keep their state per thread, so it would run silent and without controls
  until those two use process-wide state (as `get_current_framebuffer` already does).
- Sets up its controllers only in `retro_set_controller_port_device`, which Game37 never calls.
- `retro_unserialize` fails until the first `retro_run` (`MUST_INITIALIZE`). States are large (40+ MB
  GameCube, 88+ MB Wii, UNVERIFIED).
- Saves: GameCube memory cards (GCI folders) and the Wii NAND under `<save dir>/User/`. No `SAVE_RAM`.
- Process-global Sys and User paths: switching between a GameCube and a Wii Game in one Game37
  process may keep the first one's paths (UNVERIFIED; issue #520 reports a hang on a second load).
- Wii Remote through a RetroPad is approximate (pointer on the right stick, shake on R1, tilt on the
  left stick).
- Content: `iso gcm rvz wbfs wia ciso gcz elf dol m3u`.

## Gaps in Game37 (from reading the code)

- `JOYPAD_MASK` answered only after `GET_INPUT_BITMASKS` (`src/libretro/callbacks.rs`).
- Audio and input callbacks are per-thread (`callbacks.rs` thread-local `SLOT`).
- No `SET_SERIALIZATION_QUIRKS`; the start state is restored before any `retro_run`
  (`src/session/sdl.rs`).
- No `retro_set_controller_port_device` (`src/libretro/ffi.rs`).
- `SET_MESSAGE`/`SET_MESSAGE_EXT` refused, so a Core's reason for failing (missing BIOS, encrypted
  dump) never reaches the player.
- `keeps_own_saves` and `stick_is_dpad` are per-Core special cases for PPSSPP and N64 (`src/cores.rs`).
- The BIOS model is "any of these file names directly in `bios/`" with PS1 wording
  (`cores.rs`, `launch.rs`, `screens/settings/bios.rs`, `screens/setup/cores.rs`).
- The assets download handles only `PPSSPP.zip` (`cores.rs`).
- Touch and screen swap are wired to melonDS DS (`src/session/sdl.rs`, `input.rs`).
- Core options can't change during play (`GET_VARIABLE_UPDATE` always false).
- No disk control (multi-disc PS2/GameCube Games).
- PS2, 3DS, GameCube and Wii are External throughout: `systems.rs`, `emulators.rs` (PCSX2, Azahar,
  Dolphin), Setup's emulator step, the missing-emulator notices, tests and smoke shots; and on the
  paused `windows` branch, its emulator detection.

## Test content (in `.testdata/`, never committed)

- PS2: wLaunchELF `BOOT.ELF` (Academic Free License 2.0, github.com/ps2homebrew/wLaunchELF) as
  `games/ps2/wLaunchELF (Homebrew).elf`; needs the owner's PS2 BIOS.
- 3DS: ftpd 3.2.1 `ftpd.3dsx` (GPL-3.0, github.com/mtheall/ftpd) as `games/3ds/ftpd (Homebrew).3dsx`.
- GameCube, Wii: FCE Ultra GX 4.0.3 (GPL, github.com/dborth/fceugx) as
  `games/gc/FCE Ultra GX (Homebrew).dol` and `games/wii/FCE Ultra GX (Homebrew).dol`; it boots to a menu.
- Cores in `data/cores/`, Dolphin's `Sys` in `data/system/dolphin-emu/Sys`.

## Sources

- LRPS2: https://github.com/libretro/parallel-ps2 (libretro/main.cpp, pcsx2/PAD/PAD.cpp, MTGSOwner.h,
  GSDeviceOGL.cpp), https://docs.libretro.com/library/lrps2/,
  https://github.com/libretro/libretro-core-info/blob/master/pcsx2_libretro.info
- Azahar: https://github.com/azahar-emu/azahar (src/citra_libretro/, src/core/savestate.cpp),
  https://docs.libretro.com/library/azahar/, issue https://github.com/azahar-emu/azahar/issues/2742
- Dolphin: https://github.com/libretro/dolphin (Source/Core/DolphinLibretro/), https://buildbot.libretro.com/assets/system/Dolphin.zip
- Game37: src/libretro/, src/session/, src/cores.rs, src/systems.rs, src/emulators.rs
