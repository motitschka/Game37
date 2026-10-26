#!/usr/bin/env bash
# Builds the Nix flake in a clean nixos/nix container, with the flake's tests, then starts Game37 from
# it headless with a NixOS-like /run/opengl-driver and /etc/fonts: the menu, an N64 Game and a PSP
# Game (under xvfb-run) with real Cores.
#
#   scripts/release/nix-check.sh
#
# Needs the test Cores and Games in .testdata/ of the main checkout (see CLAUDE.md). Keeps /nix in
# the Docker volume game37-release-check-nix, so a second run is quick. Writes
# target/release-check/out/nix.log, nix-<shot>.log and nix-<shot>.png; nix.log ends in
# "RESULT_OK nix" when everything passed.
set -euo pipefail
source "$(dirname "$0")/common.sh"

testdata=$(git -C "$repo" rev-parse --path-format=absolute --git-common-dir)/../.testdata
[ -d "$testdata/data/cores" ] || { echo "missing $testdata (Cores and test Games)" >&2 && exit 1; }

main() {
    echo "game37 $version from $(git -C "$repo" describe --always --dirty), flake in nixos/nix:latest"
    source_tar "$out/src-nix.tar"
    # PPSSPP needs more than Docker's 64 MB /dev/shm.
    docker run --rm --shm-size=1g \
        -v "$out/src-nix.tar:/src.tar:ro" \
        -v "$(realpath "$testdata"):/testdata:ro" \
        -v "$out/out:/out" \
        -v game37-release-check-nix:/nix \
        -e HOST_ID="$(id -u):$(id -g)" \
        nixos/nix:latest bash -c '
set -euo pipefail
trap "chown -R \$HOST_ID /out" EXIT
nix() { command nix --extra-experimental-features "nix-command flakes" "$@"; }
pkg() { local paths=($(nix build --no-link --print-out-paths --inputs-from path:/work "nixpkgs#$1")); echo "$paths"; }
mkdir -p /work && tar -C /work -xf /src.tar && cd /work
echo "nixpkgs $(grep -m1 "\"rev\"" flake.lock)"
nix build path:/work --no-update-lock-file -L --print-out-paths --no-link > /tmp/game37 2>/tmp/build.log ||
    { tail -40 /tmp/build.log; exit 1; }
grep -E "test result: |FAILED" /tmp/build.log || echo "build cached, tests ran in an earlier build"
out=$(cat /tmp/game37)
"$(pkg glibc.bin)/bin/ldd" "$out/bin/.game37-wrapped" | tee /tmp/ldd |
    grep -E "libGL|libz|libstdc|libgomp|libgcc_s"
if grep "not found" /tmp/ldd; then exit 1; fi
cat "$out/share/applications/game37.desktop"
ls "$out/share/icons/hicolor/scalable/apps/game37.svg"
mkdir -p /run /etc/fonts && ln -s "$(pkg mesa)" /run/opengl-driver
printf "<?xml version=\"1.0\"?>\n<fontconfig><dir>%s/share/fonts</dir><cachedir>/tmp/fc</cachedir></fontconfig>\n" \
    "$(pkg adwaita-fonts)" > /etc/fonts/fonts.conf
xvfb=$(pkg xvfb-run)/bin/xvfb-run
mkdir -p /tmp/root/empty && cp -r /testdata/games /tmp/root/games
printf "games_root = \"/tmp/root/empty\"\n" > /tmp/root/empty.toml
printf "games_root = \"/tmp/root/games\"\n" > /tmp/root/cfg.toml
failed=0
# shot <name> <config> <actions> [wrapper and extra args...]: one screenshot with a fresh data dir.
shot() {
    local name=$1 cfg=$2 actions=$3 wrap=$4; shift 4
    local data=/tmp/root/data-$name
    mkdir -p "$data" && ln -s /testdata/data/cores "$data/cores" && ln -s /testdata/data/system "$data/system"
    if $wrap "$out/bin/game37" --screenshot "/out/nix-$name.bmp" --config "$cfg" --data-dir "$data" \
        --actions "$actions" "$@" > "/out/nix-$name.log" 2>&1; then
        echo "ok   $name"
    else
        echo "FAIL $name"; tail -5 "/out/nix-$name.log"; failed=1
    fi
}
shot menu /tmp/root/empty.toml "nexttab" ""
shot n64 /tmp/root/cfg.toml "nexttab,right,right,right,confirm,confirm,confirm" "$xvfb -a" --frames 300
shot psp /tmp/root/cfg.toml "nexttab,left,left,left,confirm,confirm,confirm" "$xvfb -a" --frames 900
exit $failed
'
    rm "$out/src-nix.tar"
    for shot in menu n64 psp; do to_png "$out/out/nix-$shot.bmp"; done
    echo "RESULT_OK nix"
}

mkdir -p "$out/out"
main 2>&1 | tee "$out/out/nix.log"
