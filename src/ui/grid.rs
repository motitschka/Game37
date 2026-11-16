//! The layout every grid of covers shares (FR-UI-7): Console Pages, "See all" and Search results.
//! The player sets how many Games a row holds; the row always runs from margin to margin with the
//! same gaps, so the covers grow or shrink to fit.

use super::cover::COVER_GAP;
use super::{M, Rect, W};

/// The fewest and most Games per row the player can choose, and the number until they do.
pub const MIN_PER_ROW: usize = 6;
pub const MAX_PER_ROW: usize = 10;
pub const DEFAULT_PER_ROW: usize = 8;

/// Space between one row's covers and the next row's.
const ROW_GAP: f32 = 72.0;

/// A grid's columns, cover size and pitches for one Games-per-row setting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub cols: usize,
    pub cover_w: f32,
    pub cover_h: f32,
    /// From one cover's left edge to the next one's.
    pub pitch_x: f32,
    /// From one row's top to the next one's.
    pub pitch_y: f32,
}

impl Grid {
    /// The grid with `per_row` Games per row, kept to [`MIN_PER_ROW`]..=[`MAX_PER_ROW`].
    pub fn new(per_row: usize) -> Grid {
        let cols = per_row.clamp(MIN_PER_ROW, MAX_PER_ROW);
        let n = cols as f32;
        let cover_w = (W - 2.0 * M - (n - 1.0) * COVER_GAP) / n;
        let cover_h = cover_w * 1.5;
        Grid {
            cols,
            cover_w,
            cover_h,
            pitch_x: cover_w + COVER_GAP,
            pitch_y: cover_h + ROW_GAP,
        }
    }

    /// How tall `len` Games' rows are.
    pub fn content_h(&self, len: usize) -> f32 {
        len.div_ceil(self.cols) as f32 * self.pitch_y
    }

    /// Where Game `i`'s cover goes in a grid whose first row's top is at `top`.
    pub fn cover(&self, i: usize, top: f32) -> Rect {
        let (row, col) = (i / self.cols, i % self.cols);
        Rect::new(
            M + col as f32 * self.pitch_x,
            top + row as f32 * self.pitch_y,
            self.cover_w,
            self.cover_h,
        )
    }
}

impl Default for Grid {
    fn default() -> Grid {
        Grid::new(DEFAULT_PER_ROW)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Within a hundredth of a pixel: the widths are not whole numbers.
    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn every_row_runs_from_margin_to_margin_with_2_by_3_covers() {
        for n in MIN_PER_ROW..=MAX_PER_ROW {
            let grid = Grid::new(n);
            assert_eq!(grid.cols, n);
            let first = grid.cover(0, 0.0);
            let last = grid.cover(n - 1, 0.0);
            assert_eq!(first.x, 96.0, "{n} per row starts at the margin");
            assert!(
                close(last.x + last.w, 1824.0),
                "{n} per row ends at {}",
                last.x + last.w
            );
            assert!(
                close(grid.pitch_x - grid.cover_w, COVER_GAP),
                "{n} per row keeps the gap"
            );
            assert!(close(grid.cover_h, grid.cover_w * 1.5), "{n} per row is 2:3");
            assert_eq!(grid.cover(n, 0.0).x, 96.0, "the next Game starts the next row");
            assert!(close(grid.cover(n, 0.0).y, grid.cover_h + 72.0));
        }
        assert!(close(Grid::new(8).cover_w, 191.5));
        assert!(close(Grid::new(8).cover_h, 287.25));
    }

    #[test]
    fn a_count_outside_6_to_10_is_kept_within_it() {
        assert_eq!(Grid::new(3).cols, 6);
        assert_eq!(Grid::new(40).cols, 10);
        assert_eq!(Grid::default().cols, 8);
    }

    #[test]
    fn content_h_counts_a_part_row_as_a_row() {
        let grid = Grid::new(6);
        assert_eq!(grid.content_h(0), 0.0);
        assert_eq!(grid.content_h(6), grid.pitch_y);
        assert_eq!(grid.content_h(7), 2.0 * grid.pitch_y);
    }
}
