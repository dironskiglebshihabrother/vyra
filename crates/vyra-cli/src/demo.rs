//! A small collection for trying the application: tagged tracks of digital silence
//! with generated covers. Nothing is heard when it plays.

use std::path::Path;

use image::{Rgb, RgbImage};
use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{Accessor, ItemKey, Tag, TagExt, TagType};

struct DemoAlbum {
    artist: &'static str,
    title: &'static str,
    year: u32,
    hue: f32,
    bits: u16,
    sample_rate: u32,
    /// Cover inside the files when true, `cover.png` in the folder otherwise.
    embedded_cover: bool,
    tracks: &'static [&'static str],
}

const ALBUMS: &[DemoAlbum] = &[
    DemoAlbum { artist: "Анна Левина", title: "Тихая вода", year: 2019, hue: 208.0, bits: 24, sample_rate: 96_000, embedded_cover: true, tracks: &["Перед рассветом", "Тихая вода", "Мост", "Письмо без адреса"] },
    DemoAlbum { artist: "Анна Левина", title: "Ночной трамвай", year: 2022, hue: 262.0, bits: 16, sample_rate: 48_000, embedded_cover: false, tracks: &["Депо", "Ночной трамвай", "Окно", "Кольцо"] },
    DemoAlbum { artist: "Nina Sol", title: "Harbor Lights", year: 2021, hue: 24.0, bits: 16, sample_rate: 44_100, embedded_cover: true, tracks: &["Dusk", "Harbor Lights", "Lanterns", "Salt", "Low Tide"] },
    DemoAlbum { artist: "Квартал 12", title: "Этажи", year: 2016, hue: 142.0, bits: 16, sample_rate: 44_100, embedded_cover: false, tracks: &["Подъезд", "Этажи", "Соседи", "Лифт"] },
    DemoAlbum { artist: "Orbit Hotel", title: "Slow Machines", year: 2018, hue: 328.0, bits: 24, sample_rate: 96_000, embedded_cover: true, tracks: &["Start", "Slow Machines", "Glass Room", "Relay"] },
    DemoAlbum { artist: "Marek Trio", title: "Blue Hours", year: 2009, hue: 222.0, bits: 16, sample_rate: 44_100, embedded_cover: false, tracks: &["Blue Hours", "Waltz for M.", "Corner Table", "After Rain"] },
];

/// Seconds of silence in every demo track.
const SECONDS: u32 = 8;

pub fn make(dir: &Path) -> Result<(), String> {
    for (index, album) in ALBUMS.iter().enumerate() {
        let folder = dir.join(album.artist).join(album.title);
        std::fs::create_dir_all(&folder).map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
        let cover = cover_png(album.hue, index);
        if !album.embedded_cover {
            std::fs::write(folder.join("cover.png"), &cover).map_err(|e| e.to_string())?;
        }

        for (number, title) in album.tracks.iter().enumerate() {
            let path = folder.join(format!("{:02} {title}.wav", number + 1));
            write_silence(&path, album.sample_rate, album.bits)?;

            let mut tag = Tag::new(TagType::Id3v2);
            tag.set_title((*title).to_string());
            tag.set_artist(album.artist.to_string());
            tag.set_album(album.title.to_string());
            tag.set_track(number as u32 + 1);
            tag.insert_text(ItemKey::AlbumArtist, album.artist.to_string());
            tag.insert_text(ItemKey::RecordingDate, album.year.to_string());
            if album.embedded_cover {
                tag.push_picture(
                    Picture::unchecked(cover.clone()).pic_type(PictureType::CoverFront).mime_type(MimeType::Png).build(),
                );
            }
            tag.save_to_path(&path, WriteOptions::default()).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        println!("{} / {}: {} tracks", album.artist, album.title, album.tracks.len());
    }
    Ok(())
}

fn write_silence(path: &Path, sample_rate: u32, bits: u16) -> Result<(), String> {
    let block = 2 * u32::from(bits) / 8;
    let data_len = sample_rate * SECONDS * block;
    let mut file = Vec::with_capacity(data_len as usize + 44);
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(36 + data_len).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16u32.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&2u16.to_le_bytes());
    file.extend_from_slice(&sample_rate.to_le_bytes());
    file.extend_from_slice(&(sample_rate * block).to_le_bytes());
    file.extend_from_slice(&(block as u16).to_le_bytes());
    file.extend_from_slice(&bits.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&data_len.to_le_bytes());
    file.resize(file.len() + data_len as usize, 0);
    std::fs::write(path, file).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn hsl(hue: f32, saturation: f32, lightness: f32) -> Rgb<u8> {
    let c = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let h = (hue.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = lightness - c / 2.0;
    Rgb([((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8])
}

/// A simple flat cover: a field, a horizon and a disc, in colours of one hue family.
fn cover_png(hue: f32, variant: usize) -> Vec<u8> {
    const SIZE: u32 = 600;
    let field = hsl(hue, 0.46, 0.46);
    let ground = hsl(hue + 28.0, 0.52, 0.28);
    let disc = hsl(hue - 20.0, 0.60, 0.74);

    let image = RgbImage::from_fn(SIZE, SIZE, |x, y| {
        let (fx, fy) = (x as f32 / SIZE as f32, y as f32 / SIZE as f32);
        match variant % 3 {
            0 => {
                let in_disc = (fx - 0.5).powi(2) + (fy - 0.56).powi(2) < 0.30f32.powi(2);
                if fy > 0.70 { ground } else if in_disc { disc } else { field }
            }
            1 => {
                let in_disc = (fx - 0.5).powi(2) + (fy - 0.5).powi(2) < 0.34f32.powi(2);
                if (fx - 0.5).abs() < 0.10 { disc } else if in_disc { ground } else { field }
            }
            _ => {
                let in_peak = fy > 0.2 + (fx - 0.5).abs() * 1.6;
                let in_disc = (fx - 0.74).powi(2) + (fy - 0.28).powi(2) < 0.11f32.powi(2);
                if in_disc { disc } else if in_peak { ground } else { field }
            }
        }
    });

    let mut bytes = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("a PNG can always be written to memory");
    bytes
}
