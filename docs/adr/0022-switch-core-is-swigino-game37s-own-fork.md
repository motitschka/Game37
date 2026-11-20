# The Switch Core is swigino, Game37's own fork of suyu, downloaded from Game37's releases

ADR 0019 put Switch inside Game37 on suyu's libretro Core, built on the player's machine from suyu's
pinned source archive with `patches/suyu/`. That stopped working twice in its first week: suyu
re-tagged the pinned release, so its URL returned 404, and a server one of suyu's dependencies comes
from refused connections. The patch file was also about to grow by Save States (M9), which reach into
suyu's CPU, kernel, GPU and audio code, and every player paid for a long build with a C++ toolchain.

**The Switch Core is swigino, Game37's own fork of suyu, kept in Game37's repository as `swigino/`**
(M13). It is suyu's release source, imported once without suyu's history, plus Game37's changes as
ordinary commits. Its submodules stay links to their own projects. It follows suyu's releases: the
changes between two suyu commits are applied with a three-way merge (`swigino/SWIGINO.md`), and the
changes that suit suyu go back as pull requests. The Core is named swigino, file
`swigino_libretro.so`, so it isn't taken for suyu's own release Core. The folders it writes keep
suyu's name (`<save dir>/suyu/`, `<system dir>/suyu/`), so nobody's In-game Saves or keys move.

**Game37 downloads it like every other Core, from Game37's own GitHub releases.** GitHub Actions
builds it in the manylinux_2_28 container (glibc 2.28) with every library suyu can bundle built from
source and linked in, so it loads on any current distribution, and publishes it for a tag
`swigino-<version>`. Game37 pins the release by the zip's SHA-256 and installs nothing that doesn't
match. `scripts/build-swigino-core.sh` builds the same Core locally.

## Consequences

- ADR 0004 (Cores come from the libretro build server) gets one exception: swigino comes from
  Game37's own release, pinned by URL and checksum. NFR-5 names GitHub's releases, and the downloader
  follows exactly one kind of redirect, to GitHub's release asset host; it follows none for the build
  server.
- Setup's Cores step and Settings → Cores list swigino with the other Cores, and Play downloads it
  when it's missing. The "Core not built yet" message and Settings → BIOS's "Core not built" are gone;
  the BIOS page's Switch row shows the keys alone. Settings → Cores now holds the twelve Cores its
  layout was made for: the next Core needs a new layout for that page.
- Game37's repository now carries suyu's source (about 21 MB compressed), so a takedown notice against
  the Switch code would reach Game37's own repository. The owner accepts that.
- Moving to a newer suyu is now a Game37 change, with Game37's tests, smoke shots and manual checks,
  and a new release with a new pinned checksum.
- The published Core isn't yet byte-for-byte reproducible: a local build of the same source differs.
  The recipe is in the repository (`scripts/ci/build-swigino-portable.sh`).
- ADR 0019's other decisions stand: Switch is Embedded, Ryubing is gone, the player's keys are copied
  in before each start, and there are no Save States, Resume State or Restart until M9.
