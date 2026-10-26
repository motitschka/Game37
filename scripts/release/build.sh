#!/usr/bin/env bash
# Builds Game37 in a clean container of one distro, with the packages the README's "Build from
# source" step 1 installs, and starts the build headless once.
#
#   scripts/release/build.sh <arch|ubuntu|fedora|debian>
#
# The container uses the host's rustup stable toolchain and a `cargo vendor` copy of the crates, so it
# builds offline with `--locked`. Writes target/release-check/out/<distro>.log and <distro>.png, and
# for arch and ubuntu, which ship prebuilt binaries, the tarball and its .sha256 in
# target/release-check/pkg/. The log ends in "RESULT_OK <distro>" when everything passed.
set -euo pipefail
source "$(dirname "$0")/common.sh"

distro=${1:?usage: build.sh <arch|ubuntu|fedora|debian>}
image=$(image_of "$distro")
install=$(readme_install '#### 1. Install the build tools and libraries' "$distro")
toolchain=$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu

main() {
    echo "game37 $version from $(git -C "$repo" describe --always --dirty), $distro in $image"
    cargo vendor --quiet --locked --manifest-path "$repo/Cargo.toml" "$out/vendor" >/dev/null
    source_tar "$out/src-$distro.tar"
    docker run --rm \
        -v "$out/src-$distro.tar:/src.tar:ro" \
        -v "$toolchain:/rust:ro" \
        -v "$out/vendor:/vendor:ro" \
        -v "$out/out:/out" \
        -e PATH=/rust/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
        -e HOST_ID="$(id -u):$(id -g)" -e DISTRO="$distro" -e INSTALL="$install" \
        "$image" bash -c '
set -euo pipefail
trap "chown -R \$HOST_ID /out" EXIT
echo "+ $INSTALL"
eval "$INSTALL" >/tmp/install.log 2>&1 || { tail -30 /tmp/install.log; exit 1; }
grep "^PRETTY_NAME" /etc/os-release
echo "SDL3 $(pkg-config --modversion sdl3), SDL3_ttf $(pkg-config --modversion sdl3-ttf)"
printf "[source.crates-io]\nreplace-with = \"vendored\"\n[source.vendored]\ndirectory = \"/vendor\"\n" \
    > /tmp/cargo-config.toml
mkdir -p /root/.cargo /build && cp /tmp/cargo-config.toml /root/.cargo/config.toml
tar -C /build -xf /src.tar && cd /build
cargo build --release --locked --offline 2>&1 | tail -3
ldd target/release/game37 | tee /tmp/ldd | grep -i sdl
if grep "not found" /tmp/ldd; then exit 1; fi
mkdir -p /tmp/games /tmp/data && printf "games_root = \"/tmp/games\"\n" > /tmp/cfg.toml
target/release/game37 --screenshot "/out/$DISTRO.bmp" --config /tmp/cfg.toml --data-dir /tmp/data --actions ""
cp target/release/game37 "/out/game37-$DISTRO"
'
    rm "$out/src-$distro.tar"
    to_png "$out/out/$distro.bmp"
    case $distro in arch | ubuntu) package ;; esac
    echo "RESULT_OK $distro"
}

# The tarball: one folder game37-<version>/ with the binary, README.md and LICENSE, owned root/root.
package() {
    local name=game37-$version-x86_64-$distro.tar.gz stage=$out/stage-$distro
    grep -qaF "$version" "$out/out/game37-$distro" || { echo "the binary doesn't say $version" >&2 && exit 1; }
    rm -rf "$stage" && mkdir -p "$stage/game37-$version"
    install -m755 "$out/out/game37-$distro" "$stage/game37-$version/game37"
    install -m644 "$repo/README.md" "$repo/LICENSE" "$stage/game37-$version/"
    tar -C "$stage" --owner=root:0 --group=root:0 --no-recursion -czf "$out/pkg/$name" \
        "game37-$version" "game37-$version/game37" "game37-$version/README.md" "game37-$version/LICENSE"
    rm -r "$stage"
    (cd "$out/pkg" && sha256sum "$name" | tee "$name.sha256")
}

mkdir -p "$out/out" "$out/pkg"
main 2>&1 | tee "$out/out/$distro.log"
