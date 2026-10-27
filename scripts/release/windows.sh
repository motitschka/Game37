#!/usr/bin/env bash
# Cross-compiles Game37 for Windows (x86_64-pc-windows-gnu, ADR 0015) in a Debian container with
# MinGW-w64, and runs what it built under Wine there.
#
#   scripts/release/windows.sh build    game37.exe and the DLLs it needs, in target/release-check/windows/
#   scripts/release/windows.sh test [filter]   the unit tests built for Windows, run under Wine
#   scripts/release/windows.sh clippy   clippy for the Windows target, all targets, -D warnings
#   scripts/release/windows.sh smoke    build, then one headless screenshot under Wine (windows-smoke.png)
#
# Like build.sh, the container uses the host's rustup stable toolchain (which has the
# x86_64-pc-windows-gnu rust-std) and a `cargo vendor` copy of the crates, so it builds offline with
# `--locked`. SDL3 and SDL3_ttf are built from source by the sdl3 crate (see Cargo.toml). The image,
# game37-windows-build:debian13, is built once from the Dockerfile below and reused. Builds and the
# Wine prefix are kept in target/release-check/windows-cache/, so later runs are incremental. Each log
# in target/release-check/out/windows-<command>.log ends in "RESULT_OK windows-<command>".
set -euo pipefail
source "$(dirname "$0")/common.sh"

command=${1:?usage: windows.sh <build|test [filter]|clippy|smoke>}
image=game37-windows-build:debian13
toolchain=$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu
cache=$out/windows-cache
font=/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf

dockerfile() {
    cat <<'EOF'
FROM debian:13
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        gcc libc6-dev gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 binutils-mingw-w64-x86-64 cmake make pkg-config \
        wine wine64 xvfb xauth libgl1-mesa-dri file ca-certificates \
    && rm -rf /var/lib/apt/lists/*
EOF
}

# -mbig-obj: HarfBuzz's debug build has more sections than a plain COFF object file allows.
# --init: bash runs a script's last command in its own place, and xvfb-run as PID 1 waits forever,
# because Xvfb never tells PID 1 that it is ready.
# in_container <script>: runs <script> with bash in the container, as the host user, with the source
# in /tmp/build, the crates in /vendor, the toolchain in /rust, the cache in /cache and the results in /out.
in_container() {
    source_tar "$out/src-windows.tar"
    docker run --rm --init --user "$(id -u):$(id -g)" \
        -v "$out/src-windows.tar:/src.tar:ro" \
        -v "$toolchain:/rust:ro" \
        -v "$out/vendor:/vendor:ro" \
        -v "$cache:/cache" \
        -v "$out/windows:/out" \
        -e PATH=/rust/bin:/usr/local/bin:/usr/bin:/bin \
        -e HOME=/cache/home \
        -e CARGO_TARGET_DIR=/cache/target \
        -e CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
        -e CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine \
        -e WINEDEBUG=-all \
        -e CXXFLAGS_x86_64_pc_windows_gnu=-Wa,-mbig-obj \
        "$image" bash -c "$(prelude)
$1"
    rm "$out/src-windows.tar"
}

# What every container script starts with: cargo set up for the vendored crates, the source unpacked,
# and `runtime_path`, the folders a Windows binary built here finds its DLLs in.
prelude() {
    cat <<'EOF'
set -euo pipefail
mkdir -p "$HOME/.cargo" /tmp/build
printf '[source.crates-io]\nreplace-with = "vendored"\n[source.vendored]\ndirectory = "/vendor"\n' \
    > "$HOME/.cargo/config.toml"
tar -C /tmp/build -xf /src.tar && cd /tmp/build
target=x86_64-pc-windows-gnu
mingw_dirs="$(dirname "$(x86_64-w64-mingw32-gcc -print-libgcc-file-name)") /usr/x86_64-w64-mingw32/lib"
# runtime_path <profile>: the folders with the SDL DLLs of that profile's build, and MinGW's own.
runtime_path() {
    local dirs
    dirs=$(find "/cache/target/$target/$1/build" -name 'SDL3*.dll' -printf '%h\n' | sort -u)
    echo $dirs $mingw_dirs | tr ' ' ';'
}
EOF
}

build_script() {
    cat <<'EOF'
cargo build --release --locked --offline --target $target 2>&1 | grep -v "^ *Compiling "
rm -rf /out/* && cp "/cache/target/$target/release/game37.exe" /out/
# Every DLL the exe needs that isn't part of Windows, found by following imports: SDL's own, then
# whatever of MinGW's runtime they link.
IFS=';' read -ra search <<<"$(runtime_path release)"
todo=(/out/game37.exe)
while ((${#todo[@]})); do
    file=${todo[0]} && todo=("${todo[@]:1}")
    for dll in $(x86_64-w64-mingw32-objdump -p "$file" | sed -n 's/^\tDLL Name: //p'); do
        [[ -e /out/$dll ]] && continue
        for dir in "${search[@]}"; do
            if [[ -e $dir/$dll ]]; then
                cp "$dir/$dll" /out/ && todo+=("/out/$dll") && break
            fi
        done
    done
done
cd /out && file -- * && ls -l
EOF
}

main() {
    echo "game37 $version from $(git -C "$repo" describe --always --dirty), windows $command in $image"
    dockerfile | docker build -q -t "$image" - >/dev/null
    cargo vendor --quiet --locked --manifest-path "$repo/Cargo.toml" "$out/vendor" >/dev/null
    mkdir -p "$cache/home" "$cache/target"
    case $command in
    build) in_container "$(build_script)" ;;
    clippy) in_container 'cargo clippy --locked --offline --target $target --all-targets -- -D warnings' ;;
    test)
        in_container '
export WINEPATH=$(cargo test --locked --offline --target $target --no-run 2>&1 | tail -1 >&2; runtime_path debug)
xvfb-run -a cargo test --locked --offline --target $target -- '"${2:-}"' 2>&1 | grep -v "^test .* ok$"
'
        ;;
    smoke)
        in_container "$(build_script)"
        mkdir -p "$out/windows/fonts" && cp "$font" "$out/windows/fonts/"
        # A Windows-style Config and data folder, made fresh in the cache, which Wine sees as Z:.
        rm -rf "$cache/smoke" && mkdir -p "$cache/smoke/games" "$cache/smoke/data"
        printf '%s\n' "games_root = 'Z:\\cache\\smoke\\games'" >"$cache/smoke/config.toml"
        # It takes seconds; the timeout is there so that a hang fails the check instead of waiting.
        in_container 'cd /out && timeout -k 10 120 xvfb-run -a wine game37.exe --screenshot smoke.bmp \
    --config "Z:\cache\smoke\config.toml" --data-dir "Z:\cache\smoke\data" --actions "" ||
    { echo "the screenshot run failed or timed out ($?)" >&2 && exit 1; }'
        rm -r "$out/windows/fonts"
        mv "$out/windows/smoke.bmp" "$out/out/windows-smoke.bmp"
        to_png "$out/out/windows-smoke.bmp"
        ;;
    *) echo "unknown command '$command'" >&2 && exit 1 ;;
    esac
    echo "RESULT_OK windows-$command"
}

mkdir -p "$out/out" "$out/windows"
main "$@" 2>&1 | tee "$out/out/windows-$command.log"
