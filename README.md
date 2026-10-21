# Game37

An all-in-one game library for Nintendo and Sony consoles on Linux. Game37 plays the older Systems
itself, from NES to PSP, through libretro Cores inside its own window. Newer Systems, from GameCube to
PS4, are handed to their Standalone Emulators. Drop your games into one folder and browse them all from
a single console-style menu, with a controller or with mouse and keyboard.

<p align="center">
  <img src="design/mockups/01-home.svg" width="49%" alt="Home with its Shelves (mockup)">
  <img src="design/mockups/03-carousel.svg" width="49%" alt="The Carousel of Systems (mockup)">
</p>

<sub>Design mockups from <code>design/mockups/</code>; the running app follows them closely.</sub>

## Status

Game37 is in development. The library and both ways of playing work today. Artwork, the Details Page,
Settings, search and First-run Setup are still to come.

| Milestone | What it brings | State |
|---|---|---|
| M1 Skeleton | Library scanning, Home and Shelves, Carousel, Console Pages | Done |
| M2 External launching | Standalone Emulators for the 8 newer Systems, Playtime | Done |
| M3 Embedded player | libretro Cores, Pause Menu, Save States, In-game Saves | Done |
| M4 OpenGL rendering | N64, DS, PS1 and PSP inside Game37 | Done |
| M5 Artwork, Details Page, Options Menu | SteamGridDB covers, per-Game page and actions | Planned |
| M6 Settings, search, First-run Setup | Everything a new player needs, without editing files | Planned |
| M7 Speedrun timer | Your own Splits per Game, LiveSplit-compatible | Planned |

Details are in [docs/milestones.md](docs/milestones.md), and the current state in
[docs/progress.md](docs/progress.md).

## What it does

- **One library for 17 Systems.** Game37 scans one folder per System, cleans up file names into Display
  Titles (`Super Mario World (USA) [!].sfc` becomes "Super Mario World"), and notices added, removed and
  renamed Games while it runs.
- **Home:** four Shelves (Favorites, Continue playing, Recently added, Most played) with up to 20 Games
  each and a "See all" grid.
- **Carousel:** every System in a fixed order, themed per System, opening that System's Console Page:
  a sortable grid of its Games.
- **Retro Systems play inside Game37.** Each Core downloads automatically the first time you play that
  System. You get a Pause Menu, three Save State slots with screenshots, a Resume State written on quit,
  and In-game Saves that are always kept. Up to four controllers are supported, and a keyboard layout
  for playing without one.
- **Newer Systems open in their emulator.** Game37 starts the right Flatpak emulator fullscreen with the
  Game, steps aside while you play, and comes back when the emulator closes. If an emulator isn't
  installed, its System says so and shows the install command.
- **Playtime and last played** are tracked for every Game, embedded and external.

## Systems

Listed in System Order: Nintendo first, then Sony, each by European release date.

| System | Folder | Played with | Game files |
|---|---|---|---|
| NES | `nes` | Mesen Core | `.nes` `.fds` `.unf` `.unif` |
| Game Boy | `gb` | SameBoy Core | `.gb` |
| SNES | `snes` | bsnes Core | `.sfc` `.smc` |
| Nintendo 64 | `n64` | Mupen64Plus-Next Core | `.n64` `.z64` `.v64` |
| Game Boy Color | `gbc` | SameBoy Core | `.gbc` |
| Game Boy Advance | `gba` | mGBA Core | `.gba` |
| GameCube | `gc` | Dolphin | `.iso` `.gcm` `.rvz` `.ciso` `.gcz` |
| Nintendo DS | `nds` | melonDS DS Core | `.nds` |
| Wii | `wii` | Dolphin | `.iso` `.wbfs` `.rvz` `.wia` `.ciso` `.gcz` |
| 3DS | `3ds` | Azahar | `.3ds` `.cci` `.cxi` `.3dsx` |
| Wii U | `wiiu` | Cemu | `.wua` `.wud` `.wux`, or a game folder with `code/`, `content/` and `meta/` |
| Switch | `switch` | Ryubing | `.nsp` `.xci` `.nro` |
| PlayStation | `ps1` | Beetle PSX HW Core | `.cue` (with its `.bin`) `.chd` `.pbp` `.m3u` |
| PlayStation 2 | `ps2` | PCSX2 | `.iso` `.chd` `.cso` `.zso` |
| PSP | `psp` | PPSSPP Core | `.iso` `.cso` `.pbp` `.chd` |
| PlayStation 3 | `ps3` | RPCS3 | a game folder with `PS3_GAME/`, or with `PARAM.SFO` and `USRDIR/` |
| PlayStation 4 | `ps4` | shadPS4 | a game folder with `eboot.bin` and `sce_sys/` |

Systems played with a Core are **Embedded Systems**; the others are **External Systems**.

## Requirements

- Linux on x86_64. Game37 is developed on Arch Linux with GNOME on Wayland; other platforms aren't
  supported.
- To build: Rust 1.88 or newer, a C compiler, SDL3 and SDL3_ttf.
- At runtime: OpenGL, fontconfig and the Adwaita Sans font (GNOME's interface font; fontconfig picks a
  fallback if it's missing), Flatpak for the External Systems, and XWayland or an X session for PSP.

On Arch Linux:

```bash
sudo pacman -S --needed rust base-devel sdl3 sdl3_ttf fontconfig adwaita-fonts flatpak
```

## Build and run

```bash
git clone https://github.com/motitschka/Game37.git
cd Game37
cargo build --release
./target/release/game37
```

Game37 isn't packaged; you run it from the build. F11 switches between window and fullscreen.

## Setting up

First-run Setup arrives in M6. Until then, set Game37 up by hand.

**1. Write the Config.** Without `~/.config/game37/config.toml`, Game37 only shows a message asking for
it. Copy the example; its one setting is the Games Root, which defaults to `~/Games`:

```bash
mkdir -p ~/.config/game37 && cp config.example.toml ~/.config/game37/config.toml
```

**2. Make the System folders** in the Games Root:

```bash
mkdir -p ~/Games/{nes,gb,snes,n64,gbc,gba,gc,nds,wii,3ds,wiiu,switch,ps1,ps2,psp,ps3,ps4,bios}
```

**3. Add your games.** Put each game in its System's folder, using the file types from the
[Systems](#systems) table:

- Subfolders are fine: `ps1/Final Fantasy VII/…` is found too.
- A multi-disc PS1 game appears once if you add an `.m3u` playlist that lists its discs.
- Games copied in while Game37 runs appear without a restart.

**4. Add a PS1 BIOS** if you play PlayStation games. Put one of `scph5500.bin`, `scph5501.bin`,
`scph5502.bin`, `scph5503.bin`, `scph7003.bin` or `scph5552.bin` in `~/Games/bios/`. Game37 tells you
which file is missing if there is none. No other Embedded System needs a BIOS.

**5. Install the emulators** for the External Systems you want, from Flathub:

```bash
flatpak install flathub org.DolphinEmu.dolphin-emu info.cemu.Cemu org.azahar_emu.Azahar io.github.ryubing.Ryujinx net.pcsx2.PCSX2 net.rpcs3.RPCS3 net.shadps4.shadPS4
```

Dolphin covers both GameCube and Wii. Some emulators need files from your own console, set up inside
the emulator itself rather than in `~/Games`: Switch keys for Ryubing, the PS3 firmware for RPCS3, and
the PS4 modules for shadPS4.

The Cores need no setup. Each one downloads from the libretro build server the first time you play its
System, with a progress screen.

## Controls

Menus use Nintendo-style buttons: the right face button confirms and the bottom one goes back.

| In menus | Controller | Keyboard | Mouse |
|---|---|---|---|
| Move | D-pad | Arrow keys | Point, or scroll the wheel |
| Confirm | Right face button (A) | Enter or Space | Click |
| Back | Bottom face button (B) | Esc or Backspace | Mouse back button |
| Switch between Home and the Carousel | L1 / R1 | Q / E or Tab | Click a tab |
| Change the sort order on a Console Page | Top face button (X) | S | |
| Fullscreen | | F11 | |

In an embedded game, buttons map by position to the original console's controller. Controllers become
players 1 to 4 in the order they connect. Without a controller, the keyboard plays like this
(RetroArch's default layout):

| Console button | Key |
|---|---|
| D-pad | Arrow keys |
| A / B / X / Y | X / Z / S / A |
| L / R | Q / W |
| Start / Select | Enter / Right Shift |
| R3 (DS: swap screens) | Tab |

- **Pause Menu:** the Guide button, Select + Start, or Esc. It offers resume, save and load in three
  slots, and quit to the library. Quitting writes a Resume State, so the next launch continues where you
  stopped.
- **Nintendo DS:** one large and one small screen; R3 swaps them. Tap the touch screen with the mouse,
  or with a right-stick cursor.
- **External Systems:** quit with the emulator's own controls, and Game37 comes back to the front.

## Where your data lives

| Path | What's there |
|---|---|
| `~/.config/game37/config.toml` | The Config: your Games Root |
| `~/.config/game37/secrets.toml` | Your SteamGridDB API key, from M5 on. Keep it private (`chmod 600`) |
| `~/Games/bios/` | BIOS files |
| `~/Games/saves/<system>/` | In-game Saves |
| `~/Games/states/<system>/` | Save States and Resume States |
| `~/.local/share/game37/cores/` | Downloaded Cores |
| `~/.local/share/game37/library.db` | The library: Playtime, last played, date added, sort order |

All your progress lives in the Games Root, so copying `~/Games` backs up games, saves and states in one
go. Everything under `~/.local/share/game37/` can be rebuilt.

## Privacy

Game37 has no telemetry. It only goes online to download Cores from the libretro build server, and, from
M5 on, to fetch Artwork from SteamGridDB when you've added a key.

## Known limitations

- PSP needs an X display (XWayland on GNOME): the PPSSPP Core doesn't start on Wayland's EGL. Without
  one, Game37 shows a message instead.
- N64 rendering is only confirmed with test programs so far, not with a real game.
- Azahar, Ryubing and shadPS4 stay open in their own game list when a game stops, so Playtime keeps
  counting until you close the emulator's window.
- Two Games in the same folder with the same name but different extensions share their save files.

## Development

All three checks must pass before a commit:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

- `cargo test -- --ignored` runs the tests against real Cores and test ROMs. They read local files from
  `.testdata/`, which is gitignored and never committed.
- `scripts/smoke-screens.sh` renders every screen headless into `target/smoke/` to check for visual
  regressions.
- `python3 design/generate_mockups.py` regenerates the screen mockups in `design/mockups/`.

The specification lives in `docs/`:

- [requirements.md](docs/requirements.md): what Game37 must do.
- [user-stories.md](docs/user-stories.md): the same, from the player's side.
- [milestones.md](docs/milestones.md): the build order.
- [adr/](docs/adr/): the decisions behind the design.
- [CONTEXT.md](CONTEXT.md): the glossary of terms used in code, docs and the interface.

Game37 never includes games, BIOS files, console keys or API keys, and the repository must never contain
any. Use dumps of games and firmware you own.

## License

Copyright (C) 2026 Matei Novotny

Game37 is free software: you can redistribute it and/or modify it under the terms of the GNU General
Public License as published by the Free Software Foundation, either version 3 of the License, or (at
your option) any later version.

Game37 is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the
implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the [GNU General Public
License](LICENSE) for more details.

The libretro Cores and Standalone Emulators that Game37 uses are separate projects under their own
licenses. Game37 downloads or starts them, but doesn't ship them.
