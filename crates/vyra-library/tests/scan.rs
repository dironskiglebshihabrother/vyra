//! Scanning a small collection built on the fly: tags, grouping into albums, covers.

use std::path::{Path, PathBuf};

use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{Accessor, ItemKey, Tag, TagExt, TagType};
use vyra_library::{Library, ScanProgress};

/// One second of silence as a 16-bit stereo WAV file.
fn write_wav(path: &Path) {
    let sample_rate = 44_100u32;
    let data = vec![0u8; sample_rate as usize * 4];
    let mut file = Vec::new();
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16u32.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&2u16.to_le_bytes());
    file.extend_from_slice(&sample_rate.to_le_bytes());
    file.extend_from_slice(&(sample_rate * 4).to_le_bytes());
    file.extend_from_slice(&4u16.to_le_bytes());
    file.extend_from_slice(&16u16.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&(data.len() as u32).to_le_bytes());
    file.extend_from_slice(&data);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, file).unwrap();
}

fn png(color: [u8; 3]) -> Vec<u8> {
    let image = image::RgbImage::from_pixel(64, 64, image::Rgb(color));
    let mut bytes = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
    bytes
}

struct Tags<'a> {
    title: &'a str,
    artist: &'a str,
    album: &'a str,
    album_artist: Option<&'a str>,
    track: u32,
    disc: Option<u32>,
    cover: Option<[u8; 3]>,
}

fn tagged(path: PathBuf, tags: Tags<'_>) -> PathBuf {
    write_wav(&path);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(tags.title.to_string());
    tag.set_artist(tags.artist.to_string());
    tag.set_album(tags.album.to_string());
    tag.set_track(tags.track);
    if let Some(disc) = tags.disc {
        tag.set_disk(disc);
    }
    if let Some(album_artist) = tags.album_artist {
        tag.insert_text(ItemKey::AlbumArtist, album_artist.to_string());
    }
    if let Some(color) = tags.cover {
        tag.push_picture(
            Picture::unchecked(png(color)).pic_type(PictureType::CoverFront).mime_type(MimeType::Png).build(),
        );
    }
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    path
}

fn build_collection(root: &Path) {
    let album = root.join("Nina Sol").join("Evening Tape");
    for (file, title, track, disc) in [
        ("CD1/01 Dusk.wav", "Dusk", 1, 1),
        ("CD1/02 Harbor.wav", "Harbor", 2, 1),
        ("CD2/01 Lanterns.wav", "Lanterns", 1, 2),
    ] {
        tagged(
            album.join(file),
            Tags {
                title,
                artist: "Nina Sol",
                album: "Evening Tape",
                album_artist: Some("Nina Sol"),
                track,
                disc: Some(disc),
                cover: None,
            },
        );
    }
    std::fs::write(album.join("cover.png"), png([200, 60, 40])).unwrap();

    let mix = root.join("Mixes").join("Road Mix");
    tagged(
        mix.join("01.wav"),
        Tags { title: "First", artist: "Alto", album: "Road Mix", album_artist: None, track: 1, disc: None, cover: Some([20, 40, 200]) },
    );
    tagged(
        mix.join("02.wav"),
        Tags { title: "Second", artist: "Basso", album: "Road Mix", album_artist: None, track: 2, disc: None, cover: None },
    );

    // The same album title in another folder is another album.
    tagged(
        root.join("Other").join("Evening Tape").join("01.wav"),
        Tags { title: "Unrelated", artist: "Someone", album: "Evening Tape", album_artist: None, track: 1, disc: None, cover: None },
    );

    write_wav(&root.join("Loose").join("field recording.wav"));
    std::fs::write(root.join("Loose").join("notes.txt"), "not audio").unwrap();
}

fn scan(library: &Library) -> vyra_library::ScanSummary {
    library.scan(&|_: ScanProgress| {}).unwrap()
}

#[test]
fn a_collection_is_scanned_into_albums() {
    let music = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    build_collection(music.path());

    let library = Library::open(data.path()).unwrap();
    library.add_root(music.path()).unwrap();
    let summary = scan(&library);

    assert_eq!(summary.tracks, 7);
    assert_eq!(summary.added, 7);
    assert_eq!(summary.unreadable, 0);
    assert_eq!(summary.albums, 4);

    let albums = library.albums().unwrap();
    let find = |title: &str, artist: &str| {
        albums
            .iter()
            .find(|a| a.title == title && a.artist == artist)
            .unwrap_or_else(|| panic!("album {title} by {artist} is missing: {albums:?}"))
    };

    // Disc folders are one album, played disc by disc.
    let evening = find("Evening Tape", "Nina Sol");
    assert_eq!(evening.track_count, 3);
    let detail = library.album(evening.id).unwrap().unwrap();
    let order: Vec<&str> = detail.tracks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(order, ["Dusk", "Harbor", "Lanterns"]);

    // The cover lying in the folder was found and cached in both sizes.
    let hash = evening.artwork.as_deref().expect("the folder cover was not found");
    assert!(library.artwork_file(hash, 400).is_file());
    assert!(library.artwork_file(hash, 1000).is_file());
    assert!(evening.color.as_deref().is_some_and(|c| c.starts_with('#') && c.len() == 7));

    // Different artists without an album artist make a compilation, with the embedded cover.
    let mix = find("Road Mix", "Разные исполнители");
    assert_eq!(mix.track_count, 2);
    assert!(mix.artwork.is_some());

    // The same title in another folder stays a separate album.
    assert_eq!(find("Evening Tape", "Someone").track_count, 1);

    // Files without tags are grouped by folder and named after it.
    let loose = find("Loose", "");
    let detail = library.album(loose.id).unwrap().unwrap();
    assert_eq!(detail.tracks[0].title, "field recording");
    assert_eq!(detail.tracks[0].codec, "WAV");
    assert!((900..=1100).contains(&detail.tracks[0].duration_ms));
}

#[test]
fn a_second_scan_reads_only_what_changed() {
    let music = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    build_collection(music.path());

    let library = Library::open(data.path()).unwrap();
    library.add_root(music.path()).unwrap();
    scan(&library);

    let again = scan(&library);
    assert_eq!((again.added, again.removed, again.tracks), (0, 0, 7));

    std::fs::remove_file(music.path().join("Mixes").join("Road Mix").join("02.wav")).unwrap();
    let after_delete = scan(&library);
    assert_eq!((after_delete.added, after_delete.removed, after_delete.tracks), (0, 1, 6));

    // With one artist left the album is no longer a compilation.
    let albums = library.albums().unwrap();
    assert!(albums.iter().any(|a| a.title == "Road Mix" && a.artist == "Alto"));
}

#[test]
fn search_ignores_case_and_finds_cyrillic() {
    let music = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    tagged(
        music.path().join("Кино").join("Группа крови").join("01.wav"),
        Tags { title: "Группа крови", artist: "Кино", album: "Группа крови", album_artist: None, track: 1, disc: None, cover: None },
    );
    build_collection(music.path());

    let library = Library::open(data.path()).unwrap();
    library.add_root(music.path()).unwrap();
    scan(&library);

    let results = library.search("ГРУППА").unwrap();
    assert_eq!(results.tracks.len(), 1);
    assert_eq!(results.albums.len(), 1);
    assert_eq!(library.search("кин").unwrap().artists[0].name, "Кино");
    assert_eq!(library.search("harbor").unwrap().tracks[0].title, "Harbor");
    assert!(library.search("  ").unwrap().tracks.is_empty());
    assert!(library.search("100%").unwrap().tracks.is_empty());
}

#[test]
fn removing_a_folder_forgets_its_music() {
    let music = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    build_collection(music.path());

    let library = Library::open(data.path()).unwrap();
    library.add_root(music.path()).unwrap();
    scan(&library);
    assert_eq!(library.artists().unwrap().len(), 4);

    let root = library.roots().unwrap().remove(0);
    library.remove_root(&root).unwrap();

    let counts = library.counts().unwrap();
    assert_eq!((counts.tracks, counts.albums), (0, 0));
    assert!(library.roots().unwrap().is_empty());
}
