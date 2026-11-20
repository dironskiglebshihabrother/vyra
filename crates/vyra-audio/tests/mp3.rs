//! MP3: the most common format in real collections. Files are encoded on the fly with LAME.

use std::path::{Path, PathBuf};

use mp3lame_encoder::{Bitrate, Builder, FlushGap, InterleavedPcm, Quality};
use vyra_audio::offline::render_to_vec;
use vyra_audio::{DeviceSample, SampleKind, Samples, SymphoniaDecoder};

const RATE: u32 = 44_100;

/// A stereo tone, quiet at both ends so a cut in the middle is audible in the numbers.
fn tone(frames: usize, offset: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(frames * 2);
    for frame in offset..offset + frames {
        let t = frame as f64 / f64::from(RATE);
        samples.push(((t * 440.0 * std::f64::consts::TAU).sin() * 12_000.0) as i16);
        samples.push(((t * 660.0 * std::f64::consts::TAU).sin() * 9_000.0) as i16);
    }
    samples
}

fn write_mp3(path: &Path, pcm: &[i16]) {
    let mut builder = Builder::new().unwrap();
    builder.set_num_channels(2).unwrap();
    builder.set_sample_rate(RATE).unwrap();
    builder.set_brate(Bitrate::Kbps192).unwrap();
    builder.set_quality(Quality::Good).unwrap();
    builder.set_to_write_vbr_tag(true).unwrap();
    let mut encoder = builder.build().unwrap();

    let mut audio = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(pcm.len() / 2));
    encoder.encode_to_vec(InterleavedPcm(pcm), &mut audio).unwrap();
    encoder.flush_to_vec::<FlushGap>(&mut audio).unwrap();

    // The LAME tag carries the encoder delay and the padding. The encoder leaves an empty
    // frame for it at the start of the stream; the finished tag is written over that frame.
    let mut tag = Vec::with_capacity(encoder.lame_tag_size());
    encoder.lame_tag_encode_to_vec(&mut tag);
    assert!(!tag.is_empty(), "the encoder gave no LAME tag");
    let boundary = encoder.id3v2_tag_size();
    audio[boundary..boundary + tag.len()].copy_from_slice(&tag);
    std::fs::write(path, audio).unwrap();
}

fn mp3(dir: &Path, name: &str, pcm: &[i16]) -> PathBuf {
    let path = dir.join(format!("{name}.mp3"));
    write_mp3(&path, pcm);
    path
}

fn decode(path: &Path) -> (SymphoniaDecoder, Vec<f32>) {
    let mut decoder = SymphoniaDecoder::open(path).unwrap();
    let mut samples = Vec::new();
    while let Some(block) = decoder.next_block().unwrap() {
        match block.samples {
            Samples::Float(values) => samples.extend(values),
            Samples::Int(_) => panic!("an MP3 file decoded to integers"),
        }
    }
    (decoder, samples)
}

#[test]
fn an_mp3_file_decodes_to_the_sound_that_was_encoded() {
    let dir = tempfile::tempdir().unwrap();
    let frames = 3 * RATE as usize;
    let pcm = tone(frames, 0);
    let path = mp3(dir.path(), "tone", &pcm);

    let (decoder, decoded) = decode(&path);

    assert_eq!(decoder.spec().kind, SampleKind::Float32);
    assert_eq!((decoder.spec().sample_rate, decoder.spec().channels), (RATE, 2));
    assert!(decoder.is_intact());

    // Encoder delay and padding are trimmed, so the length is exact.
    assert_eq!(decoded.len() / 2, frames, "the decoded length differs from the source");

    // And the trimming is aligned: the decoded tone follows the original closely.
    let mut error = 0.0f64;
    for (got, want) in decoded.iter().zip(&pcm) {
        let diff = f64::from(*got) - f64::from(*want) / 32_768.0;
        error += diff * diff;
    }
    let rms = (error / decoded.len() as f64).sqrt();
    assert!(rms < 0.01, "the decoded tone is off by {rms}");
}

#[test]
fn two_mp3_files_join_without_extra_frames() {
    let dir = tempfile::tempdir().unwrap();
    let first_frames = 50_000;
    let second_frames = 61_234;
    let first = mp3(dir.path(), "a", &tone(first_frames, 0));
    let second = mp3(dir.path(), "b", &tone(second_frames, first_frames));

    let rendered = render_to_vec([first, second], |_| DeviceSample::F32).unwrap();

    assert_eq!(rendered.formats.len(), 1);
    assert_eq!(rendered.tracks[1].byte_offset, first_frames * 2 * 4);
    assert_eq!(rendered.bytes.len(), (first_frames + second_frames) * 2 * 4);
}

#[test]
fn seeking_in_an_mp3_file_lands_on_the_frame() {
    let dir = tempfile::tempdir().unwrap();
    let frames = 4 * RATE as usize;
    let path = mp3(dir.path(), "seek", &tone(frames, 0));
    let (_, whole) = decode(&path);

    for target in [1usize, 1_152, 30_000, 100_001] {
        let mut decoder = SymphoniaDecoder::open(&path).unwrap();
        decoder.seek(target as u64).unwrap();
        let mut rest = Vec::new();
        while let Some(block) = decoder.next_block().unwrap() {
            if let Samples::Float(values) = block.samples {
                rest.extend(values);
            }
        }
        assert_eq!(rest.len() / 2, frames - target, "frame {target}: length");

        // MP3 frames overlap, so the first samples after a seek settle within a frame.
        let settled = 2 * 2_304;
        let expected = &whole[target * 2..];
        let worst = rest[settled..].iter().zip(&expected[settled..]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst < 0.01, "frame {target}: the sound after the seek is off by {worst}");
    }
}

