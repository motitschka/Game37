# Game37 Core milestones

Milestones that bring more Systems inside Game37, or give a System's Core more features. They run on
their own schedule, apart from the road to 1.0 in [milestones.md](milestones.md). Background on the
Cores: [roadmap-external-systems.md](roadmap-external-systems.md),
[research/wiiu-ps3-spikes.md](research/wiiu-ps3-spikes.md) and
[research/switch-core.md](research/switch-core.md).

**Base branch:** `main` (`b1-hardening` is merged into it).

## Status

| Milestone | System | Status | Branch |
|---|---|---|---|
| M9 | Switch: every saving feature | Not started | `m9-switch-saves` |
| M10 | PS3 inside Game37 | Not started | `m10-ps3` |
| M11 | PS4 inside Game37 | Not started | `m11-ps4` |
| M12 | Wii U inside Game37 | Not started | `m12-wiiu` |
| M13 | Switch: a Core of our own | F1 and F2 done; F3 waits on D4 | `m13-switch-core` |

The owner's order for the new Systems is PS3, then PS4, then Wii U. M9 doesn't depend on them, and
they don't depend on it. M10 to M12 don't depend on each other either: what they share, Embedded
Systems whose Core has no Save States, already exists for Switch (ADR 0019). M13's first two stages
are best done before M9, so M9's Core changes land in `swigino/` (see M13).

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
has shipped them, so they mean new work inside suyu, carried in `swigino/` (M13). S1 and S2 don't
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
milestone here: no PS4 Core exists, so Game37 makes one, the way it carries `swigino/` for
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
  modelled on swigino's (`scripts/build-suyu-core.sh`, and its changes in `git log -- swigino`): the
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

---

## M13 — Switch on a Core of our own

**Goal:** Game37's Switch Core is Game37's own: swigino, a fork of suyu kept in Game37's repository
as `swigino/`, in which Game37's libretro changes are commits instead of a patch file, which follows
suyu's releases, and from which Game37 publishes a ready-built Core that loads on any distribution.
Players download it on first use like every other Core instead of building suyu themselves.
`scripts/build-suyu-core.sh` stays for development and offline use.

**Why.** `patches/suyu/` was one 235-line patch (input on port 0, the frontend's folders, the pause
watchdog, a GCC 16 fix). M9 adds `retro_reset` and Save States, which reach into suyu's CPU, kernel,
GPU and audio code. A patch set that size has to be rebased by hand on every suyu release, with no
history, review or tests of its own. And every player pays for the build: suyu's source, about
700 MB of dependencies, a C++20 compiler and Vulkan, SDL3 and FFmpeg development files.

**What stays.** suyu remains upstream: swigino takes the suyu releases the owner chooses and offers
its general fixes back (M9 S5). Game37 never ships keys, firmware or Games (NFR-6). The Core still
renders with its own Vulkan device, so ADR 0003 stands.

**Decisions for the owner** (D1 to D3 before F1, D4 before F3)
- **D1 Where the fork lives — decided (owner, 2026-11-20): inside Game37's repository, as
  `swigino/`.** suyu's release source is imported once, without suyu's history (about 21 MB
  compressed; all of it 419 MB). Its 26 submodules stay links to their own projects in the root
  `.gitmodules`, fetched only by the build script. GPL-3.0 needs a published Core's source published
  with it, which Game37's public repository does. The owner accepts that a takedown notice against
  the Switch code would reach Game37's own repository (Nintendo has had Switch emulators and their
  forks taken down, on GitHub too: yuzu's forks in 2024). This replaces an earlier choice the same
  day of a separate `swigino` repository on GitHub.
- **D2 Its name — decided (owner, 2026-11-20): a name of its own: swigino**, file
  `swigino_libretro.so`, without Nintendo's trademarks (no "Switch", "NX", "Nintendo"). It says the
  Core isn't suyu's release Core, and stops a copy of upstream's `suyu_libretro.so` in the Cores folder
  from being taken for it. The folders it writes keep their names (`<save dir>/suyu/`,
  `<system dir>/suyu/`), so nobody's In-game Saves or keys move.
- **D3 How closely it follows suyu — decided (owner, 2026-11-20): a soft fork.** suyu's changes
  between two of its commits are applied to `swigino/` with a three-way merge when Game37 moves to a
  newer suyu, and general fixes are offered back. If suyu stops releasing, or the Save State work
  makes each update cost more than it brings, a hard fork gets its own ADR.
- **D4 Who builds the published Core.** Game37's GitHub Actions, or the owner's machine.
  *Recommended:* Actions, in a container of an older distribution so the Core loads on every current
  one, with the recipe in the repository so anyone can rebuild the published file from its source.

**Scope**
- **F1 swigino/:** suyu's release source imported into `swigino/` in one commit that names its
  suyu commit (`suyu-commit:`), then one Game37 commit per change: the GCC 16 fix, the frontend's
  folders, the controller on port 0, the pause watchdog. `swigino/SWIGINO.md` says what it is, where
  it comes from, what Game37 changes and how to move to a newer suyu. Check: every imported entry
  equals suyu's tree, and the four changes give files byte-identical to the old patched archive.
- **F2 Game37 builds swigino/:** the build script fetches the submodules (shallow), builds
  `swigino/` out of tree in the build cache and installs the Core. `patches/suyu/` is deleted. The
  real-Core tests (`cargo test -- --ignored`) and the smoke shots pass on the Core it builds.
- **F3 A Core that loads anywhere (needs D4):** a GitHub Actions workflow builds the libretro Core
  from `swigino/` in an older-distribution container, with Boost, FFmpeg (built PIC, only the
  decoders suyu uses) and libstdc++ linked in statically. A check fails the build if the Core needs
  any shared library outside glibc's. Each Core release is published on Game37's GitHub releases,
  zipped, with its SHA-256. The Core stays under `MAX_CORE_BYTES` (64 MiB in `src/cores.rs`;
  today's build is 39 MB without FFmpeg inside). If it can't, the limit grows for this Core alone,
  with the reason in a comment.
- **F4 Game37 downloads it:** the Switch `CoreInfo` gets the new name, swigino (D2), and a pinned
  source: the release URL, version and SHA-256. `CoreInfo::downloads` becomes true, so Setup's Cores
  step and Settings → Cores list it with progress like the others. Play before it is there offers
  the download instead of the build command (`src/launch.rs`, `src/screens/settings/bios.rs`). The
  build script (renamed for swigino) installs the same file name, so a Core built locally is used as
  is. A leftover `suyu_libretro.so` is no longer loaded. Tests drive the download from a local copy
  of a release, never the network.
- **F5 Following suyu:** the routine in `swigino/SWIGINO.md` (apply suyu's diff between two commits
  with `git apply -3`, leaving out what suyu doesn't release; regenerate `.gitmodules`; build and
  test), done once for real when suyu next releases, and scripted if it proves fiddly. General fixes
  are offered to suyu as pull requests. Once suyu carries one, Game37's own change drops out.
- **F6 Docs:** an ADR (the Switch Core is Game37's own fork of suyu in `swigino/`: it replaces ADR
  0019's "built from suyu's pinned source with patches" and, with F4, amends ADR 0004, whose Cores
  come from the buildbot), FR-NEW-5, README's Switch section, CHANGELOG, the comments in
  `src/cores.rs`, a [manual-checks.md](manual-checks.md) section (download on a fresh setup, the build
  script as fallback). M9's and M11's mentions of `patches/suyu/` are already updated.

**Order with M9.** F1 and F2 come before M9 S1, so `retro_reset` and Save States are written as
commits in `swigino/` rather than added to a patch file. F3 and F4 don't block M9 and can follow it.

**Delivers:** a rewritten FR-NEW-5 · Setup's Cores step covers every Embedded System again

**Done when**
- `swigino/` holds suyu's release source, one commit per Game37 change, with the import check's
  result in the Notes.
- `patches/suyu/` is gone, the build script builds `swigino/`, and the real-Core tests pass on it.
- On a fresh data dir, Setup downloads the Switch Core, checks its SHA-256, and Space NX plays.
- The published Core loads in containers of at least two other current distributions (manual check,
  steps in [manual-checks.md](manual-checks.md)).
- Every earlier check still passes (tests, real-Core tests, smoke shots).

**Notes**
- **suyu re-tagged the pinned release.** Tag `v14.1.0` and its release are gone. The same assets
  are now on release `v0.1.4`, whose tag points at a later commit (`96fdc5ff`, 2026-11-06). So the
  old build script's pinned URL returned 404 on a fresh machine (the archive is still at
  `…/releases/download/v0.1.4/suyu-v14.1.0-source.tar.gz`, same SHA-256). F2 ends the download.
- **F1 (2026-11-20):** the base is suyu commit `347fcf91` (2026-10-12), which the old archive names
  in its `GIT-COMMIT` file, not the moved tag. `swigino/` is `git archive` of it: all 5,389 files
  and 26 submodule links equal suyu's tree in mode and hash. Left out are the 17 files suyu's
  `.gitattributes` marks `export-ignore` ("not release source artifacts"): `tools-local/`, with a
  21 MB `suyu-cmd.exe`, and prebuilt `.exe`/`.obj`/`.bin` files under `tools/static_recompiler/`.
  The old archive held the same files plus the submodules' contents and four `GIT-*` version files.
  Khronos' `externals/spirv-headers-new.zip` is force-added past Game37's `*.zip` rule. Applied in
  order, the four change commits give `retro_core.cpp` and `cheat_engine.cpp` byte-identical to the
  old patched archive. suyu's `.github/workflows/` sit under `swigino/`, where GitHub doesn't run
  them.
- **F2 (2026-11-20): suyu's dependencies are fetched while configuring, from many hosts.** One of
  them, `git.eden-emu.dev` (Nintendo's timezone data, `nx_tzdb` 230326), refused TLS connections
  for the whole session, so a first build on a fresh machine fails there; that hit the old script
  too, from the same `cpmfile.json`. suyu's CPMUtil downloads into `swigino/.cache/cpm` by default;
  the build script now sets `CPM_SOURCE_CACHE` to `~/.cache/game37/suyu-build/cpm`, so nothing lands
  in the checkout and later builds need no network. F3's published Core takes this away for players.
- **F2 done (2026-11-20):** the build script built `swigino/` from scratch (1415 steps, all
  dependencies from the seeded cache) into a 40.7 MB Core, and
  `hw_suyu_boots_homebrew_keeps_its_files_in_game37s_folders_and_quits` passes on it. That Core is
  now the one in `.testdata/data/cores/`. Not yet re-run on it: the smoke shots, which don't load a
  Core.
- If this works, M11's shadPS4 Core, now planned as `patches/shadps4/`, could follow the same route.
  That is M11's call.
