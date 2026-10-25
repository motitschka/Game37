# M6 design notes

Decisions for M6 (Settings, search, First-run Setup) that the specs leave open. The specs stay the
source of truth: FR-SET-1, FR-SRCH-1–3, FR-UI-2, 3, 5, FR-CFG-1–4, FR-LIB-3, NFR-7 in
[requirements.md](requirements.md), US-15, 20, 21, 28, 32–34, 36, 37 in
[user-stories.md](user-stories.md). The mockups `design/mockups/17-*` onwards show these screens.

## Where things are stored

- **Config** (`config.toml`): only `games_root`, as today. First-run Setup writes it at the end; Settings
  → Games Root location rewrites it. Its existence still means setup is done (FR-CFG-1).
- **`secrets.toml`** next to the Config: `steamgriddb_api_key`, written by Settings and First-run Setup
  with mode 600, replaced atomically (write a temp file in the same folder, then rename).
- **The library database's `settings` table** (where sort order and the last System already live):
  Menu button style, Startup window mode, and the last window mode for "Remember last".

## Opening Settings and search

- The top bar of every screen with chrome shows the **search pill** ("Search games", with the Ctrl F
  and L3 glyphs, as in the Home mockup) and, right of it, a **gear icon** for Settings, then the clock.
  Both are clickable.
- **Search** opens with Ctrl+F, L3 or a click on the pill, from any screen with chrome that is not a
  modal and has no text field active (US-21 "from anywhere").
- **Settings** opens with the controller's Start (`Action::Start`), Ctrl+, on the keyboard (the GNOME
  shortcut for Preferences) or a click on the gear, under the same conditions. Home and Consoles show
  a "Settings" hint.
- Both are pushed screens, so Back returns to where the player was. Opening one while it is already
  open does nothing.

## Settings

A pushed screen titled "Settings". On the left is a column of sections; on the right are the
focused section's rows. Up and Down move within a column. Right or Confirm goes from a section into
its rows, and Left or Back goes back to the sections. Back on the sections leaves Settings.

| Section | Rows |
|---|---|
| Library | Games Root location (shows the path) · Rescan library (shows the Game count) · Hidden games (shows how many) · Reset Playtime and stats |
| Artwork | SteamGridDB API key (shows "Not set", or "Set · ends in ab12") · Remove key (only when one is set) |
| BIOS | BIOS status, one row per System (all 17, in System Order) |
| Cores | Update all · one row per Core: its state |
| Controls | Menu button style (Nintendo / Xbox / PlayStation) · Controller test |
| Display | Startup window mode (Remember last / Always fullscreen / Always windowed) |
| About | Version, license (GPL-3.0), the paths in use (Config, Games Root, data folder), credits (libretro Cores from the libretro buildbot, Artwork from SteamGridDB) |

- **Choice rows** (button style, window mode) cycle with Left/Right or Confirm and apply at once.
  Switching the button style flips Confirm and Back immediately (US-32).
- **Menu button style:**
  - Nintendo (default): the right face button confirms and the bottom one goes back. Glyphs A/B/X/Y.
  - Xbox: the bottom face button confirms and the right one goes back. Glyphs A/B/X/Y in Xbox
    positions, so Confirm shows "A" on the bottom button.
  - PlayStation: positioned like Xbox, with glyphs ✕ ○ □ △.
  - Options and Sort keep their buttons in every style (left and top face buttons), so only Confirm
    and Back swap.
  - It only changes menus. Embedded Games get the same positional pad as before.
- **Startup window mode:** read at start. Remember last restores the fullscreen/windowed state saved
  when the player toggled it (F11) and at quit. The window size isn't remembered. Game37 never
  installs an autostart entry (FR-UI-5).
- **Games Root location:** a text field prefilled with the current path, typed with the keyboard or
  the on-screen keyboard (`ui/text_entry.rs`). `~` expands to the home folder.
  - Saving checks that the folder exists or can be created, creates the System folders and `bios/`,
    rewrites the Config, moves the library and watcher to the new root, and rescans.
  - A Game's identity is its path relative to the Games Root, so moving the whole folder keeps
    Playtime, Favorites and renames.
- **Rescan library:** scans every System folder now and says what changed ("2 new Games, 1 gone"),
  or "No changes" (FR-LIB-3).
- **Hidden games:** a sub-list of hidden Games: cover thumbnail, Display Title, System. Confirm
  unhides the focused one. The list is empty-stated when nothing is hidden (US-15). An unhidden Game
  returns to its Shelves; its Favorite stays cleared.
- **Reset Playtime and stats:** a confirmation ("Reset Playtime and stats for every Game? This can't
  be undone.", Cancel focused). It clears Playtime, last played and play counts for every Game, so
  Continue playing and Most played empty out. Favorites, renames, hidden Games, Artwork, In-game
  Saves and Save States stay.
- **SteamGridDB API key:** Confirm opens a text field (empty, not prefilled). While typing, every
  character except the last four shows as a dot. Ctrl+V pastes.
  - Saving checks the characters as loading does, writes `secrets.toml` and restarts the Artwork
    service with the new key, so covers start loading without a restart (US-20).
  - Saving an empty field changes nothing. Remove key deletes it from `secrets.toml` and stops
    fetching; the cache stays.
  - The key appears nowhere else in full: not in a row, a message, a log or `Debug` output.
  - The row explains where to get a key: steamgriddb.com → Profile → Preferences → API.
- **BIOS status:** one row per System in System Order (FR-CAR-8). PS1 shows the file found ("Found
  scph5501.bin") or what is missing and where it goes ("Missing: put scph5500.bin, scph5501.bin or
  scph5502.bin in ~/Games/bios/"), using the same check as launching. Other Embedded Systems say "No
  BIOS needed". External Systems say "Handled by <Emulator>".
- **Cores:** one row per Embedded System's Core: its name, and "Installed" or "Not downloaded".
  Confirm on a row downloads (or redownloads) that Core. "Update all" redownloads every installed
  Core, plus PPSSPP's assets when PPSSPP is installed. Progress and failures show in the row. It
  uses the existing Core download (`cores.rs`).
- **Controller test:** a pushed screen listing the connected controllers by name, with a drawn pad
  for the first one (or the one last used):
  - Every button lights up while held, sticks show their position, triggers show how far they're
    pressed (US-34).
  - Menu actions don't fire on this screen. Holding Back for 1.5 s leaves; Esc and the "‹ Settings"
    click leave at once. The hint says "Hold B to exit", with B following the button style.
  - Without a controller, it says to connect one.

## Search

A pushed screen titled "Search". The top bar's pill becomes the field, and the results grid fills
the rest: the same covers as a Console Page, each with its System badge (FR-SRCH-3).

- **Matching:** live, on Display Title, case- and accent-insensitive, across all Systems. Hidden
  Games never match.
  - Order: titles that start with the query, then titles with a word starting with it, then any
    other match. Alphabetical within each group.
  - An empty query shows a hint ("Type to search 23 Games") instead of a grid. No matches says so.
- **Keyboard:** typing goes into the field. Down or Enter moves into the results. In the results,
  arrows move, Enter opens the Details Page, and F opens the Options Menu. Up from the top row, or
  Ctrl+F, goes back to the field.
- **Controller:** the on-screen keyboard from rename is docked at the bottom of the screen
  (FR-SRCH-2), with the grid between it and the field updating live. Its buttons are rename's: Y
  Backspace, X Space. Its Done key (or Start) moves into the results and hides the keyboard. Up
  from the top row brings the keyboard back.
- **Mouse:** a click on a result opens it, and a click on the field focuses it.
- Back from the results closes search. Back from the field closes it too, after clearing a
  non-empty query first. The query survives a visit to a Details Page and back.

## First-run Setup

Shown when no Config exists at the Config path in use. An unreadable Config keeps today's error
screen. It is a pushed sequence of steps under a "Setup · step n of 5" header. Back goes to the
previous step, and leaving the first step quits.

1. **Systems:** all 17 in System Order as tiles with checkboxes, none ticked. Each tile says "Plays
   in Game37" for Embedded Systems or "Needs <Emulator>" for External ones. "Select all" and
   Continue (only enabled with at least one ticked).
2. **Emulators** (skipped when no External System is ticked): each ticked External System with its
   Standalone Emulator (name and Flatpak id, no alternatives) and Installed / Not installed.
   - Below the list is one copyable line, `flatpak install flathub <ids of the missing ones>`, with
     Copy and Check again. When everything is installed, it says so instead.
   - Continue works either way; a missing emulator shows on the Carousel as today.
3. **Folders:** the Games Root (default `~/Games`, with Change, which uses the same text field as
   Settings), the folders that will be made (`<system>/` for each ticked System, plus `bios/`), and
   "Create folders". Existing folders are fine. Each shows a ✓ once made.
4. **Cores** (skipped when no Embedded System is ticked): each ticked Embedded System's Core with a
   progress bar.
   - Download starts them one after another, and a failed one offers Retry. Cores already in the
     data folder show "Installed". PSP also fetches PPSSPP's assets.
   - Continue works once nothing is downloading, finished or not.
   - With PS1 ticked, a note says which BIOS file to put in `<Games Root>/bios/`.
5. **SteamGridDB key (optional):** what it is for, where to get one, the masked field, and Skip.
   Finish writes `secrets.toml` (if a key was entered), then the Config, then opens Home with
   the library, watcher, emulator check and Artwork started as if Game37 had started with that
   Config.

## Smoke shots

Add a shot to `scripts/smoke-screens.sh` for every new screen and step: Settings (each section),
Hidden games, the key field, Controller test, Search with the keyboard and with the on-screen
keyboard, and each First-run Setup step, using a missing Config path under the smoke dir.
