# Game37 progress and handoff

Where the build stands after milestone 4, written so a new chat can pick up M5 without this one's
history. The specs are still the source of truth: [requirements.md](requirements.md),
[user-stories.md](user-stories.md), [milestones.md](milestones.md) and the [ADRs](adr/). This file
covers what they don't: what's built, where it lives, how to check it, the decisions made along the
way, and what's still open.

## Status

| Milestone | State | Branch | In `main`? |
|---|---|---|---|
| M1 Skeleton | Done, reviewed | `m1-skeleton` | Yes |
| M2 External launching | Done, reviewed | `m2-external` | Yes |
| M3 Embedded player (software) | Done, reviewed, regression-checked | `m3-embedded` | No |
| M4 OpenGL hardware rendering | Done, reviewed, regression-checked | `m4-opengl` | No |
| M5 Artwork, Details Page, Options Menu | Not started | | |
| M6 Settings, search, First-run Setup | Not started | | |

Each milestone branch builds on the previous one, so **`m4-opengl` holds everything**. Merging it into
`main` brings in M3 and M4 together; the owner hadn't approved that merge yet when this file was
written. Start M5 from `m4-opengl` (or from `main` once it's merged), on a new branch `m5-…`.

## Code map

| Path | What it is |
|---|---|
| `src/systems.rs` | The 17 Systems in System Order, the only place Systems are listed |
| `src/config.rs` | Config loading (`~/.config/game37/config.toml`), XDG dirs |
| `src/library/` | `scan.rs` (folder scan, folder Games, PS1 `.m3u`, PSP `EBOOT.PBP` titles), `title.rs` (Display Titles), `db.rs` (SQLite library, Shelves, sort, settings), `watch.rs` (live Games Root watching) |
| `src/emulators.rs`, `src/launch.rs` | Standalone Emulators for External Systems, Flatpak detection, launching with Playtime; `confirm_game` decides what Confirm on a Game does |
| `src/cores.rs` | libretro Core table (per System: file, options, required BIOS, system dir, GL host kind, stick mode), Core and PPSSPP-asset downloads |
| `src/libretro/` | The Core host: own FFI, callbacks, `hw.rs` (OpenGL FBO, readback), `log.c` (variadic log callback) |
| `src/session/` | Running an embedded Game: `sdl.rs` (session, audio, timing, GL host choice), `input.rs` (ports, pads, keyboard, pointer, sticks, pause trigger), `timing.rs`, `saves.rs` (In-game Saves), `states.rs` (Save States, Resume State), `gl.rs` / `glx.rs` (GL hosts) |
| `src/screens/` | Home, game grid (See all, Console Pages), Carousel, Pause Menu, Playing, download, messages |
| `src/ui/` | `gfx.rs` (drawing over SDL_Renderer, game textures), `nav.rs` (screens, tabs, modals), `controls.rs` / `input.rs` (menu input), `chrome.rs`, `text.rs`, `theme.rs` |
| `src/app.rs`, `src/cli.rs` | Window and loop, screenshot mode, command-line options |

## How to check everything

- `cargo test`: 341 unit and integration tests, no external data needed.
- `cargo test -- --ignored`: 11 tests against real Cores and test ROMs in `.testdata/` (below).
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`.
- `scripts/smoke-screens.sh [out-dir]` renders every screen and flow from M1–M4 headless (21 shots,
  about 12 s) into `target/smoke/`. Look at them before every new milestone, and add a shot for
  every new screen.

Screenshot mode drives the real app headless: `cargo run -- --screenshot out.bmp --config <cfg>
--data-dir <dir> --actions nexttab,right,confirm,wait:120,back --frames 300`. Actions are `up`,
`down`, `left`, `right`, `confirm`, `back`, `options`, `sort`, `prevtab`, `nexttab`, `click:X:Y`
and `wait:N`. While a Game runs, `back` is Esc. `--missing-emulators wiiu,ps3` simulates missing
Standalone Emulators. **Use a fresh `--data-dir` per run**: the Carousel remembers its position in
the library database, so a reused one starts somewhere else.

### `.testdata/` (gitignored, local only, about 121 MB)

| Path | Contents | Source |
|---|---|---|
| `data/cores/` | mesen, bsnes, sameboy, mgba, mednafen_psx_hw, mupen64plus_next, melondsds, ppsspp (`*_libretro.so`) | buildbot.libretro.com nightly, linux x86_64 |
| `data/system/PPSSPP/` | PPSSPP's assets | buildbot.libretro.com/assets/system/PPSSPP.zip |
| `games/nes`, `snes` | 240p Test Suite | pinobatch/240p-test-mini v0.23; SNES from the testsuite240p SourceForge project |
| `games/gb`, `gbc` | 144p Test Suite (the same ROM, both folders) | pinobatch/240p-test-mini v0.23 |
| `games/gba` | 160p Test Suite | pinobatch/240p-test-mini v0.23 |
| `games/n64` | N64 NICCC demo | github.com/PeterLemon/N64 (Unlicense) |
| `games/nds` | devkitPro hbmenu `BOOT.NDS` | github.com/devkitPro/nds-hb-menu v0.11.0 |
| `games/psp/CMFileManager/` | `EBOOT.PBP` plus its `.prx` files | github.com/joel16/CMFileManager-PSP v4.12 |
| `games/ps1` | A dummy `.cue`/`.bin` for the missing-BIOS check | Made locally |
| `cfg.toml` | Config pointing at `.testdata/games` | Made locally |

Tests and scripts copy `games/` into scratch dirs; nothing writes into `.testdata/`. The owner's real
library (`~/Games`, `~/.config/game37/config.toml`) exists but is empty apart from the System folders.

## Decisions beyond the specs

- **Library:** a Game's identity is its path relative to the Games Root. Absent Games keep their
  rows, so Playtime survives a re-add. Recently added's See all lists every Game, newest first.
  Wii U also accepts `.wua`/`.wud`/`.wux` files (FR-LIB-4 updated). PSP `EBOOT.PBP` takes its
  folder's name as its Display Title.
- **UI:** everything is drawn by hand on SDL_Renderer (OpenGL backend) at a logical 1920×1080. The
  font is Adwaita Sans, found via `fc-match`. Keyboard: arrows, Enter/Space confirm, Esc/Backspace
  back, Q/E/Tab switch tabs, F options, S sort, F11 fullscreen.
- **External launching:** `flatpak run --filesystem=<Games Root>:ro <id> <args>`; arguments are
  in `docs/research/external-emulators.md`. Game37 minimises itself while an emulator runs and
  restores itself after. Azahar, Ryubing and shadPS4 stay open in their game list when a game
  stops, so Playtime runs until their window is closed (FR-LIB-8 as written); the Playing screen
  says so.
- **Embedded play:**
  - Keyboard map: RetroArch's defaults (arrows, X=A, Z=B, S=X, A=Y, Q=L, W=R, Enter=Start,
    Right Shift=Select); Tab is R3 for DS. Pads fill the lowest free of 4 ports.
  - Audio: dynamic rate control around 50 ms queued.
  - Timing: lockstep within 1% of the display's refresh rate, otherwise an accumulator.
  - Saves and states: In-game Saves and states mirror the Game's subfolder under
    `saves/<system>/` and `states/<system>/`. Pausing stops Playtime.
- **OpenGL Cores:** each Core gets its own GL context; frames are read back with `glReadPixels` and
  drawn like software frames (no texture sharing). PPSSPP uses a GLX context on XWayland
  (`session/glx.rs`), because its `glewInit()` fails on EGL, which SDL uses on Wayland. Without an
  X display, PSP shows a message instead. Read-back frames are drawn ignoring alpha (PPSSPP leaves
  it at 0).
- **Core options:** Game37 answers `GET_VARIABLE` with per-Core overrides from `cores.rs`:
  - Beetle PSX HW: `hardware_gl`.
  - melonDS DS: hybrid layouts, one small screen, `touch_mode=auto`, cursor shown, **JIT off**
    (the JIT crashed the test homebrew).
- **PS1 BIOS:** checked strictly before anything downloads: a non-empty `scph5500/5501/5502/5503`,
  `scph7003` or `scph5552.bin` in `<Games Root>/bios/`. This holds even though Beetle has an
  OpenBIOS fallback.
- **Sticks:** analog sticks reach every Core. The left stick also presses the d-pad except on N64
  and PSP, where the arrow keys tilt the analog stick instead.

## Known issues and checks only the owner can do

- **N64:** the test demo stays black even in software mode; it doesn't boot on Mupen64Plus-Next. The
  OpenGL path is proven by PSP, so N64 needs a real Game to confirm it renders.
- **PS1:** can't run without the owner's BIOS. Only the missing-BIOS message is verified.
- **Manual, in a real window:**
  - audio without crackle at full speed;
  - two pads as players 1 and 2;
  - DS touch with a real mouse and with the right stick;
  - R3 swap feel;
  - holding a direction in the Carousel;
  - window resizing and F11;
  - GNOME bringing Game37 back to the front after an external emulator exits.
- **External emulators (M2's "done when"):** each of the 8 External Systems starting with a real
  Game. RPCS3 needs firmware, Ryubing `prod.keys`, and shadPS4 its modules.
- **Smaller known gaps:**
  - A cancelled Core download stuck in DNS or connect lingers in the background until its 30 s
    timeout, then cleans up.
  - If `~/Games` is itself a mount point, the watcher misses a remount.
  - Two Games in the same folder with the same stem but different extensions share save files.
- Open from M4's last review (low severity, not fixed):
  - No test pins that the GL loader is cleared after a failed `Hw::start`; assert
    `get_proc_address` is `None` afterwards.
  - Nothing tests `Hw::resize` itself, only the pure `grown()`. A fake host whose FBO build fails
    should see `context_destroy`/`context_reset` happen once.
  - Nothing tests the session's wiring of the keyboard stick (`sdl.rs` ~301), only
    `keyboard_stick`.
  - With PPSSPP's assets already present, the download screen still says "(2 of 2)".
  - With the PPSSPP Core installed but its assets missing, the Core is downloaded again after the
    assets.

## How the work was run

The main session planned and verified; fresh subagents built each piece in their own git worktrees
(the `delegate` plugin's builder, builder-lite and mechanic roles), and fresh-context reviewers
reviewed each milestone (two rounds at most). Every builder had gates to pass, including "the test
fails without the fix". The rules builders follow are in [CLAUDE.md](../CLAUDE.md). The owner's
standing instructions, also in Claude's memory for this repo:

- Commit and push often. Never merge into `main` without the owner's OK.
- Build the milestones one after another, hands-off. Before starting a new one, regression-check
  everything (`cargo test`, `cargo test -- --ignored`, the smoke shots).
- If a usage limit hits, wait for the reset and carry on.

Test data was only downloaded after the owner approved the exact list (file, source, size). Keep
asking first for anything new.

Cost from the `agent-cost` skill (input-equivalent tokens): M1 10.9M, M3 10.7M, M4 20.7M (main 7.4M,
13 subagents 13.3M; the OpenGL work needed several rounds). M2 wasn't measured separately. Measure
a milestone with `agent-cost --project Game37 --since <its start time>`.

## Next: M5 (Artwork, Details Page, Options Menu)

Scope is in [milestones.md](milestones.md#m5--artwork-details-page-options-menu). Points to know
before starting:

- **SteamGridDB key:** real covers need the owner's API key in `~/.config/game37/secrets.toml`
  (FR-CFG-3; never committed, never invented). Build and test without it: placeholder art already
  works (FR-ART-3), and use an injected HTTP fetcher in tests. The "real covers" check needs the
  owner to add the key.
- **Details Page:** selecting a Game opens it, and Play launches. Today Confirm launches directly
  (`launch::confirm_game`), so M5 moves that onto the Details Page's Play. Mockups 05 and 10 show
  the page.
- **Options Menu:** Y or right-click anywhere, and on the Details Page (mockup 06): Favorite (with
  the replace picker after 5, FR-HOME-6), load Save State (the slots exist in `session/states.rs`),
  choose Artwork, rename (`db.rs` has `rename`), hide (`set_hidden`), show in file manager.
- **Smoke shots:** add shots for each new screen to `scripts/smoke-screens.sh`.

M6 follows: Settings, search, window modes, First-run Setup, README.
