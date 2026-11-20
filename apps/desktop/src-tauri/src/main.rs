//! VYRA desktop application: the window, and the glue between it and the core crates.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod player;
mod settings;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tauri::Manager;
use tauri::http::{Response, StatusCode};
use vyra_audio::OutputMode;
use vyra_audio::engine::{Command, Engine};
use vyra_library::Library;

use crate::player::Player;
use crate::settings::Settings;

/// Everything the commands share.
pub struct AppState {
    pub library: Arc<Library>,
    pub engine: Engine,
    pub player: Mutex<Player>,
    pub scanning: AtomicBool,
    pub settings: Mutex<Settings>,
    pub data_dir: PathBuf,
}

fn data_dir() -> PathBuf {
    // Tests and portable setups point the application at another folder.
    if let Some(custom) = std::env::var_os("VYRA_DATA_DIR") {
        return PathBuf::from(custom);
    }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("VYRA")
}

fn main() {
    tauri::Builder::default()
        .register_uri_scheme_protocol("vyra-art", |context, request| {
            artwork_response(context.app_handle(), request.uri().path())
        })
        .setup(|app| {
            let data_dir = data_dir();
            std::fs::create_dir_all(&data_dir)?;
            let library = Arc::new(Library::open(&data_dir.join("library"))?);
            let settings = Settings::load(&data_dir);

            let handle = app.handle().clone();
            let engine = Engine::start(move |event| player::on_engine_event(&handle, event));
            engine.send(Command::SetVolume(settings.volume));
            engine.send(Command::SetOutput {
                device_id: settings.device_id.clone(),
                mode: if settings.exclusive { OutputMode::Exclusive } else { OutputMode::Shared },
            });

            let player = Player { volume: settings.volume, exclusive: settings.exclusive, ..Player::default() };

            app.manage(AppState {
                library,
                engine,
                player: Mutex::new(player),
                scanning: AtomicBool::new(false),
                settings: Mutex::new(settings),
                data_dir,
            });

            // New files are picked up on every start.
            commands::start_scan(app.handle());

            // Files and folders given on the command line play at once. This is how
            // "Open with" from Explorer reaches the application.
            let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).filter(|p| p.exists()).collect();
            if !paths.is_empty() {
                commands::open_paths(app.handle(), &paths);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::library_status,
            commands::add_music_folder,
            commands::remove_music_folder,
            commands::rescan,
            commands::albums,
            commands::recent_albums,
            commands::album,
            commands::artists,
            commands::artist_albums,
            commands::tracks,
            commands::search,
            commands::play_album,
            commands::play_tracks,
            commands::toggle_pause,
            commands::next_track,
            commands::previous_track,
            commands::jump_to,
            commands::seek,
            commands::set_volume,
            commands::player_snapshot,
            commands::queue,
            commands::output_devices,
            commands::set_output,
        ])
        .run(tauri::generate_context!())
        .expect("the application could not start");
}

/// Serves cached covers to the window: `vyra-art://localhost/<hash>/<size>`.
fn artwork_response(app: &tauri::AppHandle, path: &str) -> Response<Cow<'static, [u8]>> {
    let not_found = || Response::builder().status(StatusCode::NOT_FOUND).body(Cow::Borrowed(&[][..])).unwrap();

    let mut parts = path.trim_matches('/').split('/');
    let (Some(hash), Some(size)) = (parts.next(), parts.next()) else {
        return not_found();
    };
    // The hash goes into a file name, so nothing but hex digits is accepted.
    if hash.is_empty() || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return not_found();
    }
    let Some(state) = app.try_state::<AppState>() else {
        return not_found();
    };
    let file = state.library.artwork_file(hash, size.parse().unwrap_or(400));
    match std::fs::read(file) {
        Ok(bytes) => Response::builder()
            .header("Content-Type", "image/jpeg")
            .header("Cache-Control", "public, max-age=31536000, immutable")
            .header("Access-Control-Allow-Origin", "*")
            .body(Cow::Owned(bytes))
            .unwrap(),
        Err(_) => not_found(),
    }
}
