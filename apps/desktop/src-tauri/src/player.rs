//! The player state the window shows, kept in step with engine events.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use vyra_audio::engine::Event;
use vyra_audio::{DeviceSample, NotBitPerfect, OutputMode, SampleKind, SignalPath};
use vyra_library::Track;

use crate::AppState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Stopped,
    Playing,
    Paused,
}

/// One reason why playback is not bit-perfect, in a shape the window can word itself.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reason {
    LossySource,
    SharedMode,
    SampleRate { from: u32, to: u32 },
    Channels { from: u16, to: u16 },
    Float,
    BitDepth { from: u8, to: u8 },
}

/// The signal path line: what the file is, what the device gets.
#[derive(Clone, Debug, Serialize)]
pub struct Signal {
    pub device: String,
    pub exclusive: bool,
    pub bit_perfect: bool,
    pub source_rate: u32,
    /// Bit depth of the file, `None` for lossy formats.
    pub source_bits: Option<u8>,
    pub output_rate: u32,
    /// Bit depth the device receives, `None` for float.
    pub output_bits: Option<u8>,
    pub reasons: Vec<Reason>,
}

impl Signal {
    fn new(device: String, path: &SignalPath) -> Self {
        Self {
            device,
            exclusive: path.mode == OutputMode::Exclusive,
            bit_perfect: path.bit_perfect,
            source_rate: path.source.sample_rate,
            source_bits: match path.source.kind {
                SampleKind::Int { bits } => Some(bits),
                SampleKind::Float32 => None,
            },
            output_rate: path.output.sample_rate,
            output_bits: match path.output.sample {
                DeviceSample::F32 => None,
                other => other.valid_bits(),
            },
            reasons: path
                .reasons
                .iter()
                .map(|reason| match *reason {
                    NotBitPerfect::LossySource => Reason::LossySource,
                    NotBitPerfect::SharedMode => Reason::SharedMode,
                    NotBitPerfect::SampleRateChanged { from, to } => Reason::SampleRate { from, to },
                    NotBitPerfect::ChannelsChanged { from, to } => Reason::Channels { from, to },
                    NotBitPerfect::ConvertedToFloat => Reason::Float,
                    NotBitPerfect::BitDepthReduced { from, to } => Reason::BitDepth { from, to },
                })
                .collect(),
        }
    }
}

#[derive(Default)]
pub struct Player {
    pub queue: Vec<Track>,
    pub index: Option<usize>,
    pub status: Status,
    pub position_ms: u64,
    pub volume: f32,
    pub exclusive: bool,
    /// Where the queue came from, for example an album title.
    pub context: String,
    pub signal: Option<Signal>,
}

/// What the window needs to draw the player.
#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub status: Status,
    pub index: Option<usize>,
    pub track: Option<Track>,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub volume: f32,
    pub exclusive: bool,
    pub queue_len: usize,
    pub context: String,
    pub signal: Option<Signal>,
}

impl Player {
    pub fn current(&self) -> Option<&Track> {
        self.index.and_then(|index| self.queue.get(index))
    }

    pub fn snapshot(&self) -> Snapshot {
        let track = self.current().cloned();
        Snapshot {
            status: self.status,
            index: self.index,
            duration_ms: track.as_ref().map_or(0, |t| t.duration_ms),
            track,
            position_ms: self.position_ms,
            volume: self.volume,
            exclusive: self.exclusive,
            queue_len: self.queue.len(),
            context: self.context.clone(),
            signal: self.signal.clone(),
        }
    }
}

#[derive(Clone, Serialize)]
struct PositionPayload {
    index: usize,
    position_ms: u64,
}

#[derive(Clone, Serialize)]
struct ErrorPayload {
    message: String,
}

/// Called on the engine thread for every engine event.
pub fn on_engine_event(app: &AppHandle, event: Event) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let mut player = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    match event {
        Event::Opened { device_name, path } => {
            player.signal = Some(Signal::new(device_name, &path));
        }
        Event::TrackStarted { index, .. } => {
            player.index = Some(index);
            player.position_ms = 0;
            if player.status == Status::Stopped {
                player.status = Status::Playing;
            }
        }
        Event::Position { index, frames, sample_rate } => {
            let position_ms = frames * 1000 / u64::from(sample_rate.max(1));
            player.position_ms = position_ms;
            let _ = app.emit("position", PositionPayload { index, position_ms });
            return;
        }
        Event::Paused(paused) => {
            player.status = if paused { Status::Paused } else { Status::Playing };
        }
        Event::FileSkipped { reason } => {
            let _ = app.emit("player-error", ErrorPayload { message: reason });
            return;
        }
        Event::Stopped { finished } => {
            player.status = Status::Stopped;
            player.position_ms = 0;
            if finished {
                // The queue played out: point back at its first track.
                player.index = (!player.queue.is_empty()).then_some(0);
            }
        }
        Event::Error { message } => {
            player.status = Status::Stopped;
            let _ = app.emit("player-error", ErrorPayload { message });
        }
    }
    let _ = app.emit("player", player.snapshot());
}
