//! The direct path: what the device receives must be the samples of the file.

mod common;

use common::{Container, Pcm};
use vyra_audio::offline::render_to_vec;
use vyra_audio::{DeviceSample, SampleKind, StreamSpec};

const FRAMES: usize = 30_011;

#[test]
fn wav_reaches_the_device_bit_for_bit() {
    let dir = tempfile::tempdir().unwrap();
    for (bits, sample_rate, target) in
        [(16, 44_100, DeviceSample::I16), (24, 192_000, DeviceSample::I24), (32, 384_000, DeviceSample::I32)]
    {
        let pcm = Pcm::generate(bits, sample_rate, 2, FRAMES, u64::from(bits));
        let path = Container::Wav.write(&pcm, dir.path(), &format!("tone{bits}"));

        let rendered = render_to_vec([path], |_| target).unwrap();

        assert_eq!(rendered.formats[0].0.sample_rate, sample_rate);
        assert_eq!(rendered.bytes.len(), pcm.device_bytes(target).len(), "{bits} bit: length");
        assert!(rendered.bytes == pcm.device_bytes(target), "{bits} bit: content differs");
    }
}

#[test]
fn flac_reaches_the_device_bit_for_bit() {
    let dir = tempfile::tempdir().unwrap();
    for (bits, sample_rate, target) in
        // The test encoder stops at 96 kHz. Higher rates are covered by the WAV test.
        [(16, 44_100, DeviceSample::I16), (24, 96_000, DeviceSample::I24), (24, 88_200, DeviceSample::I24In32)]
    {
        let pcm = Pcm::generate(bits, sample_rate, 2, FRAMES, 7 + u64::from(bits));
        let path = Container::Flac.write(&pcm, dir.path(), &format!("tone{bits}_{sample_rate}"));

        let rendered = render_to_vec([path], |_| target).unwrap();

        assert_eq!(rendered.bytes.len(), pcm.device_bytes(target).len(), "{bits} bit: length");
        assert!(rendered.bytes == pcm.device_bytes(target), "{bits} bit at {sample_rate}: content differs");
    }
}

#[test]
fn a_narrow_source_in_a_wider_container_is_only_padded_with_zeros() {
    let dir = tempfile::tempdir().unwrap();
    let pcm = Pcm::generate(16, 44_100, 2, FRAMES, 3);
    let path = Container::Flac.write(&pcm, dir.path(), "cd");

    for target in [DeviceSample::I24, DeviceSample::I24In32, DeviceSample::I32] {
        let rendered = render_to_vec([path.clone()], |_| target).unwrap();
        assert!(rendered.bytes == pcm.device_bytes(target), "{target:?}: content differs");
    }
}

#[test]
fn the_stream_format_is_read_from_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let pcm = Pcm::generate(24, 96_000, 2, 5_000, 11);
    let path = Container::Flac.write(&pcm, dir.path(), "hires");

    let rendered = render_to_vec([path], |_| DeviceSample::I24).unwrap();

    let expected = StreamSpec { kind: SampleKind::Int { bits: 24 }, sample_rate: 96_000, channels: 2 };
    assert_eq!(rendered.formats, [(expected, DeviceSample::I24)]);
    assert_eq!(rendered.tracks.len(), 1);
    assert_eq!(rendered.tracks[0].spec, expected);
}

#[test]
fn mono_files_keep_one_channel() {
    let dir = tempfile::tempdir().unwrap();
    let pcm = Pcm::generate(16, 48_000, 1, FRAMES, 5);
    let path = Container::Wav.write(&pcm, dir.path(), "mono");

    let rendered = render_to_vec([path], |_| DeviceSample::I16).unwrap();

    assert_eq!(rendered.formats[0].0.channels, 1);
    assert!(rendered.bytes == pcm.device_bytes(DeviceSample::I16));
}
