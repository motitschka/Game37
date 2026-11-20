#!/usr/bin/env bash
# Builds the Switch Core that plays Switch games inside Game37 (ADR 0019) from swigino/, Game37's
# own fork of suyu's libretro Core, and installs it where Game37 looks for Cores.
#
#   scripts/build-swigino-core.sh [--data-dir DIR] [--jobs N]
#
# swigino/ is suyu's source with Game37's changes: the Core connects a controller, keeps its files
# in Game37's folders and pauses under Game37's Pause Menu (swigino/SWIGINO.md). Its submodules are
# fetched into this checkout the first time (shallow), and while configuring, suyu's other bundled
# dependencies (about 700 MB, kept in the build cache). It needs a git checkout of Game37, a C++20
# compiler, Vulkan, SDL3, OpenSSL, Opus and FFmpeg development files. cmake and ninja are used from
# the system when present, else from a private Python venv in the build cache. Nothing is installed
# system-wide and no sudo is needed.
#
# It never downloads keys, firmware or games: Switch games need keys dumped from your own Switch
# (see Settings → BIOS).
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
data_dir=${XDG_DATA_HOME:-$HOME/.local/share}/game37
jobs=$(nproc)
while [ $# -gt 0 ]; do
    case $1 in
        --data-dir) data_dir=$2; shift 2 ;;
        --jobs) jobs=$2; shift 2 ;;
        -h | --help) sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option $1" >&2; exit 2 ;;
    esac
done

if ! git -C "$repo" rev-parse --git-dir >/dev/null 2>&1; then
    echo "This needs a git checkout of Game37 (swigino/'s submodules come through git)." >&2
    exit 1
fi
echo "Fetching swigino's submodules (the first time takes a while)"
git -C "$repo" submodule update --init --recursive --depth 1 --jobs 8 -- swigino

cache=${XDG_CACHE_HOME:-$HOME/.cache}/game37/suyu-build
mkdir -p "$cache"

# cmake and ninja: the system's, else a private venv's.
if ! command -v cmake >/dev/null || ! command -v ninja >/dev/null; then
    if [ ! -x "$cache/venv/bin/cmake" ]; then
        echo "Setting up cmake and ninja in $cache/venv"
        python3 -m venv "$cache/venv"
        "$cache/venv/bin/pip" install --quiet cmake ninja
    fi
    PATH=$cache/venv/bin:$PATH
fi

build=$cache/swigino
echo "Configuring (fetches suyu's bundled dependencies the first time)"
# suyu's downloaded dependencies go in the build cache, not in swigino/.cache inside the checkout.
cmake -S "$repo/swigino" -B "$build" -GNinja \
    -DCMAKE_BUILD_TYPE=Release -DCPM_SOURCE_CACHE="$cache/cpm" \
    -DYUZU_TZDB_PATH="$repo/swigino/game37/nx_tzdb/230326" \
    -DENABLE_QT=OFF -DYUZU_CMD=OFF -DYUZU_TESTS=OFF \
    -DYUZU_ROOM=OFF -DYUZU_ROOM_STANDALONE=OFF \
    -DENABLE_WEB_SERVICE=OFF -DUSE_DISCORD_PRESENCE=OFF \
    -DSUYU_BUILD_LIBRETRO_CORE=ON \
    -Dfmt_FORCE_BUNDLED=ON \
    -DVulkanHeaders_FORCE_BUNDLED=ON -DVulkanUtilityLibraries_FORCE_BUNDLED=ON \
    -DYUZU_USE_BUNDLED_FFMPEG=OFF
echo "Building (the first build takes a while)"
cmake --build "$build" --target suyu_libretro -j "$jobs"

mkdir -p "$data_dir/cores"
installed=$data_dir/cores/swigino_libretro.so
cp "$build/src/libretro_core/swigino_libretro.so" "$installed.part"
strip --strip-debug "$installed.part"
mv "$installed.part" "$installed"
echo "Installed $installed"
