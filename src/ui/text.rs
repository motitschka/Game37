//! Fonts: Adwaita Sans (an Inter derivative, like the mockups) found through fontconfig, one
//! SDL_ttf face per weight and size.
//!
//! Adwaita Sans is a variable font. SDL_ttf 3.2 has no variation API, but FreeType opens a named
//! instance (SemiBold, Bold, ...) when the face index carries it in its upper bits, and fontconfig
//! reports exactly that index for a family and weight. So each weight is opened as fontconfig's
//! (file, index) match, which also works for families shipped as one file per weight.
//!
//! Windows has no fontconfig: there the variable font comes in `fonts\` next to `game37.exe`,
//! opened at the same named-instance indexes, and Segoe UI is the fallback.

use std::collections::HashMap;
use std::ffi::CString;
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(unix)]
use std::process::Command;

use sdl3::surface::Surface;
use sdl3::sys::pixels::SDL_Color;
use sdl3::sys::properties::{
    SDL_CreateProperties, SDL_DestroyProperties, SDL_SetFloatProperty, SDL_SetNumberProperty,
    SDL_SetStringProperty,
};
use sdl3::ttf::Sdl3TtfContext;
use sdl3::ttf::sys as ttf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    Medium,
    SemiBold,
    Bold,
    ExtraBold,
}

impl Weight {
    const ALL: [Weight; 5] = [
        Weight::Regular,
        Weight::Medium,
        Weight::SemiBold,
        Weight::Bold,
        Weight::ExtraBold,
    ];

    /// The fontconfig weight name.
    #[cfg(unix)]
    fn fc_name(self) -> &'static str {
        match self {
            Weight::Regular => "regular",
            Weight::Medium => "medium",
            Weight::SemiBold => "semibold",
            Weight::Bold => "bold",
            Weight::ExtraBold => "extrabold",
        }
    }

    /// The face index of this weight's named instance in `AdwaitaSans-Regular.ttf`, as fontconfig
    /// reports it on Linux (Regular is the font's default instance).
    #[cfg(windows)]
    fn adwaita_index(self) -> i64 {
        match self {
            Weight::Regular => 0,
            Weight::Medium => 5 << 16,
            Weight::SemiBold => 6 << 16,
            Weight::Bold => 7 << 16,
            Weight::ExtraBold => 8 << 16,
        }
    }

    /// The Segoe UI file in `%WINDIR%\Fonts` nearest this weight.
    #[cfg(windows)]
    fn segoe_file(self) -> &'static str {
        match self {
            Weight::Regular => "segoeui.ttf",
            Weight::Medium | Weight::SemiBold => "seguisb.ttf",
            Weight::Bold | Weight::ExtraBold => "segoeuib.ttf",
        }
    }
}

/// A font file and face index (named instance in the upper 16 bits).
#[derive(Debug, Clone)]
struct FaceSource {
    file: PathBuf,
    index: i64,
}

/// Runs `fc-match` for `pattern`, returning the matched (family, file, index).
#[cfg(unix)]
fn fc_match(pattern: &str) -> Option<(String, FaceSource)> {
    let out = Command::new("fc-match")
        .args(["-f", "%{family[0]}\t%{file}\t%{index}", pattern])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let mut parts = text.split('\t');
    let family = parts.next()?.to_owned();
    let file = PathBuf::from(parts.next()?);
    let index = parts.next()?.trim().parse().unwrap_or(0);
    (file.is_file()).then_some((family, FaceSource { file, index }))
}

/// Finds Adwaita Sans, falling back to the system's sans-serif font, at every weight.
#[cfg(unix)]
fn find_faces() -> Result<HashMap<Weight, FaceSource>, String> {
    let family = match fc_match("Adwaita Sans") {
        Some((family, _)) if family == "Adwaita Sans" => family,
        _ => match fc_match("sans-serif") {
            Some((family, _)) => family,
            None => {
                return Err(
                    "Game37 needs a font: install Adwaita Sans (or any sans-serif font) \
                                and make sure fontconfig's fc-match is available."
                        .to_owned(),
                );
            }
        },
    };
    Weight::ALL
        .into_iter()
        .map(|w| {
            let (_, source) = fc_match(&format!("{family}:{}", w.fc_name()))
                .ok_or_else(|| format!("fc-match found no {} face of {family}", w.fc_name()))?;
            Ok((w, source))
        })
        .collect()
}

/// Finds Adwaita Sans in `fonts\` next to `game37.exe`, falling back to Segoe UI, at every weight.
#[cfg(windows)]
fn find_faces() -> Result<HashMap<Weight, FaceSource>, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Can't find game37.exe: {e}"))?;
    let windir = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
    faces_in(
        exe.parent().unwrap_or(Path::new(".")),
        &Path::new(&windir).join("Fonts"),
    )
}

/// `find_faces` for `game37.exe` in `exe_dir` and the system fonts in `system_fonts`.
#[cfg(windows)]
fn faces_in(exe_dir: &Path, system_fonts: &Path) -> Result<HashMap<Weight, FaceSource>, String> {
    let adwaita = exe_dir.join("fonts").join("AdwaitaSans-Regular.ttf");
    Weight::ALL
        .into_iter()
        .map(|w| {
            let source = if adwaita.is_file() {
                FaceSource {
                    file: adwaita.clone(),
                    index: w.adwaita_index(),
                }
            } else {
                let file = system_fonts.join(w.segoe_file());
                if !file.is_file() {
                    return Err(format!("Game37 needs a font: {} isn't there.", adwaita.display()));
                }
                FaceSource { file, index: 0 }
            };
            Ok((w, source))
        })
        .collect()
}

/// An open SDL_ttf face at one size.
pub struct Face {
    raw: *mut ttf::TTF_Font,
    ascent: i32,
}

impl Face {
    fn open(source: &FaceSource, size: f32) -> Result<Face, String> {
        let file = CString::new(source.file.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        // SAFETY: plain SDL property and SDL_ttf calls on values that outlive them; the property set
        // is destroyed once the font has been created from it.
        unsafe {
            let props = SDL_CreateProperties();
            SDL_SetStringProperty(props, ttf::TTF_PROP_FONT_CREATE_FILENAME_STRING, file.as_ptr());
            SDL_SetNumberProperty(props, ttf::TTF_PROP_FONT_CREATE_FACE_NUMBER, source.index);
            SDL_SetFloatProperty(props, ttf::TTF_PROP_FONT_CREATE_SIZE_FLOAT, size);
            let raw = ttf::TTF_OpenFontWithProperties(props);
            SDL_DestroyProperties(props);
            if raw.is_null() {
                return Err(format!(
                    "Can't open {}: {}",
                    source.file.display(),
                    sdl3::get_error()
                ));
            }
            Ok(Face {
                raw,
                ascent: ttf::TTF_GetFontAscent(raw),
            })
        }
    }

    /// Distance from the top of rendered text to its baseline.
    pub fn ascent(&self) -> i32 {
        self.ascent
    }

    /// Renders `text` in white with alpha; colour comes from the texture's colour modulation.
    pub fn render(&self, text: &str) -> Result<Surface<'static>, String> {
        // SDL_ttf reads a length of 0 as "NUL-terminated", which an empty `&str` isn't.
        if text.is_empty() {
            return Err("there is no text to render".to_owned());
        }
        let white = SDL_Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        };
        // SAFETY: `self.raw` is an open font; the pointer and length describe `text`. SDL_ttf
        // returns a new surface that the `Surface` takes ownership of.
        unsafe {
            let surface = ttf::TTF_RenderText_Blended(self.raw, text.as_ptr().cast(), text.len(), white);
            if surface.is_null() {
                return Err(sdl3::get_error().to_string());
            }
            Ok(Surface::from_ll(surface))
        }
    }

    /// Width in pixels of `text` rendered by [`Face::render`], without rendering it.
    pub fn width(&self, text: &str) -> Result<i32, String> {
        // SDL_ttf reads a length of 0 as "NUL-terminated", which an empty `&str` isn't.
        if text.is_empty() {
            return Ok(0);
        }
        let (mut w, mut h) = (0, 0);
        // SAFETY: `self.raw` is an open font; the pointer and length describe `text`, and SDL_ttf
        // only writes the two locals.
        let ok =
            unsafe { ttf::TTF_GetStringSize(self.raw, text.as_ptr().cast(), text.len(), &mut w, &mut h) };
        if ok {
            Ok(w)
        } else {
            Err(sdl3::get_error().to_string())
        }
    }
}

impl Drop for Face {
    fn drop(&mut self) {
        // SAFETY: `raw` came from TTF_OpenFontWithProperties and is closed once. `Fonts` keeps the
        // SDL_ttf context alive until every face is dropped.
        unsafe { ttf::TTF_CloseFont(self.raw) }
    }
}

/// Every face in use, opened on first use per (weight, size).
pub struct Fonts {
    sources: HashMap<Weight, FaceSource>,
    faces: HashMap<(Weight, u16), Face>,
    // Declared last so SDL_ttf shuts down after the faces close.
    _ttf: Sdl3TtfContext,
}

impl Fonts {
    pub fn new() -> Result<Fonts, String> {
        let sources = find_faces()?;
        let ttf = sdl3::ttf::init().map_err(|e| format!("Can't start SDL_ttf: {e}"))?;
        Ok(Fonts {
            sources,
            faces: HashMap::new(),
            _ttf: ttf,
        })
    }

    pub fn face(&mut self, weight: Weight, size: u16) -> Result<&Face, String> {
        if !self.faces.contains_key(&(weight, size)) {
            let face = Face::open(&self.sources[&weight], size as f32)?;
            self.faces.insert((weight, size), face);
        }
        Ok(&self.faces[&(weight, size)])
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn the_adwaita_sans_next_to_the_exe_comes_first_then_segoe_ui() {
        let exe_dir = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        assert!(faces_in(exe_dir.path(), system.path()).is_err(), "no font at all");

        for file in ["segoeui.ttf", "seguisb.ttf", "segoeuib.ttf"] {
            std::fs::write(system.path().join(file), b"").unwrap();
        }
        let faces = faces_in(exe_dir.path(), system.path()).unwrap();
        assert_eq!(faces[&Weight::Bold].file, system.path().join("segoeuib.ttf"));
        assert_eq!(faces[&Weight::Bold].index, 0);

        std::fs::create_dir(exe_dir.path().join("fonts")).unwrap();
        let adwaita = exe_dir.path().join("fonts").join("AdwaitaSans-Regular.ttf");
        std::fs::write(&adwaita, b"").unwrap();
        let faces = faces_in(exe_dir.path(), system.path()).unwrap();
        assert_eq!(faces[&Weight::Regular].file, adwaita);
        assert_eq!(faces[&Weight::Regular].index, 0);
        assert_eq!(faces[&Weight::SemiBold].index, 6 << 16);
    }
}
