#!/usr/bin/env bash
# Builds the Switch Core from swigino/ so that it loads on any current Linux distribution, and checks
# that it does: run inside the manylinux_2_28 container (AlmaLinux 8, glibc 2.28, GCC 14), as Game37's
# GitHub Actions workflow does (.github/workflows/swigino-core.yml).
#
#   scripts/ci/build-swigino-portable.sh SRC BUILD OUT
#
# SRC is a Game37 checkout with swigino/'s submodules, BUILD a build folder (suyu's downloaded
# dependencies are kept in BUILD/cpm), OUT where the Core and its SHA-256 go. Everything suyu can
# bundle is built from source and linked in: FFmpeg (only the decoders suyu uses), OpenSSL, Opus,
# Boost, zlib, zstd, lz4. The Core needs only glibc and the C++ runtime from the system; libvulkan
# is loaded at run time, as before. FFmpeg is built without X11 and libdrm, which only its hardware
# decoding would use. -Bsymbolic lets FFmpeg's x86 assembly, which refers to its own
# constants directly, link into a shared library. cpp-httplib's getaddrinfo_a is in glibc 2.28's
# libanl (later glibc moved it into libc and keeps libanl.so.1 for programs like this). Locally:
#
#   docker run --rm -v "$PWD:/src:ro" -v "$HOME/.cache/game37/portable:/build" -v "$PWD/target/core:/out" \
#       quay.io/pypa/manylinux_2_28_x86_64 /src/scripts/ci/build-swigino-portable.sh /src /build /out
set -euo pipefail

src=$1
build=$2
out=$3

# The checkout belongs to another user inside the container; suyu's CMake runs git in it.
git config --global --add safe.directory '*'
dnf install -y -q nasm perl-IPC-Cmd perl-Time-Piece >/dev/null
# FFmpeg's configure finds libdrm by itself, which would leave the Core needing libdrm.so.
rpm -q libdrm-devel >/dev/null && rpm -e --nodeps libdrm-devel
if ! command -v ninja >/dev/null; then
    pipx install --quiet ninja
fi

# glslangValidator compiles suyu's host shaders while building. EL8 doesn't package it, so it is built
# from a pinned Khronos release, checked by its commit.
glslang_tag=16.6.0
glslang_commit=e1b562a8bed273a02f30b59b66a5d499793cede5
if [ ! -x "$build/tools/bin/glslangValidator" ]; then
    rm -rf "$build/glslang"
    git clone --quiet --depth 1 --branch "$glslang_tag" https://github.com/KhronosGroup/glslang "$build/glslang"
    if [ "$(git -C "$build/glslang" rev-parse HEAD)" != "$glslang_commit" ]; then
        echo "glslang $glslang_tag isn't the pinned commit $glslang_commit" >&2
        exit 1
    fi
    cmake -S "$build/glslang" -B "$build/glslang/build" -GNinja -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$build/tools" -DENABLE_OPT=OFF -DGLSLANG_TESTS=OFF -DBUILD_EXTERNAL=OFF
    cmake --build "$build/glslang/build" --target install
fi
PATH=$build/tools/bin:$PATH

cmake -S "$src/swigino" -B "$build/core" -GNinja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCPM_SOURCE_CACHE="$build/cpm" \
    -DYUZU_TZDB_PATH="$src/swigino/game37/nx_tzdb/230326" \
    -DENABLE_QT=OFF -DYUZU_CMD=OFF -DYUZU_TESTS=OFF \
    -DYUZU_ROOM=OFF -DYUZU_ROOM_STANDALONE=OFF \
    -DENABLE_WEB_SERVICE=OFF -DUSE_DISCORD_PRESENCE=OFF \
    -DSUYU_BUILD_LIBRETRO_CORE=ON \
    -DCPMUTIL_FORCE_BUNDLED=ON \
    -DYUZU_USE_BUNDLED_FFMPEG=OFF -DYUZU_USE_EXTERNAL_FFMPEG=ON \
    -DENABLE_LIBUSB=OFF -DENABLE_SDL3=OFF \
    -DCMAKE_DISABLE_FIND_PACKAGE_X11=TRUE \
    -DCMAKE_SHARED_LINKER_FLAGS="-Wl,-Bsymbolic -lanl"
cmake --build "$build/core" --target suyu_libretro -j "$(nproc)"

core=$build/core/src/libretro_core/suyu_libretro.so
mkdir -p "$out"
strip --strip-debug -o "$out/suyu_libretro.so" "$core"

# Only these may come from the system: glibc's libraries and the C++ runtime every distribution has.
allowed='^(libc\.so\.6|libm\.so\.6|libdl\.so\.2|libpthread\.so\.0|librt\.so\.1|libanl\.so\.1|ld-linux-x86-64\.so\.2|libstdc\+\+\.so\.6|libgcc_s\.so\.1)$'
needed=$(readelf -d "$out/suyu_libretro.so" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p')
echo "The Core needs: $(echo "$needed" | tr '\n' ' ')"
if extra=$(echo "$needed" | grep -Ev "$allowed"); then
    echo "The Core needs shared libraries a distribution may not have: $(echo "$extra" | tr '\n' ' ')" >&2
    exit 1
fi
# Load it the way a frontend does, so a symbol nothing provides fails here rather than in Game37.
/opt/python/cp312-cp312/bin/python3 -c 'import ctypes, sys; ctypes.CDLL(sys.argv[1], mode=2)' \
    "$out/suyu_libretro.so"
echo "The Core loads (dlopen with RTLD_NOW)."
glibc=$(objdump -T "$out/suyu_libretro.so" | grep -oE 'GLIBC_[0-9.]+' | sort -Vu | tail -1)
echo "Newest glibc symbol: $glibc"

size=$(stat -c %s "$out/suyu_libretro.so")
echo "Size: $size bytes"
if [ "$size" -gt $((64 * 1024 * 1024)) ]; then
    echo "The Core is over Game37's 64 MiB limit for Cores (MAX_CORE_BYTES)." >&2
    exit 1
fi
(cd "$out" && sha256sum suyu_libretro.so > suyu_libretro.so.sha256)
cat "$out/suyu_libretro.so.sha256"
