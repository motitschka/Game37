# N64 Games lose the black border they draw themselves

Since ADR 0020 a Game's picture fills the screen's height or width. Banjo-Kazooie still showed bars on
the owner's 16:10 screen: Game37 scaled its 640×480 frame to 1600×1200, but the Game draws its picture
inside that frame with a black border of its own, about 40 pixels left, 30 right, 23 at the top and 30 at
the bottom. Many N64 Games do this, each by a different amount, and some not at all; PAL Games already
lose the PAL frame's borders through the Core's overscan options (FR-EMB-13).

**Game37 finds the border itself and cuts it off.** For N64 Games it looks at the Core's frame now and
then and finds the rectangle outside which every pixel is black. It cuts to that rectangle once the same
rectangle has held for a few seconds, so a dark scene, a fade or a black loading screen doesn't make
the picture jump; a frame that is black all over changes nothing. It never cuts more than a fifth of the
frame from any side. The cut part is then fitted to the window like any picture: its aspect ratio is the
Core's, scaled by how much is cut each way, so nothing is stretched.

## Considered

- **One fixed cut for every N64 Game, like the PAL crop:** simple, but Games with a smaller border or
  none would lose picture at their edges, and Games with a larger one would keep bars.
- **A list of cuts per Game:** exact, but it has to be made and kept for every Game the owner has.
- **Leaving the border:** the owner wants the picture to fill the height.

## Consequences

- When a Game's border changes (a menu with a border, play without one), the picture changes size once,
  a few seconds later.
- A Game whose real picture is black along an edge for a long time can lose that strip until something
  is drawn there; the fifth-per-side limit bounds it.
- Save State thumbnails and the Core's frame are unchanged; only what is drawn on screen is cut.
- The PAL overscan options stay: they cut the PAL frame's borders before Game37 looks for the Game's own.
