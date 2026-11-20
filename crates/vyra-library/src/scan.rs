//! The folder scanner: finds audio files, reads their tags and groups them into albums.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use lofty::file::{AudioFile, FileType, TaggedFileExt};
use lofty::tag::{Accessor, ItemKey};
use rayon::prelude::*;
use rusqlite::Connection;
use serde::Serialize;

use crate::{AUDIO_EXTENSIONS, Library, Result, artwork, connection_of, sql_params};

/// Name given to albums whose tracks have different artists and no album artist.
const VARIOUS_ARTISTS: &str = "Разные исполнители";

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum ScanProgress {
    /// Walking the folders.
    Listing { files: usize },
    /// Reading tags of new and changed files.
    Reading { done: usize, total: usize },
    /// Caching covers.
    Artwork { done: usize, total: usize },
    Done { summary: ScanSummary },
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct ScanSummary {
    pub tracks: u32,
    pub albums: u32,
    pub added: u32,
    pub removed: u32,
    pub unreadable: u32,
}

struct Found {
    root_id: i64,
    path: PathBuf,
    size: i64,
    mtime: i64,
}

struct Parsed {
    root_id: i64,
    path: String,
    folder: String,
    size: i64,
    mtime: i64,
    title: String,
    artist: Option<String>,
    album_artist: Option<String>,
    album: Option<String>,
    disc_no: Option<u32>,
    track_no: Option<u32>,
    year: Option<i32>,
    genre: Option<String>,
    compilation: bool,
    duration_ms: i64,
    sample_rate: Option<u32>,
    bit_depth: Option<u32>,
    channels: Option<u32>,
    codec: &'static str,
}

pub(crate) fn run(library: &Library, on_progress: &(dyn Fn(ScanProgress) + Sync)) -> Result<ScanSummary> {
    // What the database already knows.
    let (roots, known) = {
        let connection = connection_of(library);
        let roots: Vec<(i64, String)> = connection
            .prepare("SELECT id, path FROM roots")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let known: HashMap<String, (i64, i64, i64)> = connection
            .prepare("SELECT path, size, mtime, root_id FROM tracks")?
            .query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?, row.get(3)?))))?
            .collect::<rusqlite::Result<_>>()?;
        (roots, known)
    };

    // Walk the folders that are reachable. A missing root keeps its tracks: the disk may
    // simply be unplugged.
    let mut found = Vec::new();
    let mut online_roots = HashSet::new();
    for (root_id, root) in &roots {
        let root_path = Path::new(root);
        if !root_path.is_dir() {
            continue;
        }
        online_roots.insert(*root_id);
        for entry in walkdir::WalkDir::new(root_path).into_iter().filter_map(std::result::Result::ok) {
            if !entry.file_type().is_file() || !is_audio(entry.path()) {
                continue;
            }
            let Ok(metadata) = entry.metadata() else { continue };
            found.push(Found {
                root_id: *root_id,
                path: entry.into_path(),
                size: i64::try_from(metadata.len()).unwrap_or(i64::MAX),
                mtime: metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs() as i64),
            });
            if found.len().is_multiple_of(500) {
                on_progress(ScanProgress::Listing { files: found.len() });
            }
        }
    }
    on_progress(ScanProgress::Listing { files: found.len() });

    let seen: HashSet<String> = found.iter().map(|f| f.path.to_string_lossy().into_owned()).collect();
    let changed: Vec<&Found> = found
        .iter()
        .filter(|f| {
            known.get(f.path.to_string_lossy().as_ref()).is_none_or(|(size, mtime, _)| *size != f.size || *mtime != f.mtime)
        })
        .collect();

    // Read tags in parallel. The database is not locked meanwhile.
    let total = changed.len();
    let done = AtomicUsize::new(0);
    let parsed: Vec<Option<Parsed>> = changed
        .par_iter()
        .map(|file| {
            let result = parse(file);
            let count = done.fetch_add(1, Ordering::Relaxed) + 1;
            if count.is_multiple_of(25) || count == total {
                on_progress(ScanProgress::Reading { done: count, total });
            }
            result
        })
        .collect();
    let unreadable = parsed.iter().filter(|p| p.is_none()).count() as u32;

    let removed_paths: Vec<&String> = known
        .iter()
        .filter(|(path, (_, _, root_id))| online_roots.contains(root_id) && !seen.contains(*path))
        .map(|(path, _)| path)
        .collect();
    let added = parsed.iter().flatten().filter(|p| !known.contains_key(&p.path)).count() as u32;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);

    // Write everything in one transaction.
    let without_cover: Vec<(i64, String, String)> = {
        let mut connection = connection_of(library);
        let transaction = connection.transaction()?;
        {
            let mut upsert = transaction.prepare(
                "INSERT INTO tracks (path, root_id, folder, size, mtime, title, artist, album_artist, album, disc_no,
                                     track_no, year, genre, compilation, duration_ms, sample_rate, bit_depth, channels,
                                     codec, search, added_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
                 ON CONFLICT(path) DO UPDATE SET
                   root_id = excluded.root_id, folder = excluded.folder, size = excluded.size, mtime = excluded.mtime,
                   title = excluded.title, artist = excluded.artist, album_artist = excluded.album_artist,
                   album = excluded.album, disc_no = excluded.disc_no, track_no = excluded.track_no,
                   year = excluded.year, genre = excluded.genre, compilation = excluded.compilation,
                   duration_ms = excluded.duration_ms, sample_rate = excluded.sample_rate,
                   bit_depth = excluded.bit_depth, channels = excluded.channels, codec = excluded.codec,
                   search = excluded.search",
            )?;
            for track in parsed.iter().flatten() {
                let search = [track.title.as_str(), track.artist.as_deref().unwrap_or(""), track.album.as_deref().unwrap_or("")]
                    .join(" ")
                    .to_lowercase();
                upsert.execute(sql_params![
                    track.path,
                    track.root_id,
                    track.folder,
                    track.size,
                    track.mtime,
                    track.title,
                    track.artist,
                    track.album_artist,
                    track.album,
                    track.disc_no,
                    track.track_no,
                    track.year,
                    track.genre,
                    track.compilation,
                    track.duration_ms,
                    track.sample_rate,
                    track.bit_depth,
                    track.channels,
                    track.codec,
                    search,
                    now,
                ])?;
            }
            let mut delete = transaction.prepare("DELETE FROM tracks WHERE path = ?1")?;
            for path in &removed_paths {
                delete.execute([path])?;
            }
        }
        regroup(&transaction)?;
        let without_cover = transaction
            .prepare(
                "SELECT a.id, a.folder, (SELECT path FROM tracks t WHERE t.album_id = a.id
                               ORDER BY COALESCE(t.disc_no, 1), COALESCE(t.track_no, 9999), t.path LIMIT 1)
                 FROM albums a WHERE a.artwork IS NULL",
            )?
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?))
            })?
            .filter_map(|row| match row {
                Ok((id, folder, Some(path))) => Some(Ok((id, folder, path))),
                Ok((_, _, None)) => None,
                Err(e) => Some(Err(e)),
            })
            .collect::<rusqlite::Result<_>>()?;
        transaction.commit()?;
        without_cover
    };

    // Covers are found and shrunk outside the lock, then recorded.
    let total = without_cover.len();
    let done = AtomicUsize::new(0);
    let covers: Vec<(i64, String, String)> = without_cover
        .par_iter()
        .filter_map(|(album_id, folder, track_path)| {
            let cover = artwork::find(Path::new(track_path), Path::new(folder))
                .and_then(|bytes| artwork::store(library.artwork_dir(), &bytes));
            let count = done.fetch_add(1, Ordering::Relaxed) + 1;
            if count.is_multiple_of(10) || count == total {
                on_progress(ScanProgress::Artwork { done: count, total });
            }
            cover.map(|(hash, color)| (*album_id, hash, color))
        })
        .collect();

    let summary = {
        let mut connection = connection_of(library);
        let transaction = connection.transaction()?;
        {
            let mut update = transaction.prepare("UPDATE albums SET artwork = ?2, color = ?3 WHERE id = ?1")?;
            for (album_id, hash, color) in &covers {
                update.execute(sql_params![album_id, hash, color])?;
            }
        }
        transaction.commit()?;
        ScanSummary {
            tracks: connection.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?,
            albums: connection.query_row("SELECT COUNT(*) FROM albums", [], |r| r.get(0))?,
            added,
            removed: removed_paths.len() as u32,
            unreadable,
        }
    };
    on_progress(ScanProgress::Done { summary });
    Ok(summary)
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn non_empty(text: Option<impl AsRef<str>>) -> Option<String> {
    text.map(|t| t.as_ref().trim().to_string()).filter(|t| !t.is_empty())
}

fn parse(file: &Found) -> Option<Parsed> {
    let tagged = lofty::read_from_path(&file.path).ok()?;
    let properties = tagged.properties();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

    let stem = file.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let title = tag.and_then(|t| non_empty(t.title())).unwrap_or(stem);
    let year = tag.and_then(|t| {
        t.date().map(|d| i32::from(d.year)).or_else(|| {
            t.get_string(ItemKey::Year).and_then(|y| y.trim().get(..4)).and_then(|y| y.parse().ok())
        })
    });

    let codec = match tagged.file_type() {
        FileType::Flac => "FLAC",
        FileType::Mpeg => "MP3",
        FileType::Mp4 => "M4A",
        FileType::Wav => "WAV",
        FileType::Aiff => "AIFF",
        FileType::Vorbis => "OGG",
        FileType::Opus => "Opus",
        FileType::Aac => "AAC",
        _ => "",
    };

    Some(Parsed {
        root_id: file.root_id,
        path: file.path.to_string_lossy().into_owned(),
        folder: album_folder(&file.path),
        size: file.size,
        mtime: file.mtime,
        title,
        artist: tag.and_then(|t| non_empty(t.artist())),
        album_artist: tag.and_then(|t| non_empty(t.get_string(ItemKey::AlbumArtist))),
        album: tag.and_then(|t| non_empty(t.album())),
        disc_no: tag.and_then(|t| t.disk()),
        track_no: tag.and_then(|t| t.track()),
        year: year.filter(|y| (1000..=9999).contains(y)),
        genre: tag.and_then(|t| non_empty(t.genre())),
        compilation: tag.and_then(|t| t.get_string(ItemKey::FlagCompilation)).is_some_and(|v| v.trim() == "1"),
        duration_ms: i64::try_from(properties.duration().as_millis()).unwrap_or(0),
        sample_rate: properties.sample_rate(),
        bit_depth: properties.bit_depth().map(u32::from),
        channels: properties.channels().map(u32::from),
        codec,
    })
}

/// Folder an album lives in. Disc folders such as `CD1` or `Disc 2` belong to their parent.
fn album_folder(track: &Path) -> String {
    let parent = track.parent().unwrap_or(Path::new(""));
    let name = parent.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let is_disc_folder = ["cd", "disc", "disk", "диск"].iter().any(|prefix| {
        name.strip_prefix(prefix).is_some_and(|rest| {
            let rest = rest.trim_start_matches([' ', '_', '-', '.']);
            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
        })
    });
    let folder = if is_disc_folder { parent.parent().unwrap_or(parent) } else { parent };
    folder.to_string_lossy().into_owned()
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn most_common<T: std::hash::Hash + Eq + Clone + Ord>(values: impl Iterator<Item = T>) -> Option<T> {
    let mut counts: HashMap<T, usize> = HashMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    counts.into_iter().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0))).map(|(value, _)| value)
}

struct GroupRow {
    id: i64,
    album: Option<String>,
    album_artist: Option<String>,
    artist: Option<String>,
    year: Option<i32>,
    compilation: bool,
    duration_ms: i64,
    added_at: i64,
    album_id: Option<i64>,
}

/// Rebuilds albums from tracks.
///
/// Tracks of one folder with the same album tag form an album. The folder is part of the
/// key, so two albums that only share a title stay apart. Tracks without an album tag are
/// grouped by folder and named after it.
pub(crate) fn regroup(connection: &Connection) -> Result<()> {
    let mut groups: HashMap<(String, String), Vec<GroupRow>> = HashMap::new();
    {
        let mut statement = connection.prepare(
            "SELECT id, folder, album, album_artist, artist, year, compilation, duration_ms, added_at, album_id FROM tracks",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let folder: String = row.get(1)?;
            let album: Option<String> = row.get(2)?;
            let key = (folder, album.as_deref().map(normalize).unwrap_or_default());
            groups.entry(key).or_default().push(GroupRow {
                id: row.get(0)?,
                album,
                album_artist: row.get(3)?,
                artist: row.get(4)?,
                year: row.get(5)?,
                compilation: row.get(6)?,
                duration_ms: row.get(7)?,
                added_at: row.get(8)?,
                album_id: row.get(9)?,
            });
        }
    }

    // Covers survive the rebuild.
    let covers: HashMap<i64, (Option<String>, Option<String>)> = connection
        .prepare("SELECT id, artwork, color FROM albums")?
        .query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))?
        .collect::<rusqlite::Result<_>>()?;

    connection.execute("DELETE FROM albums", [])?;
    let mut insert = connection.prepare(
        "INSERT INTO albums (id, title, artist, year, folder, from_folder, track_count, duration_ms, artwork, color,
                             search, artist_search, added_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?;
    let mut assign = connection.prepare("UPDATE tracks SET album_id = ?2 WHERE id = ?1")?;

    for ((folder, album_key), rows) in &groups {
        let from_folder = album_key.is_empty();
        let title = if from_folder {
            Path::new(folder).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| folder.clone())
        } else {
            rows.iter().find_map(|r| r.album.clone()).unwrap_or_default()
        };

        let artist = most_common(rows.iter().filter_map(|r| r.album_artist.clone())).unwrap_or_else(|| {
            let artists: HashSet<String> = rows.iter().filter_map(|r| r.artist.as_deref().map(normalize)).collect();
            let compilation = rows.iter().any(|r| r.compilation);
            match artists.len() {
                1 if !compilation => rows.iter().find_map(|r| r.artist.clone()).unwrap_or_default(),
                0 => String::new(),
                _ => VARIOUS_ARTISTS.to_string(),
            }
        });

        let hash = blake3::hash(format!("{folder}\u{0}{album_key}").as_bytes());
        let mut id_bytes = [0u8; 8];
        id_bytes.copy_from_slice(&hash.as_bytes()[..8]);
        let album_id = i64::from_le_bytes(id_bytes) & i64::MAX;

        let (artwork, color) = covers.get(&album_id).cloned().unwrap_or_default();
        insert.execute(sql_params![
            album_id,
            title,
            artist,
            most_common(rows.iter().filter_map(|r| r.year)),
            folder,
            from_folder,
            rows.len() as i64,
            rows.iter().map(|r| r.duration_ms).sum::<i64>(),
            artwork,
            color,
            format!("{title} {artist}").to_lowercase(),
            artist.to_lowercase(),
            rows.iter().map(|r| r.added_at).min().unwrap_or(0),
        ])?;

        for row in rows.iter().filter(|r| r.album_id != Some(album_id)) {
            assign.execute(sql_params![row.id, album_id])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disc_folders_belong_to_the_album_folder() {
        assert_eq!(album_folder(Path::new("D:/Music/Artist/Album/CD1/01.flac")), "D:/Music/Artist/Album");
        assert_eq!(album_folder(Path::new("D:/Music/Artist/Album/Disc 2/01.flac")), "D:/Music/Artist/Album");
        assert_eq!(album_folder(Path::new("D:/Music/Artist/Album/Диск 3/01.flac")), "D:/Music/Artist/Album");
        assert_eq!(album_folder(Path::new("D:/Music/Artist/Discovery/01.flac")), "D:/Music/Artist/Discovery");
        assert_eq!(album_folder(Path::new("D:/Music/Artist/CDs/01.flac")), "D:/Music/Artist/CDs");
    }

    #[test]
    fn the_most_common_value_wins_and_ties_are_stable() {
        assert_eq!(most_common([1999, 2011, 1999].into_iter()), Some(1999));
        assert_eq!(most_common([2011, 1999].into_iter()), Some(1999));
        assert_eq!(most_common(std::iter::empty::<i32>()), None);
    }
}
