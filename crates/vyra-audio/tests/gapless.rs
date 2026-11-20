//! Joining tracks: two files that continue each other must sound as one.

mod common;

use common::{Container, Pcm};
use vyra_audio::offline::render_to_vec;
use vyra_audio::{DeviceSample, RenderError, Renderer, Step};

#[test]
fn two_halves_join_into_the_whole() {
    let dir = tempfile::tempdir().unwrap();
    for (container, bits, target) in [
        (Container::Wav, 16, DeviceSample::I16),
        (Container::Flac, 16, DeviceSample::I16),
        (Container::Flac, 24, DeviceSample::I24In32),
    ] {
        let whole = Pcm::generate(bits, 44_100, 2, 40_003, 21);
        // The cut falls inside a FLAC block and is not a round number on purpose.
        let cut = 17_321;
        let first = container.write(&whole.slice(0..cut), dir.path(), &format!("a{bits}"));
        let second = container.write(&whole.slice(cut..whole.frames()), dir.path(), &format!("b{bits}"));

        let rendered = render_to_vec([first, second], |_| target).unwrap();

        assert!(rendered.bytes == whole.device_bytes(target), "{container:?} {bits} bit: the join is not exact");
        assert_eq!(rendered.formats.len(), 1, "one format means one uninterrupted output stream");
        assert_eq!(rendered.tracks.len(), 2);
        assert_eq!(rendered.tracks[1].byte_offset, cut * 2 * target.bytes());
    }
}

#[test]
fn a_format_change_between_tracks_is_announced() {
    let dir = tempfile::tempdir().unwrap();
    let cd = Container::Flac.write(&Pcm::generate(16, 44_100, 2, 6_000, 1), dir.path(), "cd");
    let hires = Container::Flac.write(&Pcm::generate(24, 96_000, 2, 6_000, 2), dir.path(), "hires");

    let rendered = render_to_vec([cd, hires], |spec| match spec.sample_rate {
        44_100 => DeviceSample::I16,
        _ => DeviceSample::I24,
    })
    .unwrap();

    assert_eq!(rendered.formats.len(), 2);
    assert_eq!(rendered.formats[1].0.sample_rate, 96_000);
    assert_eq!(rendered.tracks[1].byte_offset, 6_000 * 2 * 2);
    assert_eq!(rendered.bytes.len(), 6_000 * 2 * 2 + 6_000 * 2 * 3);
}

#[test]
fn a_file_that_cannot_be_opened_does_not_stop_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let first = Pcm::generate(16, 44_100, 2, 4_000, 8);
    let second = Pcm::generate(16, 44_100, 2, 4_000, 9);
    let paths = [
        Container::Wav.write(&first, dir.path(), "first"),
        dir.path().join("missing.flac"),
        Container::Wav.write(&second, dir.path(), "second"),
    ];

    let mut renderer = Renderer::new(paths);
    let mut bytes = Vec::new();
    let mut failures = 0;
    let mut started = Vec::new();
    loop {
        match renderer.step(&mut bytes) {
            Ok(Step::FormatChange { .. }) => renderer.set_output(DeviceSample::I16),
            Ok(Step::TrackStart { index, .. }) => started.push(index),
            Ok(Step::Audio { .. }) => {}
            Ok(Step::End) => break,
            Err(RenderError::Decode(_)) => failures += 1,
            Err(other) => panic!("unexpected error: {other}"),
        }
    }

    assert_eq!(failures, 1);
    assert_eq!(started, [0, 2]);
    let mut expected = first.device_bytes(DeviceSample::I16);
    expected.extend(second.device_bytes(DeviceSample::I16));
    assert!(bytes == expected);
}
