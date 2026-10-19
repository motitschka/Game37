# Hybrid: embed older systems, hand newer ones to standalone emulators

Game37 runs NES, SNES, GB, GBC, GBA, N64, DS, PS1 and PSP itself through libretro Cores, and starts standalone emulators for GameCube, Wii, Wii U, 3DS, Switch, PS2, PS3 and PS4. A pure launcher would feel like many apps stitched together. Embedding everything isn't possible: the newer systems either have no libretro core or their cores lag far behind the standalone emulators. N64, DS and PSP are embedded even though they need GPU rendering and special screen handling, because the owner wanted as much as possible to play inside Game37.

## Consequences

Game37 has two very different play paths (an in-process libretro frontend and a process launcher) that share one library. Features like Save States, the Pause Menu and Resume States exist only on the embedded path.
