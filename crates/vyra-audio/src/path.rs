//! The signal path report: what happens to the sound between the file and the device.

use std::fmt;

use crate::pack::{DeviceFormat, DeviceSample};
use crate::spec::{SampleKind, StreamSpec};

/// How the output device is shared with other programs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputMode {
    /// The stream goes through the Windows mixer together with other programs.
    Shared,
    /// The program owns the device and feeds it directly.
    Exclusive,
}

/// One reason why playback is not bit-perfect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotBitPerfect {
    /// The codec is lossy, the term does not apply.
    LossySource,
    /// The Windows mixer sits between the program and the device.
    SharedMode,
    /// The device runs at another sample rate.
    SampleRateChanged { from: u32, to: u32 },
    /// The device has another channel count.
    ChannelsChanged { from: u16, to: u16 },
    /// Integer samples are converted to float.
    ConvertedToFloat,
    /// The device takes fewer bits than the source has.
    BitDepthReduced { from: u8, to: u8 },
}

impl fmt::Display for NotBitPerfect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotBitPerfect::LossySource => f.write_str("lossy source"),
            NotBitPerfect::SharedMode => f.write_str("shared mode goes through the Windows mixer"),
            NotBitPerfect::SampleRateChanged { from, to } => {
                write!(f, "sample rate changed from {from} to {to} Hz")
            }
            NotBitPerfect::ChannelsChanged { from, to } => write!(f, "channels changed from {from} to {to}"),
            NotBitPerfect::ConvertedToFloat => f.write_str("samples converted to float"),
            NotBitPerfect::BitDepthReduced { from, to } => {
                write!(f, "bit depth reduced from {from} to {to}")
            }
        }
    }
}

/// What happens to the sound on its way to the device.
///
/// The engine fills this in, the interface only shows it. `bit_perfect` is true when the
/// bytes handed to the device are the decoded samples, at most padded with zero bits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalPath {
    pub source: StreamSpec,
    pub output: DeviceFormat,
    pub mode: OutputMode,
    pub bit_perfect: bool,
    pub reasons: Vec<NotBitPerfect>,
}

impl SignalPath {
    pub fn evaluate(source: StreamSpec, output: DeviceFormat, mode: OutputMode) -> Self {
        let mut reasons = Vec::new();

        if !source.is_lossless() {
            reasons.push(NotBitPerfect::LossySource);
        }
        if mode == OutputMode::Shared {
            reasons.push(NotBitPerfect::SharedMode);
        }
        if source.sample_rate != output.sample_rate {
            reasons.push(NotBitPerfect::SampleRateChanged { from: source.sample_rate, to: output.sample_rate });
        }
        if source.channels != output.channels {
            reasons.push(NotBitPerfect::ChannelsChanged { from: source.channels, to: output.channels });
        }
        if let SampleKind::Int { bits } = source.kind {
            match output.sample.valid_bits() {
                None => reasons.push(NotBitPerfect::ConvertedToFloat),
                Some(valid) if valid < bits => {
                    reasons.push(NotBitPerfect::BitDepthReduced { from: bits, to: valid });
                }
                Some(_) => {}
            }
        }

        Self { source, output, mode, bit_perfect: reasons.is_empty(), reasons }
    }
}

impl fmt::Display for SignalPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mode = match self.mode {
            OutputMode::Shared => "shared",
            OutputMode::Exclusive => "exclusive",
        };
        write!(
            f,
            "{} -> {} kHz, {}, {} ch, {mode}",
            self.source,
            f64::from(self.output.sample_rate) / 1000.0,
            self.output.sample,
            self.output.channels,
        )?;
        if self.bit_perfect {
            f.write_str(" | bit-perfect")
        } else {
            let reasons: Vec<String> = self.reasons.iter().map(ToString::to_string).collect();
            write!(f, " | not bit-perfect: {}", reasons.join("; "))
        }
    }
}

/// Picks the device layouts worth trying for a source, best first.
///
/// For lossless sources the list starts with layouts that keep every bit.
pub fn preferred_layouts(source: StreamSpec) -> Vec<DeviceSample> {
    match source.kind {
        SampleKind::Int { bits } if bits <= 16 => {
            vec![DeviceSample::I16, DeviceSample::I24In32, DeviceSample::I24, DeviceSample::I32]
        }
        SampleKind::Int { bits } if bits <= 24 => {
            vec![DeviceSample::I24In32, DeviceSample::I24, DeviceSample::I32, DeviceSample::I16]
        }
        SampleKind::Int { .. } => {
            vec![DeviceSample::I32, DeviceSample::I24In32, DeviceSample::I24, DeviceSample::I16]
        }
        SampleKind::Float32 => {
            vec![DeviceSample::I24In32, DeviceSample::I32, DeviceSample::I24, DeviceSample::I16]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CD: StreamSpec = StreamSpec { kind: SampleKind::Int { bits: 16 }, sample_rate: 44_100, channels: 2 };

    fn device(sample_rate: u32, sample: DeviceSample) -> DeviceFormat {
        DeviceFormat { sample_rate, channels: 2, sample }
    }

    #[test]
    fn exclusive_output_at_the_source_format_is_bit_perfect() {
        for sample in [DeviceSample::I16, DeviceSample::I24, DeviceSample::I24In32, DeviceSample::I32] {
            let path = SignalPath::evaluate(CD, device(44_100, sample), OutputMode::Exclusive);
            assert!(path.bit_perfect, "{sample:?}: {path}");
        }
    }

    #[test]
    fn shared_mode_is_never_bit_perfect() {
        let path = SignalPath::evaluate(CD, device(44_100, DeviceSample::F32), OutputMode::Shared);
        assert!(!path.bit_perfect);
        assert!(path.reasons.contains(&NotBitPerfect::SharedMode));
        assert!(path.reasons.contains(&NotBitPerfect::ConvertedToFloat));
    }

    #[test]
    fn a_narrower_device_is_reported() {
        let hires = StreamSpec { kind: SampleKind::Int { bits: 24 }, sample_rate: 96_000, channels: 2 };
        let path = SignalPath::evaluate(hires, device(96_000, DeviceSample::I16), OutputMode::Exclusive);
        assert_eq!(path.reasons, [NotBitPerfect::BitDepthReduced { from: 24, to: 16 }]);
    }

    #[test]
    fn lossy_sources_never_count_as_bit_perfect() {
        let mp3 = StreamSpec { kind: SampleKind::Float32, sample_rate: 44_100, channels: 2 };
        let path = SignalPath::evaluate(mp3, device(44_100, DeviceSample::I24In32), OutputMode::Exclusive);
        assert_eq!(path.reasons, [NotBitPerfect::LossySource]);
    }
}
