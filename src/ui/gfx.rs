//! All drawing goes through [`Gfx`], so the SDL_Renderer backend can be swapped without touching
//! screens. Coordinates are logical 1920×1080 pixels, letterboxed into the window.

use std::collections::HashMap;
use std::hash::Hash;
use std::path::Path;

use sdl3::pixels::FColor;
use sdl3::render::{BlendMode, FPoint, FRect, Texture, TextureCreator, Vertex, VertexIndices, WindowCanvas};
use sdl3::video::{WindowContext, WindowFlags};

use super::Rect;
use super::text::{Fonts, Weight};
use super::theme::Colour;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Centre,
    Right,
}

/// How a line of text looks: the mockups' font size (logical pixels), weight and fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextStyle {
    pub size: u16,
    pub weight: Weight,
    pub colour: Colour,
}

impl TextStyle {
    pub const fn new(size: u16, weight: Weight, colour: Colour) -> TextStyle {
        TextStyle { size, weight, colour }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TextKey {
    text: String,
    size: u16,
    weight: Weight,
}

impl TextKey {
    fn new(text: &str, style: TextStyle) -> TextKey {
        TextKey {
            text: text.to_owned(),
            size: style.size,
            weight: style.weight,
        }
    }
}

struct CachedText<'tc> {
    texture: Texture<'tc>,
    w: f32,
    h: f32,
    ascent: f32,
}

/// Cached text unused for this many frames is dropped from the cache.
const TEXT_CACHE_FRAMES: u64 = 600;

/// Values kept for as long as they are used: one not used for [`TEXT_CACHE_FRAMES`] frames is
/// dropped at the end of a frame.
struct FrameCache<K, V> {
    /// Each value with the frame it was last used in.
    entries: HashMap<K, (V, u64)>,
    frame: u64,
}

impl<K: Eq + Hash, V> FrameCache<K, V> {
    fn new() -> FrameCache<K, V> {
        FrameCache {
            entries: HashMap::new(),
            frame: 0,
        }
    }

    /// The value for `key`, made with `make` if it isn't cached, marked as used this frame.
    fn get_or_insert_with(&mut self, key: K, make: impl FnOnce() -> V) -> &mut V {
        let frame = self.frame;
        let entry = self.entries.entry(key).or_insert_with(|| (make(), frame));
        entry.1 = frame;
        &mut entry.0
    }

    /// Ends a frame, dropping the values not used for [`TEXT_CACHE_FRAMES`] frames.
    fn end_frame(&mut self) {
        self.frame += 1;
        let frame = self.frame;
        self.entries
            .retain(|_, (_, last_used)| frame - *last_used < TEXT_CACHE_FRAMES);
    }
}

pub struct Gfx<'tc> {
    canvas: WindowCanvas,
    creator: &'tc TextureCreator<WindowContext>,
    fonts: Fonts,
    /// Rendered text, or `None` where rendering failed, so it isn't retried every frame.
    texts: FrameCache<TextKey, Option<CachedText<'tc>>>,
    /// Text widths, measured without rendering.
    widths: FrameCache<TextKey, f32>,
}

fn fcolour(c: Colour) -> FColor {
    FColor {
        r: c.r as f32 / 255.0,
        g: c.g as f32 / 255.0,
        b: c.b as f32 / 255.0,
        a: c.a as f32 / 255.0,
    }
}

fn vertex(p: FPoint, color: FColor) -> Vertex {
    Vertex {
        position: p,
        color,
        tex_coord: FPoint::new(0.0, 0.0),
    }
}

/// Points per quarter circle of a rounded corner.
const CORNER_STEPS: usize = 10;

/// The outline of a rounded rect, clockwise, `4 * (CORNER_STEPS + 1)` points whatever the radius.
fn rounded_outline(r: Rect, radius: f32) -> Vec<FPoint> {
    let radius = radius.clamp(0.0, r.w.min(r.h) / 2.0);
    let corners = [
        (r.x + r.w - radius, r.y + radius, -90.0f32),
        (r.x + r.w - radius, r.y + r.h - radius, 0.0),
        (r.x + radius, r.y + r.h - radius, 90.0),
        (r.x + radius, r.y + radius, 180.0),
    ];
    let mut points = Vec::with_capacity(4 * (CORNER_STEPS + 1));
    for (cx, cy, start) in corners {
        for i in 0..=CORNER_STEPS {
            let a = (start + 90.0 * i as f32 / CORNER_STEPS as f32).to_radians();
            points.push(FPoint::new(cx + radius * a.cos(), cy + radius * a.sin()));
        }
    }
    points
}

/// Triangle indices joining two closed outlines with the same number of points.
fn band_indices(n: usize, outer: usize, inner: usize, indices: &mut Vec<i32>) {
    for i in 0..n {
        let j = (i + 1) % n;
        let (o0, o1, i0, i1) = (
            (outer + i) as i32,
            (outer + j) as i32,
            (inner + i) as i32,
            (inner + j) as i32,
        );
        indices.extend_from_slice(&[o0, o1, i1, o0, i1, i0]);
    }
}

/// Renders `text` into a texture, logging why if it can't.
fn render<'tc>(
    fonts: &mut Fonts,
    creator: &'tc TextureCreator<WindowContext>,
    text: &str,
    style: TextStyle,
) -> Option<CachedText<'tc>> {
    let face = fonts
        .face(style.weight, style.size)
        .map_err(|e| eprintln!("{e}"))
        .ok()?;
    let ascent = face.ascent() as f32;
    let surface = face
        .render(text)
        .map_err(|e| eprintln!("Can't render {text:?}: {e}"))
        .ok()?;
    let texture = creator.create_texture_from_surface(&surface).ok()?;
    let (w, h) = (texture.width() as f32, texture.height() as f32);
    Some(CachedText {
        texture,
        w,
        h,
        ascent,
    })
}

/// Half the width of the soft edge drawn around shapes, standing in for anti-aliasing.
const FEATHER: f32 = 0.5;

impl<'tc> Gfx<'tc> {
    pub fn new(
        mut canvas: WindowCanvas,
        creator: &'tc TextureCreator<WindowContext>,
        fonts: Fonts,
    ) -> Gfx<'tc> {
        canvas.set_blend_mode(BlendMode::Blend);
        Gfx {
            canvas,
            creator,
            fonts,
            texts: FrameCache::new(),
            widths: FrameCache::new(),
        }
    }

    pub fn clear(&mut self, c: Colour) {
        self.canvas
            .set_draw_color(sdl3::pixels::Color::RGBA(c.r, c.g, c.b, c.a));
        self.canvas.clear();
    }

    pub fn fill_rect(&mut self, r: Rect, c: Colour) {
        self.canvas
            .set_draw_color(sdl3::pixels::Color::RGBA(c.r, c.g, c.b, c.a));
        let _ = self.canvas.fill_rect(FRect::new(r.x, r.y, r.w, r.h));
    }

    pub fn fill_round_rect(&mut self, r: Rect, radius: f32, c: Colour) {
        self.fill_round_rect_with(r, radius, |_| c);
    }

    /// A rounded rect filled with a vertical gradient from `top` to `bottom`.
    pub fn fill_round_rect_vgradient(&mut self, r: Rect, radius: f32, top: Colour, bottom: Colour) {
        self.fill_round_rect_gradient(r, radius, top, bottom, (0.0, 1.0));
    }

    /// A rounded rect filled with a linear gradient from `from` at its top-left corner to `to` at
    /// `end`, given as fractions of the rect's width and height: an SVG `linearGradient` from
    /// (0, 0) to `end` in bounding-box units.
    pub fn fill_round_rect_gradient(
        &mut self,
        r: Rect,
        radius: f32,
        from: Colour,
        to: Colour,
        end: (f32, f32),
    ) {
        let (ex, ey) = end;
        let len2 = ex * ex + ey * ey;
        self.fill_round_rect_with(r, radius, |p| {
            let (u, v) = ((p.x - r.x) / r.w, (p.y - r.y) / r.h);
            from.mix(to, (u * ex + v * ey) / len2)
        });
    }

    /// A rounded rect coloured `colour_at` each point of its outline, interpolated in between.
    fn fill_round_rect_with(&mut self, r: Rect, radius: f32, colour_at: impl Fn(FPoint) -> Colour) {
        let inner = rounded_outline(r.inset(FEATHER), radius - FEATHER);
        let outer = rounded_outline(r.inset(-FEATHER), radius + FEATHER);
        let n = inner.len();
        let mut vertices: Vec<Vertex> = inner.iter().map(|&p| vertex(p, fcolour(colour_at(p)))).collect();
        vertices.extend(outer.iter().map(|&p| vertex(p, fcolour(colour_at(p).alpha(0.0)))));
        let mut indices = Vec::with_capacity(n * 9);
        for i in 1..n - 1 {
            indices.extend_from_slice(&[0, i as i32, i as i32 + 1]);
        }
        band_indices(n, n, 0, &mut indices);
        self.geometry(&vertices, &indices);
    }

    /// An outline of `width` centred on the rect's edge, like an SVG stroke.
    pub fn stroke_round_rect(&mut self, r: Rect, radius: f32, width: f32, c: Colour) {
        let half = width / 2.0;
        let edges = [-half - FEATHER, -half + FEATHER, half - FEATHER, half + FEATHER];
        let (solid, clear) = (fcolour(c), fcolour(c.alpha(0.0)));
        let mut vertices = Vec::new();
        for (k, d) in edges.into_iter().enumerate() {
            let colour = if k == 0 || k == 3 { clear } else { solid };
            vertices.extend(
                rounded_outline(r.inset(d), radius - d)
                    .into_iter()
                    .map(|p| vertex(p, colour)),
            );
        }
        let n = vertices.len() / 4;
        let mut indices = Vec::new();
        for k in 0..3 {
            band_indices(n, k * n, (k + 1) * n, &mut indices);
        }
        self.geometry(&vertices, &indices);
    }

    /// A dashed outline of `width` centred on the rect's edge, like an SVG stroke with
    /// `stroke-dasharray="dash gap"`.
    pub fn stroke_round_rect_dashed(
        &mut self,
        r: Rect,
        radius: f32,
        width: f32,
        dash: f32,
        gap: f32,
        c: Colour,
    ) {
        let outline = rounded_outline(r, radius);
        let n = outline.len();
        let mut along = 0.0;
        for i in 0..n {
            let (a, b) = (outline[i], outline[(i + 1) % n]);
            let len = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
            if len == 0.0 {
                continue;
            }
            // Walk this edge in pieces that end at dash or gap boundaries.
            let mut t = 0.0;
            while t < len {
                let phase = along % (dash + gap);
                let (on, left) = if phase < dash {
                    (true, dash - phase)
                } else {
                    (false, dash + gap - phase)
                };
                let step = left.min(len - t);
                if on {
                    let p = |d: f32| (a.x + (b.x - a.x) * d / len, a.y + (b.y - a.y) * d / len);
                    let (x1, y1) = p(t);
                    let (x2, y2) = p(t + step);
                    self.line(x1, y1, x2, y2, width, c);
                }
                t += step;
                along += step;
            }
        }
    }

    /// A straight line of `width` with square ends.
    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, c: Colour) {
        let len = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
        if len == 0.0 {
            return;
        }
        let (nx, ny) = (-(y2 - y1) / len * width / 2.0, (x2 - x1) / len * width / 2.0);
        self.fill_polygon(
            &[
                (x1 + nx, y1 + ny),
                (x2 + nx, y2 + ny),
                (x2 - nx, y2 - ny),
                (x1 - nx, y1 - ny),
            ],
            c,
        );
    }

    /// Fills a convex polygon.
    pub fn fill_polygon(&mut self, points: &[(f32, f32)], c: Colour) {
        if points.len() < 3 {
            return;
        }
        let colour = fcolour(c);
        let vertices: Vec<Vertex> = points
            .iter()
            .map(|&(x, y)| vertex(FPoint::new(x, y), colour))
            .collect();
        let mut indices = Vec::with_capacity(3 * (points.len() - 2));
        for i in 1..points.len() as i32 - 1 {
            indices.extend_from_slice(&[0, i, i + 1]);
        }
        self.geometry(&vertices, &indices);
    }

    /// Restricts drawing to `r` until called again; `None` draws everywhere.
    pub fn clip(&mut self, r: Option<Rect>) {
        let rect = r.map(|r| {
            sdl3::rect::Rect::new(
                r.x.floor() as i32,
                r.y.floor() as i32,
                r.w.ceil().max(1.0) as u32,
                r.h.ceil().max(1.0) as u32,
            )
        });
        self.canvas.set_clip_rect(rect);
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, c: Colour) {
        self.fill_round_rect(
            Rect::new(cx - radius, cy - radius, 2.0 * radius, 2.0 * radius),
            radius,
            c,
        );
    }

    pub fn stroke_circle(&mut self, cx: f32, cy: f32, radius: f32, width: f32, c: Colour) {
        let r = Rect::new(cx - radius, cy - radius, 2.0 * radius, 2.0 * radius);
        self.stroke_round_rect(r, radius, width, c);
    }

    /// An elliptical radial gradient from `c` at `opacity` in the centre to transparent at the
    /// radii, like the mockups' background glow.
    pub fn radial_glow(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: Colour, opacity: f32) {
        const RINGS: usize = 48;
        const SEGMENTS: usize = 96;
        let mut vertices = vec![vertex(FPoint::new(cx, cy), fcolour(c.alpha(opacity)))];
        for ring in 1..=RINGS {
            let t = ring as f32 / RINGS as f32;
            let colour = fcolour(c.alpha(opacity * (1.0 - t)));
            for s in 0..SEGMENTS {
                let a = std::f32::consts::TAU * s as f32 / SEGMENTS as f32;
                vertices.push(vertex(
                    FPoint::new(cx + rx * t * a.cos(), cy + ry * t * a.sin()),
                    colour,
                ));
            }
        }
        let mut indices = Vec::new();
        for s in 0..SEGMENTS {
            indices.extend_from_slice(&[0, 1 + s as i32, 1 + ((s + 1) % SEGMENTS) as i32]);
        }
        for ring in 1..RINGS {
            band_indices(
                SEGMENTS,
                1 + ring * SEGMENTS,
                1 + (ring - 1) * SEGMENTS,
                &mut indices,
            );
        }
        self.geometry(&vertices, &indices);
    }

    fn geometry(&mut self, vertices: &[Vertex], indices: &[i32]) {
        let _ = self
            .canvas
            .render_geometry(vertices, None, VertexIndices::from(indices));
    }

    /// Width of `text` as [`Gfx::text`] would draw it. Where SDL_ttf can't measure it, an estimate
    /// on the wide side, so layouts still truncate.
    pub fn measure(&mut self, text: &str, style: TextStyle) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let fonts = &mut self.fonts;
        *self.widths.get_or_insert_with(TextKey::new(text, style), || {
            match fonts
                .face(style.weight, style.size)
                .and_then(|face| face.width(text))
            {
                Ok(w) => w as f32,
                Err(e) => {
                    let estimate = text.chars().count() as f32 * style.size as f32 * 0.6;
                    eprintln!("Can't measure {text:?}: {e}; estimating {estimate} px");
                    estimate
                }
            }
        })
    }

    /// Draws one line of text with its baseline at `y`, aligned on `x`. Returns its width.
    pub fn text(&mut self, x: f32, y: f32, text: &str, style: TextStyle, align: Align) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let (fonts, creator) = (&mut self.fonts, self.creator);
        let cached = self
            .texts
            .get_or_insert_with(TextKey::new(text, style), || render(fonts, creator, text, style));
        let Some(entry) = cached else {
            return 0.0;
        };
        let c = style.colour;
        entry.texture.set_color_mod(c.r, c.g, c.b);
        entry.texture.set_alpha_mod(c.a);
        let left = match align {
            Align::Left => x,
            Align::Centre => x - entry.w / 2.0,
            Align::Right => x - entry.w,
        };
        let dst = FRect::new(left.round(), (y - entry.ascent).round(), entry.w, entry.h);
        let _ = self.canvas.copy(&entry.texture, None, dst);
        entry.w
    }

    /// Splits `text` into lines no wider than `max_w`, breaking at spaces.
    pub fn wrap(&mut self, text: &str, style: TextStyle, max_w: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in text.lines() {
            let mut line = String::new();
            for word in paragraph.split(' ') {
                let candidate = if line.is_empty() {
                    word.to_owned()
                } else {
                    format!("{line} {word}")
                };
                if !line.is_empty() && self.measure(&candidate, style) > max_w {
                    lines.push(std::mem::replace(&mut line, word.to_owned()));
                } else {
                    line = candidate;
                }
            }
            lines.push(line);
        }
        lines
    }

    /// Shows the frame and drops text not drawn for a while.
    pub fn present(&mut self) {
        self.canvas.present();
        self.texts.end_frame();
        self.widths.end_frame();
    }

    /// Writes the current frame (before `present`) as a BMP.
    pub fn save_bmp(&self, path: &Path) -> Result<(), String> {
        let surface = self.canvas.read_pixels(None).map_err(|e| e.to_string())?;
        surface.save_bmp(path).map_err(|e| e.to_string())
    }

    /// Converts window coordinates (as in mouse events) to logical coordinates.
    pub fn to_logical(&self, x: f32, y: f32) -> (f32, f32) {
        let (mut lx, mut ly) = (x, y);
        // SAFETY: the renderer is alive for as long as `self.canvas`.
        unsafe {
            sdl3::sys::render::SDL_RenderCoordinatesFromWindow(self.canvas.raw(), x, y, &mut lx, &mut ly);
        }
        (lx, ly)
    }

    /// Whether the window can be seen: neither minimized nor hidden.
    pub fn is_shown(&self) -> bool {
        let flags = WindowFlags::from(self.canvas.window().window_flags());
        !flags.intersects(WindowFlags::MINIMIZED | WindowFlags::HIDDEN)
    }

    /// Gets out of the way while a Standalone Emulator runs.
    pub fn minimize(&mut self) {
        self.canvas.window_mut().minimize();
    }

    /// Comes back in front. GNOME on Wayland refuses a bare raise, but lets a window restore
    /// itself from minimised.
    pub fn restore(&mut self) {
        let window = self.canvas.window_mut();
        window.restore();
        window.raise();
    }

    pub fn toggle_fullscreen(&mut self) {
        let window = self.canvas.window_mut();
        let fullscreen = window.fullscreen_state() != sdl3::video::FullscreenType::Off;
        let _ = window.set_fullscreen(!fullscreen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_used_every_frame_is_never_evicted() {
        let mut cache = FrameCache::new();
        let mut made = 0;
        for _ in 0..3 * TEXT_CACHE_FRAMES {
            cache.get_or_insert_with("Chrono", || {
                made += 1;
                6.0
            });
            cache.end_frame();
        }
        assert_eq!(made, 1);
    }

    #[test]
    fn an_unused_entry_is_evicted_after_600_frames() {
        let mut cache = FrameCache::new();
        cache.get_or_insert_with("Chrono", || 6.0);
        for _ in 0..TEXT_CACHE_FRAMES - 1 {
            cache.end_frame();
        }
        assert!(cache.entries.contains_key("Chrono"));
        cache.end_frame();
        assert!(!cache.entries.contains_key("Chrono"));
    }

    #[test]
    fn a_cached_failure_is_not_retried() {
        let mut cache: FrameCache<&str, Option<f32>> = FrameCache::new();
        let mut tries = 0;
        for _ in 0..10 {
            let value = cache.get_or_insert_with("too wide", || {
                tries += 1;
                None
            });
            assert_eq!(*value, None);
            cache.end_frame();
        }
        assert_eq!(tries, 1);
    }
}
