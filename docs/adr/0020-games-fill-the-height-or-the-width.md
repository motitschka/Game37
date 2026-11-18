# A Game fills the screen's height or its width

ADR 0018 holds a running Game at the 16:9 frame of 1920×1080, fitted into the window like the menus
were before it. On the owner's 16:10 laptop screen (1920×1200) that leaves bars above and below every
Game, even a 4:3 one that could fill the height, and on a screen wider than 16:9 a wide picture (DS
side by side) stops at the frame's sides. The owner wants every Game's picture as large as the screen
allows: it always fills the whole height or the whole width, with bars only on the other two sides.

**The picture is fitted to the window, not to the 16:9 frame.** It is the largest rect of the Game's
aspect ratio that fits the window's drawable size, centred, drawn in window pixels outside the 1920-wide
logical canvas. A 4:3 Game on 1920×1200 is 1600×1200; on a 16:9 screen every Game looks as it does now.
The mouse on a touch screen maps from the same rect. The Pause Menu and the messages over a Game keep
their 1920×1080 layout, centred in the window; their dimmed backdrop covers the whole window.

**DS and 3DS Large and small: the large screen fills the height, the small one takes the width that is
left**, even when that makes it small. The owner chose this over keeping the large screen three times the
small one. Game37 lays the two screens out itself:

- The large screen is as tall as the window. The small screen sits at the bottom right, beside it, as
  wide as the width left over. So the two together fill the window exactly, and the large screen is
  three to six times the small one's height, depending on the window's shape.
- Never less than three times: on a wide screen (21:9) the pair stays at three times, fills the height
  and has bars at the sides.
- Never more than six times: where the small screen would be smaller than a sixth of the large one, or
  would have no room at all, the pair stays at six times and fills the width instead, with bars above
  and below. Six is the largest proportion Azahar itself offers.

What that gives (large screen, small screen; bars):

| Screen    | DS                      | 3DS, top screen large                | 3DS, touch screen large            |
|-----------|-------------------------|--------------------------------------|------------------------------------|
| 1920×1080 | 1440×1080, 480×360      | 1694×1016, 226×169; 32 above, below  | 1440×1080, 480×288                 |
| 1920×1200 | 1600×1200, 320×240      | 1694×1016, 226×169; 92 above, below  | 1589×1192, 331×199; 4 above, below |
| 2560×1080 | 1440×1080, 480×360; 320 at the sides | 1800×1080, 480×360; 140 at the sides | 1440×1080, 600×360; 260 at the sides |

(A 3DS top screen at full height is 1800 wide on 16:9 and 2000 on 16:10, which leaves the small screen
120 pixels or nothing, so the six-times rule decides there.)

For Large and small, Game37 runs the Core in its side-by-side layout, both screens at the same size in
one frame, and draws each screen from its part of that frame into its own rect. The layout follows the
window as it is resized or goes fullscreen. R3 and Tab swap the screens by changing which part Game37
draws large; the Core's layout no longer changes. A click on the touch screen maps from wherever it is
drawn to its part of the frame. Side by side and One screen stay the Cores' own layouts, fitted to the
window like any other picture.

## Considered

- **The Cores' own Large and small layout, with its proportion set to fit the window:** melonDS DS
  offers only 2:1 and 3:1, so DS could not reach the five times a 16:10 screen needs. Azahar offers 1 to
  6 in steps of 0.25, but a resize or F11 mid-Game would mean changing a Core option and getting a new
  frame size.
- **Keeping the large screen three times the small one and fitting the pair** (1440×1080 and 480×360
  on 1920×1200, with 60-pixel bars above and below): the small screen stays larger, but the picture
  doesn't fill the height. The owner preferred filling the height.
- **No lower limit on the small screen:** a 3DS top screen filling a 16:9 height would leave the touch
  screen 120×90, and on 16:10 no room at all.

## Consequences

- FR-UI-6, FR-DS-1 and FR-NEW-2 now describe this. ADR 0018's "a running Game stays at 1080" now holds
  for the Pause Menu and messages over a Game, not for the Game's picture.
- On a 16:9 screen nothing changes for single-screen Systems, nor for DS's Large and small.
- In Large and small each screen is scaled from the Core's side-by-side frame. If the large screen looks
  softer than in the Core's own large-screen layout, the Core's render resolution has to go up.
- The touch cursor the Cores draw stays inside the touch screen's part of the frame, so it moves with
  that screen wherever Game37 draws it.
- Screenshots of a Game, and the smoke shots of a running Game, change on a screen taller or wider than
  16:9.
