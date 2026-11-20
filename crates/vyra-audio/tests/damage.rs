//! Damaged files: playback goes on, and the loss is counted instead of passing unnoticed.

mod common;

use common::{Container, Pcm};
use vyra_audio::SymphoniaDecoder;

fn decode_all(decoder: &mut SymphoniaDecoder) -> u64 {
    let mut frames = 0;
    while let Some(block) = decoder.next_block().unwrap() {
        frames += block.frames() as u64;
    }
    frames
}

#[test]
fn a_clean_file_is_reported_intact() {
    let dir = tempfile::tempdir().unwrap();
    let pcm = Pcm::generate(16, 44_100, 2, 40_000, 51);
    let path = Container::Flac.write(&pcm, dir.path(), "clean");

    let mut decoder = SymphoniaDecoder::open(&path).unwrap();
    let frames = decode_all(&mut decoder);

    assert_eq!(frames, 40_000);
    assert!(decoder.is_intact());
    assert_eq!(decoder.total_frames(), Some(40_000));
}

#[test]
fn a_damaged_frame_is_noticed() {
    let dir = tempfile::tempdir().unwrap();
    let pcm = Pcm::generate(16, 44_100, 2, 40_000, 52);
    let path = Container::Flac.write(&pcm, dir.path(), "damaged");

    // Wipe a stretch in the middle of the file, inside one of the frames.
    let mut bytes = std::fs::read(&path).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle..middle + 64].fill(0);
    std::fs::write(&path, bytes).unwrap();

    let mut decoder = SymphoniaDecoder::open(&path).unwrap();
    let frames = decode_all(&mut decoder);

    assert!(!decoder.is_intact(), "the damage went unnoticed");
    assert!(frames < 40_000, "a damaged frame cannot produce all of its audio");
    assert!(
        decoder.lost_frames() > 0 || decoder.damaged_packets() > 0,
        "neither counter recorded the damage"
    );
}
