# Game37 Core milestones

Milestones that bring more Systems inside Game37, or give a System's Core more features. They run on
their own schedule, apart from the road to 1.0 in [milestones.md](milestones.md). Background on the
Cores: [roadmap-external-systems.md](roadmap-external-systems.md),
[research/wiiu-ps3-spikes.md](research/wiiu-ps3-spikes.md) and
[research/switch-core.md](research/switch-core.md).

**Base branch:** `b1-hardening`, until it is merged into `main`; then `main`. Whoever merges it
updates this line.

## Status

| Milestone | System | Status | Branch |
|---|---|---|---|
| M9 | Switch: every saving feature | Not started | `m9-switch-saves` |
| M10 | PS3 inside Game37 | Not started | `m10-ps3` |
| M11 | PS4 inside Game37 | Not started | `m11-ps4` |
| M12 | Wii U inside Game37 | Not started | `m12-wiiu` |

The owner's order for the new Systems is PS3, then PS4, then Wii U. M9 doesn't depend on them, and
they don't depend on it. M10 to M12 don't depend on each other either: what they share, Embedded
Systems whose Core has no Save States, already exists for Switch (ADR 0019).

## How to work on a milestone

When the owner says "work on milestone M<n>":

1. Read that milestone's section here, including its **Notes**, which earlier sessions left for the
   next one. Read nothing else up front: the section names the files and documents each stage needs.
2. Work on the milestone's branch from the table. If it exists on `origin`, fetch it and continue it.
   Otherwise create it from the base branch and push it at once (`git push -u origin <branch>`).
3. Work through the stages in order, and within a stage in small steps. Each commit is one verified,
   self-contained step: `cargo test`, `cargo clippy --all-targets -- -D warnings` and
   `cargo fmt --check` pass first, plus the stage's ignored real-Core tests where it names them
   (`cargo test -- --ignored`, see `CLAUDE.md`). Push after every commit.
4. When a stage is done, the same commit updates this file: the Status row (e.g. "P2 of P4 done") and
   the milestone's Notes (what was learned, what the next stage should know, anything left open).
   A session that ends mid-stage writes its Notes too, so the next one picks up where it stopped.
5. Stop and ask the owner when a stage needs something only they have: a BIOS, keys, firmware or a
   Game dumped from their own hardware, or a decision the section marks as theirs. Never download or
   commit any of those (NFR-6). Building a Core from a pinned source release, as
   `scripts/build-suyu-core.sh` does, needs no separate go-ahead: asking for the milestone is it.
6. Each milestone adds a smoke shot for every new screen (`scripts/smoke-screens.sh`), a section in
   [manual-checks.md](manual-checks.md) for what only a real Game can show, and an ADR for each
   decision that changes an earlier one.
7. When every "Done when" holds, set the Status row to "Done, not merged" and tell the owner. Only the
   owner merges into the base branch or `main`; never push to `main`.

## Decisions already taken for M10 to M12

The roadmap left two questions for the owner (its D1 and D2). These milestones take the answers
below; if the owner changes one, the milestones change with it.

- **Embedded without Save States.** Cemu, RPCS3 and shadPS4 can't save a state, so these Systems play
  inside Game37 the way Switch does today: In-game Saves under `<Games Root>/saves/<system>/`, the
  Pause Menu with Save state, Load state and Restart game greyed with a note, no Resume State.
- **Replaced outright.** As in ADR 0016, each System's Standalone Emulator is removed once the System
  is Embedded, with no "open in standalone" fallback (ADR 0008).

---

## M9 — Switch saves like every other System

**Goal:** A Switch Game has the same five saving features as every other Embedded System: In-game
Saves in `~/Games/saves/switch/`, 3 Save State slots with screenshots, a Resume State, Restart game,
and Delete save data for that one Game. Today it has only the first (ADR 0019, FR-NEW-5).

Save States are the risk. suyu has none (`retro_serialize_size` is 0), and no yuzu-derived emulator
has shipped them, so they mean new work inside suyu, carried in `patches/suyu/`. S1 and S2 don't
depend on it and come first; S3 decides how S4 is built before any of it is.

**Scope**
- **S1 Restart game:** the Pause Menu's Restart works for Switch. suyu has no `retro_reset`, so
  either Game37's patch adds one (stop the emulated Switch and boot the Game again inside the Core)
  or Game37 ends the Session and starts the Game again. First check whether suyu survives
  `retro_unload_game` followed by `retro_load_game` in one process, since only one Core is loaded per
  process.
- **S2 Delete save data for one Game:** suyu keeps every Game's save in one NAND
  (`saves/switch/suyu/nand/user/save/…/<title ID>/`), so `InGameSaves` is `Shared` for Switch. Game37
  learns each Game's title ID (from the Core once it has loaded the Game, or from the Game file), maps
  it to that Game's save folder, and Delete save data removes only that folder. Homebrew (`.nro`)
  keeps its files on the emulated SD card, and Delete save data says so instead.
- **S3 Save State spike (time-boxed, before S4):** on a branch, find what a Switch Save State must
  hold and whether suyu can restore it: every emulated CPU core's registers, the emulated OS's
  threads, handles, events and services, guest memory, the GPU's state (flushed and rebuilt from
  guest memory rather than saved from Vulkan), and the audio renderer. Measure how big a state is and
  how long it takes to write, since a Switch has 4 GB of memory and every quit would write a Resume
  State. The outcome is an ADR: how S4 is built, or why it can't be yet and what would change that.
- **S4 Save States and Resume State**, built the way S3's ADR says: the 3 slots with screenshots and
  timestamps (FR-EMB-5), the Resume State on quit (FR-EMB-6), Load save state and Start from
  power-on in the Options Menu, and the Save States count on the Details Page, all as on other
  Systems. States that need a long write show progress instead of looking frozen. A state from an
  older pinned suyu that no longer loads is reported, not silently dropped.
- **S5 Docs:** FR-NEW-5 and ADR 0019's consequences rewritten for what Switch now has, a new section
  in [manual-checks.md](manual-checks.md), and the patches offered upstream to suyu.

**Delivers:** FR-EMB-4–6 and FR-EMB-15 for Switch · a rewritten FR-NEW-5

**Done when**
- Restart over a Switch homebrew test Game (`.testdata/`) boots it again from the start, and the Game
  plays on after it.
- Delete save data on one Switch Game removes its folder in the NAND and leaves another Game's save as
  it was (a test with dummy NAND folders in a temp dir).
- A real-Core test saves a state of a moving homebrew test Game, lets it run on, loads the state, and
  sees the picture as it was when saved; quitting and starting again continues from the Resume State.
- The Pause Menu over a Switch Game shows Save state, Load state and Restart game enabled, with no
  "not supported" note.
- On the owner's machine: with their own keys and dump, a real Game's Save State, Resume State,
  Restart and Delete save data all work, and an In-game Save made before them is still there after
  (manual check).
- Every earlier check still passes (tests, real-Core tests, smoke shots).

**Notes**
- None yet.

---

## M10 — PS3 inside Game37

**Goal:** PS3 Games play inside Game37 through the RPCS3 Core from the libretro buildbot
(`rpcs3_libretro.so`, GPL-2.0), and RPCS3 the Standalone Emulator is no longer used. This is the
roadmap's Phase 2.

What is known (Phase 0 spike, [research/wiiu-ps3-spikes.md](research/wiiu-ps3-spikes.md)): the Core
loads and unloads safely in Game37's process. It is 98 MB unzipped, over `MAX_CORE_BYTES` (64 MiB,
`src/cores.rs`). It boots nothing without the PS3 BIOS (`PS3UPDAT.PUP`), homebrew included. On its
default Vulkan renderer a missing BIOS fails `retro_load_game`; on `rpcs3_renderer=OpenGL` (OpenGL
4.6 core, which Game37's host gives) `retro_load_game` succeeds although the boot failed, leaving a
silent black screen. Even a failed boot writes `dev_hdd0/` (with `savedata/`), `dev_hdd1/`,
`dev_flash*/`, `cache/`, `config.yml` and `vfs.yml` under `<system dir>/rpcs3/`. The spike code is on
the `spike-wiiu-ps3` branch (`spike_run_a_game` in `src/session/sdl.rs`), which is not for merging.

**Scope**
- **P1 A running Game (needs the owner's `PS3UPDAT.PUP`):** a `BiosRule` for PS3: the player's
  `PS3UPDAT.PUP`, free from PlayStation's system software page, in `<Games Root>/bios/ps3/`, checked
  before Play with a message naming that folder (FR-EMB-12), and shown in Setup and Settings → BIOS.
  Game37 never downloads it. Then run the spike's "still to run" steps on both renderers with the
  `primitives` test from RPCS3's ps3autotests in `.testdata/`, compare with its expected picture, and
  pick the renderer: Vulkan if it hands frames over in memory without asking Game37 for a Vulkan
  context, otherwise OpenGL. Record the BIOS install time on the first load and the quit time with a
  warm cache in the Notes.
- **P2 The Core in Game37:** an `RPCS3` entry in `src/cores.rs` with its options pinned (the renderer
  from P1, the per-game settings database on, patches off); `MAX_CORE_BYTES` raised to 128 MiB, with
  the comment that names the largest Core updated. PS3 becomes `Kind::Embedded` in `systems::SYSTEMS`.
  Folder Games load through `PS3_DISC.SFB` (a disc dump) or `PARAM.SFO` (extracted), the markers the
  library already looks for; homebrew `.elf`/`.self` join the extensions; `.pkg` stays out (FR-LIB-5).
  The Core's system folder goes under the data dir (as `system_dir` does for Cores with assets), and
  a `vfs.yml` written before the first load points `dev_hdd0` at `<Games Root>/saves/ps3/`, so
  In-game Saves land there (FR-EMB-15). Real-Core tests are `needs_ps3_bios_…`, like `needs_ps2_bios_…`.
- **P3 Starting and quitting:** a failed boot never becomes a black screen: the Core's logged
  `booting … failed: <reason>`, or no frame within a few seconds of the start, is a failed start with
  the reason shown (FR-EMB-14). The first load, which installs the BIOS, shows a "Preparing PS3…"
  Message instead of a frozen screen. Quitting can take a minute while the Core writes its caches, so
  Quit shows a "Closing…" screen and waits; it never kills the Core. Both screens get mockups in
  `design/generate_mockups.py` first, then smoke shots.
- **P4 RPCS3 the Standalone Emulator goes:** removed from `src/emulators.rs`, Setup's emulator step,
  the missing-emulator notices, tests and smoke shots, as M8 did for PCSX2. A new ADR (PS3 runs
  inside Game37), the Systems table and FR-NEW sections of [requirements.md](requirements.md), the
  README, the roadmap's Phase 2 status, and a section in [manual-checks.md](manual-checks.md).

**Done when**
- A PS3 homebrew test program boots and shows its picture in a real-Core test that checks the picture
  as the renderer shows it, with the owner's BIOS.
- Without a BIOS, Play shows the message naming `bios/ps3/`; a broken boot shows its reason, never a
  black screen.
- A second Game starts after the first one quits, in the same Game37 run.
- In-game Saves are under `saves/ps3/`, and nothing is written outside the Games Root and Game37's own
  folders.
- On the owner's machine: a real Game of their own plays with sound and controls, and quits through
  the Pause Menu (manual check).
- Every earlier check still passes, and Setup no longer offers RPCS3.

**Notes**
- None yet.

---

## M11 — PS4 inside Game37

**Goal:** PS4 Games play inside Game37 through a Core Game37 builds from shadPS4's source, and
shadPS4 the Standalone Emulator is no longer used. This is the roadmap's Phase 4, the largest
milestone here: no PS4 Core exists, so Game37 makes one, the way it carries `patches/suyu/` for
Switch.

The approach is the one suyu's Core already uses with Game37 (ADR 0019): the emulator renders on its
own Vulkan device and hands each finished frame to Game37 in memory as XRGB8888, so Game37 needs no
Vulkan and ADR 0003 stands. shadPS4 is Vulkan-only, so this is the only way that avoids a Vulkan host
in Game37.

**Scope**
- **Q1 Spike:** build shadPS4's newest release from its pinned source (version and SHA-256) on Arch,
  and run a PS4 homebrew test program with it on its own, with its folders redirected to a temp dir.
  Read where its frontend meets the emulator (window, Vulkan presenter, audio output, input,
  folders, the emulation threads) and where a libretro frontend hooks in. Find which system modules
  (`sys_modules`, from the player's own PS4 firmware) it needs for homebrew and for real Games, and
  check its licence against Game37's GPL-3.0 (ADR 0014). The outcome is
  `docs/research/ps4-core.md` and an ADR. If the ADR finds the port isn't workable yet, the milestone
  stops there, with the reason in the Notes.
- **Q2 A libretro frontend for shadPS4:** `patches/shadps4/` and `scripts/build-shadps4-core.sh`,
  modelled on suyu's (`scripts/build-suyu-core.sh`, `patches/suyu/0001-game37-libretro.patch`): the
  libretro entry points; frames rendered offscreen and handed over through `video_refresh`; sound
  through libretro rather than its own audio device; RetroPad port 0 as player 1's DualShock 4, with
  the touchpad click on a spare button; every folder under the frontend's save and system folders,
  nothing in shadPS4's own; a pause when `retro_run` stops being called (suyu's watchdog). No Save
  States and no `retro_reset`.
- **Q3 The Core in Game37:** a `SHADPS4` entry in `src/cores.rs` with `CoreInfo::downloads` false,
  like suyu; Play before it is built says how to build it. PS4 becomes `Kind::Embedded`; a Game is
  still a folder with `eboot.bin` and `sce_sys/` (FR-LIB-4). A `BiosRule` for the player's own system
  modules in `<Games Root>/bios/ps4/`, if Q1 found any are needed; In-game Saves under
  `<Games Root>/saves/ps4/`. Real-Core tests use a homebrew test program in `.testdata/`.
- **Q4 shadPS4 the Standalone Emulator goes:** removed from `src/emulators.rs`, Setup, the notices,
  tests and smoke shots; ADR, requirements, README, roadmap and manual checks updated, as in P4.

**Done when**
- `scripts/build-shadps4-core.sh` builds and installs the Core on a fresh checkout, without sudo,
  and Settings → BIOS shows it as built.
- A PS4 homebrew test program shows its picture in a real-Core test that checks the picture as the
  renderer shows it, takes input, and pauses under the Pause Menu.
- In-game Saves are under `saves/ps4/`, and nothing is written to shadPS4's own folders.
- On the owner's machine: a real Game of their own plays with sound and controls (manual check).
- Every earlier check still passes, and Setup no longer offers shadPS4.

**Risks**
- This is months of work, then upkeep each time the pinned shadPS4 version changes.
- Every frame is copied from the GPU to the CPU and uploaded again, which costs speed (unmeasured
  until Q1).
- Many PS4 Games don't run in shadPS4 at all yet; the milestone is about the Games shadPS4 runs on its
  own running the same inside Game37.

**Notes**
- None yet.

---

## M12 — Wii U inside Game37

**Goal:** Wii U Games play inside Game37 through the Cemu Core from the libretro buildbot
(`cemu_libretro.so`, MPL-2.0), and Cemu the Standalone Emulator is no longer used. This is the
roadmap's Phase 1.

What is known (Phase 0 spike, [research/wiiu-ps3-spikes.md](research/wiiu-ps3-spikes.md)): it works
on Game37's existing OpenGL host with `cemu_gpu_api=OpenGL` (OpenGL 4.5 core), with no GLX host or
Vulkan, including on a real Wayland window. Picture, input (RetroPad port 0 is the GamePad), sound
and quit (under 0.2 s) all work with homebrew. The first 300 frames take about 8.7 s while shaders
compile. The Core unpacks 173 graphic packs (13 MB), a `keys.txt` template, logs and a shader cache
into its system folder, and keeps saves in `<Games Root>/saves/wiiu/Cemu/mlc01/`. Two screens and
touch weren't tried.

**Scope**
- **W1 The Core in Game37:** a `CEMU` entry in `src/cores.rs` with its options pinned (OpenGL, async
  shaders on, internal resolution). Wii U becomes `Kind::Embedded`: `.wua`, `.wud` and `.wux` stay,
  homebrew `.rpx` joins them, and a folder Game (`code/ content/ meta/`) loads through its
  `code/<name>.rpx`. The Core's system folder goes under the data dir, not the BIOS folder. Encrypted
  `.wud`/`.wux` need the player's own `keys.txt` in `<Games Root>/bios/wiiu/`, checked before Play
  with a message naming that folder, and copied into the Core's folder before each start, as Switch's
  keys are; the Core's template `keys.txt` is never taken for the player's. In-game Saves stay under
  `saves/wiiu/`. Real-Core tests use DrawU and Tank Trap from the spike, in `.testdata/`.
- **W2 The GamePad's screen:** the TV and GamePad screens shown with the same three layouts as DS and
  3DS (FR-DS-1, FR-NEW-2), R3 (Tab) swapping them, and touch on the GamePad screen through the mouse
  and the right-stick cursor, if the Core takes it through `RETRO_DEVICE_POINTER` as Azahar does.
  Mockups first in `design/generate_mockups.py`, then smoke shots. If the Core can't show the GamePad
  screen, the Notes say so and the TV screen alone is shown.
- **W3 Cemu the Standalone Emulator goes:** removed from `src/emulators.rs`, Setup, the notices,
  tests and smoke shots; ADR, requirements, README, roadmap and manual checks updated, as in P4.

**Done when**
- A Wii U homebrew test Game shows its picture in a real-Core test that checks the picture as the
  renderer shows it, and takes input.
- Without `keys.txt`, Play on an encrypted Game shows the message naming `bios/wiiu/`.
- In-game Saves are under `saves/wiiu/`; Playtime is recorded; Quit goes through the Pause Menu.
- On the owner's machine: a real Game of their own plays with sound and controls, with the GamePad
  screen in each layout (manual check).
- Every earlier check still passes, and Setup no longer offers Cemu.

**Notes**
- None yet.
