//! Settings kept between runs.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub volume: f32,
    /// Output device identifier, `None` for the system default.
    pub device_id: Option<String>,
    pub exclusive: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { volume: 1.0, device_id: None, exclusive: false }
    }
}

fn file(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

impl Settings {
    /// Reads the settings. A missing or broken file gives the defaults.
    pub fn load(data_dir: &Path) -> Self {
        std::fs::read_to_string(file(data_dir))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) {
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let target = file(data_dir);
            let temporary = target.with_extension("tmp");
            if std::fs::write(&temporary, text).is_ok() {
                let _ = std::fs::rename(temporary, target);
            }
        }
    }
}
