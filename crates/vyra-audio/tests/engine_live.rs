//! Engine checks that need a real output device. They play digital silence, so nothing
//! is heard. Run them with `cargo test -p vyra-audio --test engine_live -- --ignored`.

#![cfg(windows)]

mod common;

use std::sync::mpsc;
use std::time::{Duration, Instant};

use common::{Container, Pcm};
use vyra_audio::engine::{Command, Engine, Event};

fn silence(seconds: usize, sample_rate: u32) -> Pcm {
    Pcm { bits: 16, sample_rate, channels: 2, samples: vec![0; seconds * sample_rate as usize * 2] }
}

fn wait_for<T>(events: &mpsc::Receiver<Event>, timeout: Duration, mut pick: impl FnMut(&Event) -> Option<T>) -> T {
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = events.recv_timeout(left).expect("the expected event did not arrive in time");
        if let Event::Error { message } = &event {
            panic!("engine error: {message}");
        }
        if let Some(value) = pick(&event) {
            return value;
        }
    }
}

#[test]
#[ignore = "needs an audio output device"]
fn a_queue_plays_to_the_end_and_reports_every_track() {
    let dir = tempfile::tempdir().unwrap();
    let queue = vec![
        Container::Wav.write(&silence(1, 44_100), dir.path(), "one"),
        Container::Wav.write(&silence(1, 44_100), dir.path(), "two"),
        Container::Wav.write(&silence(1, 48_000), dir.path(), "three"),
    ];

    let (sender, events) = mpsc::channel();
    let engine = Engine::start(move |event| drop(sender.send(event)));
    engine.send(Command::Play { queue, index: 0 });

    let mut started = Vec::new();
    let mut opened = 0;
    let finished = wait_for(&events, Duration::from_secs(15), |event| match event {
        Event::TrackStarted { index, .. } => {
            started.push(*index);
            None
        }
        Event::Opened { .. } => {
            opened += 1;
            None
        }
        Event::Stopped { finished } => Some(*finished),
        _ => None,
    });

    assert!(finished);
    assert_eq!(started, [0, 1, 2]);
    assert_eq!(opened, 2, "two formats mean the device is opened twice");
}

#[test]
#[ignore = "needs an audio output device"]
fn pause_seek_and_jump_work_while_playing() {
    let dir = tempfile::tempdir().unwrap();
    let queue = vec![
        Container::Wav.write(&silence(6, 44_100), dir.path(), "long"),
        Container::Wav.write(&silence(2, 44_100), dir.path(), "short"),
    ];

    let (sender, events) = mpsc::channel();
    let engine = Engine::start(move |event| drop(sender.send(event)));
    engine.send(Command::Play { queue, index: 0 });
    wait_for(&events, Duration::from_secs(5), |e| matches!(e, Event::TrackStarted { index: 0, .. }).then_some(()));

    // The position moves forward.
    let first = wait_for(&events, Duration::from_secs(3), |e| match e {
        Event::Position { frames, .. } => Some(*frames),
        _ => None,
    });
    let later = wait_for(&events, Duration::from_secs(3), |e| match e {
        Event::Position { frames, .. } if *frames > first => Some(*frames),
        _ => None,
    });
    assert!(later > first);

    // Pause holds the position.
    engine.send(Command::Pause);
    wait_for(&events, Duration::from_secs(2), |e| matches!(e, Event::Paused(true)).then_some(()));
    std::thread::sleep(Duration::from_millis(600));
    while events.try_recv().is_ok() {}
    engine.send(Command::Resume);
    wait_for(&events, Duration::from_secs(2), |e| matches!(e, Event::Paused(false)).then_some(()));

    // A seek lands near the asked second.
    engine.send(Command::Seek { seconds: 4.0 });
    let after_seek = wait_for(&events, Duration::from_secs(5), |e| match e {
        Event::Position { frames, sample_rate, index: 0 } if *frames >= 4 * u64::from(*sample_rate) => {
            Some(*frames as f64 / f64::from(*sample_rate))
        }
        _ => None,
    });
    assert!((4.0..5.0).contains(&after_seek), "position after the seek: {after_seek}");

    // A jump starts the other track, and the queue then ends by itself.
    engine.send(Command::Jump { index: 1 });
    wait_for(&events, Duration::from_secs(5), |e| matches!(e, Event::TrackStarted { index: 1, .. }).then_some(()));
    let finished = wait_for(&events, Duration::from_secs(8), |e| match e {
        Event::Stopped { finished } => Some(*finished),
        _ => None,
    });
    assert!(finished);
}
