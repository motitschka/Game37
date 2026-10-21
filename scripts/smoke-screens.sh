#!/usr/bin/env bash
# Renders one headless screenshot per screen and flow built so far, for a visual regression check.
#
#   scripts/smoke-screens.sh [out-dir]      (default: target/smoke)
#
# Uses the test Cores and test ROMs in .testdata/ (see CLAUDE.md) plus empty dummy files for
# External Systems. Never touches ~/Games, ~/.config/game37 or ~/.local/share/game37. Each shot
# gets a fresh data dir, because the Carousel remembers its position in the library database.
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-$repo/target/smoke}
testdata=$repo/.testdata
[ -d "$testdata/data/cores" ] || { echo "missing $testdata (Cores and test ROMs)" >&2; exit 1; }

rm -rf "$out"
mkdir -p "$out/root"
root=$out/root
cp -r "$testdata/games" "$root/games"
for f in "gc/Metroid Prime (USA).iso" "wiiu/Xenoblade Chronicles X (Europe).wua" \
    "ps2/Persona 4 (USA).iso" "switch/Hello World.nro"; do
    mkdir -p "$root/games/$(dirname "$f")"
    : > "$root/games/$f"
done
mkdir -p "$root/empty"
printf 'games_root = "%s/games"\n' "$root" > "$root/cfg.toml"
printf 'games_root = "%s/empty"\n' "$root" > "$root/empty.toml"

(cd "$repo" && cargo build -q)
bin=$repo/target/debug/game37

# shot <name> <config> <actions> [extra args...]
shot() {
    local name=$1 cfg=$2 actions=$3
    shift 3
    local data=$root/data-$name
    mkdir -p "$data"
    ln -s "$testdata/data/cores" "$data/cores"
    ln -s "$testdata/data/system" "$data/system"
    if "$bin" --screenshot "$out/$name.bmp" --config "$cfg" --data-dir "$data" --actions "$actions" "$@" \
        >"$out/$name.log" 2>&1; then
        magick "$out/$name.bmp" -resize 50% "$out/$name.png" && rm "$out/$name.bmp"
        echo "ok   $name"
    else
        echo "FAIL $name (see $out/$name.log)"
    fi
}

# M1: Home, Carousel, Console Pages, See all, no Config
shot m1-no-config "$root/missing.toml" ""
shot m1-home-empty "$root/empty.toml" ""
shot m1-home "$root/cfg.toml" ""
shot m1-see-all "$root/cfg.toml" "down,down,$(printf 'right,%.0s' {1..25})confirm"
shot m1-carousel "$root/cfg.toml" "nexttab,right,right"
shot m1-console-page "$root/cfg.toml" "nexttab,right,right,confirm,sort"
shot m1-console-empty "$root/cfg.toml" "nexttab,left,left,confirm"
# M2: missing-emulator badge and notice, launching, messages
shot m2-carousel-missing "$root/cfg.toml" "nexttab,left,left,left,left,left,left,left" --missing-emulators wiiu
shot m2-notice "$root/cfg.toml" "nexttab,left,left,left,left,left,left,left,confirm" --missing-emulators wiiu
shot m5-details-missing "$root/cfg.toml" \
    "nexttab,left,left,left,left,left,left,left,confirm,down,confirm,left" --missing-emulators wiiu
shot m2-playing "$root/cfg.toml" "nexttab,right,right,right,right,right,right,confirm,confirm,confirm"
# M3: each Embedded System running, Pause Menu with a saved slot
shot m3-nes "$root/cfg.toml" "nexttab,confirm,confirm,confirm" --frames 180
shot m3-gb "$root/cfg.toml" "nexttab,right,confirm,confirm,confirm" --frames 600
shot m3-snes "$root/cfg.toml" "nexttab,right,right,confirm,confirm,confirm" --frames 180
shot m3-gbc "$root/cfg.toml" "nexttab,right,right,right,right,confirm,confirm,confirm" --frames 600
shot m3-gba "$root/cfg.toml" "nexttab,right,right,right,right,right,confirm,confirm,confirm" --frames 240
shot m3-pause-saved "$root/cfg.toml" \
    "nexttab,right,right,confirm,confirm,confirm,wait:120,back,down,confirm,confirm,confirm"
# M4: OpenGL Cores (N64, PSP), DS layout, PS1 without a BIOS
shot m4-n64 "$root/cfg.toml" "nexttab,right,right,right,confirm,confirm,confirm" --frames 300
shot m4-ds "$root/cfg.toml" "nexttab,right,right,right,right,right,right,right,confirm,confirm,confirm" \
    --frames 300
shot m4-psp "$root/cfg.toml" "nexttab,left,left,left,confirm,confirm,confirm" --frames 900
shot m4-ps1-no-bios "$root/cfg.toml" "nexttab,left,left,left,left,left,confirm,confirm,confirm"
# M5: the Details Page
shot m5-details "$root/cfg.toml" "nexttab,right,right,confirm,confirm"

echo "screenshots in $out"
