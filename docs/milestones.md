# Game37 Milestones

The build order for v1 (M1–M6), followed by the speedrun timer (M7). Each milestone leaves Game37 runnable and builds on the one before it. Story and requirement IDs refer to [user-stories.md](user-stories.md) and [requirements.md](requirements.md).

Testing uses free, legal homebrew games and test ROMs, never commercial game files. Nothing from `~/Games/` is ever committed.

## M1 — Skeleton

**Goal:** Game37 opens, finds games, and can be navigated with a controller or mouse.

**Scope**
- Rust project, SDL3 window, dark theme.
- Controller, keyboard and mouse input. Menus use Nintendo-style confirm/back.
- Reads the owner's Config from `~/.config/game37/config.toml`.
- Library scanner for all 17 System folders, including folder Games for PS3, PS4 and Wii U. Display Titles are cleaned up.
- Live watching of the Games Root.
- Local library database (date added, last played, Playtime, flags).
- Home with all four Shelves, 20-game limit, "See all" grid, and empty-Shelf hints.
- Carousel in System Order with per-System theming, European years, wrap-around, remembered position, faster scrolling when held, and dimmed empty Systems.
- Console Pages with a global sort order and the empty-System message.
- Placeholder art (Display Title and System icon).
- `.gitignore` that keeps game, BIOS, key and secret files out of the repo.

**Delivers:** US-1, US-2, US-3 (clean names), US-7, US-8, US-9, US-10, US-11, US-12, US-31, US-35, US-38
**Requirements:** FR-LIB-1, 2, 4, 5, 6, 7 · FR-HOME-1–5 · FR-CAR-1–8 · FR-UI-1, 2, 4 · FR-CFG-1, 2 · FR-ART-3 · NFR-6

**Done when**
- Homebrew files placed in several System folders appear on the right Console Pages and in Recently added, without a restart.
- Every screen in this milestone can be reached with a controller alone and with a mouse alone.

## M2 — External launching

**Goal:** Game37 becomes useful: every newer console launches from it.

**Scope**
- Launch Dolphin (GameCube, Wii), Cemu, Azahar, Ryubing, PCSX2, RPCS3 and shadPS4 with the selected Game, fullscreen where supported.
- Game37 stays open in the background and comes back to the front when the emulator exits.
- Playtime and last played are recorded from the emulator process, so Continue playing and Most played fill up.
- Detect which Standalone Emulators are installed. Missing ones get a dimmed card with a warning badge, and a Console Page notice with the install command, a copy button, and Check again. Play is disabled until installed.
- Until M5, selecting a Game launches it directly, because the Details Page doesn't exist yet.

**Delivers:** US-4 (tracking), US-5 (external part), US-30, US-39
**Requirements:** FR-EXT-1–4 · FR-LIB-8 · FR-CAR-9, 10

**Done when**
- Each of the 8 External Systems starts its emulator with a test Game.
- Quitting the emulator returns to Game37, with Playtime updated.
- Removing an emulator's Flatpak shows the badge and notice. Reinstalling it and pressing Check again clears them.

## M3 — Embedded player (software rendering)

**Goal:** Retro games play inside Game37.

**Scope**
- libretro frontend: Core loading, video, audio with stable timing, and input.
- Core download from the libretro build server to `~/.local/share/game37/cores/`, with progress.
- NES (Mesen), SNES (bsnes), GB and GBC (SameBoy), GBA (mGBA).
- Pause Menu (Guide, Select+Start, Esc): resume, save, load, quit.
- 3 Save State slots with screenshots and timestamps. A Resume State is written on quit.
- In-game Saves go to `~/Games/saves/<system>/`, and states to `~/Games/states/<system>/`.
- 2–4 controllers and a fixed keyboard layout. Buttons map by position.

**Delivers:** US-5 (embedded resume), US-22 (5 systems), US-23, US-24, US-25, US-26, US-27
**Requirements:** FR-EMB-2–11 · FR-LIB-9

**Done when**
- A homebrew test game runs at full speed without audio crackle on each of the 5 Systems.
- Save, load and resume work.
- Two controllers control players 1 and 2.

## M4 — OpenGL hardware rendering

**Goal:** All nine Embedded Systems run inside Game37.

**Scope**
- libretro OpenGL hardware-render support.
- PS1 (Beetle PSX HW), N64 (Mupen64Plus-Next), DS (melonDS DS), PSP (PPSSPP).
- DS: large and small screen, R3 swaps them, touch with the mouse and a right-stick cursor.
- Missing-BIOS messages that give the exact file name and `~/Games/bios/`.
- Verify the melonDS DS core is on the build server. Pick a fallback if it isn't.

**Delivers:** US-22 (all 9 systems), US-28 (launch message), US-29
**Requirements:** FR-EMB-1, FR-EMB-12 · FR-DS-1–3 · NFR-4

**Done when**
- A homebrew test game runs on each of the 4 Systems.
- Launching PS1 without a BIOS shows the correct message.
- DS swap and touch work with both a controller and a mouse.

## M5 — Artwork, Details Page, Options Menu

**Goal:** The library looks and works like a real console menu.

**Scope**
- SteamGridDB: automatic top-result matching, tall covers and wide hero images, local cache, fallback without a key. In this milestone the key is read from `secrets.toml` written by hand. The Settings field arrives in M6.
- Details Page with hero background and the free information. Selecting a Game now opens it, and Play launches.
- Options Menu on Y or right-click anywhere, and on the Details Page: Favorite (with a replace picker after 5), load Save State, choose Artwork, rename, hide, show in file manager.

**Delivers:** US-3 (rename), US-6, US-13, US-14, US-15 (hiding), US-16, US-17, US-18, US-19
**Requirements:** FR-ART-1–4 · FR-DET-1–6 · FR-HOME-6

**Done when**
- With a key, Games show real covers and hero art, and a restart doesn't re-download them.
- Every Options Menu action works from a Shelf, a grid and the Details Page.

## M6 — Settings, search, First-run Setup

**Goal:** v1 is complete, and someone who cloned the repo can get going.

**Scope**
- Settings: SteamGridDB key, Hidden games (unhide), Games Root location, Rescan, Update Cores, Controller test, Reset stats, BIOS status, Startup window mode, Menu button style, About.
- Search: Ctrl+F, L3 or the search icon, a live-filtering grid with System badges, and an on-screen keyboard.
- Window mode: Remember last (default), Always fullscreen, Always windowed. No autostart.
- First-run Setup when no Config exists: pick Systems → show emulators → copyable `flatpak install` line without already-installed emulators → create folders → download Cores → optional key.
- README with build instructions.

**Delivers:** US-15 (unhiding), US-20, US-21, US-28 (Settings status), US-32 (switch), US-33, US-34, US-36, US-37
**Requirements:** FR-SET-1 · FR-SRCH-1–3 · FR-UI-3, 5 · FR-CFG-3, 4 · FR-LIB-3 · NFR-7

**Done when**
- A fresh user account with no Config goes from First-run Setup to playing a homebrew game by following only the in-app steps and the README.
- Search finds Games with both the keyboard and the controller.

## M7 — Speedrun timer

**Goal:** Speedrunners time their Runs in Game37 against their own Splits. This comes after v1.

**Scope**
- Mockups for the Speedrun Page, the Splits editor, the in-game splits panel and the Timer Window, added to `design/generate_mockups.py` before building.
- Speedrun action on the Details Page, and the Speedrun Page: Categories with Personal Best and attempt count, Splits editor, Run history.
- Categories stored as LiveSplit `.lss` files under `~/Games/splits/<system>/`, picked up live. Data Game37 doesn't use (game time, autosplitter settings) survives a rewrite.
- Real-time timer with Split, Undo split, Skip split and Reset. Keyboard defaults are LiveSplit's numpad keys, and the controller splits with Share/Capture. All can be changed in Settings → Speedrun controls.
- Comparison against Personal Best or Sum of Best, Best Segments, attempt count and Run history.
- Embedded Runs: fresh boot without the Resume State, splits panel beside or over the picture, Pause Menu run actions, and separate In-game Saves under `~/Games/saves/<system>/speedrun/`.
- External Runs: the Timer Window, and controller split input while the emulator has focus.
- Verify whether GNOME's global shortcuts portal delivers keyboard split keys while an emulator has focus. If it doesn't, External Runs split with the controller only, and the Timer Window says so.

**Delivers:** US-40, US-41, US-42, US-43, US-44, US-45
**Requirements:** FR-RUN-1–13

**Done when**
- On a homebrew test game, a Category with 3 Splits is created and two Runs are finished, one with a controller alone and one with a keyboard alone. The faster Run is the Personal Best, the Best Segments and Sum of Best are right, and the attempt count includes a reset Run.
- A `.lss` file made in LiveSplit One appears as a Category with its Personal Best. A file Game37 wrote opens in LiveSplit One with the same times.
- In-game Saves from normal play are unchanged after a Run.
- An External Run opens the Timer Window, and the controller Split button works while the emulator has focus.
