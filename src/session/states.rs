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

/// What the player can delete of a Game's save data (FR-DET-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveData {
    ResumeState,
    /// Every Save State slot, with its screenshot.
    SaveStates,
    /// The In-game Save, and the Resume State with it.
    InGameSave,
    Everything,
}

impl SaveData {
    /// The files deleting it removes for the Game whose states are at `states` and whose In-game
    /// Save is `srm`, in the order they go. The Resume State goes before the In-game Save, so a
    /// failure in between never leaves one over a new save file.
    pub fn paths(self, states: &StatePaths, srm: &Path) -> Vec<PathBuf> {
        let slots = || (1..=SLOTS).flat_map(|n| [states.slot(n), states.thumb(n)]);
        match self {
            SaveData::ResumeState => vec![states.resume()],
            SaveData::SaveStates => slots().collect(),
            SaveData::InGameSave => vec![
                // A Resume State holds the old save memory: resuming would bring the old progress
                // back and write it over the new save file.
                states.resume(),
                srm.to_path_buf(),
            ],
            SaveData::Everything => [states.resume(), srm.to_path_buf()]
                .into_iter()
                .chain(slots())
                .collect(),
        }
    }
}

/// Deletes `what` for good: exactly the files [`SaveData::paths`] names, no folder and nothing
/// else. A file that isn't there is fine. Stops at the first file that can't be deleted, and
/// returns the error, naming it.
pub fn delete(what: SaveData, states: &StatePaths, srm: &Path) -> Result<(), String> {
    for path in what.paths(states, srm) {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                eprintln!("game37: can't delete {}: {e}", path.display());
                return Err(format!("{} couldn't be deleted: {e}.", path.display()));
            }
        }
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

/// How many calendar days before `now` `then` was: 0 the same day, 1 the day before.
pub fn days_ago(then: Local, now: Local) -> i64 {
    days(now.year, now.month, now.day) - days(then.year, then.month, then.day)
}

/// When a slot was saved, relative to `now`: "Today, 21:14", "Yesterday, 18:02", else
/// "19.10.2026, 09:30".
pub fn when_label(then: Local, now: Local) -> String {
    let time = format!("{:02}:{:02}", then.hour, then.minute);
    match days_ago(then, now) {
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

    /// Every save file of `p` and its In-game Save `srm`, with a tiny dummy in each.
    fn fill(p: &StatePaths, srm: &Path) -> Vec<PathBuf> {
        let mut all = vec![p.resume(), srm.to_path_buf()];
        all.extend((1..=SLOTS).flat_map(|n| [p.slot(n), p.thumb(n)]));
        for path in &all {
            saves::write(path, b"x").unwrap();
        }
        all
    }

    #[test]
    fn each_delete_removes_exactly_its_files_and_no_other_games() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let game = |rel: &str| {
            let rel = Path::new(rel);
            (
                StatePaths::new(root, "nds", rel),
                saves::srm_path(root, "nds", rel),
            )
        };
        let (p, srm) = game("nds/Mario.nds");
        let every = fill(&p, &srm);
        let cases = [
            (SaveData::ResumeState, vec![p.resume()]),
            (
                SaveData::SaveStates,
                (1..=SLOTS).flat_map(|n| [p.slot(n), p.thumb(n)]).collect(),
            ),
            (SaveData::InGameSave, vec![p.resume(), srm.clone()]),
            (SaveData::Everything, every),
        ];
        for (what, gone) in cases {
            let mine = fill(&p, &srm);
            // Another Game with the same stem in a subfolder, and one with another stem beside it.
            let others: Vec<_> = ["nds/hacks/Mario.nds", "nds/Mario Kart.nds"]
                .into_iter()
                .flat_map(|rel| {
                    let (p, srm) = game(rel);
                    fill(&p, &srm)
                })
                .collect();
            delete(what, &p, &srm).unwrap();
            for path in &mine {
                assert_eq!(
                    path.exists(),
                    !gone.contains(path),
                    "{what:?}: {}",
                    path.display()
                );
            }
            for path in &others {
                assert!(path.exists(), "{what:?} deleted {}", path.display());
            }
            assert!(root.join("states/nds").is_dir() && root.join("saves/nds").is_dir());
            delete(what, &p, &srm).expect("deleting what is already gone is fine");
        }
    }

    #[test]
    fn deleting_the_in_game_save_deletes_the_resume_state_too() {
        let dir = tempfile::tempdir().unwrap();
        let rel = Path::new("nds/Mario.nds");
        let p = StatePaths::new(dir.path(), "nds", rel);
        let srm = saves::srm_path(dir.path(), "nds", rel);
        fill(&p, &srm);
        delete(SaveData::InGameSave, &p, &srm).unwrap();
        assert!(!srm.exists());
        assert!(!p.resume().exists(), "resuming would bring the old save back");
        assert!(p.slot(1).exists(), "the Save States stay");
    }

    #[test]
    fn a_file_that_cant_be_deleted_is_an_error_naming_it() {
        let dir = tempfile::tempdir().unwrap();
        let rel = Path::new("snes/T.sfc");
        let p = StatePaths::new(dir.path(), "snes", rel);
        let srm = saves::srm_path(dir.path(), "snes", rel);
        // A folder where the Resume State would be: `remove_file` refuses it.
        fs::create_dir_all(p.resume()).unwrap();
        saves::write(&srm, b"x").unwrap();
        let e = delete(SaveData::InGameSave, &p, &srm).unwrap_err();
        assert!(e.contains("T.resume.state couldn't be deleted"), "{e}");
        assert!(srm.exists(), "the In-game Save stays while the Resume State does");
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
