//! Rendering without a device. Tests and diagnostics use it to see the exact bytes a
//! device would receive.

use std::path::PathBuf;

use crate::pack::DeviceSample;
use crate::render::{RenderError, Renderer, Step};
use crate::spec::StreamSpec;

/// Where a track begins in the rendered bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackMark {
    pub index: usize,
    pub path: PathBuf,
    pub spec: StreamSpec,
    pub byte_offset: usize,
}

/// The result of an offline render.
#[derive(Debug, Default)]
pub struct Rendered {
    pub bytes: Vec<u8>,
    pub tracks: Vec<TrackMark>,
    /// Every stream format met on the way, with the layout chosen for it.
    pub formats: Vec<(StreamSpec, DeviceSample)>,
}

/// Renders the files into memory. `choose` picks the device layout for a stream format.
pub fn render_to_vec(
    paths: impl IntoIterator<Item = PathBuf>,
    mut choose: impl FnMut(&StreamSpec) -> DeviceSample,
) -> Result<Rendered, RenderError> {
    let mut renderer = Renderer::new(paths);
    let mut rendered = Rendered::default();
    loop {
        match renderer.step(&mut rendered.bytes)? {
            Step::Audio { .. } => {}
            Step::TrackStart { index, path, spec, .. } => {
                rendered.tracks.push(TrackMark { index, path, spec, byte_offset: rendered.bytes.len() });
            }
            Step::FormatChange { spec } => {
                let sample = choose(&spec);
                rendered.formats.push((spec, sample));
                renderer.set_output(sample);
            }
            Step::End => return Ok(rendered),
        }
    }
}
