//! What the program remembers between starts. The token is not in here.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub server: String,
    pub sound: bool,
    pub notifications: bool,
    /// Closing the contact list hides it instead of ending the program.
    pub stay_in_tray: bool,
    pub scheme: Scheme,
    /// Drawing with the graphics card. Off, the program draws by itself and
    /// spares the threads and the memory of a graphics driver. Read at the start only.
    pub hardware_acceleration: bool,
}

/// Light or dark: as the system says, or as the member chose.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    #[default]
    System,
    Light,
    Dark,
}

impl Scheme {
    /// The number the windows know it by (`Theme.choose`, ui/theme.slint).
    pub fn index(self) -> i32 {
        self as i32
    }

    pub fn from_index(index: i32) -> Self {
        match index {
            1 => Scheme::Light,
            2 => Scheme::Dark,
            _ => Scheme::System,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server: String::new(),
            sound: true,
            notifications: true,
            stay_in_tray: false,
            scheme: Scheme::System,
            hardware_acceleration: false,
        }
    }
}

fn path() -> Option<PathBuf> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    return dirs::config_dir().map(|dir| dir.join("gradido-chat").join("settings.json"));
    // Android: the activity's internal data path; iOS: the app's Documents directory.
    #[cfg(any(target_os = "android", target_os = "ios"))]
    return None;
}

impl Settings {
    pub fn load() -> Self {
        path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}
