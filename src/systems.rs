//! The 19 Systems Game37 knows about, defined once in System Order (FR-CAR-8).

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
    /// An Upcoming System: no emulator can play it yet. Game37 lists its Games, and Play explains
    /// why it can't start them.
    Upcoming,
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
pub static SYSTEMS: [System; 19] = [
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
        kind: Kind::Embedded,
        colour: 0x6A3FB5,
        // What Dolphin loads as a GameCube Game: disc images in each form it reads, and homebrew
        // (.dol, .elf; FR-NEW-4). No .m3u until Game37 can swap discs.
        extensions: &["iso", "gcm", "rvz", "ciso", "gcz", "dol", "elf"],
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
        kind: Kind::Embedded,
        colour: 0x1F9FD6,
        // What Dolphin loads as a Wii Game: disc images in each form it reads, and homebrew (.dol,
        // .elf; FR-NEW-4). No .m3u until Game37 can swap discs.
        extensions: &["iso", "wbfs", "rvz", "wia", "ciso", "gcz", "dol", "elf"],
        folder_markers: &[],
    },
    System {
        id: "3ds",
        name: "Nintendo 3DS",
        short: "3DS",
        maker: Maker::Nintendo,
        year: 2011,
        kind: Kind::Embedded,
        colour: 0xC9302C,
        // What Azahar loads as a Game: cartridge and executable dumps, their z-compressed forms,
        // their bundles (.bcci, .bcxi: one file holding the dump with its update and DLC, which
        // Azahar plays as the dump), and homebrew (.3dsx, .elf, .axf; FR-NEW-4). .cia is an
        // installer for the emulator's own storage, which Azahar can't load (FR-LIB-5).
        extensions: &[
            "3ds", "cci", "cxi", "3dsx", "app", "elf", "axf", "zcci", "zcxi", "z3dsx", "bcci", "bcxi",
        ],
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
        kind: Kind::Embedded,
        colour: 0xE60012,
        extensions: &["nsp", "xci", "nro"],
        folder_markers: &[],
    },
    System {
        id: "switch2",
        name: "Switch 2",
        short: "SWITCH 2",
        maker: Maker::Nintendo,
        year: 2025,
        kind: Kind::Upcoming,
        colour: 0xE8590C,
        extensions: &["nsp", "xci"],
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
        kind: Kind::Embedded,
        colour: 0x2443B8,
        // What LRPS2 loads, less .gz and .m3u (no disc swapping yet); .elf is homebrew (FR-NEW-4).
        extensions: &["iso", "chd", "cso", "zso", "ciso", "cue", "elf"],
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
    System {
        id: "ps5",
        name: "PlayStation 5",
        short: "PS5",
        maker: Maker::Sony,
        year: 2020,
        kind: Kind::Upcoming,
        colour: 0x00897B,
        // A dump is a folder like a PS4 Game's.
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
        assert_eq!(SYSTEMS.len(), 19);
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
        assert_eq!(SYSTEMS[18].id, "ps5");
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
    fn folder_game_systems_are_wiiu_ps3_ps4_and_ps5() {
        let ids: Vec<_> = SYSTEMS
            .iter()
            .filter(|s| s.has_folder_games())
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, ["wiiu", "ps3", "ps4", "ps5"]);
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
    fn fourteen_embedded_and_only_wii_u_ps3_and_ps4_external() {
        assert_eq!(SYSTEMS.iter().filter(|s| s.kind == Kind::Embedded).count(), 14);
        let external: Vec<_> = SYSTEMS
            .iter()
            .filter(|s| s.kind == Kind::External)
            .map(|s| s.id)
            .collect();
        assert_eq!(external, ["wiiu", "ps3", "ps4"]);
    }

    #[test]
    fn switch_2_and_ps5_are_the_upcoming_systems() {
        let upcoming: Vec<_> = SYSTEMS
            .iter()
            .filter(|s| s.kind == Kind::Upcoming)
            .map(|s| s.id)
            .collect();
        assert_eq!(upcoming, ["switch2", "ps5"]);
    }

    #[test]
    fn gamecube_and_wii_take_what_dolphin_loads_but_no_disc_lists_yet() {
        let gc = by_id("gc").unwrap();
        for ext in ["iso", "gcm", "rvz", "ciso", "gcz", "dol", "elf"] {
            assert!(gc.extensions.contains(&ext), "gc {ext}");
        }
        let wii = by_id("wii").unwrap();
        for ext in ["iso", "wbfs", "rvz", "wia", "ciso", "gcz", "dol", "elf"] {
            assert!(wii.extensions.contains(&ext), "wii {ext}");
        }
        assert!(!gc.extensions.contains(&"m3u") && !wii.extensions.contains(&"m3u"));
    }

    #[test]
    fn three_ds_takes_what_azahar_loads_as_games_but_no_installers() {
        let three_ds = by_id("3ds").unwrap();
        for ext in [
            "3ds", "cci", "cxi", "3dsx", "app", "elf", "axf", "zcci", "zcxi", "z3dsx", "bcci", "bcxi",
        ] {
            assert!(three_ds.extensions.contains(&ext), "{ext}");
        }
        assert!(!three_ds.extensions.contains(&"cia"), "Azahar can't load a .cia");
    }

    #[test]
    fn ps2_takes_lrps2_homebrew_and_discs_but_no_disc_lists_yet() {
        let ps2 = by_id("ps2").unwrap();
        for ext in ["elf", "iso", "ciso", "cue", "chd", "cso", "zso"] {
            assert!(ps2.extensions.contains(&ext), "{ext}");
        }
        assert!(!ps2.extensions.contains(&"m3u") && !ps2.extensions.contains(&"gz"));
    }
}
