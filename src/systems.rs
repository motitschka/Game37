//! The 17 Systems Game37 knows about, defined once in System Order (FR-CAR-8).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Maker {
    Nintendo,
    Sony,
}

impl Maker {
    pub fn name(self) -> &'static str {
        match self {
            Maker::Nintendo => "Nintendo",
            Maker::Sony => "Sony",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Embedded,
    External,
}

#[derive(Debug)]
pub struct System {
    /// Stable identifier, also the System's folder name under the Games Root.
    pub id: &'static str,
    pub name: &'static str,
    /// Badge label on placeholder art and search results.
    pub short: &'static str,
    pub maker: Maker,
    /// European release year.
    pub year: u16,
    pub kind: Kind,
    /// Theme colour as 0xRRGGBB.
    pub colour: u32,
    /// Recognised file extensions, lowercase, without the dot. Each matching file is one Game.
    pub extensions: &'static [&'static str],
    /// For Systems whose Games can be folders (FR-LIB-4): a subfolder is one Game when it
    /// contains every entry of at least one of these alternatives. Empty for file-only Systems.
    pub folder_markers: &'static [&'static [&'static str]],
}

impl System {
    /// The Carousel card caption, e.g. "Nintendo · 2002".
    pub fn caption(&self) -> String {
        format!("{} · {}", self.maker.name(), self.year)
    }

    pub fn has_folder_games(&self) -> bool {
        !self.folder_markers.is_empty()
    }
}

/// Every System, in System Order: Nintendo first, then Sony, each by European release year.
pub static SYSTEMS: [System; 17] = [
    System {
        id: "nes",
        name: "NES",
        short: "NES",
        maker: Maker::Nintendo,
        year: 1986,
        kind: Kind::Embedded,
        colour: 0xC62828,
        extensions: &["nes", "fds", "unf", "unif"],
        folder_markers: &[],
    },
    System {
        id: "gb",
        name: "Game Boy",
        short: "GB",
        maker: Maker::Nintendo,
        year: 1990,
        kind: Kind::Embedded,
        colour: 0x7A9A12,
        extensions: &["gb"],
        folder_markers: &[],
    },
    System {
        id: "snes",
        name: "SNES",
        short: "SNES",
        maker: Maker::Nintendo,
        year: 1992,
        kind: Kind::Embedded,
        colour: 0x6A4C9C,
        extensions: &["sfc", "smc"],
        folder_markers: &[],
    },
    System {
        id: "n64",
        name: "Nintendo 64",
        short: "N64",
        maker: Maker::Nintendo,
        year: 1997,
        kind: Kind::Embedded,
        colour: 0x1E9E57,
        extensions: &["n64", "z64", "v64"],
        folder_markers: &[],
    },
    System {
        id: "gbc",
        name: "Game Boy Color",
        short: "GBC",
        maker: Maker::Nintendo,
        year: 1998,
        kind: Kind::Embedded,
        colour: 0xD4A017,
        extensions: &["gbc"],
        folder_markers: &[],
    },
    System {
        id: "gba",
        name: "Game Boy Advance",
        short: "GBA",
        maker: Maker::Nintendo,
        year: 2001,
        kind: Kind::Embedded,
        colour: 0x4B3FA8,
        extensions: &["gba"],
        folder_markers: &[],
    },
    System {
        id: "gc",
        name: "GameCube",
        short: "GAMECUBE",
        maker: Maker::Nintendo,
        year: 2002,
        kind: Kind::External,
        colour: 0x6A3FB5,
        extensions: &["iso", "gcm", "rvz", "ciso", "gcz"],
        folder_markers: &[],
    },
    System {
        id: "nds",
        name: "Nintendo DS",
        short: "DS",
        maker: Maker::Nintendo,
        year: 2005,
        kind: Kind::Embedded,
        colour: 0x7D8898,
        extensions: &["nds"],
        folder_markers: &[],
    },
    System {
        id: "wii",
        name: "Wii",
        short: "WII",
        maker: Maker::Nintendo,
        year: 2006,
        kind: Kind::External,
        colour: 0x1F9FD6,
        extensions: &["iso", "wbfs", "rvz", "wia", "ciso", "gcz"],
        folder_markers: &[],
    },
    System {
        id: "3ds",
        name: "Nintendo 3DS",
        short: "3DS",
        maker: Maker::Nintendo,
        year: 2011,
        kind: Kind::External,
        colour: 0xC9302C,
        // .cia is an installer for the emulator's own storage, so it is not listed (FR-LIB-5).
        extensions: &["3ds", "cci", "cxi", "3dsx"],
        folder_markers: &[],
    },
    System {
        id: "wiiu",
        name: "Wii U",
        short: "WII U",
        maker: Maker::Nintendo,
        year: 2012,
        kind: Kind::External,
        colour: 0x0A8FBF,
        extensions: &["wua", "wud", "wux"],
        folder_markers: &[&["code", "content", "meta"]],
    },
    System {
        id: "switch",
        name: "Switch",
        short: "SWITCH",
        maker: Maker::Nintendo,
        year: 2017,
        kind: Kind::External,
        colour: 0xE60012,
        extensions: &["nsp", "xci", "nro"],
        folder_markers: &[],
    },
    System {
        id: "ps1",
        name: "PlayStation",
        short: "PS1",
        maker: Maker::Sony,
        year: 1995,
        kind: Kind::Embedded,
        colour: 0x8C8C96,
        // .bin tracks belong to a .cue sheet and are never Games on their own.
        extensions: &["cue", "chd", "pbp", "m3u"],
        folder_markers: &[],
    },
    System {
        id: "ps2",
        name: "PlayStation 2",
        short: "PS2",
        maker: Maker::Sony,
        year: 2000,
        kind: Kind::External,
        colour: 0x2443B8,
        extensions: &["iso", "chd", "cso", "zso"],
        folder_markers: &[],
    },
    System {
        id: "psp",
        name: "PSP",
        short: "PSP",
        maker: Maker::Sony,
        year: 2005,
        kind: Kind::Embedded,
        colour: 0x3A3F4B,
        extensions: &["iso", "cso", "pbp", "chd"],
        folder_markers: &[],
    },
    System {
        id: "ps3",
        name: "PlayStation 3",
        short: "PS3",
        maker: Maker::Sony,
        year: 2007,
        kind: Kind::External,
        colour: 0x2B2F3A,
        // A disc dump has PS3_GAME/; an extracted game has PARAM.SFO next to USRDIR/.
        extensions: &[],
        folder_markers: &[&["PS3_GAME"], &["PARAM.SFO", "USRDIR"]],
    },
    System {
        id: "ps4",
        name: "PlayStation 4",
        short: "PS4",
        maker: Maker::Sony,
        year: 2013,
        kind: Kind::External,
        colour: 0x0B3D91,
        extensions: &[],
        folder_markers: &[&["eboot.bin", "sce_sys"]],
    },
];

pub fn by_id(id: &str) -> Option<&'static System> {
    SYSTEMS.iter().find(|s| s.id == id)
}

/// Position of a System in System Order.
pub fn index_of(id: &str) -> Option<usize> {
    SYSTEMS.iter().position(|s| s.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_order_is_nintendo_then_sony_by_year() {
        assert_eq!(SYSTEMS.len(), 17);
        let first_sony = SYSTEMS.iter().position(|s| s.maker == Maker::Sony).unwrap();
        assert!(SYSTEMS[first_sony..].iter().all(|s| s.maker == Maker::Sony));
        for pair in SYSTEMS[..first_sony]
            .windows(2)
            .chain(SYSTEMS[first_sony..].windows(2))
        {
            assert!(
                pair[0].year <= pair[1].year,
                "{} before {}",
                pair[0].id,
                pair[1].id
            );
        }
        assert_eq!(SYSTEMS[0].id, "nes");
        assert_eq!(SYSTEMS[16].id, "ps4");
    }

    #[test]
    fn ids_are_unique_and_lookups_agree() {
        for (i, s) in SYSTEMS.iter().enumerate() {
            assert_eq!(index_of(s.id), Some(i));
            assert_eq!(by_id(s.id).map(|f| f.id), Some(s.id));
        }
        assert!(by_id("dreamcast").is_none());
    }

    #[test]
    fn folder_game_systems_are_ps3_ps4_wiiu() {
        let ids: Vec<_> = SYSTEMS
            .iter()
            .filter(|s| s.has_folder_games())
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, ["wiiu", "ps3", "ps4"]);
        assert!(
            SYSTEMS
                .iter()
                .all(|s| s.has_folder_games() || !s.extensions.is_empty())
        );
    }

    #[test]
    fn caption_shows_maker_and_european_year() {
        assert_eq!(by_id("gc").unwrap().caption(), "Nintendo · 2002");
    }

    #[test]
    fn nine_embedded_eight_external() {
        assert_eq!(SYSTEMS.iter().filter(|s| s.kind == Kind::Embedded).count(), 9);
        assert_eq!(SYSTEMS.iter().filter(|s| s.kind == Kind::External).count(), 8);
    }
}
