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

    /// The colour `t` of the way from `self` to `other`, with `t` clamped to 0 to 1.
    pub fn mix(self, other: Colour, t: f32) -> Colour {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Colour {
            r: lerp(self.r, other.r),
            g: lerp(self.g, other.g),
            b: lerp(self.b, other.b),
            a: lerp(self.a, other.a),
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
/// Missing-emulator warnings (FR-CAR-9).
pub const WARN: Colour = Colour::hex(0xFFB020);
/// What is ready, such as an installed emulator: the mockups' `C["ahead"]`.
pub const SUCCESS: Colour = Colour::hex(0x3DDC84);
/// Destructive actions, e.g. Reset Playtime and stats.
pub const DANGER: Colour = Colour::hex(0xFF8F98);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_goes_from_one_colour_to_the_other_and_clamps() {
        let (a, b) = (Colour::hex(0x000000), Colour::hex(0xFF8040));
        assert_eq!(a.mix(b, 0.0), a);
        assert_eq!(a.mix(b, 1.0), b);
        assert_eq!(a.mix(b, 0.5), Colour::hex(0x804020));
        assert_eq!(a.mix(b, 2.0), b);
    }
}
