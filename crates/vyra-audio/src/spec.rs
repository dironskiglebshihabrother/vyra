//! Description of a decoded stream.

use std::fmt;

/// How the samples of a stream are represented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleKind {
    /// Signed integer PCM with this many significant bits. Comes from lossless sources.
    Int { bits: u8 },
    /// 32-bit floating point. Lossy decoders produce it.
    Float32,
}

/// Format of a decoded stream: sample representation, rate and channel count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamSpec {
    pub kind: SampleKind,
    pub sample_rate: u32,
    pub channels: u16,
}

impl StreamSpec {
    /// True when the source keeps the original samples, so bit-perfect output is possible.
    pub fn is_lossless(&self) -> bool {
        matches!(self.kind, SampleKind::Int { .. })
    }
}

impl fmt::Display for StreamSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let khz = f64::from(self.sample_rate) / 1000.0;
        match self.kind {
            SampleKind::Int { bits } => write!(f, "{khz} kHz, {bits} bit, {} ch", self.channels),
            SampleKind::Float32 => write!(f, "{khz} kHz, float, {} ch", self.channels),
        }
    }
}
