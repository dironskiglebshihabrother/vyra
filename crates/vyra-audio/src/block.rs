//! A run of decoded frames.

use crate::spec::StreamSpec;

/// Interleaved samples of one block.
#[derive(Clone, Debug, PartialEq)]
pub enum Samples {
    /// Integer samples aligned to the most significant bit of an `i32`.
    ///
    /// A 16-bit sample `v` is stored as `v << 16`, a 24-bit one as `v << 8`. Every source
    /// depth fits without loss and packing for a device becomes a plain shift.
    Int(Vec<i32>),
    /// Floating point samples in the range -1.0..=1.0.
    Float(Vec<f32>),
}

/// Decoded frames together with the format they are in.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub spec: StreamSpec,
    pub samples: Samples,
}

impl Block {
    /// Number of frames. One frame holds one sample for every channel.
    pub fn frames(&self) -> usize {
        let samples = match &self.samples {
            Samples::Int(v) => v.len(),
            Samples::Float(v) => v.len(),
        };
        samples / usize::from(self.spec.channels.max(1))
    }

    /// Drops frames from the start of the block. Used after an accurate seek.
    pub fn skip_frames(&mut self, frames: usize) {
        let count = (frames * usize::from(self.spec.channels)).min(match &self.samples {
            Samples::Int(v) => v.len(),
            Samples::Float(v) => v.len(),
        });
        match &mut self.samples {
            Samples::Int(v) => drop(v.drain(..count)),
            Samples::Float(v) => drop(v.drain(..count)),
        }
    }
}
