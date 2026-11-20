//! VYRA audio engine.
//!
//! The crate knows nothing about the library, the queue or the interface. It takes
//! audio files and turns them into bytes for an output device.
//!
//! Two paths lead from a decoder to a device:
//!
//! * the direct path keeps integer samples as they were decoded and only pads them
//!   with zero bits to fit a wider container;
//! * the processing path converts samples and is never reported as bit-perfect.

pub mod block;
pub mod decoder;
pub mod offline;
pub mod pack;
pub mod path;
pub mod render;
pub mod spec;

pub use block::{Block, Samples};
pub use decoder::{DecodeError, SymphoniaDecoder};
pub use pack::{DeviceFormat, DeviceSample};
pub use path::{NotBitPerfect, OutputMode, SignalPath};
pub use render::{RenderError, Renderer, Step};
pub use spec::{SampleKind, StreamSpec};
