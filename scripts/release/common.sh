# Sourced by the release scripts: paths, the version from Cargo.toml, each distro's Docker image and
# the README's install commands. Everything the scripts write goes under $out (target/release-check/).

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repo/Cargo.toml" | head -1)
out=$repo/target/release-check

image_of() {
    case $1 in
    arch) echo archlinux:latest ;;
    ubuntu) echo ubuntu:26.04 ;;
    fedora) echo fedora:44 ;;
    debian) echo debian:13 ;;
    *) echo "unknown distro '$1'" >&2 && return 1 ;;
    esac
}

# readme_install <heading> <distro>: the README's install command for <distro> in the section under
# <heading>, made non-interactive for a root shell in a container. Taking it from the README checks
# that the command people copy works as written.
readme_install() {
    local manager line
    case $2 in
    arch) manager=pacman ;;
    fedora) manager=dnf ;;
    ubuntu | debian) manager=apt ;;
    esac
    line=$(awk -v h="$1" '$0 == h { on = 1; next } on && /^#+ / { exit } on' "$repo/README.md" |
        grep -m1 "^ *sudo $manager ") || {
        echo "no 'sudo $manager' line under '$1' in README.md" >&2
        return 1
    }
    line=$(sed 's/^ *sudo //' <<<"$line")
    case $manager in
    pacman) echo "pacman -Syu --noconfirm ${line#pacman -S }" ;;
    dnf) echo "dnf install -y ${line#dnf install }" ;;
    apt) echo "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y ${line#apt install }" ;;
    esac
}

# source_tar <file>: the files git tracks, as in the working tree, without target/ or .testdata/.
source_tar() {
    git -C "$repo" ls-files -z | tar -C "$repo" --null -T - -cf "$1"
}

# to_png <bmp>: a half-size PNG next to it, for looking at; drops the BMP.
to_png() {
    magick "$1" -resize 50% "${1%.bmp}.png" && rm "$1"
}
