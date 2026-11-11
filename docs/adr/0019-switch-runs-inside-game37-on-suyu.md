# Switch runs inside Game37 on the suyu Core, built from source

Switch becomes an Embedded System. It plays inside Game37 through suyu's libretro Core instead of
in Ryubing, which Game37 no longer uses. This changes ADR 0001's split for Switch, takes it out of
ADR 0007's Flatpak emulators, and replaces ADR 0008's "Switch always goes to Ryubing".

No Switch Core is on the libretro build server, but suyu (a yuzu/Eden-derived emulator, GPL-3.0)
publishes one with each release. That Core can't be used as released: it is built against Ubuntu
24.04's Boost and FFmpeg, so it doesn't load on other distributions, and three gaps make it unfit
for Game37 (research in `docs/research/switch-core.md`):

- It never connects a controller, so games see no input at all. suyu's own frontends connect
  player 1 from their configs; the Core has none.
- On desktop Linux it ignores the frontend's folders and writes its NAND (the In-game Saves),
  keys, config and caches to suyu's XDG folders, and copies keys it finds from other emulators'.
- The emulated Switch runs on its own threads, so stopping `retro_run` (the Pause Menu) doesn't
  pause it.

So Game37 builds the Core itself: `scripts/build-suyu-core.sh` downloads suyu's pinned release
(version and SHA-256), applies `patches/suyu/`, which fix the three gaps, and installs the Core where
Game37 looks for Cores. The first build takes a while (about 700 MB of suyu's dependencies are
fetched and cached), needs no sudo, and never downloads keys, firmware or games. The patches are
worth sending upstream; once a suyu release carries them and loads on any distribution, Game37 can
download that release like any other Core.

## Consequences

- Switch is the only Embedded System whose Core isn't downloaded: `CoreInfo::downloads` is false
  for it. Setup's Cores step and Settings → Cores list only downloaded Cores; Play before the Core
  is built says how to build it, and Settings → BIOS shows whether it is built.
- suyu has no Save States and no `retro_reset`. The Pause Menu greys Save state, Load state and
  Restart game with a note, the Options Menu leaves out Load save state and Start from power-on,
  Details shows no Save States count, and no Resume State is written.
- Encrypted Games (`.nsp`, `.xci`) need the player's own `prod.keys` in `<Games Root>/bios/switch/`,
  checked before Play like a BIOS and copied to the Core before each start. Homebrew (`.nro`) needs
  none.
- The Core renders with its own Vulkan device and hands Game37 finished frames, so ADR 0003 (OpenGL
  in Game37) stands.
- A Game that needs its update to start can't yet be given one: the Core takes an update only through
  an environment variable, and Game37 lists every `.nsp` in `switch/` as a Game. That is the next gap
  to close.
- suyu is a young, fast-moving fork. The pinned version changes only when it has been checked with
  the test Games.
