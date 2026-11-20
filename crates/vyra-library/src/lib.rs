//! VYRA music library: the database, the folder scanner and the artwork cache.

mod artwork;
mod scan;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

pub use scan::{ScanProgress, ScanSummary, is_audio, natural_key};

/// File extensions the audio engine can play today.
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "wav", "aiff", "aif", "m4a", "aac", "ogg", "oga"];

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("cannot prepare the data folder {}: {source}", path.display())]
    DataDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the folder does not exist: {}", .0.display())]
    MissingFolder(PathBuf),
}

type Result<T> = std::result::Result<T, LibraryError>;

#[derive(Clone, Debug, Serialize)]
pub struct Album {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    pub track_count: u32,
    pub duration_ms: u64,
    /// Hash of the cover image, or `None` when the album has no cover.
    pub artwork: Option<String>,
    /// Average colour of the cover as `#rrggbb`.
    pub color: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Track {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_id: Option<i64>,
    pub disc_no: Option<u32>,
    pub track_no: Option<u32>,
    pub duration_ms: u64,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub codec: String,
    pub artwork: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AlbumDetail {
    pub album: Album,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Artist {
    pub name: String,
    pub album_count: u32,
    pub track_count: u32,
    /// Cover of one of the albums, used as the artist picture.
    pub artwork: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SearchResults {
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Counts {
    pub tracks: u32,
    pub albums: u32,
    pub artists: u32,
}

const ALBUM_COLUMNS: &str = "id, title, artist, year, track_count, duration_ms, artwork, color";
const TRACK_COLUMNS: &str = "t.id, t.path, t.title, t.artist, t.album, t.album_id, t.disc_no, t.track_no, \
     t.duration_ms, t.sample_rate, t.bit_depth, t.codec, a.artwork";

/// The library of one installation. Safe to share between threads.
pub struct Library {
    connection: Mutex<Connection>,
    artwork_dir: PathBuf,
}

impl Library {
    /// Opens the library kept in `data_dir`, creating it on first use.
    pub fn open(data_dir: &Path) -> Result<Self> {
        let artwork_dir = data_dir.join("artwork");
        std::fs::create_dir_all(&artwork_dir)
            .map_err(|source| LibraryError::DataDir { path: artwork_dir.clone(), source })?;

        let connection = Connection::open(data_dir.join("library.db"))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&connection)?;
        Ok(Self { connection: Mutex::new(connection), artwork_dir })
    }

    fn with<T>(&self, work: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut guard = self.connection.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        work(&mut guard)
    }

    /// Folder the cached covers are in.
    pub fn artwork_dir(&self) -> &Path {
        &self.artwork_dir
    }

    /// Path of a cached cover in one of the stored sizes.
    pub fn artwork_file(&self, hash: &str, size: u32) -> PathBuf {
        artwork::file(&self.artwork_dir, hash, size)
    }

    pub fn roots(&self) -> Result<Vec<String>> {
        self.with(|c| {
            let mut statement = c.prepare("SELECT path FROM roots ORDER BY path")?;
            let rows = statement.query_map([], |row| row.get(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Adds a music folder. Scanning is a separate step.
    pub fn add_root(&self, path: &Path) -> Result<()> {
        if !path.is_dir() {
            return Err(LibraryError::MissingFolder(path.to_path_buf()));
        }
        let text = path.to_string_lossy().into_owned();
        self.with(|c| {
            c.execute("INSERT OR IGNORE INTO roots (path) VALUES (?1)", [&text])?;
            Ok(())
        })
    }

    /// Removes a music folder together with everything scanned from it.
    pub fn remove_root(&self, path: &str) -> Result<()> {
        self.with(|c| {
            let transaction = c.transaction()?;
            let id: Option<i64> =
                transaction.query_row("SELECT id FROM roots WHERE path = ?1", [path], |r| r.get(0)).optional()?;
            if let Some(id) = id {
                transaction.execute("DELETE FROM tracks WHERE root_id = ?1", [id])?;
                transaction.execute("DELETE FROM roots WHERE id = ?1", [id])?;
                scan::regroup(&transaction)?;
            }
            transaction.commit()?;
            Ok(())
        })
    }

    /// Reads a file that is not in the library, so it can be played straight from
    /// Explorer. Its cover is cached like any other.
    pub fn external_track(&self, path: &Path) -> Option<Track> {
        scan::external(self, path)
    }

    /// Scans every music folder: reads new and changed files, forgets removed ones,
    /// groups tracks into albums and caches covers.
    pub fn scan(&self, on_progress: &(dyn Fn(ScanProgress) + Sync)) -> Result<ScanSummary> {
        scan::run(self, on_progress)
    }

    pub fn counts(&self) -> Result<Counts> {
        self.with(|c| {
            Ok(Counts {
                tracks: c.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?,
                albums: c.query_row("SELECT COUNT(*) FROM albums", [], |r| r.get(0))?,
                artists: c.query_row("SELECT COUNT(DISTINCT artist) FROM albums", [], |r| r.get(0))?,
            })
        })
    }

    /// Albums ordered by artist and year.
    pub fn albums(&self) -> Result<Vec<Album>> {
        self.with(|c| {
            let sql = format!(
                "SELECT {ALBUM_COLUMNS} FROM albums ORDER BY artist COLLATE NOCASE, year, title COLLATE NOCASE"
            );
            let mut statement = c.prepare(&sql)?;
            let rows = statement.query_map([], album_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Albums added last, newest first.
    pub fn recent_albums(&self, limit: u32) -> Result<Vec<Album>> {
        self.with(|c| {
            let sql = format!("SELECT {ALBUM_COLUMNS} FROM albums ORDER BY added_at DESC, id LIMIT ?1");
            let mut statement = c.prepare(&sql)?;
            let rows = statement.query_map([limit], album_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn album(&self, id: i64) -> Result<Option<AlbumDetail>> {
        self.with(|c| {
            let sql = format!("SELECT {ALBUM_COLUMNS} FROM albums WHERE id = ?1");
            let Some(album) = c.query_row(&sql, [id], album_from_row).optional()? else {
                return Ok(None);
            };
            let sql = format!(
                "SELECT {TRACK_COLUMNS} FROM tracks t LEFT JOIN albums a ON a.id = t.album_id \
                 WHERE t.album_id = ?1 ORDER BY COALESCE(t.disc_no, 1), COALESCE(t.track_no, 9999), t.path"
            );
            let mut statement = c.prepare(&sql)?;
            let tracks = statement.query_map([id], track_from_row)?.collect::<rusqlite::Result<_>>()?;
            Ok(Some(AlbumDetail { album, tracks }))
        })
    }

    pub fn artists(&self) -> Result<Vec<Artist>> {
        self.with(|c| artists_where(c, "", &[]))
    }

    /// Albums of one artist, oldest first.
    pub fn artist_albums(&self, name: &str) -> Result<Vec<Album>> {
        self.with(|c| {
            let sql =
                format!("SELECT {ALBUM_COLUMNS} FROM albums WHERE artist = ?1 ORDER BY year, title COLLATE NOCASE");
            let mut statement = c.prepare(&sql)?;
            let rows = statement.query_map([name], album_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// One page of all tracks, ordered by artist, album and track number.
    pub fn tracks(&self, offset: u32, limit: u32) -> Result<Vec<Track>> {
        self.with(|c| {
            let sql = format!(
                "SELECT {TRACK_COLUMNS} FROM tracks t LEFT JOIN albums a ON a.id = t.album_id \
                 ORDER BY t.artist COLLATE NOCASE, t.album COLLATE NOCASE, COALESCE(t.disc_no, 1), \
                 COALESCE(t.track_no, 9999), t.path LIMIT ?2 OFFSET ?1"
            );
            let mut statement = c.prepare(&sql)?;
            let rows = statement.query_map([offset, limit], track_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Tracks by identifier, in the order the identifiers were given.
    pub fn tracks_by_ids(&self, ids: &[i64]) -> Result<Vec<Track>> {
        self.with(|c| {
            let sql =
                format!("SELECT {TRACK_COLUMNS} FROM tracks t LEFT JOIN albums a ON a.id = t.album_id WHERE t.id = ?1");
            let mut statement = c.prepare(&sql)?;
            let mut tracks = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(track) = statement.query_row([id], track_from_row).optional()? {
                    tracks.push(track);
                }
            }
            Ok(tracks)
        })
    }

    /// Looks the text up in titles, artists and albums. Case does not matter.
    pub fn search(&self, text: &str) -> Result<SearchResults> {
        let needle = text.trim().to_lowercase();
        if needle.is_empty() {
            return Ok(SearchResults::default());
        }
        let pattern = format!("%{}%", needle.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        self.with(|c| {
            let sql = format!(
                "SELECT {ALBUM_COLUMNS} FROM albums WHERE search LIKE ?1 ESCAPE '\\' \
                 ORDER BY title COLLATE NOCASE LIMIT 24"
            );
            let albums = c.prepare(&sql)?.query_map([&pattern], album_from_row)?.collect::<rusqlite::Result<_>>()?;

            let artists = artists_where(c, "WHERE artist_search LIKE ?1 ESCAPE '\\'", &[&pattern])?;

            let sql = format!(
                "SELECT {TRACK_COLUMNS} FROM tracks t LEFT JOIN albums a ON a.id = t.album_id \
                 WHERE t.search LIKE ?1 ESCAPE '\\' ORDER BY t.title COLLATE NOCASE LIMIT 60"
            );
            let tracks = c.prepare(&sql)?.query_map([&pattern], track_from_row)?.collect::<rusqlite::Result<_>>()?;

            Ok(SearchResults { albums, artists, tracks })
        })
    }
}

fn artists_where(c: &Connection, filter: &str, values: &[&dyn rusqlite::ToSql]) -> Result<Vec<Artist>> {
    let sql = format!(
        "SELECT artist, COUNT(*), SUM(track_count), \
                (SELECT artwork FROM albums b WHERE b.artist = albums.artist AND b.artwork IS NOT NULL \
                 ORDER BY b.year DESC LIMIT 1) \
         FROM albums {filter} GROUP BY artist ORDER BY artist COLLATE NOCASE"
    );
    let mut statement = c.prepare(&sql)?;
    let rows = statement.query_map(values, |row| {
        Ok(Artist { name: row.get(0)?, album_count: row.get(1)?, track_count: row.get(2)?, artwork: row.get(3)? })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn album_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Album> {
    Ok(Album {
        id: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        year: row.get(3)?,
        track_count: row.get(4)?,
        duration_ms: row.get::<_, i64>(5)?.max(0) as u64,
        artwork: row.get(6)?,
        color: row.get(7)?,
    })
}

fn track_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
        id: row.get(0)?,
        path: row.get(1)?,
        title: row.get(2)?,
        artist: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
        album: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        album_id: row.get(5)?,
        disc_no: row.get(6)?,
        track_no: row.get(7)?,
        duration_ms: row.get::<_, i64>(8)?.max(0) as u64,
        sample_rate: row.get(9)?,
        bit_depth: row.get(10)?,
        codec: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
        artwork: row.get(12)?,
    })
}

fn migrate(connection: &Connection) -> Result<()> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 1 {
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE roots (
               id   INTEGER PRIMARY KEY,
               path TEXT NOT NULL UNIQUE
             );
             CREATE TABLE tracks (
               id           INTEGER PRIMARY KEY,
               path         TEXT NOT NULL UNIQUE,
               root_id      INTEGER NOT NULL REFERENCES roots(id),
               folder       TEXT NOT NULL,
               size         INTEGER NOT NULL,
               mtime        INTEGER NOT NULL,
               title        TEXT NOT NULL,
               artist       TEXT,
               album_artist TEXT,
               album        TEXT,
               disc_no      INTEGER,
               track_no     INTEGER,
               year         INTEGER,
               genre        TEXT,
               compilation  INTEGER NOT NULL DEFAULT 0,
               duration_ms  INTEGER NOT NULL,
               sample_rate  INTEGER,
               bit_depth    INTEGER,
               channels     INTEGER,
               codec        TEXT,
               album_id     INTEGER,
               search       TEXT NOT NULL,
               added_at     INTEGER NOT NULL
             );
             CREATE INDEX tracks_by_album ON tracks (album_id, disc_no, track_no);
             CREATE INDEX tracks_by_root ON tracks (root_id);
             CREATE TABLE albums (
               id            INTEGER PRIMARY KEY,
               title         TEXT NOT NULL,
               artist        TEXT NOT NULL,
               year          INTEGER,
               folder        TEXT NOT NULL,
               from_folder   INTEGER NOT NULL DEFAULT 0,
               track_count   INTEGER NOT NULL,
               duration_ms   INTEGER NOT NULL,
               artwork       TEXT,
               color         TEXT,
               search        TEXT NOT NULL,
               artist_search TEXT NOT NULL,
               added_at      INTEGER NOT NULL
             );
             CREATE INDEX albums_by_artist ON albums (artist);
             PRAGMA user_version = 1;
             COMMIT;",
        )?;
    }
    Ok(())
}

pub(crate) fn connection_of(library: &Library) -> std::sync::MutexGuard<'_, Connection> {
    library.connection.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) use params as sql_params;
