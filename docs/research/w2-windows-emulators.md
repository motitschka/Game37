# Standalone Emulators on Windows (research for milestone W2)

Gathered 2026-10-27 from the projects' own sites, GitHub repositories and the winget-pkgs manifests. Command lines are the same as Game37's Linux ones (src/emulators.rs), so only the executable changes. "UNVERIFIED" marks what couldn't be checked against a primary source.

| System | Emulator | Executable | How it ships on Windows | Where it lands | winget | Arguments |
|---|---|---|---|---|---|---|
| GameCube, Wii | Dolphin | `Dolphin.exe` | Current releases: portable 7z only. The NSIS installer's last release is 5.0 (2016). | Installer: `%ProgramFiles%\Dolphin\`. Portable: anywhere. | `DolphinEmulator.Dolphin`, but **stuck at 5.0 from 2016**: show the download page instead. | `-b -C Dolphin.Display.Fullscreen=True -e <file>` |
| Wii U | Cemu | `Cemu.exe` | Portable zip. | winget: `%LOCALAPPDATA%\Microsoft\WinGet\Packages\Cemu.Cemu_*\Cemu_<ver>\Cemu.exe`, link in `%LOCALAPPDATA%\Microsoft\WinGet\Links\`. | `Cemu.Cemu` (current) | `-f -g <file>` |
| 3DS | Azahar | `azahar.exe` (UNVERIFIED casing) | Installer and portable zip. | Installer: `%ProgramFiles%\Azahar\`. | `AzaharEmu.Azahar` (current) | `-f <file>` |
| Switch | Ryubing | `Ryujinx.exe` | Portable zip/7z from git.ryujinx.app (UNVERIFIED asset names). | Anywhere. | none | `<file> --fullscreen` |
| PS2 | PCSX2 | `pcsx2-qt.exe` | Installer (Inno) and portable 7z. | Installer: `%ProgramFiles%\PCSX2\`; uninstall key `{13CEE6E5-8EB3-47D3-882E-E9DBB6A3251C}}_is1`. | `PCSX2Team.PCSX2` (current) | `-batch -fullscreen <file>` |
| PS3 | RPCS3 | `rpcs3.exe` | Portable zip from rpcs3.net (UNVERIFIED file name; page is Cloudflare-gated). | Anywhere (it writes next to itself, so not Program Files). | none | `--no-gui --fullscreen <file>` |
| PS4 | shadPS4 | `shadPS4.exe` | Portable zip. | winget: `%LOCALAPPDATA%\Microsoft\WinGet\Packages\shadps4-emu.shadPS4_*\shadPS4.exe`, link `...\WinGet\Links\shadPS4.exe`. | `shadps4-emu.shadPS4` (current) | `-g <file> --fullscreen true` |

## What this means for Game37

- Most emulators are portable, so auto-detection finds only installed copies (Azahar, PCSX2, an old Dolphin) and winget's folders (Cemu, shadPS4, also Azahar and PCSX2 when installed through winget). Choosing the `.exe` is the common case, not the exception.
- winget commands to show: Cemu, Azahar, PCSX2, shadPS4. Download pages for Dolphin (https://dolphin-emu.org/download/), Ryubing (https://ryujinx.app/), RPCS3 (https://rpcs3.net/download).
- Firmware and keys are handled by each emulator as on Linux (PS2 BIOS, PS3 firmware, Switch keys, PS4 firmware). Folder Games (Wii U `code\*.rpx`, PS3 `EBOOT.BIN`, PS4 `eboot.bin`) resolve the same way as on Linux; `.wua` files go straight to Cemu.

## Sources

- Dolphin: https://github.com/dolphin-emu/dolphin/blob/master/Installer/Installer.nsi, https://github.com/dolphin-emu/dolphin/blob/master/Readme.md, winget https://github.com/microsoft/winget-pkgs/blob/master/manifests/d/DolphinEmulator/Dolphin/5.0/DolphinEmulator.Dolphin.installer.yaml
- Cemu: https://github.com/cemu-project/Cemu/releases/tag/v2.6, https://github.com/microsoft/winget-pkgs/blob/master/manifests/c/Cemu/Cemu/2.6/Cemu.Cemu.installer.yaml
- Azahar: https://github.com/azahar-emu/azahar/releases/tag/2126.2, https://github.com/microsoft/winget-pkgs/blob/master/manifests/a/AzaharEmu/Azahar/2126.2/AzaharEmu.Azahar.installer.yaml
- Ryubing: https://docs.ryujinx.app/guides/setup-guide/
- PCSX2: https://pcsx2.net/docs/setup/running/, https://pcsx2.net/docs/advanced/cli/, https://github.com/microsoft/winget-pkgs/blob/master/manifests/p/PCSX2Team/PCSX2/2.8.2/PCSX2Team.PCSX2.installer.yaml
- RPCS3: https://github.com/RPCS3/rpcs3/pull/6407, https://github.com/RPCS3/rpcs3/pull/13370
- shadPS4: https://github.com/shadps4-emu/shadPS4/releases/tag/v.0.20.0, https://github.com/microsoft/winget-pkgs/blob/master/manifests/s/shadps4-emu/shadPS4/0.20.0/shadps4-emu.shadPS4.installer.yaml
