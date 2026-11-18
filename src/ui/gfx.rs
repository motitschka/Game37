//! All drawing goes through [`Gfx`], so the SDL_Renderer backend can be swapped without touching
//! screens. Coordinates are logical pixels, 1920 wide and [`Gfx::h`] tall, letterboxed into the
//! window: see [`Gfx::fit`]. A running Game's picture and the backdrops over it are drawn in window
//! pixels instead, so they reach past the logical canvas to the window's edges (ADR 0020).

use std::collections::HashMap;
use std::hash::Hash;
use std::path::Path;

use sdl3::pixels::FColor;
use sdl3::render::{BlendMode, FPoint, FRect, Texture, TextureCreator, Vertex, VertexIndices, WindowCanvas};
use sdl3::sys::render::SDL_RendererLogicalPresentation;
use sdl3::video::{WindowContext, WindowFlags};

use super::Rect;
use super::text::{Fonts, Weight};
use super::theme::Colour;
use crate::artwork::Picture;
use crate::libretro;

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

/// Whether the window opens fullscreen (FR-UI-3, US-33). A change takes effect at the next start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowMode {
    /// As it was when Game37 last quit or F11 last toggled it; windowed the first time.
    #[default]
    RememberLast,
    AlwaysFullscreen,
    AlwaysWindowed,
}

impl WindowMode {
    pub const ALL: [WindowMode; 3] = [
        WindowMode::RememberLast,
        WindowMode::AlwaysFullscreen,
        WindowMode::AlwaysWindowed,
    ];

    /// The name the library's `settings` table stores it under.
    pub fn id(self) -> &'static str {
        match self {
            WindowMode::RememberLast => "remember_last",
            WindowMode::AlwaysFullscreen => "always_fullscreen",
            WindowMode::AlwaysWindowed => "always_windowed",
        }
    }

    pub fn from_id(id: &str) -> Option<WindowMode> {
        WindowMode::ALL.into_iter().find(|m| m.id() == id)
    }

    /// The label its Settings row shows.
    pub fn label(self) -> &'static str {
        match self {
            WindowMode::RememberLast => "Remember last",
            WindowMode::AlwaysFullscreen => "Always fullscreen",
            WindowMode::AlwaysWindowed => "Always windowed",
        }
    }

    /// The next mode in [`WindowMode::ALL`], wrapping around.
    pub fn next(self) -> WindowMode {
        let i = WindowMode::ALL.iter().position(|&m| m == self).unwrap_or(0);
        WindowMode::ALL[(i + 1) % WindowMode::ALL.len()]
    }

    /// The previous mode in [`WindowMode::ALL`], wrapping around.
    pub fn prev(self) -> WindowMode {
        let n = WindowMode::ALL.len();
        let i = WindowMode::ALL.iter().position(|&m| m == self).unwrap_or(0);
        WindowMode::ALL[(i + n - 1) % n]
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

/// A picture texture not drawn for this many frames is dropped: it is made again, from the
/// Artwork already in memory, when next drawn.
const PICTURE_FRAMES: u64 = 300;
/// How many bytes of picture textures are kept at most, about 130 covers or 16 heroes. Beyond it,
/// the least recently drawn are dropped first; those drawn this frame are always kept.
const PICTURE_BUDGET: usize = 128 * 1024 * 1024;

/// Textures for decoded Artwork, keyed by [`Picture::id`], bounded by [`PICTURE_FRAMES`] and
/// [`PICTURE_BUDGET`] so browsing thousands of Games doesn't grow GPU memory without limit.
struct PictureCache<V> {
    /// Each value with its size in bytes and the frame it was last drawn in.
    entries: HashMap<u64, (V, usize, u64)>,
    bytes: usize,
    frame: u64,
}

impl<V> PictureCache<V> {
    fn new() -> PictureCache<V> {
        PictureCache {
            entries: HashMap::new(),
            bytes: 0,
            frame: 0,
        }
    }

    /// The value for `id`, made with `make` if it isn't cached, marked as drawn this frame. Making
    /// one first drops the least recently drawn values, not drawn this frame, until `bytes` more
    /// fit the budget.
    fn get_or_insert_with(&mut self, id: u64, bytes: usize, make: impl FnOnce() -> V) -> &mut V {
        let frame = self.frame;
        if !self.entries.contains_key(&id) {
            while self.bytes + bytes > PICTURE_BUDGET {
                let oldest = self
                    .entries
                    .iter()
                    .filter(|(_, (_, _, used))| *used < frame)
                    .min_by_key(|(_, (_, _, used))| *used)
                    .map(|(id, _)| *id);
                let Some(oldest) = oldest else { break };
                self.remove(oldest);
            }
            self.entries.insert(id, (make(), bytes, frame));
            self.bytes += bytes;
        }
        let entry = self.entries.get_mut(&id).expect("inserted above");
        entry.2 = frame;
        &mut entry.0
    }

    fn remove(&mut self, id: u64) {
        if let Some((_, bytes, _)) = self.entries.remove(&id) {
            self.bytes -= bytes;
        }
    }

    /// Ends a frame, dropping the values not drawn for [`PICTURE_FRAMES`] frames.
    fn end_frame(&mut self) {
        self.frame += 1;
        let frame = self.frame;
        let mut dropped = 0;
        self.entries.retain(|_, (_, bytes, used)| {
            let keep = frame - *used < PICTURE_FRAMES;
            if !keep {
                dropped += *bytes;
            }
            keep
        });
        self.bytes -= dropped;
    }
}

/// The part of a `src_w`×`src_h` picture that fills a `dst_w`×`dst_h` rect without stretching:
/// the largest centred region with the rect's shape, as (x, y, w, h) in picture pixels. What lies
/// outside it is cropped.
pub fn crop_to_fill(src_w: f32, src_h: f32, dst_w: f32, dst_h: f32) -> (f32, f32, f32, f32) {
    if src_w <= 0.0 || src_h <= 0.0 || dst_w <= 0.0 || dst_h <= 0.0 {
        return (0.0, 0.0, src_w.max(0.0), src_h.max(0.0));
    }
    // Compare the shapes without dividing: is the picture wider than the rect?
    if src_w * dst_h > dst_w * src_h {
        let w = src_h * dst_w / dst_h;
        ((src_w - w) / 2.0, 0.0, w, src_h)
    } else {
        let h = src_w * dst_h / dst_w;
        (0.0, (src_h - h) / 2.0, src_w, h)
    }
}

/// Where and how strongly what is drawn lands: an offset in logical pixels and an opacity
/// (0 to 1), so a panel can slide and fade in as one. [`Layer::NONE`] draws as asked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layer {
    pub dx: f32,
    pub dy: f32,
    pub opacity: f32,
}

impl Layer {
    pub const NONE: Layer = Layer {
        dx: 0.0,
        dy: 0.0,
        opacity: 1.0,
    };

    fn point(self, p: FPoint) -> FPoint {
        FPoint::new(p.x + self.dx, p.y + self.dy)
    }

    fn rect(self, r: Rect) -> FRect {
        FRect::new(r.x + self.dx, r.y + self.dy, r.w, r.h)
    }

    /// `alpha` (0 to 255) at this layer's opacity.
    fn alpha(self, alpha: u8) -> u8 {
        (alpha as f32 * self.opacity.clamp(0.0, 1.0)).round() as u8
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
    /// The running Game's last frame. Kept here rather than in its screen, because a texture
    /// borrows the texture creator and a `Box<dyn Screen>` can't.
    game: Option<GameTexture<'tc>>,
    /// Images screens set by key, such as Save State screenshots.
    images: HashMap<&'static str, Texture<'tc>>,
    /// Artwork textures, or `None` where making one failed, so it isn't retried every frame.
    pictures: PictureCache<Option<Texture<'tc>>>,
    /// The renderer's logical height, as last set: `app::canvas` starts it at 1080.
    logical_h: u32,
    /// A fullscreen change asked of the window that it hasn't shown yet.
    fullscreen: FullscreenRequest,
    /// The layer everything is drawn on, see [`Gfx::with_layer`].
    layer: Layer,
    /// Whether things fade in as they arrive (`Ctx::animate`, set each frame by the app). Off by
    /// default, so screenshot mode and tests draw everything in full.
    animate: bool,
    /// When each Artwork picture was first drawn, so it fades in over [`ARTWORK_FADE_SECS`].
    arrived: HashMap<u64, std::time::Instant>,
}

/// How long Artwork takes to fade in over its placeholder when it arrives.
const ARTWORK_FADE_SECS: f32 = 0.18;

/// A fullscreen change asked of the window, until the window shows it. SDL applies fullscreen a
/// frame or more after it is asked, so F11 flips what was last asked rather than what the window
/// shows: otherwise a second F11 before the first took effect asks for the same again and is lost
/// (#50).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct FullscreenRequest {
    pending: Option<bool>,
}

impl FullscreenRequest {
    /// The window shows `actual`: a request it now matches is done.
    fn observe(&mut self, actual: bool) {
        if self.pending == Some(actual) {
            self.pending = None;
        }
    }

    /// Asks for `on`.
    fn ask(&mut self, on: bool) {
        self.pending = Some(on);
    }

    /// What F11 asks for while the window shows `actual`: the opposite of the pending request, or
    /// of `actual` when there is none.
    fn toggled(&mut self, actual: bool) -> bool {
        self.observe(actual);
        !self.pending.unwrap_or(actual)
    }
}

/// The logical height to draw at in a window `pixel_w`×`pixel_h` pixels: what is drawn over a
/// running Game keeps the mockups' 1080 (the Game's picture is drawn in window pixels), the menus
/// fill the window's shape.
fn fit_height(game: bool, pixel_w: u32, pixel_h: u32) -> u32 {
    if game {
        super::H as u32
    } else {
        super::logical_height(pixel_w, pixel_h)
    }
}

/// `r`, in window pixels, in logical coordinates, for a logical canvas drawn at `canvas` (window
/// pixels, 1920 logical pixels wide): as SDL converts the mouse ([`Gfx::to_logical`]). Outside the
/// canvas that is below 0 or past its width and height.
fn window_to_logical(r: Rect, canvas: Rect) -> Rect {
    if canvas.w <= 0.0 {
        return r;
    }
    let scale = canvas.w / super::W;
    Rect::new(
        (r.x - canvas.x) / scale,
        (r.y - canvas.y) / scale,
        r.w / scale,
        r.h / scale,
    )
}

/// Where `canvas`'s logical canvas is drawn in its window, in window pixels.
fn canvas_rect(canvas: &WindowCanvas) -> Rect {
    let mut r = sdl3::sys::rect::SDL_FRect {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };
    // SAFETY: the renderer is alive for as long as `canvas`.
    unsafe { sdl3::sys::render::SDL_GetRenderLogicalPresentationRect(canvas.raw(), &mut r) };
    Rect::new(r.x, r.y, r.w, r.h)
}

/// Runs `f` on `canvas` drawing in window pixels: SDL clips what is drawn in logical coordinates to
/// the logical canvas, so its logical presentation is off while `f` runs and back as it was after.
/// `f` gets `layer` in window pixels.
fn in_window_pixels<T>(
    canvas: &mut WindowCanvas,
    layer: Layer,
    f: impl FnOnce(&mut WindowCanvas, Layer) -> T,
) -> T {
    let scale = canvas_rect(canvas).w / super::W;
    let layer = Layer {
        dx: layer.dx * scale,
        dy: layer.dy * scale,
        opacity: layer.opacity,
    };
    let (w, h, mode) = canvas.logical_size();
    let _ = canvas.set_logical_size(0, 0, SDL_RendererLogicalPresentation::DISABLED);
    let out = f(canvas, layer);
    let _ = canvas.set_logical_size(w, h, mode);
    out
}

struct GameTexture<'tc> {
    texture: Texture<'tc>,
    width: u32,
    height: u32,
    format: libretro::PixelFormat,
}

fn sdl_format(format: libretro::PixelFormat) -> sdl3::pixels::PixelFormat {
    use sdl3::pixels::PixelFormat;
    match format {
        libretro::PixelFormat::Xrgb1555 => PixelFormat::XRGB1555,
        libretro::PixelFormat::Rgb565 => PixelFormat::RGB565,
        libretro::PixelFormat::Xrgb8888 => PixelFormat::XRGB8888,
        // The alpha a Core leaves in its framebuffer isn't the picture's (PPSSPP leaves 0): a
        // Game's picture is opaque.
        libretro::PixelFormat::Rgba32 => PixelFormat::RGBX32,
    }
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

/// Sharp pixels at any scale: SDL's pixel-art filter (SDL 3.4), else nearest neighbour.
fn set_pixel_art_scaling(texture: &Texture<'_>) {
    use sdl3::sys::render::SDL_SetTextureScaleMode;
    use sdl3::sys::surface::{SDL_SCALEMODE_NEAREST, SDL_SCALEMODE_PIXELART};
    // SAFETY: the texture is alive.
    unsafe {
        if !SDL_SetTextureScaleMode(texture.raw(), SDL_SCALEMODE_PIXELART) {
            SDL_SetTextureScaleMode(texture.raw(), SDL_SCALEMODE_NEAREST);
        }
    }
}

fn set_linear_scaling(texture: &Texture<'_>) {
    use sdl3::sys::render::SDL_SetTextureScaleMode;
    use sdl3::sys::surface::SDL_SCALEMODE_LINEAR;
    // SAFETY: the texture is alive.
    unsafe { SDL_SetTextureScaleMode(texture.raw(), SDL_SCALEMODE_LINEAR) };
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
            game: None,
            images: HashMap::new(),
            pictures: PictureCache::new(),
            logical_h: super::H as u32,
            fullscreen: FullscreenRequest::default(),
            layer: Layer::NONE,
            animate: false,
            arrived: HashMap::new(),
        }
    }

    /// Sets the logical size for this frame from the window's size in pixels: 1920×1080 for a
    /// running Game (`game`), so the Pause Menu and messages over it look as on a 16:9 screen (the
    /// Game's picture fills the window: [`Gfx::draw_game_frame`]); for the menus 1920
    /// wide and as tall as the window's shape asks, so they fill a 16:10 screen. Sets `ui::h` to
    /// match. Call it before drawing a frame and before converting the mouse.
    pub fn fit(&mut self, game: bool) {
        let (w, h) = self.output_size();
        let logical_h = fit_height(game, w, h);
        if logical_h != self.logical_h
            && self
                .canvas
                .set_logical_size(1920, logical_h, SDL_RendererLogicalPresentation::LETTERBOX)
                .is_ok()
        {
            self.logical_h = logical_h;
        }
        super::set_h(self.logical_h as f32);
    }

    /// The logical height this frame is drawn at (`ui::h`).
    pub fn h(&self) -> f32 {
        self.logical_h as f32
    }

    /// The window's size in pixels.
    fn output_size(&self) -> (u32, u32) {
        let (mut w, mut h) = (0, 0);
        // SAFETY: the renderer is alive for as long as `self.canvas`. Not the "Current" variant,
        // which follows a render target: the window's own size is wanted.
        unsafe { sdl3::sys::render::SDL_GetRenderOutputSize(self.canvas.raw(), &mut w, &mut h) };
        (w.max(0) as u32, h.max(0) as u32)
    }

    /// The window's size in pixels, which a running Game's picture is fitted to (ADR 0020).
    pub fn window_size(&self) -> (f32, f32) {
        let (w, h) = self.output_size();
        (w as f32, h as f32)
    }

    /// Converts a rect in window pixels, such as a Game's picture, to logical coordinates, as
    /// [`Gfx::to_logical`] converts the mouse.
    pub fn to_logical_rect(&self, r: Rect) -> Rect {
        window_to_logical(r, canvas_rect(&self.canvas))
    }

    /// Copies a Core's video frame into the game texture, remaking it when the frame's size or
    /// pixel format changed. Don't pass a duped frame: it's the one already uploaded.
    pub fn upload_game_frame(&mut self, frame: &libretro::Frame) {
        if frame.width == 0 || frame.height == 0 {
            return;
        }
        let fits = self
            .game
            .as_ref()
            .is_some_and(|g| (g.width, g.height, g.format) == (frame.width, frame.height, frame.format));
        if !fits {
            self.game = None;
            let texture = match self.creator.create_texture_streaming(
                sdl_format(frame.format),
                frame.width,
                frame.height,
            ) {
                Ok(texture) => texture,
                Err(e) => {
                    eprintln!("game37: can't make the game texture: {e}");
                    return;
                }
            };
            if frame.format == libretro::PixelFormat::Rgba32 {
                // Only frames a Core drew with OpenGL are RGBA. Those are often drawn at several
                // times the System's resolution: smooth scaling, not pixel art.
                set_linear_scaling(&texture);
            } else {
                set_pixel_art_scaling(&texture);
            }
            self.game = Some(GameTexture {
                texture,
                width: frame.width,
                height: frame.height,
                format: frame.format,
            });
        }
        let game = self.game.as_mut().expect("made above");
        if let Err(e) = game.texture.update(None, &frame.data, frame.pitch) {
            eprintln!("game37: can't upload the game frame: {e}");
        }
    }

    /// The window and the OpenGL context of an `opengl` renderer, for a Core's context to sit
    /// next to; `None` on another renderer.
    pub fn gl_context(&self) -> Option<(sdl3::video::Window, sdl3::sys::video::SDL_GLContext)> {
        use sdl3::sys::render::{SDL_FlushRenderer, SDL_GetRendererName};
        use sdl3::sys::video::SDL_GL_GetCurrentContext;
        let renderer = self.canvas.raw();
        // SAFETY: the renderer is alive; its name is a static string. Flushing activates the
        // renderer, which makes its context current.
        let context = unsafe {
            let name = SDL_GetRendererName(renderer);
            if name.is_null() || std::ffi::CStr::from_ptr(name) != c"opengl" {
                return None;
            }
            SDL_FlushRenderer(renderer);
            SDL_GL_GetCurrentContext()
        };
        (!context.is_null()).then(|| (self.canvas.window().clone(), context))
    }

    /// Draws the game texture stretched over `r`, in window pixels: a Game's picture fills the
    /// window's height or width, past the logical canvas (ADR 0020).
    pub fn draw_game_frame(&mut self, r: Rect) {
        let Some(game) = &mut self.game else { return };
        in_window_pixels(&mut self.canvas, self.layer, |canvas, layer| {
            game.texture.set_alpha_mod(layer.alpha(255));
            let _ = canvas.copy(&game.texture, None, layer.rect(r));
            game.texture.set_alpha_mod(255);
        });
    }

    /// Sets the image drawn for `key` from RGB pixels, or forgets it for `None`.
    pub fn set_image(&mut self, key: &'static str, image: Option<&crate::session::states::Rgb>) {
        self.images.remove(key);
        let Some(image) = image else { return };
        let made = self
            .creator
            .create_texture_static(sdl3::pixels::PixelFormat::RGB24, image.width, image.height)
            .map_err(|e| e.to_string())
            .and_then(|mut t| {
                t.update(None, &image.data, image.width as usize * 3)
                    .map_err(|e| e.to_string())?;
                Ok(t)
            });
        match made {
            Ok(texture) => {
                set_pixel_art_scaling(&texture);
                self.images.insert(key, texture);
            }
            Err(e) => eprintln!("game37: can't make an image: {e}"),
        }
    }

    /// Draws the image set for `key` stretched over `r`; returns whether there is one.
    pub fn draw_image(&mut self, key: &str, r: Rect) -> bool {
        let layer = self.layer;
        let Some(texture) = self.images.get_mut(key) else {
            return false;
        };
        texture.set_alpha_mod(layer.alpha(255));
        let _ = self.canvas.copy(texture, None, layer.rect(r));
        texture.set_alpha_mod(255);
        true
    }

    /// Draws Artwork filling `r` with corners of `radius`: scaled to cover `r` and cropped to it,
    /// centred, never stretched. Its texture is made on first draw and kept while it is drawn.
    pub fn draw_picture(&mut self, picture: &Picture, r: Rect, radius: f32) {
        let fade = self.picture_opacity(picture.id);
        self.draw_picture_at(picture, r, radius, fade);
    }

    /// Draws a picture built into Game37 (a System logo) like [`Gfx::draw_picture`] with square
    /// corners, but shown at once: it doesn't arrive, so it doesn't fade in like Artwork. Returns
    /// false, drawing nothing, when its texture can't be made.
    pub fn draw_builtin_picture(&mut self, picture: &Picture, r: Rect) -> bool {
        self.draw_picture_at(picture, r, 0.0, 1.0)
    }

    /// [`Gfx::draw_picture`] at `fade` (0 to 1) times the layer's opacity. Returns false, drawing
    /// nothing, when the picture's texture can't be made.
    fn draw_picture_at(&mut self, picture: &Picture, r: Rect, radius: f32, fade: f32) -> bool {
        let creator = self.creator;
        let texture = self
            .pictures
            .get_or_insert_with(picture.id, picture.rgba.len(), || {
                let made = creator
                    .create_texture_static(sdl3::pixels::PixelFormat::RGBA32, picture.width, picture.height)
                    .map_err(|e| e.to_string())
                    .and_then(|mut t| {
                        t.update(None, &picture.rgba, picture.width as usize * 4)
                            .map_err(|e| e.to_string())?;
                        Ok(t)
                    });
                match made {
                    Ok(mut texture) => {
                        set_linear_scaling(&texture);
                        texture.set_blend_mode(BlendMode::Blend);
                        Some(texture)
                    }
                    Err(e) => {
                        eprintln!("game37: can't make an Artwork texture: {e}");
                        None
                    }
                }
            });
        let Some(texture) = texture else { return false };
        let (pw, ph) = (picture.width as f32, picture.height as f32);
        let (cx, cy, cw, ch) = crop_to_fill(pw, ph, r.w, r.h);
        // Each point's place in the texture, from its place in `r`; the soft edge just outside
        // `r` samples the picture's own edge.
        let uv = |p: FPoint| {
            let u = (cx + (p.x - r.x) / r.w * cw) / pw;
            let v = (cy + (p.y - r.y) / r.h * ch) / ph;
            FPoint::new(u.clamp(0.0, 1.0), v.clamp(0.0, 1.0))
        };
        let textured = |p: FPoint, alpha: f32| Vertex {
            position: p,
            color: FColor {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: alpha,
            },
            tex_coord: uv(p),
        };
        let inner = rounded_outline(r.inset(FEATHER), radius - FEATHER);
        let outer = rounded_outline(r.inset(-FEATHER), radius + FEATHER);
        let n = inner.len();
        let mut vertices: Vec<Vertex> = inner.iter().map(|&p| textured(p, 1.0)).collect();
        vertices.extend(outer.iter().map(|&p| textured(p, 0.0)));
        let mut indices = Vec::with_capacity(n * 9);
        for i in 1..n - 1 {
            indices.extend_from_slice(&[0, i as i32, i as i32 + 1]);
        }
        band_indices(n, n, 0, &mut indices);
        let layer = self.layer;
        let opacity = layer.opacity.clamp(0.0, 1.0) * fade;
        let vertices: Vec<Vertex> = vertices
            .into_iter()
            .map(|v| Vertex {
                position: layer.point(v.position),
                color: FColor {
                    a: v.color.a * opacity,
                    ..v.color
                },
                tex_coord: v.tex_coord,
            })
            .collect();
        let _ = self
            .canvas
            .render_geometry(&vertices, Some(&*texture), VertexIndices::from(&indices[..]));
        true
    }

    /// The window's display refresh rate in Hz, if SDL knows it.
    pub fn refresh_rate(&self) -> Option<f64> {
        use sdl3::sys::video::{SDL_GetCurrentDisplayMode, SDL_GetDisplayForWindow};
        // SAFETY: the window is alive for as long as `self.canvas`. SDL returns null or a display
        // mode that stays valid until the display changes; it's read right away.
        let mode = unsafe {
            SDL_GetCurrentDisplayMode(SDL_GetDisplayForWindow(self.canvas.window().raw())).as_ref()
        }?;
        let rate = if mode.refresh_rate_numerator > 0 && mode.refresh_rate_denominator > 0 {
            mode.refresh_rate_numerator as f64 / mode.refresh_rate_denominator as f64
        } else {
            mode.refresh_rate as f64
        };
        (rate > 0.0).then_some(rate)
    }

    /// Whether things fade in as they arrive: the app sets it from `Ctx::animate` every frame.
    pub fn set_animate(&mut self, on: bool) {
        self.animate = on;
    }

    /// How far the Artwork picture `id` has faded in, 0 to 1: 1 once it has been shown for
    /// [`ARTWORK_FADE_SECS`], and always 1 without animation.
    pub fn picture_opacity(&mut self, id: u64) -> f32 {
        if !self.animate {
            return 1.0;
        }
        let now = std::time::Instant::now();
        let first = *self.arrived.entry(id).or_insert(now);
        ((now - first).as_secs_f32() / ARTWORK_FADE_SECS).min(1.0)
    }

    /// Draws what `f` draws on `layer`, on top of the layer already in use (offsets add up,
    /// opacities multiply), then goes back to it.
    pub fn with_layer<T>(&mut self, layer: Layer, f: impl FnOnce(&mut Self) -> T) -> T {
        let outer = self.layer;
        self.layer = Layer {
            dx: outer.dx + layer.dx,
            dy: outer.dy + layer.dy,
            opacity: outer.opacity * layer.opacity,
        };
        let out = f(self);
        self.layer = outer;
        out
    }

    pub fn clear(&mut self, c: Colour) {
        self.canvas
            .set_draw_color(sdl3::pixels::Color::RGBA(c.r, c.g, c.b, c.a));
        self.canvas.clear();
    }

    /// Fills the whole window with `c`, past the logical canvas too: a backdrop over everything
    /// drawn so far, a running Game's picture included wherever it shows (ADR 0020).
    pub fn fill_window(&mut self, c: Colour) {
        let (w, h) = self.window_size();
        in_window_pixels(&mut self.canvas, self.layer, |canvas, layer| {
            canvas.set_draw_color(sdl3::pixels::Color::RGBA(c.r, c.g, c.b, layer.alpha(c.a)));
            let _ = canvas.fill_rect(FRect::new(layer.dx, layer.dy, w, h));
        });
    }

    pub fn fill_rect(&mut self, r: Rect, c: Colour) {
        let layer = self.layer;
        self.canvas
            .set_draw_color(sdl3::pixels::Color::RGBA(c.r, c.g, c.b, layer.alpha(c.a)));
        let _ = self.canvas.fill_rect(layer.rect(r));
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
        let layer = self.layer;
        let rect = r
            .map(|r| Rect::new(r.x + layer.dx, r.y + layer.dy, r.w, r.h))
            .map(|r| {
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
        if self.layer == Layer::NONE {
            let _ = self
                .canvas
                .render_geometry(vertices, None, VertexIndices::from(indices));
        } else {
            let layered = self.layered(vertices);
            let _ = self
                .canvas
                .render_geometry(&layered, None, VertexIndices::from(indices));
        }
    }

    /// `vertices` moved and faded by the current layer.
    fn layered(&self, vertices: &[Vertex]) -> Vec<Vertex> {
        let layer = self.layer;
        vertices
            .iter()
            .map(|v| Vertex {
                position: layer.point(v.position),
                color: FColor {
                    a: v.color.a * layer.opacity.clamp(0.0, 1.0),
                    ..v.color
                },
                tex_coord: v.tex_coord,
            })
            .collect()
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
        let layer = self.layer;
        entry.texture.set_color_mod(c.r, c.g, c.b);
        entry.texture.set_alpha_mod(layer.alpha(c.a));
        let left = match align {
            Align::Left => x,
            Align::Centre => x - entry.w / 2.0,
            Align::Right => x - entry.w,
        } + layer.dx;
        let dst = FRect::new(
            left.round(),
            (y + layer.dy - entry.ascent).round(),
            entry.w,
            entry.h,
        );
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

    /// Shows the frame and drops text and Artwork not drawn for a while.
    pub fn present(&mut self) {
        self.canvas.present();
        self.texts.end_frame();
        self.widths.end_frame();
        self.pictures.end_frame();
    }

    /// Writes the current frame (before `present`) as a BMP: the whole window, a running Game's
    /// picture past the logical canvas included.
    pub fn save_bmp(&mut self, path: &Path) -> Result<(), String> {
        let surface = in_window_pixels(&mut self.canvas, self.layer, |canvas, _| canvas.read_pixels(None))
            .map_err(|e| e.to_string())?;
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

    /// Starts or stops the system's text input (and input method) on the window.
    pub fn set_text_input(&mut self, on: bool) {
        let window = self.canvas.window();
        let text_input = window.subsystem().text_input();
        if on {
            text_input.start(window);
        } else {
            text_input.stop(window);
        }
    }

    /// Whether the window is fullscreen now, which may lag a request by a frame or more. The loop
    /// reads it every frame, which also ends a request the window now shows.
    pub fn is_fullscreen(&mut self) -> bool {
        let actual = self.canvas.window().fullscreen_state() != sdl3::video::FullscreenType::Off;
        self.fullscreen.observe(actual);
        actual
    }

    pub fn set_fullscreen(&mut self, on: bool) {
        self.fullscreen.ask(on);
        let _ = self.canvas.window_mut().set_fullscreen(on);
    }

    /// F11: flips what was last asked, so a press before the last one took effect still counts.
    pub fn toggle_fullscreen(&mut self) {
        let actual = self.canvas.window().fullscreen_state() != sdl3::video::FullscreenType::Off;
        let on = self.fullscreen.toggled(actual);
        self.set_fullscreen(on);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// #50: two F11 presses before the window follows the first end where they started, and a
    /// change the window manager made after a request was shown is flipped from what it shows.
    #[test]
    fn f11_flips_the_last_request_until_the_window_shows_it() {
        let mut request = FullscreenRequest::default();
        // Windowed; F11 asks for fullscreen, and again before the window follows: back to windowed.
        let first = request.toggled(false);
        request.ask(first);
        assert!(first);
        let second = request.toggled(false);
        request.ask(second);
        assert!(!second, "the second press undoes the first");
        // The window never left windowed, so the request is done.
        request.observe(false);
        assert_eq!(request.pending, None);

        // F11 to fullscreen, which the window shows; then the window manager leaves fullscreen.
        let on = request.toggled(false);
        request.ask(on);
        request.observe(true);
        assert_eq!(request.pending, None);
        assert!(
            request.toggled(false),
            "F11 follows the window, not the old request"
        );
    }

    #[test]
    fn a_running_game_keeps_1080_and_the_menus_fill_the_window() {
        assert_eq!(fit_height(true, 1920, 1200), 1080);
        assert_eq!(fit_height(true, 1024, 768), 1080);
        assert_eq!(fit_height(false, 1920, 1200), 1200);
        assert_eq!(fit_height(false, 1920, 1080), 1080);
    }

    #[test]
    fn window_modes_cycle_both_ways_and_wrap() {
        use WindowMode::{AlwaysFullscreen, AlwaysWindowed, RememberLast};
        let all = [RememberLast, AlwaysFullscreen, AlwaysWindowed];
        assert_eq!(WindowMode::ALL, all);
        assert_eq!(
            all.map(WindowMode::next),
            [AlwaysFullscreen, AlwaysWindowed, RememberLast]
        );
        assert_eq!(
            all.map(WindowMode::prev),
            [AlwaysWindowed, RememberLast, AlwaysFullscreen]
        );
        assert_eq!(WindowMode::default(), RememberLast);
        assert_eq!(
            all.map(WindowMode::label),
            ["Remember last", "Always fullscreen", "Always windowed"]
        );
        for mode in all {
            assert_eq!(WindowMode::from_id(mode.id()), Some(mode));
        }
        assert_eq!(WindowMode::from_id("kiosk"), None);
    }

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

    /// One cover's worth of bytes: a 400×600 RGBA picture.
    const COVER_BYTES: usize = 400 * 600 * 4;

    #[test]
    fn a_picture_drawn_every_frame_keeps_its_texture() {
        let mut cache = PictureCache::new();
        let mut made = 0;
        for _ in 0..3 * PICTURE_FRAMES {
            cache.get_or_insert_with(7, COVER_BYTES, || made += 1);
            cache.end_frame();
        }
        assert_eq!(made, 1);
    }

    #[test]
    fn a_picture_not_drawn_for_300_frames_loses_its_texture() {
        let mut cache = PictureCache::new();
        cache.get_or_insert_with(7, COVER_BYTES, || ());
        for _ in 0..PICTURE_FRAMES - 1 {
            cache.end_frame();
        }
        assert!(cache.entries.contains_key(&7));
        cache.end_frame();
        assert!(!cache.entries.contains_key(&7));
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn over_the_budget_the_least_recently_drawn_textures_go_first() {
        let mut cache = PictureCache::new();
        let fit = (PICTURE_BUDGET / COVER_BYTES) as u64;
        // A Shelf scrolled past one cover a frame, the first cover drawn again on the way.
        for id in 0..fit {
            cache.get_or_insert_with(id, COVER_BYTES, || ());
            if id > 0 {
                cache.get_or_insert_with(0, COVER_BYTES, || ());
            }
            cache.end_frame();
        }
        cache.get_or_insert_with(fit, COVER_BYTES, || ());
        assert!(cache.bytes <= PICTURE_BUDGET);
        assert!(cache.entries.contains_key(&0), "drawn last frame");
        assert!(!cache.entries.contains_key(&1), "the least recently drawn");
        assert!(cache.entries.contains_key(&2));
        assert!(cache.entries.contains_key(&fit));
    }

    #[test]
    fn textures_drawn_this_frame_are_kept_over_the_budget() {
        let mut cache = PictureCache::new();
        let n = (PICTURE_BUDGET / COVER_BYTES) as u64 + 5;
        for id in 0..n {
            cache.get_or_insert_with(id, COVER_BYTES, || ());
        }
        assert_eq!(cache.entries.len() as u64, n);
        cache.end_frame();
        cache.get_or_insert_with(n, COVER_BYTES, || ());
        assert!(cache.bytes <= PICTURE_BUDGET);
    }

    #[test]
    fn a_failed_texture_is_not_made_again_every_frame() {
        let mut cache: PictureCache<Option<()>> = PictureCache::new();
        let mut tries = 0;
        for _ in 0..10 {
            cache.get_or_insert_with(7, COVER_BYTES, || {
                tries += 1;
                None
            });
            cache.end_frame();
        }
        assert_eq!(tries, 1);
    }

    #[test]
    fn a_picture_of_the_rects_shape_is_not_cropped() {
        assert_eq!(crop_to_fill(400.0, 600.0, 176.0, 264.0), (0.0, 0.0, 400.0, 600.0));
    }

    #[test]
    fn a_wider_picture_is_cropped_at_both_sides() {
        // A 342×482 cover in a 2:3 cover rect: 321.33 of its 342 columns, centred.
        let (x, y, w, h) = crop_to_fill(342.0, 482.0, 176.0, 264.0);
        assert!((w - 482.0 * 2.0 / 3.0).abs() < 1e-3, "{w}");
        assert!((x - (342.0 - w) / 2.0).abs() < 1e-3, "{x}");
        assert_eq!((y, h), (0.0, 482.0));
    }

    #[test]
    fn a_taller_picture_is_cropped_at_top_and_bottom() {
        let (x, y, w, h) = crop_to_fill(400.0, 800.0, 176.0, 264.0);
        assert_eq!((x, w), (0.0, 400.0));
        assert!((h - 600.0).abs() < 1e-3, "{h}");
        assert!((y - 100.0).abs() < 1e-3, "{y}");
    }

    #[test]
    fn a_hero_fills_the_screen_from_its_middle() {
        // A 1920×620 hero behind the 1920×1080 Details Page: its full height, the middle 57%.
        let (x, y, w, h) = crop_to_fill(1920.0, 620.0, 1920.0, 1080.0);
        assert_eq!((y, h), (0.0, 620.0));
        assert!((w - 620.0 * 16.0 / 9.0).abs() < 1e-2, "{w}");
        assert!((x + w / 2.0 - 960.0).abs() < 1e-2, "{x}");
    }

    /// A hidden offscreen window `w`×`h` pixels with a software renderer drawing in logical
    /// 1920×1080, as `app::canvas` sets it up. SDL must outlive the canvas. Hold
    /// `libretro::tests::serial()` while it lives, so no other test has SDL up at the same time.
    pub(crate) fn offscreen_canvas(w: u32, h: u32) -> (sdl3::Sdl, WindowCanvas) {
        sdl3::hint::set("SDL_VIDEO_DRIVER", "offscreen");
        let sdl = sdl3::init().unwrap();
        let window = sdl.video().unwrap().window("gfx", w, h).hidden().build().unwrap();
        let mut canvas = sdl3::render::create_renderer(window, Some(c"software")).unwrap();
        canvas
            .set_logical_size(1920, 1080, SDL_RendererLogicalPresentation::LETTERBOX)
            .unwrap();
        canvas.set_blend_mode(BlendMode::Blend);
        (sdl, canvas)
    }

    /// The window `gfx` shows, read back as RGB.
    pub(crate) struct Shot {
        pub w: u32,
        pub h: u32,
        pitch: usize,
        rgb: Vec<u8>,
    }

    impl Shot {
        /// Saves what `gfx` shows as a BMP in `dir` and reads it back.
        pub(crate) fn take(gfx: &mut Gfx<'_>, dir: &Path) -> Shot {
            let at = dir.join("shot.bmp");
            gfx.save_bmp(&at).unwrap();
            let shot = sdl3::surface::Surface::load_bmp(&at)
                .unwrap()
                .convert_format(sdl3::pixels::PixelFormat::RGB24)
                .unwrap();
            Shot {
                w: shot.width(),
                h: shot.height(),
                pitch: shot.pitch() as usize,
                rgb: shot.with_lock(<[u8]>::to_vec),
            }
        }

        pub(crate) fn at(&self, x: u32, y: u32) -> [u8; 3] {
            let i = y as usize * self.pitch + x as usize * 3;
            [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
        }
    }

    #[test]
    fn a_rect_in_window_pixels_converts_to_logical_coordinates() {
        // 1920×1200: the 1920×1080 canvas 60 pixels down.
        let canvas = Rect::new(0.0, 60.0, 1920.0, 1080.0);
        let picture = Rect::new(160.0, 0.0, 1600.0, 1200.0);
        assert_eq!(
            window_to_logical(picture, canvas),
            Rect::new(160.0, -60.0, 1600.0, 1200.0)
        );
        // 1280×720: the canvas fills the window at 2/3 scale.
        let canvas = Rect::new(0.0, 0.0, 1280.0, 720.0);
        let picture = Rect::new(160.0, 0.0, 960.0, 720.0);
        assert_eq!(
            window_to_logical(picture, canvas),
            Rect::new(240.0, 0.0, 1440.0, 1080.0)
        );
        // A minimized window has no canvas.
        assert_eq!(window_to_logical(picture, Rect::new(0.0, 0.0, 0.0, 0.0)), picture);
    }

    /// ADR 0020 on a 1920×1200 window: a 4:3 Game's picture is 1600×1200, past the 1920×1080
    /// canvas, as the renderer shows it; a click anywhere on it touches, mapped from the same rect.
    #[test]
    fn a_games_picture_fills_a_tall_window_and_the_mouse_touches_all_of_it() {
        use crate::session::input::{Mouse, touch};
        use crate::session::timing;

        let _serial = crate::libretro::tests::serial();
        let dir = tempfile::TempDir::new().unwrap();
        let (_sdl, canvas) = offscreen_canvas(1920, 1200);
        let creator = canvas.texture_creator();
        let mut gfx = Gfx::new(canvas, &creator, Fonts::new().unwrap());
        gfx.fit(true);
        assert_eq!((gfx.h(), gfx.window_size()), (1080.0, (1920.0, 1200.0)));

        // A red 4:3 frame (XRGB8888: B, G, R, X).
        let frame = libretro::Frame {
            width: 4,
            height: 3,
            pitch: 16,
            format: libretro::PixelFormat::Xrgb8888,
            data: [0x00, 0x00, 0xff, 0xff].repeat(12),
        };
        gfx.upload_game_frame(&frame);
        gfx.clear(Colour::hex(0x000000));
        let picture = timing::fit(4.0 / 3.0, gfx.window_size());
        gfx.draw_game_frame(picture);
        let shot = Shot::take(&mut gfx, dir.path());
        assert_eq!((shot.w, shot.h), (1920, 1200), "the shot is the whole window");
        let (red, black) = ([0xff, 0, 0], [0, 0, 0]);
        for (x, y) in [
            (160, 0),
            (1759, 0),
            (160, 1199),
            (1759, 1199),
            (960, 30),
            (960, 1170),
        ] {
            assert_eq!(shot.at(x, y), red, "the picture at ({x}, {y})");
        }
        for (x, y) in [(159, 600), (1760, 600), (0, 0), (1919, 1199)] {
            assert_eq!(shot.at(x, y), black, "the bar at ({x}, {y})");
        }

        // The mouse, in window coordinates as SDL reports it, converted as `app.rs` converts it.
        let logical_picture = gfx.to_logical_rect(picture);
        let click = |x, y| {
            let mouse = Mouse {
                at: Some(gfx.to_logical(x, y)),
                left: true,
            };
            touch(logical_picture, mouse).map(|p| (p.x, p.y))
        };
        assert_eq!(click(960.0, 600.0), Some((0, 0)), "the window's centre");
        assert_eq!(
            click(160.0, 0.0),
            Some((-0x7fff, -0x7fff)),
            "the top left, above the canvas"
        );
        assert_eq!(
            click(1760.0, 1200.0),
            Some((0x7fff, 0x7fff)),
            "the bottom right, below it"
        );
        assert_eq!(click(560.0, 900.0), Some((-0x4000, 0x4000)));
        assert!(click(960.0, 30.0).is_some(), "above the canvas, on the picture");
        assert_eq!(click(100.0, 600.0), None, "in the black bar");
        assert_eq!(click(1800.0, 30.0), None, "the other bar");
    }
}
