//! Plays a list of files on a device and reports what happens.
//!
//! This is the simplest complete path through the engine: renderer, output stream and
//! the reopening of the device when the stream format changes between tracks.

use std::path::PathBuf;

use crate::output::OutputError;
use crate::output::wasapi::{OutputStream, StreamRequest, StreamStats};
use crate::path::{OutputMode, SignalPath};
use crate::render::{RenderError, Renderer, Step};
use crate::spec::StreamSpec;

#[derive(Clone, Debug)]
pub struct PlayOptions {
    /// Device identifier, or `None` for the system default.
    pub device_id: Option<String>,
    pub mode: OutputMode,
}

/// Something worth telling the user while a list plays.
#[derive(Clone, Debug)]
pub enum PlayEvent {
    /// The device was opened for a new stream format.
    Opened { device_name: String, path: SignalPath, buffer_frames: u32 },
    TrackStart { index: usize, path: PathBuf, spec: StreamSpec, total_frames: Option<u64> },
    /// A file could not be played and was skipped.
    FileSkipped { reason: String },
    /// Frames handed to the device since the stream was opened.
    Progress { frames: u64, sample_rate: u32 },
}

#[derive(Debug, thiserror::Error)]
pub enum PlayError {
    #[error(transparent)]
    Output(#[from] OutputError),
    #[error(transparent)]
    Render(#[from] RenderError),
}

/// Plays the files in order and returns when the last one ends.
pub fn play(
    paths: impl IntoIterator<Item = PathBuf>,
    options: &PlayOptions,
    mut on_event: impl FnMut(PlayEvent),
) -> Result<StreamStats, PlayError> {
    let mut renderer = Renderer::new(paths);
    let mut stream: Option<OutputStream> = None;
    let mut totals = StreamStats::default();
    let mut sample_rate = 0;
    let mut reported_second = u64::MAX;
    let mut buffer = Vec::new();

    loop {
        buffer.clear();
        match renderer.step(&mut buffer) {
            Ok(Step::FormatChange { spec }) => {
                // The old stream plays out before the device is reopened in a new format.
                if let Some(old) = stream.take() {
                    let stats = old.finish()?;
                    totals.frames_played += stats.frames_played;
                    totals.underruns += stats.underruns;
                }
                let request = StreamRequest { device_id: options.device_id.clone(), mode: options.mode, spec };
                let (opened_stream, opened) = OutputStream::open(request)?;
                renderer.set_output(opened.format.sample);
                sample_rate = opened.format.sample_rate;
                reported_second = u64::MAX;
                on_event(PlayEvent::Opened {
                    device_name: opened.device_name,
                    path: opened.path,
                    buffer_frames: opened.buffer_frames,
                });
                stream = Some(opened_stream);
            }
            Ok(Step::TrackStart { index, path, spec, total_frames }) => {
                on_event(PlayEvent::TrackStart { index, path, spec, total_frames });
            }
            Ok(Step::Audio { .. }) => {
                let Some(active) = stream.as_mut() else {
                    return Err(RenderError::OutputNotSet.into());
                };
                active.write(&buffer)?;
                let frames = active.frames_played();
                let second = frames / u64::from(sample_rate.max(1));
                if second != reported_second {
                    reported_second = second;
                    on_event(PlayEvent::Progress { frames, sample_rate });
                }
            }
            Ok(Step::End) => break,
            Err(RenderError::Decode(error)) => on_event(PlayEvent::FileSkipped { reason: error.to_string() }),
            Err(error) => return Err(error.into()),
        }
    }

    if let Some(last) = stream.take() {
        let stats = last.finish()?;
        totals.frames_played += stats.frames_played;
        totals.underruns += stats.underruns;
    }
    Ok(totals)
}
