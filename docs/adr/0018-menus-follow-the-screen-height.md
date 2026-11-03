# The menus follow the screen's height; Games stay 16:9

Game37 drew everything on a fixed logical 1920×1080 canvas, letterboxed into the window. On the owner's
16:10 laptop screen (1920×1200) that left black bars above and below every menu. The owner wants the
menus to fill the screen and the Games to keep their own aspect ratios.

The logical canvas stays 1920 wide, and its height now follows the window's shape: 1920 × height ÷
width in pixels, at least 1080 and at most 1440 (4:3), so a 16:10 screen gets 1920×1200. `Gfx::fit`
sets it before the mouse is mapped and before every draw, and stores it in a per-thread value that
screens read with `ui::h()`, in `handle` as well as in `draw`; input is handled against the height of
the frame last drawn. Whenever a Game is the base screen it is held at 1080, so a
Game's picture, the Pause Menu and any message over a Game are drawn exactly as before.

Screens lay out against `ui::h()`: what hangs from the top stays where it is, and what sits at the
bottom keeps its distance from the bottom (the hint bar, Setup's buttons, the ends of lists and grids,
fades, full-height panels); dialogs and the Carousel stay centred. At a height of 1080 every screen
draws exactly as it did: when this was made, every 1080 smoke shot was compared pixel for pixel with
the 0.2.0-beta.2 ones, and only the clock and on-screen temp paths differed. Nothing is stretched.

## Considered

- **Stretching the 1920×1080 picture to fill the screen:** no work, but everything 11% too tall,
  circles as ovals.
- **Cropping the sides (overscan):** cuts off the logo, the clock and the edges of every screen.
- **Passing the height through `Ctx`:** `handle` has `Ctx` but helpers called from scroll and reload
  code have neither `Ctx` nor `Gfx`, so the height would have had to be threaded through about twenty
  files; the per-thread value keeps every test at 1080 without changes.

## Consequences

- Lists, grids and Shelves show more on a taller screen; a resize or F11 refits their scroll.
- A wider-than-16:9 screen (e.g. 21:9) still shows bars at the sides, and one taller than 4:3 bars above
  and below.
- Screenshot mode takes `--size WxH`, and the smoke script renders the main menus at 1920×1200 as well.
- The mockups in `design/` stay 1920×1080: the 1080 layout is the design, and taller screens add room at
  the bottom of lists.
