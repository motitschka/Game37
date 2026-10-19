//! The chrome every screen shares, after `background`, `glyph`, `hintbar`, `logo` and `topbar` in
//! `design/generate_mockups.py`.

use super::gfx::{Align, Gfx, TextStyle};
use super::input::{Action, ButtonStyle, InputKind, glyph_label};
use super::nav::Hint;
use super::text::Weight;
use super::theme::{ACCENT, BG, LINE, MUTED, SURFACE2, TEXT};
use super::{H, M, Rect, W};

pub fn background(gfx: &mut Gfx<'_>) {
    gfx.clear(BG);
    gfx.radial_glow(W / 2.0, 0.0, 0.9 * W, 0.9 * 520.0, ACCENT, 0.18);
}

const GLYPH_LABEL: TextStyle = TextStyle::new(14, Weight::Bold, TEXT);
const GLYPH_LETTER: TextStyle = TextStyle::new(16, Weight::Bold, TEXT);

/// Face-button letters are drawn round, everything else as a key cap.
fn is_round(label: &str) -> bool {
    matches!(label, "A" | "B" | "X" | "Y")
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
        gfx.text(x + 16.0, cy + 6.0, label, GLYPH_LETTER, Align::Centre);
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
    pub clock: &'a str,
    pub kind: InputKind,
    pub style: ButtonStyle,
    pub pointer: Option<(f32, f32)>,
}

/// Clickable areas of the top bar.
pub struct TopBarHits {
    pub tabs: Vec<(Rect, usize)>,
    pub back: Option<Rect>,
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

    gfx.text(
        W - M,
        76.0,
        bar.clock,
        TextStyle::new(24, Weight::SemiBold, TEXT),
        Align::Right,
    );
    TopBarHits { tabs, back }
}
