# Design review: motion and screens (2026-11-11)

A pass over every screen and how it moves, from the smoke shots (`scripts/smoke-screens.sh`) and the
code, measured against `design/generate_mockups.py`. Branch `design-motion`. Ticked items are done
on that branch; the rest are proposals, in order of how much they'd improve the feel.

## How Game37 moved before this pass

Only the Carousel animated (its cards ease, τ = 38 ms). Everything else jumped in a single frame:
Shelves and grids scrolled to the focus at once, the focused cover grew by 8 px at once, screens cut
from one to the next, and the Options panel and Pause Menu appeared fully formed. With a controller
that's the bit that feels cheapest: each press teleports the picture, so the eye has to find the
focus again.

## Motion

- [x] **One motion system.** `ui::anim::Eased`: a value that follows its target over real time,
  frame-rate independent (an exponential approach), and lands at once when `Ctx::animate` is off,
  so screenshot mode and tests still draw the settled state. Three time constants cover everything:
  scroll 45 ms, focus 35 ms, fade 50 ms. That's quick enough never to slow a held stick down.
- [x] **Shelves and grids glide** to the focus instead of jumping (Home, Console Pages, See all,
  Search results). Mouse hits follow what's drawn, since they're built while drawing.
- [x] **The focused cover grows in** and its ring fades in after each move, rather than popping.
- [x] **Screens fade in and rise** 16 px when opened, closed back to or switched to, under a top bar
  that stays still, so the frame of the app reads as fixed and only the content changes.
- [x] **The Options panel slides in** from the right and the **Pause Menu** from the left, each
  fading in together with its dimmed backdrop. `Gfx` got a drawing `Layer` (offset and opacity)
  for this.
- [ ] **List highlights glide.** The Options and Pause Menu rows, Settings sections and Setup rows
  still move their highlight bar in one frame. An eased bar between rows would match the covers.
- [ ] **Artwork fades in** when it arrives, instead of swapping from placeholder to picture in one
  frame (most visible on a first scroll through a big library).
- [ ] **Reduced motion.** A Settings → Display switch that turns all of this off, for players who
  find motion uncomfortable; it only has to set what `Ctx::animate` already controls.

## Screens

- [x] **Home, an empty Shelf with focus** drew the full white focus ring with its violet glow around
  a box that only holds a hint: the loudest thing on the screen for something you can't act on. It
  now gets a quiet accent outline in place of its dashed border.
- [ ] **Home, the paging arrows** sit on top of the last visible cover. Put them in the side margin,
  or fade the Shelf's edge under them.
- [ ] **Details** puts everything in the top half and leaves the bottom ~40% empty. Give the cover
  and the actions more of the height. (Its file path line is fine: it's shortened with `~`, so a
  real Games Root reads `~/Games/snes/…`; it only looks long in the smoke shots, whose Games Root
  is under `/tmp`. The same goes for Settings → BIOS.)
- [ ] **Small text at TV distance.** Hint bar labels and table headers (e.g. "PLAYTIME", 13–15 px)
  are small for a couch at 1080p. Raise the smallest sizes to about 17 px.

## Checked and fine

The Carousel (tint blends while sliding, readable captions), the Options menu layout, Search, the
missing-emulator notice, and Setup's steps all match the mockups and read well.
