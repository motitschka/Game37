#!/usr/bin/env bash
# Builds the suyu libretro Core that plays Switch games inside Game37 (ADR 0019) and installs it
# where Game37 looks for Cores.
#
#   scripts/build-suyu-core.sh [--data-dir DIR] [--jobs N]
#
# suyu's own release Core can't be used as it is: it is built for Ubuntu's libraries, never
# connects a controller (so games see no input), writes to suyu's own folders instead of Game37's,
# and keeps running under Game37's Pause Menu. This script builds suyu's pinned release from
# source with Game37's patches (patches/suyu/) that fix those.
#
# It downloads suyu's source (about 90 MB) and, while configuring, its bundled dependencies (about
# 700 MB, kept in the build cache), and needs a C++20 compiler, Vulkan, SDL3, OpenSSL, Opus and
# FFmpeg development files. cmake and ninja are used from the system when present, else from a
# private Python venv in the build cache. Nothing is installed system-wide and no sudo is needed.
#
# It never downloads keys, firmware or games: Switch games need keys dumped from your own Switch
# (see Settings → BIOS).
set -euo pipefail

SUYU_VERSION=v14.1.0
SUYU_SOURCE_URL="https://github.com/suyu-emu/suyu-main/releases/download/${SUYU_VERSION}/suyu-${SUYU_VERSION}-source.tar.gz"
SUYU_SOURCE_SHA256=773fc0b3fbd34ab572717e75dd3eeba6ec12d0ef80893d28d37b37fdbab9830a

repo=$(cd "$(dirname "$0")/.." && pwd)
data_dir=${XDG_DATA_HOME:-$HOME/.local/share}/game37
jobs=$(nproc)
while [ $# -gt 0 ]; do
    case $1 in
        --data-dir) data_dir=$2; shift 2 ;;
        --jobs) jobs=$2; shift 2 ;;
        -h | --help) sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option $1" >&2; exit 2 ;;
    esac
done

cache=${XDG_CACHE_HOME:-$HOME/.cache}/game37/suyu-build
mkdir -p "$cache"
cd "$cache"

# cmake and ninja: the system's, else a private venv's.
if ! command -v cmake >/dev/null || ! command -v ninja >/dev/null; then
    if [ ! -x venv/bin/cmake ]; then
        echo "Setting up cmake and ninja in $cache/venv"
        python3 -m venv venv
        venv/bin/pip install --quiet cmake ninja
    fi
    PATH=$cache/venv/bin:$PATH
fi

archive=suyu-${SUYU_VERSION}-source.tar.gz
if [ ! -f "$archive" ]; then
    echo "Downloading suyu $SUYU_VERSION source"
    curl -fL --progress-bar -o "$archive.part" "$SUYU_SOURCE_URL"
    mv "$archive.part" "$archive"
fi
echo "$SUYU_SOURCE_SHA256  $archive" | sha256sum -c --quiet - || {
    echo "The suyu source archive doesn't match its pinned checksum; delete $cache/$archive and retry." >&2
    exit 1
}

src=$cache/suyu-${SUYU_VERSION}-source
stamp=$src/.game37-patched
patches=$(cat "$repo"/patches/suyu/*.patch | sha256sum | cut -d' ' -f1)
if [ ! -f "$stamp" ] || [ "$(cat "$stamp")" != "$patches" ]; then
    echo "Unpacking and patching"
    rm -rf "$src"
    tar xzf "$archive"
    for p in "$repo"/patches/suyu/*.patch; do
        patch -d "$src" -p1 --quiet < "$p"
    done
    echo "$patches" > "$stamp"
fi

cd "$src"
if [ ! -f build/build.ninja ]; then
    echo "Configuring (fetches suyu's bundled dependencies the first time)"
    cmake -B build -GNinja \
        -DCMAKE_BUILD_TYPE=Release \
        -DENABLE_QT=OFF -DYUZU_CMD=OFF -DYUZU_TESTS=OFF \
        -DYUZU_ROOM=OFF -DYUZU_ROOM_STANDALONE=OFF \
        -DENABLE_WEB_SERVICE=OFF -DUSE_DISCORD_PRESENCE=OFF \
        -DSUYU_BUILD_LIBRETRO_CORE=ON \
        -Dfmt_FORCE_BUNDLED=ON \
        -DVulkanHeaders_FORCE_BUNDLED=ON -DVulkanUtilityLibraries_FORCE_BUNDLED=ON \
        -DYUZU_USE_BUNDLED_FFMPEG=OFF
fi
echo "Building (the first build takes a while)"
cmake --build build --target suyu_libretro -j "$jobs"

mkdir -p "$data_dir/cores"
installed=$data_dir/cores/suyu_libretro.so
cp build/src/libretro_core/suyu_libretro.so "$installed.part"
strip --strip-debug "$installed.part"
mv "$installed.part" "$installed"
echo "Installed $installed"
