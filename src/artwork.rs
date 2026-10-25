//! Artwork (FR-ART-1–4): a tall cover and a wide hero per Game from SteamGridDB, cached under
//! `<data dir>/artwork/` (ADR 0005) and handed to screens decoded, without ever blocking input
//! (NFR-3).
//!
//! Screens ask for a Game's Artwork while drawing. The first ask queues the work and answers
//! [`Art::Loading`]; later asks answer from memory. Two background threads do the work: one reads
//! cached files, the other talks to SteamGridDB one request at a time. [`Artwork::poll`], once a
//! frame, collects their results and records what was found in the library (FR-LIB-7), so a restart
//! with everything cached makes no network requests at all (FR-ART-4).
//!
//! Only Games a screen asks about are looked up, never the whole library: SteamGridDB's terms forbid
//! bulk loading. Only SteamGridDB hosts are contacted (NFR-5), and the API key only goes to the API.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::io::{Cursor, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::config::ApiKey;
use crate::library::db::{DbError, GameRow, Library};

const API: &str = "https://www.steamgriddb.com/api/v2";

/// Static PNG and JPEG covers in the tall sizes, without NSFW or humour entries.
const COVER_QUERY: &str =
    "dimensions=600x900,342x482,660x930&types=static&nsfw=false&humor=false&mimes=image/png,image/jpeg";
/// Static PNG and JPEG heroes in the wide sizes, without NSFW or humour entries.
const HERO_QUERY: &str =
    "dimensions=1920x620,3840x1240,1600x650&types=static&nsfw=false&humor=false&mimes=image/png,image/jpeg";

/// A cap on an API response.
const MAX_API_BYTES: u64 = 4 * 1024 * 1024;
/// A cap on one Artwork file as downloaded. The largest heroes are a few MB.
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// How many of SteamGridDB's top results a lookup tries in turn while they are no good.
const FILES_TRIED: usize = 3;
/// A cap on a decoded file's width and height, so a malicious file can't exhaust memory.
const MAX_DIMENSION: u32 = 8192;
/// How many bytes of decoded Artwork are kept in memory. Beyond it, the least recently asked for
/// is dropped and read again from the cache (not the network) when next asked for.
const MEMORY_BUDGET: usize = 256 * 1024 * 1024;

/// Which of a Game's Artwork.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// The tall cover on Shelves, grids and the Details Page.
    Cover,
    /// The wide hero behind the Details Page.
    Hero,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Cover => "cover",
            Kind::Hero => "hero",
        }
    }

    /// The SteamGridDB endpoint and query listing a game's Artwork of this kind.
    fn list_path(self, game: u64) -> String {
        match self {
            Kind::Cover => format!("/grids/game/{game}?{COVER_QUERY}"),
            Kind::Hero => format!("/heroes/game/{game}?{HERO_QUERY}"),
        }
    }

    /// The box decoded Artwork is drawn to fill: covers as large as the Details Page shows them,
    /// heroes the whole screen.
    fn max_size(self) -> (u32, u32) {
        match self {
            Kind::Cover => (400, 600),
            Kind::Hero => (1920, 1080),
        }
    }
}

/// Decoded Artwork, ready to become a texture: RGBA8, rows top to bottom, `width * 4` bytes each.
pub struct Picture {
    /// Unique per decoded `Picture` for the whole run, so a texture cache can key on it: a Game
    /// whose Artwork changes gets a `Picture` with a new id.
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl fmt::Debug for Picture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Picture {} ({}×{})", self.id, self.width, self.height)
    }
}

static NEXT_PICTURE: AtomicU64 = AtomicU64::new(1);

/// What to draw for a Game's Artwork right now.
#[derive(Debug, Clone)]
pub enum Art {
    /// Being read or fetched: draw the placeholder until it arrives.
    Loading,
    Ready(Arc<Picture>),
    /// There is none (no API key, no match, nothing of this kind, or fetching failed this run):
    /// draw the Display Title and System icon (FR-ART-3).
    Placeholder,
}

/// One SteamGridDB result the player can choose instead (FR-ART-2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// SteamGridDB's id for the file, as [`Artwork::chosen`] reports it.
    pub id: u64,
    pub kind: Kind,
    /// The full-size file, fetched only once chosen.
    pub url: String,
    /// The small preview to show in a picker, see [`Artwork::preview`].
    pub preview: String,
    pub width: u32,
    pub height: u32,
    /// Who made it, for crediting.
    pub author: Option<String>,
    /// The SteamGridDB game it belongs to.
    game: u64,
}

/// The other SteamGridDB results for a Game, for "Choose different Artwork".
#[derive(Debug, Clone)]
pub enum Candidates {
    Loading,
    /// In SteamGridDB's order; the first is the one picked automatically.
    Ready(Arc<[Candidate]>),
    /// There are none to choose from, and why.
    Unavailable(Reason),
}

/// Why a Game has no [`Candidates`], for the picker to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// There is no SteamGridDB API key, or Artwork is off altogether.
    NoKey,
    /// SteamGridDB knows no game by the Display Title.
    NoMatch,
    /// SteamGridDB couldn't be reached, or refused the key, this run.
    Failed,
}

/// One HTTP GET. Only API requests carry the key; Artwork files come from the CDN without it.
pub struct Request<'a> {
    pub url: &'a str,
    pub api_key: Option<&'a ApiKey>,
    /// The body may be at most this long.
    pub max_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchError {
    /// Couldn't connect, or the connection broke.
    Connect,
    /// Connected, but the server didn't answer 200.
    Status(u16),
    /// The body was longer than `max_bytes`.
    TooLarge,
}

/// Performs an HTTP GET and returns its body. Injectable so tests run offline; production uses
/// [`ureq_fetch`].
pub type Fetch = Arc<dyn Fn(&Request) -> Result<Vec<u8>, FetchError> + Send + Sync>;

/// A Game's Artwork, from SteamGridDB or the cache. Lives on `Ctx`; every method returns at once.
pub struct Artwork {
    /// `<data dir>/artwork/`; `None` turns Artwork off altogether ([`Artwork::off`]).
    dir: Option<PathBuf>,
    key: Option<ApiKey>,
    fetch: Fetch,
    /// Set once SteamGridDB's API refuses the key (401, 403) or asks to slow down (429): no further
    /// requests this run.
    stopped: Arc<AtomicBool>,
    budget: usize,
    /// Started on first use, so a `Ctx` that never shows Artwork starts no threads.
    workers: OnceCell<Workers>,
    state: RefCell<State>,
}

impl fmt::Debug for Artwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Artwork")
            .field("dir", &self.dir)
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct State {
    slots: HashMap<Key, Slot>,
    /// By library id, kind and the Display Title they were listed for.
    candidates: HashMap<(i64, Kind, String), Candidates>,
    /// The last job id handed out.
    jobs: u64,
    /// Ticks on every ask, for dropping the least recently asked for.
    clock: u64,
    /// Decoded bytes held by `Slot::Ready`.
    bytes: usize,
}

impl State {
    fn next_job(&mut self) -> u64 {
        self.jobs += 1;
        self.jobs
    }

    fn is_current(&self, key: &Key, job: u64) -> bool {
        matches!(self.slots.get(key), Some(Slot::Loading(j)) if *j == job)
    }

    fn remove(&mut self, key: &Key) {
        if let Some(Slot::Ready { picture, .. }) = self.slots.remove(key) {
            self.bytes -= picture.rgba.len();
        }
    }

    /// Stores `picture` for `key`, then drops the least recently asked for pictures until the
    /// decoded bytes fit `budget`, keeping `key`'s however large it is.
    fn ready(&mut self, key: Key, picture: Picture, budget: usize) {
        self.remove(&key);
        self.bytes += picture.rgba.len();
        let used = self.clock;
        self.slots.insert(
            key.clone(),
            Slot::Ready {
                picture: Arc::new(picture),
                used,
            },
        );
        while self.bytes > budget {
            let oldest = self
                .slots
                .iter()
                .filter(|(k, _)| **k != key)
                .filter_map(|(k, slot)| match slot {
                    Slot::Ready { used, .. } => Some((*used, k.clone())),
                    _ => None,
                })
                .min_by_key(|(used, _)| *used);
            let Some((_, oldest)) = oldest else { break };
            self.remove(&oldest);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    /// A Game's (library id) Artwork, for its Display Title: once the title changes, what was
    /// matched automatically for the old one is asked for afresh.
    Game(i64, Kind, String),
    /// A candidate's preview, by URL.
    Preview(String, Kind),
}

enum Slot {
    Loading(u64),
    Ready { picture: Arc<Picture>, used: u64 },
    Placeholder,
}

/// What a Game's `artwork` column records (FR-LIB-7), as JSON.
///
/// A search answers for the Display Title it was made for: once the title changes (a rename, or a
/// new file name), `game` and the picks made automatically no longer hold, and the next ask
/// searches again. Picks the player chose stay.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Record {
    /// The Display Title last searched for; `None` in a record from before titles were recorded,
    /// which counts as searched for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    /// The SteamGridDB game `title` matched; `None` means the search found no match.
    game: Option<u64>,
    /// `None`: not looked up yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cover: Option<Pick>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hero: Option<Pick>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Pick {
    /// SteamGridDB has nothing of this kind for the game.
    NoArtwork,
    /// The SteamGridDB file shown, picked automatically or `chosen` by the player.
    File {
        id: u64,
        url: String,
        #[serde(default)]
        chosen: bool,
    },
}

impl Pick {
    fn is_chosen(&self) -> bool {
        matches!(self, Pick::File { chosen: true, .. })
    }
}

impl Record {
    /// What searching the Display Title `title` found (`Some(None)`: no match), if it was searched.
    fn search(&self, title: &str) -> Option<Option<u64>> {
        (self.title.as_deref() == Some(title)).then_some(self.game)
    }

    /// The pick of `kind` that holds for the Display Title `title`: the player's choice always, an
    /// automatic one only while it was matched for `title`.
    fn pick(&self, kind: Kind, title: &str) -> Option<&Pick> {
        let pick = match kind {
            Kind::Cover => self.cover.as_ref(),
            Kind::Hero => self.hero.as_ref(),
        }?;
        (pick.is_chosen() || self.search(title).is_some()).then_some(pick)
    }

    /// The record once searching `title` found `game`: the player's choices stay, and automatic
    /// picks only if they came from this same search.
    fn searched(self, title: &str, game: Option<u64>) -> Record {
        if self.search(title) == Some(game) {
            return self;
        }
        let chosen = |pick: Option<Pick>| pick.filter(Pick::is_chosen);
        Record {
            title: Some(title.to_owned()),
            game,
            cover: chosen(self.cover),
            hero: chosen(self.hero),
        }
    }

    fn set(&mut self, kind: Kind, pick: Pick) {
        match kind {
            Kind::Cover => self.cover = Some(pick),
            Kind::Hero => self.hero = Some(pick),
        }
    }
}

/// A Game's record; `None` if it has none yet or it is unreadable, which means look it up again.
fn read_record(library: &Library, id: i64) -> Option<Record> {
    let text = library.artwork(id).ok().flatten()?;
    serde_json::from_str(&text).ok()
}

fn write_record(library: &Library, id: i64, record: &Record) -> Result<(), DbError> {
    let text = serde_json::to_string(record).expect("a Record always serialises");
    library.set_artwork(id, &text)
}

/// Applies `update` to Game `id`'s record (an empty one if it has none) and writes it if that
/// changed anything. For the background results: on failure the Artwork still shows, and the next
/// run looks it up again.
fn update_record(library: &Library, id: i64, update: impl FnOnce(Record) -> Record) {
    let before = read_record(library, id);
    let after = update(before.clone().unwrap_or_default());
    if before.as_ref() == Some(&after) {
        return;
    }
    if let Err(e) = write_record(library, id, &after) {
        eprintln!("game37: can't record Artwork: {e}");
    }
}

impl Artwork {
    /// Artwork cached under `dir` (`<data dir>/artwork/`), fetched from SteamGridDB if there is a
    /// key.
    pub fn start(dir: PathBuf, key: Option<ApiKey>) -> Artwork {
        // One agent for the whole run, so requests to the same host reuse its connection.
        let agent = ureq_agent();
        Artwork::new(
            dir,
            key,
            Arc::new(move |request: &Request| ureq_fetch(&agent, request)),
        )
    }

    /// Like [`Artwork::start`], fetching through `fetch`: tests and screenshot mode pass a fake.
    pub fn new(dir: PathBuf, key: Option<ApiKey>, fetch: Fetch) -> Artwork {
        Artwork {
            dir: Some(dir),
            key,
            fetch,
            stopped: Arc::new(AtomicBool::new(false)),
            budget: MEMORY_BUDGET,
            workers: OnceCell::new(),
            state: RefCell::new(State::default()),
        }
    }

    /// No Artwork at all: every Game shows placeholder art, and nothing is read or fetched.
    pub fn off() -> Artwork {
        let fetch: Fetch = Arc::new(|_: &Request| Err(FetchError::Connect));
        Artwork {
            dir: None,
            ..Artwork::new(PathBuf::new(), None, fetch)
        }
    }

    fn online(&self) -> bool {
        self.key.is_some() && !self.stopped.load(Ordering::Relaxed)
    }

    fn workers(&self, dir: &Path) -> &Workers {
        self.workers.get_or_init(|| {
            Workers::spawn(
                dir.to_owned(),
                self.key.clone(),
                Arc::clone(&self.fetch),
                Arc::clone(&self.stopped),
            )
        })
    }

    /// `game`'s tall cover. Call it every frame the cover is on screen.
    pub fn cover(&self, library: &Library, game: &GameRow) -> Art {
        self.art(library, game, Kind::Cover)
    }

    /// `game`'s wide hero. Call it every frame the hero is on screen.
    pub fn hero(&self, library: &Library, game: &GameRow) -> Art {
        self.art(library, game, Kind::Hero)
    }

    /// `game`'s Artwork of `kind`: from memory if it is there; otherwise queues reading it from the
    /// cache, or looking it up on SteamGridDB, and answers `Loading`.
    pub fn art(&self, library: &Library, game: &GameRow, kind: Kind) -> Art {
        let Some(dir) = &self.dir else {
            return Art::Placeholder;
        };
        let key = Key::Game(game.id, kind, game.title.clone());
        let mut state = self.state.borrow_mut();
        state.clock += 1;
        let now = state.clock;
        if let Some(slot) = state.slots.get_mut(&key) {
            return slot.art(now);
        }
        let record = read_record(library, game.id).unwrap_or_default();
        let search = record.search(&game.title);
        let mut slot = match (record.pick(kind, &game.title), search) {
            (Some(Pick::File { id, url, .. }), _) => {
                let job = state.next_job();
                self.workers(dir).disk(DiskJob {
                    key: key.clone(),
                    job,
                    kind,
                    path: cache_path(dir, kind, *id, url),
                    url: url.clone(),
                });
                Slot::Loading(job)
            }
            // Searched this Display Title before: nothing of this kind, or no match.
            (Some(Pick::NoArtwork), _) | (None, Some(None)) => Slot::Placeholder,
            _ if self.online() => {
                let job = state.next_job();
                self.workers(dir).net(NetJob::Lookup {
                    key: key.clone(),
                    job,
                    game: search.flatten(),
                });
                Slot::Loading(job)
            }
            _ => Slot::Placeholder,
        };
        let art = slot.art(now);
        state.slots.insert(key, slot);
        art
    }

    /// The SteamGridDB results of `kind` for `game` to choose from (FR-ART-2): queues the lookup
    /// on first ask and answers `Loading` until it arrives.
    pub fn candidates(&self, library: &Library, game: &GameRow, kind: Kind) -> Candidates {
        let Some(dir) = &self.dir else {
            return Candidates::Unavailable(Reason::NoKey);
        };
        let mut state = self.state.borrow_mut();
        let key = (game.id, kind, game.title.clone());
        if let Some(candidates) = state.candidates.get(&key) {
            return candidates.clone();
        }
        let search = read_record(library, game.id).and_then(|r| r.search(&game.title));
        let candidates = match search {
            Some(None) => Candidates::Unavailable(Reason::NoMatch),
            _ if self.online() => {
                self.workers(dir).net(NetJob::Candidates {
                    game_id: game.id,
                    kind,
                    title: game.title.clone(),
                    game: search.flatten(),
                });
                Candidates::Loading
            }
            _ if self.key.is_none() => Candidates::Unavailable(Reason::NoKey),
            // SteamGridDB refused the key or asked to slow down earlier this run.
            _ => Candidates::Unavailable(Reason::Failed),
        };
        state.candidates.insert(key, candidates.clone());
        candidates
    }

    /// `candidate`'s small preview, fetched on first ask and kept in memory only.
    pub fn preview(&self, candidate: &Candidate) -> Art {
        let Some(dir) = &self.dir else {
            return Art::Placeholder;
        };
        let key = Key::Preview(candidate.preview.clone(), candidate.kind);
        let mut state = self.state.borrow_mut();
        state.clock += 1;
        let now = state.clock;
        if let Some(slot) = state.slots.get_mut(&key) {
            return slot.art(now);
        }
        let mut slot = if self.online() {
            let job = state.next_job();
            self.workers(dir).net(NetJob::Download {
                key: key.clone(),
                job,
                url: candidate.preview.clone(),
                path: None,
            });
            Slot::Loading(job)
        } else {
            Slot::Placeholder
        };
        let art = slot.art(now);
        state.slots.insert(key, slot);
        art
    }

    /// The id of the [`Candidate`] `game` shows as its Artwork of `kind`, if any.
    pub fn chosen(&self, library: &Library, game: &GameRow, kind: Kind) -> Option<u64> {
        match read_record(library, game.id)?.pick(kind, &game.title)? {
            Pick::File { id, .. } => Some(*id),
            Pick::NoArtwork => None,
        }
    }

    /// Makes `candidate` `game`'s Artwork of its kind, for good (FR-ART-2, FR-LIB-7): it stays
    /// when the Display Title changes. The next ask fetches and shows it.
    pub fn choose(&self, library: &Library, game: &GameRow, candidate: &Candidate) -> Result<(), DbError> {
        let mut record = read_record(library, game.id).unwrap_or_default();
        if record.search(&game.title).is_none() {
            record = record.searched(&game.title, Some(candidate.game));
        }
        record.set(
            candidate.kind,
            Pick::File {
                id: candidate.id,
                url: candidate.url.clone(),
                chosen: true,
            },
        );
        write_record(library, game.id, &record)?;
        self.state
            .borrow_mut()
            .remove(&Key::Game(game.id, candidate.kind, game.title.clone()));
        Ok(())
    }

    /// Collects finished background work and records it in `library`. Never blocks; call it once a
    /// frame. Returns whether anything arrived.
    pub fn poll(&self, library: &Library) -> bool {
        let Some(workers) = self.workers.get() else {
            return false;
        };
        let mut arrived = false;
        while let Ok(done) = workers.done.try_recv() {
            arrived = true;
            self.finish(library, workers, done);
        }
        arrived
    }

    /// Whether any Artwork or candidate list asked for is still on its way.
    pub fn busy(&self) -> bool {
        let state = self.state.borrow();
        state.slots.values().any(|s| matches!(s, Slot::Loading(_)))
            || state
                .candidates
                .values()
                .any(|c| matches!(c, Candidates::Loading))
    }

    fn finish(&self, library: &Library, workers: &Workers, done: Done) {
        let mut state = self.state.borrow_mut();
        match done {
            Done::Art { key, job, outcome } => {
                let current = state.is_current(&key, job);
                match outcome {
                    Outcome::Loaded(picture) if current => state.ready(key, picture, self.budget),
                    Outcome::Unreadable { url, path } if current && self.online() => {
                        workers.net(NetJob::Download {
                            key,
                            job,
                            url,
                            path: Some(path),
                        });
                    }
                    Outcome::Found { game, pick, picture } => {
                        let Key::Game(id, kind, title) = &key else { return };
                        update_record(library, *id, |record| {
                            let mut record = record.searched(title, Some(game));
                            // The player may have chosen other Artwork meanwhile: theirs stays.
                            if record.pick(*kind, title).is_none() {
                                record.set(*kind, pick);
                            }
                            record
                        });
                        if current {
                            match picture {
                                Some(picture) => state.ready(key, picture, self.budget),
                                None => {
                                    state.slots.insert(key, Slot::Placeholder);
                                }
                            }
                        }
                    }
                    Outcome::NoMatch | Outcome::Searched { .. } => {
                        let game = match outcome {
                            Outcome::Searched { game } => Some(game),
                            _ => None,
                        };
                        let Key::Game(id, _, title) = &key else { return };
                        update_record(library, *id, |record| record.searched(title, game));
                        if current {
                            state.slots.insert(key, Slot::Placeholder);
                        }
                    }
                    _ if current => {
                        state.slots.insert(key, Slot::Placeholder);
                    }
                    _ => {}
                }
            }
            Done::Candidates {
                game_id,
                kind,
                title,
                result,
            } => {
                let candidates = match result {
                    Ok((game, list)) => {
                        update_record(library, game_id, |record| record.searched(&title, Some(game)));
                        Candidates::Ready(list.into())
                    }
                    Err(Failure::NoMatch) => {
                        update_record(library, game_id, |record| record.searched(&title, None));
                        Candidates::Unavailable(Reason::NoMatch)
                    }
                    Err(Failure::Failed) => Candidates::Unavailable(Reason::Failed),
                };
                state.candidates.insert((game_id, kind, title), candidates);
            }
        }
    }
}

impl Slot {
    fn art(&mut self, now: u64) -> Art {
        match self {
            Slot::Loading(_) => Art::Loading,
            Slot::Ready { picture, used } => {
                *used = now;
                Art::Ready(Arc::clone(picture))
            }
            Slot::Placeholder => Art::Placeholder,
        }
    }
}

/// Where an Artwork file is cached: `<artwork dir>/<kind>/<SteamGridDB id>.<extension>`.
fn cache_path(dir: &Path, kind: Kind, id: u64, url: &str) -> PathBuf {
    let url = url.to_ascii_lowercase();
    let extension = if url.ends_with(".png") {
        "png"
    } else if url.ends_with(".jpg") || url.ends_with(".jpeg") {
        "jpg"
    } else {
        "img"
    };
    dir.join(kind.name()).join(format!("{id}.{extension}"))
}

/// Whether `url` is an Artwork file on a SteamGridDB host, over https (NFR-5): its CDN
/// (`*.steamgriddb.com`) or its older S3 bucket (`s3.amazonaws.com/steamgriddb/`). Anything else in
/// a response is never fetched.
fn allowed_artwork_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    // Only a plain host name: no user info, no port.
    if host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return false;
    }
    // SteamGridDB's file names are plain hex: nothing that could step out of the bucket.
    if path.contains("..") || path.contains('\\') || path.contains('%') {
        return false;
    }
    let host = host.to_ascii_lowercase();
    host == "steamgriddb.com"
        || host.ends_with(".steamgriddb.com")
        || (host == "s3.amazonaws.com" && path.starts_with("/steamgriddb/"))
}

/// Percent-encodes `text` for a URL path segment.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Decodes a PNG or JPEG file and scales it down to the smallest size that still covers `kind`'s
/// box, since drawing fills the box and crops the rest. Shrinking it to fit inside the box instead
/// would make a wide hero blurry once it is enlarged again to fill the screen's height.
fn decode(bytes: &[u8], kind: Kind) -> Option<Picture> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    let (w, h) = kind.max_size();
    let (iw, ih) = (image.width(), image.height());
    let scale = (w as f64 / iw as f64).max(h as f64 / ih as f64);
    let image = if scale < 1.0 {
        let nw = ((iw as f64 * scale).round() as u32).max(w);
        let nh = ((ih as f64 * scale).round() as u32).max(h);
        image.resize_exact(nw, nh, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let rgba = image.into_rgba8();
    Some(Picture {
        id: NEXT_PICTURE.fetch_add(1, Ordering::Relaxed),
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

/// Writes `bytes` to `path` whole or not at all. A failure only costs a download next run.
fn save(path: &Path, bytes: &[u8]) {
    let Some(dir) = path.parent() else { return };
    let written = std::fs::create_dir_all(dir).and_then(|()| {
        let mut file = tempfile::NamedTempFile::new_in(dir)?;
        file.write_all(bytes)?;
        file.persist(path).map_err(|e| e.error)?;
        Ok(())
    });
    if let Err(e) = written {
        eprintln!("game37: can't cache Artwork in {}: {e}", path.display());
    }
}

struct Workers {
    disk: Sender<DiskJob>,
    net: Sender<NetJob>,
    done: Receiver<Done>,
}

impl Workers {
    /// Starts the two worker threads. They are detached and end once the `Artwork` is dropped and
    /// their current job is done.
    fn spawn(dir: PathBuf, key: Option<ApiKey>, fetch: Fetch, stopped: Arc<AtomicBool>) -> Workers {
        let (done_tx, done) = mpsc::channel();
        let (disk, disk_rx) = mpsc::channel::<DiskJob>();
        let (net, net_rx) = mpsc::channel::<NetJob>();
        let disk_done = done_tx.clone();
        thread::spawn(move || {
            for job in disk_rx {
                let fallback = Done::Art {
                    key: job.key.clone(),
                    job: job.job,
                    outcome: Outcome::Failed,
                };
                let done = panic::catch_unwind(AssertUnwindSafe(|| job.run())).unwrap_or(fallback);
                if disk_done.send(done).is_err() {
                    break;
                }
            }
        });
        thread::spawn(move || {
            let mut worker = Net {
                dir,
                key,
                fetch,
                stopped,
                searched: HashMap::new(),
            };
            for job in net_rx {
                let fallback = job.failed();
                let done = panic::catch_unwind(AssertUnwindSafe(|| worker.run(job))).unwrap_or(fallback);
                if done_tx.send(done).is_err() {
                    break;
                }
            }
        });
        Workers { disk, net, done }
    }

    /// Queues `job`. The threads only end once `Workers` is dropped, so sending can't fail.
    fn disk(&self, job: DiskJob) {
        let _ = self.disk.send(job);
    }

    fn net(&self, job: NetJob) {
        let _ = self.net.send(job);
    }
}

/// Reads a cached Artwork file.
struct DiskJob {
    key: Key,
    job: u64,
    kind: Kind,
    path: PathBuf,
    /// Where to fetch it again if the file is gone or broken.
    url: String,
}

impl DiskJob {
    fn run(self) -> Done {
        let picture = std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| decode(&bytes, self.kind));
        let outcome = match picture {
            Some(picture) => Outcome::Loaded(picture),
            None => Outcome::Unreadable {
                url: self.url,
                path: self.path,
            },
        };
        Done::Art {
            key: self.key,
            job: self.job,
            outcome,
        }
    }
}

enum NetJob {
    /// Finds a Game's top result of `key`'s kind: searches `key`'s Display Title unless the
    /// SteamGridDB `game` is known, lists that game's Artwork, then fetches, caches and decodes the
    /// first of the top [`FILES_TRIED`] that is any good.
    Lookup { key: Key, job: u64, game: Option<u64> },
    /// Fetches and decodes one file, caching it at `path` if there is one.
    Download {
        key: Key,
        job: u64,
        url: String,
        path: Option<PathBuf>,
    },
    /// Lists a Game's Artwork of `kind` for the player to choose from.
    Candidates {
        game_id: i64,
        kind: Kind,
        title: String,
        game: Option<u64>,
    },
}

impl NetJob {
    /// What to report if the job panics.
    fn failed(&self) -> Done {
        match self {
            NetJob::Lookup { key, job, .. } | NetJob::Download { key, job, .. } => Done::Art {
                key: key.clone(),
                job: *job,
                outcome: Outcome::Failed,
            },
            NetJob::Candidates {
                game_id, kind, title, ..
            } => Done::Candidates {
                game_id: *game_id,
                kind: *kind,
                title: title.clone(),
                result: Err(Failure::Failed),
            },
        }
    }
}

enum Done {
    Art {
        key: Key,
        job: u64,
        outcome: Outcome,
    },
    Candidates {
        game_id: i64,
        kind: Kind,
        /// The Display Title they were listed for.
        title: String,
        result: Result<(u64, Vec<Candidate>), Failure>,
    },
}

enum Outcome {
    Loaded(Picture),
    /// The cached file is missing or broken.
    Unreadable {
        url: String,
        path: PathBuf,
    },
    /// The Display Title matched SteamGridDB's `game`, whose top result of the kind is `pick`.
    Found {
        game: u64,
        pick: Pick,
        picture: Option<Picture>,
    },
    NoMatch,
    /// The Display Title matched SteamGridDB's `game`, but its files didn't arrive this run: the
    /// next run fetches them without searching again.
    Searched {
        game: u64,
    },
    /// Couldn't fetch or decode it this run.
    Failed,
}

#[derive(Debug)]
enum Failure {
    NoMatch,
    Failed,
}

/// Why an Artwork file couldn't be shown.
enum FileError {
    Fetch(FetchError),
    /// Not a file on a SteamGridDB host, or not one that decodes.
    Unusable,
}

#[derive(Deserialize)]
struct Envelope<T> {
    success: bool,
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

#[derive(Deserialize)]
struct FoundGame {
    id: u64,
}

#[derive(Deserialize)]
struct ApiArtwork {
    id: u64,
    /// `None` (or null) in an entry that is no use: it is skipped.
    url: Option<String>,
    thumb: Option<String>,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    author: Option<Author>,
}

#[derive(Deserialize)]
struct Author {
    name: Option<String>,
}

/// The network worker's side: one request at a time.
struct Net {
    dir: PathBuf,
    key: Option<ApiKey>,
    fetch: Fetch,
    stopped: Arc<AtomicBool>,
    /// Display Title searches done this run, so a Game's cover and hero search once.
    searched: HashMap<String, Option<u64>>,
}

impl Net {
    fn run(&mut self, job: NetJob) -> Done {
        match job {
            NetJob::Lookup { key, job, game } => {
                let outcome = self.lookup(&key, game);
                Done::Art { key, job, outcome }
            }
            NetJob::Download { key, job, url, path } => {
                let kind = match key {
                    Key::Game(_, kind, _) | Key::Preview(_, kind) => kind,
                };
                let outcome = match self.download(&url, kind, path.as_deref()) {
                    Ok(picture) => Outcome::Loaded(picture),
                    Err(_) => Outcome::Failed,
                };
                Done::Art { key, job, outcome }
            }
            NetJob::Candidates {
                game_id,
                kind,
                title,
                game,
            } => {
                let result = self
                    .resolve(&title, game)
                    .and_then(|game| Ok((game, self.list(kind, game)?)));
                Done::Candidates {
                    game_id,
                    kind,
                    title,
                    result,
                }
            }
        }
    }

    fn lookup(&mut self, key: &Key, game: Option<u64>) -> Outcome {
        let Key::Game(_, kind, title) = key else {
            return Outcome::Failed;
        };
        let kind = *kind;
        let game = match self.resolve(title, game) {
            Ok(game) => game,
            Err(Failure::NoMatch) => return Outcome::NoMatch,
            Err(Failure::Failed) => return Outcome::Failed,
        };
        let Ok(list) = self.list(kind, game) else {
            return Outcome::Failed;
        };
        // A file that is no good, or that its host won't serve, gives way to the next. If none of
        // them would ever do, that is recorded as none, so a restart doesn't fetch them again.
        let mut refused = false;
        for top in list.iter().take(FILES_TRIED) {
            let path = cache_path(&self.dir, kind, top.id, &top.url);
            match self.download(&top.url, kind, Some(&path)) {
                Ok(picture) => {
                    return Outcome::Found {
                        game,
                        pick: Pick::File {
                            id: top.id,
                            url: top.url.clone(),
                            chosen: false,
                        },
                        picture: Some(picture),
                    };
                }
                Err(FileError::Fetch(FetchError::Connect)) => return Outcome::Searched { game },
                Err(FileError::Fetch(FetchError::Status(_))) => refused = true,
                Err(FileError::Fetch(FetchError::TooLarge) | FileError::Unusable) => {}
            }
        }
        if refused {
            return Outcome::Searched { game };
        }
        Outcome::Found {
            game,
            pick: Pick::NoArtwork,
            picture: None,
        }
    }

    /// The SteamGridDB game for a Game with the Display Title `title`: `known`, or the top search
    /// result for `title`.
    fn resolve(&mut self, title: &str, known: Option<u64>) -> Result<u64, Failure> {
        if let Some(game) = known {
            return Ok(game);
        }
        if let Some(found) = self.searched.get(title) {
            return found.ok_or(Failure::NoMatch);
        }
        let found: Vec<FoundGame> = self.api(&format!("/search/autocomplete/{}", encode(title)))?;
        let found = found.first().map(|g| g.id);
        self.searched.insert(title.to_owned(), found);
        found.ok_or(Failure::NoMatch)
    }

    /// `game`'s Artwork of `kind` that is on SteamGridDB's own hosts, in SteamGridDB's order.
    fn list(&self, kind: Kind, game: u64) -> Result<Vec<Candidate>, Failure> {
        let listed: Vec<ApiArtwork> = self.api(&kind.list_path(game))?;
        let candidates = listed
            .into_iter()
            .filter_map(|i| {
                let (url, preview) = (i.url?, i.thumb?);
                (allowed_artwork_url(&url) && allowed_artwork_url(&preview)).then(|| Candidate {
                    id: i.id,
                    kind,
                    url,
                    preview,
                    width: i.width,
                    height: i.height,
                    author: i.author.and_then(|a| a.name),
                    game,
                })
            })
            .collect();
        Ok(candidates)
    }

    /// GETs an API path and returns its `data`. A 404 (SteamGridDB's "Game not found") is no data.
    fn api<T: DeserializeOwned>(&self, path: &str) -> Result<Vec<T>, Failure> {
        let Some(key) = &self.key else {
            return Err(Failure::Failed);
        };
        let body = match self.get(&format!("{API}{path}"), Some(key), MAX_API_BYTES) {
            Ok(body) => body,
            Err(FetchError::Status(404)) => return Ok(Vec::new()),
            Err(_) => return Err(Failure::Failed),
        };
        match serde_json::from_slice::<Envelope<T>>(&body) {
            Ok(envelope) if envelope.success => Ok(envelope.data),
            _ => Err(Failure::Failed),
        }
    }

    /// Fetches an Artwork file, without the key, and decodes it; caches it at `path` if given.
    fn download(&self, url: &str, kind: Kind, path: Option<&Path>) -> Result<Picture, FileError> {
        if !allowed_artwork_url(url) {
            return Err(FileError::Unusable);
        }
        let bytes = self.get(url, None, MAX_FILE_BYTES).map_err(FileError::Fetch)?;
        let picture = decode(&bytes, kind).ok_or(FileError::Unusable)?;
        if let Some(path) = path {
            save(path, &bytes);
        }
        Ok(picture)
    }

    /// One request, unless SteamGridDB has asked for no more this run. The API refusing the key or
    /// asking to slow down stops every further request; a file host's error fails only that file.
    fn get(&self, url: &str, api_key: Option<&ApiKey>, max_bytes: u64) -> Result<Vec<u8>, FetchError> {
        if self.stopped.load(Ordering::Relaxed) {
            return Err(FetchError::Connect);
        }
        let result = (self.fetch)(&Request {
            url,
            api_key,
            max_bytes,
        });
        if let Err(FetchError::Status(401 | 403 | 429)) = result
            && api_key.is_some()
        {
            self.stopped.store(true, Ordering::Relaxed);
        }
        result
    }
}

/// The production agent: `ureq` over rustls, https only, 30 s per step and a minute for the body.
/// Redirects are not followed, so no host outside NFR-5 is ever reached.
fn ureq_agent() -> ureq::Agent {
    let step = Some(Duration::from_secs(30));
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_resolve(step)
        .timeout_connect(step)
        .timeout_send_request(step)
        .timeout_recv_response(step)
        .timeout_recv_body(Some(Duration::from_secs(60)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build();
    config.into()
}

/// The production fetcher, through `agent` (see [`ureq_agent`]).
fn ureq_fetch(agent: &ureq::Agent, request: &Request) -> Result<Vec<u8>, FetchError> {
    let mut get = agent.get(request.url);
    if let Some(key) = request.api_key {
        get = get.header("Authorization", format!("Bearer {}", key.expose()));
    }
    let response = get.call().map_err(|_| FetchError::Connect)?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(FetchError::Status(status));
    }
    response
        .into_body()
        .with_config()
        .limit(request.max_bytes)
        .read_to_vec()
        .map_err(|e| match e {
            ureq::Error::BodyExceedsLimit(_) => FetchError::TooLarge,
            _ => FetchError::Connect,
        })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::library::FoundGame;
    use crate::library::db::SortOrder;
    use std::sync::{Condvar, Mutex};
    use std::time::Instant;

    pub(crate) const CDN: &str = "https://cdn2.steamgriddb.com/file/sgdb-cdn";

    pub(crate) type Response = Result<Vec<u8>, FetchError>;

    /// An offline SteamGridDB: answers each URL from the first route it starts with, and logs every
    /// request with whether it carried the key. Closing the gate holds every request until opened.
    #[derive(Clone)]
    pub(crate) struct Fake {
        routes: Arc<Mutex<Vec<(String, Response)>>>,
        log: Arc<Mutex<Vec<(String, bool)>>>,
        gate: Arc<(Mutex<bool>, Condvar)>,
    }

    impl Fake {
        pub(crate) fn new() -> Fake {
            Fake {
                routes: Arc::default(),
                log: Arc::default(),
                gate: Arc::new((Mutex::new(true), Condvar::new())),
            }
        }

        pub(crate) fn route(&self, prefix: &str, response: Response) -> &Fake {
            self.routes.lock().unwrap().push((prefix.to_owned(), response));
            self
        }

        /// Like [`Fake::route`], ahead of every route already there.
        pub(crate) fn route_first(&self, prefix: &str, response: Response) -> &Fake {
            self.routes
                .lock()
                .unwrap()
                .insert(0, (prefix.to_owned(), response));
            self
        }

        pub(crate) fn api(&self, path: &str, data: &str) -> &Fake {
            let body = format!("{{\"success\":true,\"data\":{data}}}");
            self.route(&format!("{API}{path}"), Ok(body.into_bytes()))
        }

        pub(crate) fn fetch(&self) -> Fetch {
            let fake = self.clone();
            Arc::new(move |request: &Request| {
                let (open, opened) = &*fake.gate;
                let mut is_open = open.lock().unwrap();
                while !*is_open {
                    is_open = opened.wait(is_open).unwrap();
                }
                drop(is_open);
                let url = request.url.to_owned();
                fake.log
                    .lock()
                    .unwrap()
                    .push((url.clone(), request.api_key.is_some()));
                let routes = fake.routes.lock().unwrap();
                let route = routes.iter().find(|(prefix, _)| url.starts_with(prefix.as_str()));
                route.map_or(Err(FetchError::Status(500)), |(_, response)| response.clone())
            })
        }

        fn set_gate(&self, open: bool) {
            *self.gate.0.lock().unwrap() = open;
            self.gate.1.notify_all();
        }

        fn urls(&self) -> Vec<String> {
            self.log
                .lock()
                .unwrap()
                .iter()
                .map(|(url, _)| url.clone())
                .collect()
        }

        pub(crate) fn calls(&self) -> usize {
            self.log.lock().unwrap().len()
        }
    }

    pub(crate) fn png(w: u32, h: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 30, 255]))
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn listing(list: &[(u64, &str)]) -> String {
        let entries: Vec<String> = list
            .iter()
            .map(|(id, url)| {
                format!(
                    "{{\"id\":{id},\"score\":0,\"style\":\"alternate\",\"url\":\"{url}\",\
                     \"thumb\":\"{CDN}/thumb/{id}.jpg\",\"width\":600,\"height\":900,\
                     \"author\":{{\"name\":\"someone\",\"steam64\":\"1\",\"avatar\":\"x\"}}}}"
                )
            })
            .collect();
        format!("[{}]", entries.join(","))
    }

    /// SteamGridDB as the tests know it: "Alpha" is game 42, with covers 7 (600×900) and 8
    /// (342×482) and hero 9 (1920×620); "Beta" is game 50 with cover 11 and no heroes.
    pub(crate) fn steamgriddb() -> Fake {
        let fake = Fake::new();
        fake.api(
            "/search/autocomplete/Alpha",
            "[{\"id\":42,\"name\":\"Alpha\"},{\"id\":43}]",
        )
        .api(
            "/grids/game/42?",
            &listing(&[
                (7, &format!("{CDN}/grid/7.png")),
                (8, &format!("{CDN}/grid/8.png")),
            ]),
        )
        .api("/heroes/game/42?", &listing(&[(9, &format!("{CDN}/hero/9.png"))]))
        .api("/search/autocomplete/Beta", "[{\"id\":50}]")
        .api(
            "/grids/game/50?",
            &listing(&[(11, &format!("{CDN}/grid/11.png"))]),
        )
        .api("/heroes/game/50?", "[]")
        .route(&format!("{CDN}/grid/7.png"), Ok(png(600, 900)))
        .route(&format!("{CDN}/grid/8.png"), Ok(png(342, 482)))
        .route(&format!("{CDN}/grid/11.png"), Ok(png(600, 900)))
        .route(&format!("{CDN}/hero/9.png"), Ok(png(1920, 620)))
        .route(&format!("{CDN}/thumb/"), Ok(png(60, 90)));
        fake
    }

    /// A library file in `dir` with the Games "Alpha" and "Beta", returned in that order.
    fn library(dir: &Path) -> (Library, Vec<GameRow>) {
        let mut library = Library::open(&dir.join("library.db")).unwrap();
        let found: Vec<FoundGame> = ["Alpha", "Beta"]
            .iter()
            .map(|title| FoundGame {
                system: "snes",
                rel_path: format!("snes/{title}.sfc"),
                title: (*title).to_owned(),
            })
            .collect();
        library.sync_system("snes", &found, 0).unwrap();
        let games = library.system_games("snes", SortOrder::AZ).unwrap();
        (library, games)
    }

    fn artwork(dir: &Path, fake: &Fake, key: bool) -> Artwork {
        let key = key.then(|| ApiKey::new("0123456789abcdef0123456789abcdef"));
        Artwork::new(dir.join("artwork"), key, fake.fetch())
    }

    /// Asks for `ask` every few milliseconds, polling in between, until it is no longer loading.
    pub(crate) fn settle<T>(
        artwork: &Artwork,
        library: &Library,
        ask: impl Fn() -> T,
        loading: fn(&T) -> bool,
    ) -> T {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            artwork.poll(library);
            let answer = ask();
            if !loading(&answer) {
                return answer;
            }
            assert!(Instant::now() < deadline, "never arrived");
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn wait(artwork: &Artwork, library: &Library, game: &GameRow, kind: Kind) -> Art {
        settle(
            artwork,
            library,
            || artwork.art(library, game, kind),
            |a| matches!(a, Art::Loading),
        )
    }

    fn wait_candidates(artwork: &Artwork, library: &Library, game: &GameRow) -> Vec<Candidate> {
        let ask = || artwork.candidates(library, game, Kind::Cover);
        match settle(artwork, library, ask, |c| matches!(c, Candidates::Loading)) {
            Candidates::Ready(list) => list.to_vec(),
            other => panic!("no candidates: {other:?}"),
        }
    }

    fn size(art: &Art) -> (u32, u32) {
        match art {
            Art::Ready(picture) => {
                assert_eq!(picture.rgba.len(), (picture.width * picture.height * 4) as usize);
                (picture.width, picture.height)
            }
            other => panic!("expected Artwork, got {other:?}"),
        }
    }

    fn is_placeholder(art: &Art) -> bool {
        matches!(art, Art::Placeholder)
    }

    /// Fetches both Games' covers and Alpha's hero through `fake` in a library in `dir`.
    fn fetch_all(dir: &Path, fake: &Fake) {
        let (library, games) = library(dir);
        let artwork = artwork(dir, fake, true);
        size(&wait(&artwork, &library, &games[0], Kind::Cover));
        size(&wait(&artwork, &library, &games[0], Kind::Hero));
        size(&wait(&artwork, &library, &games[1], Kind::Cover));
    }

    #[test]
    fn artwork_is_scaled_down_only_as_far_as_it_still_fills_its_box() {
        let size = |w, h, kind| {
            let picture = decode(&png(w, h), kind).unwrap();
            (picture.width, picture.height)
        };
        // A 3840×1240 hero keeps the full 1080 lines it needs to fill the screen's height.
        assert_eq!(size(3840, 1240, Kind::Hero), (3345, 1080));
        assert_eq!(size(1920, 620, Kind::Hero), (1920, 620));
        assert_eq!(size(600, 900, Kind::Cover), (400, 600));
        // A wide cover keeps the height it needs to fill the 2:3 cover.
        assert_eq!(size(1200, 900, Kind::Cover), (800, 600));
    }

    #[test]
    fn artwork_top_cover_and_hero_are_fetched_decoded_and_cached() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        let alpha = &games[0];

        assert!(matches!(artwork.cover(&library, alpha), Art::Loading));
        // Scaled down to fit 400×600.
        assert_eq!(size(&wait(&artwork, &library, alpha, Kind::Cover)), (400, 600));
        assert_eq!(size(&wait(&artwork, &library, alpha, Kind::Hero)), (1920, 620));

        let cached = dir.path().join("artwork");
        assert_eq!(std::fs::read(cached.join("cover/7.png")).unwrap(), png(600, 900));
        assert_eq!(std::fs::read(cached.join("hero/9.png")).unwrap(), png(1920, 620));
        let file = |id: u64, path: &str| Pick::File {
            id,
            url: format!("{CDN}/{path}"),
            chosen: false,
        };
        assert_eq!(
            read_record(&library, alpha.id),
            Some(Record {
                title: Some("Alpha".to_owned()),
                game: Some(42),
                cover: Some(file(7, "grid/7.png")),
                hero: Some(file(9, "hero/9.png")),
            })
        );
        // One search serves both; only the API gets the key.
        assert_eq!(
            fake.urls(),
            [
                format!("{API}/search/autocomplete/Alpha"),
                format!("{API}/grids/game/42?{COVER_QUERY}"),
                format!("{CDN}/grid/7.png"),
                format!("{API}/heroes/game/42?{HERO_QUERY}"),
                format!("{CDN}/hero/9.png"),
            ]
        );
        for (url, had_key) in fake.log.lock().unwrap().iter() {
            assert_eq!(*had_key, url.starts_with(API), "{url}");
        }
        // Beta was never asked about, so never looked up.
        assert_eq!(read_record(&library, games[1].id), None);
    }

    #[test]
    fn artwork_restart_serves_the_cache_without_fetching() {
        let dir = tempfile::tempdir().unwrap();
        fetch_all(dir.path(), &steamgriddb());

        let fake = Fake::new();
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        assert_eq!(
            size(&wait(&artwork, &library, &games[0], Kind::Cover)),
            (400, 600)
        );
        assert_eq!(
            size(&wait(&artwork, &library, &games[0], Kind::Hero)),
            (1920, 620)
        );
        assert_eq!(
            size(&wait(&artwork, &library, &games[1], Kind::Cover)),
            (400, 600)
        );
        assert_eq!(fake.calls(), 0);
    }

    #[test]
    fn artwork_missing_from_the_cache_is_fetched_again_without_searching() {
        let dir = tempfile::tempdir().unwrap();
        fetch_all(dir.path(), &steamgriddb());
        std::fs::remove_file(dir.path().join("artwork/cover/7.png")).unwrap();

        let fake = steamgriddb();
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        assert_eq!(
            size(&wait(&artwork, &library, &games[0], Kind::Cover)),
            (400, 600)
        );
        assert_eq!(fake.urls(), [format!("{CDN}/grid/7.png")]);
        assert!(dir.path().join("artwork/cover/7.png").exists());
    }

    #[test]
    fn artwork_without_a_key_makes_no_requests() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, false);
        assert!(is_placeholder(&artwork.cover(&library, &games[0])));
        assert!(is_placeholder(&artwork.hero(&library, &games[0])));
        assert!(matches!(
            artwork.candidates(&library, &games[0], Kind::Cover),
            Candidates::Unavailable(Reason::NoKey)
        ));
        assert!(!artwork.busy());
        assert!(!artwork.poll(&library));
        assert_eq!(fake.calls(), 0);
        assert_eq!(read_record(&library, games[0].id), None);

        let off = Artwork::off();
        assert!(is_placeholder(&off.cover(&library, &games[0])));
        assert!(matches!(
            off.candidates(&library, &games[0], Kind::Hero),
            Candidates::Unavailable(Reason::NoKey)
        ));
    }

    #[test]
    fn artwork_refused_key_stops_every_further_request() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new();
        fake.route(API, Err(FetchError::Status(401)));
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);

        assert!(is_placeholder(&wait(&artwork, &library, &games[0], Kind::Cover)));
        assert_eq!(fake.calls(), 1);
        assert!(is_placeholder(&artwork.hero(&library, &games[0])));
        assert!(is_placeholder(&artwork.cover(&library, &games[1])));
        assert!(matches!(
            artwork.candidates(&library, &games[1], Kind::Cover),
            Candidates::Unavailable(Reason::Failed)
        ));
        assert!(!artwork.busy());
        assert_eq!(fake.calls(), 1);
        // A refused key is not "no match": the next run tries again.
        assert_eq!(read_record(&library, games[0].id), None);
    }

    #[test]
    fn artwork_file_host_refusing_a_file_fails_only_that_file() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        // An old S3 object that was removed, or a CDN bot challenge.
        fake.route_first(&format!("{CDN}/grid/7.png"), Err(FetchError::Status(403)));
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        wait(&artwork, &library, &games[0], Kind::Cover);
        assert!(fake.urls().contains(&format!("{CDN}/grid/7.png")));
        let calls = fake.calls();

        // The API is still asked: Beta's cover arrives.
        assert_eq!(
            size(&wait(&artwork, &library, &games[1], Kind::Cover)),
            (400, 600)
        );
        assert_eq!(fake.urls()[calls], format!("{API}/search/autocomplete/Beta"));
    }

    #[test]
    fn artwork_top_file_that_does_not_decode_gives_way_to_the_next_which_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        fake.route_first(&format!("{CDN}/grid/7.png"), Ok(b"not an image".to_vec()));
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        assert_eq!(
            size(&wait(&artwork, &library, &games[0], Kind::Cover)),
            (342, 482)
        );
        assert_eq!(artwork.chosen(&library, &games[0], Kind::Cover), Some(8));
        assert!(dir.path().join("artwork/cover/8.png").exists());
        assert!(!dir.path().join("artwork/cover/7.png").exists());
        drop((artwork, library));

        let fake = Fake::new();
        let (library, games) = self::library(dir.path());
        let artwork = self::artwork(dir.path(), &fake, true);
        assert_eq!(
            size(&wait(&artwork, &library, &games[0], Kind::Cover)),
            (342, 482)
        );
        assert_eq!(fake.calls(), 0);
    }

    #[test]
    fn artwork_whose_top_files_are_all_no_good_is_remembered_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new();
        let files: Vec<(u64, String)> = (1..=4).map(|id| (id, format!("{CDN}/grid/{id}.png"))).collect();
        let files: Vec<(u64, &str)> = files.iter().map(|(id, url)| (*id, url.as_str())).collect();
        fake.api("/search/autocomplete/Alpha", "[{\"id\":42}]")
            .api("/grids/game/42?", &listing(&files))
            .route(&format!("{CDN}/grid/2.png"), Err(FetchError::TooLarge))
            .route(&format!("{CDN}/grid/"), Ok(b"not an image".to_vec()));
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        assert!(is_placeholder(&wait(&artwork, &library, &games[0], Kind::Cover)));
        let tried: Vec<String> = fake.urls().into_iter().filter(|u| u.starts_with(CDN)).collect();
        assert_eq!(tried, [1, 2, 3].map(|id| format!("{CDN}/grid/{id}.png")));
        let record = read_record(&library, games[0].id).unwrap();
        assert_eq!((record.game, record.cover), (Some(42), Some(Pick::NoArtwork)));
        drop((artwork, library));

        let fake = Fake::new();
        let (library, games) = self::library(dir.path());
        let artwork = self::artwork(dir.path(), &fake, true);
        assert!(is_placeholder(&artwork.cover(&library, &games[0])));
        assert_eq!(fake.calls(), 0);
    }

    #[test]
    fn artwork_entries_without_a_file_or_preview_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        let rest = listing(&[
            (7, &format!("{CDN}/grid/7.png")),
            (8, &format!("{CDN}/grid/8.png")),
        ]);
        let list = format!(
            "[{{\"id\":5,\"url\":\"{CDN}/grid/5.png\",\"thumb\":null}},\
             {{\"id\":6,\"thumb\":\"{CDN}/thumb/6.jpg\"}},{}",
            &rest[1..]
        );
        let body = format!("{{\"success\":true,\"data\":{list}}}");
        fake.route_first(&format!("{API}/grids/game/42?"), Ok(body.into_bytes()))
            .route_first(&format!("{CDN}/grid/5.png"), Ok(png(600, 900)));
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        let candidates = wait_candidates(&artwork, &library, &games[0]);
        assert_eq!(candidates.iter().map(|c| c.id).collect::<Vec<_>>(), [7, 8]);
        assert_eq!(
            size(&wait(&artwork, &library, &games[0], Kind::Cover)),
            (400, 600)
        );
        assert_eq!(artwork.chosen(&library, &games[0], Kind::Cover), Some(7));
    }

    #[test]
    fn artwork_file_that_fails_to_arrive_is_fetched_next_run_without_searching() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        fake.route_first(&format!("{CDN}/grid/7.png"), Err(FetchError::Connect));
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        assert!(is_placeholder(&wait(&artwork, &library, &games[0], Kind::Cover)));
        assert!(
            !fake.urls().contains(&format!("{CDN}/grid/8.png")),
            "offline: no use trying the next"
        );
        let searched = Record {
            title: Some("Alpha".to_owned()),
            game: Some(42),
            ..Record::default()
        };
        assert_eq!(read_record(&library, games[0].id), Some(searched));
        drop((artwork, library));

        let fake = steamgriddb();
        let (library, games) = self::library(dir.path());
        let artwork = self::artwork(dir.path(), &fake, true);
        assert_eq!(
            size(&wait(&artwork, &library, &games[0], Kind::Cover)),
            (400, 600)
        );
        assert_eq!(
            fake.urls(),
            [
                format!("{API}/grids/game/42?{COVER_QUERY}"),
                format!("{CDN}/grid/7.png"),
            ]
        );
    }

    #[test]
    fn artwork_candidates_that_fail_to_arrive_say_so_and_are_not_no_match() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new();
        fake.route(API, Err(FetchError::Connect));
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        let ask = || artwork.candidates(&library, &games[0], Kind::Hero);
        let answer = settle(&artwork, &library, ask, |c| matches!(c, Candidates::Loading));
        assert!(
            matches!(answer, Candidates::Unavailable(Reason::Failed)),
            "{answer:?}"
        );
        assert_eq!(
            read_record(&library, games[0].id),
            None,
            "the next run tries again"
        );
    }

    #[test]
    fn artwork_from_other_hosts_is_never_fetched() {
        let dir = tempfile::tempdir().unwrap();
        let foreign = [
            "https://evil.example.com/grid/1.png",
            "https://cdn2.steamgriddb.com.evil.example.com/grid/2.png",
            "http://cdn2.steamgriddb.com/grid/3.png",
        ];
        let fake = Fake::new();
        fake.api("/search/autocomplete/Alpha", "[{\"id\":42}]").api(
            "/grids/game/42?",
            &listing(&[
                (1, foreign[0]),
                (2, foreign[1]),
                (3, foreign[2]),
                (4, &format!("{CDN}/grid/4.png")),
            ]),
        );
        for url in foreign {
            fake.route(url, Ok(png(600, 900)));
        }
        fake.route(&format!("{CDN}/grid/4.png"), Ok(png(342, 482)));
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);

        // The top result on a SteamGridDB host.
        assert_eq!(
            size(&wait(&artwork, &library, &games[0], Kind::Cover)),
            (342, 482)
        );
        let candidates = wait_candidates(&artwork, &library, &games[0]);
        assert_eq!(candidates.iter().map(|c| c.id).collect::<Vec<_>>(), [4]);
        for url in fake.urls() {
            assert!(!foreign.contains(&url.as_str()), "fetched {url}");
        }
    }

    #[test]
    fn only_steamgriddb_hosts_over_https_are_allowed() {
        for url in [
            "https://cdn2.steamgriddb.com/file/sgdb-cdn/grid/abc.png",
            "https://CDN2.SteamGridDB.com/file/sgdb-cdn/thumb/abc.jpg",
            "https://s3.amazonaws.com/steamgriddb/grid/f033ab37c30201f73f142449d037028d.png",
        ] {
            assert!(allowed_artwork_url(url), "{url}");
        }
        for url in [
            "http://cdn2.steamgriddb.com/grid/abc.png",
            "https://evilsteamgriddb.com/grid/abc.png",
            "https://cdn2.steamgriddb.com.evil.com/grid/abc.png",
            "https://cdn2.steamgriddb.com@evil.com/grid/abc.png",
            "https://cdn2.steamgriddb.com:8443/grid/abc.png",
            "https://s3.amazonaws.com/otherbucket/abc.png",
            "https://s3.amazonaws.com/steamgriddb/../otherbucket/abc.png",
            "https://s3.amazonaws.com/steamgriddb/%2e%2e/otherbucket/abc.png",
            "https://evil.com/s3.amazonaws.com/steamgriddb/abc.png",
            "ftp://cdn2.steamgriddb.com/abc.png",
            "",
        ] {
            assert!(!allowed_artwork_url(url), "{url}");
        }
    }

    #[test]
    fn artwork_no_match_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new();
        fake.api("/search/autocomplete/Alpha", "[]");
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);

        assert!(is_placeholder(&wait(&artwork, &library, &games[0], Kind::Cover)));
        assert_eq!(fake.calls(), 1);
        let no_match = Record {
            title: Some("Alpha".to_owned()),
            ..Record::default()
        };
        assert_eq!(read_record(&library, games[0].id), Some(no_match));
        assert!(is_placeholder(&artwork.hero(&library, &games[0])));
        assert!(matches!(
            artwork.candidates(&library, &games[0], Kind::Cover),
            Candidates::Unavailable(Reason::NoMatch)
        ));
        assert_eq!(fake.calls(), 1);
        drop((artwork, library));

        let fake = steamgriddb();
        let (library, games) = self::library(dir.path());
        let artwork = self::artwork(dir.path(), &fake, true);
        assert!(is_placeholder(&artwork.cover(&library, &games[0])));
        assert_eq!(fake.calls(), 0);
    }

    #[test]
    fn artwork_kind_with_no_results_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        // Beta has covers but no heroes.
        assert!(is_placeholder(&wait(&artwork, &library, &games[1], Kind::Hero)));
        let calls = fake.calls();
        drop((artwork, library));

        let (library, games) = self::library(dir.path());
        let artwork = self::artwork(dir.path(), &fake, true);
        assert!(is_placeholder(&artwork.hero(&library, &games[1])));
        assert_eq!(fake.calls(), calls);
    }

    #[test]
    fn artwork_candidates_and_the_choice_persist_across_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        let alpha = &games[0];
        assert_eq!(size(&wait(&artwork, &library, alpha, Kind::Cover)), (400, 600));
        assert_eq!(artwork.chosen(&library, alpha, Kind::Cover), Some(7));

        let candidates = wait_candidates(&artwork, &library, alpha);
        assert_eq!(candidates.iter().map(|c| c.id).collect::<Vec<_>>(), [7, 8]);
        let eight = &candidates[1];
        assert_eq!(eight.preview, format!("{CDN}/thumb/8.jpg"));
        assert_eq!(eight.author.as_deref(), Some("someone"));
        let preview = settle(
            &artwork,
            &library,
            || artwork.preview(eight),
            |a| matches!(a, Art::Loading),
        );
        assert_eq!(size(&preview), (60, 90));

        artwork.choose(&library, alpha, eight).unwrap();
        assert_eq!(artwork.chosen(&library, alpha, Kind::Cover), Some(8));
        assert_eq!(size(&wait(&artwork, &library, alpha, Kind::Cover)), (342, 482));
        drop((artwork, library));

        let fake = Fake::new();
        let (library, games) = self::library(dir.path());
        let artwork = self::artwork(dir.path(), &fake, true);
        assert_eq!(artwork.chosen(&library, &games[0], Kind::Cover), Some(8));
        assert_eq!(
            size(&wait(&artwork, &library, &games[0], Kind::Cover)),
            (342, 482)
        );
        assert_eq!(fake.calls(), 0);
    }

    #[test]
    fn artwork_choice_is_kept_over_a_lookup_still_on_its_way() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        let alpha = &games[0];
        fake.set_gate(false);
        assert!(matches!(artwork.cover(&library, alpha), Art::Loading));
        let eight = Candidate {
            id: 8,
            kind: Kind::Cover,
            url: format!("{CDN}/grid/8.png"),
            preview: format!("{CDN}/thumb/8.jpg"),
            width: 342,
            height: 482,
            author: None,
            game: 42,
        };
        artwork.choose(&library, alpha, &eight).unwrap();
        fake.set_gate(true);

        assert_eq!(size(&wait(&artwork, &library, alpha, Kind::Cover)), (342, 482));
        settle(&artwork, &library, || artwork.busy(), |busy| *busy);
        assert_eq!(artwork.chosen(&library, alpha, Kind::Cover), Some(8));
    }

    #[test]
    fn artwork_requests_return_at_once_and_arrive_by_polling() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        fake.set_gate(false);

        let asked = Instant::now();
        assert!(matches!(artwork.cover(&library, &games[0]), Art::Loading));
        assert!(matches!(artwork.hero(&library, &games[0]), Art::Loading));
        assert!(matches!(
            artwork.candidates(&library, &games[0], Kind::Cover),
            Candidates::Loading
        ));
        assert!(!artwork.poll(&library));
        assert!(artwork.busy());
        assert!(asked.elapsed() < Duration::from_secs(1));
        thread::sleep(Duration::from_millis(50));
        assert!(!artwork.poll(&library));
        assert!(matches!(artwork.cover(&library, &games[0]), Art::Loading));

        fake.set_gate(true);
        assert_eq!(
            size(&wait(&artwork, &library, &games[0], Kind::Cover)),
            (400, 600)
        );
    }

    #[test]
    fn artwork_beyond_the_memory_budget_is_read_again_from_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        fetch_all(dir.path(), &steamgriddb());

        let fake = Fake::new();
        let (library, games) = library(dir.path());
        let mut artwork = artwork(dir.path(), &fake, true);
        // Room for one cover.
        artwork.budget = 400 * 600 * 4;
        size(&wait(&artwork, &library, &games[0], Kind::Cover));
        size(&wait(&artwork, &library, &games[1], Kind::Cover));
        assert!(matches!(artwork.cover(&library, &games[0]), Art::Loading));
        size(&wait(&artwork, &library, &games[0], Kind::Cover));
        assert!(artwork.state.borrow().bytes <= artwork.budget);
        assert_eq!(fake.calls(), 0);
    }

    /// Renames library Game `game` to `title` and reads it back, as screens do after a rename.
    fn rename(library: &Library, game: &GameRow, title: &str) -> GameRow {
        library.rename(game.id, Some(title.to_owned())).unwrap();
        library.game(game.id).unwrap().unwrap()
    }

    /// "Gamma" on SteamGridDB: game 60 with cover 12 and hero 13.
    fn with_gamma(fake: &Fake) {
        fake.api("/search/autocomplete/Gamma", "[{\"id\":60}]")
            .api(
                "/grids/game/60?",
                &listing(&[(12, &format!("{CDN}/grid/12.png"))]),
            )
            .api(
                "/heroes/game/60?",
                &listing(&[(13, &format!("{CDN}/hero/13.png"))]),
            )
            .route(&format!("{CDN}/grid/12.png"), Ok(png(200, 300)))
            .route(&format!("{CDN}/hero/13.png"), Ok(png(1500, 500)));
    }

    #[test]
    fn artwork_matched_automatically_is_searched_again_under_a_new_display_title() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        with_gamma(&fake);
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        assert_eq!(
            size(&wait(&artwork, &library, &games[0], Kind::Cover)),
            (400, 600)
        );
        let calls = fake.calls();

        let gamma = rename(&library, &games[0], "Gamma");
        assert_eq!(
            artwork.chosen(&library, &gamma, Kind::Cover),
            None,
            "cover 7 was Alpha's"
        );
        assert_eq!(size(&wait(&artwork, &library, &gamma, Kind::Cover)), (200, 300));
        assert_eq!(
            fake.urls()[calls..],
            [
                format!("{API}/search/autocomplete/Gamma"),
                format!("{API}/grids/game/60?{COVER_QUERY}"),
                format!("{CDN}/grid/12.png"),
            ]
        );
        assert_eq!(artwork.chosen(&library, &gamma, Kind::Cover), Some(12));
        drop((artwork, library));

        // The new match is recorded: a restart fetches nothing.
        let fake = Fake::new();
        let (library, _) = self::library(dir.path());
        let gamma = library.game(gamma.id).unwrap().unwrap();
        let artwork = self::artwork(dir.path(), &fake, true);
        assert_eq!(size(&wait(&artwork, &library, &gamma, Kind::Cover)), (200, 300));
        assert_eq!(fake.calls(), 0);
    }

    #[test]
    fn artwork_no_match_is_searched_again_under_a_new_display_title() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new();
        fake.api("/search/autocomplete/Alpha", "[]");
        with_gamma(&fake);
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        assert!(is_placeholder(&wait(&artwork, &library, &games[0], Kind::Cover)));
        let gamma = rename(&library, &games[0], "Gamma");
        assert_eq!(size(&wait(&artwork, &library, &gamma, Kind::Cover)), (200, 300));
        assert_eq!(fake.urls()[1], format!("{API}/search/autocomplete/Gamma"));
    }

    #[test]
    fn artwork_the_player_chose_survives_a_new_display_title_without_fetching() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        with_gamma(&fake);
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        let alpha = &games[0];
        let candidates = wait_candidates(&artwork, &library, alpha);
        artwork.choose(&library, alpha, &candidates[1]).unwrap();
        assert_eq!(size(&wait(&artwork, &library, alpha, Kind::Cover)), (342, 482));
        let calls = fake.calls();

        let gamma = rename(&library, alpha, "Gamma");
        assert_eq!(size(&wait(&artwork, &library, &gamma, Kind::Cover)), (342, 482));
        assert_eq!(artwork.chosen(&library, &gamma, Kind::Cover), Some(8));
        assert_eq!(fake.calls(), calls, "the chosen cover is read from the cache");
        drop((artwork, library));

        // After a restart too; the hero, never chosen, is matched under the new title.
        let fake2 = steamgriddb();
        with_gamma(&fake2);
        let (library, _) = self::library(dir.path());
        let artwork = self::artwork(dir.path(), &fake2, true);
        assert_eq!(size(&wait(&artwork, &library, &gamma, Kind::Cover)), (342, 482));
        assert_eq!(fake2.calls(), 0);
        assert_eq!(size(&wait(&artwork, &library, &gamma, Kind::Hero)), (1500, 500));
        assert_eq!(fake2.urls()[0], format!("{API}/search/autocomplete/Gamma"));
        assert_eq!(
            artwork.chosen(&library, &gamma, Kind::Cover),
            Some(8),
            "and the choice stays"
        );
    }

    #[test]
    fn artwork_under_an_unchanged_display_title_makes_no_requests() {
        let dir = tempfile::tempdir().unwrap();
        let fake = steamgriddb();
        let (library, games) = library(dir.path());
        let artwork = artwork(dir.path(), &fake, true);
        size(&wait(&artwork, &library, &games[0], Kind::Cover));
        let calls = fake.calls();
        // Renamed to the title it already has, and read back afresh.
        let same = rename(&library, &games[0], "Alpha");
        size(&wait(&artwork, &library, &same, Kind::Cover));
        assert!(matches!(
            artwork.candidates(&library, &same, Kind::Cover),
            Candidates::Loading | Candidates::Ready(_)
        ));
        settle(&artwork, &library, || artwork.busy(), |busy| *busy);
        // Only the candidate listing, which needs no search: the match is known.
        assert_eq!(
            fake.urls()[calls..],
            [format!("{API}/grids/game/42?{COVER_QUERY}")]
        );
    }

    #[test]
    fn jpeg_decodes_and_other_files_do_not() {
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::new(1600, 650))
            .write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .unwrap();
        let picture = decode(&jpeg, Kind::Hero).unwrap();
        assert_eq!((picture.width, picture.height), (1600, 650));
        assert_eq!(picture.rgba.len(), 1600 * 650 * 4);
        assert!(decode(b"GIF89a not really", Kind::Cover).is_none());
        assert!(decode(&png(10, 10)[..20], Kind::Cover).is_none());
    }

    #[test]
    fn titles_are_percent_encoded() {
        assert_eq!(encode("Super Mario World"), "Super%20Mario%20World");
        assert_eq!(encode("Pokémon: Red/Blue?"), "Pok%C3%A9mon%3A%20Red%2FBlue%3F");
    }
}
