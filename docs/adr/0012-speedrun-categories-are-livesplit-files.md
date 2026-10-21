# Speedrun Categories are LiveSplit files in the Games Root

Each Category is stored as a LiveSplit `.lss` file under `<Games Root>/splits/<system>/`, not in the library database. Following ADR 0005, progress lives in the Games Root, so one copy backs it up. Import and export become plain file copies: runners already keep their splits as `.lss` files, and LiveSplit and LiveSplit One open the files Game37 writes. The cost is reading and rewriting LiveSplit's XML while keeping the parts Game37 doesn't use, such as game time and autosplitter settings.
