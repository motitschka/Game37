#!/usr/bin/env bash
# Cross-compiles Game37 for Windows (x86_64-pc-windows-gnu, ADR 0015) in a Debian container with
# MinGW-w64, and runs what it built under Wine there.
#
#   scripts/release/windows.sh build    game37.exe and the DLLs it needs, in target/release-check/windows/
#   scripts/release/windows.sh test [filter]   the unit tests built for Windows, run under Wine
#   scripts/release/windows.sh clippy   clippy for the Windows target, all targets, -D warnings
#   scripts/release/windows.sh smoke    build, then one headless screenshot under Wine (windows-smoke.png)
#   scripts/release/windows.sh installer   build, then the NSIS installer and its .sha256 in pkg/
#   scripts/release/windows.sh installer-check   that installer installed, run and uninstalled under Wine
#
# Like build.sh, the container uses the host's rustup stable toolchain (which has the
# x86_64-pc-windows-gnu rust-std) and a `cargo vendor` copy of the crates, so it builds offline with
# `--locked`. SDL3 and SDL3_ttf are built from source by the sdl3 crate (see Cargo.toml). The image,
# game37-windows-build:debian13, is built once from the Dockerfile below and reused. Builds and the
# Wine prefix (wine/) are kept in target/release-check/windows-cache/, so later runs are incremental. Each log
# in target/release-check/out/windows-<command>.log ends in "RESULT_OK windows-<command>".
set -euo pipefail
source "$(dirname "$0")/common.sh"

command=${1:?usage: windows.sh <build|test [filter]|clippy|smoke|installer|installer-check>}
image=game37-windows-build:debian13
toolchain=$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu
cache=$out/windows-cache
font=/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf
setup=game37-$version-x86_64-windows-setup.exe

dockerfile() {
    cat <<'EOF'
FROM debian:13
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        gcc libc6-dev gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 binutils-mingw-w64-x86-64 cmake make pkg-config \
        wine wine64 xvfb xauth libgl1-mesa-dri file ca-certificates \
    && rm -rf /var/lib/apt/lists/*
# For the installer: NSIS, the icon made from the SVG, the font download, and 32-bit Wine, because
# NSIS installers are 32-bit programs.
RUN dpkg --add-architecture i386 && apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        nsis librsvg2-bin icoutils curl xz-utils wine32:i386 \
    && rm -rf /var/lib/apt/lists/*
EOF
}

# -mbig-obj: HarfBuzz's debug build has more sections than a plain COFF object file allows.
# The tests run with Wine's 64-bit loader: with 32-bit Wine installed too, `wine` starts the 32-bit
# one, which can't start the test binaries ("could not load kernel32.dll").
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
        -e CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=/usr/lib/wine/wine64 \
        -e WINEPREFIX=/cache/wine \
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
# The app icon that build.rs puts in game37.exe, made from the SVG at the sizes Windows shows. It is
# only replaced when it changes, so that builds stay incremental.
mkdir -p /tmp/icon /cache/icon
for size in 16 24 32 48 64 128 256; do
    rsvg-convert -w $size -h $size data/game37.svg -o /tmp/icon/$size.png
done
icotool -c -o /tmp/icon/game37.ico /tmp/icon/{16,24,32,48,64,128}.png --raw=/tmp/icon/256.png
cmp -s /tmp/icon/game37.ico /cache/icon/game37.ico || cp /tmp/icon/game37.ico /cache/icon/game37.ico
export GAME37_ICO=/cache/icon/game37.ico
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

# Runs after build_script: the installer from installer/game37.nsi, with what the build collected,
# Adwaita Sans and its licence, LICENSE and README.md, written to /out/$setup.
installer_script() {
    cat <<'EOF'
# Adwaita Sans (SIL OFL 1.1) from GNOME's release archive, checked against the SHA-256 GNOME publishes.
fonts=adwaita-fonts-51.0
fonts_sha256=fa104ae2c1b96580d322f563ffe8b2dfca5296b0eca4971efd2d8011405123d2
archive=/cache/downloads/$fonts.tar.xz
if ! sha256sum --status -c <<<"$fonts_sha256  $archive" 2>/dev/null; then
    mkdir -p /cache/downloads
    curl -sSfL -o "$archive" "https://download.gnome.org/sources/adwaita-fonts/51/$fonts.tar.xz"
    sha256sum -c <<<"$fonts_sha256  $archive"
fi
stage=/tmp/stage
mkdir -p $stage/fonts
cp /out/game37.exe /out/*.dll /tmp/build/LICENSE /tmp/build/README.md "$GAME37_ICO" $stage/
tar -xJf "$archive" -O "$fonts/sans/AdwaitaSans-Regular.ttf" >$stage/fonts/AdwaitaSans-Regular.ttf
tar -xJf "$archive" -O "$fonts/LICENSE" >$stage/fonts/AdwaitaSans-LICENSE.txt
# The installer lists its files, so every DLL the build collected must be in that list.
for dll in /out/*.dll; do
    grep -qF "File \"\${STAGE}\\${dll#/out/}\"" /tmp/build/installer/game37.nsi ||
        { echo "installer/game37.nsi doesn't install ${dll#/out/}" >&2 && exit 1; }
done
makensis -V2 -DVERSION="$VERSION" -DVERSION_NUM="${VERSION%%-*}.0" -DSTAGE=$stage -DOUTFILE="/out/$SETUP" \
    /tmp/build/installer/game37.nsi
file "/out/$SETUP"
EOF
}

# In a fresh Wine prefix with a Config and a Games Root in place: installs /cache/installer-check/setup.exe
# silently and checks its files, shortcut and uninstall key; takes a screenshot with the installed
# game37.exe; runs it normally for a while, which writes the log and the library database; then
# uninstalls silently and checks that the program is gone and the Config, data and Games Root stayed.
installer_check_script() {
    cat <<'EOF'
fail() { echo "FAILED: $*" >&2 && exit 1; }
export WINEPREFIX=/cache/installer-check/prefix
cd /cache/installer-check
wineboot --init && wineserver -w
winvar() { wine cmd /c echo "%$1%" | tr -d '\r'; }
unix() { winepath -u "$1" | tr -d '\r'; }
prog="$(winvar ProgramW6432)\Game37"
prog_u=$(unix "$prog")
start_menu_u=$(unix "$(winvar ProgramData)\Microsoft\Windows\Start Menu\Programs")
config_u=$(unix "$(winvar APPDATA)")/game37
data_u=$(unix "$(winvar LOCALAPPDATA)")/game37
games="$(winvar USERPROFILE)\Games"
games_u=$(unix "$games")
key='HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\Game37'

# What a player has before installing: a Config and a Games Root with a Game in it.
mkdir -p "$config_u" "$games_u/nes"
printf "games_root = '%s'\n" "$games" >"$config_u/config.toml"
echo "not a real Game" >"$games_u/nes/planted.nes"

echo "== install"
wine setup.exe /S && wineserver -w
expected="LICENSE README.md SDL3.dll SDL3_ttf.dll fonts fonts/AdwaitaSans-LICENSE.txt \
fonts/AdwaitaSans-Regular.ttf game37.exe uninstall.exe"
installed=$(cd "$prog_u" && find . -mindepth 1 | sed 's|^\./||' | LC_ALL=C sort | xargs)
echo "$prog: $installed"
[[ $installed == "$expected" ]] || fail "expected $expected"
[[ -f $start_menu_u/Game37.lnk ]] || fail "no Start menu shortcut"
wine reg query "$key" /reg:64 | tr -d '\r' | tee reg.txt
values=$(sed 's/^ *//; s/  */ /g' reg.txt)
for value in "DisplayName REG_SZ Game37" "DisplayVersion REG_SZ $VERSION" "Publisher REG_SZ Matei Novotny" \
    "DisplayIcon REG_SZ $prog\game37.exe,0" "InstallLocation REG_SZ $prog" \
    "UninstallString REG_SZ \"$prog\uninstall.exe\"" \
    "QuietUninstallString REG_SZ \"$prog\uninstall.exe\" /S" \
    "NoModify REG_DWORD 0x1" "NoRepair REG_DWORD 0x1"; do
    grep -qFx "$value" <<<"$values" || fail "no '$value' in the uninstall key"
done
grep -qE '^EstimatedSize REG_DWORD 0x[1-9a-f]' <<<"$values" || fail "no EstimatedSize"

echo "== screenshot with the installed game37.exe"
mkdir -p shot/games shot/data
printf '%s\n' "games_root = 'Z:\cache\installer-check\shot\games'" >shot/config.toml
timeout -k 10 120 xvfb-run -a wine "$prog\game37.exe" --screenshot 'Z:\cache\installer-check\home.bmp' \
    --config 'Z:\cache\installer-check\shot\config.toml' --data-dir 'Z:\cache\installer-check\shot\data' \
    --actions "" || fail "the screenshot run failed or timed out ($?)"
[[ -s home.bmp ]] || fail "no screenshot"
# Wine's desktop process from that run must end before the next run brings up another X server.
wineserver -w

echo "== a normal run of the installed game37.exe, stopped after 20 s"
status=0
timeout -k 5 20 xvfb-run -a -s "-screen 0 1920x1080x24" wine "$prog\game37.exe" || status=$?
wineserver -k || true
wineserver -w
echo "-- logs\game37.log, empty when nothing went wrong:"
cat "$data_u/logs/game37.log" || fail "no log"
[[ $status == 124 ]] || fail "game37.exe ended by itself ($status)"
kept=("$config_u/config.toml" "$data_u/library.db" "$data_u/logs/game37.log" "$games_u/nes/planted.nes")
sha256sum "${kept[@]}" >kept.sha256 || fail "missing Config, data or Game"

echo "== uninstall"
wine "$prog\uninstall.exe" /S && wineserver -w
[[ ! -e $prog_u ]] || fail "left in $prog: $(cd "$prog_u" && find . -mindepth 1)"
[[ ! -e $start_menu_u/Game37.lnk ]] || fail "the Start menu shortcut is still there"
! wine reg query "$key" /reg:64 >/dev/null 2>&1 || fail "the uninstall key is still there"
echo "kept after uninstalling:" && sha256sum -c kept.sha256 || fail "the Config, data or Games Root changed"
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
    installer)
        in_container "VERSION='$version' SETUP='$setup'
$(build_script)
$(installer_script)"
        mkdir -p "$out/pkg" && mv "$out/windows/$setup" "$out/pkg/"
        (cd "$out/pkg" && sha256sum "$setup" | tee "$setup.sha256")
        ;;
    installer-check)
        [[ -f $out/pkg/$setup ]] || { echo "no pkg/$setup: run 'windows.sh installer' first" >&2 && exit 1; }
        rm -rf "$cache/installer-check" && mkdir -p "$cache/installer-check"
        cp "$out/pkg/$setup" "$cache/installer-check/setup.exe"
        in_container "VERSION='$version'
$(installer_check_script)"
        mv "$cache/installer-check/home.bmp" "$out/out/windows-installer-home.bmp"
        to_png "$out/out/windows-installer-home.bmp"
        ;;
    *) echo "unknown command '$command'" >&2 && exit 1 ;;
    esac
    echo "RESULT_OK windows-$command"
}

mkdir -p "$out/out" "$out/windows"
main "$@" 2>&1 | tee "$out/out/windows-$command.log"
