//! The dark theme (FR-UI-4): the colours `C` of `design/generate_mockups.py`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colour {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Colour {
    /// An opaque colour from 0xRRGGBB.
    pub const fn hex(rgb: u32) -> Colour {
        Colour {
            r: (rgb >> 16) as u8,
            g: (rgb >> 8) as u8,
            b: rgb as u8,
            a: 255,
        }
    }

    /// The same colour with its alpha multiplied by `opacity` (0 to 1).
    pub fn alpha(self, opacity: f32) -> Colour {
        Colour {
            a: (self.a as f32 * opacity.clamp(0.0, 1.0)).round() as u8,
            ..self
        }
    }
}

pub const BG: Colour = Colour::hex(0x0B0D12);
pub const SURFACE: Colour = Colour::hex(0x151922);
pub const SURFACE2: Colour = Colour::hex(0x1F2431);
pub const LINE: Colour = Colour::hex(0x2A3142);
pub const TEXT: Colour = Colour::hex(0xF2F4F8);
pub const MUTED: Colour = Colour::hex(0x8A93A6);
pub const DIM: Colour = Colour::hex(0x5B6477);
pub const ACCENT: Colour = Colour::hex(0x8B7BFF);
pub const STAR: Colour = Colour::hex(0xFFC84A);
