//! Commands the window calls.

use std::path::PathBuf;
use std::sync::MutexGuard;
use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vyra_audio::OutputMode;
use vyra_audio::engine::Command;
use vyra_audio::output::wasapi;
use vyra_library::{Album, AlbumDetail, Artist, Counts, ScanProgress, SearchResults, Track};

use crate::AppState;
use crate::player::{Player, Snapshot, Status};

type CommandResult<T> = Result<T, String>;

fn text(error: impl ToString) -> String {
    error.to_string()
}

fn lock_player<'a>(state: &'a State<'_, AppState>) -> MutexGuard<'a, Player> {
    state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

// Library

#[derive(Serialize)]
pub struct LibraryStatus {
    roots: Vec<String>,
    counts: Counts,
    scanning: bool,
    /// The user's Music folder, offered on first run when it exists.
    suggested_root: Option<String>,
}

#[tauri::command]
pub fn library_status(state: State<'_, AppState>) -> CommandResult<LibraryStatus> {
    let suggested_root = std::env::var_os("USERPROFILE")
        .map(|home| PathBuf::from(home).join("Music"))
        .filter(|path| path.is_dir())
        .map(|path| path.to_string_lossy().into_owned());
    Ok(LibraryStatus {
        roots: state.library.roots().map_err(text)?,
        counts: state.library.counts().map_err(text)?,
        scanning: state.scanning.load(Ordering::Relaxed),
        suggested_root,
    })
}

/// Scans the music folders on a background thread. A scan already running is left alone.
pub fn start_scan(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.library.roots().map(|roots| roots.is_empty()).unwrap_or(true) {
        return;
    }
    if state.scanning.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let progress_app = app.clone();
        let result = state.library.scan(&move |progress: ScanProgress| {
            let _ = progress_app.emit("scan", progress);
        });
        state.scanning.store(false, Ordering::Release);
        if let Err(error) = result {
            let _ = app.emit("scan-error", error.to_string());
        }
        let _ = app.emit("library-changed", ());
    });
}

/// Adds a music folder. Without a path the system folder dialog is shown.
#[tauri::command]
pub async fn add_music_folder(app: AppHandle, path: Option<String>) -> CommandResult<Option<String>> {
    let chosen = match path {
        Some(path) => Some(PathBuf::from(path)),
        None => tauri::async_runtime::spawn_blocking(|| {
            rfd::FileDialog::new().set_title("Папка с музыкой").pick_folder()
        })
        .await
        .map_err(text)?,
    };
    let Some(folder) = chosen else { return Ok(None) };

    app.state::<AppState>().library.add_root(&folder).map_err(text)?;
    start_scan(&app);
    let _ = app.emit("library-changed", ());
    Ok(Some(folder.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn remove_music_folder(app: AppHandle, state: State<'_, AppState>, path: String) -> CommandResult<()> {
    state.library.remove_root(&path).map_err(text)?;
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command]
pub fn rescan(app: AppHandle) {
    start_scan(&app);
}

#[tauri::command]
pub fn albums(state: State<'_, AppState>) -> CommandResult<Vec<Album>> {
    state.library.albums().map_err(text)
}

#[tauri::command]
pub fn recent_albums(state: State<'_, AppState>, limit: u32) -> CommandResult<Vec<Album>> {
    state.library.recent_albums(limit).map_err(text)
}

#[tauri::command]
pub fn album(state: State<'_, AppState>, id: i64) -> CommandResult<Option<AlbumDetail>> {
    state.library.album(id).map_err(text)
}

#[tauri::command]
pub fn artists(state: State<'_, AppState>) -> CommandResult<Vec<Artist>> {
    state.library.artists().map_err(text)
}

#[tauri::command]
pub fn artist_albums(state: State<'_, AppState>, name: String) -> CommandResult<Vec<Album>> {
    state.library.artist_albums(&name).map_err(text)
}

#[tauri::command]
pub fn tracks(state: State<'_, AppState>, offset: u32, limit: u32) -> CommandResult<Vec<Track>> {
    state.library.tracks(offset, limit).map_err(text)
}

#[tauri::command]
pub fn search(state: State<'_, AppState>, text: String) -> CommandResult<SearchResults> {
    state.library.search(&text).map_err(self::text)
}

// Player

fn start_queue(app: &AppHandle, state: &State<'_, AppState>, queue: Vec<Track>, index: usize, context: String) {
    if queue.is_empty() {
        return;
    }
    let index = index.min(queue.len() - 1);
    let paths = queue.iter().map(|track| PathBuf::from(&track.path)).collect();
    let snapshot = {
        let mut player = lock_player(state);
        player.queue = queue;
        player.index = Some(index);
        player.status = Status::Playing;
        player.position_ms = 0;
        player.context = context;
        player.snapshot()
    };
    state.engine.send(Command::Play { queue: paths, index });
    let _ = app.emit("player", snapshot);
    let _ = app.emit("queue-changed", ());
}

/// Plays files and folders that came from outside the library.
///
/// A folder plays its audio files in natural name order. A single file plays alone.
pub fn open_paths(app: &AppHandle, paths: &[PathBuf]) {
    let state = app.state::<AppState>();
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            let mut inside: Vec<PathBuf> = std::fs::read_dir(path)
                .map(|entries| entries.flatten().map(|e| e.path()).filter(|p| p.is_file() && vyra_library::is_audio(p)).collect())
                .unwrap_or_default();
            inside.sort_by_cached_key(|p| {
                vyra_library::natural_key(&p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
            });
            files.extend(inside);
        } else if vyra_library::is_audio(path) {
            files.push(path.clone());
        }
    }

    let queue: Vec<Track> = files.iter().filter_map(|file| state.library.external_track(file)).collect();
    let context = match paths {
        [single] => single.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        _ => String::new(),
    };
    start_queue(app, &state, queue, 0, context);
}

/// Plays an album from one of its tracks. The album is the playback context.
#[tauri::command]
pub fn play_album(app: AppHandle, state: State<'_, AppState>, id: i64, index: usize) -> CommandResult<()> {
    let Some(detail) = state.library.album(id).map_err(text)? else {
        return Err("album not found".into());
    };
    start_queue(&app, &state, detail.tracks, index, detail.album.title);
    Ok(())
}

/// Plays a list of tracks, for example the visible part of the track list.
#[tauri::command]
pub fn play_tracks(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
    index: usize,
    context: String,
) -> CommandResult<()> {
    let queue = state.library.tracks_by_ids(&ids).map_err(text)?;
    start_queue(&app, &state, queue, index, context);
    Ok(())
}

/// Restarts the queue at a track when the engine has nothing loaded.
fn restart(state: &State<'_, AppState>, player: &mut Player, index: usize) {
    let paths: Vec<PathBuf> = player.queue.iter().map(|track| PathBuf::from(&track.path)).collect();
    if index < paths.len() {
        player.index = Some(index);
        player.status = Status::Playing;
        player.position_ms = 0;
        state.engine.send(Command::Play { queue: paths, index });
    }
}

#[tauri::command]
pub fn toggle_pause(app: AppHandle, state: State<'_, AppState>) {
    let mut player = lock_player(&state);
    match player.status {
        Status::Playing => state.engine.send(Command::Pause),
        Status::Paused => state.engine.send(Command::Resume),
        Status::Stopped => {
            let index = player.index.unwrap_or(0);
            restart(&state, &mut player, index);
            let _ = app.emit("player", player.snapshot());
        }
    }
}

fn go_to(app: &AppHandle, state: &State<'_, AppState>, index: usize) {
    let mut player = lock_player(state);
    if index >= player.queue.len() {
        return;
    }
    if player.status == Status::Stopped {
        restart(state, &mut player, index);
    } else {
        player.index = Some(index);
        player.position_ms = 0;
        state.engine.send(Command::Jump { index });
    }
    let _ = app.emit("player", player.snapshot());
}

#[tauri::command]
pub fn next_track(app: AppHandle, state: State<'_, AppState>) {
    let next = lock_player(&state).index.map_or(0, |index| index + 1);
    go_to(&app, &state, next);
}

/// Goes to the previous track, or to the start of this one when it has played for a while.
#[tauri::command]
pub fn previous_track(app: AppHandle, state: State<'_, AppState>) {
    let (index, position_ms, playing) = {
        let player = lock_player(&state);
        (player.index.unwrap_or(0), player.position_ms, player.status != Status::Stopped)
    };
    if playing && (position_ms > 3000 || index == 0) {
        state.engine.send(Command::Seek { seconds: 0.0 });
        lock_player(&state).position_ms = 0;
    } else {
        go_to(&app, &state, index.saturating_sub(1));
    }
}

#[tauri::command]
pub fn jump_to(app: AppHandle, state: State<'_, AppState>, index: usize) {
    go_to(&app, &state, index);
}

#[tauri::command]
pub fn seek(state: State<'_, AppState>, seconds: f64) {
    let mut player = lock_player(&state);
    if player.status != Status::Stopped {
        player.position_ms = (seconds.max(0.0) * 1000.0) as u64;
        state.engine.send(Command::Seek { seconds });
    }
}

#[tauri::command]
pub fn set_volume(state: State<'_, AppState>, volume: f32) {
    let volume = volume.clamp(0.0, 1.0);
    lock_player(&state).volume = volume;
    state.engine.send(Command::SetVolume(volume));
    let mut settings = state.settings.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    settings.volume = volume;
    settings.save(&state.data_dir);
}

#[tauri::command]
pub fn player_snapshot(state: State<'_, AppState>) -> Snapshot {
    lock_player(&state).snapshot()
}

#[tauri::command]
pub fn queue(state: State<'_, AppState>) -> Vec<Track> {
    lock_player(&state).queue.clone()
}

// Output

#[derive(Serialize)]
pub struct OutputDevice {
    id: String,
    name: String,
    is_default: bool,
}

#[derive(Serialize)]
pub struct OutputState {
    devices: Vec<OutputDevice>,
    /// Chosen device, `None` when playback follows the system default.
    device_id: Option<String>,
    exclusive: bool,
}

#[tauri::command]
pub fn output_devices(state: State<'_, AppState>) -> CommandResult<OutputState> {
    let devices = wasapi::devices()
        .map_err(text)?
        .into_iter()
        .map(|device| OutputDevice { id: device.id, name: device.name, is_default: device.is_default })
        .collect();
    let settings = state.settings.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(OutputState { devices, device_id: settings.device_id.clone(), exclusive: settings.exclusive })
}

#[tauri::command]
pub fn set_output(app: AppHandle, state: State<'_, AppState>, device_id: Option<String>, exclusive: bool) {
    {
        let mut settings = state.settings.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        settings.device_id = device_id.clone();
        settings.exclusive = exclusive;
        settings.save(&state.data_dir);
    }
    let snapshot = {
        let mut player = lock_player(&state);
        player.exclusive = exclusive;
        player.snapshot()
    };
    let mode = if exclusive { OutputMode::Exclusive } else { OutputMode::Shared };
    state.engine.send(Command::SetOutput { device_id, mode });
    let _ = app.emit("player", snapshot);
}
