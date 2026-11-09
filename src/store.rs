//! Files on disk.
//!
//! Settings and layouts live in `~/.config/tessera/config.json`, remembered
//! windows and arrangements in `~/.local/share/tessera/`. `TESSERA_HOME` puts
//! everything into one folder instead, for a portable or test copy.
//!
//! Only the service writes these files. Writes go to a temporary file that is
//! renamed over the old one, so a crash never leaves half a file behind. A file
//! that cannot be read is moved aside as `*.broken` instead of being lost.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
}

fn portable() -> Option<PathBuf> {
    std::env::var_os("TESSERA_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

pub fn config_dir() -> PathBuf {
    portable().unwrap_or_else(|| xdg("XDG_CONFIG_HOME", ".config").join("tessera"))
}

pub fn data_dir() -> PathBuf {
    portable().unwrap_or_else(|| xdg("XDG_DATA_HOME", ".local/share").join("tessera"))
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn windows_path() -> PathBuf {
    data_dir().join("windows.json")
}

pub fn snapshots_path() -> PathBuf {
    data_dir().join("snapshots.json")
}

/// Read `path`, or the default when it is missing or broken.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> T {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return T::default(),
    };
    match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => {
            eprintln!(
                "tessera: {} is unreadable ({error}), starting fresh",
                path.display()
            );
            let _ = std::fs::rename(path, path.with_extension("json.broken"));
            T::default()
        }
    }
}

pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Config;

    #[test]
    fn round_trip_and_broken_files() {
        let dir = std::env::temp_dir().join(format!("tessera-store-{}", std::process::id()));
        let path = dir.join("config.json");
        let mut config = Config::default();
        config.settings.remember = false;
        save(&path, &config).unwrap();
        assert_eq!(load::<Config>(&path), config);

        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load::<Config>(&path), Config::default());
        assert!(path.with_extension("json.broken").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
