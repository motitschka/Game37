//! The Standalone Emulators that play External Systems, and which of them are installed.
//!
//! Each External System has exactly one emulator, installed as a Flatpak (ADR 0007). Standalone
//! emulators for Embedded Systems are never used (ADR 0008).

use std::collections::HashSet;

use crate::systems::{Kind, System};

#[derive(Debug, PartialEq, Eq)]
pub struct Emulator {
    pub name: &'static str,
    pub flatpak_id: &'static str,
}

pub static DOLPHIN: Emulator = Emulator {
    name: "Dolphin",
    flatpak_id: "org.DolphinEmu.dolphin-emu",
};
pub static CEMU: Emulator = Emulator {
    name: "Cemu",
    flatpak_id: "info.cemu.Cemu",
};
pub static AZAHAR: Emulator = Emulator {
    name: "Azahar",
    flatpak_id: "org.azahar_emu.Azahar",
};
pub static RYUBING: Emulator = Emulator {
    name: "Ryubing",
    flatpak_id: "io.github.ryubing.Ryujinx",
};
pub static PCSX2: Emulator = Emulator {
    name: "PCSX2",
    flatpak_id: "net.pcsx2.PCSX2",
};
pub static RPCS3: Emulator = Emulator {
    name: "RPCS3",
    flatpak_id: "net.rpcs3.RPCS3",
};
pub static SHADPS4: Emulator = Emulator {
    name: "shadPS4",
    flatpak_id: "net.shadps4.shadPS4",
};

/// The Standalone Emulator for an External System; `None` for Embedded Systems.
pub fn for_system(system: &System) -> Option<&'static Emulator> {
    match system.id {
        "gc" | "wii" => Some(&DOLPHIN),
        "wiiu" => Some(&CEMU),
        "3ds" => Some(&AZAHAR),
        "switch" => Some(&RYUBING),
        "ps2" => Some(&PCSX2),
        "ps3" => Some(&RPCS3),
        "ps4" => Some(&SHADPS4),
        _ => None,
    }
}

impl Emulator {
    /// The command a player runs to install it (FR-CAR-9).
    pub fn install_command(&self) -> String {
        format!("flatpak install flathub {}", self.flatpak_id)
    }
}

/// Which Standalone Emulators are installed, as last checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emulators {
    /// Installed Flatpak app ids, or `None` to treat every emulator as installed.
    installed: Option<HashSet<String>>,
}

impl Emulators {
    /// Treats every emulator as installed: for tests, and for screens shown without a Config.
    pub fn all_installed() -> Emulators {
        Emulators { installed: None }
    }

    pub fn from_installed<I: IntoIterator<Item = S>, S: Into<String>>(ids: I) -> Emulators {
        Emulators {
            installed: Some(ids.into_iter().map(Into::into).collect()),
        }
    }

    /// Asks Flatpak which apps are installed (about 20 ms). If Flatpak itself can't be run, every
    /// emulator counts as missing, so the notices tell the player what to install.
    pub fn detect() -> Emulators {
        let output = std::process::Command::new("flatpak")
            .args(["list", "--app", "--columns=application"])
            .output();
        match output {
            Ok(out) if out.status.success() => {
                Emulators::from_installed(parse_flatpak_list(&String::from_utf8_lossy(&out.stdout)))
            }
            Ok(out) => {
                eprintln!(
                    "game37: flatpak list failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
                Emulators::from_installed(Vec::<String>::new())
            }
            Err(e) => {
                eprintln!("game37: can't run flatpak: {e}");
                Emulators::from_installed(Vec::<String>::new())
            }
        }
    }

    /// The System's emulator if it is an External System whose emulator isn't installed.
    pub fn missing(&self, system: &System) -> Option<&'static Emulator> {
        if system.kind != Kind::External {
            return None;
        }
        let emulator = for_system(system)?;
        match &self.installed {
            Some(installed) if !installed.contains(emulator.flatpak_id) => Some(emulator),
            _ => None,
        }
    }
}

/// App ids from `flatpak list --app --columns=application`, one per line.
fn parse_flatpak_list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case("application id"))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::{SYSTEMS, by_id};

    #[test]
    fn every_external_system_has_an_emulator_and_no_embedded_one_does() {
        for system in &SYSTEMS {
            match system.kind {
                Kind::External => assert!(for_system(system).is_some(), "{}", system.id),
                Kind::Embedded => assert!(for_system(system).is_none(), "{}", system.id),
            }
        }
    }

    #[test]
    fn gamecube_and_wii_share_dolphin() {
        assert_eq!(for_system(by_id("gc").unwrap()), Some(&DOLPHIN));
        assert_eq!(for_system(by_id("wii").unwrap()), Some(&DOLPHIN));
    }

    #[test]
    fn install_command_names_the_flathub_id() {
        assert_eq!(CEMU.install_command(), "flatpak install flathub info.cemu.Cemu");
    }

    #[test]
    fn a_missing_flatpak_marks_only_its_systems() {
        let list = "org.DolphinEmu.dolphin-emu\nnet.pcsx2.PCSX2\n\ninfo.cemu.Cemu\n";
        let emulators = Emulators::from_installed(parse_flatpak_list(list));
        assert_eq!(emulators.missing(by_id("gc").unwrap()), None);
        assert_eq!(emulators.missing(by_id("wiiu").unwrap()), None);
        assert_eq!(emulators.missing(by_id("ps3").unwrap()), Some(&RPCS3));
        assert_eq!(emulators.missing(by_id("switch").unwrap()), Some(&RYUBING));
    }

    #[test]
    fn embedded_systems_are_never_missing_an_emulator() {
        let none = Emulators::from_installed(Vec::<String>::new());
        assert_eq!(none.missing(by_id("snes").unwrap()), None);
        assert_eq!(none.missing(by_id("ps1").unwrap()), None);
        assert_eq!(none.missing(by_id("ps2").unwrap()), Some(&PCSX2));
    }

    #[test]
    fn all_installed_reports_nothing_missing() {
        let all = Emulators::all_installed();
        assert!(SYSTEMS.iter().all(|s| all.missing(s).is_none()));
    }

    #[test]
    fn a_header_line_is_not_an_app_id() {
        assert_eq!(
            parse_flatpak_list("Application ID\nnet.rpcs3.RPCS3\n"),
            ["net.rpcs3.RPCS3"]
        );
    }
}
