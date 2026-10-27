# Manual checks on Windows

For the tester, on a real Windows 11 PC (x86_64), with the installer from the Releases page. Milestone W
is done when every box here is ticked or has an issue that was fixed (docs/milestones.md, ADR 0015).
Work top to bottom: later checks build on earlier ones.

**When something fails:** open an issue at https://github.com/motitschka/Game37/issues with what you did,
what you expected, what happened, a screenshot, and the log file
`%LOCALAPPDATA%\game37\logs\game37.log` (the previous run's is `game37.old.log`). Note the box's number.

**You need:** a controller if you have one (an Xbox or PlayStation pad), and a few free homebrew test
Games; ask the owner for the test pack. Never use game files you don't own.

## 1. Install

- [ ] 1.1 Download `game37-<version>-x86_64-windows-setup.exe` and its `.sha256`. In PowerShell,
      `Get-FileHash .\game37-<version>-x86_64-windows-setup.exe` matches the `.sha256` file.
- [ ] 1.2 Running it, SmartScreen says "Windows protected your PC"; **More info** → **Run anyway** works.
      The next prompt names an unknown publisher.
- [ ] 1.3 The installer finishes. Game37 is in the Start menu with its icon, and in Settings → Apps →
      Installed apps as "Game37" with its version and publisher.

## 2. First-run Setup

- [ ] 2.1 Starting Game37 from the Start menu opens First-run Setup, with no console window behind it.
- [ ] 2.2 Text is crisp and in the same font as the screenshots in the README.
- [ ] 2.3 Pick a few Embedded Systems (NES, SNES, Game Boy Advance, Nintendo 64, PSP) and two External
      Systems (PlayStation 2 and Wii U).
- [ ] 2.4 The emulator step shows each emulator as "Found" with its path or "Not found". Copy the
      `winget install` line, paste it into PowerShell, run it, then press **Check again**: PCSX2 and Cemu
      now show as Found.
- [ ] 2.5 For an emulator with a download page (choose GameCube to see Dolphin), **Download page** opens
      the page in your browser.
- [ ] 2.6 **Choose .exe** opens Game37's file browser. With the controller alone: move between places
      and folders, open folders, go up with B, and choose an `.exe`. The row then shows its path. Do it
      once with the mouse too.
- [ ] 2.7 The folders step creates `%USERPROFILE%\Games` with one folder per chosen System.
- [ ] 2.8 The Cores download with progress. Finishing Setup lands on Home.
- [ ] 2.9 `%APPDATA%\game37\config.toml` exists. If you chose an `.exe` in 2.6, it's listed under
      `[emulators]`.

## 3. Library

- [ ] 3.1 Copying a homebrew Game into its System folder (e.g. `Games\nes`) makes it appear in Recently
      added and on its Console Page within a few seconds, without restarting.
- [ ] 3.2 The Console Page's empty-System message shows the folder with backslashes, like `~\Games\gba`.
- [ ] 3.3 Options Menu (Y) → **Show file in file manager** opens Explorer with the file selected.

## 4. Playing inside Game37

- [ ] 4.1 A homebrew Game runs on NES, SNES and Game Boy Advance at full speed, with sound and no
      crackling.
- [ ] 4.2 Nintendo 64 and PSP show a picture (these use OpenGL).
- [ ] 4.3 The controller plays; the keyboard plays without a controller.
- [ ] 4.4 Pause Menu (Guide, Select + Start, or Esc): save to a slot, load it, **Restart game**, quit
      to the library. Starting the Game again continues where you quit.
- [ ] 4.5 Options Menu → **Start from power-on** starts the Game fresh.
- [ ] 4.6 Options Menu → **Delete save data**: holding A for a second deletes; a quick double tap
      doesn't.

## 5. Emulators outside Game37

- [ ] 5.1 A PlayStation 2 Game starts in PCSX2, fullscreen. When you close PCSX2, Game37 comes back to
      the front and the Game's Playtime has gone up.
- [ ] 5.2 Unzip an emulator somewhere by hand (e.g. Dolphin into Downloads). Its System's Console Page
      shows the missing-emulator notice; **Choose .exe** there and pick it: the notice disappears and a
      Game starts in it.
- [ ] 5.3 Settings → Emulators lists every emulator with its path or "Not found". Choosing a different
      `.exe` works, and forgetting a chosen one makes it "Not found" again (unless it's also installed).

## 6. Window and settings

- [ ] 6.1 F11 switches fullscreen; Settings → Display → Startup window mode is kept after a restart.
- [ ] 6.2 Settings → Games Root shows `~\Games`; moving it to another folder works.

## 7. Updating and uninstalling

- [ ] 7.1 Running the installer again (same or newer version) updates in place; your library is still
      there afterwards.
- [ ] 7.2 Uninstalling from Installed apps removes Game37 from the Start menu and Program Files, but
      `%USERPROFILE%\Games`, `%APPDATA%\game37` and `%LOCALAPPDATA%\game37` are all still there.
