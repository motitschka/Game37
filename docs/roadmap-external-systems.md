# Roadmap: Wii U, Switch, PS3 and PS4 inside Game37

How the four External Systems could become Embedded Systems, the way PS2, 3DS, GameCube and Wii did in
M8 (ADR 0016). This is a proposal, not a decision: nothing here changes ADR 0001, 0003, 0008 or 0016
until the owner accepts a phase and it gets its own ADR. Gathered 2026-11-11 from the libretro
buildbot, the libretro core-info files and docs, and a read of Game37's frontend. "UNVERIFIED" marks
what nobody has run with Game37 yet.

## Where each System stands

| System | Standalone Emulator today | Core on the buildbot | Renders with | Save States | Verdict |
|---|---|---|---|---|---|
| Wii U | Cemu | **Yes**: `cemu_libretro.so` (Cemu 2.6, MPL-2.0; 10 MB zip, 25 MB unzipped) | OpenGL 4.5 core **or** Vulkan | Never (Cemu can't serialize) | Can be embedded now |
| PS3 | RPCS3 | **Yes**: `rpcs3_libretro.so` (RPCS3 0.0.38, GPLv2; 32 MB zip, 93 MB unzipped) | Vulkan on its own device, frames handed over in memory; **or** OpenGL 4.5 in the frontend's context | No | Can be embedded now |
| Switch | Ryubing | No (none for Ryubing, Eden or Citron) | n/a | n/a | Needs a port |
| PS4 | shadPS4 | No | Vulkan only | n/a | Needs a port |

Both new Cores are community ports credited to "WizzardSK" alongside the upstream teams, and both
nightlies were rebuilt on 2026-11-11. How long they have existed and how closely they follow upstream
is UNVERIFIED. The buildbot also has `vita3k_libretro.so` (PS Vita, Vulkan ≥ 1.1 only), which is out
of scope here but shows where Cores for newer Systems are heading.

## What changes for the player

Embedding brings Wii U and PS3 the Pause Menu, one way to quit, Playtime measured from the Core, and
Game37's own controller handling. Unlike M8, it **does not** bring Save States or a Resume State: neither
Core can serialize. That is the main trade-off against keeping Cemu and RPCS3 as Standalone Emulators,
and the owner has to decide it (decision D1 below).

---

## Phase 0: Spikes (small, before any commitment)

**Status (2026-11-11):** run on the `spike-wiiu-ps3` branch; findings in
[research/wiiu-ps3-spikes.md](research/wiiu-ps3-spikes.md).
- **Cemu: works.** Picture (offscreen and on a real Wayland window), input, sound and quit all work
  on Game37's existing OpenGL host, with no GLX host or Vulkan needed. Wii U's main risk is gone.
- **RPCS3: blocked on the PS3 BIOS.** The Core loads and unloads safely in Game37's process and gets
  an OpenGL 4.6 context. Without `PS3UPDAT.PUP` it boots nothing, so a running Game is untested. On
  its OpenGL renderer it reports a failed boot as a successful load, which would leave the player on
  a silent black screen.

The plan as written before the spikes:

Load each Core in Game37's real frontend with homebrew test content and record what breaks, the way
`docs/research/m8-newer-cores.md` did for M8. Nothing merges to `main` from this phase.

**Cemu**
- Does the OpenGL 4.5 path draw in Game37? The core-info notes say it "renders black" on Wayland in
  RetroArch. Game37 already keeps a GLX host for PPSSPP (`src/session/glx.rs`), which may be the fix.
  If neither works, the only remaining path is Vulkan, which Game37 doesn't have (see Phase 4). **This
  is the main risk to Wii U.**
- Which threads call `video_refresh`, `audio_sample_batch` and `input_state` with the multicore
  recompiler? The audio and input state is already process-wide since M8 (`callbacks.rs` `SHARED`),
  but the GL calls still need checking.
- Two screens (TV and GamePad): which layouts the Core's options give, and whether touch arrives
  through `RETRO_DEVICE_POINTER` as it does for Azahar.

**RPCS3**
- Vulkan renderer with the frame handed over in memory: does it need a Vulkan HW context from the
  frontend at all, or only a software `video_refresh`? If it needs none, Game37 can use the renderer
  RPCS3 actually maintains. Otherwise use the OpenGL 4.5 path, which Game37's GL host accepts.
- LLVM and RPCS3's memory handling install SIGSEGV handlers alongside Rust's (UNVERIFIED, as LRPS2's
  fastmem was).
- Unloading: the docs say closing a Game with a large cache "can take a minute or more". What Game37's
  quit path does during that time.
- Installing the BIOS (`PS3UPDAT.PUP`) on the first load: how long it takes and what the player sees.

**Test content** (in `.testdata/`, never committed): a Wii U homebrew `.rpx` built from devkitPro's
`wut` samples; a PS3 homebrew `.self`/`.elf` from PSL1GHT samples. RPCS3 needs an installed BIOS even
for homebrew, so its tests follow the `needs_ps2_bios_…` pattern: `needs_ps3_bios_…`.

**Done when** both findings files exist and decisions D1 and D2 are taken.

---

## Phase 1: Wii U inside Game37 (medium, M8-sized for one System)

Wii U goes first: its Core is smaller, homebrew needs no BIOS, and its two-screen handling reuses what
DS and 3DS already have.

- `systems::SYSTEMS`: Wii U becomes `Kind::Embedded`. Extensions stay `wua wud wux`. A folder Game
  (`code/ content/ meta/`) is loaded through its `code/<name>.rpx`, as Cemu is launched today.
- `cores.rs`: a `CEMU` entry with options pinned: `cemu_gpu_api=OpenGL`, internal resolution, async
  shaders on, and a screen layout matching the DS/3DS defaults.
- Cemu's system directory goes under the data dir, not `<Games Root>/bios/`: the Core unpacks 173
  graphic packs (13 MB), logs and a shader cache there (Phase 0). The player's `keys.txt` stays in
  `<Games Root>/bios/` and is linked in.
- **Cores without Save States.** This is new for an Embedded System. The Pause Menu, the Options Menu
  and the quit path skip Save States and the Resume State for a Core that says it has none (one flag
  on `CoreInfo`, not a System check). The Details Page already hides Save State entries for External
  and Upcoming Systems (`src/screens/details.rs`); generalise that instead of copying it.
- Saves: Cemu keeps its own (`mlc01/` under the save directory, so `<Games Root>/saves/wiiu/Cemu/mlc01/`),
  like the M8 Cores. Updates and DLC installed there are found by the Core.
- Keys: encrypted `.wud`/`.wux` need `keys.txt` in `<system dir>/Cemu/`. Setup and Settings → BIOS say
  where it goes, as they do for PS2's BIOS. It is never committed (NFR-6), and a missing key reaches
  the player through `SET_MESSAGE`.
- GamePad: two screens, touch on the GamePad screen through the mouse, the layout switched with the
  existing screen-swap control. Gyro is not supported by the Core.
- Remove Cemu from `emulators.rs`, Setup's emulator step, the missing-emulator notices, tests and smoke
  shots (as M8 did for PCSX2, Azahar and Dolphin).
- New smoke shots for a Wii U Game playing and for the Pause Menu without Save States.

**Done when** a Wii U homebrew plays inside Game37 with sound and controls, quits through the Pause
Menu, records Playtime, and its saves are under `saves/wiiu/`.

## Phase 2: PS3 inside Game37 (large)

- Raise `MAX_CORE_BYTES` (64 MB) in `src/cores.rs`: RPCS3 is 93 MB unzipped. Keep a cap, for example
  128 MB, and update the comment that names Azahar as the largest Core.
- `systems::SYSTEMS`: PS3 becomes `Kind::Embedded`. Folder Games load through `PS3_DISC.SFB` (disc
  dump) or `PARAM.SFO` (extracted). These files are already what the folder markers look for. `.iso`
  could join the extensions; encrypted ISOs need a `.dkey` next to them. `.pkg` stays out (FR-LIB-5).
- A failed boot must not become a black screen. On the OpenGL renderer, `retro_load_game` succeeds
  even when the boot fails (Phase 0). Check the BIOS before loading, and treat the Core's logged
  `booting … failed: <reason>`, or no frame soon after the start, as a failed start with a message.
- BIOS: `PS3UPDAT.PUP` from Sony's update page, in the system directory. The Core installs it on the
  first load, which takes a while, so Game37 shows a "Preparing PS3…" Message instead of a frozen
  screen. Settings → BIOS gets a PS3 row.
- Storage: RPCS3 writes `dev_hdd0/` (game data, **saves**), `dev_hdd1/` and `cache/` under
  `<system dir>/rpcs3/`. Saves don't belong in the BIOS folder: point `dev_hdd0` at
  `<Games Root>/saves/ps3/` through the Core's `vfs.yml`, written before the first load. `cache/`
  grows without limit, so Settings could show its size and offer to clear it (a later nicety).
- Quit: unloading can take a minute. The Pause Menu's Quit shows a "Closing…" screen and waits. It
  never kills the Core, since only one Core may load per process and the shader cache must be written.
- Options pinned: renderer (from Phase 0), the per-game settings database on, patches off by default.
- Remove RPCS3 from `emulators.rs` and the rest, as for Cemu.

**Done when** a PS3 homebrew plays inside Game37, a second Game starts after the first one quits, and
saves are under `saves/ps3/`.

---

## Phase 3: Switch (large, now the priority)

**Status (2026-11-11): done** on branch `switch-embedded`, see ADR 0019. S0–S3 below are carried out:
the Core is built from suyu's pinned source with `patches/suyu/` (`scripts/build-suyu-core.sh`), which
fix input, folders and pausing; Switch is Embedded, Ryubing is gone. Left open: Game updates and
DLC (S2's last point and S3's library point), a commercial Game with the owner's own keys, and
sending the patches upstream.

**Update 2026-11-11:** a Switch Core exists after all, outside the buildbot: suyu's libretro Core,
shipped with every release of `suyu-emu/suyu-main` (GPL-3.0). No port from scratch is needed. Adopting
it is still a large job, because the Core was made for RetroArch on Ubuntu, not for Game37. Details:
[research/switch-core.md](research/switch-core.md).

**S0. Spike (blocked on the owner's OK).** Run Space NX (MIT homebrew) through a Game37 Session, as
Phase 0 did for Cemu. Running a community fork's binary needs the owner's go-ahead. Check picture,
input, both audio outputs, quit, files written (with XDG folders redirected), and whether the Game keeps
running while `retro_run` isn't called.

**S1. A Core that loads on Arch.** The release's Core links Ubuntu 24.04's Boost 1.83 and FFmpeg 6.
- Preferred: build `suyu_libretro.so` from the release's source and dependency archives, with FFmpeg
  bundled and Boost linked statically, so it loads on any distro. That needs `cmake`, `ninja`, Boost
  headers and Vulkan headers on the build machine.
- Or ask upstream to publish a portable Linux libretro build (they already build an AppImage).
- Either way, ADR 0004 needs an amendment: this Core comes from a pinned release (version and
  SHA-256) of suyu, not from the buildbot. It's still downloaded on first use and never committed.

**S2. Core changes Game37 needs**, as upstream pull requests or, failing that, patches Game37 carries:
- Use the frontend's save and system folders on Linux (the Android build already does), so In-game
  Saves (the NAND) land in `<Games Root>/saves/switch/` and nothing is written to `~/.local/share/suyu`.
  Stop adopting keys from other emulators' folders.
- Pause the emulated Switch when the frontend stops calling `retro_run` (or offer a pause), so Game37's
  Pause Menu pauses the Game and its sound.
- Smooth sound through libretro, so Game37 controls volume, muting and the Pause Menu. Use Host audio
  only if that can't be made smooth.
- Take a Game's update (and DLC) as a path Game37 passes at load, not only as an environment variable.

**S3. Game37 integration** (like Phase 1):
- Switch becomes `Kind::Embedded` with a `SUYU` Core entry: software video, no GL host, `suyu_show_perf`
  off.
- Keys: the player's own `prod.keys` (and `title.keys`) in `<Games Root>/bios/switch/`, copied into the
  Core's system folder. They are never committed, never logged, and Setup says where they go. A missing
  key is checked before load: an `.nsp`/`.xci` without keys gets a clear message, while `.nro` homebrew
  needs none.
- Cores without Save States (shared with Phase 1); no Restart (no `retro_reset`).
- Library: update and DLC files in `switch/` aren't Games of their own. Tell them apart by their title
  ID (an update is the base ID with `800` at the end), list the base Game, and pass its newest update
  at load.
- Ryubing is removed from `emulators.rs`, Setup and the notices, revising ADR 0008's "Switch always
  goes to Ryubing". Whether Ryubing stays as a fallback while the Core is young is decision D3.

**Risks.**
- The fork is young and fast-moving: renamed, moved and re-versioned (0.0.14 → 14.0.1) within a
  month. Pin a version, and test each bump with the spike Games.
- Nintendo has had Switch emulator projects taken down. Game37 never ships the Core, keys or firmware;
  it downloads a pinned GPL-3 release. If upstream disappears, the pinned Core keeps working where it
  is installed, but new installs fail. Building the Core ourselves (S1) also covers that.
- Performance: every frame is copied from the GPU to the CPU and then uploaded again (1280×720 docked,
  more at higher resolutions). UNVERIFIED until S0.

## Phase 4: PS4 (extra large, or waiting)

shadPS4 is Vulkan-only and has no Core. There are two ways to embed it:

1. **Port it the way RPCS3's Vulkan path works:** render on the Core's own Vulkan device and hand each
   finished frame to the frontend in memory. Game37 already reads every frame back, so this needs no
   Vulkan in Game37 and leaves ADR 0003 alone. It costs a GPU-to-CPU copy per frame. The port also has
   to load shadPS4's `sys_modules` from the system directory. Months of work, then upkeep.
2. **Give Game37 a Vulkan HW context** (`RETRO_HW_CONTEXT_VULKAN`), reversing ADR 0003. That is a large
   change on its own, but it is also what Cemu needs if its OpenGL path fails in Phase 0, and what
   Vita3K, LRPS2's paraLLEl-GS and the Vulkan renderers of the M8 Cores need. Worth its own ADR the
   first time a Core needs it, not before.

Until then, PS4 stays External with shadPS4.

---

## Decisions for the owner

- **D1: Wii U and PS3 without Save States.** Are the Pause Menu, one way to quit and Playtime from the
  Core worth having, when Save States never come? If not, they stay External and Phases 1–2 stop.
- **D2: Replace outright or keep a way back?** ADR 0016 replaced PCSX2, Azahar and Dolphin with no
  fallback, and ADR 0008 forbids "open in standalone". The same rule here means dropping Cemu and RPCS3.
- **D3: Switch through suyu's Core, and Ryubing.** Adopt suyu's Core (S0–S3), and decide whether
  Ryubing goes at once (ADR 0008) or stays as a fallback while the Core is young. PS4: port or wait.
- **D4: Vulkan in Game37.** Only if Phase 0 shows Cemu can't draw through OpenGL in Game37, or when a
  wanted Core is Vulkan-only.

## Order and size

| Phase | Size | Depends on | Unblocks |
|---|---|---|---|
| 0 Spikes | Small | nothing | D1, D2, D4 |
| 1 Wii U | Medium | Phase 0, D1, D2 | the "Core without Save States" support PS3 reuses |
| 2 PS3 | Large | Phase 1 | — |
| 3 Switch | Large (suyu's Core exists) | S0 run, D3 | — |
| 4 PS4 | XL or wait | D3 (D4 if not done the RPCS3 way) | — |

Phases 1 and 2 could ship as a 0.3.0 beta, as M8 shipped as 0.2.0. Phases 3 and 4 run on their own
schedule and don't block 1.0 or M7.

## Sources

- Buildbot listing: <https://buildbot.libretro.com/nightly/linux/x86_64/latest/>
- Core info: [cemu_libretro.info](https://github.com/libretro/libretro-core-info/blob/master/cemu_libretro.info),
  [rpcs3_libretro.info](https://github.com/libretro/libretro-core-info/blob/master/rpcs3_libretro.info),
  [vita3k_libretro.info](https://github.com/libretro/libretro-core-info/blob/master/vita3k_libretro.info)
- libretro docs: [Cemu](https://docs.libretro.com/library/cemu/), [RPCS3](https://docs.libretro.com/library/rpcs3/)
- Switch emulator legal situation (secondary, low-quality sources):
  [GameTechWiki: Eden](https://emulation.gametechwiki.com/index.php/Eden)
