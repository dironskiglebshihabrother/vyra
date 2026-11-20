//! Command line tool for the audio engine.
//!
//! ```text
//! vyra-cli devices
//! vyra-cli info <file>...
//! vyra-cli play [--exclusive] [--device <part of the name>] <file or folder>...
//! vyra-cli render --layout <i16|i24|i24in32|i32|f32> --out <file> <file or folder>...
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use vyra_audio::offline::render_to_vec;
use vyra_audio::output::wasapi;
use vyra_audio::player::{PlayEvent, PlayOptions, play};
use vyra_audio::{DeviceSample, OutputMode, SymphoniaDecoder};

const USAGE: &str = "\
usage:
  vyra-cli devices
  vyra-cli info <file>...
  vyra-cli play [--exclusive] [--device <part of the name>] <file or folder>...
  vyra-cli render --layout <i16|i24|i24in32|i32|f32> --out <file> <file or folder>...";

const EXTENSIONS: &[&str] = &["mp3", "flac", "wav", "aiff", "aif", "m4a", "aac", "ogg", "oga"];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((command, rest)) => match command.as_str() {
            "devices" => devices(),
            "info" => info(rest),
            "play" => play_command(rest),
            "render" => render_command(rest),
            _ => Err(USAGE.to_string()),
        },
        None => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn devices() -> Result<(), String> {
    for device in wasapi::devices().map_err(|e| e.to_string())? {
        let mark = if device.is_default { "*" } else { " " };
        println!("{mark} {}", device.name);
    }
    Ok(())
}

fn info(args: &[String]) -> Result<(), String> {
    let files = collect_files(args)?;
    for path in files {
        match SymphoniaDecoder::open(&path) {
            Ok(decoder) => {
                let length = decoder
                    .total_frames()
                    .map(|frames| format_time(frames, decoder.spec().sample_rate))
                    .unwrap_or_else(|| "unknown length".into());
                println!("{}: {}, {length}", file_name(&path), decoder.spec());
            }
            Err(error) => println!("{error}"),
        }
    }
    Ok(())
}

fn play_command(args: &[String]) -> Result<(), String> {
    let mut mode = OutputMode::Shared;
    let mut device_filter: Option<String> = None;
    let mut inputs = Vec::new();

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--exclusive" => mode = OutputMode::Exclusive,
            "--device" => device_filter = Some(iter.next().ok_or("--device needs a name")?.to_lowercase()),
            other => inputs.push(other.to_string()),
        }
    }
    let files = collect_files(&inputs)?;

    let device_id = match device_filter {
        Some(filter) => {
            let all = wasapi::devices().map_err(|e| e.to_string())?;
            let found = all
                .into_iter()
                .find(|d| d.name.to_lowercase().contains(&filter))
                .ok_or_else(|| format!("no output device has \"{filter}\" in its name"))?;
            Some(found.id)
        }
        None => None,
    };

    let options = PlayOptions { device_id, mode };
    let stats = play(files, &options, |event| match event {
        PlayEvent::Opened { device_name, path, buffer_frames } => {
            println!("\n{device_name}, buffer {buffer_frames} frames");
            println!("{path}");
        }
        PlayEvent::TrackStart { index, path, total_frames, spec } => {
            let length = total_frames.map(|f| format_time(f, spec.sample_rate)).unwrap_or_default();
            println!("\n{}. {} {length}", index + 1, file_name(&path));
        }
        PlayEvent::FileSkipped { reason } => println!("\nskipped: {reason}"),
        PlayEvent::Progress { frames, sample_rate } => {
            print!("\r   {}", format_time(frames, sample_rate));
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }
    })
    .map_err(|e| e.to_string())?;

    println!("\n\nframes played: {}, underruns: {}", stats.frames_played, stats.underruns);
    Ok(())
}

fn render_command(args: &[String]) -> Result<(), String> {
    let mut layout = None;
    let mut out = None;
    let mut inputs = Vec::new();

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--layout" => {
                layout = Some(match iter.next().map(String::as_str) {
                    Some("i16") => DeviceSample::I16,
                    Some("i24") => DeviceSample::I24,
                    Some("i24in32") => DeviceSample::I24In32,
                    Some("i32") => DeviceSample::I32,
                    Some("f32") => DeviceSample::F32,
                    _ => return Err("--layout takes i16, i24, i24in32, i32 or f32".into()),
                });
            }
            "--out" => out = Some(PathBuf::from(iter.next().ok_or("--out needs a file name")?)),
            other => inputs.push(other.to_string()),
        }
    }
    let layout = layout.ok_or(USAGE)?;
    let out = out.ok_or(USAGE)?;
    let files = collect_files(&inputs)?;

    let rendered = render_to_vec(files, |_| layout).map_err(|e| e.to_string())?;
    for track in &rendered.tracks {
        println!("{:>12}  {}  {}", track.byte_offset, file_name(&track.path), track.spec);
    }
    std::fs::write(&out, &rendered.bytes).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    println!("{} bytes written to {}", rendered.bytes.len(), out.display());
    Ok(())
}

/// Expands folders into the audio files they hold, in natural name order.
fn collect_files(inputs: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for input in inputs {
        let path = PathBuf::from(input);
        if path.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(&path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?
                .filter_map(|entry| entry.ok().map(|e| e.path()))
                .filter(|p| p.is_file() && is_audio(p))
                .collect();
            entries.sort_by_cached_key(|p| natural_key(&file_name(p)));
            files.extend(entries);
        } else {
            files.push(path);
        }
    }
    if files.is_empty() {
        return Err(USAGE.to_string());
    }
    Ok(files)
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

/// Sort key that puts `2.mp3` before `10.mp3`.
fn natural_key(name: &str) -> Vec<(u8, u64, String)> {
    let mut key = Vec::new();
    let mut chars = name.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            let mut number = 0u64;
            while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                number = number.saturating_mul(10).saturating_add(u64::from(d.to_digit(10).unwrap_or(0)));
                chars.next();
            }
            key.push((0, number, String::new()));
        } else {
            let mut text = String::new();
            while let Some(&t) = chars.peek().filter(|t| !t.is_ascii_digit()) {
                text.extend(t.to_lowercase());
                chars.next();
            }
            key.push((1, 0, text));
        }
    }
    key
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

fn format_time(frames: u64, sample_rate: u32) -> String {
    let seconds = frames / u64::from(sample_rate.max(1));
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::natural_key;

    #[test]
    fn numbers_sort_by_value() {
        let mut names = vec!["10 - b.flac", "2 - a.flac", "1 - c.flac", "Track 11.mp3", "track 9.mp3"];
        names.sort_by_cached_key(|n| natural_key(n));
        assert_eq!(names, ["1 - c.flac", "2 - a.flac", "10 - b.flac", "track 9.mp3", "Track 11.mp3"]);
    }
}
