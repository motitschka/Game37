//! The System logos (FR-CAR-2, FR-CAR-3): one PNG per System id in `assets/logos/`, rendered by
//! `design/make_logos.sh` already coloured for the dark menus, built into the binary and decoded
//! on first use. Their sources and trademark note are in `design/logos/SOURCES.md`.

use std::sync::OnceLock;

use crate::artwork::Picture;

use super::Rect;
use super::gfx::{Align, Gfx};

/// Each System id with a logo and its PNG file.
const FILES: &[(&str, &[u8])] = &[
    ("nes", include_bytes!("../../assets/logos/nes.png")),
    ("gb", include_bytes!("../../assets/logos/gb.png")),
    ("snes", include_bytes!("../../assets/logos/snes.png")),
    ("n64", include_bytes!("../../assets/logos/n64.png")),
    ("gbc", include_bytes!("../../assets/logos/gbc.png")),
    ("gba", include_bytes!("../../assets/logos/gba.png")),
    ("gc", include_bytes!("../../assets/logos/gc.png")),
    ("nds", include_bytes!("../../assets/logos/nds.png")),
    ("wii", include_bytes!("../../assets/logos/wii.png")),
    ("3ds", include_bytes!("../../assets/logos/3ds.png")),
    ("wiiu", include_bytes!("../../assets/logos/wiiu.png")),
    ("switch", include_bytes!("../../assets/logos/switch.png")),
    ("switch2", include_bytes!("../../assets/logos/switch2.png")),
    ("ps1", include_bytes!("../../assets/logos/ps1.png")),
    ("ps2", include_bytes!("../../assets/logos/ps2.png")),
    ("psp", include_bytes!("../../assets/logos/psp.png")),
    ("ps3", include_bytes!("../../assets/logos/ps3.png")),
    ("ps4", include_bytes!("../../assets/logos/ps4.png")),
    ("ps5", include_bytes!("../../assets/logos/ps5.png")),
];

/// The decoded logos, by place in [`FILES`], each decoded the first time it is asked for and kept
/// for the process.
static DECODED: [OnceLock<Option<Picture>>; FILES.len()] = [const { OnceLock::new() }; FILES.len()];

/// The logo of the System `id`, or `None` when Game37 has none for it or it can't be decoded (said
/// once on stderr): the caller then draws the System's name.
pub fn logo(id: &str) -> Option<&'static Picture> {
    let i = FILES.iter().position(|&(file_id, _)| file_id == id)?;
    DECODED[i]
        .get_or_init(|| {
            let decoded = image::load_from_memory_with_format(FILES[i].1, image::ImageFormat::Png);
            match decoded {
                Ok(image) => {
                    let rgba = image.into_rgba8();
                    Some(Picture::new(rgba.width(), rgba.height(), rgba.into_raw()))
                }
                Err(e) => {
                    eprintln!("game37: can't decode the {id} logo: {e}");
                    None
                }
            }
        })
        .as_ref()
}

/// Where a `w`×`h` picture lands fitted into `bounds`: as large as fits with its shape kept, never
/// larger than its own size, centred vertically, and horizontally per `align`.
pub fn fit(w: f32, h: f32, bounds: Rect, align: Align) -> Rect {
    let scale = (bounds.w / w).min(bounds.h / h).min(1.0);
    let (fw, fh) = (w * scale, h * scale);
    let x = match align {
        Align::Left => bounds.x,
        Align::Centre => bounds.x + (bounds.w - fw) / 2.0,
        Align::Right => bounds.x + bounds.w - fw,
    };
    Rect::new(x, bounds.y + (bounds.h - fh) / 2.0, fw, fh)
}

/// Where a `w`×`h` logo lands drawn at about `area` px² whatever its shape (FR-CAR-2): scaled so
/// its area is `area`, but no wider or taller than `bounds` and never larger than its own size, then
/// placed in `bounds` as [`fit`] does.
pub fn fit_area(w: f32, h: f32, area: f32, bounds: Rect, align: Align) -> Rect {
    let scale = (area / (w * h)).sqrt().min(1.0);
    fit(w * scale, h * scale, bounds, align)
}

/// Whether a `w`×`h` logo is square enough (Switch's, PlayStation's) to need the System's name
/// beside it (FR-CAR-3).
pub fn is_square(w: f32, h: f32) -> bool {
    w / h < 1.6
}

/// Draws the logo of the System `id` at about `area` px² inside `bounds` (see [`fit_area`]) at the
/// layer's opacity, and returns where. Returns `None`, drawing nothing, when there is no logo to
/// draw or it couldn't be drawn: the caller then draws the System's name.
pub fn draw(gfx: &mut Gfx<'_>, id: &str, area: f32, bounds: Rect, align: Align) -> Option<Rect> {
    let mut at = None;
    drawn(logo(id), |picture| {
        let r = fit_area(picture.width as f32, picture.height as f32, area, bounds, align);
        at = Some(r);
        gfx.draw_builtin_picture(picture, r)
    })
    .then_some(at)
    .flatten()
}

/// Whether a logo was drawn: `draw_logo` draws `logo`, if there is one, and says whether it could.
fn drawn(logo: Option<&Picture>, draw_logo: impl FnOnce(&Picture) -> bool) -> bool {
    logo.is_some_and(draw_logo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::SYSTEMS;

    #[test]
    fn every_system_has_a_logo_that_decodes() {
        for system in &SYSTEMS {
            let picture = logo(system.id).unwrap_or_else(|| panic!("no logo for {}", system.id));
            assert!(
                picture.width > 0 && picture.height > 0,
                "{} logo is empty",
                system.id
            );
            assert_eq!(
                picture.rgba.len(),
                picture.width as usize * picture.height as usize * 4
            );
            // Made inside 800×340, twice the largest box the menus draw it in.
            assert!(
                picture.width <= 800 && picture.height <= 340,
                "{}: {picture:?}",
                system.id
            );
        }
    }

    #[test]
    fn an_unknown_system_has_no_logo() {
        assert!(logo("dreamcast").is_none());
    }

    /// A logo whose texture can't be made counts as not drawn, so the caller draws the name.
    #[test]
    fn a_logo_that_could_not_be_drawn_falls_back_to_the_name() {
        let snes = logo("snes");
        assert!(drawn(snes, |_| true), "drawn");
        assert!(!drawn(snes, |_| false), "the texture couldn't be made");
        assert!(!drawn(None, |_| panic!("no logo to draw")), "no logo");
    }

    #[test]
    fn fit_keeps_the_shape_and_stays_inside_the_box() {
        let bounds = Rect::new(100.0, 50.0, 390.0, 170.0);
        // Wider than the box: fills its width.
        let r = fit(800.0, 200.0, bounds, Align::Centre);
        assert_eq!((r.w, r.h), (390.0, 97.5));
        // Taller than the box: fills its height.
        let r = fit(340.0, 340.0, bounds, Align::Centre);
        assert_eq!((r.w, r.h), (170.0, 170.0));
        for (w, h) in [(800.0, 200.0), (340.0, 340.0), (800.0, 340.0), (120.0, 900.0)] {
            let r = fit(w, h, bounds, Align::Centre);
            assert!((r.w / r.h - w / h).abs() < 1e-4, "{w}×{h} changed shape: {r:?}");
            assert!(
                r.x >= bounds.x && r.x + r.w <= bounds.x + bounds.w + 1e-3,
                "{r:?}"
            );
            assert!(
                r.y >= bounds.y && r.y + r.h <= bounds.y + bounds.h + 1e-3,
                "{r:?}"
            );
        }
    }

    /// A long thin logo and a square one come out at about the same area, inside the caps, and
    /// neither is drawn larger than its own size.
    #[test]
    fn fit_area_draws_logos_of_any_shape_at_about_the_same_area() {
        let bounds = Rect::new(126.0, 135.0, 440.0, 96.0);
        let area = 6000.0;
        for (w, h) in [(800.0, 100.0), (340.0, 340.0)] {
            let r = fit_area(w, h, area, bounds, Align::Left);
            let drawn = r.w * r.h;
            assert!((drawn / area - 1.0).abs() <= 0.15, "{w}×{h}: {drawn} px²");
            assert!((r.w / r.h - w / h).abs() < 1e-4, "{w}×{h} changed shape: {r:?}");
            assert!(r.w <= bounds.w + 1e-3 && r.h <= bounds.h + 1e-3, "{w}×{h}: {r:?}");
            assert_eq!(r.x, bounds.x, "aligned left");
            assert!(
                (r.y + r.h / 2.0 - (bounds.y + bounds.h / 2.0)).abs() < 1e-3,
                "centred"
            );
        }
        // A large area is held to the box: the long one by its width, the square one by its height.
        let r = fit_area(800.0, 100.0, 60_000.0, bounds, Align::Left);
        assert_eq!((r.w, r.h), (440.0, 55.0));
        let r = fit_area(340.0, 340.0, 60_000.0, bounds, Align::Left);
        assert_eq!((r.w, r.h), (96.0, 96.0));
        // A small logo is never enlarged past its own size.
        let r = fit_area(80.0, 40.0, 22_000.0, bounds, Align::Left);
        assert_eq!((r.w, r.h), (80.0, 40.0));
    }

    #[test]
    fn a_logo_narrower_than_1_6_times_its_height_is_square() {
        assert!(is_square(340.0, 340.0), "Switch");
        assert!(is_square(448.0, 340.0), "PlayStation");
        assert!(!is_square(800.0, 340.0), "Wii");
        assert!(!is_square(800.0, 95.0), "Nintendo 3DS");
        for system in &SYSTEMS {
            let picture = logo(system.id).unwrap();
            let square = is_square(picture.width as f32, picture.height as f32);
            assert_eq!(
                square,
                ["switch", "switch2", "ps1"].contains(&system.id),
                "{}",
                system.id
            );
        }
    }

    #[test]
    fn fit_never_enlarges_a_picture_past_its_own_size() {
        let r = fit(100.0, 40.0, Rect::new(0.0, 0.0, 420.0, 64.0), Align::Left);
        assert_eq!((r.w, r.h), (100.0, 40.0));
    }

    #[test]
    fn fit_centres_vertically_and_aligns_horizontally() {
        let bounds = Rect::new(126.0, 151.0, 420.0, 64.0);
        let left = fit(800.0, 200.0, bounds, Align::Left);
        assert_eq!(left, Rect::new(126.0, 151.0, 256.0, 64.0));
        let small = fit(100.0, 40.0, bounds, Align::Left);
        assert_eq!(small, Rect::new(126.0, 163.0, 100.0, 40.0));
        let centred = fit(100.0, 40.0, bounds, Align::Centre);
        assert_eq!(centred, Rect::new(286.0, 163.0, 100.0, 40.0));
        // Centred on the box's centre both ways.
        let r = fit(800.0, 340.0, Rect::new(765.0, 465.0, 390.0, 170.0), Align::Centre);
        assert!((r.x + r.w / 2.0 - 960.0).abs() < 1e-3 && (r.y + r.h / 2.0 - 550.0).abs() < 1e-3);
    }
}
