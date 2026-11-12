# Cemu and RPCS3 in Game37's frontend (Phase 0 spikes)

Phase 0 of [the roadmap](../roadmap-external-systems.md), run on 2026-11-11 on the `spike-wiiu-ps3`
branch, which is not for `main`. Both Cores ran through a real `Session` (`Session::start`, `tick`,
`draw`, drop) on the renderer the tests use, with the buildbot nightlies of 2026-11-11. The machine
was the owner's Arch laptop: 16 threads, 26 GB RAM, a Wayland session. "UNVERIFIED" marks what
these runs could not show.

## How it was run

- The branch adds `CEMU` and `RPCS3` entries in `src/cores.rs`, makes Wii U and PS3 `Kind::Embedded`
  (Wii U also takes `.rpx`, PS3 `.elf`), and adds one ignored test, `spike_run_a_game` in
  `src/session/sdl.rs`. The test is driven by environment variables: `SPIKE_SYSTEM`, `SPIKE_GAME`,
  `SPIKE_FRAMES`, `SPIKE_EVERY`, `SPIKE_OUT`, `SPIKE_PRESS`, `SPIKE_HOLD`, `SPIKE_ROOT`,
  `SPIKE_OPTS` (extra Core options) and `SPIKE_VIDEO_DRIVER`.
- The Cores and test content are in `.testdata/spike/`, which is gitignored and kept apart from
  `.testdata/games` so the smoke shots don't change:
  - Wii U: DrawU 1.0 (`.rpx`, GPL-3.0, github.com/misterfonka/drawu) and Tank Trap 1.0 (`.wua`, MIT,
    github.com/crementif/untitledsandgame).
  - PS3: `tests/rsx/primitives/primitives.ppu.elf` from RPCS3's own ps3autotests (GPL-2.0), with its
    expected picture.
- Core sizes: Cemu is 10 MB zipped and 27 MB unzipped. RPCS3 is 33 MB zipped and **98 MB (93.6 MiB)
  unzipped**, over `MAX_CORE_BYTES` (64 MiB).

## Cemu (Wii U): works

| Check | Result |
|---|---|
| OpenGL host | Asks for OpenGL 4.5 core with `cemu_gpu_api=OpenGL`. Game37's SDL host gives it one with a 5120×2880 framebuffer, and frames are 1280×720 RGBA. **No GLX host and no Vulkan needed.** |
| Picture on the offscreen renderer | DrawU's text menu and Tank Trap's animated title and gameplay are drawn correctly. |
| Picture on a real Wayland window (`SDL_VIDEO_DRIVER=wayland`) | Correct, about 9000 colours shown. The RetroArch docs' "black screen on Wayland" doesn't happen with Game37's host. |
| Start | 0.1–0.2 s to load. The first 300 frames take about 8.7 s while shaders compile, and later frames run at about 58 per second in an unpaced loop. `retro_run` keeps itself to real time (120 frames take 1.95 s). |
| Input | The Core reads RetroPad port 0 bit by bit (ids 0–15) and analog (device 5), plus the pointer (device 6). Game37's answers arrive: A on Tank Trap's SANDBOX starts a game, and the d-pad moves the menu. DrawU ignores input on its own side (UNVERIFIED why, likely the homebrew itself). Port 0 is the Wii U GamePad. |
| Sound | Arrives at the stated rate (about 208k samples per 120 frames against 192k expected at 48 kHz), from Cemu's own threads. Silent on the title screen, peak 26823 in game. |
| Quit | 0.02–0.14 s, clean. |
| Files | Under the system directory (`<Games Root>/bios/` for a Core without assets): `Cemu/` with **173 graphic packs (13 MB) that the Core unpacks itself**, a `keys.txt` template, `log.txt`, `shaderCache/`, `sdcard/`, `titles/` and `account_name.txt`. Saves go in `<Games Root>/saves/wiiu/Cemu/`; `mlc01/` hadn't appeared yet in 300 frames. |
| Log | One `glError=0x500` on the first backbuffer draw, then none. |

**Consequences for Phase 1**
- The OpenGL path is enough, so decision D4 (Vulkan) isn't needed for Wii U.
- Cemu's system directory shouldn't be the player's BIOS folder. Graphic packs, logs and the shader
  cache are replaceable data, like Dolphin's `Sys`. Give Cemu its system directory under the data dir
  (as `system_dir` does for Cores with assets), and copy or link the player's `keys.txt` from
  `<Games Root>/bios/` into it. The template `keys.txt` with its example key must never be mistaken
  for the player's.
- Two screens weren't tried (the default layout shows the TV screen only). The GamePad's touch
  through the pointer is UNVERIFIED.

## RPCS3 (PS3): blocked on the BIOS

RPCS3 boots nothing without the PS3 BIOS, homebrew included: `firmware_missing`. No `PS3UPDAT.PUP`
was available, so the picture, sound, input, frame rate and quit time of a running Game are all
UNVERIFIED. What the runs did show:

| Check | Result |
|---|---|
| Loading the library | Loads, initialises and unloads in Game37's process without crashing (LLVM, its signal handling and TSC calibration included). TSC isn't a usable clock source on this machine; RPCS3 logs that and carries on. |
| Default renderer (Vulkan) | `retro_load_game` fails and Game37 shows its generic "Couldn't start the game". The Core gives its reason (`firmware_missing`) only through the log callback. It asked for no HW context before failing, so whether its Vulkan renderer needs anything from Game37 is UNVERIFIED. |
| `rpcs3_renderer=OpenGL` | Asks for **OpenGL 4.6 core**, which Game37's host gives (a 3840×2160 framebuffer), and starts in 0.8 s. Then **`retro_load_game` succeeds although the boot failed**: the session runs with 0×0 frames, no sound and no input queries, and quits in 6 ms. A player would see a black screen and no message. |
| Files | Even a failed boot writes 3.8 MB into `<system dir>/rpcs3/`: `dev_hdd0/` (with `savedata/`), `dev_hdd1/`, `dev_flash*/`, `cache/` (PPU code per Game), `config.yml`, `vfs.yml`, `rpcs3_detailed.log`, and a `savestates/` folder (upstream's experimental feature; the Core says states aren't supported). |

**Consequences for Phase 2**
- Game37's BIOS check before loading (a `BiosRule` for `PS3UPDAT.PUP`, or an installed `dev_flash`)
  is required, not optional: on the OpenGL renderer a missing BIOS is otherwise a silent black screen.
- Other boot failures on OpenGL (a missing disc key, a broken dump) would look the same. Game37 needs
  to catch them: read the Core's log for `booting … failed: <reason>`, or treat "no frame for N
  seconds after start" as a failed start.
- `vfs.yml` exists and can be written before the first load to move `dev_hdd0` (saves) to
  `<Games Root>/saves/ps3/`, as planned. The rest (`cache/`, `dev_flash/`, logs) belongs under the
  data dir, as for Cemu.
- `MAX_CORE_BYTES` must rise above 98 MB.

## Still to run (needs `PS3UPDAT.PUP`)

Put Sony's `PS3UPDAT.PUP` (free from PlayStation's system software page, never committed) somewhere
local, then:

```bash
SPIKE_BIOS=/path/to/PS3UPDAT.PUP SPIKE_ROOT=/tmp/ps3-spike SPIKE_SYSTEM=ps3 \
  SPIKE_GAME="primitives (Test).elf" SPIKE_FRAMES=1200 SPIKE_EVERY=300 SPIKE_OUT=/tmp/ps3-spike/out \
  cargo test --lib spike_run_a_game -- --ignored --nocapture
```

Run it once on the default renderer and once with `SPIKE_OPTS=rpcs3_renderer=OpenGL`, and compare
with `.testdata/spike/ps3/primitives.expected.png`. Then check how long the BIOS install on the first
load takes, the quit time with a warm cache, and the files written.
