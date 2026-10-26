#!/usr/bin/env bash
# Checks a prebuilt tarball the way a user gets it: in a fresh container of its distro with only the
# packages the README's "Prebuilt binary" section installs, it checks the .sha256, unpacks the
# tarball, checks that every library is found and starts Game37 headless once.
#
#   scripts/release/runtime-check.sh <arch|ubuntu>
#
# Needs the tarball from `build.sh <distro>` in target/release-check/pkg/. Writes
# target/release-check/out/<distro>-runtime.log and <distro>-runtime.png; the log ends in
# "RESULT_OK <distro>-runtime" when everything passed.
set -euo pipefail
source "$(dirname "$0")/common.sh"

distro=${1:?usage: runtime-check.sh <arch|ubuntu>}
image=$(image_of "$distro")
install=$(readme_install '### Prebuilt binary (Arch Linux and Ubuntu)' "$distro")
name=game37-$version-x86_64-$distro.tar.gz
[ -f "$out/pkg/$name" ] || { echo "no $out/pkg/$name: run build.sh $distro first" >&2 && exit 1; }

main() {
    echo "$name in $image"
    docker run --rm \
        -v "$out/pkg:/pkg:ro" \
        -v "$out/out:/out" \
        -e HOST_ID="$(id -u):$(id -g)" -e DISTRO="$distro" -e INSTALL="$install" \
        -e NAME="$name" -e VERSION="$version" \
        "$image" bash -c '
set -euo pipefail
trap "chown -R \$HOST_ID /out" EXIT
echo "+ $INSTALL"
eval "$INSTALL" >/tmp/install.log 2>&1 || { tail -30 /tmp/install.log; exit 1; }
grep "^PRETTY_NAME" /etc/os-release
mkdir /tmp/rt && cp "/pkg/$NAME" "/pkg/$NAME.sha256" /tmp/rt && cd /tmp/rt
sha256sum -c "$NAME.sha256"
tar -xzf "$NAME"
ldd "game37-$VERSION/game37" | tee /tmp/ldd | grep -i sdl
if grep "not found" /tmp/ldd; then exit 1; fi
mkdir -p /tmp/games /tmp/data && printf "games_root = \"/tmp/games\"\n" > /tmp/cfg.toml
"game37-$VERSION/game37" --screenshot "/out/$DISTRO-runtime.bmp" --config /tmp/cfg.toml \
    --data-dir /tmp/data --actions ""
'
    to_png "$out/out/$distro-runtime.bmp"
    echo "RESULT_OK $distro-runtime"
}

mkdir -p "$out/out"
main 2>&1 | tee "$out/out/$distro-runtime.log"
