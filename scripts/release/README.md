# Cutting a release

These scripts build and check the release files in clean Docker containers. They read the version from
`Cargo.toml` and write only under `target/release-check/`:

- `pkg/`: the tarballs and their `.sha256` files, the files to upload;
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
| `windows.sh smoke` | `build`, then one headless screenshot under Wine (`windows-smoke.png`), with a two-minute timeout. Copies the host's Adwaita Sans next to the exe for the run. |

Both container scripts take the install command from `README.md` itself, so they also check that the
commands people copy work. The distro builds use the host's rustup stable toolchain.

`windows.sh` builds in `game37-windows-build:debian13`, an image it makes once from `debian:13` with
MinGW-w64 and Wine, also with the host's toolchain (which needs the `x86_64-pc-windows-gnu` target).
Its builds and Wine prefix stay in `windows-cache/`, so later runs are incremental.

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
- The tarball contents: `tar -tzvf target/release-check/pkg/game37-<version>-x86_64-arch.tar.gz` lists
  `game37-<version>/` with `game37` (`rwxr-xr-x`), `README.md` and `LICENSE`, all `root/root`.

Then upload the two tarballs and the two `.sha256` files from `pkg/` to the GitHub release for the tag
`v<version>`.

## Cleaning up

The scripts remove their containers (`--rm`). What stays:

```bash
docker rmi archlinux:latest ubuntu:26.04 fedora:44 debian:13 nixos/nix:latest game37-windows-build:debian13
docker volume rm game37-release-check-nix
rm -r target/release-check/vendor target/release-check/out/game37-* target/release-check/windows-cache
```

Keep `pkg/` until the files are uploaded.
