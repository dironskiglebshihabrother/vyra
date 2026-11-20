//! Output through WASAPI, shared and exclusive.
//!
//! Every stream owns one thread. The thread opens the device, registers itself with the
//! multimedia scheduler and then only moves bytes from a ring buffer to the device. It
//! does not allocate, lock or log while audio is running.

use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use wasapi::{
    AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WasapiError, WaveFormat,
    calculate_period_100ns, initialize_mta,
};
use windows::Win32::Media::Audio::{
    AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED, AUDCLNT_E_DEVICE_IN_USE, AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED,
    AUDCLNT_E_UNSUPPORTED_FORMAT,
};
use windows::Win32::System::Threading::{AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW};
use windows::core::w;

use super::{DeviceInfo, OutputError};
use crate::pack::{DeviceFormat, DeviceSample};
use crate::path::{OutputMode, SignalPath, preferred_layouts};
use crate::spec::StreamSpec;

/// How much audio the ring buffer between the engine and the device holds.
const RING_MILLIS: u64 = 500;
/// How long the device may stay silent before the stream is treated as broken.
const EVENT_TIMEOUT_MS: u32 = 2000;

impl From<WasapiError> for OutputError {
    fn from(error: WasapiError) -> Self {
        if let WasapiError::Windows(windows_error) = &error {
            let code = windows_error.code();
            if code == AUDCLNT_E_DEVICE_IN_USE {
                return OutputError::DeviceBusy;
            }
            if code == AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED {
                return OutputError::ExclusiveNotAllowed;
            }
        }
        OutputError::Backend(error.to_string())
    }
}

/// Lists the output devices that are plugged in and enabled.
pub fn devices() -> Result<Vec<DeviceInfo>, OutputError> {
    let _ = initialize_mta();
    let enumerator = DeviceEnumerator::new()?;
    let default_id = enumerator.get_default_device(Direction::Render).and_then(|d| d.get_id()).ok();

    let collection = enumerator.get_device_collection(Direction::Render)?;
    let mut list = Vec::new();
    for index in 0..collection.get_nbr_devices()? {
        let device = collection.get_device_at_index(index)?;
        let id = device.get_id()?;
        list.push(DeviceInfo {
            is_default: default_id.as_deref() == Some(id.as_str()),
            name: device.get_friendlyname().unwrap_or_else(|_| id.clone()),
            id,
        });
    }
    Ok(list)
}

/// What to open.
#[derive(Clone, Debug)]
pub struct StreamRequest {
    /// Device identifier from [`devices`], or `None` for the system default.
    pub device_id: Option<String>,
    pub mode: OutputMode,
    /// Format of the audio that will be written.
    pub spec: StreamSpec,
    /// Starting volume, 0.0..=1.0. Has an effect in shared mode only.
    pub gain: f32,
    /// Start silent and wait for [`OutputStream::set_paused`].
    pub paused: bool,
}

/// What was opened.
#[derive(Clone, Debug)]
pub struct Opened {
    pub device_name: String,
    /// Layout the caller must pack audio into.
    pub format: DeviceFormat,
    pub path: SignalPath,
    /// Size of the device buffer in frames.
    pub buffer_frames: u32,
}

/// Counters of a finished stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamStats {
    pub frames_played: u64,
    /// Times the device asked for audio and the ring buffer had none.
    pub underruns: u64,
}

struct Shared {
    stop: AtomicBool,
    finish: AtomicBool,
    paused: AtomicBool,
    frames_played: AtomicU64,
    underruns: AtomicU64,
    failed: AtomicBool,
    error: Mutex<Option<String>>,
    /// Volume as the bits of an `f32`. Applied to float streams only, so the direct
    /// path of exclusive mode stays untouched.
    gain: AtomicU32,
}

impl Shared {
    fn new(gain: f32) -> Self {
        Self {
            stop: AtomicBool::new(false),
            finish: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            frames_played: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            failed: AtomicBool::new(false),
            error: Mutex::new(None),
            gain: AtomicU32::new(gain.to_bits()),
        }
    }
}

/// A running output stream. Audio written to it is played by the device thread.
pub struct OutputStream {
    producer: rtrb::Producer<u8>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl OutputStream {
    /// Opens the device and starts the stream thread. Returns when the format is agreed.
    pub fn open(request: StreamRequest) -> Result<(Self, Opened), OutputError> {
        let ring_bytes = ring_capacity(&request.spec);
        let (producer, consumer) = rtrb::RingBuffer::<u8>::new(ring_bytes);
        let shared = Arc::new(Shared::new(request.gain));
        shared.paused.store(request.paused, Ordering::Relaxed);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);

        let thread_shared = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("vyra-output".into())
            .spawn(move || stream_thread(request, consumer, thread_shared, ready_tx))
            .map_err(|e| OutputError::Backend(e.to_string()))?;

        match ready_rx.recv() {
            Ok(Ok(opened)) => Ok((Self { producer, shared, thread: Some(thread) }, opened)),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(OutputError::Backend("the output thread ended before opening the device".into()))
            }
        }
    }

    /// Queues audio for playback. Blocks while the ring buffer is full.
    pub fn write(&mut self, mut bytes: &[u8]) -> Result<(), OutputError> {
        while !bytes.is_empty() {
            self.check()?;
            match self.producer.write(bytes) {
                Ok(written) => bytes = &bytes[written..],
                Err(e) if e.kind() == ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(2)),
                Err(e) => return Err(OutputError::Backend(e.to_string())),
            }
        }
        Ok(())
    }

    /// Queues as much as fits right now and returns the number of bytes taken.
    pub fn try_write(&mut self, bytes: &[u8]) -> Result<usize, OutputError> {
        self.check()?;
        if bytes.is_empty() {
            return Ok(0);
        }
        match self.producer.write(bytes) {
            Ok(written) => Ok(written),
            Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(0),
            Err(e) => Err(OutputError::Backend(e.to_string())),
        }
    }

    /// Sets the volume, 0.0..=1.0. Has an effect in shared mode only.
    pub fn set_gain(&self, gain: f32) {
        self.shared.gain.store(gain.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    /// Tells the stream that no more audio will come. It plays out what it has.
    pub fn mark_finished(&self) {
        self.shared.finish.store(true, Ordering::Release);
    }

    /// True once the stream thread has ended, after a finish, a stop or a failure.
    pub fn has_ended(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Frames handed to the device so far.
    pub fn frames_played(&self) -> u64 {
        self.shared.frames_played.load(Ordering::Relaxed)
    }

    pub fn underruns(&self) -> u64 {
        self.shared.underruns.load(Ordering::Relaxed)
    }

    /// Plays silence while paused. Queued audio stays in the buffer.
    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
    }

    /// Plays everything that was written, then closes the device.
    pub fn finish(mut self) -> Result<StreamStats, OutputError> {
        self.shared.finish.store(true, Ordering::Release);
        self.join()
    }

    /// Closes the device at once. Queued audio is dropped.
    pub fn stop(mut self) -> Result<StreamStats, OutputError> {
        self.shared.stop.store(true, Ordering::Release);
        self.join()
    }

    fn join(&mut self) -> Result<StreamStats, OutputError> {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.check()?;
        Ok(StreamStats { frames_played: self.frames_played(), underruns: self.underruns() })
    }

    fn check(&self) -> Result<(), OutputError> {
        if self.shared.failed.load(Ordering::Acquire) {
            let message = self.shared.error.lock().ok().and_then(|mut e| e.take());
            return Err(OutputError::Stopped(message.unwrap_or_else(|| "unknown reason".into())));
        }
        Ok(())
    }
}

impl Drop for OutputStream {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn ring_capacity(spec: &StreamSpec) -> usize {
    // Four bytes per sample is the widest layout a device takes.
    let bytes_per_second = u64::from(spec.sample_rate) * u64::from(spec.channels) * 4;
    (bytes_per_second * RING_MILLIS / 1000) as usize
}

fn wave_format(sample: DeviceSample, spec: &StreamSpec) -> WaveFormat {
    let (store_bits, valid_bits, kind) = match sample {
        DeviceSample::I16 => (16, 16, SampleType::Int),
        DeviceSample::I24 => (24, 24, SampleType::Int),
        DeviceSample::I24In32 => (32, 24, SampleType::Int),
        DeviceSample::I32 => (32, 32, SampleType::Int),
        DeviceSample::F32 => (32, 32, SampleType::Float),
    };
    WaveFormat::new(store_bits, valid_bits, kind, spec.sample_rate as usize, usize::from(spec.channels), None)
}

struct OpenClient {
    client: AudioClient,
    opened: Opened,
}

fn open_client(request: &StreamRequest) -> Result<OpenClient, OutputError> {
    let enumerator = DeviceEnumerator::new()?;
    let device = match &request.device_id {
        Some(id) => enumerator.get_device(id).map_err(|_| OutputError::DeviceNotFound)?,
        None => enumerator.get_default_device(Direction::Render).map_err(|_| OutputError::DeviceNotFound)?,
    };
    let device_name = device.get_friendlyname().unwrap_or_else(|_| "output device".into());
    let mut client = device.get_iaudioclient()?;
    let (default_period, _minimum_period) = client.get_device_period()?;
    let spec = request.spec;

    match request.mode {
        OutputMode::Shared => {
            // The mixer takes float. Windows converts the rate and the channels to its own
            // mix format; the report names that conversion.
            let format = wave_format(DeviceSample::F32, &spec);
            let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: default_period };
            client.initialize_client(&format, Direction::Render, mode, None)?;

            let mix = client.get_mixformat()?;
            let mixer_side = DeviceFormat {
                sample_rate: mix.get_samplespersec(),
                channels: mix.get_nchannels(),
                sample: DeviceSample::F32,
            };
            let buffer_frames = client.get_buffer_size()?;
            Ok(OpenClient {
                client,
                opened: Opened {
                    device_name,
                    format: DeviceFormat { sample_rate: spec.sample_rate, channels: spec.channels, sample: DeviceSample::F32 },
                    path: SignalPath::evaluate(spec, mixer_side, OutputMode::Shared),
                    buffer_frames,
                },
            })
        }
        OutputMode::Exclusive => {
            let mut chosen = None;
            for sample in preferred_layouts(spec) {
                if let Ok(accepted) = client.is_supported_exclusive_with_quirks(&wave_format(sample, &spec)) {
                    chosen = Some((sample, accepted));
                    break;
                }
            }
            let (sample, format) = chosen.ok_or(OutputError::FormatNotSupported { spec })?;

            // Buffers of Intel HDA devices must be a multiple of 128 bytes.
            let period = client.calculate_aligned_period_near(default_period, Some(128), &format)?;
            let mode = StreamMode::EventsExclusive { period_hns: period };
            match client.initialize_client(&format, Direction::Render, mode, None) {
                Ok(()) => {}
                Err(WasapiError::Windows(e)) if e.code() == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED => {
                    // The device reports the nearest size it accepts. Open again with it.
                    let aligned_frames = client.get_buffer_size()?;
                    let aligned = calculate_period_100ns(i64::from(aligned_frames), i64::from(spec.sample_rate));
                    client = device.get_iaudioclient()?;
                    client.initialize_client(
                        &format,
                        Direction::Render,
                        StreamMode::EventsExclusive { period_hns: aligned },
                        None,
                    )?;
                }
                Err(WasapiError::Windows(e)) if e.code() == AUDCLNT_E_UNSUPPORTED_FORMAT => {
                    return Err(OutputError::FormatNotSupported { spec });
                }
                Err(e) => return Err(e.into()),
            }

            let device_format = DeviceFormat { sample_rate: spec.sample_rate, channels: spec.channels, sample };
            let buffer_frames = client.get_buffer_size()?;
            Ok(OpenClient {
                client,
                opened: Opened {
                    device_name,
                    format: device_format,
                    path: SignalPath::evaluate(spec, device_format, OutputMode::Exclusive),
                    buffer_frames,
                },
            })
        }
    }
}

fn stream_thread(
    request: StreamRequest,
    consumer: rtrb::Consumer<u8>,
    shared: Arc<Shared>,
    ready: mpsc::SyncSender<Result<Opened, OutputError>>,
) {
    let _ = initialize_mta();
    let open = match open_client(&request) {
        Ok(open) => open,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let layout = StreamLayout {
        frame_bytes: open.opened.format.frame_bytes(),
        buffer_frames: open.opened.buffer_frames as usize,
        sample_rate: open.opened.format.sample_rate,
        channels: usize::from(open.opened.format.channels),
        is_float: open.opened.format.sample == DeviceSample::F32,
    };
    let _ = ready.send(Ok(open.opened));

    if let Err(error) = run_stream(&open.client, consumer, &shared, layout) {
        if let Ok(mut slot) = shared.error.lock() {
            *slot = Some(error.to_string());
        }
        shared.failed.store(true, Ordering::Release);
    }
}

#[derive(Clone, Copy)]
struct StreamLayout {
    frame_bytes: usize,
    buffer_frames: usize,
    sample_rate: u32,
    channels: usize,
    is_float: bool,
}

/// Scales float samples in place, moving smoothly from one volume to another so a
/// change of the slider does not click.
fn apply_gain(bytes: &mut [u8], channels: usize, from: f32, to: f32) {
    let frame_bytes = channels * 4;
    let frames = bytes.len() / frame_bytes;
    if frames == 0 {
        return;
    }
    let step = (to - from) / frames as f32;
    for (index, frame) in bytes.chunks_exact_mut(frame_bytes).enumerate() {
        let gain = from + step * index as f32;
        for sample in frame.as_chunks_mut::<4>().0 {
            *sample = (f32::from_le_bytes(*sample) * gain).to_le_bytes();
        }
    }
}

fn run_stream(
    client: &AudioClient,
    mut consumer: rtrb::Consumer<u8>,
    shared: &Shared,
    layout: StreamLayout,
) -> Result<(), OutputError> {
    let StreamLayout { frame_bytes, buffer_frames, sample_rate, channels, is_float } = layout;
    let event = client.set_get_eventhandle()?;
    let render = client.get_audiorenderclient()?;
    let mut scratch = vec![0u8; buffer_frames * frame_bytes];
    let mut current_gain = f32::from_bits(shared.gain.load(Ordering::Relaxed));

    // Raise the thread to the audio class of the multimedia scheduler.
    let mut task_index = 0u32;
    // SAFETY: the task name is a static string and the index points to a live local.
    let mmcss = unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task_index) }.ok();

    // Wait for the first audio, so playback starts with sound and not with a gap.
    while consumer.slots() < scratch.len().min(consumer.buffer().capacity())
        && !shared.finish.load(Ordering::Acquire)
        && !shared.stop.load(Ordering::Acquire)
    {
        thread::sleep(Duration::from_millis(2));
    }

    let mut started = false;
    let mut drained = false;
    let result = loop {
        if shared.stop.load(Ordering::Acquire) {
            break Ok(());
        }

        if started && event.wait_for_event(EVENT_TIMEOUT_MS).is_err() {
            break Err(OutputError::Stopped("the device stopped asking for audio".into()));
        }

        let free_frames = match client.get_available_space_in_frames() {
            Ok(frames) => (frames as usize).min(buffer_frames),
            Err(error) => break Err(error.into()),
        };
        if free_frames == 0 {
            if !started {
                thread::sleep(Duration::from_millis(1));
            }
            continue;
        }
        let wanted = free_frames * frame_bytes;
        let chunk = &mut scratch[..wanted];

        let mut filled = 0;
        if !shared.paused.load(Ordering::Relaxed) {
            let available = consumer.slots();
            let take = wanted.min(available - available % frame_bytes);
            if take > 0 {
                match consumer.read(&mut chunk[..take]) {
                    Ok(read) => filled = read,
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(e) => break Err(OutputError::Backend(e.to_string())),
                }
            }
            if filled < wanted {
                if shared.finish.load(Ordering::Acquire) && consumer.slots() < frame_bytes {
                    drained = true;
                } else if started {
                    shared.underruns.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        chunk[filled..].fill(0);

        if is_float && filled > 0 {
            let target_gain = f32::from_bits(shared.gain.load(Ordering::Relaxed));
            if current_gain != 1.0 || target_gain != 1.0 {
                apply_gain(&mut chunk[..filled], channels, current_gain, target_gain);
            }
            current_gain = target_gain;
        }

        if let Err(error) = render.write_to_device(chunk, None) {
            break Err(error.into());
        }
        shared.frames_played.fetch_add((filled / frame_bytes) as u64, Ordering::Relaxed);

        if !started {
            if let Err(error) = client.start_stream() {
                break Err(error.into());
            }
            started = true;
        }

        if drained {
            // Let the device play what it still holds before the stream is stopped.
            let tail_ms = (buffer_frames as u64 * 1000 / u64::from(sample_rate.max(1))) * 2 + 20;
            thread::sleep(Duration::from_millis(tail_ms));
            break Ok(());
        }
    };

    if started {
        let _ = client.stop_stream();
    }
    if let Some(handle) = mmcss {
        // SAFETY: the handle came from AvSetMmThreadCharacteristicsW on this thread.
        let _ = unsafe { AvRevertMmThreadCharacteristics(handle) };
    }
    result
}
