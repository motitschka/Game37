//! Scroll offsets for lists and grids that keep the focused item in view, and wheel notches.

/// The offset, nearest to `offset`, at which the span `start..start + len` (in content coordinates)
/// lies inside a view of length `view`. A span longer than the view is aligned to its start.
pub fn keep_visible(offset: f32, start: f32, len: f32, view: f32) -> f32 {
    if start < offset || len > view {
        start
    } else if start + len > offset + view {
        start + len - view
    } else {
        offset
    }
}

/// `offset` limited to the range that keeps content of length `content` filling a view of length
/// `view`: never before the start, never past the end.
pub fn clamp(offset: f32, content: f32, view: f32) -> f32 {
    offset.min(content - view).max(0.0)
}

/// A wheel's `dy` turned into whole notches (1.0 each): a mouse sends one event a notch, a
/// touchpad many small ones, so the `dy` not yet a whole notch is kept for the next event.
#[derive(Debug, Default, Clone, Copy)]
pub struct Notches {
    rest: f32,
}

impl Notches {
    /// Adds `dy` to what came before in the same direction and returns the whole notches it makes,
    /// signed like `dy`, keeping the rest. A turn the other way drops the rest and starts afresh.
    pub fn turn(&mut self, dy: f32) -> i32 {
        // A little short of 1.0, so ten touchpad steps of 0.1 make a notch whatever their rounding.
        const NOTCH: f32 = 1.0 - 1e-4;
        if dy * self.rest < 0.0 {
            self.rest = 0.0;
        }
        self.rest += dy;
        let mut notches = 0;
        while self.rest >= NOTCH {
            self.rest -= 1.0;
            notches += 1;
        }
        while self.rest <= -NOTCH {
            self.rest += 1.0;
            notches -= 1;
        }
        notches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_small_steps_make_two_notches_and_one_notch_makes_one() {
        let mut notches = Notches::default();
        let turned: i32 = (0..20).map(|_| notches.turn(0.1)).sum();
        assert_eq!(turned, 2);
        let mut notches = Notches::default();
        let turned: i32 = (0..20).map(|_| notches.turn(-0.1)).sum();
        assert_eq!(turned, -2, "the other way too");
        assert_eq!(Notches::default().turn(1.0), 1);
        assert_eq!(Notches::default().turn(-3.0), -3);
    }

    #[test]
    fn a_turn_the_other_way_drops_the_rest() {
        let mut notches = Notches::default();
        assert_eq!(notches.turn(0.6), 0);
        assert_eq!(notches.turn(-0.6), 0, "the 0.6 isn't carried");
        assert_eq!(notches.turn(-0.4), -1);
        assert_eq!(notches.turn(0.5), 0);
        assert_eq!(notches.turn(0.5), 1);
    }

    #[test]
    fn a_visible_span_keeps_the_offset() {
        assert_eq!(keep_visible(100.0, 150.0, 50.0, 300.0), 100.0);
        assert_eq!(keep_visible(100.0, 100.0, 300.0, 300.0), 100.0);
    }

    #[test]
    fn a_span_before_the_view_scrolls_back_to_its_start() {
        assert_eq!(keep_visible(100.0, 40.0, 50.0, 300.0), 40.0);
    }

    #[test]
    fn a_span_after_the_view_scrolls_until_its_end_shows() {
        assert_eq!(keep_visible(0.0, 280.0, 50.0, 300.0), 30.0);
    }

    #[test]
    fn a_span_longer_than_the_view_aligns_to_its_start() {
        assert_eq!(keep_visible(0.0, 100.0, 500.0, 300.0), 100.0);
    }

    #[test]
    fn clamp_stays_within_the_content() {
        assert_eq!(clamp(-20.0, 1000.0, 300.0), 0.0);
        assert_eq!(clamp(900.0, 1000.0, 300.0), 700.0);
        assert_eq!(clamp(50.0, 1000.0, 300.0), 50.0);
        assert_eq!(clamp(50.0, 200.0, 300.0), 0.0, "short content never scrolls");
    }
}
