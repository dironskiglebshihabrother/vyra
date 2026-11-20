//! Cover art: finding it, shrinking it and keeping it on disk by content hash.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use lofty::file::TaggedFileExt;
use lofty::picture::PictureType;

/// Stored sizes: the small one for grids and lists, the large one for the player.
pub const SIZES: [u32; 2] = [400, 1000];

const FOLDER_NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart"];
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp"];

pub fn file(dir: &Path, hash: &str, size: u32) -> PathBuf {
    let size = if size <= SIZES[0] { SIZES[0] } else { SIZES[1] };
    dir.join(format!("{hash}_{size}.jpg"))
}

/// Cover of a track: the picture inside the file, an image next to it, or an image in
/// the album folder when the track lies in a disc subfolder.
pub fn find(track: &Path, album_folder: &Path) -> Option<Vec<u8>> {
    let parent = track.parent();
    embedded(track)
        .or_else(|| parent.and_then(in_folder))
        .or_else(|| if parent == Some(album_folder) { None } else { in_folder(album_folder) })
}

fn embedded(track: &Path) -> Option<Vec<u8>> {
    let tagged = lofty::read_from_path(track).ok()?;
    let mut fallback = None;
    for tag in tagged.tags() {
        for picture in tag.pictures() {
            if picture.pic_type() == PictureType::CoverFront {
                return Some(picture.data().to_vec());
            }
            fallback.get_or_insert_with(|| picture.data().to_vec());
        }
    }
    fallback
}

fn in_folder(folder: &Path) -> Option<Vec<u8>> {
    let mut best: Option<(usize, PathBuf)> = None;
    for entry in std::fs::read_dir(folder).ok()?.flatten() {
        let path = entry.path();
        let extension = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase);
        if !extension.is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.as_str())) {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).map(str::to_lowercase).unwrap_or_default();
        // Known names win in the listed order; any other image is the last resort.
        let rank = FOLDER_NAMES.iter().position(|name| stem == *name).unwrap_or(FOLDER_NAMES.len());
        if best.as_ref().is_none_or(|(current, _)| rank < *current) {
            best = Some((rank, path));
        }
    }
    std::fs::read(best?.1).ok()
}

/// Stores a cover in both sizes and returns its hash and average colour.
///
/// Identical images share one hash, so an album cover repeated in every track is kept once.
pub fn store(dir: &Path, bytes: &[u8]) -> Option<(String, String)> {
    let hash = blake3::hash(bytes).to_hex()[..32].to_string();
    let image = image::load_from_memory(bytes).ok()?;

    for size in SIZES {
        let target = file(dir, &hash, size);
        if target.exists() {
            continue;
        }
        let side = image.width().max(image.height());
        let scaled = if side > size { image.resize(size, size, FilterType::CatmullRom) } else { image.clone() };
        let temporary = target.with_extension("tmp");
        let written = File::create(&temporary).ok().and_then(|out| {
            JpegEncoder::new_with_quality(BufWriter::new(out), 88).encode_image(&scaled.to_rgb8()).ok()
        });
        if written.is_none() || std::fs::rename(&temporary, &target).is_err() {
            let _ = std::fs::remove_file(&temporary);
            return None;
        }
    }

    Some((hash, average_color(&image)))
}

fn average_color(image: &image::DynamicImage) -> String {
    let tiny = image.resize_exact(8, 8, FilterType::Triangle).to_rgb8();
    let mut sum = [0u32; 3];
    for pixel in tiny.pixels() {
        for (total, channel) in sum.iter_mut().zip(pixel.0) {
            *total += u32::from(channel);
        }
    }
    let count = tiny.width() * tiny.height();
    format!("#{:02x}{:02x}{:02x}", sum[0] / count, sum[1] / count, sum[2] / count)
}
