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
shot m3-pause-restart-confirm "$root/cfg.toml" \
    "nexttab,right,right,confirm,confirm,confirm,wait:120,back,down,down,down,confirm"
# M4: OpenGL Cores (N64, PSP), DS layout, PS1 without a BIOS
shot m4-n64 "$root/cfg.toml" "nexttab,right,right,right,confirm,confirm,confirm" --frames 300
shot m4-ds "$root/cfg.toml" "nexttab,right,right,right,right,right,right,right,confirm,confirm,confirm" \
    --frames 300
shot m4-psp "$root/cfg.toml" "nexttab,left,left,left,confirm,confirm,confirm" --frames 900
shot m4-ps1-no-bios "$root/cfg.toml" "nexttab,left,left,left,left,left,confirm,confirm,confirm"
# M5: the Details Page, the Options Menu, the Favorite replace picker (six Favorites asked for), the
# Options Menu's slot picker for the SNES Game (m3-pause-saved filled its slot 1), renaming
shot m5-details "$root/cfg.toml" "nexttab,right,right,confirm,confirm"
shot m5-options "$root/cfg.toml" "down,down,options"
shot m5-favorite-replace "$root/cfg.toml" "down,down$(printf ',options,confirm,right%.0s' {1..5}),options,confirm,down"
shot m5-load-state "$root/cfg.toml" "nexttab,right,right,confirm,confirm,options,down,confirm"
# Choose artwork: offline, without --artwork, the picker says it needs a SteamGridDB API key
shot m5-choose-artwork "$root/cfg.toml" "down,down,options,down,confirm"
# Rename with the on-screen keyboard: erase the Display Title, then Shift, m, k and a space
shot m5-rename "$root/cfg.toml" \
    "down,down,options,down,down,confirm$(printf ',options%.0s' {1..40}),down,down,confirm,$(printf 'right,%.0s' {1..7})confirm,up,left,confirm,sort,up"
# M6: top bar entries
shot m6-search-open "$root/cfg.toml" "down,down,search"
shot m6-settings-open "$root/cfg.toml" "settings"
# First-run Setup step 2: GameCube, Switch and PlayStation 2 ticked, then every System
shot m6-setup-emulators "$root/missing.toml" \
    "down,confirm,down,right,confirm,up,right,right,right,right,confirm,settings" --missing-emulators switch,ps2
shot m6-setup-emulators-all "$root/missing.toml" "sort,settings" --missing-emulators wii,3ds,wiiu,switch,ps2,ps3,ps4
# First-run Setup step 3: NES, GameCube, Switch and PlayStation 2 ticked, on through step 2, then
# Create folders, under the Games Root that screenshot mode puts in the data dir
shot m6-setup-folders "$root/missing.toml" \
    "confirm,down,confirm,down,right,confirm,up,right,right,right,right,confirm,settings,settings,confirm" \
    --missing-emulators switch,ps2
# First-run Setup step 4: NES and PSP ticked, step 2 passed over, Create folders on step 3, then the
# Cores, which the smoke data dirs already have, so nothing downloads
shot m6-setup-cores "$root/missing.toml" "confirm,down,down,right,right,confirm,settings,confirm,settings"
# First-run Setup step 5: NES ticked, step 2 passed over, Create folders on step 3, on through the
# Cores on step 4 (already in the smoke data dir, so nothing downloads), to the SteamGridDB key
# field, typing into it on the keyboard without finishing (so no Config appears: the shots here
# share missing.toml).
shot m6-setup-key "$root/missing.toml" "confirm,settings,confirm,settings,settings,text:abcdef0123456789"
# First-run Setup's Finish: NES ticked, step 2 passed over, Create folders on step 3, on through
# the Cores on step 4 and the (here empty) key step, then Finish, which writes its own Config
# in the smoke dir and switches to Home. The shots above share missing.toml, so check no Finish
# wrote it.
shot m6-setup-finish "$root/setup-finish.toml" "confirm,settings,confirm,settings,settings,settings,wait:5"
[ -f "$root/setup-finish.toml" ] || echo "FAIL m6-setup-finish (Finish wrote no Config)"
[ ! -e "$root/missing.toml" ] || echo "FAIL $root/missing.toml exists, so the no-Config shots had a Config"

# M6: Controller test
shot m6-controller-test "$root/cfg.toml" "" --screen controller-test

# M6: Search. Typing (text:) makes the player's input kind the keyboard, and any other action the
# controller's: the controller shot types on the keyboard, then presses Up, which the field ignores,
# to show the on-screen keyboard. The empty shot clicks the field to show it as on the keyboard.
shot m6-search-typed "$root/cfg.toml" "search,text:test"
shot m6-search-results "$root/cfg.toml" "search,text:test,down,right"
shot m6-search-empty "$root/cfg.toml" "search,click:1000:68"
shot m6-search-controller "$root/cfg.toml" "search,text:test,up"

# M6: Settings: Library with the focus on Hidden games, the Hidden games list after hiding a Game
# from its Options Menu (three downs reach Hide game: an empty Load save state is skipped), the Reset
# confirmation, About
shot m6-settings-library "$root/cfg.toml" "settings,right,down,down"
shot m6-settings-hidden "$root/cfg.toml" "down,down,options,down,down,down,confirm,settings,right,down,down,confirm"
shot m6-settings-reset "$root/cfg.toml" "settings,right,down,down,down,confirm"
shot m6-settings-about "$root/cfg.toml" "settings$(printf ',down%.0s' {1..6})"
# M6: Settings BIOS and Cores, each with the focus on its first row (no download starts)
shot m6-settings-bios "$root/cfg.toml" "settings,down,down,right"
shot m6-settings-cores "$root/cfg.toml" "settings,down,down,down,right"

# M6: Settings Controls and Display: the Controls rows, Controls after switching the style to Xbox
# (its hint glyphs show Xbox labels), and the Display row
shot m6-settings-controls "$root/cfg.toml" "settings,down,down,down,down,right"
shot m6-settings-xbox "$root/cfg.toml" "settings,down,down,down,down,right,right"
shot m6-settings-display "$root/cfg.toml" "settings,down,down,down,down,down,right"

# M6: Settings Artwork: the key row with no key set (screenshot mode runs Artwork off, so it shows
# "Not set"), and the key entry modal with a typed key
shot m6-settings-artwork "$root/cfg.toml" "settings,down,right"
shot m6-settings-key "$root/cfg.toml" "settings,down,right,confirm,text:abcdef0123456789"
# M6: Settings Library: the Games Root entry, prefilled with the current Games Root (not saved)
shot m6-settings-games-root "$root/cfg.toml" "settings,right,confirm"

echo "screenshots in $out"
