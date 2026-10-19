# Game37 User Stories

Two people use Game37:
- **The owner** has a hand-written Config and every emulator installed.
- **A new player** cloned the repo and starts from First-run Setup.

"Player" means either of them. Each story lists its acceptance criteria and the [requirements](requirements.md) it covers.

## Epic 1: Library

**US-1 Drop in a game and see it.** As a player, I want to copy a game file into its System folder and see it appear, so I don't have to manage a library by hand.
- A file copied into `~/Games/snes/` appears on that Console Page and in Recently added without a restart.
- A deleted file disappears.
- Settings → Rescan library finds changes made while Game37 was closed.
- *Covers FR-LIB-1, FR-LIB-2, FR-LIB-3*

**US-2 Folder games.** As a player with PS3, PS4 or Wii U games, I want a game folder to count as one Game, so these Systems work like the others.
- A `BLUS12345/` folder containing `PS3_GAME/` appears as one Game in PS3.
- The files inside a game folder never appear as separate Games.
- *Covers FR-LIB-4, FR-LIB-5*

**US-3 Clean names.** As a player, I want readable titles instead of raw file names, so the library looks tidy.
- `Super Mario World (USA) [!].sfc` is shown as "Super Mario World".
- I can rename a Game from the Options Menu, and the file on disk is unchanged.
- *Covers FR-LIB-6, FR-DET-5*

**US-4 Playtime.** As a player, I want Game37 to track how long I've played each Game, so I can see my most-played games.
- Playtime grows while an embedded game runs, and while an external emulator process runs.
- The Details Page shows the Playtime.
- Settings can reset Playtime and stats.
- *Covers FR-LIB-7, FR-LIB-8, FR-SET-1*

## Epic 2: Home

**US-5 Jump back in.** As a player, I want Home to show what I played recently, so continuing takes one or two presses.
- Continue playing lists Games by last played, newest first.
- Launching from it continues from the Resume State for embedded games.
- *Covers FR-HOME-2, FR-HOME-4, FR-EMB-6*

**US-6 Favorites.** As a player, I want to keep up to five Favorites at the front, so my main games are always on top.
- Favorites is the first Shelf.
- Favoriting a 6th Game opens a picker of the current five, and the one I pick is replaced.
- *Covers FR-HOME-2, FR-HOME-6, FR-DET-4*

**US-7 Discover new and most-played games.** As a player, I want Recently added and Most played Shelves, so I see new games and my staples.
- Recently added shows the last 20 Games found.
- Most played is ordered by Playtime.
- *Covers FR-HOME-4*

**US-8 See everything on a Shelf.** As a player, I want to open a full grid from any Shelf, so the 20-game limit never hides anything.
- Each Shelf ends with "See all", which opens a grid of every matching Game.
- *Covers FR-HOME-3*

**US-9 Understand empty Shelves.** As a new player, I want empty Shelves to tell me how to fill them, so the screen isn't confusing on day one.
- An empty Favorites Shelf shows "Press Y on a game to add a favorite", or the equivalent for the current button style.
- An empty Continue playing Shelf says it fills as I play.
- *Covers FR-HOME-5*

## Epic 3: Browse by console

**US-10 Browse a console.** As a player, I want to pick a console from a themed carousel and see all its games, so I can browse the way I think about my collection.
- L1/R1 or the tabs switch between Home and Carousel.
- The background colour and logo change as I move between Systems.
- Selecting a System opens a grid of tall covers.
- *Covers FR-CAR-1, FR-CAR-2, FR-CAR-3*

**US-11 Know where files go.** As a new player, I want consoles without games to tell me where to put files, so I can fill my library.
- A System with no Games still appears in the Carousel.
- Its Console Page shows the exact folder path.
- *Covers FR-CAR-4*

**US-12 One sort order everywhere.** As a player, I want my chosen sort order to apply to every console, so I don't have to set it again each time.
- I change the sort to Most played on Wii, open PS2, and it's sorted by Most played too.
- The choice survives a restart.
- *Covers FR-CAR-5*

## Epic 4: Game details and options

**US-13 Look before launching.** As a player, I want a details page before a game starts, so I can see my progress and avoid launching by accident.
- Selecting a Game opens the Details Page with hero art, Playtime, last played and Save State count.
- Pressing confirm on Play launches the Game.
- *Covers FR-DET-1, FR-DET-2*

**US-14 Quick actions anywhere.** As a player, I want game actions on Y or right-click wherever a game is highlighted, so I don't have to open the details page first.
- Y or right-click on a Shelf, a grid, a search result or the Details Page opens the Options Menu.
- The menu offers favorite, load state, choose artwork, rename, hide and show in file manager.
- *Covers FR-DET-3, FR-DET-4*

**US-15 Hide and unhide.** As a player, I want to hide games I don't care about, and get them back later.
- A hidden Game disappears from every Shelf, grid and search.
- Settings → Hidden games lists it, and unhiding restores it.
- *Covers FR-DET-6*

**US-16 Open the file.** As a player, I want to jump to a game's file, so I can manage it in my file manager.
- "Show file" opens the file manager with the file selected.
- *Covers FR-DET-4*

## Epic 5: Artwork

**US-17 Real cover art.** As a player with a SteamGridDB key, I want covers and backgrounds fetched automatically, so the library looks like a real console menu.
- After a scan, matched Games show a tall cover in grids and Shelves, and a wide hero image on the Details Page.
- Art is cached, and a restart doesn't re-download it.
- *Covers FR-ART-1, FR-ART-4*

**US-18 Fix wrong art.** As a player, I want to pick different art when the automatic match is wrong.
- "Choose different Artwork" shows other SteamGridDB results, and my pick persists.
- *Covers FR-ART-2*

**US-19 Works without a key.** As a new player without a key, I want Game37 to still look clean.
- Games without art show the Display Title and a System icon. Nothing is broken or blank.
- *Covers FR-ART-3*

**US-20 Enter my key easily.** As a player, I want to paste my SteamGridDB key in Settings, and know it stays on my device.
- Settings has a key field.
- The key is saved to `~/.config/game37/secrets.toml` and never appears in the repo.
- Art starts loading without a restart.
- *Covers FR-CFG-3, NFR-6*

## Epic 6: Search

**US-21 Find a game fast.** As a player with a large library, I want to search by name from anywhere.
- Ctrl+F, L3 or the search icon opens search.
- Results filter live as I type, and each result shows a System badge.
- With a controller, an on-screen keyboard appears.
- *Covers FR-SRCH-1, FR-SRCH-2, FR-SRCH-3*

## Epic 7: Playing embedded games

**US-22 Play retro games inside Game37.** As a player, I want NES, SNES, Game Boy, GBA, N64, DS, PS1 and PSP games to run inside Game37, so it feels like one app.
- Each of the nine Embedded Systems runs a test game at full speed with sound.
- N64, DS, PS1 and PSP run through OpenGL hardware rendering.
- *Covers FR-EMB-1, NFR-4*

**US-23 Cores just appear.** As a player, I want the needed emulator core downloaded automatically, so I never hunt for files.
- The first launch of a System downloads its Core with a progress indicator.
- Later launches use the stored copy.
- Settings can update or redownload Cores.
- *Covers FR-EMB-2, FR-SET-1*

**US-24 Pause, save, quit.** As a player, I want a pause menu I can reach from the controller, so I can save or quit without a keyboard.
- Guide, Select+Start or Esc opens the Pause Menu.
- From it I can resume, save to one of 3 slots, load a slot, or quit to the library.
- Slots show a screenshot and a timestamp.
- *Covers FR-EMB-3, FR-EMB-4, FR-EMB-5*

**US-25 Never lose progress.** As a player, I want to continue exactly where I quit, and keep my In-game Saves regardless.
- Quitting writes a Resume State, and relaunching continues from it.
- In-game Saves are always written to `~/Games/saves/<system>/`.
- *Covers FR-EMB-6, FR-EMB-7, FR-LIB-9*

**US-26 Play together.** As a player with friends, I want 2–4 controllers to work in embedded games.
- The second connected controller is player 2, and so on.
- *Covers FR-EMB-8*

**US-27 Play with a keyboard.** As a player without a controller, I want a sensible keyboard layout.
- The arrow keys and a fixed set of keys play any embedded game.
- *Covers FR-EMB-9*

**US-28 Clear BIOS errors.** As a player, I want to be told exactly which BIOS file is missing and where it goes.
- Launching PS1 without a BIOS shows the expected file name and the `~/Games/bios/` path.
- Settings shows BIOS status per System.
- *Covers FR-EMB-12, FR-SET-1*

**US-29 DS screens and touch.** As a DS player, I want a big main screen, a quick swap, and touch input with any device.
- One large and one small screen are shown, and R3 swaps them.
- Mouse clicks and a right-stick cursor both work on the touch screen.
- *Covers FR-DS-1, FR-DS-2, FR-DS-3*

## Epic 8: Playing external games

**US-30 Launch newer consoles.** As a player, I want GameCube, Wii, Wii U, 3DS, Switch, PS2, PS3 and PS4 games to open in their emulator from Game37.
- Play starts the right Standalone Emulator fullscreen, with the Game loaded.
- When I quit the emulator with its own controls, Game37 is in front again, with Playtime updated.
- *Covers FR-EXT-1, FR-EXT-2, FR-EXT-3, FR-LIB-8*

## Epic 9: Controls, window and settings

**US-31 Controller or mouse, my choice.** As a player, I want every screen to work with only a controller or only mouse and keyboard.
- I can reach every screen and action in Game37 using either input type alone.
- *Covers FR-UI-1*

**US-32 Familiar buttons.** As a player, I want Nintendo-style confirm/back by default, with a switch in Settings.
- The right face button confirms by default.
- Switching the setting flips confirm and back immediately.
- *Covers FR-UI-2*

**US-33 Window behaviour.** As a player, I want Game37 to open the way I left it, or always fullscreen or windowed if I choose.
- The default restores the last window mode.
- Settings offers Always fullscreen and Always windowed.
- Game37 never starts on login.
- *Covers FR-UI-3, FR-UI-5*

**US-34 Check my controller.** As a player, I want a controller test screen, so I can see what Game37 receives.
- Pressing any button or moving a stick highlights it on the test screen.
- *Covers FR-SET-1*

## Epic 10: Setup

**US-35 Owner skips setup.** As the owner, I want Game37 to know my setup already, so I never see setup screens.
- With my Config present, Game37 opens straight to Home, using my installed emulators and paths.
- *Covers FR-CFG-1, FR-CFG-2*

**US-36 New player gets going.** As a new player, I want to pick my consoles and get one command to install what's needed.
- First-run Setup lets me tick Systems.
- It shows the emulator for each External System, and a single copyable `flatpak install` line without already-installed emulators.
- It creates my folders, downloads Cores, and optionally takes my API key.
- *Covers FR-CFG-4*

**US-37 Build it myself.** As a new player, I want clear build instructions.
- The README explains the dependencies and `cargo build --release`.
- *Covers NFR-7*
