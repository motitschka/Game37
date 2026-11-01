# PS2, 3DS, GameCube and Wii run inside Game37

PlayStation 2, 3DS, GameCube and Wii become Embedded Systems. They run inside Game37 through libretro
Cores, like the older Systems, instead of in their standalone emulators: LRPS2 (`pcsx2_libretro`) for
PS2, Azahar (`azahar_libretro`) for 3DS and Dolphin (`dolphin_libretro`) for GameCube and Wii. This
changes the split of ADR 0001 for these four Systems, and they leave ADR 0007's Flatpak emulators.

The owner chose to replace the standalone emulators outright, with no per-System switch back to PCSX2,
Azahar or Dolphin: one way to play each System, with the Pause Menu, Save States, Resume State and
Playtime that the other Embedded Systems have.

All three Cores render through an OpenGL core context, which Game37's SDL host already makes, so ADR
0003 (OpenGL, not Vulkan) stands. Their Vulkan renderers (and LRPS2's paraLLEl-GS) stay out of reach.

## Consequences

- Wii U, Switch, PS3 and PS4 stay External, the only Systems that need a Flatpak emulator.
- These Cores keep their In-game Saves themselves (memory cards, NAND, SD card files) instead of
  handing save memory to Game37, so FR-EMB-7 is met by the Core writing them under
  `<Games Root>/saves/<system>/`, like PPSSPP.
- PS2 needs the player's own PS2 BIOS dump; Game37 says where it goes, as for PS1.
- Some Games of these Systems may run slower or worse than in the standalone emulators (OpenGL only,
  every frame read back, everything on one thread); there is no fallback to them.
- The paused Windows work (ADR 0015 on the `windows` branch) loses its emulator detection for these
  four Systems when it resumes.
