//! The playback engine: one thread that takes commands and reports events.
//!
//! The engine plays a queue of files. It knows nothing about albums or the library;
//! whoever owns the queue decides what goes into it.
//!
//! Events describe what is audible. The renderer runs ahead of the device by the size
//! of the ring buffer, so a track start is reported when its first frame reaches the
//! device, not when it was decoded.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use crate::output::OutputError;
use crate::output::wasapi::{OutputStream, StreamRequest};
use crate::path::{OutputMode, SignalPath};
use crate::render::{RenderError, Renderer, Step};
use crate::spec::StreamSpec;

/// How often the position is reported while playing.
const POSITION_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
pub enum Command {
    /// Replaces the queue and starts playing at `index`.
    Play { queue: Vec<PathBuf>, index: usize },
    /// Jumps to another track of the current queue.
    Jump { index: usize },
    Pause,
    Resume,
    Stop,
    /// Moves inside the current track, in seconds from its start.
    Seek { seconds: f64 },
    /// Volume 0.0..=1.0. Works in shared mode; exclusive mode plays at full scale.
    SetVolume(f32),
    /// Changes the device or the mode. Playback continues from the same place.
    SetOutput { device_id: Option<String>, mode: OutputMode },
    Shutdown,
}

#[derive(Clone, Debug)]
pub enum Event {
    /// The device was opened for a stream format.
    Opened { device_name: String, path: SignalPath },
    /// A track became audible.
    TrackStarted { index: usize, path: PathBuf, spec: StreamSpec, total_frames: Option<u64> },
    Position { index: usize, frames: u64, sample_rate: u32 },
    Paused(bool),
    /// A file could not be played and was skipped.
    FileSkipped { reason: String },
    /// Playback ended. `finished` is true when the queue played to its end.
    Stopped { finished: bool },
    Error { message: String },
}

/// Handle to a running engine. Dropping it shuts the engine down.
pub struct Engine {
    commands: Sender<Command>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Engine {
    /// Starts the engine thread. `on_event` is called from that thread.
    pub fn start(on_event: impl FnMut(Event) + Send + 'static) -> Self {
        let (commands, receiver) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("vyra-engine".into())
            .spawn(move || Worker::new(receiver, Box::new(on_event)).run())
            .expect("the engine thread could not be started");
        Self { commands, thread: Some(thread) }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A track start waiting for its first frame to reach the device.
struct Mark {
    /// Frames written to the current stream before this track.
    at: u64,
    index: usize,
    path: PathBuf,
    spec: StreamSpec,
    total_frames: Option<u64>,
    /// Position inside the track its first written frame has.
    base: u64,
}

/// The track that is audible now.
struct Audible {
    index: usize,
    spec: StreamSpec,
    /// Stream frame at which the track became audible.
    start: u64,
    base: u64,
}

struct Session {
    queue: Vec<PathBuf>,
    renderer: Renderer,
    stream: Option<OutputStream>,
    frame_bytes: usize,
    /// Frames written into the current stream.
    written: u64,
    /// Rendered bytes the stream has not taken yet.
    pending: Vec<u8>,
    pending_offset: usize,
    marks: VecDeque<Mark>,
    audible: Option<Audible>,
    /// Frame to seek to as soon as the first track is opened.
    start_frame: u64,
    paused: bool,
    /// The renderer has nothing more; the stream is playing out.
    ended: bool,
}

struct Worker {
    commands: Receiver<Command>,
    on_event: Box<dyn FnMut(Event) + Send>,
    session: Option<Session>,
    device_id: Option<String>,
    mode: OutputMode,
    volume: f32,
    last_position: Instant,
}

impl Worker {
    fn new(commands: Receiver<Command>, on_event: Box<dyn FnMut(Event) + Send>) -> Self {
        Self {
            commands,
            on_event,
            session: None,
            device_id: None,
            mode: OutputMode::Shared,
            volume: 1.0,
            last_position: Instant::now(),
        }
    }

    fn emit(&mut self, event: Event) {
        (self.on_event)(event);
    }

    fn run(mut self) {
        loop {
            let busy = self.session.as_ref().is_some_and(|s| !s.paused);
            let command = if busy {
                match self.commands.try_recv() {
                    Ok(command) => Some(command),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            } else {
                match self.commands.recv_timeout(Duration::from_millis(100)) {
                    Ok(command) => Some(command),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            };

            if let Some(command) = command {
                if matches!(command, Command::Shutdown) {
                    self.close(false);
                    return;
                }
                self.handle(command);
                continue;
            }

            if let Err(message) = self.advance() {
                self.emit(Event::Error { message });
                self.close(false);
            }
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Play { queue, index } => self.begin(queue, index, 0, false),
            Command::Jump { index } => {
                if let Some(session) = self.session.take() {
                    let paused = session.paused;
                    drop(session.stream);
                    self.begin(session.queue, index, 0, paused);
                }
            }
            Command::Pause => self.set_paused(true),
            Command::Resume => self.set_paused(false),
            Command::Stop => self.close(false),
            Command::Seek { seconds } => {
                let Some(position) = self.audible_position() else { return };
                let frame = (seconds.max(0.0) * f64::from(position.sample_rate)) as u64;
                self.restart_at(position.index, frame);
            }
            Command::SetVolume(volume) => {
                self.volume = volume.clamp(0.0, 1.0);
                if let Some(stream) = self.session.as_ref().and_then(|s| s.stream.as_ref()) {
                    stream.set_gain(self.volume);
                }
            }
            Command::SetOutput { device_id, mode } => {
                self.device_id = device_id;
                self.mode = mode;
                if let Some(position) = self.audible_position() {
                    self.restart_at(position.index, position.frames);
                }
            }
            Command::Shutdown => {}
        }
    }

    /// Starts a queue at a track and a frame inside it.
    fn begin(&mut self, queue: Vec<PathBuf>, index: usize, frame: u64, paused: bool) {
        self.close_quietly();
        if index >= queue.len() {
            self.emit(Event::Stopped { finished: true });
            return;
        }
        let renderer = Renderer::starting_at(queue[index..].iter().cloned(), index);
        self.session = Some(Session {
            queue,
            renderer,
            stream: None,
            frame_bytes: 0,
            written: 0,
            pending: Vec::new(),
            pending_offset: 0,
            marks: VecDeque::new(),
            audible: None,
            start_frame: frame,
            paused,
            ended: false,
        });
        self.last_position = Instant::now() - POSITION_INTERVAL;
    }

    fn restart_at(&mut self, index: usize, frame: u64) {
        if let Some(session) = self.session.take() {
            let paused = session.paused;
            drop(session.stream);
            self.begin(session.queue, index, frame, paused);
        }
    }

    fn set_paused(&mut self, paused: bool) {
        let Some(session) = self.session.as_mut() else { return };
        if session.paused == paused {
            return;
        }
        session.paused = paused;
        if let Some(stream) = &session.stream {
            stream.set_paused(paused);
        }
        self.emit(Event::Paused(paused));
    }

    fn close(&mut self, finished: bool) {
        if self.session.take().is_some() {
            self.emit(Event::Stopped { finished });
        }
    }

    fn close_quietly(&mut self) {
        self.session = None;
    }

    fn audible_position(&self) -> Option<Position> {
        let session = self.session.as_ref()?;
        match (&session.audible, &session.stream) {
            (Some(audible), Some(stream)) => Some(Position {
                index: audible.index,
                frames: audible.base + stream.frames_played().saturating_sub(audible.start),
                sample_rate: audible.spec.sample_rate,
            }),
            // Nothing is audible yet: a track was asked for and is still being opened.
            _ => session.marks.front().map(|mark| Position {
                index: mark.index,
                frames: mark.base,
                sample_rate: mark.spec.sample_rate,
            }),
        }
    }

    /// One round of work while playing: feed the stream, follow the audible timeline.
    fn advance(&mut self) -> Result<(), String> {
        let Some(session) = self.session.as_mut() else { return Ok(()) };
        if session.paused {
            return Ok(());
        }
        let mut events = Vec::new();
        let mut idle = true;

        // Push rendered audio that is still waiting.
        if session.pending_offset < session.pending.len() {
            let stream = session.stream.as_mut().ok_or("audio was rendered without an output stream")?;
            let taken = stream.try_write(&session.pending[session.pending_offset..]).map_err(|e| e.to_string())?;
            if taken > 0 {
                session.pending_offset += taken;
                idle = false;
            }
        } else if !session.ended {
            idle = false;
            session.pending.clear();
            session.pending_offset = 0;
            match session.renderer.step(&mut session.pending) {
                Ok(Step::Audio { frames }) => session.written += frames as u64,
                Ok(Step::TrackStart { index, path, spec, total_frames }) => {
                    let base = std::mem::take(&mut session.start_frame);
                    if base > 0 {
                        session.renderer.seek(base).map_err(|e| e.to_string())?;
                    }
                    session.marks.push_back(Mark { at: session.written, index, path, spec, total_frames, base });
                }
                Ok(Step::FormatChange { spec }) => {
                    // The old stream plays out before the device is opened in a new format.
                    if let Some(old) = session.stream.take() {
                        old.finish().map_err(|e| e.to_string())?;
                        while let Some(mark) = session.marks.pop_front() {
                            events.push(track_started(&mark));
                            session.audible = None;
                        }
                    }
                    let request = StreamRequest {
                        device_id: self.device_id.clone(),
                        mode: self.mode,
                        spec,
                        gain: self.volume,
                        paused: session.paused,
                    };
                    match OutputStream::open(request) {
                        Ok((stream, opened)) => {
                            session.renderer.set_output(opened.format.sample);
                            session.frame_bytes = opened.format.frame_bytes();
                            session.written = 0;
                            session.audible = None;
                            session.stream = Some(stream);
                            events.push(Event::Opened { device_name: opened.device_name, path: opened.path });
                        }
                        Err(error) => return Err(describe_output_error(&error)),
                    }
                }
                Ok(Step::End) => {
                    session.ended = true;
                    if let Some(stream) = &session.stream {
                        stream.mark_finished();
                    }
                }
                Err(RenderError::Decode(error)) => events.push(Event::FileSkipped { reason: error.to_string() }),
                Err(error) => return Err(error.to_string()),
            }
        }

        // Follow what the device has actually played.
        let mut finished = false;
        if let Some(stream) = &session.stream {
            let played = stream.frames_played();
            while session.marks.front().is_some_and(|mark| mark.at <= played) {
                let mark = session.marks.pop_front().expect("the front mark was just checked");
                events.push(track_started(&mark));
                session.audible = Some(Audible { index: mark.index, spec: mark.spec, start: mark.at, base: mark.base });
            }
            if session.ended && stream.has_ended() {
                finished = true;
            }
        } else if session.ended {
            finished = true;
        }

        let position = if self.last_position.elapsed() >= POSITION_INTERVAL { self.audible_position() } else { None };
        for event in events {
            self.emit(event);
        }
        if let Some(position) = position {
            self.last_position = Instant::now();
            self.emit(Event::Position {
                index: position.index,
                frames: position.frames,
                sample_rate: position.sample_rate,
            });
        }

        if finished {
            if let Some(stream) = self.session.as_mut().and_then(|s| s.stream.take()) {
                stream.finish().map_err(|e| e.to_string())?;
            }
            self.close(true);
        } else if idle {
            thread::sleep(Duration::from_millis(4));
        }
        Ok(())
    }
}

struct Position {
    index: usize,
    frames: u64,
    sample_rate: u32,
}

fn track_started(mark: &Mark) -> Event {
    Event::TrackStarted {
        index: mark.index,
        path: mark.path.clone(),
        spec: mark.spec,
        total_frames: mark.total_frames,
    }
}

fn describe_output_error(error: &OutputError) -> String {
    error.to_string()
}
