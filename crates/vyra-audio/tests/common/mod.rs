//! Fixtures built on the fly: PCM data with a known content, written as WAV or FLAC.

#![allow(dead_code)]

use std::path::Path;

use flacenc::component::BitRepr;
use flacenc::error::Verify;
use vyra_audio::DeviceSample;

/// Block size the test encoder uses.
pub const FLAC_BLOCK: usize = 4096;

/// PCM audio with samples kept as plain integers of `bits` width, interleaved.
#[derive(Clone)]
pub struct Pcm {
    pub bits: u8,
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<i32>,
}

impl Pcm {
    /// A tone followed by full-range noise. The tone exercises the predictors of a
    /// lossless encoder, the noise covers every bit including both extremes.
    ///
    /// The tone ends on a multiple of the FLAC block size. A block that mixes tone and
    /// full-range noise makes the test encoder emit a frame of several gigabytes.
    pub fn generate(bits: u8, sample_rate: u32, channels: u16, frames: usize, seed: u64) -> Self {
        let tone_frames = if frames > 2 * FLAC_BLOCK { frames / 2 / FLAC_BLOCK * FLAC_BLOCK } else { frames };
        let max = (1i64 << (bits - 1)) - 1;
        let min = -(1i64 << (bits - 1));
        let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut next = move || {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            state >> 16
        };

        let mut samples = Vec::with_capacity(frames * usize::from(channels));
        for frame in 0..frames {
            for channel in 0..channels {
                let value = if frame < tone_frames {
                    let phase = frame as f64 * (0.031 + 0.007 * f64::from(channel));
                    let tone = phase.sin() * 0.7 * max as f64;
                    tone as i64 + (next() % 5) as i64 - 2
                } else {
                    min + (next() % (1u64 << bits)) as i64
                };
                samples.push(value.clamp(min, max) as i32);
            }
        }
        // Both extremes are present whatever the generator produced. They sit in the
        // noise part when there is one, so the tone stays smooth.
        let last = samples.len() - 1;
        samples[last] = max as i32;
        samples[last - 1] = min as i32;
        Self { bits, sample_rate, channels, samples }
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }

    /// The frames in `range` as a separate piece of audio.
    pub fn slice(&self, range: std::ops::Range<usize>) -> Self {
        let channels = usize::from(self.channels);
        Self {
            samples: self.samples[range.start * channels..range.end * channels].to_vec(),
            ..self.clone()
        }
    }

    /// The bytes a device must receive when these samples are played untouched.
    pub fn device_bytes(&self, target: DeviceSample) -> Vec<u8> {
        let bits = u32::from(self.bits);
        let mut out = Vec::new();
        for &value in &self.samples {
            match target {
                DeviceSample::I16 => {
                    assert_eq!(bits, 16, "narrowing is not a lossless case");
                    out.extend_from_slice(&(value as i16).to_le_bytes());
                }
                DeviceSample::I24 => out.extend_from_slice(&(value << (24 - bits)).to_le_bytes()[..3]),
                DeviceSample::I24In32 | DeviceSample::I32 => {
                    out.extend_from_slice(&(value << (32 - bits)).to_le_bytes());
                }
                DeviceSample::F32 => panic!("float is never a lossless target"),
            }
        }
        out
    }

    pub fn write_wav(&self, path: &Path) {
        let bytes_per_sample = usize::from(self.bits) / 8;
        let block_align = bytes_per_sample * usize::from(self.channels);
        let data_len = self.samples.len() * bytes_per_sample;

        let mut fmt = Vec::new();
        let extensible = self.bits > 16;
        fmt.extend_from_slice(&(if extensible { 0xFFFEu16 } else { 1 }).to_le_bytes());
        fmt.extend_from_slice(&self.channels.to_le_bytes());
        fmt.extend_from_slice(&self.sample_rate.to_le_bytes());
        fmt.extend_from_slice(&(self.sample_rate * block_align as u32).to_le_bytes());
        fmt.extend_from_slice(&(block_align as u16).to_le_bytes());
        fmt.extend_from_slice(&u16::from(self.bits).to_le_bytes());
        if extensible {
            fmt.extend_from_slice(&22u16.to_le_bytes());
            fmt.extend_from_slice(&u16::from(self.bits).to_le_bytes());
            let mask: u32 = if self.channels == 2 { 0x3 } else { 0x4 };
            fmt.extend_from_slice(&mask.to_le_bytes());
            // KSDATAFORMAT_SUBTYPE_PCM
            fmt.extend_from_slice(&[
                0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
            ]);
        }

        let mut file = Vec::new();
        file.extend_from_slice(b"RIFF");
        file.extend_from_slice(&((4 + 8 + fmt.len() + 8 + data_len) as u32).to_le_bytes());
        file.extend_from_slice(b"WAVE");
        file.extend_from_slice(b"fmt ");
        file.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        file.extend_from_slice(&fmt);
        file.extend_from_slice(b"data");
        file.extend_from_slice(&(data_len as u32).to_le_bytes());
        for &value in &self.samples {
            file.extend_from_slice(&value.to_le_bytes()[..bytes_per_sample]);
        }
        std::fs::write(path, file).unwrap();
    }

    pub fn write_flac(&self, path: &Path) {
        let config = flacenc::config::Encoder::default().into_verified().unwrap();
        let source = flacenc::source::MemSource::from_samples(
            &self.samples,
            usize::from(self.channels),
            usize::from(self.bits),
            self.sample_rate as usize,
        );
        let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size).unwrap();
        let mut sink = flacenc::bitsink::ByteSink::new();
        stream.write(&mut sink).unwrap();
        let mut bytes = sink.as_slice().to_vec();
        // flacenc counts the short last block in the minimum block size. The reference
        // encoder leaves it out, as the format description says, and writes the same
        // value as the maximum. Fixtures follow the reference encoder.
        let (min_block, max_block) = bytes[8..12].split_at_mut(2);
        min_block.copy_from_slice(max_block);

        let raw_len = self.samples.len() * usize::from(self.bits) / 8;
        assert!(bytes.len() < raw_len * 2 + 4096, "the encoder produced {} bytes for {raw_len} bytes of PCM", bytes.len());
        std::fs::write(path, bytes).unwrap();
    }
}

/// A file format a fixture can be written in.
#[derive(Clone, Copy, Debug)]
pub enum Container {
    Wav,
    Flac,
}

impl Container {
    pub fn write(self, pcm: &Pcm, dir: &Path, name: &str) -> std::path::PathBuf {
        let path = match self {
            Container::Wav => dir.join(format!("{name}.wav")),
            Container::Flac => dir.join(format!("{name}.flac")),
        };
        match self {
            Container::Wav => pcm.write_wav(&path),
            Container::Flac => pcm.write_flac(&path),
        }
        path
    }
}
