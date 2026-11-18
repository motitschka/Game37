# Changelog

## Unreleased

### New
- **Switch plays inside Game37,** on the suyu Core, with the Pause Menu, one way to quit and Playtime
  like the other Embedded Systems. The Core isn't on the libretro build server: run
  `scripts/build-suyu-core.sh` once to build it (Settings → BIOS shows whether it's there). Encrypted
  Games (`.nsp`, `.xci`) need your own `prod.keys` in `~/Games/bios/switch/`; homebrew (`.nro`) needs
  none. Switch has no Save States, Resume State or Restart game. Ryubing is no longer used.
- **Switch 2 Games in `.nup` files are listed** in `switch2/`, next to `.nsp` and `.xci`. Like every
  Switch 2 Game they can't be played yet: Play says "Not playable yet" and starts nothing.
- **Home shows two Shelves per screen, with bigger covers.** Up, down and the mouse wheel move one whole
  Shelf with a short snap, so Home never stops on half a Shelf; a touchpad swipe moves one Shelf per
  notch. Empty Shelves are as tall as the others.
- **Settings → Home** lists the four Shelves in Home's order: A switches one on or off, left and right
  move it up or down. A new player's Home opens on their Games, not on two empty Shelves.
- **Games per row** in Settings → Display: 6 to 10 (default 8) on Console Pages, "See all" and Search.
  The grid now runs from margin to margin, centred, and the covers grow or shrink to fit.
- **Seven sort orders:** the Sort button (or a click on the sort chip) opens a list of A–Z, Z–A, Last
  played, Most played, Recently added, Newest release and Oldest release; picking one jumps to the first
  Game. Games without a release date come last in both release orders, and a grid sorted by release
  says when dates need a key, can't be fetched, or are still arriving.
- **Getting around:** Left on a Shelf's first Game jumps to its "All games" item; sideways swipes and
  Shift+wheel move a Shelf, and the Carousel moves one System per touchpad notch. Page Up, Page Down,
  Home and End move through grids and Search; Shift+Tab switches back a view. In Settings, ‹ and ›
  step a choice back and forward with one click.
- **Release dates** come from SteamGridDB with a key. Game37 asks for the dates it is missing only while
  a grid sorted by Newest release or Oldest release is on screen, one request per Game, never for the
  whole library on its own. The Details Page shows the release year.
- **System logos** on the Carousel cards and the Console Page header, in their official colours, with
  dark parts drawn light so each reads on its card, all at about the same size (square ones with the
  System's name beside them). They
  are trademarks of Nintendo and Sony and are not under Game37's licence (see the README).
- **Show Playtime** in Settings → Display: Off, Grids (default) or Grids and Home.

### Changed
- **Games fill the screen's height or its width:** a Game's picture is now as large as the screen
  allows at its own aspect ratio, with bars only on the other two sides. On a 1920×1200 screen a 4:3
  Game is 1600×1200 instead of 1440×1080 with bars all round, and on a screen wider than 16:9 a wide
  picture (DS side by side) fills the width. The Pause Menu and messages keep their size, centred, and
  dim the whole screen. On a 16:9 screen Games look as before.
- **DS and 3DS Large and small fill the screen too:** the large screen is as tall as the screen and the
  small one sits at its bottom right in the width left over, three to six times smaller, following the
  window as it is resized or goes fullscreen. On a 1920×1200 screen a DS's large screen is 1600×1200.
  R3 and Tab swap which screen is large, and a click on the touch screen touches it wherever it is.
- **Shelves wrap on a press, not while held:** Right on a Shelf's last item goes round to its first Game and Left on the first goes to the last, but holding Left or Right stops at the end.
- **Artwork is searched by the file name, not the Display Title.** Renaming a Game in Game37 now only
  changes the name shown, and its cover and hero stay. To get different Artwork, rename the file, or
  pick it with Choose artwork. A year in parentheses stays in the title, so a remake can be told from
  the original: `The Legend of Zelda - Ocarina of Time (2026).nup` finds the Switch 2 Artwork, not the
  N64 one. The `.nkit` of a `.nkit.iso` no longer shows in titles.
- **"See all" opens every Game you have,** in that Shelf's order, with Games never played last, and can
  be sorted there without changing the Console Pages' order. On Favorites it still shows only the
  Favorites.
- **Playtime shows under the selected cover only,** not under every cover, and not before a Game has
  run for a minute.
- **The library database gains a column** for release dates. It upgrades on the first start; an older
  Game37 can't open it afterwards.

## 0.2.0-beta.3 (2026-11-03)

The menus fill screens taller than 16:9, such as 16:10 and 3:2 laptop screens.

### Changed
- **The menus fill 16:10 screens:** on a screen taller than 16:9, up to 4:3, such as a 1920×1200 laptop
  screen, the menus now fill the whole screen instead of showing black bars above and below. The extra
  height goes to the lists and grids, and nothing is stretched. A screen taller than 4:3 (such as 5:4)
  still shows thin bars above and below, and one wider than 16:9 bars at the sides. Games keep their
  own aspect ratio, as before.

## 0.2.0-beta.2 (2026-11-03)

Switch 2 and PS5 join the library ahead of an emulator that can play them, Wii Games can use a
Classic Controller, and Switch controllers press the right buttons in PlayStation Games.

### New
- **Switch 2 and PS5 are listed as Upcoming Systems.** No emulator can play them yet, but Game37
  already finds their Games in `switch2/` and `ps5/` (`.nsp` and `.xci` for Switch 2, game folders with
  `eboot.bin` and `sce_sys/` for PS5) and shows them on the Carousel and their Console Pages. Setup
  offers both, makes their folders when you tick them, and lays its Systems out in 7 columns. Setup,
  Settings → BIOS and the Details Page mark them "Not playable yet", and Play on one of their Games
  says so and starts nothing.
- **Classic Controller on Wii:** a Wii Game's Options Menu has a Controller row, Wii Remote and Nunchuk
  (the default, as before) or Classic Controller, kept per Game from its next start. With the Classic
  Controller the pad maps to it button for button (L2 and R2 are ZL and ZR, R3 is HOME), so Mario
  Kart Wii plays like Mario Kart 8 Deluxe: A accelerates, B brakes, R drifts, L uses an item and X
  looks behind. See the README's Playing section for the full mapping. A Resume State or Save State
  keeps the controller it was made with, so after changing it, start the Game with Start from power-on.

### Upgrading
- Setup only runs on a new install, so an existing Games Root gets no `switch2/` or `ps5/` folder. To
  add Switch 2 or PS5 Games, make the folder yourself, e.g. `mkdir -p ~/Games/switch2 ~/Games/ps5`.
- This version adds a column to the library database (`library.db`); 0.2.0-beta.1 can't open the
  library any more once this version has.

### Changed
- **Switch controllers on PS1, PS2 and PSP:** a controller with Nintendo letters (Switch Pro
  Controller, Joy-Con pair) maps its face buttons by letter in PlayStation Games: A is ✕, B is ○, X is
  □ and Y is △, so A confirms and B goes back, as on an Xbox controller. Before, they mapped by
  position, which made B confirm. On every other System, and for every other controller, buttons
  still map by position.

## 0.2.0-beta.1 (2026-11-02)

PS2, 3DS, GameCube and Wii now play inside Game37, like the older Systems, with libretro Cores in
place of their Flatpak emulators. Thirteen Systems are Embedded now; only Wii U (Cemu), Switch
(Ryubing), PS3 (RPCS3) and PS4 (shadPS4) still need a Flatpak emulator.

### New
- **PS2, 3DS, GameCube and Wii play inside Game37,** with the Pause Menu, Save States, a Resume State
  and Playtime: PS2 with the LRPS2 Core, 3DS with the Azahar Core, GameCube and Wii with the Dolphin
  Core. Each Core downloads the first time you play its System, and all of them render with OpenGL.
- **PS2:** needs a BIOS dumped from your own PS2, with any file name (a 4 to 8 MB file), in
  `<Games Root>/bios/pcsx2/bios/`.
  Without one, a PS2 Game says so and names that folder. Settings → BIOS has a PS2 row, and Setup's
  BIOS note names every ticked System that needs a BIOS. Each Game gets its own memory card in
  `<Games Root>/saves/ps2/`, named after the Game's file (`Okami.iso` gets `Okami.ps2`), which Game37
  makes already formatted at the Game's first start. PS2 also takes `.ciso` and `.cue` disc images and
  `.elf` homebrew.
- **3DS:** plays homebrew and decrypted Games, including `.app`, `.elf`, `.axf`, the compressed `.zcci`,
  `.zcxi` and `.z3dsx`, and the `.bcci` and `.bcxi` bundles. An encrypted dump shows a message saying it
  must be decrypted.
- **DS and 3DS screen layouts** in Settings → Display: Large and small (the default, the large screen
  three times the small one), Side by side, or One screen. R3, or Tab on the keyboard, swaps the
  screens. The mouse touches the touch screen; the right stick moves a cursor that L3 taps with, on DS
  as on 3DS. On DS, holding R2 blows into the microphone.
- **GameCube and Wii:** the Dolphin Core downloads Dolphin's system files with it. Both take `.dol` and
  `.elf` homebrew besides disc images. On Wii the controller is a Wii Remote with a Nunchuk; see the
  README's Playing section for the full button mapping.

### Changed
- **Upgrading from 0.1:** PS2, 3DS, GameCube and Wii Games no longer open in the PCSX2, Azahar or
  Dolphin Flatpaks, and Setup no longer offers those emulators; you can uninstall them if nothing
  else needs them. Saves made in them aren't moved into Game37: Game37 keeps these Systems' saves in
  the Games Root now, PS2 memory cards in `saves/ps2/`, 3DS saves in `saves/3ds/Azahar/`, and Dolphin's
  whole User folder (memory cards, the Wii's system memory, and also Dolphin's settings, logs and shader
  cache) in `saves/gc/User/` and `saves/wii/User/`.
- **DS controls:** the right-stick cursor taps with L3, as on 3DS, instead of R2, and the microphone
  button moved from L3 to R2, where holding it now blows into the microphone (before, it heard silence).
- Delete save data on a 3DS, GameCube or Wii Game keeps the In-game Save, and says which Core keeps
  it: those Cores keep In-game Saves in their own files, shared by every Game of the System.

### Known issues
- OpenGL only, no Vulkan: some PS2, 3DS, GameCube and Wii Games may run slower than in the standalone
  emulators.
- **PS2:** after loading a Save State on a screen that doesn't change by itself, such as a still menu,
  the old picture stays until the Game draws again; pressing a button brings it back. This is a bug in
  the LRPS2 Core.
- **3DS:** encrypted dumps don't play, and there's no HOME Menu, as the 3DS's system files aren't
  supported. The Azahar Core's Save States break with every update of the Core: a slot that can't load
  says so.
- **GameCube and Wii:** Games on more than one disc can't change discs yet. Wii plays with the Wii
  Remote and Nunchuk only.
- New Super Mario Bros. and Zelda: Phantom Hourglass can freeze on DS
  ([#42](https://github.com/motitschka/Game37/issues/42)). Use Start from power-on to continue from your
  In-game Save.

## 0.1.0-beta.2 (2026-10-27)

### New
- **Start from power-on** in the Options Menu: boots an Embedded Game without its Resume State. Your
  In-game Save still loads, so a Game whose Resume State froze can be continued from its last in-game
  save.
- **Delete save data** in the Options Menu: delete a Game's Resume State, its Save States, its In-game
  Save, or everything. Hold Confirm for a second to delete; deleting is permanent. Deleting the
  In-game Save also deletes the Resume State, so the next launch starts a new save file. PSP Games keep
  their saves in PPSSPP's own files, which this doesn't touch.
- **Restart game** in the Pause Menu resets the Game, like the power button.
- **DS:** the large screen fills the height of a 16:9 display, and the small screen the rest of the
  width.
- When a Game ends, Game37 logs how fast it really ran, to help track down sound or speed problems.

### Fixed
- After a Nintendo 64 Game had been played, DS Games and their music ran two to three times too fast
  until Game37 was restarted: starting the N64 Game turned off vsync for the whole program.
- After quitting a Nintendo 64 Game, Game37's own screen was drawn shrunk into a corner with its text as
  white boxes until it was restarted.
- European (PAL) Nintendo 64 Games showed black bands above and below the picture; they now fill the
  screen.

### Known issues
- New Super Mario Bros. and Zelda: Phantom Hourglass can freeze on DS
  ([#42](https://github.com/motitschka/Game37/issues/42)). Use Start from power-on to continue from your
  In-game Save.

## 0.1.0-beta.1 (2026-10-26)

The first beta: everything planned for v1. First-run Setup, one library for 17 Systems, retro Systems
playing inside Game37 through libretro Cores, newer Systems opening in their Flatpak emulator,
Playtime, SteamGridDB Artwork, the Details Page and Options Menu, Settings and Search.
