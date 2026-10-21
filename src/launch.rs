//! Playing a Game of an External System: its Standalone Emulator runs as a separate process while
//! Game37 waits in the background, then records Playtime (FR-EXT-1, FR-EXT-2, FR-LIB-8).
//!
//! Screens only ask for a launch through `Ctx::launch`; `app.rs` hands the request to a
//! [`Launcher`], which spawns through an injectable [`Spawner`] so tests and screenshot mode never
//! start an emulator.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use crate::cores;
use crate::ctx::{Ctx, Play};
use crate::emulators::{self, Emulator, PATH};
use crate::library::db::GameRow;
use crate::screens::download_core::DownloadCore;
use crate::screens::home::tilde;
use crate::screens::message::Message;
use crate::screens::playing::Playing;
use crate::systems::{self, Kind, System};
use crate::ui::nav::{Nav, Transition};

/// A run shorter than this that ends with a non-zero status is a failure to start, not a play.
pub const EARLY: Duration = Duration::from_secs(3);
pub const FAILED_TITLE: &str = "Couldn't start the game";
const NOT_PLAYABLE_TITLE: &str = "Not playable yet";
const BIOS_MISSING_TITLE: &str = "PlayStation BIOS missing";

/// What Play on a Game's Details Page does (FR-DET-1): asks to play it when its System's
/// Core or Standalone Emulator is installed, downloads an Embedded System's Core first when it
/// isn't (FR-EMB-2, US-23), checks for a required BIOS (FR-EMB-12), else explains why it can't be
/// played. An Embedded Game continues from its Resume State (FR-EMB-6).
pub fn confirm_game(game: &GameRow, ctx: &mut Ctx) -> Transition {
    confirm_play(Play::resume(game.clone()), ctx)
}

/// [`confirm_game`] for `play`, which may start an Embedded Game from a Save State slot
/// (FR-DET-4): the slot goes along through the BIOS check and a first Core download.
pub fn confirm_play(play: Play, ctx: &mut Ctx) -> Transition {
    let Some(system) = systems::by_id(&play.game.system) else {
        return Transition::None;
    };
    if system.kind == Kind::Embedded
        && let Some(core) = cores::for_system(system)
    {
        if !cores::has_required_bios(&ctx.games_root, core) {
            return Transition::Push(Box::new(Message::new(
                BIOS_MISSING_TITLE,
                missing_bios_message(&ctx.games_root),
            )));
        }
        if !cores::is_installed(&ctx.data_dir, core) {
            let screen = if core.system_in_data_dir && !cores::assets_dir(&ctx.data_dir).is_dir() {
                let assets = (ctx.start_assets_download)(ctx.data_dir.clone());
                DownloadCore::new_with_assets(play, core, assets)
            } else {
                let download = (ctx.start_download)(core, ctx.data_dir.clone());
                DownloadCore::new(play, core, download)
            };
            return Transition::Push(Box::new(screen));
        }
    }
    match why_not_playable(play, ctx) {
        Some(message) => Transition::Push(Box::new(Message::new(NOT_PLAYABLE_TITLE, message))),
        None => Transition::None,
    }
}

/// FR-EMB-12's message for PS1's missing BIOS: the only Core with a `required_bios` list today,
/// so this wording is PS1-specific rather than generated from the list generically.
fn missing_bios_message(games_root: &Path) -> String {
    format!(
        "PlayStation games need a BIOS file. Put scph5501.bin (North America), scph5502.bin \
         (Europe) or scph5500.bin (Japan) in {}.",
        tilde(&games_root.join("bios"), dirs::home_dir().as_deref())
    )
}

/// Asks to play `play`'s Game (`Ctx::play` or `Ctx::launch`) and returns `None`, or returns why it
/// can't be played. Called once an Embedded Game's Core, if it has one, is already installed and
/// its required BIOS, if any, is in place.
fn why_not_playable(play: Play, ctx: &mut Ctx) -> Option<String> {
    let game = &play.game;
    let system = systems::by_id(&game.system)?;
    let message = match system.kind {
        Kind::Embedded => match cores::for_system(system) {
            None => later_update(system),
            Some(_) => {
                ctx.play = Some(play);
                return None;
            }
        },
        Kind::External => match (emulators::for_system(system), ctx.emulators.missing(system)) {
            (None, _) => later_update(system),
            (Some(_), Some(missing)) => format!(
                "{} isn't installed, so {} games can't be played yet. The install command is on the \
                 {} Console Page.",
                missing.name, system.name, system.name
            ),
            (Some(_), None) => {
                ctx.launch = Some(game.clone());
                return None;
            }
        },
    };
    Some(message)
}

fn later_update(system: &System) -> String {
    format!(
        "Playing {} games inside Game37 comes in a later update.",
        system.name
    )
}

/// The command that starts `file` in `emulator`, granting it read access to the Games Root,
/// which some emulator Flatpaks (PCSX2) can't otherwise see.
pub fn command(emulator: &Emulator, games_root: &Path, file: &Path) -> Vec<OsString> {
    let mut grant = OsString::from("--filesystem=");
    grant.push(escape_filesystem(games_root));
    grant.push(":ro");
    let mut argv = vec!["flatpak".into(), "run".into(), grant, emulator.flatpak_id.into()];
    argv.extend(emulator.args.iter().map(|&a| {
        if a == PATH {
            file.as_os_str().to_owned()
        } else {
            a.into()
        }
    }));
    argv
}

/// `path` escaped for a `--filesystem` value, which Flatpak ends at the first unescaped `:` and
/// in which `\` escapes the next character (`parse_filesystem_flags` in `flatpak-context.c`).
fn escape_filesystem(path: &Path) -> OsString {
    let mut escaped = Vec::new();
    for &b in path.as_os_str().as_bytes() {
        if b == b'\\' || b == b':' {
            escaped.push(b'\\');
        }
        escaped.push(b);
    }
    OsString::from_vec(escaped)
}

/// The file to hand `emulator` for the Game at `game_path`: the path itself for a file Game, the
/// emulator's `folder_file` for a folder Game. `Err` says what is missing.
pub fn game_file(emulator: &Emulator, game_path: &Path, home: Option<&Path>) -> Result<PathBuf, String> {
    if !game_path.is_dir() {
        return match game_path.is_file() {
            true => Ok(game_path.to_owned()),
            false => Err(format!("{} isn't there any more.", tilde(game_path, home))),
        };
    }
    emulator
        .folder_file
        .iter()
        .find_map(|components| find(game_path, components))
        .ok_or_else(|| {
            let wanted: Vec<String> = emulator.folder_file.iter().map(|c| c.join("/")).collect();
            format!("Its folder has no {}.", wanted.join(" or "))
        })
}

/// Follows `components` down from `dir`, matching names case-insensitively; a `*.ext` component
/// matches the first file with that extension in name order.
fn find(dir: &Path, components: &[&str]) -> Option<PathBuf> {
    let (first, rest) = components.split_first()?;
    let mut names: Vec<OsString> = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| Some(e.ok()?.file_name()))
        .collect();
    names.sort();
    let matches = |name: &OsString| {
        let name = name.to_string_lossy().to_lowercase();
        match first.strip_prefix('*') {
            Some(ext) => name.ends_with(&ext.to_lowercase()),
            None => name == first.to_lowercase(),
        }
    };
    let path = dir.join(names.iter().find(|n| matches(n))?);
    if rest.is_empty() {
        path.is_file().then_some(path)
    } else {
        find(&path, rest)
    }
}

/// Why a run with exit `code` (`None` when it had none) counts as a failure to start.
pub fn exit_reason(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("It stopped with exit code {code}."),
        None => "It was stopped without an exit code.".to_owned(),
    }
}

/// The message shown when `emulator` couldn't start `title`.
pub fn failure_text(emulator: &Emulator, title: &str, reason: &str, log: &str) -> String {
    format!(
        "{} couldn't start {title}.\n{reason}\nDetails are in {log}",
        emulator.name
    )
}

/// How a spawned process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exited {
    /// `None` when it had no exit code, e.g. it was killed by a signal.
    pub code: Option<i32>,
    pub at: Instant,
}

/// Starts a process; the receiver gets its exit.
pub trait Spawner {
    fn spawn(&mut self, argv: &[OsString], log: &Path) -> io::Result<Receiver<Exited>>;
}

/// Runs the real command.
pub struct Processes;

impl Spawner for Processes {
    fn spawn(&mut self, argv: &[OsString], log: &Path) -> io::Result<Receiver<Exited>> {
        spawn_process(argv, log)
    }
}

/// For screenshot mode: nothing runs, and nothing ever exits.
#[derive(Default)]
pub struct NeverExits(Vec<Sender<Exited>>);

impl Spawner for NeverExits {
    fn spawn(&mut self, _: &[OsString], _: &Path) -> io::Result<Receiver<Exited>> {
        let (tx, rx) = mpsc::channel();
        self.0.push(tx);
        Ok(rx)
    }
}

/// Spawns `argv` with stdout and stderr appended to `log` (or discarded if it can't be opened),
/// and waits for it on a thread, so the UI thread never blocks.
pub fn spawn_process(argv: &[OsString], log: &Path) -> io::Result<Receiver<Exited>> {
    let mut file = open_log(log);
    if let Some(f) = &mut file {
        let line: Vec<_> = argv.iter().map(|a| a.to_string_lossy()).collect();
        let _ = writeln!(f, "game37: {}", line.join(" "));
    }
    let stdout = file.as_ref().and_then(|f| f.try_clone().ok());
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(stdout.map_or_else(Stdio::null, Stdio::from))
        .stderr(file.map_or_else(Stdio::null, Stdio::from))
        // Its own process group, so a Ctrl+C in Game37's terminal doesn't also stop the emulator.
        .process_group(0)
        .spawn()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let code = child.wait().ok().and_then(|s| s.code());
        let _ = tx.send(Exited {
            code,
            at: Instant::now(),
        });
    });
    Ok(rx)
}

fn open_log(log: &Path) -> Option<File> {
    if let Some(dir) = log.parent() {
        fs::create_dir_all(dir).ok()?;
    }
    OpenOptions::new().create(true).append(true).open(log).ok()
}

struct Running {
    game_id: i64,
    title: String,
    emulator: &'static Emulator,
    log: PathBuf,
    started: Instant,
    exits: Receiver<Exited>,
}

/// Starts Games in their Standalone Emulators and records Playtime when they exit.
pub struct Launcher {
    spawner: Box<dyn Spawner>,
    /// `<data dir>/logs`, where each emulator's output goes.
    log_dir: PathBuf,
    home: Option<PathBuf>,
    running: Option<Running>,
    /// [`EARLY`], shortened in tests so they needn't run that long.
    early: Duration,
}

impl Launcher {
    pub fn new(spawner: Box<dyn Spawner>, log_dir: PathBuf) -> Launcher {
        Launcher {
            spawner,
            log_dir,
            home: dirs::home_dir(),
            running: None,
            early: EARLY,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// Starts `game` and pushes the Playing screen; on failure pushes a message and returns false.
    pub fn start(&mut self, game: &GameRow, nav: &mut Nav, ctx: &Ctx) -> bool {
        let Some(emulator) = systems::by_id(&game.system).and_then(emulators::for_system) else {
            return false;
        };
        let log = self.log_dir.join(format!("{}.log", emulator.flatpak_id));
        let spawned = game_file(
            emulator,
            &ctx.games_root.join(&game.rel_path),
            self.home.as_deref(),
        )
        .and_then(|file| {
            let argv = command(emulator, &ctx.games_root, &file);
            self.spawner
                .spawn(&argv, &log)
                .map_err(|e| format!("flatpak couldn't be started: {e}."))
        });
        match spawned {
            Ok(exits) => {
                nav.push(Box::new(Playing::new(&game.title, emulator)));
                self.running = Some(Running {
                    game_id: game.id,
                    title: game.title.clone(),
                    emulator,
                    log,
                    started: Instant::now(),
                    exits,
                });
                true
            }
            Err(reason) => {
                self.fail(nav, emulator, &game.title, &reason, &log);
                false
            }
        }
    }

    /// Checks whether the emulator exited. If it did: records the play (unless it failed to
    /// start), pops the Playing screen and returns the outcome: `Err` holds the failure message.
    pub fn poll(&mut self, nav: &mut Nav, ctx: &mut Ctx, now: i64) -> Option<Result<(), String>> {
        let exited = match self.running.as_ref()?.exits.try_recv() {
            Ok(exited) => exited,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Exited {
                code: None,
                at: Instant::now(),
            },
        };
        let run = self.running.take()?;
        nav.pop();
        let elapsed = exited.at.saturating_duration_since(run.started);
        if elapsed < self.early && exited.code != Some(0) {
            let reason = exit_reason(exited.code);
            return Some(Err(self.fail(nav, run.emulator, &run.title, &reason, &run.log)));
        }
        record(ctx, run.game_id, now, elapsed);
        Some(Ok(()))
    }

    /// Game37 is quitting: records Playtime up to now, leaving the emulator running.
    pub fn quit(&mut self, nav: &mut Nav, ctx: &mut Ctx, now: i64) {
        if self.poll(nav, ctx, now).is_some() {
            return;
        }
        if let Some(run) = self.running.take() {
            record(ctx, run.game_id, now, run.started.elapsed());
        }
    }

    /// Notes the failure in the emulator's log and pushes a message saying so; returns its text.
    fn fail(&self, nav: &mut Nav, emulator: &Emulator, title: &str, reason: &str, log: &Path) -> String {
        if let Some(mut f) = open_log(log) {
            let _ = writeln!(f, "game37: {reason}");
        }
        let text = failure_text(emulator, title, reason, &tilde(log, self.home.as_deref()));
        nav.push(Box::new(Message::new(FAILED_TITLE, text.clone())));
        text
    }
}

/// Records a play of `elapsed` that ended at `ended_at`, and reloads the views.
pub fn record(ctx: &mut Ctx, game_id: i64, ended_at: i64, elapsed: Duration) {
    let secs = elapsed.as_secs_f64().round() as u64;
    if let Err(e) = ctx.library.record_play(game_id, ended_at, secs) {
        eprintln!("game37: can't record Playtime: {e}");
    }
    ctx.revision += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulators::{AZAHAR, CEMU, DOLPHIN, PCSX2, RPCS3, RYUBING, SHADPS4};
    use crate::library::FoundGame;
    use crate::library::db::SortOrder;
    use crate::screens::{Home, Stub};
    use crate::ui::input::{Action, Input, Pointer};
    use crate::ui::nav::Hits;

    fn argv(emulator: &Emulator, file: &str) -> Vec<String> {
        command(emulator, Path::new("/home/ann/Games"), Path::new(file))
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect()
    }

    fn expect(id: &str, args: &[&str]) -> Vec<String> {
        ["flatpak", "run", "--filesystem=/home/ann/Games:ro", id]
            .iter()
            .chain(args)
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn each_emulator_gets_its_fullscreen_command() {
        let g = "/home/ann/Games/x/My Game.iso";
        let cases: [(&Emulator, &[&str]); 7] = [
            (
                &DOLPHIN,
                &["-b", "-C", "Dolphin.Display.Fullscreen=True", "-e", g],
            ),
            (&CEMU, &["-f", "-g", g]),
            (&AZAHAR, &["-f", g]),
            (&RYUBING, &[g, "--fullscreen"]),
            (&PCSX2, &["-batch", "-fullscreen", g]),
            (&RPCS3, &["--no-gui", "--fullscreen", g]),
            (&SHADPS4, &["-g", g, "--fullscreen", "true"]),
        ];
        for (emulator, args) in cases {
            assert_eq!(
                argv(emulator, g),
                expect(emulator.flatpak_id, args),
                "{}",
                emulator.name
            );
        }
    }

    #[test]
    fn the_games_root_grant_escapes_colons_and_backslashes() {
        let root = Path::new("/mnt/a:b\\c/Games");
        let argv = command(&PCSX2, root, Path::new("/x.iso"));
        assert_eq!(argv[2], OsString::from("--filesystem=/mnt/a\\:b\\\\c/Games:ro"));
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        File::create(path).unwrap();
    }

    #[test]
    fn a_wii_u_folder_passes_its_first_rpx_and_a_wua_file_passes_itself() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("Zelda");
        touch(&game.join("Code/b.RPX"));
        touch(&game.join("Code/a.rpx"));
        touch(&game.join("Code/0.txt"));
        assert_eq!(game_file(&CEMU, &game, None), Ok(game.join("Code/a.rpx")));
        let wua = dir.path().join("Mario.wua");
        touch(&wua);
        assert_eq!(game_file(&CEMU, &wua, None), Ok(wua));
    }

    #[test]
    fn a_ps3_folder_passes_the_disc_eboot_else_the_extracted_one() {
        let dir = tempfile::tempdir().unwrap();
        let disc = dir.path().join("Disc");
        touch(&disc.join("ps3_game/USRDIR/eboot.bin"));
        touch(&disc.join("USRDIR/EBOOT.BIN"));
        assert_eq!(
            game_file(&RPCS3, &disc, None),
            Ok(disc.join("ps3_game/USRDIR/eboot.bin"))
        );
        let extracted = dir.path().join("Extracted");
        touch(&extracted.join("USRDIR/EBOOT.BIN"));
        touch(&extracted.join("PARAM.SFO"));
        assert_eq!(
            game_file(&RPCS3, &extracted, None),
            Ok(extracted.join("USRDIR/EBOOT.BIN"))
        );
    }

    #[test]
    fn a_ps4_folder_passes_its_eboot() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("Game/EBOOT.BIN"));
        touch(&dir.path().join("Game/sce_sys/param.sfo"));
        assert_eq!(
            game_file(&SHADPS4, &dir.path().join("Game"), None),
            Ok(dir.path().join("Game/EBOOT.BIN"))
        );
    }

    #[test]
    fn a_missing_file_says_what_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("Game/USRDIR/other.bin"));
        assert_eq!(
            game_file(&RPCS3, &dir.path().join("Game"), None),
            Err("Its folder has no PS3_GAME/USRDIR/EBOOT.BIN or USRDIR/EBOOT.BIN.".to_owned())
        );
        let gone = dir.path().join("gone.iso");
        assert_eq!(
            game_file(&PCSX2, &gone, Some(dir.path())),
            Err("~/gone.iso isn't there any more.".to_owned())
        );
    }

    #[test]
    fn failure_text_names_the_emulator_the_game_the_reason_and_the_log() {
        assert_eq!(
            failure_text(&RPCS3, "Demon's Souls", &exit_reason(Some(3)), "~/.local/x.log"),
            "RPCS3 couldn't start Demon's Souls.\nIt stopped with exit code 3.\n\
             Details are in ~/.local/x.log"
        );
        assert_eq!(exit_reason(None), "It was stopped without an exit code.");
    }

    /// Runs `sh -c <script>` in place of the emulator.
    struct Shell(&'static str);

    impl Spawner for Shell {
        fn spawn(&mut self, _: &[OsString], log: &Path) -> io::Result<Receiver<Exited>> {
            spawn_process(&["sh".into(), "-c".into(), self.0.into()], log)
        }
    }

    struct World {
        _dir: tempfile::TempDir,
        ctx: Ctx,
        nav: Nav,
        launcher: Launcher,
        game: GameRow,
    }

    fn world(script: &'static str) -> World {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = Ctx::empty();
        ctx.games_root = dir.path().join("Games");
        touch(&ctx.games_root.join("gc/Zelda.iso"));
        let found = FoundGame {
            system: "gc",
            rel_path: "gc/Zelda.iso".into(),
            title: "Zelda".into(),
        };
        ctx.library.sync_system("gc", &[found], 0).unwrap();
        let game = ctx.library.system_games("gc", SortOrder::AZ).unwrap().remove(0);
        let nav = Nav::with_tabs(vec![
            ("Home", Box::new(Home::new())),
            ("Consoles", Box::new(Stub::new("Consoles"))),
        ]);
        let mut launcher = Launcher::new(Box::new(Shell(script)), dir.path().join("data/logs"));
        launcher.home = None;
        World {
            _dir: dir,
            ctx,
            nav,
            launcher,
            game,
        }
    }

    impl World {
        fn start(&mut self) -> bool {
            self.launcher.start(&self.game, &mut self.nav, &self.ctx)
        }

        fn wait(&mut self) -> Result<(), String> {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if let Some(outcome) = self.launcher.poll(&mut self.nav, &mut self.ctx, 1000) {
                    return outcome;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!("the fake emulator never exited");
        }

        fn row(&self) -> GameRow {
            self.ctx
                .library
                .system_games("gc", SortOrder::AZ)
                .unwrap()
                .remove(0)
        }
    }

    /// Kills a fake emulator that prints its pid first (`echo $$; exec ...`) when the test ends,
    /// so a long-running one doesn't outlive it. Declare it after the `World`, so it drops first.
    struct KillOnDrop(PathBuf);

    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                let log = fs::read_to_string(&self.0).unwrap_or_default();
                if let Some(pid) = log.lines().find_map(|l| l.trim().parse::<u32>().ok()) {
                    let _ = Command::new("kill").arg(pid.to_string()).status();
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    fn kill_on_drop(w: &World) -> KillOnDrop {
        KillOnDrop(w._dir.path().join("data/logs/org.DolphinEmu.dolphin-emu.log"))
    }

    #[test]
    fn a_one_second_run_records_a_second_of_playtime_and_reloads_the_views() {
        let mut w = world("sleep 1");
        assert!(w.start());
        assert_eq!(w.nav.top().title(), "Playing");
        assert_eq!(w.wait(), Ok(()));
        let row = w.row();
        assert!((1..=3).contains(&row.playtime_secs), "{}", row.playtime_secs);
        assert_eq!(row.last_played, Some(1000));
        assert_eq!(w.ctx.revision, 1);
        assert_eq!(w.nav.depth(), 1, "the Playing screen is gone");
        assert!(!w.launcher.is_running());
    }

    #[test]
    fn an_early_failure_records_nothing_and_explains_itself() {
        let mut w = world("echo oops >&2; exit 3");
        assert!(w.start());
        let text = w.wait().unwrap_err();
        let log = w._dir.path().join("data/logs/org.DolphinEmu.dolphin-emu.log");
        assert_eq!(
            text,
            format!(
                "Dolphin couldn't start Zelda.\nIt stopped with exit code 3.\nDetails are in {}",
                log.display()
            )
        );
        let row = w.row();
        assert_eq!((row.playtime_secs, row.last_played), (0, None));
        assert_eq!(w.ctx.revision, 0);
        assert_eq!(w.nav.top().title(), "Couldn't start the game");
        assert_eq!(w.nav.depth(), 2, "the message replaced the Playing screen");
        let logged = fs::read_to_string(log).unwrap();
        assert!(
            logged.contains("oops") && logged.contains("exit code 3"),
            "{logged}"
        );
    }

    #[test]
    fn a_non_zero_exit_after_the_early_window_still_records_playtime() {
        let mut w = world("sleep 1; exit 3");
        w.launcher.early = Duration::from_millis(100);
        assert!(w.start());
        assert_eq!(w.wait(), Ok(()));
        let row = w.row();
        assert!((1..=3).contains(&row.playtime_secs), "{}", row.playtime_secs);
        assert_eq!(row.last_played, Some(1000));
        assert_eq!(w.nav.depth(), 1, "no failure message");
    }

    #[test]
    fn the_emulator_runs_in_its_own_process_group() {
        // Exits 0 only when its process group id is its own pid.
        let mut w = world(r#"[ "$(cut -d' ' -f5 /proc/$$/stat)" = "$$" ]"#);
        assert!(w.start());
        assert_eq!(w.wait(), Ok(()));
    }

    #[test]
    fn a_missing_game_file_does_not_launch() {
        let mut w = world("exit 0");
        fs::remove_file(w.ctx.games_root.join("gc/Zelda.iso")).unwrap();
        assert!(!w.start());
        assert!(!w.launcher.is_running());
        assert_eq!(w.nav.top().title(), "Couldn't start the game");
    }

    #[test]
    fn quitting_while_playing_records_playtime_so_far() {
        let mut w = world("echo $$; exec sleep 30");
        let _child = kill_on_drop(&w);
        assert!(w.start());
        std::thread::sleep(Duration::from_millis(1100));
        w.launcher.quit(&mut w.nav, &mut w.ctx, 2000);
        let row = w.row();
        // About 1 s ran; allow for a loaded machine.
        assert!((1..=3).contains(&row.playtime_secs), "{}", row.playtime_secs);
        assert_eq!(row.last_played, Some(2000));
        assert!(!w.launcher.is_running());
    }

    #[test]
    fn input_is_swallowed_while_playing() {
        let mut w = world("echo $$; exec sleep 30");
        let _child = kill_on_drop(&w);
        assert!(w.start());
        let hits = Hits {
            tabs: vec![(crate::ui::Rect::new(0.0, 0.0, 100.0, 100.0), 1)],
            back: Some(crate::ui::Rect::new(0.0, 0.0, 100.0, 100.0)),
            hints: Vec::new(),
        };
        let inputs = [
            Input::Action(Action::NextTab),
            Input::Action(Action::Back),
            Input::Action(Action::Confirm),
            Input::Pointer(Pointer::Back),
            Input::Pointer(Pointer::Click { x: 50.0, y: 50.0 }),
        ];
        for input in inputs {
            w.nav.handle(input, &hits, &mut w.ctx);
            assert_eq!((w.nav.active_tab(), w.nav.depth()), (0, 2), "{input:?}");
            assert_eq!(w.nav.top().title(), "Playing");
        }
        assert!(w.ctx.launch.is_none());
        w.launcher.quit(&mut w.nav, &mut w.ctx, 0);
    }

    #[test]
    fn confirm_asks_for_a_launch_only_when_the_emulator_is_installed() {
        let mut w = world("exit 0");
        let game = w.game.clone();
        assert!(matches!(confirm_game(&game, &mut w.ctx), Transition::None));
        assert_eq!(w.ctx.launch.take().map(|g| g.id), Some(game.id));
        assert!(w.ctx.play.is_none());

        w.ctx.emulators = crate::emulators::Emulators::from_installed(Vec::<String>::new());
        assert!(matches!(confirm_game(&game, &mut w.ctx), Transition::Push(_)));
        assert!(w.ctx.launch.is_none());
    }

    fn game_of(w: &World, system: &str) -> GameRow {
        GameRow {
            system: system.into(),
            ..w.game.clone()
        }
    }

    #[test]
    fn confirm_asks_to_play_an_embedded_game_when_its_core_is_installed() {
        let mut w = world("exit 0");
        w.ctx.data_dir = w._dir.path().join("data");
        touch(&w.ctx.data_dir.join("cores/bsnes_libretro.so"));
        let snes = game_of(&w, "snes");
        assert!(matches!(confirm_game(&snes, &mut w.ctx), Transition::None));
        assert_eq!(
            w.ctx.play.take(),
            Some(Play::resume(snes.clone())),
            "from the Resume State"
        );
        assert!(w.ctx.launch.is_none());

        let from_slot = Play {
            game: snes,
            slot: Some(2),
        };
        assert!(matches!(
            confirm_play(from_slot.clone(), &mut w.ctx),
            Transition::None
        ));
        assert_eq!(w.ctx.play.take(), Some(from_slot), "from Save State slot 2");
    }

    #[test]
    fn confirm_on_an_embedded_game_whose_core_is_missing_downloads_it() {
        let mut w = world("exit 0");
        w.ctx.data_dir = w._dir.path().join("data");
        touch(&w.ctx.data_dir.join("cores/bsnes_libretro.so"));
        // Never touches the network: this only checks that Confirm starts a download.
        w.ctx.start_download = Box::new(cores::testing::spawn_failing);
        let gba = game_of(&w, "gba");
        assert!(matches!(confirm_game(&gba, &mut w.ctx), Transition::Push(_)));
        assert!(w.ctx.play.is_none() && w.ctx.launch.is_none());
    }

    #[test]
    fn confirm_on_ps1_with_no_bios_shows_the_missing_bios_message() {
        let mut w = world("exit 0");
        w.ctx.data_dir = w._dir.path().join("data");
        touch(&w.ctx.data_dir.join("cores/mednafen_psx_hw_libretro.so"));
        let ps1 = game_of(&w, "ps1");
        let Transition::Push(screen) = confirm_game(&ps1, &mut w.ctx) else {
            panic!("expected the missing-BIOS message to be pushed");
        };
        assert_eq!(screen.title(), "PlayStation BIOS missing");
        assert!(w.ctx.play.is_none());
    }

    #[test]
    fn confirm_on_ps1_with_any_bios_file_in_any_case_starts_it() {
        let mut w = world("exit 0");
        w.ctx.data_dir = w._dir.path().join("data");
        touch(&w.ctx.data_dir.join("cores/mednafen_psx_hw_libretro.so"));
        fs::create_dir_all(w.ctx.games_root.join("bios")).unwrap();
        fs::write(w.ctx.games_root.join("bios/SCPH5502.bin"), b"bios data").unwrap();
        let ps1 = game_of(&w, "ps1");
        assert!(matches!(confirm_game(&ps1, &mut w.ctx), Transition::None));
        assert_eq!(w.ctx.play.take().map(|p| p.game.system), Some("ps1".to_owned()));
    }

    #[test]
    fn confirm_on_ps1_with_no_core_and_no_bios_shows_the_message_and_starts_no_download() {
        let mut w = world("exit 0");
        w.ctx.data_dir = w._dir.path().join("data");
        w.ctx.start_download = Box::new(|_, _| panic!("must not download a Core without a BIOS"));
        let ps1 = game_of(&w, "ps1");
        let Transition::Push(screen) = confirm_game(&ps1, &mut w.ctx) else {
            panic!("expected the missing-BIOS message to be pushed");
        };
        assert_eq!(screen.title(), "PlayStation BIOS missing");
        assert!(w.ctx.play.is_none());
    }

    #[test]
    fn confirm_on_psp_with_nothing_installed_starts_the_assets_step() {
        let mut w = world("exit 0");
        w.ctx.data_dir = w._dir.path().join("data");
        let psp = game_of(&w, "psp");
        let Transition::Push(screen) = confirm_game(&psp, &mut w.ctx) else {
            panic!("expected the download screen to be pushed");
        };
        assert_eq!(screen.title(), "Downloading the Core");
        assert!(w.ctx.play.is_none());
    }

    #[test]
    fn confirm_on_psp_with_both_the_core_and_its_assets_installed_plays() {
        let mut w = world("exit 0");
        w.ctx.data_dir = w._dir.path().join("data");
        touch(&w.ctx.data_dir.join("cores/ppsspp_libretro.so"));
        fs::create_dir_all(cores::assets_dir(&w.ctx.data_dir)).unwrap();
        let psp = game_of(&w, "psp");
        assert!(matches!(confirm_game(&psp, &mut w.ctx), Transition::None));
        assert_eq!(w.ctx.play.take().map(|p| p.game.system), Some("psp".to_owned()));
    }

    #[test]
    fn confirm_on_psp_with_assets_already_present_skips_straight_to_the_core_step() {
        let mut w = world("exit 0");
        w.ctx.data_dir = w._dir.path().join("data");
        fs::create_dir_all(cores::assets_dir(&w.ctx.data_dir)).unwrap();
        w.ctx.start_assets_download = Box::new(|_| panic!("assets are already installed"));
        w.ctx.start_download = Box::new(cores::testing::spawn_failing);
        let psp = game_of(&w, "psp");
        assert!(matches!(confirm_game(&psp, &mut w.ctx), Transition::Push(_)));
        assert!(w.ctx.play.is_none());
    }

    #[test]
    fn confirm_asks_to_play_a_ds_game_once_its_core_is_installed() {
        let mut w = world("exit 0");
        w.ctx.data_dir = w._dir.path().join("data");
        touch(&w.ctx.data_dir.join("cores/melondsds_libretro.so"));
        let nds = game_of(&w, "nds");
        assert!(matches!(confirm_game(&nds, &mut w.ctx), Transition::None));
        assert_eq!(w.ctx.play.take().map(|p| p.game.system), Some("nds".to_owned()));
    }

    #[test]
    fn later_update_names_the_system() {
        // Every Milestone-4 Embedded System already has a Core (PSP's last of all), so no System
        // currently takes this message; it pins the wording `why_not_playable` would show for a
        // future coreless one.
        assert_eq!(
            later_update(systems::by_id("psp").unwrap()),
            "Playing PSP games inside Game37 comes in a later update."
        );
    }
}
