//! The tall Game cover used by Shelves and grids, after `cover` in `design/generate_mockups.py`.
//!
//! Until Artwork arrives (FR-ART-3), a cover is placeholder art: a gradient in the System's colour,
//! the Display Title and a System badge.

use super::Rect;
use super::gfx::{Align, Gfx, TextStyle};
use super::text::Weight;
use super::theme::{ACCENT, Colour, TEXT};
use crate::library::db::GameRow;
use crate::systems;

/// Cover size on Shelves and grids.
pub const COVER_W: f32 = 176.0;
pub const COVER_H: f32 = 264.0;
/// Space between neighbouring covers.
pub const COVER_GAP: f32 = 28.0;
/// Corner radius of a cover.
pub const COVER_RADIUS: f32 = 10.0;
/// How far the focus ring reaches outside the rect it surrounds.
pub const RING_REACH: f32 = 15.0;

const ELLIPSIS: char = '…';
/// Lines of Display Title a cover shows at most.
const TITLE_LINES: usize = 3;
/// How much of the System's colour the bottom of the placeholder gradient keeps.
const SHADE: f32 = 0.22;

const WHITE: Colour = Colour::hex(0xFFFFFF);
const BLACK: Colour = Colour::hex(0x000000);

/// Draws `game`'s cover in `r`. `badge` shows the System badge, which a page about a single
/// System leaves out.
pub fn draw(gfx: &mut Gfx<'_>, r: Rect, game: &GameRow, badge: bool, focused: bool) {
    let system = systems::by_id(&game.system);
    let colour = Colour::hex(system.map_or(0x5B6477, |s| s.colour));
    if focused {
        focus_ring(gfx, r, COVER_RADIUS);
    }
    gfx.fill_round_rect_vgradient(r, COVER_RADIUS, colour, shade(colour));
    gfx.fill_circle(r.x + r.w * 0.68, r.y + r.h * 0.3, r.w * 0.26, WHITE.alpha(0.12));
    gfx.fill_circle(r.x + r.w * 0.68, r.y + r.h * 0.3, r.w * 0.13, WHITE.alpha(0.16));
    let corner = COVER_RADIUS;
    gfx.fill_polygon(
        &[
            (r.x, r.y + r.h * 0.62),
            (r.x + r.w, r.y + r.h * 0.5),
            (r.x + r.w, r.y + r.h - corner),
            (r.x + r.w - corner, r.y + r.h),
            (r.x + corner, r.y + r.h),
            (r.x, r.y + r.h - corner),
        ],
        BLACK.alpha(0.3),
    );

    let size = (r.w * 0.105).round();
    let style = TextStyle::new(size as u16, Weight::ExtraBold, TEXT);
    let lines = fit_lines(&game.title, r.w - 24.0, TITLE_LINES, |s| gfx.measure(s, style));
    let n = lines.len();
    for (i, line) in lines.iter().enumerate() {
        let y = r.y + r.h - 16.0 - (n - 1 - i) as f32 * size * 1.12;
        gfx.text(r.x + 12.0, y, line, style, Align::Left);
    }

    if badge {
        let style = TextStyle::new(11, Weight::Bold, TEXT);
        let label = system.map_or("?", |s| s.short);
        let label = ellipsize(label, r.w - 38.0, |s| gfx.measure(s, style));
        let w = gfx.measure(&label, style) + 18.0;
        gfx.fill_round_rect(Rect::new(r.x + 10.0, r.y + 10.0, w, 22.0), 11.0, BLACK.alpha(0.5));
        gfx.text(r.x + 19.0, r.y + 25.0, &label, style, Align::Left);
    }
}

/// The focus ring around a rect with corner radius `radius`: a white ring in a faint accent glow.
pub fn focus_ring(gfx: &mut Gfx<'_>, r: Rect, radius: f32) {
    gfx.stroke_round_rect(r.inset(-10.0), radius + 8.0, 10.0, ACCENT.alpha(0.4));
    gfx.stroke_round_rect(r.inset(-5.0), radius + 5.0, 4.0, WHITE);
}

/// The dark end of a placeholder gradient.
fn shade(c: Colour) -> Colour {
    let f = |v: u8| (v as f32 * SHADE).round() as u8;
    Colour {
        r: f(c.r),
        g: f(c.g),
        b: f(c.b),
        a: c.a,
    }
}

/// A play time as "12h 05m".
pub fn playtime_label(secs: u64) -> String {
    let minutes = secs / 60;
    format!("{}h {:02}m", minutes / 60, minutes % 60)
}

/// Wraps `text` at spaces into at most `max_lines` lines no wider than `max_w`, as `measure`
/// reports widths. A word too wide for a line, and the last line when text is left over, end in
/// an ellipsis.
pub fn fit_lines(
    text: &str,
    max_w: f32,
    max_lines: usize,
    mut measure: impl FnMut(&str) -> f32,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if measure(&candidate) <= max_w {
            line = candidate;
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        if measure(word) <= max_w {
            line = word.to_owned();
        } else {
            lines.push(ellipsize(word, max_w, &mut measure));
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        let last = lines.pop().unwrap_or_default();
        lines.push(with_ellipsis(&last, max_w, &mut measure));
    }
    lines
}

/// `text` if it fits in `max_w`, else its longest prefix that fits with an ellipsis added.
pub fn ellipsize(text: &str, max_w: f32, mut measure: impl FnMut(&str) -> f32) -> String {
    if measure(text) <= max_w {
        return text.to_owned();
    }
    with_ellipsis(text, max_w, &mut measure)
}

/// The longest prefix of `text` that fits in `max_w` with an ellipsis added, always ending in one.
fn with_ellipsis(text: &str, max_w: f32, measure: &mut impl FnMut(&str) -> f32) -> String {
    let ends: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .skip(1)
        .chain([text.len()])
        .collect();
    let fitted = |end: usize| format!("{}{ELLIPSIS}", text[..end].trim_end());
    // Binary search for the longest prefix that fits: widths only grow with length.
    let (mut lo, mut hi) = (0, ends.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if measure(&fitted(ends[mid - 1])) <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    match lo {
        0 => ELLIPSIS.to_string(),
        n => fitted(ends[n - 1]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character is 10 wide.
    fn mono(s: &str) -> f32 {
        s.chars().count() as f32 * 10.0
    }

    #[test]
    fn short_titles_stay_on_one_line() {
        assert_eq!(fit_lines("Persona 4", 150.0, 3, mono), ["Persona 4"]);
    }

    #[test]
    fn titles_wrap_at_spaces() {
        assert_eq!(
            fit_lines("Kirby Planet Robobot", 150.0, 3, mono),
            ["Kirby Planet", "Robobot"]
        );
    }

    #[test]
    fn titles_longer_than_the_lines_end_in_an_ellipsis() {
        let lines = fit_lines("Monster Hunter Freedom Unite Portable Edition", 100.0, 3, mono);
        assert_eq!(lines, ["Monster", "Hunter", "Freedom…"]);
        assert!(lines.iter().all(|l| mono(l) <= 100.0));
    }

    #[test]
    fn a_word_wider_than_a_line_is_cut_with_an_ellipsis() {
        let lines = fit_lines("Supercalifragilistic Kart", 100.0, 3, mono);
        assert_eq!(lines, ["Supercali…", "Kart"]);
    }

    #[test]
    fn ellipsize_keeps_what_fits() {
        assert_eq!(ellipsize("GAMECUBE", 80.0, mono), "GAMECUBE");
        assert_eq!(ellipsize("GAMECUBE", 50.0, mono), "GAME…");
        assert_eq!(ellipsize("Pokémon", 40.0, mono), "Pok…");
        assert_eq!(ellipsize("GAMECUBE", 5.0, mono), "…");
    }

    #[test]
    fn playtime_shows_hours_and_minutes() {
        assert_eq!(playtime_label(0), "0h 00m");
        assert_eq!(playtime_label(45_600), "12h 40m");
    }
}
