//! The custom-drawn, controller-first UI (ADR 0002): drawing, text, input and screen navigation.
//!
//! All layout uses the logical coordinates of the mockups in `design/mockups/`: 1920 wide and, for
//! the menus, as tall as the window's shape asks ([`h`], 1080 to 1440), so they fill a 16:10 screen.
//! A running Game and what is drawn over it stay at 1920×1080.

pub mod anim;
pub mod chrome;
pub mod controls;
pub mod cover;
pub mod gfx;
pub mod input;
pub mod nav;
pub mod scroll;
pub mod text;
pub mod text_entry;
pub mod theme;

use std::cell::Cell;

/// Logical screen width.
pub const W: f32 = 1920.0;
/// Logical screen height of the mockups and of a running Game, and the least the menus get: see [`h`].
pub const H: f32 = 1080.0;
/// The tallest the menus get (4:3); a taller window is letterboxed.
pub const MAX_H: f32 = 1440.0;
/// Side margin.
pub const M: f32 = 96.0;

thread_local! {
    /// The logical height this frame is drawn at. Per thread, so tests running in parallel each
    /// keep their own (the default, [`H`]).
    static HEIGHT: Cell<f32> = const { Cell::new(H) };
}

/// The logical height of the canvas this frame: [`H`] for a running Game and what is drawn over
/// it, and for the menus as tall as the window's shape asks ([`logical_height`]). Bottom-anchored
/// layout follows it; top-anchored layout stays at the mockups' coordinates.
pub fn h() -> f32 {
    HEIGHT.with(Cell::get)
}

/// Sets [`h`]. Only `Gfx::fit` calls it, together with the renderer's logical size, so the two
/// never disagree.
pub(crate) fn set_h(h: f32) {
    HEIGHT.with(|c| c.set(h));
}

/// Runs `f` with [`h`] set to `h`, restoring it afterwards even if `f` panics.
#[cfg(test)]
pub(crate) fn with_h<T>(h: f32, f: impl FnOnce() -> T) -> T {
    struct Restore(f32);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_h(self.0);
        }
    }
    let _restore = Restore(self::h());
    set_h(h);
    f()
}

/// The logical height for a window `pixel_w`×`pixel_h` pixels: the height that keeps 1920 logical
/// pixels across at the window's shape, from [`H`] (16:9; a wider window gets side bars) to
/// [`MAX_H`] (4:3; a taller one gets bars top and bottom). A zero-size (minimized) window gets [`H`].
pub fn logical_height(pixel_w: u32, pixel_h: u32) -> u32 {
    if pixel_w == 0 || pixel_h == 0 {
        return H as u32;
    }
    let h = (f64::from(W) * f64::from(pixel_h) / f64::from(pixel_w)).round();
    h.clamp(f64::from(H), f64::from(MAX_H)) as u32
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    /// Shrinks the rect by `d` on every side (grows it for negative `d`).
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(
            self.x + d,
            self.y + d,
            (self.w - 2.0 * d).max(0.0),
            (self.h - 2.0 * d).max(0.0),
        )
    }
}

/// The value of the first item whose rect contains the point: how screens map the pointer to the
/// item under it, for hover focus and click to confirm.
pub fn hit<T: Copy>(items: &[(Rect, T)], x: f32, y: f32) -> Option<T> {
    items
        .iter()
        .find(|(rect, _)| rect.contains(x, y))
        .map(|&(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_finds_the_item_under_the_point() {
        let items = [
            (Rect::new(0.0, 0.0, 10.0, 10.0), 'a'),
            (Rect::new(20.0, 0.0, 10.0, 10.0), 'b'),
        ];
        assert_eq!(hit(&items, 25.0, 5.0), Some('b'));
        assert_eq!(hit(&items, 0.0, 0.0), Some('a'));
        assert_eq!(hit(&items, 10.0, 5.0), None);
    }

    #[test]
    fn logical_height_fills_16_10_and_letterboxes_beyond_16_9_and_4_3() {
        // 16:10 at any resolution fills 1200 logical pixels.
        assert_eq!(logical_height(1920, 1200), 1200);
        assert_eq!(logical_height(1280, 800), 1200);
        assert_eq!(logical_height(3840, 2400), 1200);
        // 16:9, and 1366×768 (not quite 16:9), stay at the mockups' 1080.
        assert_eq!(logical_height(1600, 900), 1080);
        assert_eq!(logical_height(1366, 768), 1080);
        // Wider than 16:9 keeps 1080, with bars at the sides.
        assert_eq!(logical_height(2560, 1080), 1080);
        // 4:3 is the tallest; portrait is letterboxed at it.
        assert_eq!(logical_height(1024, 768), 1440);
        assert_eq!(logical_height(1080, 1920), 1440);
    }

    #[test]
    fn logical_height_of_a_minimized_window_is_the_mockups() {
        assert_eq!(logical_height(0, 0), 1080);
        assert_eq!(logical_height(0, 1200), 1080);
        assert_eq!(logical_height(1920, 0), 1080);
    }

    #[test]
    fn with_h_sets_the_height_and_restores_it() {
        assert_eq!(h(), H);
        with_h(1200.0, || assert_eq!(h(), 1200.0));
        assert_eq!(h(), H);
        let _ = std::panic::catch_unwind(|| with_h(1300.0, || panic!("boom")));
        assert_eq!(h(), H);
    }
}
