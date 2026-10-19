//! The custom-drawn, controller-first UI (ADR 0002): drawing, text, input and screen navigation.
//!
//! All layout uses the logical 1920×1080 coordinates of the mockups in `design/mockups/`.

pub mod chrome;
pub mod controls;
pub mod cover;
pub mod gfx;
pub mod input;
pub mod nav;
pub mod scroll;
pub mod text;
pub mod theme;

/// Logical screen width.
pub const W: f32 = 1920.0;
/// Logical screen height.
pub const H: f32 = 1080.0;
/// Side margin.
pub const M: f32 = 96.0;

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
}
