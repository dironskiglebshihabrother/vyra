//! Turns a list of files into one continuous stream of packed bytes.

use std::collections::VecDeque;
use std::path::PathBuf;

use crate::decoder::{DecodeError, SymphoniaDecoder};
use crate::pack::{DeviceSample, pack};
use crate::spec::StreamSpec;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error(transparent)]
    Decode(#[from] DecodeError),
    #[error("the output layout was not set after a format change")]
    OutputNotSet,
}

/// What one call to [`Renderer::step`] produced.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Packed audio was appended to the buffer.
    Audio { frames: usize },
    /// A track begins. Its first frame is the next one appended.
    TrackStart { index: usize, path: PathBuf, spec: StreamSpec, total_frames: Option<u64> },
    /// The coming track has another stream format than the active one.
    ///
    /// The caller finishes the old output stream, opens a new one for `spec` and calls
    /// [`Renderer::set_output`]. Until then no audio is produced.
    FormatChange { spec: StreamSpec },
    /// The list is finished.
    End,
}

/// Decodes files one after another and packs their samples for a device.
///
/// Tracks with the same stream format are joined without a single extra or missing frame.
/// The decoder trims encoder delay and padding, so the join is exact for gapless albums.
pub struct Renderer {
    queue: VecDeque<PathBuf>,
    current: Option<SymphoniaDecoder>,
    /// Index of the current track in the original list.
    index: usize,
    opened: usize,
    announced: bool,
    /// The stream format and device layout the output is open for.
    active: Option<(StreamSpec, DeviceSample)>,
    format_change_sent: bool,
}

impl Renderer {
    pub fn new(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            queue: paths.into_iter().collect(),
            current: None,
            index: 0,
            opened: 0,
            announced: false,
            active: None,
            format_change_sent: false,
        }
    }

    /// Like [`Renderer::new`], but track numbering starts at `first_index`.
    ///
    /// Used when playback starts in the middle of a queue: the renderer gets the rest
    /// of the queue and reports indexes of the whole queue.
    pub fn starting_at(paths: impl IntoIterator<Item = PathBuf>, first_index: usize) -> Self {
        let mut renderer = Self::new(paths);
        renderer.opened = first_index;
        renderer
    }

    /// Adds a file to the end of the list.
    pub fn push(&mut self, path: PathBuf) {
        self.queue.push_back(path);
    }

    /// Moves the current track to the given frame. Call it right after a
    /// [`Step::TrackStart`], before any audio of the track was taken.
    pub fn seek(&mut self, frame: u64) -> Result<(), RenderError> {
        match &mut self.current {
            Some(decoder) => Ok(decoder.seek(frame)?),
            None => Ok(()),
        }
    }

    /// Tells the renderer which layout the output accepts for the format announced by
    /// the last [`Step::FormatChange`].
    pub fn set_output(&mut self, sample: DeviceSample) {
        if let Some(decoder) = &self.current {
            self.active = Some((decoder.spec(), sample));
            self.format_change_sent = false;
        }
    }

    /// Does the next piece of work. Audio is appended to `out`.
    ///
    /// An error concerns one file only. The bad file is dropped from the list, so calling
    /// `step` again moves on to the next track.
    pub fn step(&mut self, out: &mut Vec<u8>) -> Result<Step, RenderError> {
        loop {
            if self.current.is_none() {
                let Some(path) = self.queue.pop_front() else {
                    return Ok(Step::End);
                };
                self.index = self.opened;
                self.opened += 1;
                self.announced = false;
                self.current = Some(SymphoniaDecoder::open(&path)?);
            }
            let decoder = self.current.as_mut().expect("a decoder was just opened");
            let spec = decoder.spec();

            let sample = match self.active {
                Some((active_spec, sample)) if active_spec == spec => sample,
                _ if self.format_change_sent => return Err(RenderError::OutputNotSet),
                _ => {
                    self.format_change_sent = true;
                    return Ok(Step::FormatChange { spec });
                }
            };

            if !self.announced {
                self.announced = true;
                return Ok(Step::TrackStart {
                    index: self.index,
                    path: decoder.path().to_path_buf(),
                    spec,
                    total_frames: decoder.total_frames(),
                });
            }

            match decoder.next_block() {
                Ok(Some(block)) => {
                    let frames = block.frames();
                    pack(&block, sample, out);
                    return Ok(Step::Audio { frames });
                }
                Ok(None) => self.current = None,
                Err(e) => {
                    self.current = None;
                    return Err(e.into());
                }
            }
        }
    }
}
