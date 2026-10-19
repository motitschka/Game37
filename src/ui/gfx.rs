//! All drawing goes through [`Gfx`], so the SDL_Renderer backend can be swapped without touching
//! screens. Coordinates are logical 1920×1080 pixels, letterboxed into the window.

use std::collections::HashMap;
use std::path::Path;

use sdl3::pixels::FColor;
use sdl3::render::{BlendMode, FPoint, FRect, Texture, TextureCreator, Vertex, VertexIndices, WindowCanvas};
use sdl3::video::WindowContext;

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

struct CachedText<'tc> {
    texture: Texture<'tc>,
    w: f32,
    h: f32,
    ascent: f32,
    last_used: u64,
}

/// Rendered text unused for this many frames is dropped from the cache.
const TEXT_CACHE_FRAMES: u64 = 600;

pub struct Gfx<'tc> {
    canvas: WindowCanvas,
    creator: &'tc TextureCreator<WindowContext>,
    fonts: Fonts,
    texts: HashMap<TextKey, CachedText<'tc>>,
    frame: u64,
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
            texts: HashMap::new(),
            frame: 0,
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
        self.fill_round_rect_vgradient(r, radius, c, c);
    }

    /// A rounded rect filled with a vertical gradient from `top` to `bottom`.
    pub fn fill_round_rect_vgradient(&mut self, r: Rect, radius: f32, top: Colour, bottom: Colour) {
        let (top, bottom) = (fcolour(top), fcolour(bottom));
        let at = |p: FPoint| {
            let t = if r.h > 0.0 {
                ((p.y - r.y) / r.h).clamp(0.0, 1.0)
            } else {
                0.0
            };
            FColor {
                r: top.r + (bottom.r - top.r) * t,
                g: top.g + (bottom.g - top.g) * t,
                b: top.b + (bottom.b - top.b) * t,
                a: top.a + (bottom.a - top.a) * t,
            }
        };
        let inner = rounded_outline(r.inset(FEATHER), radius - FEATHER);
        let outer = rounded_outline(r.inset(-FEATHER), radius + FEATHER);
        let n = inner.len();
        let mut vertices: Vec<Vertex> = inner.iter().map(|&p| vertex(p, at(p))).collect();
        vertices.extend(outer.iter().map(|&p| vertex(p, FColor { a: 0.0, ..at(p) })));
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

    /// Makes sure `text` is rendered and cached, returning its key.
    fn cached(&mut self, text: &str, size: u16, weight: Weight) -> Option<TextKey> {
        let key = TextKey {
            text: text.to_owned(),
            size,
            weight,
        };
        if !self.texts.contains_key(&key) {
            let face = self.fonts.face(weight, size).map_err(|e| eprintln!("{e}")).ok()?;
            let ascent = face.ascent() as f32;
            let surface = face
                .render(text)
                .map_err(|e| eprintln!("Can't render {text:?}: {e}"))
                .ok()?;
            let texture = self.creator.create_texture_from_surface(&surface).ok()?;
            let (w, h) = (texture.width() as f32, texture.height() as f32);
            self.texts.insert(
                key.clone(),
                CachedText {
                    texture,
                    w,
                    h,
                    ascent,
                    last_used: self.frame,
                },
            );
        }
        Some(key)
    }

    /// Width of `text` as [`Gfx::text`] would draw it.
    pub fn measure(&mut self, text: &str, style: TextStyle) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        self.cached(text, style.size, style.weight)
            .map_or(0.0, |key| self.texts[&key].w)
    }

    /// Draws one line of text with its baseline at `y`, aligned on `x`. Returns its width.
    pub fn text(&mut self, x: f32, y: f32, text: &str, style: TextStyle, align: Align) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let Some(key) = self.cached(text, style.size, style.weight) else {
            return 0.0;
        };
        let c = style.colour;
        let frame = self.frame;
        let entry = self.texts.get_mut(&key).expect("just cached");
        entry.last_used = frame;
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
        self.frame += 1;
        let frame = self.frame;
        self.texts.retain(|_, t| frame - t.last_used < TEXT_CACHE_FRAMES);
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

    pub fn toggle_fullscreen(&mut self) {
        let window = self.canvas.window_mut();
        let fullscreen = window.fullscreen_state() != sdl3::video::FullscreenType::Off;
        let _ = window.set_fullscreen(!fullscreen);
    }
}
