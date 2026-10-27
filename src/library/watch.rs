//! Keeping the library in step with the Games Root while Game37 runs (FR-LIB-1, FR-LIB-2).
//!
//! A worker thread scans every System at start, then watches the Games Root and rescans a System
//! shortly after something in its folder changes. The UI thread owns the [`LibraryWatcher`] and
//! applies finished scans with [`LibraryWatcher::poll`], so the [`Library`] never leaves it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use notify::event::{AccessKind, AccessMode, ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use super::FoundGame;
use super::db::{DbError, Library};
use super::scan::{scan_system, system_for_path};
use crate::systems::{SYSTEMS, System};

/// How long a System's folder must stay quiet before it is rescanned, so a burst of changes
/// (copying a big file, extracting a folder Game) leads to one rescan.
const DEBOUNCE: Duration = Duration::from_millis(500);

/// How often the worker checks whether the Games Root is there. inotify can't see a missing root
/// appear, nor the drive or network share it is on being unmounted.
const ROOT_CHECK: Duration = Duration::from_secs(2);

/// Scans the Games Root in the background and watches it for changes. Dropping it stops watching.
pub struct LibraryWatcher {
    inbox: Sender<Msg>,
    results: Receiver<Scanned>,
    /// A scan that failed to apply, tried again before any newer one.
    retry: Option<Scanned>,
    worker: Option<JoinHandle<()>>,
}

/// A finished scan of one System's folder.
struct Scanned {
    system: &'static str,
    games: Vec<FoundGame>,
}

/// What the worker thread receives: changes from the folder watcher, or the order to stop.
enum Msg {
    Changed(notify::Result<Event>),
    Stop,
}

impl LibraryWatcher {
    /// Starts the worker thread: it scans every System in System Order, one result per System,
    /// then watches the Games Root. A missing Games Root scans as empty, and is watched and
    /// rescanned once it appears.
    pub fn start(games_root: PathBuf) -> LibraryWatcher {
        let (inbox, messages) = mpsc::channel();
        let (finished, results) = mpsc::channel();
        let changes = inbox.clone();
        let worker = thread::Builder::new()
            .name("library-watch".into())
            .spawn(move || {
                let mut worker = Worker {
                    watcher: None,
                    root: games_root,
                    root_found: false,
                    changes,
                    messages,
                    finished,
                    pending: HashMap::new(),
                    warned: false,
                };
                // The worker only returns once told to stop; the folder watcher stops with it.
                let _ = worker.run();
            })
            .expect("failed to spawn the library watcher thread");
        LibraryWatcher {
            inbox,
            results,
            retry: None,
            worker: Some(worker),
        }
    }

    /// Applies every scan finished since the last call, without blocking. Call once per frame on
    /// the UI thread. Returns whether any view changed. A scan that fails to apply is kept and
    /// applied first at the next call.
    pub fn poll(&mut self, library: &mut Library, now: i64) -> Result<bool, DbError> {
        self.apply(|scanned| library.sync_system(scanned.system, &scanned.games, now))
    }

    /// [`poll`](Self::poll), with the step that applies one scan passed in.
    fn apply(&mut self, mut sync: impl FnMut(&Scanned) -> Result<bool, DbError>) -> Result<bool, DbError> {
        let mut changed = false;
        while let Some(scanned) = self.retry.take().or_else(|| self.results.try_recv().ok()) {
            match sync(&scanned) {
                Ok(synced) => changed |= synced,
                Err(e) => {
                    self.retry = Some(scanned);
                    return Err(e);
                }
            }
        }
        Ok(changed)
    }
}

impl Drop for LibraryWatcher {
    fn drop(&mut self) {
        let _ = self.inbox.send(Msg::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The worker was told to stop, or the [`LibraryWatcher`] is gone.
struct Stopped;

/// The worker thread's state.
struct Worker {
    /// `None` while the Games Root is missing or can't be watched.
    watcher: Option<RecommendedWatcher>,
    root: PathBuf,
    /// Whether the Games Root was there when last looked at.
    root_found: bool,
    /// Where the folder watcher sends its changes.
    changes: Sender<Msg>,
    messages: Receiver<Msg>,
    finished: Sender<Scanned>,
    /// Systems waiting for a rescan, by id, with the time of their latest change.
    pending: HashMap<&'static str, Instant>,
    warned: bool,
}

impl Worker {
    fn run(&mut self) -> Result<(), Stopped> {
        self.root_found = self.root.is_dir();
        if self.root_found {
            self.watch_root();
        } else {
            self.log_root_missing();
        }

        // Each System folder is watched before it is scanned, so no change slips between the two.
        for system in &SYSTEMS {
            self.drain()?;
            self.watch_system(system);
            self.scan(system);
        }

        let mut next_check = Instant::now() + ROOT_CHECK;
        loop {
            let wake = self.next_due().map_or(next_check, |due| due.min(next_check));
            match self
                .messages
                .recv_timeout(wake.saturating_duration_since(Instant::now()))
            {
                Ok(Msg::Changed(change)) => self.on_change(change),
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return Err(Stopped),
                Err(RecvTimeoutError::Timeout) => {}
            }
            if Instant::now() >= next_check {
                self.check_root()?;
                next_check = Instant::now() + ROOT_CHECK;
            }
            self.rescan_due()?;
        }
    }

    /// Notices the Games Root appearing, or disappearing in a way inotify doesn't report.
    fn check_root(&mut self) -> Result<(), Stopped> {
        match (self.root_found, self.root.is_dir()) {
            (false, true) => self.root_appeared()?,
            (true, false) => self.root_lost(),
            _ => {}
        }
        Ok(())
    }

    /// Watches the Games Root and every System folder in it, then rescans every System.
    fn root_appeared(&mut self) -> Result<(), Stopped> {
        self.root_found = true;
        self.watch_root();
        for system in &SYSTEMS {
            self.drain()?;
            self.watch_system(system);
        }
        self.rescan_all();
        Ok(())
    }

    /// Stops watching the Games Root and rescans every System. A missing Games Root scans as
    /// empty, so its Games are marked absent and keep their data.
    fn root_lost(&mut self) {
        self.root_found = false;
        self.watcher = None;
        self.log_root_missing();
        self.rescan_all();
    }

    /// Logged once each time the Games Root goes missing, not at every check.
    fn log_root_missing(&self) {
        eprintln!(
            "Games Root {} not found: watching for it to appear",
            self.root.display()
        );
    }

    fn rescan_all(&mut self) {
        let now = Instant::now();
        self.pending.extend(SYSTEMS.iter().map(|system| (system.id, now)));
    }

    /// Handles the messages already waiting, without blocking. Every loop over Systems calls it
    /// between Systems, so dropping the [`LibraryWatcher`] never waits for more than one scan.
    fn drain(&mut self) -> Result<(), Stopped> {
        loop {
            match self.messages.try_recv() {
                Ok(Msg::Changed(change)) => self.on_change(change),
                Ok(Msg::Stop) | Err(TryRecvError::Disconnected) => return Err(Stopped),
                Err(TryRecvError::Empty) => return Ok(()),
            }
        }
    }

    /// Watches the Games Root itself, not recursively: that is enough to see System folders
    /// appear. Each System folder gets its own recursive watch, so `bios/` and `saves/` are never
    /// watched, and running out of inotify watches in one System leaves the others watched.
    fn watch_root(&mut self) {
        let changes = self.changes.clone();
        let watcher = notify::recommended_watcher(move |change| {
            let _ = changes.send(Msg::Changed(change));
        });
        let watcher = watcher.and_then(|mut w| w.watch(&self.root, RecursiveMode::NonRecursive).map(|()| w));
        match watcher {
            Ok(watcher) => self.watcher = Some(watcher),
            Err(e) => self.warn(format!("Can't watch the Games Root {}: {e}", self.root.display())),
        }
    }

    fn watch_system(&mut self, system: &System) {
        let Some(watcher) = self.watcher.as_mut() else {
            return;
        };
        let dir = self.root.join(system.id);
        if !dir.is_dir() {
            return;
        }
        if let Err(e) = watcher.watch(&dir, RecursiveMode::Recursive) {
            self.warn(format!("Can't watch {} for changes: {e}", dir.display()));
        }
    }

    /// Logs the first watcher problem only, so a persistent one doesn't flood stderr.
    fn warn(&mut self, message: String) {
        if !self.warned {
            eprintln!("{message}");
            self.warned = true;
        }
    }

    fn on_change(&mut self, change: notify::Result<Event>) {
        let event = match change {
            Ok(event) => event,
            Err(e) => return self.warn(format!("Watching the Games Root failed: {e}")),
        };
        let now = Instant::now();
        if event.need_rescan() {
            // The watcher lost events (e.g. the inotify queue overflowed): any System may have changed.
            return self.rescan_all();
        }
        if !changes_content(&event.kind) {
            return;
        }
        for path in &event.paths {
            if *path == self.root {
                // The Games Root itself was deleted or renamed away, and its watch went with it.
                let gone = matches!(
                    event.kind,
                    EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
                );
                if gone && self.root_found {
                    self.root_lost();
                }
                continue;
            }
            // Paths outside every System folder (`bios/`, `saves/`, the root itself) map to None.
            let Some(system) = system_for_path(&self.root, path) else {
                continue;
            };
            self.pending.insert(system.id, now);
            if arrives(&event.kind) && path.parent() == Some(self.root.as_path()) {
                // A System folder was created or renamed into place: watch what lands in it.
                self.watch_system(system);
            }
        }
    }

    /// When the next pending System will have been quiet for long enough.
    fn next_due(&self) -> Option<Instant> {
        self.pending.values().min().map(|&last| last + DEBOUNCE)
    }

    /// Rescans, in System Order, every pending System that has been quiet for long enough.
    fn rescan_due(&mut self) -> Result<(), Stopped> {
        let now = Instant::now();
        // Windows' folder watcher doesn't report the Games Root itself going, as inotify does, so a
        // rescan that finds it gone stops watching it; it is watched afresh once it is back.
        if self.root_found && !self.root.is_dir() && self.next_due().is_some_and(|due| now >= due) {
            self.root_lost();
        }
        for system in &SYSTEMS {
            if self
                .pending
                .get(system.id)
                .is_some_and(|&last| now >= last + DEBOUNCE)
            {
                self.pending.remove(system.id);
                self.scan(system);
                self.drain()?;
            }
        }
        Ok(())
    }

    fn scan(&self, system: &'static System) {
        let games = scan_system(&self.root, system);
        // A send only fails once the LibraryWatcher is gone, and then a Stop is on its way.
        let _ = self.finished.send(Scanned {
            system: system.id,
            games,
        });
    }
}

/// Whether an event can change which Games exist. Reads, including Game37's own scans, can't.
fn changes_content(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
}

/// Whether an event puts something at its path: a creation or the new name of a rename.
fn arrives(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Modify(ModifyKind::Name(
                RenameMode::To | RenameMode::Both | RenameMode::Any
            ))
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::*;
    use crate::library::db::SortOrder;

    /// Generous, so a loaded machine doesn't fail the tests; they finish as soon as the
    /// condition holds.
    const DEADLINE: Duration = Duration::from_secs(5);
    const NOW: i64 = 1_000;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"").unwrap();
    }

    fn paths(library: &Library, system: &str) -> Vec<String> {
        let games = library.system_games(system, SortOrder::AZ).unwrap();
        games.into_iter().map(|g| g.rel_path).collect()
    }

    /// Polls like the UI does until `done` holds, or fails the test after `DEADLINE`.
    #[track_caller]
    fn poll_until(watcher: &mut LibraryWatcher, library: &mut Library, done: impl Fn(&Library) -> bool) {
        let deadline = Instant::now() + DEADLINE;
        loop {
            watcher.poll(library, NOW).unwrap();
            if done(library) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for the library to update"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Scan results that arrive within `wait`, in order, without applying them.
    fn results_within(watcher: &LibraryWatcher, wait: Duration) -> Vec<Scanned> {
        let deadline = Instant::now() + wait;
        let mut results = Vec::new();
        while let Ok(scanned) = watcher
            .results
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            results.push(scanned);
        }
        results
    }

    /// Starts a watcher on `root` and applies its whole startup scan, one result per System.
    fn started(root: &Path) -> (LibraryWatcher, Library) {
        let watcher = LibraryWatcher::start(root.to_owned());
        let mut library = Library::open_in_memory().unwrap();
        let deadline = Instant::now() + DEADLINE;
        for system in &SYSTEMS {
            let scanned = watcher
                .results
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("startup scan timed out");
            assert_eq!(scanned.system, system.id, "startup scan goes in System Order");
            library.sync_system(scanned.system, &scanned.games, NOW).unwrap();
        }
        (watcher, library)
    }

    #[test]
    fn startup_scan_fills_the_library() {
        let root = TempDir::new().unwrap();
        touch(&root.path().join("snes/Super Mario World (USA).sfc"));
        touch(&root.path().join("gba/Golden Sun (Europe).gba"));
        let mut watcher = LibraryWatcher::start(root.path().to_owned());
        let mut library = Library::open_in_memory().unwrap();

        poll_until(&mut watcher, &mut library, |l| {
            paths(l, "snes") == ["snes/Super Mario World (USA).sfc"]
                && paths(l, "gba") == ["gba/Golden Sun (Europe).gba"]
        });
    }

    #[test]
    fn a_file_copied_in_appears_and_a_deleted_one_disappears() {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("snes")).unwrap();
        let (mut watcher, mut library) = started(root.path());
        assert!(paths(&library, "snes").is_empty());

        let game = root.path().join("snes/Chrono Trigger (USA).sfc");
        touch(&game);
        poll_until(&mut watcher, &mut library, |l| {
            paths(l, "snes") == ["snes/Chrono Trigger (USA).sfc"]
        });

        fs::remove_file(&game).unwrap();
        poll_until(&mut watcher, &mut library, |l| paths(l, "snes").is_empty());
    }

    #[test]
    fn a_renamed_file_moves_to_its_new_path() {
        let root = TempDir::new().unwrap();
        touch(&root.path().join("snes/Old Name.sfc"));
        let (mut watcher, mut library) = started(root.path());
        assert_eq!(paths(&library, "snes"), ["snes/Old Name.sfc"]);

        fs::rename(
            root.path().join("snes/Old Name.sfc"),
            root.path().join("snes/New Name.sfc"),
        )
        .unwrap();
        poll_until(&mut watcher, &mut library, |l| {
            paths(l, "snes") == ["snes/New Name.sfc"]
        });
    }

    #[test]
    fn a_folder_game_created_bit_by_bit_appears_as_one_game() {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("ps3")).unwrap();
        let (mut watcher, mut library) = started(root.path());

        let game = root.path().join("ps3/Demon's Souls (Europe)");
        fs::create_dir_all(game.join("PS3_GAME/USRDIR")).unwrap();
        touch(&game.join("PS3_GAME/PARAM.SFO"));
        touch(&game.join("PS3_GAME/USRDIR/EBOOT.BIN"));
        touch(&game.join("PS3_DISC.SFB"));

        poll_until(&mut watcher, &mut library, |l| {
            paths(l, "ps3") == ["ps3/Demon's Souls (Europe)"]
        });
    }

    #[test]
    fn a_system_folder_created_while_running_is_scanned_and_watched() {
        let root = TempDir::new().unwrap();
        let (mut watcher, mut library) = started(root.path());

        touch(&root.path().join("gb/Tetris (World).gb"));
        poll_until(&mut watcher, &mut library, |l| {
            paths(l, "gb") == ["gb/Tetris (World).gb"]
        });

        touch(&root.path().join("gb/Kirby's Dream Land (USA, Europe).gb"));
        poll_until(&mut watcher, &mut library, |l| paths(l, "gb").len() == 2);
    }

    #[test]
    fn a_burst_of_writes_to_one_system_rescans_it_once() {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("snes")).unwrap();
        let (watcher, _library) = started(root.path());

        // The burst lasts longer than DEBOUNCE, with short gaps: the wait restarts at each change.
        let count = 30;
        for i in 0..count {
            touch(&root.path().join(format!("snes/Game {i}.sfc")));
            thread::sleep(Duration::from_millis(20));
        }

        let rescan = watcher
            .results
            .recv_timeout(DEADLINE)
            .expect("no rescan after the burst");
        assert_eq!(rescan.system, "snes");
        assert_eq!(rescan.games.len(), count);
        let more: Vec<&str> = results_within(&watcher, DEBOUNCE * 2)
            .iter()
            .map(|s| s.system)
            .collect();
        assert!(
            more.is_empty(),
            "expected one rescan for the burst, then also: {more:?}"
        );
    }

    #[test]
    fn changes_outside_system_folders_rescan_nothing() {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("bios")).unwrap();
        let (watcher, _library) = started(root.path());

        touch(&root.path().join("bios/scph5501.bin"));
        touch(&root.path().join("saves/snes/Chrono Trigger (USA).srm"));
        touch(&root.path().join("notes.txt"));

        let results = results_within(&watcher, DEBOUNCE * 3);
        let systems: Vec<&str> = results.iter().map(|s| s.system).collect();
        assert!(systems.is_empty(), "unexpected rescans: {systems:?}");
    }

    #[test]
    fn a_missing_games_root_scans_empty_without_panicking() {
        let root = TempDir::new().unwrap();
        let (watcher, library) = started(&root.path().join("missing"));
        assert!(library.system_counts().unwrap().is_empty());
        assert!(results_within(&watcher, DEBOUNCE).is_empty());
    }

    #[test]
    fn a_games_root_created_while_running_is_scanned_and_watched() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("Games");
        let (mut watcher, mut library) = started(&root);

        touch(&root.join("snes/Chrono Trigger (USA).sfc"));
        poll_until(&mut watcher, &mut library, |l| {
            paths(l, "snes") == ["snes/Chrono Trigger (USA).sfc"]
        });

        touch(&root.join("snes/Secret of Mana (USA).sfc"));
        poll_until(&mut watcher, &mut library, |l| paths(l, "snes").len() == 2);
    }

    #[test]
    fn a_games_root_removed_and_recreated_goes_absent_and_comes_back() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("Games");
        touch(&root.join("snes/Chrono Trigger (USA).sfc"));
        let (mut watcher, mut library) = started(&root);
        let id = library.system_games("snes", SortOrder::AZ).unwrap()[0].id;

        fs::remove_dir_all(&root).unwrap();
        poll_until(&mut watcher, &mut library, |l| paths(l, "snes").is_empty());

        touch(&root.join("snes/Chrono Trigger (USA).sfc"));
        poll_until(&mut watcher, &mut library, |l| {
            paths(l, "snes") == ["snes/Chrono Trigger (USA).sfc"]
        });
        let back = library.system_games("snes", SortOrder::AZ).unwrap();
        assert_eq!(back[0].id, id, "the Game keeps its row and data");

        // The new Games Root is watched, not just scanned once.
        touch(&root.join("snes/Secret of Mana (USA).sfc"));
        poll_until(&mut watcher, &mut library, |l| paths(l, "snes").len() == 2);
    }

    #[test]
    fn drop_stops_the_worker_promptly() {
        let root = TempDir::new().unwrap();
        touch(&root.path().join("snes/Game.sfc"));
        let (watcher, _library) = started(root.path());
        // A change in flight while dropping must not delay it.
        touch(&root.path().join("snes/Another Game.sfc"));

        let (dropped, done) = mpsc::channel();
        thread::spawn(move || {
            drop(watcher);
            dropped.send(()).unwrap();
        });
        done.recv_timeout(Duration::from_secs(2))
            .expect("dropping the watcher took longer than 2 s");
    }

    #[test]
    fn a_scan_that_fails_to_apply_is_retried_at_the_next_poll() {
        let root = TempDir::new().unwrap();
        touch(&root.path().join("snes/Chrono Trigger (USA).sfc"));
        let mut watcher = LibraryWatcher::start(root.path().to_owned());
        let mut library = Library::open_in_memory().unwrap();

        // The snes scan fails to apply once, as if the database were busy.
        let deadline = Instant::now() + DEADLINE;
        let mut failed = false;
        while !failed {
            let applied = watcher.apply(|scanned| {
                if scanned.system == "snes" {
                    failed = true;
                    return Err(DbError::UnknownSchemaVersion(0));
                }
                library.sync_system(scanned.system, &scanned.games, NOW)
            });
            assert_eq!(
                applied.is_err(),
                failed,
                "the failure is reported by the poll that hit it"
            );
            assert!(Instant::now() < deadline, "timed out waiting for the snes scan");
            thread::sleep(Duration::from_millis(10));
        }
        assert!(paths(&library, "snes").is_empty());

        // Nothing in the snes folder changes again, so only the retry can bring the Game in.
        poll_until(&mut watcher, &mut library, |l| {
            paths(l, "snes") == ["snes/Chrono Trigger (USA).sfc"]
        });
    }
}
