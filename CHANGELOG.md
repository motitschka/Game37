# Changelog

## 0.2.0-beta.2 (2026-11-03)

Switch 2 and PS5 join the library ahead of an emulator that can play them, Wii Games can use a
Classic Controller, and Switch controllers press the right buttons in PlayStation Games.

### New
- **Switch 2 and PS5 are listed as Upcoming Systems.** No emulator can play them yet, but Game37
  already makes their `switch2/` and `ps5/` folders, finds their Games (`.nsp` and `.xci` for Switch 2,
  game folders with `eboot.bin` and `sce_sys/` for PS5) and shows them on the Carousel and their
  Console Pages. Play on one of their Games says it isn't playable yet.
- **Classic Controller on Wii:** a Wii Game's Options Menu has a Controller row, Wii Remote and Nunchuk
  (the default, as before) or Classic Controller, kept per Game from its next start. With the Classic
  Controller the pad maps to it button for button (L2 and R2 are ZL and ZR, R3 is HOME), so Mario
  Kart Wii plays like Mario Kart 8 Deluxe: A accelerates, B brakes, R drifts, L uses an item and X
  looks behind. See the README's Playing section for the full mapping. A Resume State or Save State
  keeps the controller it was made with, so after changing it, start the Game with Start from power-on.

### Upgrading
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
