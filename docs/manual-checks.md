# Manual checks for 1.0

Checks only the owner can do: in a real window, with real pads, real Games and the real SteamGridDB key.
Headless tests and smoke shots already cover everything else. 1.0 ships when every box here is ticked or
has an issue that was fixed. A failure becomes a GitHub issue; note its number next to the box.

The story in brackets stays open on GitHub until its checks pass.

## 1. Fresh account (do this first, it exercises the most)

- [ ] A user account with no Config goes from First-run Setup to playing a homebrew Game, using only
      the in-app steps and the README: real Flatpak detection, the copyable `flatpak install` line, real
      Core downloads. (US-36)

## 2. Embedded play

- [ ] Each of the 9 Embedded Systems runs a Game at full speed, with audio that doesn't crackle. (US-22)
- [ ] N64 shows a picture with a real Game (the test demo stays black). (US-22)
- [ ] PS1 runs with your BIOS in `~/Games/bios/`. (US-22)
- [ ] Two pads control players 1 and 2. (US-26)
- [ ] DS: touch with a real mouse and with the right stick; R3 swaps the screens and feels right. (US-29)

## 3. External emulators

- [ ] Each of the 8 External Systems starts its emulator with a real Game (RPCS3 needs firmware,
      Ryubing `prod.keys`, shadPS4 its modules). (US-30)
- [ ] When the emulator exits, GNOME brings Game37 back to the front and Playtime goes up. (US-30)
- [ ] Removing an emulator's Flatpak shows the badge and notice; reinstalling it and pressing Check
      again clears them. (US-39)

## 4. Artwork

- [ ] With the key, Games show real covers and hero art, and a restart doesn't download them again. (US-17)
- [ ] Entering the key in Settings makes covers appear without a restart. (US-20)
- [ ] A large library scrolls smoothly and requests stay paced; a chosen cover sticks after a restart.
      (US-17)

## 5. Controls and window

- [ ] Holding a direction in the Carousel speeds up and wraps around. (US-38)
- [ ] The Controller test with real pads, including hold to exit. (US-34)
- [ ] Menu button style with a real pad in each style. (US-32)
- [ ] Startup window mode across restarts; window resizing and F11. (US-33)

## 6. Text input and files

- [ ] Typing a rename with a real keyboard, including dead keys and an IME. (US-3)
- [ ] Typing in Search with a real keyboard, and Ctrl+V paste. (US-21)
- [ ] "Show file in file manager" opens Nautilus with the file selected. (US-16)
- [ ] Moving the Games Root in Settings while Game37 runs, and the watcher following it.
