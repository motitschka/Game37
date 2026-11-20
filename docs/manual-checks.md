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

- [ ] Each of the 9 Embedded Systems from v1 runs a Game at full speed, with audio that doesn't
      crackle; the other four are in section 7. (US-22)
- [ ] N64 shows a picture with a real Game (the test demo stays black). (US-22)
- [ ] PS1 runs with your BIOS in `~/Games/bios/`. (US-22)
- [ ] Two pads control players 1 and 2. (US-26)
- [ ] DS: touch with a real mouse and with the right stick; R3 swaps the screens and feels right. (US-29)

## 3. External emulators

- [ ] Each of the 3 External Systems (Wii U, PS3, PS4) starts its emulator with a real Game (RPCS3 needs firmware,
      shadPS4 its modules). (US-30)
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
- [ ] With a Switch controller in a PS2 Game, A confirms (✕) and B goes back (○); in an SNES Game, A is
      still the right button. An Xbox or PlayStation controller in the PS2 Game: the bottom button
      confirms.
- [ ] Startup window mode across restarts; window resizing and F11. (US-33)

## 6. Text input and files

- [ ] Typing a rename with a real keyboard, including dead keys and an IME. (US-3)
- [ ] Typing in Search with a real keyboard, and Ctrl+V paste. (US-21)
- [ ] "Show file in file manager" opens Nautilus with the file selected. (US-16)
- [ ] Moving the Games Root in Settings while Game37 runs, and the watcher following it.

## 7. PS2, 3DS, GameCube and Wii inside Game37 (M8, 0.2.0-beta.1)

- [ ] A real Game of each of PS2, 3DS, GameCube and Wii plays at full speed, with sound that doesn't
      crackle; PS2 with your BIOS in `~/Games/bios/pcsx2/bios/`.
- [ ] DS and 3DS: touch with the mouse, and with the right-stick cursor tapping with L3; on DS, L3
      never turns the microphone on.
- [ ] DS: holding R2 blows into the microphone in a Game that listens for it (e.g. to blow out a candle), and never taps.
- [ ] DS and 3DS: R3 on a pad and Tab on the keyboard swap the screens in each layout (Large and small,
      Side by side, One screen), chosen in Settings → Display.
- [ ] Wii: a real Wii Game plays with the Wii Remote and Nunchuk, buttons mapped as in the README's
      Playing section.
- [ ] PS2: loading a Save State in a real Game brings back the saved moment (on a still screen, a
      button press brings the picture back: a known LRPS2 bug).
- [ ] PS2: Delete save data on a Game deletes its memory card in `~/Games/saves/ps2/`, and the
      next start makes a new formatted one.

## 8. Switch 2, PS5 and the Wii Classic Controller (0.2.0-beta.2)

The Switch controller in PlayStation Games is checked in section 5.

- [ ] An existing install (Setup already done) shows Switch 2 and PS5 on the Carousel; after making
      `~/Games/switch2/` and `~/Games/ps5/` by hand, a Switch 2 `.xci` and a PS5 game folder show up
      on their Console Pages, and Play on either says "Not playable yet" and starts nothing.
- [ ] Wii: Mario Kart Wii with its Controller set to Classic Controller in its Options Menu: the Game
      shows the Classic Controller's controls; A accelerates, B brakes, R drifts, L uses an item, X
      looks behind, the left stick steers, and + pauses. Set back to Wii Remote and Nunchuk, the next
      start plays with those again.
- [ ] Wii: on a Game with a Resume State made with the Wii Remote and Nunchuk, switch its Controller to
      Classic Controller and press Play: note what the Game does (it resumes with the Nunchuk state);
      then Start from power-on: the Classic Controller works from the title screen.

## 9. Menus on a 16:10 screen (0.2.0-beta.3)

- [ ] On a 1920×1200 screen, fullscreen: Home, the Carousel, a Console Page, Settings and First-run
      Setup fill the screen with the hint bar at the very bottom and no black bars; a Game started from
      there fills the screen's height or width (section 12).
- [ ] Switching F11 between windowed and fullscreen, and resizing the window, on a scrolled Console
      Page and in Settings → Hidden games: the focused item stays in view and nothing is cut off.

## 10. Switch inside Game37 (ADR 0019, ADR 0022)

- [ ] On a fresh data folder, Setup's Cores step lists swigino for Switch and downloads it; Settings →
      BIOS shows the Switch keys as optional until they're there.
- [ ] Play on a Switch Game without the Core downloads it first, with the progress screen.
- [ ] `scripts/build-swigino-core.sh` builds and installs the same Core on a fresh checkout.
- [ ] The published Core (the `swigino-…` release's zip) loads in Game37 on another current
      distribution than the owner's, e.g. Fedora or Ubuntu.
- [ ] With your own `prod.keys` dumped from your own Switch in `~/Games/bios/switch/`, one of your own
      Games (`.nsp` or `.xci`) starts, plays with the controller, and makes sound; note the frame rate.
- [ ] Without the keys, Play on an `.nsp` shows "Switch keys missing" naming that folder.
- [ ] The Pause Menu over a Switch Game shows Save state, Load state and Restart game greyed with the
      note, and the Game (and its sound) stops while the menu is open, then carries on with Resume.
- [ ] An In-game Save made in a Switch Game is there after quitting and starting it again, and lives
      under `~/Games/saves/switch/suyu/nand/`. Nothing appears in `~/.local/share/suyu`.

## 11. Home and grid design (D1)

- [ ] Home on your 1920×1200 screen, and in a 1920×1080 window: exactly two whole Shelves show; the
      covers are bigger on the taller screen; nothing is cut off at the bottom.
- [ ] Down and up with the controller, the keyboard and the mouse wheel move one whole Shelf with a
      short snap; with Settings → Display → Motion set to Reduced they jump. A slow two-finger touchpad
      swipe moves one Shelf at a time, not straight to the last one.
- [ ] Moving the mouse over the Shelves while Home snaps never leaves the selection on a Shelf you can't
      see.
- [ ] Settings → Home: move Most played to the top with ← and switch Continue playing off with A, then
      back to Home: Most played is first and Continue playing is gone. The last Shelf left on can't be
      switched off. After a restart Home is the same.
- [ ] A new Games Root with a few Games and nothing played: after First-run Setup, Home opens on
      Recently added with the Games showing.
- [ ] Settings → Display → Games per row at 6, 8 and 10: a Console Page, "See all" and Search show that
      many covers per row, with equal gaps left and right; the selected Game stays selected.
- [ ] The Sort button on a Console Page opens the list of seven orders; picking one sorts and jumps to
      the first Game; another System's Console Page uses the same order, also after a restart.
- [ ] Without a SteamGridDB key, Newest release says release dates need a key.
- [ ] Keyboard: Page Up/Down, Home and End in a big grid and in Search; Shift+Tab goes back a view.
      Mouse: ‹ and › in Settings → Display step back and forward with one click; a sideways touchpad
      swipe moves the Shelf under the pointer and the Carousel one System per notch.
- [ ] "See all" on Recently added shows every Game you have, newest first; Sort changes it there, and a
      Console Page's order is unchanged afterwards. "See all" on Favorites shows only the Favorites.
- [ ] With your SteamGridDB key: sort a Console Page by Newest release and wait: Games fill in by date,
      the selected Game stays selected, and the grid doesn't jump more than once a second. Sort on to
      A–Z, wait a few seconds, then back to Newest release: only the dates fetched before you left
      it are filled in, and the rest start arriving again from there.
- [ ] The Details Page of a Game with a known date shows RELEASED with its year; an embedded Game with
      Save States and a date shows six boxes, all inside the screen.
- [ ] Settings → Display → Show Playtime: Grids shows a played Game's Playtime only under the selected
      cover on a Console Page; Grids and Home also on Home; Off shows it nowhere.
- [ ] Each of the 19 Carousel cards and its Console Page header shows that System's logo, readable on
      its background.

## 12. Games fill the screen (ADR 0020)

- [ ] On a 1920×1200 screen, fullscreen: a 4:3 Game (e.g. SNES, PS1) fills the whole height, with bars
      only at the sides; its Pause Menu fills the screen, the side panel from top to bottom and the
      hint bar at the very bottom, over a picture dimmed to every edge.
- [ ] DS in Large and small on that screen: the large screen fills the height and the small one sits at
      the bottom right, with no bars; R3 and Tab swap them, and the mouse and the right-stick cursor
      touch the touch screen whether it is drawn large or small.
- [ ] 3DS in Large and small: the top screen large with a small touch screen at the bottom right and
      thin bars above and below; R3 and Tab swap them, and touch works on the touch screen either way.
- [ ] Resizing the window and F11 during a DS or 3DS Game: the two screens follow at once, and nothing
      overlaps or gets cut off.
- [ ] Side by side and One screen look and swap as before, filling the width or the height.
- [ ] An N64 Game that draws a black border of its own (e.g. Banjo-Kazooie) on that screen: a few
      seconds after its picture appears, the picture grows once to fill the whole height, not
      stretched, and stays that size through fades and black loading screens (ADR 0021).
