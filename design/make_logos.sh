#!/usr/bin/env bash
# make_logos.sh <svg-dir> <png-dir>: renders each System logo into an 800x340 box (twice the largest
# size the menus show it at) and turns its black and near-black parts white, so it reads on the dark
# menus and on its System's card. Coloured parts keep their official colours, except the dark or grey
# ones listed in `extra` below, which would not read on the card either.
set -euo pipefail
src=$1 out=$2
mkdir -p "$out"

# The System's own rule on top of black-to-white, as magick arguments: its dark navy or grey parts
# turned white too (with a fuzz that keeps its accent colours), and accents in its card's own colour, or
# all its colours, lightened.
extra() {
    case $1 in
        n64) echo -fuzz 20% -fill white -opaque '#0124A6' ;; # "NINTENDO"; the red "64" stays
        gbc) echo -fuzz 8% -fill white -opaque '#2E3192' ;;  # "GAME BOY"; the "COLOR" letters stay
        wii) echo -fuzz 12% -fill white -opaque '#A1A0A4' ;;
        # "Wii" white; the blue "U", the card's own colour, a lighter blue
        wiiu) echo -fuzz 12% -fill white -opaque '#8C8C8C' -fuzz 15% -fill '#7FD8F8' -opaque '#0096C8' ;;
        # Grey parts white; the red "3", the card's own colour, a lighter red
        3ds) echo -fuzz 12% -fill white -opaque '#8C8C8C' -fuzz 15% -fill '#FF9A94' -opaque '#CE181E' ;;
        nds) echo -fuzz 12% -fill white -opaque '#929497' ;;
        ps2) echo -level 0%,62% ;; # its blue gradient, lighter so it reads on the blue card
    esac
}

for f in "$src"/*.svg; do
    id=$(basename "$f" .svg)
    rsvg-convert --keep-aspect-ratio -w 800 -h 340 "$f" -o "$out/$id.raw.png"
    # shellcheck disable=SC2046 # word-split on purpose: extra prints magick arguments
    magick "$out/$id.raw.png" -channel RGB -fuzz 24% -fill white -opaque black $(extra "$id") +channel \
        -strip -define png:compression-level=9 "$out/$id.png"
    rm "$out/$id.raw.png"
done
