# Game37 Requirements

Requirements for v1, plus the speedrun timer (FR-RUN), which follows v1 in milestone 7. Terms follow [CONTEXT.md](../CONTEXT.md). Decisions behind them are in [adr/](adr/). IDs are referenced from [user-stories.md](user-stories.md).

## Systems

The table is in **System Order**: Nintendo first, then Sony, each by European release date. This order is used everywhere Systems are listed (see FR-CAR-8).

| System | Folder | Year (EU) | Kind | Played with |
|---|---|---|---|---|
| NES | `nes` | 1986 | Embedded | Mesen core |
| Game Boy | `gb` | 1990 | Embedded | SameBoy core |
| SNES | `snes` | 1992 | Embedded | bsnes core |
| Nintendo 64 | `n64` | 1997 | Embedded | Mupen64Plus-Next core |
| Game Boy Color | `gbc` | 1998 | Embedded | SameBoy core |
| Game Boy Advance | `gba` | 2001 | Embedded | mGBA core |
| GameCube | `gc` | 2002 | External | Dolphin |
| Nintendo DS | `nds` | 2005 | Embedded | melonDS DS core |
| Wii | `wii` | 2006 | External | Dolphin |
| 3DS | `3ds` | 2011 | External | Azahar |
| Wii U | `wiiu` | 2012 | External | Cemu |
| Switch | `switch` | 2017 | External | Ryubing |
| PlayStation | `ps1` | 1995 | Embedded | Beetle PSX HW core |
| PlayStation 2 | `ps2` | 2000 | External | PCSX2 |
| PSP | `psp` | 2005 | Embedded | PPSSPP core |
| PlayStation 3 | `ps3` | 2007 | External | RPCS3 |
| PlayStation 4 | `ps4` | 2013 | External | shadPS4 |

## Functional requirements

### Library

- **FR-LIB-1** Game37 scans the Games Root at startup. Each System folder is scanned only for that System.
- **FR-LIB-2** Game37 watches the Games Root while running. Added, removed and renamed Games appear without a restart.
- **FR-LIB-3** Settings offers a manual "Rescan library" action.
- **FR-LIB-4** In `ps3`, `ps4` and `wiiu`, each subfolder that contains the System's expected game structure counts as one Game. In `wiiu`, a single `.wua`, `.wud` or `.wux` file is also one Game. Elsewhere, each file with a recognised extension is one Game.
- **FR-LIB-5** Games installed inside a Standalone Emulator's own storage (e.g. RPCS3 `.pkg` installs, Azahar `.cia` installs) are not listed.
- **FR-LIB-6** The Display Title is derived from the file or folder name with region, revision and dump tags removed. For example, `Super Mario World (USA) [!].sfc` becomes "Super Mario World".
- **FR-LIB-7** Game37 records, per Game: date added, last played, Playtime, Favorite flag, Hidden flag, renamed title, and chosen Artwork.
- **FR-LIB-8** Playtime for External Systems is measured as the Standalone Emulator process's running time.
- **FR-LIB-9** BIOS files live in `<Games Root>/bios/`, In-game Saves in `<Games Root>/saves/<system>/`, and Save States and Resume States in `<Games Root>/states/<system>/`.

### Home and Shelves

- **FR-HOME-1** Game37 opens on Home.
- **FR-HOME-2** Home shows the Shelves in this order: Favorites, Continue playing, Recently added, Most played.
- **FR-HOME-3** Each Shelf shows at most 20 Games. A "See all" item opens a grid of every Game matching that Shelf's rule.
- **FR-HOME-4** Recently added holds the 20 most recently found Games. Continue playing is ordered by last played. Most played is ordered by Playtime.
- **FR-HOME-5** An empty Shelf stays visible and shows a hint explaining how to fill it.
- **FR-HOME-6** At most 5 Favorites exist. Favoriting a 6th Game opens a picker to choose which Favorite to replace.

### Carousel and Console Pages

- **FR-CAR-1** L1/R1, or clicking a tab, switches between Home and Carousel.
- **FR-CAR-2** The Carousel always lists all 17 Systems in System Order, whatever was picked in First-run Setup. Each card shows the maker and European release year (e.g. "Nintendo · 2002"). There are no maker labels or dividers. The background colour changes with the highlighted System.
- **FR-CAR-3** Selecting a System opens its Console Page, a grid of tall covers.
- **FR-CAR-4** A System with no Games keeps its place in the Carousel but its card is slightly dimmed. Its Console Page explains which folder to put files in.
- **FR-CAR-5** Console Pages can be sorted A–Z, by Last played, by Most played, or by Recently added. The sort order is global: changing it on one Console Page applies to all of them until it is changed again. It persists across restarts.
- **FR-CAR-6** The Carousel opens on the System last viewed, across restarts. On the very first launch it opens on NES.
- **FR-CAR-7** The Carousel wraps around in both directions. Holding left or right speeds up scrolling. There is no shortcut to jump between makers.
- **FR-CAR-8** System Order is defined once and used everywhere Systems are listed: the Carousel, the First-run Setup checklist, Settings → BIOS status, and the README.
- **FR-CAR-9** When an External System's Standalone Emulator isn't installed:
  - Its Carousel card is dimmed and shows a warning badge.
  - Its Console Page shows a notice at the top naming the missing emulator, with the `flatpak install` command, a copy button, and a **Check again** button that clears the notice without a restart once the emulator is found.
  - Browsing, Artwork and the Options Menu still work. Play is disabled until the emulator is installed.
  - On the Details Page, Play is greyed out, focus starts on Options, and a line says which emulator is missing and that the install command is on the System's Console Page.
  - If the System also has no Games, the notice appears above the empty-library message.
- **FR-CAR-10** Embedded Systems never show the missing-emulator notice, because their Core downloads on first Play. If that download fails (e.g. offline), pressing Play shows a clear error.

### Details and Options

- **FR-DET-1** Selecting a Game opens its Details Page. Pressing confirm on Play launches it.
- **FR-DET-2** The Details Page shows the hero Artwork as background, plus System, Playtime, last played, date added, Favorite status, number of Save States (embedded only), file name and size.
- **FR-DET-3** The Options Menu opens with Y or right-click on any highlighted Game (Shelves, grids, search results) and from the Details Page.
- **FR-DET-4** Options Menu actions: add or remove Favorite, load Save State (with slot picker, embedded only), start from power-on (embedded only), delete save data: Resume State, Save States, In-game Save, or everything (embedded only), choose different Artwork, rename Display Title, hide Game, show file in the file manager.
- **FR-DET-5** Renaming never changes the file on disk.
- **FR-DET-6** Hidden Games appear in no view except Settings → Hidden games, where they can be unhidden.

### Artwork

- **FR-ART-1** Game37 fetches tall covers and wide hero images from SteamGridDB, matching the Display Title and taking the top result automatically.
- **FR-ART-2** "Choose different Artwork" lets the player browse other SteamGridDB results for that Game.
- **FR-ART-3** Without an API key, or without a match, Game37 shows the Display Title and a System icon.
- **FR-ART-4** Fetched Artwork is cached locally and not re-downloaded on every start.

### Search

- **FR-SRCH-1** Search opens with Ctrl+F on the keyboard, L3 on a controller, or a clickable search icon.
- **FR-SRCH-2** A controller player types with an on-screen keyboard.
- **FR-SRCH-3** Results are a grid filtered live by Display Title across all Systems. Each result shows a System badge.

### Embedded play

- **FR-EMB-1** Game37 loads Cores and supports libretro OpenGL hardware rendering, so all nine Embedded Systems run in v1.
- **FR-EMB-2** Game37 downloads each Core from the libretro build server the first time it is needed and stores it under `~/.local/share/game37/cores/`.
- **FR-EMB-3** The Pause Menu opens with the Guide button, with Select+Start, or with Esc.
- **FR-EMB-4** The Pause Menu offers: resume, save to slot, load from slot, restart, and quit to library.
- **FR-EMB-5** Each Game has 3 manual Save State slots. Each slot shows a screenshot and a timestamp.
- **FR-EMB-6** Quitting an embedded Game writes a Resume State. The next launch of that Game continues from it, unless the player chose Start from power-on.
- **FR-EMB-7** In-game Saves are always written to disk, regardless of Save States.
- **FR-EMB-8** 2–4 controllers are supported. Player order follows connection order.
- **FR-EMB-9** A fixed default keyboard layout allows playing without a controller.
- **FR-EMB-10** In-game buttons map by position to the original console's controller.
- **FR-EMB-11** There is no fast-forward.
- **FR-EMB-12** If a required BIOS is missing, Game37 says which file to place in which folder instead of failing silently.
- **FR-EMB-13** European (PAL) N64 Games are cropped to their picture, without the PAL frame's black borders.

### Nintendo DS

- **FR-DS-1** DS shows one large screen and one small screen, the large one three times the size of the small one: together 16:9, so on a 16:9 display the large screen fills the height and the small one the rest of the width.
- **FR-DS-2** R3 swaps which screen is large.
- **FR-DS-3** The touch screen accepts mouse clicks, and a right-stick cursor with a button to tap.

### External play

- **FR-EXT-1** Game37 starts the System's Standalone Emulator with the Game, fullscreen where the emulator supports it.
- **FR-EXT-2** Game37 stays open in the background and becomes active again when the emulator exits.
- **FR-EXT-3** Players quit External Systems with the emulator's own controls. Game37 provides no kill combo.
- **FR-EXT-4** Standalone Emulators for Embedded Systems (Mesen, bsnes, SameBoy, mGBA, melonDS, RMG, PPSSPP, DuckStation, ares), and Eden, stay installed but are never used or shown by Game37.

### Controls and window

- **FR-UI-1** Every screen is fully usable with a controller alone, and with mouse and keyboard alone.
- **FR-UI-2** Menus use Nintendo-style confirm/back by default (right face button confirms). Settings can switch to Xbox/PlayStation style.
- **FR-UI-3** Startup window mode setting: Remember last (default), Always fullscreen, Always windowed.
- **FR-UI-4** Game37 has a dark theme only.
- **FR-UI-5** Game37 does not start automatically on login.

### Settings

- **FR-SET-1** Settings contains: SteamGridDB API key, Hidden games, Games Root location, Rescan library, Update/redownload Cores, Controller test, Reset Playtime and stats, BIOS status per System, Startup window mode, Menu button style, About.

### Setup and configuration

- **FR-CFG-1** If the Config exists, Game37 starts directly on Home. If it does not exist, First-run Setup is shown.
- **FR-CFG-2** The owner's Config is written by hand outside the repo and fully describes their installed emulators and paths.
- **FR-CFG-3** The SteamGridDB API key is stored in `~/.config/game37/secrets.toml`, never in the repo, and can be entered or changed in Settings at any time.
- **FR-CFG-4** First-run Setup flow:
  1. Pick Systems.
  2. Show the chosen emulator for each External System (no alternatives).
  3. Show one copyable `flatpak install` command that leaves out emulators already installed.
  4. Create the System folders.
  5. Download Cores for the chosen Embedded Systems.
  6. Optionally enter the API key.

### Speedrun timer

Built in milestone 7, after v1.

- **FR-RUN-1** The Details Page has a Speedrun action next to Play. It opens the Game's Speedrun Page, which lists the Game's Categories, each with its Personal Best and attempt count. From there the player starts a Run, edits Splits, adds, renames or deletes a Category, and views Run history.
- **FR-RUN-2** A Category is a named, ordered list of Splits (e.g. "Any%"). A Game can have any number of Categories. The Splits editor adds, renames, reorders and removes Splits. A controller player types names with the on-screen keyboard (FR-SRCH-2). Editing Splits never discards finished Runs, and a new Split has no times until a Run records one.
- **FR-RUN-3** Each Category is a LiveSplit `.lss` file in `<Games Root>/splits/<system>/<Game>/`, where `<Game>` is the Game's path inside its System folder, without the file extension. A `.lss` file placed there appears as a Category without a restart, and the files Game37 writes open in LiveSplit and LiveSplit One. Game37 uses only real time, but keeps game times and any other LiveSplit data when it rewrites a file.
- **FR-RUN-4** Timing is real time, shown to hundredths of a second. The first press of Split starts the timer, each further press records the current Split, and the press on the last Split finishes the Run. The timer keeps running while the Pause Menu is open. Every Run whose timer started counts as an attempt.
- **FR-RUN-5** Split controls are Split, Undo split, Skip split and Reset. Reset stops the timer and sets it back to zero, ready for the next Run.
  - Keyboard: LiveSplit's defaults, Numpad 1 (Split), Numpad 8 (Undo), Numpad 2 (Skip) and Numpad 3 (Reset).
  - Controller: Split is the pad's Share/Capture button where it has one. Undo, Skip and Reset are in the Pause Menu.
  - Settings → Speedrun controls changes the keys and the controller Split button. During a Run, the controller Split button is not passed to an embedded game.
- **FR-RUN-6** The timer shows the Category's Splits, each with its time and its difference from the comparison: green when ahead, red when behind, gold for a new Best Segment. Below them are the running time, the Personal Best and the Sum of Best. The comparison can be switched between Personal Best and Sum of Best.
- **FR-RUN-7** When a Run finishes faster than the Personal Best, it becomes the new Personal Best. Any Segment faster than its Best Segment becomes the new Best Segment, even in a Run that is later reset.
- **FR-RUN-8** Run history lists every finished Run of a Category with its date and final time, newest first, with the Personal Best marked.
- **FR-RUN-9** Every Run, embedded or external, opens the Timer Window: a separate, resizable window showing the splits panel, whose position and size are remembered. It closes when the Run's Game stops. On Wayland it can't stay above a fullscreen game, so it suits a second screen or a windowed game.
- **FR-RUN-10** On an Embedded System, starting a Run boots the Game from power-on, skipping its Resume State. The splits panel is also drawn in Game37's own window: beside the picture where the picture leaves room, otherwise over a corner. Keys pressed while the Timer Window has focus still reach the game. Runs count towards Playtime.
- **FR-RUN-11** During an embedded Run the Pause Menu also offers Undo split, Skip split, Reset, Restart run (Reset plus a reboot from power-on), and Hide or Show the in-game splits panel. The Timer Window stays open either way. Loading a Save State or quitting to the library during a Run resets it.
- **FR-RUN-12** In-game Saves written during an embedded Run go to `<Games Root>/saves/<system>/speedrun/` instead of the normal place, so a Run never overwrites normal progress. They are kept between Runs.
- **FR-RUN-13** On an External System, starting a Run starts the Standalone Emulator like Play, and Game37's main window minimises as usual while the Timer Window stays open. The Run is reset if the emulator exits before the Run finishes.
- **FR-RUN-14** During an external Run, the controller Split button works while the emulator has focus. The emulator may react to that button too. Keyboard split keys work there only if the desktop's global shortcuts portal delivers them. If it doesn't, the Timer Window says that External Runs split with the controller.

## Non-functional requirements

- **NFR-1 Platform:** Linux x86_64 (developed on Arch, GNOME, Wayland). Other platforms are out of scope.
- **NFR-2 Stack:** Rust, SDL3, OpenGL.
- **NFR-3 Responsiveness:** Library navigation stays smooth with a library of thousands of Games. Artwork loads asynchronously and never blocks input.
- **NFR-4 Emulation timing:** Embedded games run at their native frame rate without audio crackle on the owner's machine.
- **NFR-5 Privacy:** No telemetry. Network access is limited to the libretro build server and SteamGridDB.
- **NFR-6 Repo hygiene:** The public repo never contains game files, BIOS files, console keys, or API keys. `.gitignore` enforces this for the usual file types.
- **NFR-7 Distribution:** v1 is built from source with `cargo build --release`. The README documents this. Game37 is not distributed as a Flatpak.
- **NFR-8 License:** GPL-3.0-or-later, copyright Matei Novotny (ADR 0014). The full text is in `LICENSE`.

## Milestones

See [milestones.md](milestones.md) for the build order and what each milestone delivers.

## Open questions

- **melonDS DS core:** whether it is published on the libretro build server needs verifying in milestone 4.
- **Global split keys:** whether GNOME's global shortcuts portal delivers keyboard split keys to Game37 while a Standalone Emulator has focus needs verifying in milestone 7 (FR-RUN-14).
