# Windows support: a native build with an installer, emulators found or picked

Game37 also runs on Windows 10 and 11 (x86_64), with the same screens and the same split between Embedded and External Systems (ADR 0001). The owner decided this before 1.0, and a tester checks it on a real Windows PC.

- **Build:** the `x86_64-pc-windows-gnu` target, cross-compiled from Linux in a container with MinGW-w64. The MSVC target would need Microsoft's build tools and their licence accepted on a Windows machine or through `xwin`; the GNU target needs neither, and SDL3, SQLite and the libretro Cores work the same with it.
- **Shipping:** an NSIS installer (`makensis` runs on Linux) that installs into Program Files with a Start menu entry and an uninstaller. Uninstalling removes the program only, never the Games Root, saves or Game37's data.
- **Cores:** the Windows builds (`.dll`) from the same libretro build server (ADR 0004), from `nightly/windows/x86_64/latest/`.
- **External Systems:** there is no Flatpak on Windows, so ADR 0007's Flatpak-only emulators apply to Linux. On Windows Game37 looks for each emulator in its usual install places (its installer's folder and registry entry, winget's install and link folders), and when it isn't found the player chooses its `.exe` once; the choice is kept in the Config. Where an emulator has a winget package, Setup and the missing-emulator notice show the `winget install` command, as Linux shows `flatpak install`.
- **Places:** the Config in `%APPDATA%\game37\`, Game37's data (library database, Cores, Artwork, logs) in `%LOCALAPPDATA%\game37\`, and the default Games Root `%USERPROFILE%\Games`.
- **OpenGL:** every Core's context is made through SDL on Windows. The GLX host that PPSSPP needs on Linux (where its `glewInit()` fails on EGL) is Linux-only.

## Consequences

Linux behaviour does not change. Code that differs by platform sits behind `cfg(windows)` / `cfg(unix)` in the few modules that touch the platform (paths, Cores, emulators, the file manager, the OpenGL host), not spread through screens. Automated checks for Windows are the cross-compiled build, the unit tests run under Wine, and a headless screenshot under Wine; only the tester's checks on real Windows count for "done".
