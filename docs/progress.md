# Game37 progress and handoff

Where the build stands after milestone 5, written so a new chat can pick up M6 without this one's
history. The specs are still the source of truth: [requirements.md](requirements.md),
[user-stories.md](user-stories.md), [milestones.md](milestones.md) and the [ADRs](adr/). This file
covers what they don't: what's built, where it lives, how to check it, the decisions made along the
way, and what's still open.

## Status

| Milestone | State | Branch | In `main`? |
|---|---|---|---|
| M1 Skeleton | Done, reviewed | `m1-skeleton` | Yes |
| M2 External launching | Done, reviewed | `m2-external` | Yes |
| M3 Embedded player (software) | Done, reviewed, regression-checked | `m3-embedded` | Yes |
| M4 OpenGL hardware rendering | Done, reviewed, regression-checked | `m4-opengl` | Yes |
| M5 Artwork, Details Page, Options Menu | Done, reviewed twice, regression-checked | `m5-artwork` | Yes |
| M6 Settings, search, First-run Setup | Not started | | |
| M7 Speedrun timer (after v1) | Specified 2026-10-21, not started | | |

Each milestone branch builds on the previous one. `main` has everything through M5 (merged on
2026-10-25), plus the M7 spec, the license and the README. Start M6 from `main`, on a new branch
`m6-…`.

## Code map

| Path | What it is |
|---|---|
| `src/systems.rs` | The 17 Systems in System Order, the only place Systems are listed |
| `src/config.rs` | Config loading (`~/.config/game37/config.toml`), the SteamGridDB key from `secrets.toml` next to it (`ApiKey`, redacted `Debug`), XDG dirs |
| `src/library/` | `scan.rs` (folder scan, folder Games, PS1 `.m3u`, PSP `EBOOT.PBP` titles), `title.rs` (Display Titles), `db.rs` (SQLite library, Shelves, sort, settings, Favorites incl. `replace_favorite`/`all_favorites`, rename, hide, Artwork records), `watch.rs` (live Games Root watching) |
| `src/emulators.rs`, `src/launch.rs` | Standalone Emulators for External Systems, Flatpak detection, launching with Playtime; `confirm_game`/`confirm_play` decide what Play does (a `Play` carries an optional Save State slot) |
| `src/cores.rs` | libretro Core table (per System: file, options, required BIOS, system dir, GL host kind, stick mode), Core and PPSSPP-asset downloads |
| `src/artwork.rs` | SteamGridDB Artwork: lookups, candidates, choices, disk cache, decoding, the job scheduler (network and disk threads) |
| `src/file_manager.rs` | "Show file": FileManager1 `ShowItems` over D-Bus, `xdg-open` fallback, on a thread |
| `src/libretro/` | The Core host: own FFI, callbacks, `hw.rs` (OpenGL FBO, readback), `log.c` (variadic log callback) |
| `src/session/` | Running an embedded Game: `sdl.rs` (session, audio, timing, GL host choice, start state), `input.rs` (ports, pads, keyboard, pointer, sticks, pause trigger), `timing.rs`, `saves.rs` (In-game Saves), `states.rs` (Save States, Resume State, slot info), `gl.rs` / `glx.rs` (GL hosts) |
| `src/screens/` | Home, game grid (See all, Console Pages), Carousel, `details.rs` (Details Page), `options.rs` (Options Menu with its replace picker, slot picker and rename view), `artwork_picker.rs`, Pause Menu, Playing, download, messages |
| `src/ui/` | `gfx.rs` (drawing over SDL_Renderer, game textures, the Artwork texture cache), `cover.rs` (covers with placeholder fallback), `text_entry.rs` (text field and on-screen keyboard; M6 search reuses it), `nav.rs` (screens, tabs, modals, `Transition::Replace`), `controls.rs` / `input.rs` (menu input, text mode), `chrome.rs`, `text.rs`, `theme.rs` |
| `src/app.rs`, `src/cli.rs` | Window and loop (events fed one at a time in the text mode the top screen wants), screenshot mode, command-line options |

## How to check everything

- `cargo test`: 464 unit and integration tests, no external data or network needed.
- `cargo test -- --ignored`: 13 tests against real Cores and test ROMs in `.testdata/` (below).
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`.
- `scripts/smoke-screens.sh [out-dir]` renders every screen and flow from M1–M5 headless (27 shots,
  about 15 s) into `target/smoke/`. Look at them before every new milestone, and add a shot for
  every new screen. A worktree has no `.testdata/`: symlink it in (it's gitignored) and remove the
  link afterwards.

Screenshot mode drives the real app headless: `cargo run -- --screenshot out.bmp --config <cfg>
--data-dir <dir> --actions nexttab,right,confirm,wait:120,back --frames 300`. Actions are `up`,
`down`, `left`, `right`, `confirm`, `back`, `options`, `sort`, `prevtab`, `nexttab`, `start`, `click:X:Y`
and `wait:N`. While a Game runs, `back` is Esc. `--missing-emulators wiiu,ps3` simulates missing
Standalone Emulators. `--artwork` turns real Artwork on (key from `secrets.toml` next to `--config`,
cache under `--data-dir`); without it screenshot mode stays offline with placeholders. **Use a fresh
`--data-dir` per run**: the Carousel remembers its position in the library database, so a reused one
starts somewhere else.

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
The owner's `~/.config/game37/secrets.toml` holds a real SteamGridDB key (mode 600); never print it.

## Decisions beyond the specs

- **Library:** a Game's identity is its path relative to the Games Root. Absent Games keep their
  rows, so Playtime survives a re-add. Recently added's See all lists every Game, newest first.
  Wii U also accepts `.wua`/`.wud`/`.wux` files (FR-LIB-4 updated). PSP `EBOOT.PBP` takes its
  folder's name as its Display Title.
- **UI:** everything is drawn by hand on SDL_Renderer (OpenGL backend) at a logical 1920×1080. The
  font is Adwaita Sans, found via `fc-match`. Keyboard: arrows, Enter/Space confirm, Esc/Backspace
  back, Q/E/Tab switch tabs, F options, S sort, F11 fullscreen. The controller's Start is
  `Action::Start` (saves a text field).
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
- **Details Page (M5):** Confirm or a click on a Game opens it with Play focused; Play calls
  `confirm_game`. Embedded Games show five stat boxes (with Save States), external ones four. The
  size is measured on a thread. With the Standalone Emulator missing, Play is greyed and focus
  starts on Options. The page re-reads its Game when `ctx.revision` changes and leaves if the Game
  was hidden or vanished.
- **Options Menu (M5):** one modal with sub-views (list, Favorite replace picker, Save State slot
  picker, rename) because the app draws only the top modal; "Choose artwork" pushes its own picker.
  `Transition::Replace` lets a Core download or BIOS message take the menu's place.
  - Favorites: the five-limit counts absent Favorites too (`all_favorites`), and the picker offers
    them marked "File not found"; the swap is one transaction. Hiding a Game also unfavorites it.
  - Load Save State: slot info from cheap `is_file()` checks, screenshots read on a thread; a slot
    the Core can't read starts the Game fresh and says so in a Message.
  - Rename: `ui/text_entry.rs` (field + on-screen keyboard, Shift with three states, symbols layer
    with common accents), 100 characters, empty or the cleaned-up title resets the rename. Typing
    keys never act while a field is active: events are fed one at a time in the right text mode.
  - Show file: gdbus `FileManager1.ShowItems` with a percent-encoded URI, else `xdg-open` on the
    folder; folder Games show the folder itself.
- **Artwork (M5):** see `docs/research/steamgriddb.md` and ADR 0005.
  - Key: `secrets.toml` next to the Config in use, read at start. Bad key (401/403/429 from the
    API) or three connection failures in a row stop requests for the run; a file host's error only
    fails that file.
  - Record: JSON in the library's `artwork` column (game id, the title it was searched for,
    automatic or chosen cover and hero, or "no match"). Renaming re-searches automatic matches;
    hand-chosen Artwork stays. Cache: `<data dir>/artwork/<cover|hero>/<SteamGridDB id>.<png|jpg>`.
  - Only PNG and JPEG (`image` crate), only https URLs on SteamGridDB hosts. A top file that won't
    decode gives way to the next candidate; all bad is recorded as no Artwork.
  - Scheduler: jobs are queued only for Games actually drawn, dropped if not asked for in 60 polls
    (~1 s), picker lists before previews before covers, newest first, at most 64 pending, requests
    at least 250 ms apart, one network thread.
  - Pictures are decoded down to the smallest size that still fills their box (covers 400×600,
    heroes 1920×1080), cached in memory up to 256 MB; textures are dropped after 300 frames unseen
    or over 128 MB.

## Known issues and checks only the owner can do

- **N64:** the test demo stays black even in software mode; it doesn't boot on Mupen64Plus-Next. The
  OpenGL path is proven by PSP, so N64 needs a real Game to confirm it renders.
- **PS1:** can't run without the owner's BIOS. Only the missing-BIOS message is verified.
- **Real covers (M5's first "done when"):** not seen yet. The sandbox blocks steamgriddb.com, so
  Artwork was only checked with fakes and with seeded cache files. The owner can run the test
  library at `<scratchpad>/realart` (dummy Games with real names, `secrets.toml` symlinked to the
  real one) or approve one screenshot run outside the sandbox.
- **Manual, in a real window:**
  - audio without crackle at full speed;
  - two pads as players 1 and 2;
  - DS touch with a real mouse and with the right stick;
  - R3 swap feel;
  - holding a direction in the Carousel;
  - window resizing and F11;
  - GNOME bringing Game37 back to the front after an external emulator exits;
  - with a key: scrolling a large library stays smooth and requests stay paced; a chosen cover
    sticks after a restart;
  - "Show file in file manager" opens Nautilus with the file selected;
  - typing a rename with a real keyboard, including dead keys and an IME.
- **External emulators (M2's "done when"):** each of the 8 External Systems starting with a real
  Game. RPCS3 needs firmware, Ryubing `prod.keys`, and shadPS4 its modules.
- **Smaller known gaps:**
  - A cancelled Core download stuck in DNS or connect lingers in the background until its 30 s
    timeout, then cleans up.
  - If `~/Games` is itself a mount point, the watcher misses a remount.
  - Two Games in the same folder with the same stem but different extensions share save files.
  - Hidden Games can't be unhidden until M6's Settings.
  - "No match" from SteamGridDB never expires; only a rename asks again.
  - The Artwork picker lists only the first page of results.
- **Open from M5's last review (low severity, not fixed; do 1 and 2 first in M6):**
  1. Among visible Games, the last cover drawn is fetched first, so the focused top-left cover of a
     new Console Page loads last (~10 s for 15 covers). Rank by the first ask's tick or by focus
     (`src/artwork.rs` ~368–371, ~886–888).
  2. Every file-host status counts as temporary, so a Game whose top files permanently return 404
     repeats a listing and three downloads every run. Treat 404/410 as unusable files
     (`src/artwork.rs` ~1304, ~1308).
  3. `all_favorites` doesn't filter hidden rows, and Hide unfavorites in a second UPDATE outside a
     transaction (`db.rs` ~380, `options.rs` ~350).
  4. Quit is now handled after that frame's watcher, launcher, `nav.tick` and `sdl::start`, so a
     download finishing in the frame the window closes starts and at once ends a session
     (`app.rs` ~150–208).
  5. Mouse events set `controls.kind` when collected, key and pad events later in `feed`, so a pad
     press then a mouse move in one frame leaves controller glyphs (`app.rs` ~630–646).
  6. No test checks that `sdl::start` pushes the slot-failure Message over the session (deleting the
     `nav.push` at `session/sdl.rs` ~535 keeps the suite green).

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

Lessons from M5:
- Builders that hit their context limit or a usage limit left work uncommitted; telling every
  builder to commit after each step once the checks pass kept later stops cheap.
- Isolated worktrees sometimes start from `main`, not the branch in use: tell builders to check
  `git log -1` and `git reset --hard <branch>` first. Agents in worktrees must run git commands
  one at a time, not chained with `&&`.
- A large milestone's diff needs two reviewers split by area; one ran out of context halfway.

Test data was only downloaded after the owner approved the exact list (file, source, size). Keep
asking first for anything new.

Cost from the `agent-cost` skill (input-equivalent tokens): M1 10.9M, M3 10.7M, M4 20.7M (main 7.4M,
13 subagents 13.3M), M5 23.5M (main 8.4M, 26 subagents 15.1M; two usage-limit interruptions and two
review rounds). M2 wasn't measured separately. Measure a milestone with
`agent-cost --project Game37 --since <its start time>`.

## Next: M6 (Settings, search, First-run Setup)

Scope is in [milestones.md](milestones.md#m6--settings-search-first-run-setup). Points to know
before starting:

- **First:** the two Artwork leftovers above (focused cover first, permanent 404s recorded).
- **Settings → SteamGridDB API key:** writes `secrets.toml` next to the Config (mode 600) and should
  restart the Artwork service so it applies without a restart (today the key is read only at start).
  Use `config::ApiKey`; never show the key in full.
- **Settings → Hidden games:** unhide with `Library::set_hidden(id, false)`; Hide also cleared the
  Favorite, which stays cleared.
- **Search:** reuse `ui/text_entry.rs` (`TextEntry::new`, `handle → Outcome`, `draw_field`,
  `draw_keyboard`, `hints`) and `Screen::text_input()`; results are a grid with System badges.
- **README:** rewrite "Setting up" for First-run Setup, and describe Settings and search.

M7, the speedrun timer, comes after v1: own Splits per Game in LiveSplit `.lss` files, a timer beside
embedded games, and a separate Timer Window for every Run (FR-RUN-1–14, ADRs 0012 and 0013).
It needs M6's on-screen keyboard and Settings.
