# Switch 2 and PS5 are Upcoming Systems

Switch 2 and PlayStation 5 join the System list before anything can play them: in November 2026 no
emulator runs either. The owner wants their folders and Games in Game37 already, so a third kind of
System sits beside Embedded and External: the **Upcoming System**. Game37 makes its folder under the
Games Root, scans it and lists its Games on the Carousel, its Console Page and in Search like any other
System's, and Play on one of its Games shows "Not playable yet" with the reason instead of starting
anything.

Both keep System Order: Switch 2 (2025) after Switch, PS5 (2020) after PS4. Switch 2 takes `.nsp` and
`.xci` files; a PS5 Game is a folder with `eboot.bin` and `sce_sys/`, like a PS4 one.

## Consequences

- There are 19 Systems. Setup's Systems step lays its tiles out in seven columns so they still fit in
  three rows, and Settings → BIOS rows are 36 px apart instead of 41 so the table still fits.
- An Upcoming System has no Core, no Standalone Emulator, no BIOS and no Save States: Setup asks to
  install or download nothing for it, Settings → BIOS shows "Not playable yet", and its Games' Options
  Menu has no Save State entries.
- When an emulator for one of them appears, the System becomes Embedded or External in
  `systems::SYSTEMS` and gets its Core or emulator like the others; its folder and Games stay as they
  are.
