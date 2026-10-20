//! Save States and Resume States: the Core's serialized machine state, kept in
//! `<Games Root>/states/<system>/`, in the Game's subfolder there (FR-LIB-9; see
//! [`saves::game_dir`]), as `<game file stem>.slot<N>.state` with a
//! `.slot<N>.png` screenshot for the three manual slots (FR-EMB-5), and as
//! `<game file stem>.resume.state` for the Resume State written on quitting (FR-EMB-6).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::saves;
use crate::libretro::{Frame, PixelFormat};

/// How many manual Save State slots each Game has.
pub const SLOTS: usize = 3;

/// Where one Game's Save States and Resume State live.
#[derive(Debug, Clone, PartialEq)]
pub struct StatePaths {
    dir: PathBuf,
    stem: String,
}

impl StatePaths {
    /// The paths for the Game at `rel_path` of `system`.
    pub fn new(games_root: &Path, system: &str, rel_path: &Path) -> StatePaths {
        let stem = rel_path.file_stem().unwrap_or(rel_path.as_os_str());
        StatePaths {
            dir: saves::game_dir(&games_root.join("states").join(system), system, rel_path),
            stem: stem.to_string_lossy().into_owned(),
        }
    }

    /// Slot `n` (1 to 3)'s Save State.
    pub fn slot(&self, n: usize) -> PathBuf {
        self.dir.join(format!("{}.slot{n}.state", self.stem))
    }

    /// Slot `n` (1 to 3)'s screenshot.
    pub fn thumb(&self, n: usize) -> PathBuf {
        self.dir.join(format!("{}.slot{n}.png", self.stem))
    }

    pub fn resume(&self) -> PathBuf {
        self.dir.join(format!("{}.resume.state", self.stem))
    }
}

/// Writes slot `n`'s Save State `state` to `state_path` and its screenshot `png` to `thumb_path`
/// so that the two always match: the screenshot goes to a temporary file first, then the state
/// is written, then the screenshot takes its place. When there is no screenshot (`png` failed or
/// can't be written), the state is saved anyway and the old screenshot removed, so the slot shows
/// its time without one. `Err` only when the state couldn't be written; the slot is then unchanged.
pub fn write_slot(
    state_path: &Path,
    state: &[u8],
    thumb_path: &Path,
    png: io::Result<Vec<u8>>,
) -> io::Result<()> {
    let tmp = saves::tmp_path(thumb_path);
    let png = png.and_then(|png| {
        if let Some(dir) = tmp.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&tmp, png)
    });
    if let Err(e) = saves::write(state_path, state) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = png.and_then(|()| fs::rename(&tmp, thumb_path)) {
        eprintln!("game37: no screenshot for {}: {e}", state_path.display());
        let _ = fs::remove_file(&tmp);
        let _ = fs::remove_file(thumb_path);
    }
    Ok(())
}

/// An RGB image, 3 bytes per pixel, rows packed.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgb {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Expands a 5- or 6-bit channel to 8 bits.
fn expand(v: u32, bits: u32) -> u8 {
    let max = (1 << bits) - 1;
    ((v * 255 + max / 2) / max) as u8
}

/// A Core's video frame as RGB.
pub fn frame_rgb(frame: &Frame) -> Rgb {
    let bpp = frame.format.bytes_per_pixel();
    let mut data = Vec::with_capacity(frame.width as usize * frame.height as usize * 3);
    for row in frame.data.chunks(frame.pitch).take(frame.height as usize) {
        for px in row[..frame.width as usize * bpp].chunks(bpp) {
            let rgb = match frame.format {
                PixelFormat::Xrgb8888 => [px[2], px[1], px[0]],
                PixelFormat::Rgba32 => [px[0], px[1], px[2]],
                PixelFormat::Rgb565 => {
                    let v = u32::from(u16::from_le_bytes([px[0], px[1]]));
                    [expand(v >> 11, 5), expand(v >> 5 & 0x3F, 6), expand(v & 0x1F, 5)]
                }
                PixelFormat::Xrgb1555 => {
                    let v = u32::from(u16::from_le_bytes([px[0], px[1]]));
                    [
                        expand(v >> 10 & 0x1F, 5),
                        expand(v >> 5 & 0x1F, 5),
                        expand(v & 0x1F, 5),
                    ]
                }
            };
            data.extend_from_slice(&rgb);
        }
    }
    Rgb {
        width: frame.width,
        height: frame.height,
        data,
    }
}

/// Encodes `image` as a PNG.
pub fn encode_png(image: &Rgb) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(io::Error::other)?;
    writer.write_image_data(&image.data).map_err(io::Error::other)?;
    writer.finish().map_err(io::Error::other)?;
    Ok(out)
}

/// Reads an 8-bit RGB or RGBA PNG as RGB.
pub fn read_png(path: &Path) -> io::Result<Rgb> {
    let decoder = png::Decoder::new(io::BufReader::new(fs::File::open(path)?));
    let mut reader = decoder.read_info().map_err(io::Error::other)?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf).map_err(io::Error::other)?;
    let bytes = &buf[..info.buffer_size()];
    let data = match (info.color_type, info.bit_depth) {
        (png::ColorType::Rgb, png::BitDepth::Eight) => bytes.to_vec(),
        (png::ColorType::Rgba, png::BitDepth::Eight) => {
            bytes.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect()
        }
        other => return Err(io::Error::other(format!("unsupported PNG {other:?}"))),
    };
    Ok(Rgb {
        width: info.width,
        height: info.height,
        data,
    })
}

/// A local date and time, as `SDL_TimeToDateTime` gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Local {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's `days_from_civil`).
fn days(year: i32, month: u32, day: u32) -> i64 {
    let y = i64::from(if month <= 2 { year - 1 } else { year });
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// When a slot was saved, relative to `now`: "Today, 21:14", "Yesterday, 18:02", else
/// "19.10.2026, 09:30".
pub fn when_label(then: Local, now: Local) -> String {
    let time = format!("{:02}:{:02}", then.hour, then.minute);
    match days(now.year, now.month, now.day) - days(then.year, then.month, then.day) {
        0 => format!("Today, {time}"),
        1 => format!("Yesterday, {time}"),
        _ => format!("{:02}.{:02}.{}, {time}", then.day, then.month, then.year),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_are_named_after_the_game_file_in_its_systems_states_folder() {
        let p = StatePaths::new(
            Path::new("/g"),
            "snes",
            Path::new("snes/Super Metroid (Europe).sfc"),
        );
        assert_eq!(
            p.slot(2),
            Path::new("/g/states/snes/Super Metroid (Europe).slot2.state")
        );
        assert_eq!(
            p.thumb(3),
            Path::new("/g/states/snes/Super Metroid (Europe).slot3.png")
        );
        assert_eq!(
            p.resume(),
            Path::new("/g/states/snes/Super Metroid (Europe).resume.state")
        );
    }

    #[test]
    fn games_with_the_same_stem_in_different_subfolders_keep_separate_states() {
        let top = StatePaths::new(Path::new("/g"), "snes", Path::new("snes/Super Mario World.sfc"));
        let hack = StatePaths::new(
            Path::new("/g"),
            "snes",
            Path::new("snes/hacks/Super Mario World.sfc"),
        );
        assert_eq!(
            top.resume(),
            Path::new("/g/states/snes/Super Mario World.resume.state")
        );
        assert_eq!(
            hack.slot(1),
            Path::new("/g/states/snes/hacks/Super Mario World.slot1.state")
        );
        assert_eq!(
            hack.thumb(1),
            Path::new("/g/states/snes/hacks/Super Mario World.slot1.png")
        );
        assert_eq!(
            hack.resume(),
            Path::new("/g/states/snes/hacks/Super Mario World.resume.state")
        );
    }

    #[test]
    fn a_slot_is_saved_with_its_screenshot_or_without_one_but_never_half() {
        let dir = tempfile::tempdir().unwrap();
        let p = StatePaths::new(dir.path(), "snes", Path::new("snes/hacks/T.sfc"));
        let (state, thumb) = (p.slot(1), p.thumb(1));
        write_slot(&state, b"s1", &thumb, Ok(b"p1".to_vec())).unwrap();
        assert_eq!(fs::read(&state).unwrap(), b"s1");
        assert_eq!(fs::read(&thumb).unwrap(), b"p1");
        assert!(!saves::tmp_path(&thumb).exists());

        write_slot(&state, b"s2", &thumb, Err(io::Error::other("no PNG"))).unwrap();
        assert_eq!(fs::read(&state).unwrap(), b"s2", "the state is saved anyway");
        assert!(!thumb.exists(), "without the old screenshot");

        write_slot(&state, b"s3", &thumb, Ok(b"p3".to_vec())).unwrap();
        fs::remove_file(&state).unwrap();
        fs::create_dir(&state).unwrap();
        assert!(write_slot(&state, b"s4", &thumb, Ok(b"p4".to_vec())).is_err());
        assert_eq!(
            fs::read(&thumb).unwrap(),
            b"p3",
            "a failed state keeps the old screenshot"
        );
        assert!(!saves::tmp_path(&thumb).exists());
    }

    #[test]
    fn timestamps_say_today_yesterday_or_the_date() {
        let at = |year, month, day, hour, minute| Local {
            year,
            month,
            day,
            hour,
            minute,
        };
        let now = at(2026, 3, 1, 10, 0);
        assert_eq!(when_label(at(2026, 3, 1, 9, 5), now), "Today, 09:05");
        assert_eq!(when_label(at(2026, 2, 28, 21, 14), now), "Yesterday, 21:14");
        assert_eq!(when_label(at(2026, 2, 27, 18, 2), now), "27.02.2026, 18:02");
        let new_year = at(2027, 1, 1, 0, 1);
        assert_eq!(when_label(at(2026, 12, 31, 23, 59), new_year), "Yesterday, 23:59");
        assert_eq!(when_label(at(2025, 12, 31, 8, 0), new_year), "31.12.2025, 08:00");
    }

    #[test]
    fn frames_convert_to_rgb_and_round_trip_through_png() {
        let frame = Frame {
            width: 2,
            height: 1,
            pitch: 6,
            format: PixelFormat::Rgb565,
            // Pure red, pure blue, then one byte of row padding.
            data: vec![0x00, 0xF8, 0x1F, 0x00, 0xAA, 0xAA],
        };
        let rgb = frame_rgb(&frame);
        assert_eq!(rgb.data, [255, 0, 0, 0, 0, 255]);
        let xrgb = Frame {
            width: 1,
            height: 1,
            pitch: 4,
            format: PixelFormat::Xrgb8888,
            data: vec![3, 2, 1, 0],
        };
        assert_eq!(frame_rgb(&xrgb).data, [1, 2, 3]);
        let green = Frame {
            format: PixelFormat::Xrgb1555,
            data: vec![0xE0, 0x03, 0, 0],
            pitch: 2,
            width: 1,
            height: 1,
        };
        assert_eq!(frame_rgb(&green).data, [0, 255, 0]);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.png");
        fs::write(&path, encode_png(&rgb).unwrap()).unwrap();
        assert_eq!(read_png(&path).unwrap(), rgb);
    }
}
