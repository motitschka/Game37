//! Scroll offsets for lists and grids that keep the focused item in view.

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

#[cfg(test)]
mod tests {
    use super::*;

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
