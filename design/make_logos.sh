#!/usr/bin/env bash
# make_logos.sh <svg-dir> <png-dir>: renders each System logo into an 800x340 box (twice the largest
# size the menus show it at) and turns its black and near-black parts white, so it reads on the dark
# menus. Coloured parts keep their official colours.
set -euo pipefail
src=$1 out=$2
mkdir -p "$out"
for f in "$src"/*.svg; do
    id=$(basename "$f" .svg)
    rsvg-convert --keep-aspect-ratio -w 800 -h 340 "$f" -o "$out/$id.raw.png"
    magick "$out/$id.raw.png" -channel RGB -fuzz 24% -fill white -opaque black +channel \
        -strip -define png:compression-level=9 "$out/$id.png"
    rm "$out/$id.raw.png"
done
