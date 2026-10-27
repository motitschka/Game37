# Cutting a release

These scripts build and check the release files in clean Docker containers. They read the version from
`Cargo.toml` and write only under `target/release-check/`:

- `pkg/`: the tarballs, the Windows installer and their `.sha256` files, the files to upload;
- `out/`: one log and one screenshot per check, and the binary each distro built;
- `vendor/`: the crates, so the containers build offline.

| Script | What it checks |
| --- | --- |
| `build.sh <arch\|ubuntu\|fedora\|debian>` | The README's "Build from source" packages, `cargo build --release --locked --offline`, `ldd`, one headless screenshot. For arch and ubuntu it also writes the tarball. |
| `runtime-check.sh <arch\|ubuntu>` | The tarball with only the README's "Prebuilt binary" packages: `sha256sum -c`, unpack, `ldd`, one headless screenshot. |
| `nix-check.sh` | The flake builds with its tests, and Game37 from it shows the menu and plays an N64 and a PSP Game. Needs `.testdata/` in the main checkout. |
| `windows.sh build` | Cross-compiles `game37.exe` for Windows with MinGW-w64 (ADR 0015) and collects the DLLs it needs in `windows/`. |
| `windows.sh test [filter]` | The unit tests built for Windows, run under Wine; `filter` is passed to the test binary. |
| `windows.sh clippy` | Clippy for the Windows target, all targets, `-D warnings`. |
| `windows.sh smoke` | `build`, then four headless screenshots under Wine, each with a two-minute timeout: Home (`windows-smoke.png`), First-run Setup's emulators step (`windows-setup-emulators.png`), the Console Page with an emulator missing (`windows-console-page-emulator-missing.png`) and Settings → Emulators (`windows-settings-emulators.png`). Copies the host's Adwaita Sans next to the exe for the run. |
| `windows.sh installer` | `build`, then the NSIS installer from `installer/game37.nsi`: `pkg/game37-<version>-x86_64-windows-setup.exe` and its `.sha256`. It holds `game37.exe` (with the app icon and version info), the SDL DLLs, `fonts\` with Adwaita Sans and its licence (downloaded from GNOME's `adwaita-fonts` release, checked by SHA-256), `LICENSE` and `README.md`. |
| `windows.sh installer-check` | That installer in a fresh Wine prefix, with a Config and a Games Root planted first: a silent install (`/S`), its files, Start menu shortcut and uninstall registry key; a screenshot with the installed `game37.exe` (`windows-installer-home.png`); a 20-second normal run, which writes `logs\game37.log` and `library.db`; a silent uninstall, after which the program folder, shortcut and key are gone and the Config, data and Games Root are unchanged. |

Both container scripts take the install command from `README.md` itself, so they also check that the
commands people copy work. The distro builds use the host's rustup stable toolchain.

`windows.sh` builds in `game37-windows-build:debian13`, an image it makes once from `debian:13` with
MinGW-w64, Wine (64- and 32-bit, as NSIS installers are 32-bit programs), NSIS and the tools that make
the icon from `data/game37.svg`, also with the host's toolchain (which needs the
`x86_64-pc-windows-gnu` target). Its builds, Wine prefix and the font download stay in
`windows-cache/`, so later runs are incremental.

## Steps

1. Set the version in `Cargo.toml` (and `Cargo.lock`, with `cargo build`), update `CHANGELOG.md` and the
   version in the README's Install section, and commit. The scripts build what git tracks, as it is in
   the working tree; each log's first line names the commit, with `-dirty` if a tracked file changed.
2. Build on every distro the README lists, one at a time (each takes about two minutes):

   ```bash
   for d in arch ubuntu fedora debian; do scripts/release/build.sh $d || break; done
   ```

3. Check the two tarballs as a user gets them:

   ```bash
   scripts/release/runtime-check.sh arch && scripts/release/runtime-check.sh ubuntu
   ```

4. Check the flake (the first run takes several minutes, later ones reuse the `/nix` volume):

   ```bash
   scripts/release/nix-check.sh
   ```

5. Check the Windows build (the first build takes several minutes):

   ```bash
   scripts/release/windows.sh clippy && scripts/release/windows.sh test && scripts/release/windows.sh smoke
   ```

6. Build the Windows installer and check it:

   ```bash
   scripts/release/windows.sh installer && scripts/release/windows.sh installer-check
   ```

## What to look at

- Each log in `target/release-check/out/` ends in `RESULT_OK <name>`. A script that fails stops there
  and shows the last lines of what failed.
- `<distro>.log`: the `PRETTY_NAME` line and the SDL3 version. If the README's package line doesn't
  install on a distro, fix the README rather than the script.
- Every screenshot (`*.png` in `out/`). The distro and runtime shots show Home with readable text.
  Ubuntu and Debian have no Adwaita Sans, so their text is in another font. `nix-menu.png` shows the
  Carousel and `nix-psp.png` CMFileManager running. `nix-n64.png` is black, because the N64 test demo
  never draws (see `docs/progress.md`); `nix-n64.log` shows that the Core ran its frames with OpenGL.
- `nix.log`: `test result: ok` lines (or "build cached" when the flake didn't change since the last
  run), and no `not found` from `ldd`.
- `windows-test.log`: `test result: ok`. `windows-smoke.png` shows Home with readable text and the
  Games Root as a Windows path (`Z:\cache\smoke\games`). The smoke run uses SDL's software renderer,
  because SDL's offscreen video driver finds no EGL under Wine; its log says so.
  `windows-setup-emulators.png`, `windows-console-page-emulator-missing.png` and
  `windows-settings-emulators.png` show First-run Setup's emulators step, the Console Page's
  missing-emulator notice and Settings → Emulators, each against its mockup in `design/mockups/`.
- `windows-installer.log`: `Nullsoft Installer` in the `file` line. `windows-installer-check.log`: the
  installed files, the uninstall key's values, the (normally empty) `game37.log` and `OK` for each kept
  file. `windows-installer-home.png` shows Home in Adwaita Sans, from the installed `fonts\`.
- The tarball contents: `tar -tzvf target/release-check/pkg/game37-<version>-x86_64-arch.tar.gz` lists
  `game37-<version>/` with `game37` (`rwxr-xr-x`), `README.md` and `LICENSE`, all `root/root`.

Then upload the two tarballs, the Windows installer and their `.sha256` files from `pkg/` to the
GitHub release for the tag `v<version>`. The installer is unsigned, so Windows SmartScreen warns about
it: "Windows protected your PC", with "More info" and then "Run anyway"; the UAC prompt names an
"Unknown publisher".

## Cleaning up

The scripts remove their containers (`--rm`). What stays:

```bash
docker rmi archlinux:latest ubuntu:26.04 fedora:44 debian:13 nixos/nix:latest game37-windows-build:debian13
docker volume rm game37-release-check-nix
rm -r target/release-check/vendor target/release-check/out/game37-* target/release-check/windows-cache
```

Keep `pkg/` until the files are uploaded.
