# swigino

swigino is the libretro Core that plays Switch Games inside Game37. It is Game37's own fork of
[suyu](https://github.com/suyu-emu/suyu-main) (GPL-3.0-or-later): this folder is suyu's source plus
Game37's changes, and it follows suyu's releases. Only suyu's libretro Core is built from it.
suyu's own README is `README.md` next to this file. Game37's plan for swigino is M13 in
`docs/core-milestones.md`.

## Where it comes from

A `suyu-commit:` line in the commit that imported suyu, or last moved it to a newer suyu, names the
suyu commit this folder is based on:

```bash
git log --format=%B -- swigino | grep -m1 '^suyu-commit:'
```

Today that is `347fcf91`, the commit suyu's `v14.1.0` source archive was cut from (suyu later
re-tagged that release as `v0.1.4` on a newer commit). The folder holds every file of that commit
except the ones suyu's `.gitattributes` marks `export-ignore`: `tools-local/` and prebuilt
`.exe`/`.obj`/`.bin` files, which suyu calls "not release source artifacts". Its submodules stay
links to their own projects, listed in Game37's root `.gitmodules` under `swigino/`.

## What Game37 changes

One Game37 commit each, all listed by `git log -- swigino`:

- the emulated controller the RetroPad drives is connected, so Games see input
- on desktop, the Core keeps its files in the frontend's folders: config, caches, logs and keys
  under `<system>/suyu`, the emulated NAND and SD card (the Game saves) under `<save>/suyu`
- the emulated Switch pauses when the frontend stops calling `retro_run`
- a GCC 16 false-positive warning is silenced
- FFmpeg builds from source natively (`YUZU_USE_EXTERNAL_FFMPEG`), instead of always as a cross
  build
- `game37/nx_tzdb/230326/` holds the time zone data suyu would download from `git.eden-emu.dev`;
  the build scripts pass it as `YUZU_TZDB_PATH`

The changes are meant to go back to suyu as pull requests. Once suyu carries one, Game37's own
change drops out at the next update.

This folder never contains keys, firmware or Games, and the Core never downloads any. Encrypted
Games need keys dumped from your own Switch.

## Building

`scripts/build-swigino-core.sh` fetches the submodules (shallow), builds the Core in
`~/.cache/game37/suyu-build/swigino` and installs it where Game37 looks for Cores. See its header for
what it needs. `scripts/ci/build-swigino-portable.sh` builds the Core Game37 publishes, one that loads
on any current distribution, in the manylinux_2_28 container; GitHub Actions runs it
(`.github/workflows/swigino-core.yml`).

## Moving to a newer suyu

With a clone of suyu at hand (`$SUYU`), `$OLD` the commit on that line and `$NEW` the one to
move to:

1. Apply suyu's changes between the two to this folder, leaving out its `export-ignore` paths
   (check suyu's `.gitattributes` for new ones):
   `git -C "$SUYU" diff --binary "$OLD" "$NEW" -- . ':!tools-local' ':!tools/static_recompiler/out'
   ':!tools/static_recompiler/*.exe' ':!tools/static_recompiler/*.obj'
   ':!tools/static_recompiler/*.bin' | git apply -3 --directory=swigino`, then resolve any conflicts
   with Game37's changes.
2. If `swigino/.gitmodules` changed, regenerate the root `.gitmodules` from it, each path under
   `swigino/`.
3. Commit with a line `suyu-commit: $NEW` in the message. Then build the Core and run Game37's
   real-Core tests (`cargo test -- --ignored`), smoke shots and the Switch manual checks before it
   goes further.
