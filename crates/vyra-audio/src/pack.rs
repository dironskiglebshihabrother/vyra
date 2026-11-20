//! Packing decoded samples into the byte layout a device expects.

use std::fmt;

use crate::block::{Block, Samples};

/// Sample layout accepted by an output device. All layouts are little endian.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceSample {
    /// 16-bit integer in two bytes.
    I16,
    /// 24-bit integer in three bytes.
    I24,
    /// 24 significant bits in the upper part of a four byte container.
    I24In32,
    /// 32-bit integer in four bytes.
    I32,
    /// 32-bit float in four bytes. The Windows shared mode mixer works in it.
    F32,
}

impl DeviceSample {
    /// Bytes one sample takes.
    pub fn bytes(self) -> usize {
        match self {
            DeviceSample::I16 => 2,
            DeviceSample::I24 => 3,
            DeviceSample::I24In32 | DeviceSample::I32 | DeviceSample::F32 => 4,
        }
    }

    /// Significant bits of an integer layout, `None` for float.
    pub fn valid_bits(self) -> Option<u8> {
        match self {
            DeviceSample::I16 => Some(16),
            DeviceSample::I24 | DeviceSample::I24In32 => Some(24),
            DeviceSample::I32 => Some(32),
            DeviceSample::F32 => None,
        }
    }
}

impl fmt::Display for DeviceSample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DeviceSample::I16 => "16 bit",
            DeviceSample::I24 => "24 bit",
            DeviceSample::I24In32 => "24 bit in 32",
            DeviceSample::I32 => "32 bit",
            DeviceSample::F32 => "float",
        })
    }
}

/// Format an output stream was opened with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample: DeviceSample,
}

impl DeviceFormat {
    /// Bytes one frame takes.
    pub fn frame_bytes(&self) -> usize {
        self.sample.bytes() * usize::from(self.channels)
    }
}

/// Appends the samples of a block to `out` in the device layout.
///
/// Integer samples going to an integer layout that is at least as deep are only shifted,
/// so the original bits reach the device unchanged. Every other combination converts the
/// value and is reported by [`crate::path::SignalPath`] as not bit-perfect.
pub fn pack(block: &Block, target: DeviceSample, out: &mut Vec<u8>) {
    match &block.samples {
        Samples::Int(samples) => {
            out.reserve(samples.len() * target.bytes());
            match target {
                DeviceSample::I16 => {
                    for &s in samples {
                        out.extend_from_slice(&((s >> 16) as i16).to_le_bytes());
                    }
                }
                DeviceSample::I24 => {
                    for &s in samples {
                        out.extend_from_slice(&s.to_le_bytes()[1..4]);
                    }
                }
                DeviceSample::I24In32 => {
                    for &s in samples {
                        out.extend_from_slice(&(s & !0xFF).to_le_bytes());
                    }
                }
                DeviceSample::I32 => {
                    for &s in samples {
                        out.extend_from_slice(&s.to_le_bytes());
                    }
                }
                DeviceSample::F32 => {
                    for &s in samples {
                        let value = (f64::from(s) / 2_147_483_648.0) as f32;
                        out.extend_from_slice(&value.to_le_bytes());
                    }
                }
            }
        }
        Samples::Float(samples) => {
            out.reserve(samples.len() * target.bytes());
            match target {
                DeviceSample::F32 => {
                    for &s in samples {
                        out.extend_from_slice(&s.to_le_bytes());
                    }
                }
                DeviceSample::I16 => {
                    for &s in samples {
                        out.extend_from_slice(&((quantize(s) >> 16) as i16).to_le_bytes());
                    }
                }
                DeviceSample::I24 => {
                    for &s in samples {
                        out.extend_from_slice(&quantize(s).to_le_bytes()[1..4]);
                    }
                }
                DeviceSample::I24In32 => {
                    for &s in samples {
                        out.extend_from_slice(&(quantize(s) & !0xFF).to_le_bytes());
                    }
                }
                DeviceSample::I32 => {
                    for &s in samples {
                        out.extend_from_slice(&quantize(s).to_le_bytes());
                    }
                }
            }
        }
    }
}

/// Turns a float sample into a 32-bit integer aligned to the most significant bit.
fn quantize(sample: f32) -> i32 {
    let scaled = f64::from(sample) * 2_147_483_648.0;
    scaled.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{SampleKind, StreamSpec};

    fn int_block(bits: u8, values: &[i32]) -> Block {
        let shift = 32 - u32::from(bits);
        Block {
            spec: StreamSpec { kind: SampleKind::Int { bits }, sample_rate: 44_100, channels: 1 },
            samples: Samples::Int(values.iter().map(|v| v << shift).collect()),
        }
    }

    #[test]
    fn sixteen_bit_reaches_a_sixteen_bit_device_unchanged() {
        let values = [0, 1, -1, i32::from(i16::MAX), i32::from(i16::MIN), 12_345];
        let mut out = Vec::new();
        pack(&int_block(16, &values), DeviceSample::I16, &mut out);
        let expected: Vec<u8> = values.iter().flat_map(|v| (*v as i16).to_le_bytes()).collect();
        assert_eq!(out, expected);
    }

    #[test]
    fn widening_only_adds_zero_bits() {
        let values = [1, -1, i32::from(i16::MAX), i32::from(i16::MIN)];
        let mut out = Vec::new();
        pack(&int_block(16, &values), DeviceSample::I24, &mut out);
        let expected: Vec<u8> = values.iter().flat_map(|v| (v << 8).to_le_bytes()[..3].to_vec()).collect();
        assert_eq!(out, expected);

        let mut out = Vec::new();
        pack(&int_block(16, &values), DeviceSample::I24In32, &mut out);
        let expected: Vec<u8> = values.iter().flat_map(|v| (v << 16).to_le_bytes()).collect();
        assert_eq!(out, expected);
    }

    #[test]
    fn float_full_scale_does_not_wrap() {
        let block = Block {
            spec: StreamSpec { kind: SampleKind::Float32, sample_rate: 44_100, channels: 1 },
            samples: Samples::Float(vec![1.0, -1.0, 2.0, -2.0]),
        };
        let mut out = Vec::new();
        pack(&block, DeviceSample::I16, &mut out);
        let values: Vec<i16> = out.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect();
        assert_eq!(values, [i16::MAX, i16::MIN, i16::MAX, i16::MIN]);
    }
}
