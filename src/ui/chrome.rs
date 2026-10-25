//! The chrome every screen shares, after `background`, `glyph`, `hintbar`, `logo`, `gear_button`
//! and `topbar` in `design/generate_mockups.py`.

use super::gfx::{Align, Gfx, TextStyle};
use super::input::{Action, ButtonStyle, InputKind, glyph_label};
use super::nav::Hint;
use super::text::Weight;
use super::theme::{ACCENT, BG, Colour, LINE, MUTED, SURFACE, SURFACE2, TEXT};
use super::{H, M, Rect, W};

pub fn background(gfx: &mut Gfx<'_>) {
    background_tinted(gfx, ACCENT, 0.18);
}

/// The background glow in a chosen colour and opacity, e.g. a System's colour on its Console
/// Page (FR-CAR-3), subtler than the Carousel's tint.
pub fn background_tinted(gfx: &mut Gfx<'_>, colour: Colour, opacity: f32) {
    gfx.clear(BG);
    gfx.radial_glow(W / 2.0, 0.0, 0.9 * W, 0.9 * 520.0, colour, opacity);
}

const GLYPH_LABEL: TextStyle = TextStyle::new(14, Weight::Bold, TEXT);
const GLYPH_LETTER: TextStyle = TextStyle::new(16, Weight::Bold, TEXT);

/// Face-button letters and symbols, and Xbox's menu button, are drawn round, everything else as a
/// key cap.
pub fn is_round(label: &str) -> bool {
    matches!(label, "A" | "B" | "X" | "Y") || is_symbol(label)
}

/// The labels [`symbol`] draws as shapes: Adwaita Sans has no ✕ or ≡, and its ○ □ △ don't match
/// them.
pub fn is_symbol(label: &str) -> bool {
    matches!(label, "✕" | "○" | "□" | "△" | "≡")
}

/// Draws a PlayStation face-button symbol or Xbox's menu button ≡ in `ink`, about `size` across and
/// centred on (`cx`, `cy`), over a button filled with `fill`. Draws nothing for other labels.
pub fn symbol(gfx: &mut Gfx<'_>, cx: f32, cy: f32, size: f32, label: &str, ink: Colour, fill: Colour) {
    let stroke = size * 0.18;
    let h = size / 2.0;
    match label {
        "✕" => {
            let d = h * 0.8;
            gfx.line(cx - d, cy - d, cx + d, cy + d, stroke, ink);
            gfx.line(cx - d, cy + d, cx + d, cy - d, stroke, ink);
        }
        "○" => gfx.stroke_circle(cx, cy, h * 0.9, stroke, ink),
        "□" => {
            let d = h * 0.8;
            gfx.stroke_round_rect(Rect::new(cx - d, cy - d, 2.0 * d, 2.0 * d), 1.0, stroke, ink);
        }
        "△" => {
            // An outline as a filled triangle with the button's fill inset by the stroke; centred
            // by its height, so the centroid sits a quarter of the circumradius below the centre.
            let r = h * 1.05;
            let tri = |r: f32| {
                let (cy, sin, cos) = (cy + h * 1.05 / 4.0, 0.866 * r, 0.5 * r);
                [(cx, cy - r), (cx + sin, cy + cos), (cx - sin, cy + cos)]
            };
            gfx.fill_polygon(&tri(r), ink);
            gfx.fill_polygon(&tri(r - 2.0 * stroke), fill);
        }
        "≡" => {
            for dy in [-0.6 * h, 0.0, 0.6 * h] {
                gfx.line(cx - h * 0.8, cy + dy, cx + h * 0.8, cy + dy, stroke, ink);
            }
        }
        _ => {}
    }
}

pub fn glyph_width(gfx: &mut Gfx<'_>, label: &str) -> f32 {
    if is_round(label) {
        32.0
    } else {
        (gfx.measure(label, GLYPH_LABEL) + 18.0).max(40.0)
    }
}

/// Draws a controller or keyboard prompt with its left edge at `x`; returns its width.
pub fn glyph(gfx: &mut Gfx<'_>, x: f32, cy: f32, label: &str) -> f32 {
    if is_round(label) {
        gfx.fill_circle(x + 16.0, cy, 16.0, SURFACE2);
        gfx.stroke_circle(x + 16.0, cy, 16.0, 2.0, TEXT);
        if is_symbol(label) {
            symbol(gfx, x + 16.0, cy, 13.0, label, TEXT, SURFACE2);
        } else {
            gfx.text(x + 16.0, cy + 6.0, label, GLYPH_LETTER, Align::Centre);
        }
        return 32.0;
    }
    let w = glyph_width(gfx, label);
    let r = Rect::new(x, cy - 14.0, w, 28.0);
    gfx.fill_round_rect(r, 8.0, SURFACE2);
    gfx.stroke_round_rect(r, 8.0, 2.0, TEXT);
    gfx.text(x + w / 2.0, cy + 5.0, label, GLYPH_LABEL, Align::Centre);
    w
}

/// Draws the hint bar, right-aligned; returns each hint's clickable area and the action it
/// performs.
pub fn hintbar(
    gfx: &mut Gfx<'_>,
    hints: &[Hint],
    kind: InputKind,
    style: ButtonStyle,
    pointer: Option<(f32, f32)>,
) -> Vec<(Rect, Action)> {
    gfx.fill_rect(Rect::new(0.0, 1010.0, W, 70.0), BG.alpha(0.92));
    gfx.fill_rect(Rect::new(0.0, 1010.0, W, 1.0), LINE);
    let mut hits = Vec::new();
    let mut x = W - M;
    for hint in hints.iter().rev() {
        let desc = TextStyle::new(18, Weight::Medium, MUTED);
        let right = x;
        x -= gfx.measure(hint.text, desc);
        let desc_x = x;
        x -= 12.0;
        for &action in hint.actions.iter().rev() {
            let label = glyph_label(action, kind, style);
            x -= glyph_width(gfx, label);
            glyph(gfx, x, 1046.0, label);
            x -= 6.0;
        }
        let area = Rect::new(x - 6.0, 1011.0, right - x + 18.0, H - 1011.0);
        let hover = pointer.is_some_and(|(px, py)| area.contains(px, py));
        let desc = if hover {
            TextStyle { colour: TEXT, ..desc }
        } else {
            desc
        };
        gfx.text(desc_x, 1052.0, hint.text, desc, Align::Left);
        if let Some(&first) = hint.actions.first() {
            hits.push((area, first));
        }
        x -= 34.0;
    }
    hits
}

pub fn logo(gfx: &mut Gfx<'_>) {
    let style = TextStyle::new(30, Weight::ExtraBold, TEXT);
    let w = gfx.text(M, 72.0, "GAME", style, Align::Left);
    gfx.text(
        M + w + 1.0,
        72.0,
        "37",
        TextStyle {
            colour: ACCENT,
            ..style
        },
        Align::Left,
    );
}

/// What the top bar shows.
pub struct TopBar<'a> {
    /// Tab labels and the active one, when the screen has tabs.
    pub tabs: Option<(&'a [&'a str], usize)>,
    /// Title of the screen Back returns to, for the "‹ title" affordance.
    pub back: Option<&'a str>,
    /// The time, when the clock shows.
    pub clock: Option<&'a str>,
    /// Whether the search pill shows: only with a library, and not under the Search screen, which
    /// draws its own field there.
    pub search: bool,
    /// Whether the Settings gear shows: only with a library.
    pub settings: bool,
    pub kind: InputKind,
    pub style: ButtonStyle,
    pub pointer: Option<(f32, f32)>,
}

/// Clickable areas of the top bar.
pub struct TopBarHits {
    pub tabs: Vec<(Rect, usize)>,
    pub back: Option<Rect>,
    pub search: Option<Rect>,
    pub settings: Option<Rect>,
}

/// Horizontal distance between neighbouring tab label centres.
const TAB_SPACING: f32 = 164.0;

pub fn topbar(gfx: &mut Gfx<'_>, bar: &TopBar<'_>) -> TopBarHits {
    let hovered = |r: Rect| bar.pointer.is_some_and(|(x, y)| r.contains(x, y));
    logo(gfx);

    let back = bar.back.map(|title| {
        let style = TextStyle::new(20, Weight::Medium, MUTED);
        let label = format!("‹ {title}");
        let w = gfx.measure(&label, style);
        let area = Rect::new(M + 150.0 - 12.0, 44.0, w + 24.0, 40.0);
        let style = if hovered(area) {
            TextStyle {
                colour: TEXT,
                ..style
            }
        } else {
            style
        };
        gfx.text(M + 150.0, 70.0, &label, style, Align::Left);
        area
    });

    let mut tabs = Vec::new();
    if let Some((labels, active)) = bar.tabs {
        let cx = W / 2.0;
        let first = cx - (labels.len() - 1) as f32 * TAB_SPACING / 2.0;
        let last = first + (labels.len() - 1) as f32 * TAB_SPACING;
        glyph(
            gfx,
            first - 154.0,
            62.0,
            glyph_label(Action::PrevTab, bar.kind, bar.style),
        );
        glyph(
            gfx,
            last + 114.0,
            62.0,
            glyph_label(Action::NextTab, bar.kind, bar.style),
        );
        for (i, label) in labels.iter().enumerate() {
            let x = first + i as f32 * TAB_SPACING;
            let on = i == active;
            let bold = TextStyle::new(24, Weight::Bold, TEXT);
            let w = gfx.measure(label, bold);
            let area = Rect::new(x - w / 2.0 - 20.0, 36.0, w + 40.0, 64.0);
            let style = match (on, hovered(area)) {
                (true, _) => bold,
                (false, hover) => TextStyle::new(24, Weight::Medium, if hover { TEXT } else { MUTED }),
            };
            gfx.text(x, 70.0, label, style, Align::Centre);
            if on {
                gfx.fill_round_rect(Rect::new(x - w / 2.0, 86.0, w, 4.0), 2.0, ACCENT);
            }
            tabs.push((area, i));
        }
    }

    let search = bar.search.then(|| {
        let pill = Rect::new(W - M - 504.0, 44.0, 330.0, 48.0);
        search_pill(gfx, pill, bar, hovered(pill));
        pill
    });
    let settings = bar.settings.then(|| {
        let (cx, cy) = (W - M - 134.0, 68.0);
        let button = Rect::new(cx - 24.0, cy - 24.0, 48.0, 48.0);
        gear_button(gfx, cx, cy, hovered(button));
        button
    });

    if let Some(clock) = bar.clock {
        let style = TextStyle::new(24, Weight::SemiBold, TEXT);
        gfx.text(W - M, 76.0, clock, style, Align::Right);
    }
    TopBarHits {
        tabs,
        back,
        search,
        settings,
    }
}

/// The top bar's "Search games" pill, with both its keyboard and controller glyphs.
fn search_pill(gfx: &mut Gfx<'_>, pill: Rect, bar: &TopBar<'_>, hover: bool) {
    let sx = pill.x;
    gfx.fill_round_rect(pill, 24.0, SURFACE);
    gfx.stroke_round_rect(pill, 24.0, 1.0, LINE);
    gfx.stroke_circle(sx + 30.0, 66.0, 8.0, 2.5, MUTED);
    gfx.line(sx + 36.0, 72.0, sx + 42.0, 78.0, 2.5, MUTED);
    // Round caps, as in the mockup.
    gfx.fill_circle(sx + 36.0, 72.0, 1.25, MUTED);
    gfx.fill_circle(sx + 42.0, 78.0, 1.25, MUTED);
    let colour = if hover { TEXT } else { MUTED };
    let label = TextStyle::new(18, Weight::Regular, colour);
    gfx.text(sx + 52.0, 74.0, "Search games", label, Align::Left);
    let pad = glyph_label(Action::Search, InputKind::Controller, bar.style);
    let keys = glyph_label(Action::Search, InputKind::Keyboard, bar.style);
    let pad_x = pill.x + pill.w - 14.0 - glyph_width(gfx, pad);
    glyph(gfx, pad_x, 68.0, pad);
    let keys_x = pad_x - 8.0 - glyph_width(gfx, keys);
    glyph(gfx, keys_x, 68.0, keys);
}

/// The top bar's Settings button: a 48 px circle with the gear in it.
fn gear_button(gfx: &mut Gfx<'_>, cx: f32, cy: f32, hover: bool) {
    gfx.fill_circle(cx, cy, 24.0, SURFACE);
    gfx.stroke_circle(cx, cy, 24.0, 1.0, LINE);
    gear(gfx, cx, cy, 13.0, if hover { TEXT } else { MUTED }, SURFACE);
}

/// A Settings gear of outer radius `r`: eight teeth round a ring, its hole filled with `hole`.
fn gear(gfx: &mut Gfx<'_>, cx: f32, cy: f32, r: f32, fill: Colour, hole: Colour) {
    use std::f32::consts::TAU;
    // The outline isn't convex, but every point of it sees the centre: fan out from there.
    let mut points = vec![(cx, cy)];
    for k in 0..8 {
        let a = TAU * k as f32 / 8.0;
        for (da, rr) in [(-0.27, r * 0.74), (-0.16, r), (0.16, r), (0.27, r * 0.74)] {
            points.push((cx + rr * (a + da).cos(), cy + rr * (a + da).sin()));
        }
    }
    points.push(points[1]);
    gfx.fill_polygon(&points, fill);
    gfx.fill_circle(cx, cy, r * 0.36, hole);
}
