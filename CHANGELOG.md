# Changelog

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
