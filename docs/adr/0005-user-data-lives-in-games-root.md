# BIOS, saves and states live in the Games Root, not XDG directories

BIOS files, In-game Saves, Save States and Resume States live under `~/Games/` (`bios/`, `saves/<system>/`, `states/<system>/`) instead of `~/.local/share/game37/`. This deliberately breaks Linux convention, so that one visible folder holds both the games and all progress, and backing it up is a single copy. Cores, cached Artwork and the library database are replaceable, so they do follow XDG.
