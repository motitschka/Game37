# A Switch Core for Game37 (research, 2026-11-11)

Switch has no Core on the libretro buildbot, but one exists elsewhere: **suyu's libretro Core**,
shipped with every release of `suyu-emu/suyu-main` (GPL-3.0, formerly
`dougchansan/suyu-v0.0.4`, pushed daily). This file covers what that Core is, what it needs from
Game37 and what Game37 needs from it, from its release files and its source
(`src/libretro_core/retro_core.cpp` at tag `v14.1.0`), then from running it in Game37. The outcome is
ADR 0019; the results of the runs are at the end.

## The Core

| | |
|---|---|
| Project | suyu, a yuzu/Eden-derived Switch emulator. Its libretro Core first shipped with 0.0.4 (2026-09-12; reported not working), then from 0.0.11 (2026-09-25) under new maintainers. Forum users report commercial Games running in RetroArch on 0.0.12. |
| Release used | v14.1.0 (2026-10-12): `suyu-libretro-linux-x86_64.tar.gz`, 13 MB, holding `suyu_libretro.so` (43 MB) and licence files |
| Content | `nsp xci nca nro`, full path. Homebrew `.nro` boots without keys; `.nsp`/`.xci`/`.nca` need a usable NCA header key |
| Video | Vulkan on its own device, rendered "headless" to a CPU buffer and handed over as **XRGB8888 1280×720** frames (repeats sent as dupes). Needs no OpenGL or Vulkan context from the frontend, so ADR 0003 stands |
| Sound | Option "Audio Output": **Host (default)** opens its own audio device and plays directly, bypassing the frontend. Frontend (libretro) sends about one frame of samples per `retro_run`, which its comments say arrives in bursts |
| Input | RetroPad port 0 only: 16 buttons and both sticks, bridged to suyu's virtual gamepad (player 1, Pro Controller) |
| Options | `suyu_resolution`, `suyu_use_docked`, `suyu_cpu_accuracy`, `suyu_fastmem`, `suyu_anti_aliasing`, `suyu_scaling_filter`, `suyu_audio_output`, `suyu_show_perf` (on by default; draws an FPS message), `suyu_online_enable`, `suyu_username`, `suyu_token` |
| Save States | None (`retro_serialize_size` is 0; declares `SERIALIZATION_QUIRK_INCOMPLETE`) |
| Reset | `retro_reset` not implemented |
| Updates | One update per session, through the environment variable `SUYU_LIBRETRO_UPDATE_PATH` (base `.nsp`/`.xci` as content, its update verified read-only, nothing installed). DLC: not handled (UNVERIFIED) |

## Problems for Game37

1. **It doesn't load on Arch.** The Core was built on Ubuntu 24.04 and links `libboost_filesystem`
   and `libboost_context` 1.83, `libavcodec.so.60` and `libavutil.so.58`. Arch has Boost 1.92 and
   FFmpeg 9 (`libavcodec.so.63`); RetroArch users on Arch hit the same "cannot open shared object".
   Arch's Boost.Context `.so` doesn't even export `jump_fcontext`/`make_fcontext` (its static
   `libboost_context.a` does). FFmpeg is used only for decoding in-game video (22 functions), so a
   Game with movies would also hit FFmpeg 6/9 struct differences if the Core were just re-pointed.
2. **It writes outside Game37's folders.** On desktop Linux it ignores the frontend's save and
   system directories (only the Android build uses them) and keeps NAND (In-game Saves), keys,
   config and caches in suyu's own XDG folders (`~/.local/share/suyu` …). It also copies any
   `prod.keys`/`title.keys`/`console.keys` it finds in `~/.local/share/{yuzu,sudachi,citron,Ryujinx}/keys`.
   That breaks FR-EMB-7 (saves under `<Games Root>/saves/<system>/`), and the tests rule (no real home
   folders) for any test that loads it.
3. **The Pause Menu wouldn't pause.** The emulated Switch runs on its own threads from load to unload
   (`System::Run()`); `retro_run` only polls input and picks up the latest frame. Nothing pauses it
   when the frontend stops calling `retro_run` (UNVERIFIED in a run). With Host audio, sound would go on
   under the Pause Menu too.
4. **Sound bypasses Game37** by default (volume, muting, the Pause Menu). The libretro route exists but
   is described as bursty; Game37's own rate control may or may not smooth it (UNVERIFIED).
5. **Not from the buildbot.** ADR 0004 says Cores come from the libretro buildbot. This one comes from a
   GitHub release of a community fork that has been renamed, moved and re-versioned (0.0.14 → 14.0.1)
   within a month.
6. **Updates are per Game.** Many Switch Games need their update file to start. Game37 lists every
   `.nsp`/`.xci` in `switch/` as a Game, so updates and DLC would show up as Games, and the Core takes
   an update only through an environment variable read at load.

## Test content

Space NX 1.1 (`spacenx.nro`, MIT, github.com/vgmoose/space-nx) in `.testdata/spike/switch/`, header
checked (`NRO0`). No keys or firmware are needed for it.

## Spike, once the owner allows running the Core

The `spike-wiiu-ps3` branch already maps Switch to a `SUYU` Core entry (software video, no GL).
Loading the Core needs either compatibility libraries or a Core built for Arch (problem 1), and the
run must redirect `XDG_DATA_HOME`, `XDG_CONFIG_HOME` and `XDG_CACHE_HOME` to temporary folders so it
can't touch the real ones (problem 2). Things to check: picture, input, both audio outputs, quit
time, files written, and whether the game keeps running while `retro_run` isn't called.

## Results (2026-11-11, after the owner's go-ahead)

- **The release Core, made to load on Arch** (Boost.Context and FFmpeg shims, never shipped): Space NX
  and the Homebrew App Store draw correctly through a Game37 Session (XRGB8888, 1280×720, right
  colours). Input reaches the Core but never the games, the emulated Switch keeps running while
  `retro_run` isn't called, and everything is written to suyu's XDG folders.
- **Built from source with Game37's patches** (`patches/suyu/`, `scripts/build-suyu-core.sh`): it
  links its own Boost and Arch's FFmpeg, and needs no shims. A temporary diagnostic showed player 1
  connected as a Pro Controller with A read as pressed while held. Space NX itself still ignores
  input: it is 2018 homebrew built on libtransistor, whose controller reading predates what suyu
  emulates. After 3 s without `retro_run` the picture moves about one frame instead of 3 s worth,
  and the log shows the watchdog pausing and resuming. With XDG pointed at an empty folder, nothing
  is written there: config, caches and logs go to the system folder, the NAND and SD card to the
  save folder.
- **Build notes:** GCC 16 needs a false positive silenced in `cheat_engine.cpp` (in the patch);
  suyu's bundled prebuilt FFmpeg isn't PIC, so the build uses the system's.
- **Not yet seen:** a commercial Game (the owner's own dump and keys), sound from a Game that makes
  any (the frontend audio route delivers samples, but the test Games are silent), and performance.
