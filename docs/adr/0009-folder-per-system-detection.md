# Systems are identified by folder, never by file extension

Each System has one folder under the Games Root, and a Game's System is the folder it is in. Extensions are ambiguous (`.iso` can be PS2, PSP, GameCube or Wii), so guessing would mislabel Games. For PS3, PS4 and Wii U, a subfolder is one Game. Games installed inside an emulator's own storage (RPCS3 `.pkg`, Azahar `.cia`) are deliberately not listed, because reading each emulator's internal layout is fragile.
