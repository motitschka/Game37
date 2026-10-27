//! The Standalone Emulators that play External Systems, and which of them are installed.
//!
//! Each External System has exactly one emulator, installed as a Flatpak on Linux (ADR 0007) and
//! found in its usual install places or chosen by the player on Windows (ADR 0015, FR-WIN-4).
//! Standalone emulators for Embedded Systems are never used (ADR 0008).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::systems::{Kind, System};

#[derive(Debug, PartialEq, Eq)]
pub struct Emulator {
    pub name: &'static str,
    /// Its key in the Config's `[emulators]` table, which holds the `.exe` chosen on Windows.
    pub key: &'static str,
    pub flatpak_id: &'static str,
    /// Its executable's file names on Windows, matched case-insensitively. Facts in
    /// `docs/research/w2-windows-emulators.md`.
    pub exe_names: &'static [&'static str],
    /// How a player gets it on Windows (FR-WIN-5).
    pub windows_install: WindowsInstall,
    /// Folders its Windows installer installs into, under `%ProgramFiles%` (or
    /// `%LOCALAPPDATA%\Programs`); empty for a portable-only emulator.
    pub install_folders: &'static [&'static str],
    /// Arguments after `flatpak run … <id>` that start a Game fullscreen (FR-EXT-1), with
    /// [`PATH`] standing for the Game's file. Facts in `docs/research/external-emulators.md`.
    pub args: &'static [&'static str],
    /// For folder Games: the file inside the folder to pass, as path components matched
    /// case-insensitively, the first alternative that exists winning. A `*.ext` component is the
    /// first file in name order with that extension. Empty when the System has no folder Games.
    pub folder_file: &'static [&'static [&'static str]],
    /// Whether the emulator stays open in its game list after a Game stops, so the player must
    /// close its window for Game37 to come back.
    pub stays_open: bool,
}

/// Stands for the Game's file in [`Emulator::args`].
pub const PATH: &str = "{path}";

pub static DOLPHIN: Emulator = Emulator {
    name: "Dolphin",
    key: "dolphin",
    flatpak_id: "org.DolphinEmu.dolphin-emu",
    exe_names: &["Dolphin.exe"],
    windows_install: WindowsInstall::Page("https://dolphin-emu.org/download/"),
    install_folders: &["Dolphin"],
    args: &["-b", "-C", "Dolphin.Display.Fullscreen=True", "-e", PATH],
    folder_file: &[],
    stays_open: false,
};
pub static CEMU: Emulator = Emulator {
    name: "Cemu",
    key: "cemu",
    flatpak_id: "info.cemu.Cemu",
    exe_names: &["Cemu.exe"],
    windows_install: WindowsInstall::Winget("Cemu.Cemu"),
    install_folders: &[],
    args: &["-f", "-g", PATH],
    folder_file: &[&["code", "*.rpx"]],
    stays_open: false,
};
pub static AZAHAR: Emulator = Emulator {
    name: "Azahar",
    key: "azahar",
    flatpak_id: "org.azahar_emu.Azahar",
    exe_names: &["azahar.exe"],
    windows_install: WindowsInstall::Winget("AzaharEmu.Azahar"),
    install_folders: &["Azahar"],
    args: &["-f", PATH],
    folder_file: &[],
    stays_open: true,
};
pub static RYUBING: Emulator = Emulator {
    name: "Ryubing",
    key: "ryubing",
    flatpak_id: "io.github.ryubing.Ryujinx",
    exe_names: &["Ryujinx.exe"],
    windows_install: WindowsInstall::Page("https://ryujinx.app/"),
    install_folders: &[],
    args: &[PATH, "--fullscreen"],
    folder_file: &[],
    stays_open: true,
};
pub static PCSX2: Emulator = Emulator {
    name: "PCSX2",
    key: "pcsx2",
    flatpak_id: "net.pcsx2.PCSX2",
    exe_names: &["pcsx2-qt.exe"],
    windows_install: WindowsInstall::Winget("PCSX2Team.PCSX2"),
    install_folders: &["PCSX2"],
    args: &["-batch", "-fullscreen", PATH],
    folder_file: &[],
    stays_open: false,
};
pub static RPCS3: Emulator = Emulator {
    name: "RPCS3",
    key: "rpcs3",
    flatpak_id: "net.rpcs3.RPCS3",
    exe_names: &["rpcs3.exe"],
    windows_install: WindowsInstall::Page("https://rpcs3.net/download"),
    install_folders: &[],
    args: &["--no-gui", "--fullscreen", PATH],
    folder_file: &[&["PS3_GAME", "USRDIR", "EBOOT.BIN"], &["USRDIR", "EBOOT.BIN"]],
    stays_open: false,
};
pub static SHADPS4: Emulator = Emulator {
    name: "shadPS4",
    key: "shadps4",
    flatpak_id: "net.shadps4.shadPS4",
    exe_names: &["shadPS4.exe"],
    windows_install: WindowsInstall::Winget("shadps4-emu.shadPS4"),
    install_folders: &[],
    args: &["-g", PATH, "--fullscreen", "true"],
    folder_file: &[&["eboot.bin"]],
    stays_open: true,
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

/// How a player gets an emulator on Windows (FR-WIN-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsInstall {
    /// Its current winget package id.
    Winget(&'static str),
    /// Its download page, for an emulator winget has no current package of.
    Page(&'static str),
}

/// What Game37 shows a player to get a missing emulator (FR-CAR-9, FR-WIN-5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallHint {
    /// A command to run: `flatpak install` on Linux, `winget install` on Windows.
    Command(String),
    /// A download page to open, on Windows, for an emulator without a winget package.
    Page(&'static str),
}

impl InstallHint {
    pub fn text(&self) -> &str {
        match self {
            InstallHint::Command(command) => command,
            InstallHint::Page(url) => url,
        }
    }
}

impl Emulator {
    /// What gets it on this OS: the `flatpak install` command on Linux; on Windows the
    /// `winget install` command, or its download page where it has no winget package.
    pub fn install_hint(&self) -> InstallHint {
        if cfg!(windows) {
            self.windows_install_hint()
        } else {
            InstallHint::Command(self.flatpak_install_command())
        }
    }

    /// [`Emulator::install_hint`]'s text, for the screens that show one line (FR-CAR-9).
    pub fn install_command(&self) -> String {
        self.install_hint().text().to_owned()
    }

    fn flatpak_install_command(&self) -> String {
        format!("flatpak install flathub {}", self.flatpak_id)
    }

    fn windows_install_hint(&self) -> InstallHint {
        match self.windows_install {
            WindowsInstall::Winget(id) => InstallHint::Command(winget_install_command(id)),
            WindowsInstall::Page(url) => InstallHint::Page(url),
        }
    }

    fn winget_id(&self) -> Option<&'static str> {
        match self.windows_install {
            WindowsInstall::Winget(id) => Some(id),
            WindowsInstall::Page(_) => None,
        }
    }
}

fn winget_install_command(id: &str) -> String {
    format!("winget install -e --id {id}")
}

/// One command that installs all of `emulators`, each once, for First-run Setup: one
/// `flatpak install` on Linux; on Windows their `winget install` commands joined with `; `, as
/// PowerShell runs them. Emulators without a winget package aren't in it: their
/// [`Emulator::install_hint`] is a download page. `None` when nothing is left to install.
pub fn install_all(emulators: &[&Emulator]) -> Option<String> {
    if cfg!(windows) {
        winget_install_all(emulators)
    } else {
        flatpak_install_all(emulators)
    }
}

fn flatpak_install_all(emulators: &[&Emulator]) -> Option<String> {
    let ids = unique(emulators.iter().map(|e| e.flatpak_id));
    (!ids.is_empty()).then(|| format!("flatpak install flathub {}", ids.join(" ")))
}

fn winget_install_all(emulators: &[&Emulator]) -> Option<String> {
    let ids = unique(emulators.iter().filter_map(|e| e.winget_id()));
    let commands: Vec<String> = ids.into_iter().map(winget_install_command).collect();
    (!commands.is_empty()).then(|| commands.join("; "))
}

fn unique<'a>(items: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen = Vec::new();
    for item in items {
        if !seen.contains(&item) {
            seen.push(item);
        }
    }
    seen
}

/// Every Standalone Emulator once, in System Order.
fn all() -> Vec<&'static Emulator> {
    let mut all: Vec<&'static Emulator> = Vec::new();
    for emulator in crate::systems::SYSTEMS.iter().filter_map(for_system) {
        if !all.contains(&emulator) {
            all.push(emulator);
        }
    }
    all
}

/// How an installed emulator was located on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// The player chose its `.exe`, and the Config keeps that choice.
    Chosen,
    /// Game37 found it in its usual install places.
    Found,
}

/// Where an installed emulator's executable is on Windows, and how Game37 knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub exe: PathBuf,
    pub how: How,
}

/// Which Standalone Emulators are installed, as last checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emulators {
    /// The installed emulators' keys, or `None` to treat every emulator as installed.
    installed: Option<HashSet<&'static str>>,
    /// Where each installed emulator is, on Windows; empty on Linux, where Flatpak runs them.
    locations: HashMap<&'static str, Location>,
}

impl Emulators {
    /// Treats every emulator as installed: for tests, and for screens shown without a Config.
    pub fn all_installed() -> Emulators {
        Emulators {
            installed: None,
            locations: HashMap::new(),
        }
    }

    /// The emulators with these Flatpak app ids count as installed; other ids are ignored.
    pub fn from_installed<I: IntoIterator<Item = S>, S: Into<String>>(ids: I) -> Emulators {
        let ids: HashSet<String> = ids.into_iter().map(Into::into).collect();
        Emulators {
            installed: Some(
                all()
                    .iter()
                    .filter(|e| ids.contains(e.flatpak_id))
                    .map(|e| e.key)
                    .collect(),
            ),
            locations: HashMap::new(),
        }
    }

    /// The emulators at `locations` (by key) count as installed, and no others.
    pub fn located(locations: HashMap<&'static str, Location>) -> Emulators {
        Emulators {
            installed: Some(locations.keys().copied().collect()),
            locations,
        }
    }

    /// Asks Flatpak which apps are installed (about 20 ms). If Flatpak itself can't be run, every
    /// emulator counts as missing, so the notices tell the player what to install. The Config at
    /// `_config_path` only matters on Windows.
    #[cfg(unix)]
    pub fn detect(_config_path: &Path) -> Emulators {
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

    /// Looks for each emulator (FR-WIN-4): the `.exe` chosen in the Config at `config_path`, then
    /// the installers' registry entries, the usual folders and winget's. A few registry reads and
    /// folder listings, so it is cheap enough for every start and every Check again.
    #[cfg(windows)]
    pub fn detect(config_path: &Path) -> Emulators {
        let chosen = crate::config::load_emulator_exes(config_path).unwrap_or_else(|e| {
            eprintln!("game37: {e}");
            HashMap::new()
        });
        Emulators::locate_all(&chosen, &Places::here())
    }

    /// Locates every emulator with [`locate`]: `chosen` holds the Config's `.exe` paths by key.
    pub fn locate_all(chosen: &HashMap<String, PathBuf>, places: &Places) -> Emulators {
        let locations = all()
            .into_iter()
            .filter_map(|e| Some((e.key, locate(e, chosen.get(e.key).map(PathBuf::as_path), places)?)))
            .collect();
        Emulators::located(locations)
    }

    /// The System's emulator if it is an External System whose emulator isn't installed.
    pub fn missing(&self, system: &System) -> Option<&'static Emulator> {
        if system.kind != Kind::External {
            return None;
        }
        let emulator = for_system(system)?;
        match &self.installed {
            Some(installed) if !installed.contains(emulator.key) => Some(emulator),
            _ => None,
        }
    }

    /// Where `emulator` is and how it was located, on Windows (Settings → Emulators); `None` when
    /// it is missing, and always on Linux.
    pub fn location(&self, emulator: &Emulator) -> Option<&Location> {
        self.locations.get(emulator.key)
    }
}

/// What Windows detection looks at besides the files themselves: the program folders from the
/// environment and the installers' Uninstall entries from the registry. [`Places::here`] reads the
/// real ones on Windows; tests fill it in with temporary folders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Places {
    /// `%ProgramFiles%`, `%ProgramW6432%` and `%ProgramFiles(x86)%`, those that are set.
    pub program_files: Vec<PathBuf>,
    /// `%LOCALAPPDATA%`, which holds `Programs\` and winget's `Microsoft\WinGet\`.
    pub local_app_data: Option<PathBuf>,
    /// Every Uninstall entry, from HKLM and HKCU, in both the 64- and 32-bit registry views.
    pub uninstall: Vec<Uninstall>,
}

/// An installer's Uninstall entry: the values that say where it installed to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Uninstall {
    pub install_location: Option<String>,
    /// An icon path, often the executable itself, maybe quoted and followed by `,<index>`.
    pub display_icon: Option<String>,
}

#[cfg(windows)]
impl Places {
    /// The real environment and registry.
    pub fn here() -> Places {
        let env = |name| {
            std::env::var_os(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let mut program_files: Vec<PathBuf> = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
            .into_iter()
            .filter_map(env)
            .collect();
        program_files.dedup();
        Places {
            program_files,
            local_app_data: env("LOCALAPPDATA"),
            uninstall: registry::uninstall_entries(),
        }
    }
}

/// Where `emulator` is on Windows, first hit winning, each only if its file is there: the `chosen`
/// `.exe`; an Uninstall entry's folder; an installer's usual folder; winget's package folder, then
/// its link.
pub fn locate(emulator: &Emulator, chosen: Option<&Path>, places: &Places) -> Option<Location> {
    if let Some(exe) = chosen.filter(|p| p.is_file()) {
        return Some(Location {
            exe: exe.to_owned(),
            how: How::Chosen,
        });
    }
    let names = emulator.exe_names;
    let from_registry = || {
        places.uninstall.iter().find_map(|entry| {
            let icon_dir = entry
                .display_icon
                .as_deref()
                .map(icon_path)
                .and_then(|p| p.parent().map(Path::to_owned));
            let dirs = [entry.install_location.as_deref().map(PathBuf::from), icon_dir];
            dirs.into_iter().flatten().find_map(|dir| exe_in(&dir, names))
        })
    };
    let local = places.local_app_data.as_deref();
    let from_folders = || {
        let roots = places
            .program_files
            .iter()
            .cloned()
            .chain(local.map(|l| l.join("Programs")));
        roots
            .flat_map(|root| emulator.install_folders.iter().map(move |f| root.join(f)))
            .find_map(|dir| exe_in(&dir, names))
    };
    let winget = local.map(|l| l.join("Microsoft").join("WinGet"));
    let from_winget_package = || {
        let packages = winget.as_ref()?.join("Packages");
        winget_package(&packages, emulator.winget_id()?, names)
    };
    let from_winget_link = || {
        let links = winget.as_ref()?.join("Links");
        let link = exe_in(&links, names)?;
        // A link to a portable package's exe: the exe's own folder is where it runs.
        let target = fs::read_link(&link)
            .ok()
            .map(|t| links.join(t))
            .filter(|t| t.is_file());
        Some(target.unwrap_or(link))
    };
    let exe = from_registry()
        .or_else(from_folders)
        .or_else(from_winget_package)
        .or_else(from_winget_link)?;
    Some(Location { exe, how: How::Found })
}

/// A `DisplayIcon` value's path: unquoted, without a trailing `,<index>`.
fn icon_path(value: &str) -> PathBuf {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix('"') {
        return PathBuf::from(rest.split('"').next().unwrap_or(rest));
    }
    match value.rsplit_once(',') {
        Some((path, index)) if index.trim().parse::<i32>().is_ok() => PathBuf::from(path),
        _ => PathBuf::from(value),
    }
}

/// The file in `dir` named one of `names`, compared case-insensitively.
fn exe_in(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .find(|e| {
            let name = e.file_name();
            names
                .iter()
                .any(|n| name.to_str().is_some_and(|s| s.eq_ignore_ascii_case(n)))
        })
        .map(|e| e.path())
        .filter(|p| p.is_file())
}

/// The exe in winget's package folder for `id` (`<id>_<source hash>\`), in it or up to two folders
/// down (Cemu's zip holds a `Cemu_<version>\` folder).
fn winget_package(packages: &Path, id: &str, names: &[&str]) -> Option<PathBuf> {
    let prefix = format!("{id}_").to_lowercase();
    sorted_dirs(packages)
        .into_iter()
        .filter(|d| {
            let name = d.file_name().map(|n| n.to_string_lossy().to_lowercase());
            name.is_some_and(|n| n.starts_with(&prefix))
        })
        .find_map(|dir| search(&dir, names, 2))
}

fn search(dir: &Path, names: &[&str], depth: u32) -> Option<PathBuf> {
    exe_in(dir, names).or_else(|| {
        (depth > 0)
            .then(|| {
                sorted_dirs(dir)
                    .into_iter()
                    .find_map(|d| search(&d, names, depth - 1))
            })
            .flatten()
    })
}

fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// The installers' Uninstall entries, read with the registry API.
#[cfg(windows)]
mod registry {
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
        REG_SAM_FLAGS, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
    };

    use super::Uninstall;

    const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

    pub fn uninstall_entries() -> Vec<Uninstall> {
        let mut entries = Vec::new();
        for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
                let Some(key) = Key::open(root, UNINSTALL, view) else {
                    continue;
                };
                for name in key.subkeys() {
                    entries.push(Uninstall {
                        install_location: key.string(&name, "InstallLocation"),
                        display_icon: key.string(&name, "DisplayIcon"),
                    });
                }
            }
        }
        entries
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    struct Key(HKEY);

    impl Key {
        fn open(root: HKEY, path: &str, view: REG_SAM_FLAGS) -> Option<Key> {
            let path = wide(path);
            let mut key = null_mut();
            // SAFETY: `path` is NUL-terminated and `key` is a valid place for the handle.
            let status = unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, KEY_READ | view, &mut key) };
            (status == ERROR_SUCCESS).then_some(Key(key))
        }

        /// The subkeys' names, each NUL-terminated.
        fn subkeys(&self) -> Vec<Vec<u16>> {
            let mut names = Vec::new();
            // A key name has at most 255 characters.
            let mut buf = [0u16; 256];
            for index in 0.. {
                let mut len = buf.len() as u32;
                // SAFETY: `buf` holds `len` characters; the optional out parameters are null.
                let status = unsafe {
                    RegEnumKeyExW(
                        self.0,
                        index,
                        buf.as_mut_ptr(),
                        &mut len,
                        null(),
                        null_mut(),
                        null_mut(),
                        null_mut(),
                    )
                };
                if status != ERROR_SUCCESS {
                    break;
                }
                names.push(buf[..len as usize].iter().copied().chain([0]).collect());
            }
            names
        }

        /// The string value `value` of the subkey `subkey` (NUL-terminated), environment
        /// variables expanded; `None` when it is missing, empty or not a string.
        fn string(&self, subkey: &[u16], value: &str) -> Option<String> {
            let value = wide(value);
            let mut buf = vec![0u16; 260];
            for _ in 0..4 {
                let mut bytes = (buf.len() * 2) as u32;
                // SAFETY: both names are NUL-terminated and `buf` holds `bytes` bytes.
                let status = unsafe {
                    RegGetValueW(
                        self.0,
                        subkey.as_ptr(),
                        value.as_ptr(),
                        RRF_RT_REG_SZ,
                        null_mut(),
                        buf.as_mut_ptr().cast(),
                        &mut bytes,
                    )
                };
                match status {
                    ERROR_SUCCESS => {
                        buf.truncate(bytes as usize / 2);
                        while buf.last() == Some(&0) {
                            buf.pop();
                        }
                        return Some(String::from_utf16_lossy(&buf)).filter(|s| !s.is_empty());
                    }
                    ERROR_MORE_DATA => buf.resize((bytes as usize).div_ceil(2) + 1, 0),
                    _ => return None,
                }
            }
            None
        }
    }

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the handle came from `RegOpenKeyExW` and is closed only here.
            unsafe { RegCloseKey(self.0) };
        }
    }
}

/// App ids from `flatpak list --app --columns=application`, one per line.
#[cfg_attr(windows, allow(dead_code))] // Only the unix `detect` asks Flatpak.
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
    #[cfg(unix)] // The Flatpak command is Linux's; emulators.rs pins the Windows hints.
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

    fn touch(path: &Path) -> PathBuf {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::File::create(path).unwrap();
        path.to_owned()
    }

    /// PCSX2 in all four kinds of place under `dir`, and the `Places` that point at them.
    fn pcsx2_everywhere(dir: &Path) -> (PathBuf, Places, [PathBuf; 4]) {
        let chosen = touch(&dir.join("Mine/pcsx2-qt.exe"));
        let registry = touch(&dir.join("Emus/PCSX2 Nightly/pcsx2-qt.exe"));
        let folder = touch(&dir.join("Program Files/PCSX2/pcsx2-qt.exe"));
        let local = dir.join("Local");
        let winget = touch(&local.join("Microsoft/WinGet/Packages/PCSX2Team.PCSX2_8wekyb3d/pcsx2-qt.exe"));
        let places = Places {
            program_files: vec![dir.join("Program Files")],
            local_app_data: Some(local),
            uninstall: vec![
                Uninstall::default(),
                Uninstall {
                    install_location: None,
                    display_icon: Some(format!("\"{}\",0", registry.display())),
                },
            ],
        };
        (chosen.clone(), places, [chosen, registry, folder, winget])
    }

    #[test]
    fn the_chosen_exe_beats_the_registry_which_beats_folders_which_beat_winget() {
        let dir = tempfile::tempdir().unwrap();
        let (chosen, places, [_, registry, folder, winget]) = pcsx2_everywhere(dir.path());
        let at = |exe: &Path, how| {
            Some(Location {
                exe: exe.to_owned(),
                how,
            })
        };
        assert_eq!(locate(&PCSX2, Some(&chosen), &places), at(&chosen, How::Chosen));
        assert_eq!(locate(&PCSX2, None, &places), at(&registry, How::Found));
        fs::remove_file(&registry).unwrap();
        assert_eq!(locate(&PCSX2, None, &places), at(&folder, How::Found));
        fs::remove_file(&folder).unwrap();
        assert_eq!(locate(&PCSX2, None, &places), at(&winget, How::Found));
        fs::remove_file(&winget).unwrap();
        assert_eq!(locate(&PCSX2, None, &places), None);
    }

    #[test]
    fn a_chosen_exe_that_is_gone_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let (chosen, places, [_, registry, ..]) = pcsx2_everywhere(dir.path());
        fs::remove_file(&chosen).unwrap();
        let found = locate(&PCSX2, Some(&chosen), &places).unwrap();
        assert_eq!((found.exe, found.how), (registry, How::Found));
    }

    #[test]
    fn an_uninstall_entry_s_install_location_counts_and_an_unquoted_icon_index_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let exe = touch(&dir.path().join("Azahar/azahar.exe"));
        let entry = |install_location: Option<&Path>, display_icon: Option<String>| Places {
            uninstall: vec![Uninstall {
                install_location: install_location.map(|p| p.display().to_string()),
                display_icon,
            }],
            ..Places::default()
        };
        let by_location = entry(Some(&dir.path().join("Azahar")), None);
        assert_eq!(
            locate(&AZAHAR, None, &by_location).map(|l| l.exe),
            Some(exe.clone())
        );
        let by_icon = entry(None, Some(format!("{},0", exe.display())));
        assert_eq!(locate(&AZAHAR, None, &by_icon).map(|l| l.exe), Some(exe));
    }

    #[test]
    fn exe_names_match_whatever_their_case() {
        let dir = tempfile::tempdir().unwrap();
        let exe = touch(&dir.path().join("Program Files/Azahar/Azahar.EXE"));
        let places = Places {
            program_files: vec![dir.path().join("Program Files")],
            ..Places::default()
        };
        assert_eq!(locate(&AZAHAR, None, &places).map(|l| l.exe), Some(exe));
    }

    #[test]
    fn winget_s_package_folder_is_searched_two_folders_down_for_its_own_id_only() {
        let dir = tempfile::tempdir().unwrap();
        let packages = dir.path().join("Microsoft/WinGet/Packages");
        touch(&packages.join("Cemu.CemuBeta_abc/Cemu.exe"));
        let exe = touch(&packages.join("Cemu.Cemu_8wekyb3d/Cemu_2.6/Cemu.exe"));
        let places = Places {
            local_app_data: Some(dir.path().to_owned()),
            ..Places::default()
        };
        assert_eq!(locate(&CEMU, None, &places).map(|l| l.exe), Some(exe.clone()));
        fs::remove_file(&exe).unwrap();
        touch(&packages.join("Cemu.Cemu_8wekyb3d/a/b/c/Cemu.exe"));
        assert_eq!(
            locate(&CEMU, None, &places),
            None,
            "three folders down is too deep"
        );
    }

    #[test]
    fn winget_s_link_is_the_last_place() {
        let dir = tempfile::tempdir().unwrap();
        let link = touch(&dir.path().join("Microsoft/WinGet/Links/shadPS4.exe"));
        let places = Places {
            local_app_data: Some(dir.path().to_owned()),
            ..Places::default()
        };
        assert_eq!(locate(&SHADPS4, None, &places).map(|l| l.exe), Some(link));
    }

    #[test]
    fn locate_all_reports_where_and_how_and_the_rest_as_missing() {
        let dir = tempfile::tempdir().unwrap();
        let dolphin = touch(&dir.path().join("x/Dolphin.exe"));
        let rpcs3 = touch(&dir.path().join("Program Files/RPCS3/rpcs3.exe"));
        let places = Places {
            program_files: vec![dir.path().join("Program Files")],
            ..Places::default()
        };
        let chosen = HashMap::from([("dolphin".to_owned(), dolphin.clone())]);
        let emulators = Emulators::locate_all(&chosen, &places);
        assert_eq!(emulators.missing(by_id("gc").unwrap()), None);
        assert_eq!(emulators.missing(by_id("wii").unwrap()), None);
        assert_eq!(
            emulators.location(&DOLPHIN),
            Some(&Location {
                exe: dolphin,
                how: How::Chosen
            })
        );
        assert!(rpcs3.is_file());
        assert_eq!(
            emulators.missing(by_id("ps3").unwrap()),
            Some(&RPCS3),
            "RPCS3 has no installer folder"
        );
        assert_eq!(emulators.location(&RPCS3), None);
    }

    #[test]
    fn every_emulator_has_its_own_key_and_an_exe_name() {
        let all = all();
        assert_eq!(all.len(), 7);
        for e in &all {
            assert!(!e.exe_names.is_empty(), "{}", e.name);
            assert_eq!(all.iter().filter(|o| o.key == e.key).count(), 1, "{}", e.key);
        }
    }

    #[test]
    fn windows_hints_are_winget_commands_or_download_pages() {
        assert_eq!(
            CEMU.windows_install_hint(),
            InstallHint::Command("winget install -e --id Cemu.Cemu".into())
        );
        assert_eq!(
            DOLPHIN.windows_install_hint(),
            InstallHint::Page("https://dolphin-emu.org/download/")
        );
        assert_eq!(
            RYUBING.windows_install_hint(),
            InstallHint::Page("https://ryujinx.app/")
        );
        assert_eq!(
            RPCS3.windows_install_hint(),
            InstallHint::Page("https://rpcs3.net/download")
        );
    }

    #[test]
    fn setup_s_windows_line_joins_the_winget_commands_for_powershell() {
        assert_eq!(
            winget_install_all(&[&DOLPHIN, &PCSX2, &SHADPS4, &PCSX2]).as_deref(),
            Some("winget install -e --id PCSX2Team.PCSX2; winget install -e --id shadps4-emu.shadPS4")
        );
        assert_eq!(winget_install_all(&[&DOLPHIN, &RPCS3]), None);
        assert_eq!(
            flatpak_install_all(&[&DOLPHIN, &PCSX2, &DOLPHIN]).as_deref(),
            Some("flatpak install flathub org.DolphinEmu.dolphin-emu net.pcsx2.PCSX2")
        );
    }

    #[test]
    fn the_hint_is_this_os_s() {
        let (hint, all) = (PCSX2.install_hint(), install_all(&[&PCSX2]));
        if cfg!(windows) {
            assert_eq!(hint, PCSX2.windows_install_hint());
            assert_eq!(all, winget_install_all(&[&PCSX2]));
        } else {
            assert_eq!(
                hint,
                InstallHint::Command("flatpak install flathub net.pcsx2.PCSX2".into())
            );
            assert_eq!(all, flatpak_install_all(&[&PCSX2]));
        }
    }

    #[test]
    #[cfg(windows)]
    fn detect_reads_the_chosen_exe_from_the_config() {
        let dir = tempfile::tempdir().unwrap();
        let exe = touch(&dir.path().join("Cemu/Cemu.exe"));
        let config = dir.path().join("config.toml");
        crate::config::save_emulator_exe(&config, "cemu", Some(&exe)).unwrap();
        let emulators = Emulators::detect(&config);
        assert_eq!(
            emulators.location(&CEMU),
            Some(&Location {
                exe,
                how: How::Chosen
            })
        );
    }
}
