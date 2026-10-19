# Cores are downloaded from the libretro build server at runtime

Game37 fetches each Core from buildbot.libretro.com on first use and stores it in `~/.local/share/game37/cores/`. Committing Cores would bloat the public repo and bring GPL source-offer obligations. Building from source makes setup slow (PPSSPP and N64 especially). Reusing RetroArch's Flatpak would add a dependency and require reaching into another app's sandbox.
