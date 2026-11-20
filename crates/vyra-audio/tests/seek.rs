//! Seeking: playback after a seek must give the same samples as decoding from the start.

mod common;

use common::{Container, Pcm};
use vyra_audio::{Samples, SymphoniaDecoder};

fn decode_rest(decoder: &mut SymphoniaDecoder) -> Vec<i32> {
    let mut samples = Vec::new();
    while let Some(block) = decoder.next_block().unwrap() {
        match block.samples {
            Samples::Int(values) => samples.extend(values),
            Samples::Float(_) => panic!("a lossless file decoded to float"),
        }
    }
    samples
}

#[test]
fn a_seek_lands_on_the_exact_frame() {
    let dir = tempfile::tempdir().unwrap();
    for (container, bits) in [(Container::Wav, 16), (Container::Flac, 16), (Container::Flac, 24)] {
        let pcm = Pcm::generate(bits, 44_100, 2, 50_000, 33);
        let path = container.write(&pcm, dir.path(), &format!("seek{bits}"));
        let shift = 32 - u32::from(bits);

        for target in [0, 1, 4_095, 4_096, 4_097, 25_000, 49_999] {
            let mut decoder = SymphoniaDecoder::open(&path).unwrap();
            decoder.seek(target as u64).unwrap();

            let decoded = decode_rest(&mut decoder);
            let expected: Vec<i32> = pcm.samples[target * 2..].iter().map(|v| v << shift).collect();

            assert_eq!(decoded.len(), expected.len(), "{container:?} {bits} bit, frame {target}: length");
            assert!(decoded == expected, "{container:?} {bits} bit, frame {target}: content differs");
        }
    }
}

#[test]
fn seeking_back_after_playing_works() {
    let dir = tempfile::tempdir().unwrap();
    let pcm = Pcm::generate(16, 44_100, 2, 30_000, 44);
    let path = Container::Flac.write(&pcm, dir.path(), "back");

    let mut decoder = SymphoniaDecoder::open(&path).unwrap();
    for _ in 0..3 {
        decoder.next_block().unwrap();
    }
    decoder.seek(100).unwrap();

    let decoded = decode_rest(&mut decoder);
    let expected: Vec<i32> = pcm.samples[100 * 2..].iter().map(|v| v << 16).collect();
    assert!(decoded == expected);
}
