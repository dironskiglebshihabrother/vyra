//! File decoding on top of Symphonia.

use std::fs::File;
use std::path::{Path, PathBuf};

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Timestamp;

use crate::block::{Block, Samples};
use crate::spec::{SampleKind, StreamSpec};

/// How many damaged packets in a row end the track instead of being skipped.
const MAX_CONSECUTIVE_ERRORS: u32 = 16;

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("cannot open {}: {source}", path.display())]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{}: format is not supported: {reason}", path.display())]
    Unsupported { path: PathBuf, reason: String },
    #[error("{}: no audio track", path.display())]
    NoAudioTrack { path: PathBuf },
    #[error("{}: the stream does not state its sample rate or channels", path.display())]
    MissingParameters { path: PathBuf },
    #[error("{}: the file holds no audio frames", path.display())]
    Empty { path: PathBuf },
    #[error("{}: read failed: {reason}", path.display())]
    Read { path: PathBuf, reason: String },
    #[error("{}: too many damaged packets in a row", path.display())]
    Damaged { path: PathBuf },
    #[error("{}: seek failed: {reason}", path.display())]
    Seek { path: PathBuf, reason: String },
}

/// Decodes one audio file block by block.
pub struct SymphoniaDecoder {
    path: PathBuf,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    spec: StreamSpec,
    bits: Option<u8>,
    total_frames: Option<u64>,
    /// Timestamp units per second of the track, as a fraction.
    time_base: Option<(u32, u32)>,
    /// A block decoded ahead of time. `open` fills it to learn the sample format.
    pending: Option<Block>,
    /// Frames still to drop after a seek that landed before the target.
    skip: u64,
    damaged_packets: u32,
    /// Timestamp the next packet must start at when the stream is continuous.
    expected_ts: Option<i64>,
    lost_frames: u64,
}

impl SymphoniaDecoder {
    pub fn open(path: &Path) -> Result<Self, DecodeError> {
        let file = File::open(path)
            .map_err(|source| DecodeError::Open { path: path.to_path_buf(), source })?;
        let stream = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(extension);
        }

        let format = symphonia::default::get_probe()
            .probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| DecodeError::Unsupported { path: path.to_path_buf(), reason: e.to_string() })?;

        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| DecodeError::NoAudioTrack { path: path.to_path_buf() })?;
        let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
            return Err(DecodeError::NoAudioTrack { path: path.to_path_buf() });
        };

        let sample_rate = params.sample_rate;
        let channels = params.channels.as_ref().map(|c| c.count());
        let (Some(sample_rate), Some(channels)) = (sample_rate, channels) else {
            return Err(DecodeError::MissingParameters { path: path.to_path_buf() });
        };
        let channels = u16::try_from(channels)
            .map_err(|_| DecodeError::MissingParameters { path: path.to_path_buf() })?;

        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|e| DecodeError::Unsupported { path: path.to_path_buf(), reason: e.to_string() })?;

        let mut this = Self {
            path: path.to_path_buf(),
            track_id: track.id,
            total_frames: track.num_frames,
            time_base: track.time_base.map(|tb| (tb.numer.get(), tb.denom.get())),
            bits: params.bits_per_sample.and_then(|b| u8::try_from(b).ok()),
            // The real kind is known only after the first packet is decoded.
            spec: StreamSpec { kind: SampleKind::Float32, sample_rate, channels },
            format,
            decoder,
            pending: None,
            skip: 0,
            damaged_packets: 0,
            expected_ts: None,
            lost_frames: 0,
        };

        let first = this.decode_next()?.ok_or_else(|| DecodeError::Empty { path: this.path.clone() })?;
        this.spec = first.spec;
        this.pending = Some(first);
        Ok(this)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn spec(&self) -> StreamSpec {
        self.spec
    }

    /// Track length in frames when the container states it.
    pub fn total_frames(&self) -> Option<u64> {
        self.total_frames
    }

    /// Number of damaged packets skipped so far.
    pub fn damaged_packets(&self) -> u32 {
        self.damaged_packets
    }

    /// Frames missing from the stream so far.
    ///
    /// A demuxer drops a frame it cannot parse without telling anyone. The decoder sees
    /// it as a jump in packet timestamps and counts the frames that fell into the hole.
    pub fn lost_frames(&self) -> u64 {
        self.lost_frames
    }

    /// True when every frame decoded so far arrived intact.
    pub fn is_intact(&self) -> bool {
        self.damaged_packets == 0 && self.lost_frames == 0
    }

    /// Returns the next block, or `None` at the end of the track.
    pub fn next_block(&mut self) -> Result<Option<Block>, DecodeError> {
        if let Some(block) = self.pending.take() {
            return Ok(Some(block));
        }
        self.decode_next()
    }

    /// Moves to the given frame. The next block starts exactly there.
    pub fn seek(&mut self, frame: u64) -> Result<(), DecodeError> {
        let seek_error = |reason: String| DecodeError::Seek { path: self.path.clone(), reason };

        let ts = self.frames_to_ts(frame);
        let seeked = self
            .format
            .seek(SeekMode::Accurate, SeekTo::Timestamp { ts: Timestamp::new(ts), track_id: self.track_id })
            .map_err(|e| seek_error(e.to_string()))?;
        self.decoder.reset();
        self.pending = None;
        self.expected_ts = None;

        let landed = self.ts_to_frames(seeked.actual_ts.get());
        self.skip = frame.saturating_sub(landed);
        Ok(())
    }

    fn frames_to_ts(&self, frame: u64) -> i64 {
        match self.time_base {
            // ts = frame / rate / (numer / denom)
            Some((numer, denom)) => {
                let ts = u128::from(frame) * u128::from(denom)
                    / (u128::from(numer) * u128::from(self.spec.sample_rate));
                i64::try_from(ts).unwrap_or(i64::MAX)
            }
            None => i64::try_from(frame).unwrap_or(i64::MAX),
        }
    }

    fn ts_to_frames(&self, ts: i64) -> u64 {
        let ts = u64::try_from(ts).unwrap_or(0);
        match self.time_base {
            Some((numer, denom)) => {
                let frames = u128::from(ts) * u128::from(numer) * u128::from(self.spec.sample_rate)
                    / u128::from(denom);
                u64::try_from(frames).unwrap_or(u64::MAX)
            }
            None => ts,
        }
    }

    fn decode_next(&mut self) -> Result<Option<Block>, DecodeError> {
        let mut errors_in_a_row = 0;
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(None),
                Err(SymphoniaError::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(e) => {
                    return Err(DecodeError::Read { path: self.path.clone(), reason: e.to_string() });
                }
            };
            if packet.track_id != self.track_id {
                continue;
            }

            let pts = packet.pts.get();
            let duration = i64::try_from(packet.dur.get()).unwrap_or(0);
            if let Some(expected) = self.expected_ts
                && duration > 0
                && pts > expected
            {
                self.lost_frames += self.ts_to_frames(pts - expected);
            }
            self.expected_ts = (duration > 0).then(|| pts.saturating_add(duration));

            let buffer = match self.decoder.decode(&packet) {
                Ok(buffer) => buffer,
                // A damaged packet is skipped. The gap is short and the track goes on.
                Err(SymphoniaError::DecodeError(_)) => {
                    self.damaged_packets += 1;
                    errors_in_a_row += 1;
                    if errors_in_a_row >= MAX_CONSECUTIVE_ERRORS {
                        return Err(DecodeError::Damaged { path: self.path.clone() });
                    }
                    continue;
                }
                Err(SymphoniaError::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(e) => {
                    return Err(DecodeError::Read { path: self.path.clone(), reason: e.to_string() });
                }
            };
            errors_in_a_row = 0;

            if buffer.frames() == 0 {
                continue;
            }
            let mut block = to_block(&buffer, self.bits, self.spec.sample_rate, self.spec.channels);

            if self.skip > 0 {
                let frames = block.frames() as u64;
                let drop = self.skip.min(frames);
                self.skip -= drop;
                if drop == frames {
                    continue;
                }
                block.skip_frames(drop as usize);
            }
            return Ok(Some(block));
        }
    }
}

fn to_block(buffer: &GenericAudioBufferRef<'_>, stated_bits: Option<u8>, sample_rate: u32, channels: u16) -> Block {
    let native_bits = match buffer {
        GenericAudioBufferRef::U8(_) | GenericAudioBufferRef::S8(_) => Some(8),
        GenericAudioBufferRef::U16(_) | GenericAudioBufferRef::S16(_) => Some(16),
        GenericAudioBufferRef::U24(_) | GenericAudioBufferRef::S24(_) => Some(24),
        GenericAudioBufferRef::U32(_) | GenericAudioBufferRef::S32(_) => Some(32),
        GenericAudioBufferRef::F32(_) | GenericAudioBufferRef::F64(_) => None,
    };

    match native_bits {
        Some(native) => {
            // FLAC and ALAC decode into 32-bit buffers whatever the real depth is.
            // The container states the real depth, so it wins when it is narrower.
            let bits = stated_bits.filter(|b| (1..=native).contains(b)).unwrap_or(native);
            let mut samples = Vec::new();
            buffer.copy_to_vec_interleaved::<i32>(&mut samples);
            Block {
                spec: StreamSpec { kind: SampleKind::Int { bits }, sample_rate, channels },
                samples: Samples::Int(samples),
            }
        }
        None => {
            let mut samples = Vec::new();
            buffer.copy_to_vec_interleaved::<f32>(&mut samples);
            Block {
                spec: StreamSpec { kind: SampleKind::Float32, sample_rate, channels },
                samples: Samples::Float(samples),
            }
        }
    }
}
