//! The black border an N64 Game draws around its own picture, which Game37 finds in the Core's
//! frame and cuts off when it draws the picture (ADR 0021). The Core's frame and Save State
//! thumbnails keep it: only what is drawn is cut.

use crate::libretro::{Frame, PixelFormat};
use crate::session::input::WHOLE_FRAME;
use crate::session::timing;
use crate::ui::Rect;

/// The brightest a border pixel's channels are, of 255. A Game's black is not always 0: dithering
/// and the Core's smooth scaling leave a little light in it.
const BLACK: u8 = 16;
/// The most cut from any side: a fifth of the frame's width or height.
const MOST: u32 = 5;
/// About how many pixels of a row (and three quarters as many of a column) are looked at: an N64
/// Game's own resolution, so a Core drawing at several times it costs no more.
const ALONG: u32 = 320;
/// The Core's frame is looked at once every this many Core frames.
const SAMPLE_EVERY: u64 = 15;
/// How many Core frames the same border must hold before it is cut: 3 seconds at 60 frames a
/// second, 3.6 at PAL's 50.
const HOLD_FRAMES: u64 = 180;

/// A rect in a frame's pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Area {
    fn right(self) -> u32 {
        self.x + self.w
    }

    fn bottom(self) -> u32 {
        self.y + self.h
    }

    /// The smallest area holding both.
    fn union(self, other: Area) -> Area {
        let (x, y) = (self.x.min(other.x), self.y.min(other.y));
        let (right, bottom) = (self.right().max(other.right()), self.bottom().max(other.bottom()));
        Area {
            x,
            y,
            w: right - x,
            h: bottom - y,
        }
    }

    /// Whether every edge of `other` is within a couple of pixels of this one's, in a frame
    /// `size` pixels big: the same border, give or take the Core's smooth scaling.
    fn close(self, other: Area, (w, h): (u32, u32)) -> bool {
        let (tx, ty) = ((w / 200).max(2), (h / 200).max(2));
        self.x.abs_diff(other.x) <= tx
            && self.right().abs_diff(other.right()) <= tx
            && self.y.abs_diff(other.y) <= ty
            && self.bottom().abs_diff(other.bottom()) <= ty
    }

    /// This area in fractions of a frame `size` pixels big.
    fn part(self, (w, h): (u32, u32)) -> Rect {
        let (w, h) = (w as f32, h as f32);
        Rect::new(
            self.x as f32 / w,
            self.y as f32 / h,
            self.w as f32 / w,
            self.h as f32 / h,
        )
    }
}

/// The part of `frame` outside which every pixel is black, in its pixels, cutting at most a fifth
/// of the frame from any side; `None` when the frame is black all over (or empty). Each row and
/// column of the outer fifth of the frame is looked at, at about [`ALONG`] of its pixels, and the
/// rows between at every few: what lies beyond the fifth is cut to it anyway, so the cost does not
/// grow with the resolution.
pub fn content(frame: &Frame) -> Option<Area> {
    let (w, h) = (frame.width, frame.height);
    let bpp = frame.format.bytes_per_pixel();
    if w == 0 || h == 0 || frame.data.len() < (h as usize - 1) * frame.pitch + w as usize * bpp {
        return None;
    }
    let lit = |x: u32, y: u32| {
        let at = y as usize * frame.pitch + x as usize * bpp;
        lit(frame.format, &frame.data[at..at + bpp])
    };
    let (step_x, step_y) = ((w / ALONG).max(1) as usize, (h / (ALONG * 3 / 4)).max(1) as usize);
    let row = |y: u32| (0..w).step_by(step_x).any(|x| lit(x, y));
    let (most_x, most_y) = (w / MOST, h / MOST);
    // Nothing lit in a side's outer fifth cuts that fifth.
    let top = (0..most_y).find(|&y| row(y));
    let bottom = (h - most_y..h).rev().find(|&y| row(y)).map(|y| y + 1);
    if top.is_none() && bottom.is_none() && !(most_y..h - most_y).step_by(step_y).any(row) {
        return None;
    }
    let (top, bottom) = (top.unwrap_or(most_y), bottom.unwrap_or(h - most_y));
    // A column is looked at in every `step_y`th row from `top` to `bottom`.
    let column = |x: u32| (top..bottom).step_by(step_y).any(|y| lit(x, y));
    let left = (0..most_x).find(|&x| column(x)).unwrap_or(most_x);
    let right = (w - most_x..w)
        .rev()
        .find(|&x| column(x))
        .map_or(w - most_x, |x| x + 1);
    Some(Area {
        x: left,
        y: top,
        w: right - left,
        h: bottom - top,
    })
}

/// Whether a pixel `px` in `format` has a channel brighter than [`BLACK`].
fn lit(format: PixelFormat, px: &[u8]) -> bool {
    let [r, g, b] = match format {
        PixelFormat::Xrgb8888 => [px[2], px[1], px[0]],
        PixelFormat::Rgba32 => [px[0], px[1], px[2]],
        PixelFormat::Rgb565 => {
            let v = u16::from_le_bytes([px[0], px[1]]);
            [(v >> 11) << 3, (v >> 5 & 0x3F) << 2, (v & 0x1F) << 3].map(|c| c as u8)
        }
        PixelFormat::Xrgb1555 => {
            let v = u16::from_le_bytes([px[0], px[1]]);
            [(v >> 10 & 0x1F) << 3, (v >> 5 & 0x1F) << 3, (v & 0x1F) << 3].map(|c| c as u8)
        }
    };
    r.max(g).max(b) > BLACK
}

/// Where a Game's picture goes when `part` of its frame (in fractions of it) is drawn: fitted to a
/// window `window` pixels big like the whole frame would be (see [`timing::fit`]), at the Core's
/// `aspect` scaled by how much is cut each way, so nothing is stretched.
pub fn fit_part(aspect: f32, part: Rect, window: (f32, f32)) -> Rect {
    timing::fit(aspect * part.w / part.h, window)
}

/// The border a Game has drawn for a while, followed over its session (ADR 0021): every
/// [`SAMPLE_EVERY`] Core frames the frame's [`content`] is looked at, and once the same one has
/// been seen in [`HOLD_FRAMES`] worth of samples it is the part drawn. A frame black all over, or
/// lit only in the middle, changes nothing: it neither counts nor resets.
#[derive(Debug, Default)]
pub struct BorderCut {
    /// Core frames seen.
    frames: u64,
    /// The frame size `candidate` and `cut` are in.
    size: (u32, u32),
    candidate: Option<Candidate>,
    /// The part drawn, in pixels; `None` draws the whole frame.
    cut: Option<Area>,
}

/// A border seen in the last samples, not yet held long enough to be cut.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    /// The content first seen, which the later samples are compared with.
    first: Area,
    /// All the samples' content together, which is what is cut: nothing drawn in any of them.
    seen: Area,
    /// How many samples after the first were the same border: black ones are not counted, so a
    /// border is not held across a loading screen it was not drawn through.
    held: u64,
}

impl BorderCut {
    /// Follows the Core's latest frame, run or duplicated.
    pub fn observe(&mut self, frame: &Frame) {
        let size = (frame.width, frame.height);
        if size != self.size {
            self.resize(size);
        }
        self.frames += 1;
        if !self.frames.is_multiple_of(SAMPLE_EVERY) {
            return;
        }
        let Some(area) = content(frame) else { return };
        // A cut held to the limit on both sides of an axis is only what the Core drew in the middle
        // (a logo, some text), not a border: it would be the same every time.
        let (most_x, most_y) = (size.0 / MOST, size.1 / MOST);
        if (area.x >= most_x && area.right() + most_x <= size.0)
            || (area.y >= most_y && area.bottom() + most_y <= size.1)
        {
            return;
        }
        match &mut self.candidate {
            Some(candidate) if candidate.first.close(area, size) => {
                candidate.seen = candidate.seen.union(area);
                candidate.held += 1;
                // The same border back after a fade keeps its cut, rather than moving a pixel.
                if candidate.held >= HOLD_FRAMES / SAMPLE_EVERY
                    && !self.cut.is_some_and(|cut| cut.close(candidate.seen, size))
                {
                    self.cut = Some(candidate.seen);
                }
            }
            _ => {
                self.candidate = Some(Candidate {
                    first: area,
                    seen: area,
                    held: 0,
                })
            }
        }
    }

    /// The frame size changed: a cut stays where the frame was only scaled (a Core drawing at
    /// another resolution), and is dropped otherwise. The border is looked for again.
    fn resize(&mut self, (w, h): (u32, u32)) {
        let (old_w, old_h) = self.size;
        let scaled = u64::from(w) * u64::from(old_h) == u64::from(h) * u64::from(old_w);
        self.cut = self.cut.filter(|_| scaled && w > 0 && h > 0).map(|cut| {
            let x = |v: u32| (u64::from(v) * u64::from(w) / u64::from(old_w)) as u32;
            let y = |v: u32| (u64::from(v) * u64::from(h) / u64::from(old_h)) as u32;
            let (left, top) = (x(cut.x), y(cut.y));
            Area {
                x: left,
                y: top,
                w: x(cut.right()) - left,
                h: y(cut.bottom()) - top,
            }
        });
        self.candidate = None;
        self.size = (w, h);
    }

    /// The part of the Core's frame drawn, in fractions of it.
    pub fn part(&self) -> Rect {
        self.cut.map_or(WHOLE_FRAME, |cut| cut.part(self.size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BANJO: Area = Area {
        x: 40,
        y: 23,
        w: 570,
        h: 427,
    };

    /// One pixel of grey `v` in `format`.
    fn grey(format: PixelFormat, v: u8) -> Vec<u8> {
        let v16 = u16::from(v);
        match format {
            PixelFormat::Xrgb8888 => vec![v, v, v, 0],
            PixelFormat::Rgba32 => vec![v, v, v, 255],
            PixelFormat::Rgb565 => ((v16 >> 3) << 11 | (v16 >> 2) << 5 | v16 >> 3)
                .to_le_bytes()
                .to_vec(),
            PixelFormat::Xrgb1555 => ((v16 >> 3) << 10 | (v16 >> 3) << 5 | v16 >> 3)
                .to_le_bytes()
                .to_vec(),
        }
    }

    /// A `w`×`h` frame in `format`, grey `inside` over `picture` and grey `outside` around it.
    fn framed(format: PixelFormat, (w, h): (u32, u32), picture: Area, inside: u8, outside: u8) -> Frame {
        let (bright, dark) = (grey(format, inside), grey(format, outside));
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let ins =
                    (picture.x..picture.right()).contains(&x) && (picture.y..picture.bottom()).contains(&y);
                data.extend_from_slice(if ins { &bright } else { &dark });
            }
        }
        Frame {
            width: w,
            height: h,
            pitch: w as usize * format.bytes_per_pixel(),
            format,
            data,
        }
    }

    fn banjo(format: PixelFormat) -> Frame {
        framed(format, (640, 480), BANJO, 128, 0)
    }

    fn black(format: PixelFormat) -> Frame {
        framed(format, (640, 480), BANJO, 0, 0)
    }

    const FORMATS: [PixelFormat; 4] = [
        PixelFormat::Xrgb8888,
        PixelFormat::Rgba32,
        PixelFormat::Rgb565,
        PixelFormat::Xrgb1555,
    ];

    #[test]
    fn banjo_kazooies_border_is_found_exactly_in_every_pixel_format() {
        for format in FORMATS {
            assert_eq!(content(&banjo(format)), Some(BANJO), "{format:?}");
        }
    }

    #[test]
    fn a_frame_without_a_border_is_all_content() {
        let whole = Area {
            x: 0,
            y: 0,
            w: 640,
            h: 480,
        };
        assert_eq!(
            content(&framed(PixelFormat::Xrgb8888, (640, 480), whole, 128, 0)),
            Some(whole)
        );
    }

    #[test]
    fn a_frame_black_all_over_has_no_content() {
        for format in FORMATS {
            assert_eq!(content(&black(format)), None, "{format:?}");
        }
        let empty = Frame::default();
        assert_eq!(content(&empty), None);
    }

    #[test]
    fn a_dark_picture_is_not_black_but_a_border_with_noise_below_the_threshold_is() {
        for format in FORMATS {
            let whole = Area {
                x: 0,
                y: 0,
                w: 640,
                h: 480,
            };
            // A night scene: dark grey all over, the picture to the edges.
            let dark = framed(format, (640, 480), whole, 40, 0);
            assert_eq!(content(&dark), Some(whole), "{format:?}");
            // Noise in the border, below the threshold.
            let noisy = framed(format, (640, 480), BANJO, 128, 12);
            assert_eq!(content(&noisy), Some(BANJO), "{format:?}");
        }
    }

    #[test]
    fn a_picture_with_just_one_lit_pixel_is_cut_to_it_within_a_fifth() {
        let dot = Area {
            x: 320,
            y: 240,
            w: 1,
            h: 1,
        };
        // A lone pixel stands on a column and row the scan looks at.
        let frame = framed(PixelFormat::Xrgb8888, (640, 480), dot, 255, 0);
        assert_eq!(
            content(&frame),
            Some(Area {
                x: 128,
                y: 96,
                w: 384,
                h: 288
            })
        );
    }

    #[test]
    fn no_side_loses_more_than_a_fifth() {
        let small = Area {
            x: 200,
            y: 10,
            w: 300,
            h: 300,
        };
        let frame = framed(PixelFormat::Xrgb8888, (640, 480), small, 128, 0);
        // Left cut 200 > 128 and right 140 > 128 are held to 128; top 10 is kept; bottom 170 > 96.
        assert_eq!(
            content(&frame),
            Some(Area {
                x: 128,
                y: 10,
                w: 384,
                h: 374
            })
        );
    }

    #[test]
    fn a_frame_drawn_at_several_times_the_resolution_is_found_exactly_too() {
        let big = Area {
            x: 160,
            y: 92,
            w: 2280,
            h: 1708,
        };
        let frame = framed(PixelFormat::Rgba32, (2560, 1920), big, 128, 0);
        assert_eq!(content(&frame), Some(big));
    }

    fn run(cut: &mut BorderCut, frame: &Frame, frames: u64) {
        for _ in 0..frames {
            cut.observe(frame);
        }
    }

    fn banjo_part() -> Rect {
        BANJO.part((640, 480))
    }

    #[test]
    fn a_border_is_cut_only_once_it_has_held_for_three_seconds() {
        let mut cut = BorderCut::default();
        let frame = banjo(PixelFormat::Rgba32);
        run(&mut cut, &frame, HOLD_FRAMES);
        assert_eq!(cut.part(), WHOLE_FRAME, "too soon");
        run(&mut cut, &frame, SAMPLE_EVERY);
        assert_eq!(cut.part(), banjo_part());
    }

    #[test]
    fn a_fade_to_black_or_a_moment_of_another_border_keeps_the_cut() {
        let mut cut = BorderCut::default();
        let frame = banjo(PixelFormat::Rgba32);
        run(&mut cut, &frame, 2 * HOLD_FRAMES);
        assert_eq!(cut.part(), banjo_part());
        // A black loading screen, longer than the hold.
        run(&mut cut, &black(PixelFormat::Rgba32), 3 * HOLD_FRAMES);
        assert_eq!(cut.part(), banjo_part(), "after black");
        // A full-screen picture for a second.
        let whole = Area {
            x: 0,
            y: 0,
            w: 640,
            h: 480,
        };
        let full = framed(PixelFormat::Rgba32, (640, 480), whole, 128, 0);
        run(&mut cut, &full, 60);
        assert_eq!(cut.part(), banjo_part(), "after a moment without the border");
        // The fade's darker frames, a different border on each sample.
        for v in (20..128).rev().step_by(8) {
            let shrunk = Area {
                x: BANJO.x + u32::from(128 - v) / 4,
                ..BANJO
            };
            run(
                &mut cut,
                &framed(PixelFormat::Rgba32, (640, 480), shrunk, v, 0),
                SAMPLE_EVERY,
            );
        }
        assert_eq!(cut.part(), banjo_part(), "after a fade");
        // A border that stays does change it.
        run(&mut cut, &full, 2 * HOLD_FRAMES);
        assert_eq!(cut.part(), WHOLE_FRAME, "the border gone for good");
    }

    #[test]
    fn a_black_frame_neither_adopts_nor_resets_a_border_being_held() {
        let mut cut = BorderCut::default();
        let frame = banjo(PixelFormat::Rgba32);
        // The first sample and five more that are the same border.
        run(&mut cut, &frame, HOLD_FRAMES / 2);
        run(&mut cut, &black(PixelFormat::Rgba32), 4 * HOLD_FRAMES);
        assert_eq!(cut.part(), WHOLE_FRAME, "black adopts nothing");
        // Elapsed time does not count: the rest of the samples are still needed.
        run(&mut cut, &frame, 6 * SAMPLE_EVERY);
        assert_eq!(cut.part(), WHOLE_FRAME, "not held in enough samples yet");
        run(&mut cut, &frame, SAMPLE_EVERY);
        assert_eq!(
            cut.part(),
            banjo_part(),
            "held in samples from before the black too"
        );
    }

    #[test]
    fn two_dim_samples_either_side_of_a_black_gap_are_not_a_held_border() {
        let mut cut = BorderCut::default();
        // A fade's dim frame, the left of it cut to the limit but not the right.
        let area = Area {
            x: 200,
            y: 23,
            w: 440,
            h: 427,
        };
        let dim = framed(PixelFormat::Rgba32, (640, 480), area, 30, 0);
        run(&mut cut, &dim, SAMPLE_EVERY);
        run(&mut cut, &black(PixelFormat::Rgba32), 4 * HOLD_FRAMES);
        run(&mut cut, &dim, SAMPLE_EVERY);
        assert_eq!(cut.part(), WHOLE_FRAME);
    }

    #[test]
    fn a_logo_in_the_middle_of_black_is_not_a_border() {
        let mut cut = BorderCut::default();
        let logo = Area {
            x: 200,
            y: 150,
            w: 240,
            h: 180,
        };
        run(
            &mut cut,
            &framed(PixelFormat::Rgba32, (640, 480), logo, 255, 0),
            10 * 60,
        );
        assert_eq!(cut.part(), WHOLE_FRAME);
        // A real border is still cut afterwards.
        run(&mut cut, &banjo(PixelFormat::Rgba32), 2 * HOLD_FRAMES);
        assert_eq!(cut.part(), banjo_part());
    }

    #[test]
    fn a_new_border_within_a_pixel_of_the_cut_keeps_the_cut() {
        let mut cut = BorderCut::default();
        run(&mut cut, &banjo(PixelFormat::Rgba32), 2 * HOLD_FRAMES);
        assert_eq!(cut.part(), banjo_part());
        // Another border for a sample, so the next is a new candidate rather than the same one.
        let whole = Area {
            x: 0,
            y: 0,
            w: 640,
            h: 480,
        };
        run(
            &mut cut,
            &framed(PixelFormat::Rgba32, (640, 480), whole, 128, 0),
            SAMPLE_EVERY,
        );
        let nudged = Area {
            x: BANJO.x + 1,
            w: BANJO.w - 1,
            ..BANJO
        };
        run(
            &mut cut,
            &framed(PixelFormat::Rgba32, (640, 480), nudged, 128, 0),
            2 * HOLD_FRAMES,
        );
        assert_eq!(cut.part(), banjo_part());
    }

    #[test]
    fn samples_a_pixel_or_two_apart_are_cut_as_their_union() {
        let mut cut = BorderCut::default();
        let b = Area {
            x: BANJO.x - 2,
            ..BANJO
        };
        let (a, b) = (
            framed(PixelFormat::Rgba32, (640, 480), BANJO, 128, 0),
            framed(PixelFormat::Rgba32, (640, 480), b, 128, 0),
        );
        for i in 0..=HOLD_FRAMES / SAMPLE_EVERY {
            run(&mut cut, if i % 2 == 0 { &a } else { &b }, SAMPLE_EVERY);
        }
        let union = Area {
            x: BANJO.x - 2,
            w: BANJO.w + 2,
            ..BANJO
        };
        assert_eq!(cut.part(), union.part((640, 480)));
    }

    #[test]
    fn a_frame_with_padded_rows_is_read_by_its_pitch() {
        let tight = banjo(PixelFormat::Xrgb8888);
        let row = 640 * 4;
        let mut data = Vec::new();
        for line in tight.data.chunks(row) {
            data.extend_from_slice(line);
            data.extend_from_slice(&[0xFF; 32]);
        }
        let padded = Frame {
            pitch: row + 32,
            data,
            ..tight
        };
        assert_eq!(content(&padded), Some(BANJO));
    }

    #[test]
    fn the_unused_byte_of_an_xrgb8888_pixel_is_not_light() {
        let mut frame = black(PixelFormat::Xrgb8888);
        for px in frame.data.chunks_mut(4) {
            px[3] = 0xFF;
        }
        assert_eq!(content(&frame), None);
        let mut frame = banjo(PixelFormat::Xrgb8888);
        for px in frame.data.chunks_mut(4) {
            px[3] = 0xFF;
        }
        assert_eq!(content(&frame), Some(BANJO));
    }

    #[test]
    fn each_session_looks_for_its_games_border_anew() {
        let mut first = BorderCut::default();
        run(&mut first, &banjo(PixelFormat::Rgba32), 2 * HOLD_FRAMES);
        assert_eq!(first.part(), banjo_part());
        // The next Game's session starts with a new one, which draws its whole frame until its
        // own border has held.
        let next = BorderCut::default();
        assert_eq!(next.part(), WHOLE_FRAME);
    }

    #[test]
    fn a_cut_follows_a_scaled_frame_and_is_dropped_for_another_shape() {
        let mut cut = BorderCut::default();
        run(&mut cut, &banjo(PixelFormat::Rgba32), 2 * HOLD_FRAMES);
        let doubled = Area {
            x: 80,
            y: 46,
            w: 1140,
            h: 854,
        };
        cut.observe(&framed(PixelFormat::Rgba32, (1280, 960), doubled, 128, 0));
        assert_eq!(cut.part(), doubled.part((1280, 960)), "scaled");
        cut.observe(&framed(PixelFormat::Rgba32, (640, 240), BANJO, 128, 0));
        assert_eq!(cut.part(), WHOLE_FRAME, "another shape");
    }

    #[test]
    fn banjo_kazooie_cut_fills_the_height_of_a_16_by_10_window_centred() {
        let picture = fit_part(4.0 / 3.0, banjo_part(), (1920.0, 1200.0));
        // 4:3 × (570 / 640) / (427 / 480): 1.3349, a hair wider than 4:3.
        let width = 1200.0 * (4.0 / 3.0) * (570.0 / 640.0) / (427.0 / 480.0);
        assert!(
            (picture.h - 1200.0).abs() < 0.01 && picture.y.abs() < 0.01,
            "{picture:?}"
        );
        assert!((picture.w - width).abs() < 0.01, "{picture:?} against {width}");
        assert!((picture.x - (1920.0 - width) / 2.0).abs() < 0.01, "{picture:?}");
        // The whole frame was 1600 wide: the cut part's 1427 shown, now 1602.
        assert!((1601.0..1603.0).contains(&picture.w));
    }
}
