//! The library database (`$XDG_DATA_HOME/game37/library.db`): every Game ever found under the Games
//! Root and what Game37 records about it (FR-LIB-7), plus UI state that persists across restarts.
//!
//! The database is replaceable (ADR 0005): losing it loses Playtime and flags, never Games.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Params, Row, params};

use super::FoundGame;
use crate::systems;

/// How many Games a Shelf shows on Home (FR-HOME-3).
pub const SHELF_SIZE: usize = 20;

/// Schema migrations, in order. Migration `i` takes the schema from `user_version` `i` to `i + 1`.
/// Append new ones; never edit one that has shipped.
const MIGRATIONS: &[&str] = &["
    CREATE TABLE games (
        id INTEGER PRIMARY KEY,
        system TEXT NOT NULL,
        rel_path TEXT NOT NULL UNIQUE,
        title TEXT NOT NULL,
        renamed_title TEXT,
        present INTEGER NOT NULL DEFAULT 1,
        date_added INTEGER NOT NULL,
        last_played INTEGER,
        playtime_secs INTEGER NOT NULL DEFAULT 0,
        favorited_at INTEGER,
        hidden INTEGER NOT NULL DEFAULT 0,
        artwork TEXT
    );
    CREATE INDEX games_system ON games (system);
    CREATE TABLE settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
"];

/// Games that appear in views: still on disk and not Hidden (FR-DET-6).
const VISIBLE: &str = "present = 1 AND hidden = 0";

const DEFAULT_SYSTEM: &str = "nes";

#[derive(Debug)]
pub enum DbError {
    CreateDir(PathBuf, std::io::Error),
    Sqlite(rusqlite::Error),
    /// The file's `user_version` is one this build doesn't know, e.g. written by a newer Game37.
    UnknownSchemaVersion(i64),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::CreateDir(path, e) => write!(f, "Can't create {}: {e}", path.display()),
            DbError::Sqlite(e) => write!(f, "Library database error: {e}"),
            DbError::UnknownSchemaVersion(v) => write!(f, "Library database has unknown schema version {v}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::Sqlite(e)
    }
}

/// The Shelves on Home, declared in the order Home shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    Favorites,
    ContinuePlaying,
    RecentlyAdded,
    MostPlayed,
}

impl Shelf {
    /// Every Shelf, in Home order.
    pub const ALL: [Shelf; 4] = [
        Shelf::Favorites,
        Shelf::ContinuePlaying,
        Shelf::RecentlyAdded,
        Shelf::MostPlayed,
    ];

    /// The Shelf's rule, as an SQL condition on top of [`VISIBLE`].
    fn filter(self) -> &'static str {
        match self {
            Shelf::Favorites => "favorited_at IS NOT NULL",
            Shelf::ContinuePlaying => "last_played IS NOT NULL",
            Shelf::RecentlyAdded => "1",
            Shelf::MostPlayed => "playtime_secs > 0",
        }
    }

    /// Primary sort key, ascending; ties fall back to Display Title and id.
    fn key(self, game: &Fetched) -> Reverse<Option<i64>> {
        match self {
            Shelf::Favorites => Reverse(game.favorited_at),
            Shelf::ContinuePlaying => SortOrder::LastPlayed.key(game),
            Shelf::RecentlyAdded => SortOrder::RecentlyAdded.key(game),
            Shelf::MostPlayed => SortOrder::MostPlayed.key(game),
        }
    }
}

/// The global Console Page sort order (FR-CAR-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    AZ,
    LastPlayed,
    MostPlayed,
    RecentlyAdded,
}

impl SortOrder {
    fn as_str(self) -> &'static str {
        match self {
            SortOrder::AZ => "az",
            SortOrder::LastPlayed => "last_played",
            SortOrder::MostPlayed => "most_played",
            SortOrder::RecentlyAdded => "recently_added",
        }
    }

    fn from_str(s: &str) -> Option<SortOrder> {
        [
            SortOrder::AZ,
            SortOrder::LastPlayed,
            SortOrder::MostPlayed,
            SortOrder::RecentlyAdded,
        ]
        .into_iter()
        .find(|o| o.as_str() == s)
    }

    /// The next order in the Sort action's cycle (FR-CAR-5), wrapping back to A–Z.
    pub fn next(self) -> SortOrder {
        match self {
            SortOrder::AZ => SortOrder::LastPlayed,
            SortOrder::LastPlayed => SortOrder::MostPlayed,
            SortOrder::MostPlayed => SortOrder::RecentlyAdded,
            SortOrder::RecentlyAdded => SortOrder::AZ,
        }
    }

    /// The label shown in a Console Page's header and sort chip.
    pub fn label(self) -> &'static str {
        match self {
            SortOrder::AZ => "A–Z",
            SortOrder::LastPlayed => "Last played",
            SortOrder::MostPlayed => "Most played",
            SortOrder::RecentlyAdded => "Recently added",
        }
    }

    /// Primary sort key, ascending; ties fall back to Display Title and id. `Reverse` puts the largest
    /// value first and `None` (never played) last.
    fn key(self, game: &Fetched) -> Reverse<Option<i64>> {
        match self {
            SortOrder::AZ => Reverse(None),
            SortOrder::LastPlayed => Reverse(game.row.last_played),
            SortOrder::MostPlayed => Reverse(Some(game.playtime_secs)),
            SortOrder::RecentlyAdded => Reverse(Some(game.row.date_added)),
        }
    }
}

/// One Game as views show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameRow {
    pub id: i64,
    /// The System's id.
    pub system: String,
    pub rel_path: String,
    /// The Display Title: the player's rename if there is one, else the cleaned title.
    pub title: String,
    /// Unix seconds.
    pub date_added: i64,
    /// Unix seconds.
    pub last_played: Option<i64>,
    pub playtime_secs: u64,
    pub favorite: bool,
}

/// A [`GameRow`] plus the columns only sorting needs.
struct Fetched {
    row: GameRow,
    playtime_secs: i64,
    favorited_at: Option<i64>,
}

impl Fetched {
    fn from_row(r: &Row) -> rusqlite::Result<Fetched> {
        let playtime_secs: i64 = r.get(6)?;
        let favorited_at: Option<i64> = r.get(7)?;
        Ok(Fetched {
            row: GameRow {
                id: r.get(0)?,
                system: r.get(1)?,
                rel_path: r.get(2)?,
                title: r.get(3)?,
                date_added: r.get(4)?,
                last_played: r.get(5)?,
                playtime_secs: playtime_secs.max(0) as u64,
                favorite: favorited_at.is_some(),
            },
            playtime_secs,
            favorited_at,
        })
    }
}

/// Sorts by `key`, then Display Title case-insensitively, then id, and keeps at most `limit`.
fn sorted(
    mut games: Vec<Fetched>,
    key: impl Fn(&Fetched) -> Reverse<Option<i64>>,
    limit: Option<usize>,
) -> Vec<GameRow> {
    games.sort_by_cached_key(|g| (key(g), g.row.title.to_lowercase(), g.row.id));
    let rows = games.into_iter().map(|g| g.row);
    match limit {
        Some(n) => rows.take(n).collect(),
        None => rows.collect(),
    }
}

pub struct Library {
    conn: Connection,
}

impl Library {
    /// Opens the database at `path`, creating it, its parent directories and its schema as needed.
    pub fn open(path: &Path) -> Result<Library, DbError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| DbError::CreateDir(dir.to_owned(), e))?;
        }
        Library::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Library, DbError> {
        Library::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Library, DbError> {
        migrate(&mut conn)?;
        Ok(Library { conn })
    }

    /// Reconciles one System's Games with a fresh scan of its folder; `found` must all belong to
    /// `system`. New Games are added with `now` as date added; Games no longer found are kept but
    /// leave every view, so their Playtime, flags and date added survive if they come back. Other
    /// Systems are untouched. Returns whether any view changed.
    pub fn sync_system(&mut self, system: &str, found: &[FoundGame], now: i64) -> Result<bool, DbError> {
        let tx = self.conn.transaction()?;
        let mut known: HashMap<String, Known> = {
            let mut stmt = tx.prepare(
                "SELECT rel_path, id, title, present, hidden, renamed_title IS NOT NULL
                 FROM games WHERE system = ?1",
            )?;
            stmt.query_map([system], |r| {
                Ok((
                    r.get(0)?,
                    Known {
                        id: r.get(1)?,
                        title: r.get(2)?,
                        present: r.get(3)?,
                        hidden: r.get(4)?,
                        renamed: r.get(5)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<_>>()?
        };

        let mut changed = false;
        for game in found {
            match known.remove(&game.rel_path) {
                None => {
                    tx.execute(
                        "INSERT INTO games (system, rel_path, title, date_added) VALUES (?1, ?2, ?3, ?4)",
                        params![system, game.rel_path, game.title, now],
                    )?;
                    changed = true;
                }
                Some(k) if !k.present || k.title != game.title => {
                    tx.execute(
                        "UPDATE games SET present = 1, title = ?2 WHERE id = ?1",
                        params![k.id, game.title],
                    )?;
                    changed |= !k.hidden && (!k.present || !k.renamed);
                }
                Some(_) => {}
            }
        }
        for k in known.values().filter(|k| k.present) {
            tx.execute("UPDATE games SET present = 0 WHERE id = ?1", [k.id])?;
            changed |= !k.hidden;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// A Shelf's Games in its order, at most `limit` of them (Home uses `Some(SHELF_SIZE)`).
    pub fn shelf(&self, shelf: Shelf, limit: Option<usize>) -> Result<Vec<GameRow>, DbError> {
        let games = self.fetch(shelf.filter(), [])?;
        Ok(sorted(games, |g| shelf.key(g), limit))
    }

    /// How many Games match a Shelf's rule, beyond the ones Home shows.
    pub fn shelf_count(&self, shelf: Shelf) -> Result<usize, DbError> {
        let sql = format!(
            "SELECT COUNT(*) FROM games WHERE {VISIBLE} AND {}",
            shelf.filter()
        );
        let n: i64 = self.conn.query_row(&sql, [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// A Console Page: one System's Games in the given order.
    pub fn system_games(&self, system: &str, order: SortOrder) -> Result<Vec<GameRow>, DbError> {
        let games = self.fetch("system = ?1", [system])?;
        Ok(sorted(games, |g| order.key(g), None))
    }

    /// One visible Game by id, e.g. for its Details Page to re-read; `None` once it is hidden or
    /// gone from its folder.
    pub fn game(&self, id: i64) -> Result<Option<GameRow>, DbError> {
        Ok(self.fetch("id = ?1", [id])?.pop().map(|g| g.row))
    }

    /// Visible Games per System id. A System with no visible Games is absent from the map.
    pub fn system_counts(&self) -> Result<HashMap<String, usize>, DbError> {
        let sql = format!("SELECT system, COUNT(*) FROM games WHERE {VISIBLE} GROUP BY system");
        let mut stmt = self.conn.prepare(&sql)?;
        let counts = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(counts)
    }

    fn fetch(&self, filter: &str, params: impl Params) -> Result<Vec<Fetched>, DbError> {
        let sql = format!(
            "SELECT id, system, rel_path, COALESCE(renamed_title, title), date_added, last_played,
                    playtime_secs, favorited_at
             FROM games WHERE {VISIBLE} AND {filter}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let games = stmt
            .query_map(params, Fetched::from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(games)
    }

    /// Favorites or unfavorites a Game. Favoriting a Favorite keeps its original time. The limit
    /// of 5 Favorites (FR-HOME-6) is the caller's to enforce.
    pub fn set_favorite(&self, id: i64, favorite: bool, now: i64) -> Result<(), DbError> {
        if favorite {
            self.conn.execute(
                "UPDATE games SET favorited_at = COALESCE(favorited_at, ?2) WHERE id = ?1",
                [id, now],
            )?;
        } else {
            self.conn
                .execute("UPDATE games SET favorited_at = NULL WHERE id = ?1", [id])?;
        }
        Ok(())
    }

    /// Every Favorite, newest first, each with whether its file is there: the limit of 5
    /// (FR-HOME-6) counts them all, so a Favorite whose file is away for now (a drive unplugged)
    /// keeps its place, and the replace picker offers it too.
    pub fn all_favorites(&self) -> Result<Vec<(GameRow, bool)>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, system, rel_path, COALESCE(renamed_title, title), date_added, last_played,
                    playtime_secs, favorited_at, present
             FROM games WHERE favorited_at IS NOT NULL",
        )?;
        let mut games = stmt
            .query_map([], |r| Ok((Fetched::from_row(r)?, r.get::<_, bool>(8)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        games.sort_by_cached_key(|(g, _)| (Shelf::Favorites.key(g), g.row.title.to_lowercase(), g.row.id));
        Ok(games.into_iter().map(|(g, present)| (g.row, present)).collect())
    }

    /// Records a play session that ended at `ended_at` and lasted `secs`.
    pub fn record_play(&self, id: i64, ended_at: i64, secs: u64) -> Result<(), DbError> {
        let secs = i64::try_from(secs).unwrap_or(i64::MAX);
        self.conn.execute(
            "UPDATE games SET last_played = ?2, playtime_secs = playtime_secs + ?3 WHERE id = ?1",
            [id, ended_at, secs],
        )?;
        Ok(())
    }

    pub fn set_hidden(&self, id: i64, hidden: bool) -> Result<(), DbError> {
        self.conn
            .execute("UPDATE games SET hidden = ?2 WHERE id = ?1", params![id, hidden])?;
        Ok(())
    }

    /// Sets the player's title for a Game; `None` restores the cleaned title.
    /// Game `id`'s cleaned-up title from its file or folder name, which a rename overrides.
    pub fn cleaned_title(&self, id: i64) -> Result<Option<String>, DbError> {
        Ok(self
            .conn
            .query_row("SELECT title FROM games WHERE id = ?1", [id], |r| r.get(0))
            .optional()?)
    }

    pub fn rename(&self, id: i64, title: Option<String>) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE games SET renamed_title = ?2 WHERE id = ?1",
            params![id, title],
        )?;
        Ok(())
    }

    /// What Game `id` records about its Artwork (FR-LIB-7), in the form `artwork.rs` writes it, or
    /// `None` if nothing is recorded yet.
    pub fn artwork(&self, id: i64) -> Result<Option<String>, DbError> {
        let value = self
            .conn
            .query_row("SELECT artwork FROM games WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        Ok(value.flatten())
    }

    pub fn set_artwork(&self, id: i64, record: &str) -> Result<(), DbError> {
        self.conn
            .execute("UPDATE games SET artwork = ?2 WHERE id = ?1", params![id, record])?;
        Ok(())
    }

    /// The global Console Page sort order; A–Z until changed.
    pub fn sort_order(&self) -> Result<SortOrder, DbError> {
        Ok(self
            .setting("sort_order")?
            .as_deref()
            .and_then(SortOrder::from_str)
            .unwrap_or_default())
    }

    pub fn set_sort_order(&self, order: SortOrder) -> Result<(), DbError> {
        self.set_setting("sort_order", order.as_str())
    }

    /// The System the Carousel was last on; NES if none was stored or the stored one is unknown.
    pub fn last_system(&self) -> Result<&'static str, DbError> {
        let stored = self.setting("last_system")?;
        let system = stored
            .as_deref()
            .and_then(systems::by_id)
            .map_or(DEFAULT_SYSTEM, |s| s.id);
        Ok(system)
    }

    pub fn set_last_system(&self, system: &str) -> Result<(), DbError> {
        self.set_setting("last_system", system)
    }

    fn setting(&self, key: &str) -> Result<Option<String>, DbError> {
        let value = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()?;
        Ok(value)
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }
}

/// What `sync_system` needs to know about a Game already in the database.
struct Known {
    id: i64,
    title: String,
    present: bool,
    hidden: bool,
    renamed: bool,
}

/// Brings the schema up to the latest version, one migration per transaction.
fn migrate(conn: &mut Connection) -> Result<(), DbError> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let start = usize::try_from(version)
        .ok()
        .filter(|&v| v <= MIGRATIONS.len())
        .ok_or(DbError::UnknownSchemaVersion(version))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(start) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(system: &'static str, title: &str) -> FoundGame {
        FoundGame {
            system,
            rel_path: format!("{system}/{title}.zip"),
            title: title.to_owned(),
        }
    }

    fn games(system: &'static str, titles: &[&str]) -> Vec<FoundGame> {
        titles.iter().map(|t| game(system, t)).collect()
    }

    fn titles(rows: &[GameRow]) -> Vec<&str> {
        rows.iter().map(|r| r.title.as_str()).collect()
    }

    fn id_of(lib: &Library, system: &str, title: &str) -> i64 {
        let rows = lib.system_games(system, SortOrder::AZ).unwrap();
        rows.iter()
            .find(|r| r.title == title)
            .unwrap_or_else(|| panic!("{title} not visible"))
            .id
    }

    #[test]
    fn sync_inserts_new_games_with_date_added() {
        let mut lib = Library::open_in_memory().unwrap();
        assert!(
            lib.sync_system("snes", &games("snes", &["Alpha", "Beta"]), 100)
                .unwrap()
        );

        let rows = lib.system_games("snes", SortOrder::AZ).unwrap();
        assert_eq!(titles(&rows), ["Alpha", "Beta"]);
        let alpha = &rows[0];
        assert_eq!(alpha.system, "snes");
        assert_eq!(alpha.rel_path, "snes/Alpha.zip");
        assert_eq!(alpha.date_added, 100);
        assert_eq!(alpha.last_played, None);
        assert_eq!(alpha.playtime_secs, 0);
        assert!(!alpha.favorite);

        assert!(
            lib.sync_system("snes", &games("snes", &["Alpha", "Beta", "Gamma"]), 200)
                .unwrap()
        );
        let dates: Vec<_> = lib
            .system_games("snes", SortOrder::AZ)
            .unwrap()
            .iter()
            .map(|r| r.date_added)
            .collect();
        assert_eq!(dates, [100, 100, 200]);
    }

    #[test]
    fn sync_without_changes_reports_none() {
        let mut lib = Library::open_in_memory().unwrap();
        let found = games("snes", &["Alpha"]);
        lib.sync_system("snes", &found, 100).unwrap();
        assert!(!lib.sync_system("snes", &found, 200).unwrap());
        assert_eq!(
            lib.system_games("snes", SortOrder::AZ).unwrap()[0].date_added,
            100
        );
    }

    #[test]
    fn sync_updates_cleaned_title() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.sync_system("snes", &games("snes", &["Alpha"]), 100).unwrap();
        let retitled = FoundGame {
            title: "Alpha Remix".to_owned(),
            ..game("snes", "Alpha")
        };
        assert!(lib.sync_system("snes", &[retitled], 200).unwrap());
        let rows = lib.system_games("snes", SortOrder::AZ).unwrap();
        assert_eq!(titles(&rows), ["Alpha Remix"]);
        assert_eq!(rows[0].date_added, 100);
    }

    #[test]
    fn removed_game_is_absent_and_returns_with_its_record() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.sync_system("snes", &games("snes", &["Alpha", "Beta"]), 100)
            .unwrap();
        let alpha = id_of(&lib, "snes", "Alpha");
        lib.record_play(alpha, 150, 60).unwrap();
        lib.set_favorite(alpha, true, 160).unwrap();

        assert!(lib.sync_system("snes", &games("snes", &["Beta"]), 200).unwrap());
        assert_eq!(
            titles(&lib.system_games("snes", SortOrder::AZ).unwrap()),
            ["Beta"]
        );

        assert!(
            lib.sync_system("snes", &games("snes", &["Alpha", "Beta"]), 300)
                .unwrap()
        );
        let rows = lib.system_games("snes", SortOrder::AZ).unwrap();
        let back = &rows[0];
        assert_eq!((back.id, back.title.as_str()), (alpha, "Alpha"));
        assert_eq!(back.date_added, 100);
        assert_eq!(back.playtime_secs, 60);
        assert_eq!(back.last_played, Some(150));
        assert!(back.favorite);
    }

    #[test]
    fn sync_of_one_system_leaves_others_untouched() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.sync_system("nes", &games("nes", &["Kid"]), 100).unwrap();
        lib.sync_system("snes", &games("snes", &["Alpha"]), 100).unwrap();

        assert!(lib.sync_system("snes", &[], 200).unwrap());
        assert_eq!(titles(&lib.system_games("nes", SortOrder::AZ).unwrap()), ["Kid"]);
        assert!(lib.system_games("snes", SortOrder::AZ).unwrap().is_empty());
        assert_eq!(
            lib.system_counts().unwrap(),
            HashMap::from([("nes".to_owned(), 1)])
        );
    }

    #[test]
    fn changes_to_hidden_games_are_not_visible() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.sync_system("snes", &games("snes", &["Alpha", "Beta"]), 100)
            .unwrap();
        lib.set_hidden(id_of(&lib, "snes", "Alpha"), true).unwrap();
        assert!(!lib.sync_system("snes", &games("snes", &["Beta"]), 200).unwrap());
        assert!(
            !lib.sync_system("snes", &games("snes", &["Alpha", "Beta"]), 300)
                .unwrap()
        );
    }

    #[test]
    fn retitling_a_renamed_game_is_not_visible() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.sync_system("snes", &games("snes", &["Alpha"]), 100).unwrap();
        lib.rename(id_of(&lib, "snes", "Alpha"), Some("Mine".to_owned()))
            .unwrap();
        let retitled = FoundGame {
            title: "Alpha Remix".to_owned(),
            ..game("snes", "Alpha")
        };
        assert!(!lib.sync_system("snes", &[retitled], 200).unwrap());
        assert_eq!(
            titles(&lib.system_games("snes", SortOrder::AZ).unwrap()),
            ["Mine"]
        );
    }

    #[test]
    fn favorites_shelf_is_most_recently_favorited_first() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.sync_system(
            "snes",
            &games("snes", &["Alpha", "Beta", "Gamma", "Delta", "eps", "Zed"]),
            1,
        )
        .unwrap();
        for (title, at) in [
            ("Alpha", 10),
            ("Beta", 30),
            ("Gamma", 20),
            ("Delta", 5),
            ("eps", 5),
        ] {
            lib.set_favorite(id_of(&lib, "snes", title), true, at).unwrap();
        }
        // Favoriting a Favorite again keeps its original time.
        lib.set_favorite(id_of(&lib, "snes", "Alpha"), true, 99).unwrap();
        lib.set_favorite(id_of(&lib, "snes", "Gamma"), false, 40).unwrap();

        let shelf = lib.shelf(Shelf::Favorites, None).unwrap();
        assert_eq!(titles(&shelf), ["Beta", "Alpha", "Delta", "eps"]);
        assert!(shelf.iter().all(|r| r.favorite));
        assert_eq!(lib.shelf_count(Shelf::Favorites).unwrap(), 4);
    }

    #[test]
    fn all_favorites_include_absent_ones_newest_first() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.sync_system("snes", &games("snes", &["A", "B", "C"]), 0)
            .unwrap();
        let (a, b) = (id_of(&lib, "snes", "A"), id_of(&lib, "snes", "B"));
        lib.set_favorite(a, true, 1).unwrap();
        lib.set_favorite(b, true, 2).unwrap();
        lib.sync_system("snes", &games("snes", &["B", "C"]), 0).unwrap();
        let all: Vec<_> = lib
            .all_favorites()
            .unwrap()
            .into_iter()
            .map(|(g, present)| (g.title, present))
            .collect();
        assert_eq!(all, [("B".to_owned(), true), ("A".to_owned(), false)]);
        assert_eq!(
            lib.shelf_count(Shelf::Favorites).unwrap(),
            1,
            "the Shelf shows B only"
        );
    }

    #[test]
    fn continue_playing_shelf_is_played_games_most_recent_first() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.sync_system("snes", &games("snes", &["Alpha", "Beta", "Gamma", "Never"]), 1)
            .unwrap();
        lib.record_play(id_of(&lib, "snes", "Alpha"), 100, 10).unwrap();
        lib.record_play(id_of(&lib, "snes", "Gamma"), 300, 10).unwrap();
        lib.record_play(id_of(&lib, "snes", "Beta"), 100, 10).unwrap();
        lib.record_play(id_of(&lib, "snes", "Alpha"), 200, 15).unwrap();

        let shelf = lib.shelf(Shelf::ContinuePlaying, None).unwrap();
        assert_eq!(titles(&shelf), ["Gamma", "Alpha", "Beta"]);
        assert_eq!(shelf[1].last_played, Some(200));
        assert_eq!(shelf[1].playtime_secs, 25);
    }

    #[test]
    fn recently_added_shelf_is_newest_first() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.sync_system("snes", &games("snes", &["Old"]), 100).unwrap();
        lib.sync_system("nes", &games("nes", &["mid", "Mid2"]), 200)
            .unwrap();
        lib.sync_system("snes", &games("snes", &["Old", "New"]), 300)
            .unwrap();

        let shelf = lib.shelf(Shelf::RecentlyAdded, None).unwrap();
        assert_eq!(titles(&shelf), ["New", "mid", "Mid2", "Old"]);
    }

    #[test]
    fn most_played_shelf_is_played_games_most_playtime_first() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.sync_system("snes", &games("snes", &["Alpha", "Beta", "Gamma", "Zero"]), 1)
            .unwrap();
        lib.record_play(id_of(&lib, "snes", "Alpha"), 100, 50).unwrap();
        lib.record_play(id_of(&lib, "snes", "Beta"), 100, 500).unwrap();
        lib.record_play(id_of(&lib, "snes", "Gamma"), 100, 50).unwrap();
        lib.record_play(id_of(&lib, "snes", "Zero"), 100, 0).unwrap();

        let shelf = lib.shelf(Shelf::MostPlayed, None).unwrap();
        assert_eq!(titles(&shelf), ["Beta", "Alpha", "Gamma"]);
        assert_eq!(lib.shelf_count(Shelf::MostPlayed).unwrap(), 3);
    }

    #[test]
    fn shelves_show_at_most_20_and_count_all() {
        let mut lib = Library::open_in_memory().unwrap();
        let mut found = Vec::new();
        for i in 0..25 {
            found.push(game("snes", &format!("Game {i:02}")));
            lib.sync_system("snes", &found, i).unwrap();
        }
        for i in 0..25 {
            let id = id_of(&lib, "snes", &format!("Game {i:02}"));
            lib.set_favorite(id, true, i).unwrap();
            lib.record_play(id, i, i as u64 + 1).unwrap();
        }
        for shelf in Shelf::ALL {
            let rows = lib.shelf(shelf, Some(SHELF_SIZE)).unwrap();
            assert_eq!(rows.len(), 20, "{shelf:?}");
            // Every rule here puts the latest Game first.
            assert_eq!(rows[0].title, "Game 24", "{shelf:?}");
            assert_eq!(rows[19].title, "Game 05", "{shelf:?}");
            assert_eq!(lib.shelf(shelf, None).unwrap().len(), 25, "{shelf:?}");
            assert_eq!(lib.shelf_count(shelf).unwrap(), 25, "{shelf:?}");
        }
    }

    #[test]
    fn hidden_and_absent_games_appear_in_no_view() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.sync_system("snes", &games("snes", &["Shown", "Hidden", "Gone"]), 100)
            .unwrap();
        for title in ["Shown", "Hidden", "Gone"] {
            let id = id_of(&lib, "snes", title);
            lib.set_favorite(id, true, 100).unwrap();
            lib.record_play(id, 100, 10).unwrap();
        }
        let hidden = id_of(&lib, "snes", "Hidden");
        lib.set_hidden(hidden, true).unwrap();
        lib.sync_system("snes", &games("snes", &["Shown", "Hidden"]), 200)
            .unwrap();

        for shelf in Shelf::ALL {
            assert_eq!(titles(&lib.shelf(shelf, None).unwrap()), ["Shown"], "{shelf:?}");
            assert_eq!(lib.shelf_count(shelf).unwrap(), 1, "{shelf:?}");
        }
        for order in [
            SortOrder::AZ,
            SortOrder::LastPlayed,
            SortOrder::MostPlayed,
            SortOrder::RecentlyAdded,
        ] {
            assert_eq!(
                titles(&lib.system_games("snes", order).unwrap()),
                ["Shown"],
                "{order:?}"
            );
        }
        assert_eq!(
            lib.system_counts().unwrap(),
            HashMap::from([("snes".to_owned(), 1)])
        );

        lib.set_hidden(hidden, false).unwrap();
        assert_eq!(
            titles(&lib.system_games("snes", SortOrder::AZ).unwrap()),
            ["Hidden", "Shown"]
        );
    }

    #[test]
    fn a_game_is_found_by_id_with_its_record_until_hidden() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.sync_system("snes", &games("snes", &["Alpha", "Beta"]), 100)
            .unwrap();
        let id = id_of(&lib, "snes", "Beta");
        lib.record_play(id, 300, 90).unwrap();

        let game = lib.game(id).unwrap().expect("Beta is visible");
        assert_eq!((game.title.as_str(), game.playtime_secs), ("Beta", 90));
        assert_eq!(game.last_played, Some(300));
        assert_eq!(lib.game(id + 100).unwrap(), None, "no such Game");
        lib.set_hidden(id, true).unwrap();
        assert_eq!(lib.game(id).unwrap(), None, "a Hidden Game");
    }

    #[test]
    fn sort_order_cycles_and_wraps() {
        assert_eq!(SortOrder::AZ.next(), SortOrder::LastPlayed);
        assert_eq!(SortOrder::LastPlayed.next(), SortOrder::MostPlayed);
        assert_eq!(SortOrder::MostPlayed.next(), SortOrder::RecentlyAdded);
        assert_eq!(
            SortOrder::RecentlyAdded.next(),
            SortOrder::AZ,
            "wraps back to A–Z"
        );
    }

    #[test]
    fn console_page_sort_orders() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.sync_system("snes", &games("snes", &["beta", "Alpha"]), 100)
            .unwrap();
        lib.sync_system("snes", &games("snes", &["beta", "Alpha", "Gamma", "delta"]), 200)
            .unwrap();
        lib.record_play(id_of(&lib, "snes", "beta"), 500, 30).unwrap();
        lib.record_play(id_of(&lib, "snes", "Gamma"), 400, 90).unwrap();
        lib.record_play(id_of(&lib, "snes", "Alpha"), 400, 30).unwrap();

        let order = |o| {
            lib.system_games("snes", o)
                .unwrap()
                .into_iter()
                .map(|r| r.title)
                .collect::<Vec<_>>()
        };
        assert_eq!(order(SortOrder::AZ), ["Alpha", "beta", "delta", "Gamma"]);
        assert_eq!(order(SortOrder::LastPlayed), ["beta", "Alpha", "Gamma", "delta"]);
        assert_eq!(order(SortOrder::MostPlayed), ["Gamma", "Alpha", "beta", "delta"]);
        assert_eq!(
            order(SortOrder::RecentlyAdded),
            ["delta", "Gamma", "Alpha", "beta"]
        );
    }

    #[test]
    fn rename_overrides_title_until_cleared() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.sync_system("snes", &games("snes", &["Alpha", "Beta"]), 100)
            .unwrap();
        let alpha = id_of(&lib, "snes", "Alpha");

        lib.rename(alpha, Some("Zulu".to_owned())).unwrap();
        assert_eq!(
            titles(&lib.system_games("snes", SortOrder::AZ).unwrap()),
            ["Beta", "Zulu"]
        );
        assert_eq!(
            titles(&lib.shelf(Shelf::RecentlyAdded, None).unwrap()),
            ["Beta", "Zulu"]
        );

        lib.rename(alpha, None).unwrap();
        assert_eq!(
            titles(&lib.system_games("snes", SortOrder::AZ).unwrap()),
            ["Alpha", "Beta"]
        );
    }

    #[test]
    fn settings_default_and_persist_across_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game37").join("library.db");
        {
            let lib = Library::open(&path).unwrap();
            assert_eq!(lib.sort_order().unwrap(), SortOrder::AZ);
            assert_eq!(lib.last_system().unwrap(), "nes");
            lib.set_sort_order(SortOrder::MostPlayed).unwrap();
            lib.set_last_system("ps2").unwrap();
        }
        let lib = Library::open(&path).unwrap();
        assert_eq!(lib.sort_order().unwrap(), SortOrder::MostPlayed);
        assert_eq!(lib.last_system().unwrap(), "ps2");
    }

    #[test]
    fn unknown_last_system_falls_back_to_nes() {
        let lib = Library::open_in_memory().unwrap();
        lib.set_last_system("dreamcast").unwrap();
        assert_eq!(lib.last_system().unwrap(), "nes");
    }

    #[test]
    fn reopening_a_file_keeps_games_and_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        {
            let mut lib = Library::open(&path).unwrap();
            lib.sync_system("snes", &games("snes", &["Alpha"]), 100).unwrap();
            let alpha = id_of(&lib, "snes", "Alpha");
            lib.record_play(alpha, 150, 60).unwrap();
            lib.set_favorite(alpha, true, 160).unwrap();
            lib.rename(alpha, Some("Mine".to_owned())).unwrap();
        }
        let mut lib = Library::open(&path).unwrap();
        let rows = lib.system_games("snes", SortOrder::AZ).unwrap();
        assert_eq!(titles(&rows), ["Mine"]);
        assert_eq!(
            (rows[0].date_added, rows[0].last_played, rows[0].playtime_secs),
            (100, Some(150), 60)
        );
        assert!(rows[0].favorite);
        assert!(!lib.sync_system("snes", &games("snes", &["Alpha"]), 200).unwrap());
    }

    #[test]
    fn artwork_record_persists_across_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let id = {
            let mut lib = Library::open(&path).unwrap();
            lib.sync_system("snes", &games("snes", &["Alpha"]), 100).unwrap();
            let id = id_of(&lib, "snes", "Alpha");
            assert_eq!(lib.artwork(id).unwrap(), None);
            lib.set_artwork(id, "{\"game\":null}").unwrap();
            id
        };
        let lib = Library::open(&path).unwrap();
        assert_eq!(lib.artwork(id).unwrap().as_deref(), Some("{\"game\":null}"));
        assert_eq!(lib.artwork(id + 1).unwrap(), None);
    }

    #[test]
    fn schema_version_is_recorded_and_unknown_versions_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        drop(Library::open(&path).unwrap());
        let conn = Connection::open(&path).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);

        conn.pragma_update(None, "user_version", 99).unwrap();
        drop(conn);
        assert!(matches!(
            Library::open(&path),
            Err(DbError::UnknownSchemaVersion(99))
        ));
    }
}
