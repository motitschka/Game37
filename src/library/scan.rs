//! Finding Games in the System folders under the Games Root (FR-LIB-1, FR-LIB-4).

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use walkdir::{DirEntry, WalkDir};

use super::FoundGame;
use super::title::display_title;
use crate::systems::{self, SYSTEMS, System};

/// Every Game under the Games Root, System by System in System Order.
/// A missing Games Root has no Games.
pub fn scan_root(root: &Path) -> Vec<FoundGame> {
    SYSTEMS
        .iter()
        .flat_map(|system| scan_system(root, system))
        .collect()
}

/// The Games in one System's folder (`<root>/<id>/`), sorted by `rel_path`.
///
/// The folder is walked recursively, following symlinks, skipping hidden and unreadable entries.
/// A file with one of the System's extensions is one Game. A subfolder holding one of the System's
/// folder markers is one Game and is not looked into. In `ps1`, a `.cue` or `.chd` listed in an
/// `.m3u` is part of that `.m3u` Game rather than a Game of its own.
pub fn scan_system(root: &Path, system: &'static System) -> Vec<FoundGame> {
    let mut games = Vec::new();
    let mut in_playlist = HashSet::new();
    let mut walk = WalkDir::new(root.join(system.id))
        .follow_links(true)
        .min_depth(1)
        .into_iter()
        .filter_entry(|entry| !is_hidden(entry.file_name()));

    while let Some(entry) = walk.next() {
        // Unreadable entries and symlink cycles come back as errors and are skipped.
        let Ok(entry) = entry else { continue };
        if entry.file_type().is_dir() {
            if has_folder_markers(entry.path(), system) {
                walk.skip_current_dir();
                if let Some(rel_path) = rel_path(root, entry.path()) {
                    let title = display_title(&entry.file_name().to_string_lossy());
                    games.push((
                        entry.into_path(),
                        FoundGame {
                            system: system.id,
                            rel_path,
                            title,
                        },
                    ));
                }
            }
            continue;
        }
        let Some(ext) = extension(&entry) else { continue };
        if !system.extensions.contains(&ext.as_str()) {
            continue;
        }
        if system.id == "ps1" && ext == "m3u" {
            in_playlist.extend(playlist_entries(entry.path()));
        }
        let Some(rel_path) = rel_path(root, entry.path()) else {
            continue;
        };
        let stem = entry.path().file_stem().unwrap_or_default().to_string_lossy();
        let title = display_title(&stem);
        games.push((
            entry.into_path(),
            FoundGame {
                system: system.id,
                rel_path,
                title,
            },
        ));
    }

    let mut games: Vec<FoundGame> = games
        .into_iter()
        .filter(|(path, _)| !(is_disc_image(path) && in_playlist.contains(path)))
        .map(|(_, game)| game)
        .collect();
    games.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    games
}

/// The System whose folder an absolute path under the Games Root is in, e.g. `<root>/snes/...`.
pub fn system_for_path(root: &Path, path: &Path) -> Option<&'static System> {
    match path.strip_prefix(root).ok()?.components().next()? {
        Component::Normal(name) => systems::by_id(name.to_str()?),
        _ => None,
    }
}

fn is_hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b".")
}

/// The lowercase extension of a file entry.
fn extension(entry: &DirEntry) -> Option<String> {
    if !entry.file_type().is_file() {
        return None;
    }
    Some(entry.path().extension()?.to_str()?.to_ascii_lowercase())
}

/// Whether `dir` contains every entry of at least one of the System's folder marker alternatives.
/// Names match case-insensitively; an entry can be a file or a folder.
fn has_folder_markers(dir: &Path, system: &System) -> bool {
    if !system.has_folder_games() {
        return false;
    }
    let Ok(read) = fs::read_dir(dir) else { return false };
    let names: HashSet<String> = read
        .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().to_lowercase()))
        .collect();
    system.folder_markers.iter().any(|alternative| {
        alternative
            .iter()
            .all(|marker| names.contains(&marker.to_lowercase()))
    })
}

/// The game identity path: relative to the Games Root, `/`-separated. `None` when not valid UTF-8,
/// since a lossy path would not lead back to the file.
fn rel_path(root: &Path, path: &Path) -> Option<String> {
    let parts: Option<Vec<&str>> = path.strip_prefix(root).ok()?.iter().map(OsStr::to_str).collect();
    Some(parts?.join("/"))
}

fn is_disc_image(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase);
    matches!(ext.as_deref(), Some("cue" | "chd"))
}

/// The files an `.m3u` playlist lists, resolved against its folder. Blank lines and `#` comments
/// are ignored. An unreadable playlist lists nothing.
fn playlist_entries(m3u: &Path) -> Vec<PathBuf> {
    let Ok(bytes) = fs::read(m3u) else {
        return Vec::new();
    };
    let dir = m3u.parent().unwrap_or(Path::new(""));
    String::from_utf8_lossy(&bytes)
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| dir.join(line))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn touch(root: &Path, rel: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"").unwrap();
    }

    fn mkdir(root: &Path, rel: &str) {
        fs::create_dir_all(root.join(rel)).unwrap();
    }

    fn scan(root: &Path, id: &str) -> Vec<FoundGame> {
        scan_system(root, systems::by_id(id).unwrap())
    }

    fn rel_paths(games: &[FoundGame]) -> Vec<&str> {
        games.iter().map(|g| g.rel_path.as_str()).collect()
    }

    #[test]
    fn files_with_the_systems_extensions_are_games_in_any_case() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        touch(root, "snes/Super Mario World (USA) [!].sfc");
        touch(root, "snes/F-Zero (Europe).SMC");
        touch(root, "snes/readme.txt");
        touch(root, "snes/Archive.zip");
        touch(root, "snes/Mario Kart 64.z64");
        mkdir(root, "snes/Folder.sfc");

        let games = scan(root, "snes");
        assert_eq!(
            games,
            [
                FoundGame {
                    system: "snes",
                    rel_path: "snes/F-Zero (Europe).SMC".into(),
                    title: "F-Zero".into(),
                },
                FoundGame {
                    system: "snes",
                    rel_path: "snes/Super Mario World (USA) [!].sfc".into(),
                    title: "Super Mario World".into(),
                },
            ]
        );
    }

    #[test]
    fn every_listed_extension_is_recognised() {
        let tmp = TempDir::new().unwrap();
        for system in SYSTEMS.iter() {
            for ext in system.extensions {
                touch(tmp.path(), &format!("{}/Game.{ext}", system.id));
            }
            assert_eq!(
                scan_system(tmp.path(), system).len(),
                system.extensions.len(),
                "{}",
                system.id
            );
        }
    }

    #[test]
    fn games_in_subfolders_are_found() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "gba/RPGs/Golden Sun (Europe).gba");
        touch(tmp.path(), "gba/Advance Wars.gba");
        assert_eq!(
            rel_paths(&scan(tmp.path(), "gba")),
            ["gba/Advance Wars.gba", "gba/RPGs/Golden Sun (Europe).gba"]
        );
    }

    #[test]
    fn iso_in_gc_is_only_a_gamecube_game() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "gc/Metroid Prime (Europe).iso");
        let games = scan_root(tmp.path());
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].system, "gc");
        assert_eq!(games[0].title, "Metroid Prime");
    }

    #[test]
    fn ps3_disc_folder_is_one_game_and_its_contents_are_not_listed() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // PS3_GAME/ itself has the PARAM.SFO + USRDIR/ form, so it would be a Game if descended into.
        touch(root, "ps3/BLUS12345/PS3_GAME/PARAM.SFO");
        touch(root, "ps3/BLUS12345/PS3_GAME/USRDIR/EBOOT.BIN");
        touch(root, "ps3/BLUS12345/PS3_DISC.SFB");

        let games = scan(root, "ps3");
        assert_eq!(
            games,
            [FoundGame {
                system: "ps3",
                rel_path: "ps3/BLUS12345".into(),
                title: "BLUS12345".into()
            }]
        );
    }

    #[test]
    fn ps3_param_sfo_and_usrdir_folder_is_one_game() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        touch(root, "ps3/Demon's Souls (USA)/PARAM.SFO");
        mkdir(root, "ps3/Demon's Souls (USA)/USRDIR");
        touch(root, "ps3/lowercase/param.sfo");
        mkdir(root, "ps3/lowercase/usrdir");
        touch(root, "ps3/Incomplete/PARAM.SFO");

        let games = scan(root, "ps3");
        assert_eq!(rel_paths(&games), ["ps3/Demon's Souls (USA)", "ps3/lowercase"]);
        assert_eq!(games[0].title, "Demon's Souls");
    }

    #[test]
    fn ps4_eboot_and_sce_sys_folder_is_one_game() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "ps4/CUSA00001/eboot.bin");
        touch(tmp.path(), "ps4/CUSA00001/sce_sys/param.sfo");
        touch(tmp.path(), "ps4/CUSA00002/eboot.bin");
        assert_eq!(rel_paths(&scan(tmp.path(), "ps4")), ["ps4/CUSA00001"]);
    }

    #[test]
    fn wiiu_folder_and_wua_file_are_games() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        mkdir(root, "wiiu/Mario Kart 8 (Europe)/code");
        mkdir(root, "wiiu/Mario Kart 8 (Europe)/content");
        touch(root, "wiiu/Mario Kart 8 (Europe)/meta/meta.xml");
        touch(root, "wiiu/Mario Kart 8 (Europe)/code/app.wux");
        touch(root, "wiiu/Splatoon (Europe).wua");

        let games = scan(root, "wiiu");
        assert_eq!(
            rel_paths(&games),
            ["wiiu/Mario Kart 8 (Europe)", "wiiu/Splatoon (Europe).wua"]
        );
        assert_eq!(games[0].title, "Mario Kart 8");
        assert_eq!(games[1].title, "Splatoon");
    }

    #[test]
    fn non_matching_subfolder_is_descended_into() {
        let tmp = TempDir::new().unwrap();
        mkdir(tmp.path(), "ps3/Collection/Deeper/BLES00001/PS3_GAME");
        touch(tmp.path(), "ps3/Collection/notes.txt");
        assert_eq!(
            rel_paths(&scan(tmp.path(), "ps3")),
            ["ps3/Collection/Deeper/BLES00001"]
        );
    }

    #[test]
    fn m3u_hides_the_disc_images_it_lists() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("ps1/FF7")).unwrap();
        fs::write(
            root.join("ps1/FF7/Final Fantasy VII.m3u"),
            "\u{feff}# discs\r\nFinal Fantasy VII (Disc 1).cue\r\n\r\nFinal Fantasy VII (Disc 2).chd\r\n\
             Final Fantasy VII (Disc 3).CUE\r\n",
        )
        .unwrap();
        touch(root, "ps1/FF7/Final Fantasy VII (Disc 1).cue");
        touch(root, "ps1/FF7/Final Fantasy VII (Disc 1).bin");
        touch(root, "ps1/FF7/Final Fantasy VII (Disc 2).chd");
        // Listed with a different case, so not hidden.
        touch(root, "ps1/FF7/Final Fantasy VII (Disc 3).cue");
        // Same name in another folder: not hidden.
        touch(root, "ps1/Final Fantasy VII (Disc 1).cue");
        touch(root, "ps1/Crash Bandicoot.pbp");

        assert_eq!(
            rel_paths(&scan(root, "ps1")),
            [
                "ps1/Crash Bandicoot.pbp",
                "ps1/FF7/Final Fantasy VII (Disc 3).cue",
                "ps1/FF7/Final Fantasy VII.m3u",
                "ps1/Final Fantasy VII (Disc 1).cue",
            ]
        );
    }

    #[test]
    fn m3u_entries_resolve_against_the_m3u_folder() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        touch(root, "ps1/Discs/Game (Disc 1).cue");
        fs::write(root.join("ps1/Game.m3u"), "Discs/Game (Disc 1).cue\n").unwrap();
        assert_eq!(rel_paths(&scan(root, "ps1")), ["ps1/Game.m3u"]);
    }

    #[test]
    fn hidden_files_and_folders_are_skipped() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "nes/.Zelda.nes");
        touch(tmp.path(), "nes/.trash/Metroid.nes");
        mkdir(tmp.path(), "ps3/.cache/BLUS12345/PS3_GAME");
        touch(tmp.path(), "nes/Kirby.nes");
        assert_eq!(rel_paths(&scan(tmp.path(), "nes")), ["nes/Kirby.nes"]);
        assert!(scan(tmp.path(), "ps3").is_empty());
    }

    #[test]
    fn missing_root_or_system_folder_has_no_games() {
        let tmp = TempDir::new().unwrap();
        assert!(scan_root(&tmp.path().join("nowhere")).is_empty());
        assert!(scan(tmp.path(), "snes").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_followed_without_looping() {
        use std::os::unix::fs::symlink;
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("Games");
        touch(&root, "gb/Tetris.gb");
        touch(tmp.path(), "elsewhere/Pokemon_Red.gb");
        symlink(tmp.path().join("elsewhere"), root.join("gb/linked")).unwrap();
        symlink(root.join("gb"), root.join("gb/loop")).unwrap();

        let games = scan(&root, "gb");
        assert_eq!(rel_paths(&games), ["gb/Tetris.gb", "gb/linked/Pokemon_Red.gb"]);
        assert_eq!(games[1].title, "Pokemon Red");
    }

    #[test]
    fn scan_root_lists_systems_in_system_order() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "ps4/CUSA00001/eboot.bin");
        mkdir(tmp.path(), "ps4/CUSA00001/sce_sys");
        touch(tmp.path(), "snes/B.sfc");
        touch(tmp.path(), "nes/A.nes");
        touch(tmp.path(), "unknown/C.nes");
        let systems: Vec<_> = scan_root(tmp.path()).iter().map(|g| g.system).collect();
        assert_eq!(systems, ["nes", "snes", "ps4"]);
    }

    #[test]
    fn system_for_path_finds_the_system_folder() {
        let root = Path::new("/home/player/Games");
        let id = |p: &str| system_for_path(root, Path::new(p)).map(|s| s.id);
        assert_eq!(
            id("/home/player/Games/snes/Super Mario World (USA).sfc"),
            Some("snes")
        );
        assert_eq!(
            id("/home/player/Games/ps3/BLUS12345/PS3_GAME/USRDIR"),
            Some("ps3")
        );
        assert_eq!(id("/home/player/Games/wiiu"), Some("wiiu"));
        assert_eq!(id("/home/player/Games"), None);
        assert_eq!(id("/home/player/Games/dreamcast/Game.cdi"), None);
        assert_eq!(id("/home/player/Other/snes/Game.sfc"), None);
    }
}
