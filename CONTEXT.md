# Game37

An all-in-one game library for Nintendo and Sony consoles that plays older systems itself and hands newer ones to standalone emulators.

## Systems and emulation

**System**:
One console Game37 knows about (e.g. SNES, PS2). Each System has exactly one folder under the Games Root and exactly one way of being played.
_Avoid_: Platform, console (in code and docs)

**Embedded System**:
A System whose games Game37 runs inside its own window through a Core.
_Avoid_: Internal system, built-in system

**External System**:
A System whose games Game37 hands to a Standalone Emulator in its own window.
_Avoid_: Launched system, delegated system

**Core**:
A libretro library that emulates one Embedded System inside Game37.
_Avoid_: Plugin, module, emulator (when meaning a core)

**Standalone Emulator**:
A separately installed emulator application (e.g. Dolphin, RPCS3) that Game37 starts for an External System.
_Avoid_: External emulator, app

**BIOS**:
A firmware file dumped from real hardware that a Core needs before a System can run.
_Avoid_: Firmware (for embedded systems), ROM

## Library

**Games Root**:
The single folder (default `~/Games/`) that holds one folder per System plus BIOS, saves and states.
_Avoid_: ROM folder, library path

**Game**:
One playable title found in a System folder: a single file, or a folder for PS3, PS4 and Wii U.
_Avoid_: ROM, title, entry

**Display Title**:
The name shown for a Game: the cleaned-up file name, unless the player has renamed it.
_Avoid_: Name, label

**Artwork**:
The cover (tall) and hero (wide) images shown for a Game, sourced from SteamGridDB.
_Avoid_: Box art, thumbnail, image

**Favorite**:
A Game the player has marked; at most five exist at once.
_Avoid_: Starred, pinned

**Hidden Game**:
A Game the player has removed from all views without touching its file.
_Avoid_: Deleted, ignored

**Playtime**:
Total time a Game has been running, measured from launch until the Core or Standalone Emulator stops.
_Avoid_: Hours played, play duration

## Views

**Home**:
The view of Shelves that Game37 opens on.
_Avoid_: Dashboard, main screen

**Shelf**:
A horizontal row of up to 20 Games on Home, built by a fixed rule (Favorites, Continue playing, Recently added, Most played).
_Avoid_: Row, rail, category

**Carousel**:
The view that lists every System, themed per System, as the way into a Console Page.
_Avoid_: System picker, console menu

**Console Page**:
The grid of all Games for one System.
_Avoid_: System view, game list

**Details Page**:
The screen for one Game showing its Artwork, Playtime and Play action.
_Avoid_: Game page, info screen

**Options Menu**:
The per-Game menu of actions (favorite, load state, choose artwork, rename, hide, show file).
_Avoid_: Context menu, actions

## Play sessions

**Pause Menu**:
The overlay Game37 shows over an Embedded System game for save states and quitting.
_Avoid_: Quick menu, overlay

**Save State**:
A snapshot of an embedded game's full machine state, stored in one of three manual slots.
_Avoid_: Snapshot, quicksave

**Resume State**:
A hidden Save State written automatically when the player quits an embedded game, used to continue exactly where they stopped.
_Avoid_: Autosave, suspend state

**In-game Save**:
The save file a game writes itself, independent of Save States.
_Avoid_: SRAM, battery save, memory card (in docs)

## Setup

**First-run Setup**:
The screen shown when no Config exists, where a new player picks Systems and gets an install command.
_Avoid_: Wizard, onboarding

**Config**:
The player's settings file outside the repo (`~/.config/game37/config.toml`); its existence means setup is done.
_Avoid_: Profile, preferences
