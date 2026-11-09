//! Installing the KWin script and talking to KWin itself.
//!
//! The script is part of the binary and is written to
//! `~/.local/share/kwin/scripts/tessera` whenever it differs from what is
//! there. KWin's QML engine caches compiled files and folder listings for the
//! whole session: an overwritten file keeps running the old code, a new file
//! name in a known folder is not found. So each version lives in a folder of
//! its own, named after a hash of its content.

use std::path::PathBuf;
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::model::Conflict;

pub const PLUGIN_ID: &str = "tessera";
const SOURCE: &str = include_str!("../kwin/main.qml");

/// Scripts that also move windows when they open or manage zones.
const CONFLICTS: &[(&str, &str, &str)] = &[
    (
        "rememberwindowpositions",
        "Remember Window Positions",
        "also restores window positions",
    ),
    ("kzones", "KZones", "also manages zones"),
    ("krohnkite", "Krohnkite", "tiles windows automatically"),
    ("polonium", "Polonium", "tiles windows automatically"),
    ("bismuth", "Bismuth", "tiles windows automatically"),
];

fn scripts_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".local/share")
        })
        .join("kwin/scripts")
}

pub fn script_dir() -> PathBuf {
    scripts_dir().join(PLUGIN_ID)
}

fn source() -> String {
    SOURCE.replace("@VERSION@", env!("CARGO_PKG_VERSION"))
}

fn main_file_name(source: &str) -> String {
    let hash = Sha256::digest(source.as_bytes());
    let short: String = hash.iter().take(5).map(|b| format!("{b:02x}")).collect();
    format!("v{short}/main.qml")
}

fn metadata(main: &str) -> String {
    format!(
        r#"{{
    "KPlugin": {{
        "Id": "{PLUGIN_ID}",
        "Name": "Tessera",
        "Description": "Window zones and remembered window positions. Managed by the Tessera app.",
        "Icon": "tessera",
        "Version": "{version}",
        "License": "MIT",
        "Authors": [{{ "Name": "Tessera" }}]
    }},
    "X-Plasma-API": "declarativescript",
    "X-Plasma-MainScript": "ui/{main}",
    "KPackageStructure": "KWin/Script"
}}
"#,
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// Write the script if it is missing or outdated. Returns whether anything
/// changed, in which case KWin needs to load it again.
pub fn install() -> Result<bool, String> {
    let dir = script_dir();
    let source = source();
    let main = main_file_name(&source);
    let ui = dir.join("contents/ui");
    if ui.join(&main).exists() && dir.join("metadata.json").exists() {
        return Ok(false);
    }
    // Older versions go; the one KWin is running stays loaded from memory.
    if let Ok(entries) = std::fs::read_dir(&ui) {
        for entry in entries.flatten() {
            let path = entry.path();
            let _ = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
        }
    }
    let file = ui.join(&main);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&file, source).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("metadata.json"), metadata(&main)).map_err(|e| e.to_string())?;
    Ok(true)
}

pub fn is_installed() -> bool {
    script_dir().join("metadata.json").exists()
}

fn read_plugin(id: &str) -> Option<bool> {
    let out = Command::new("kreadconfig6")
        .args(["--file", "kwinrc", "--group", "Plugins", "--key"])
        .arg(format!("{id}Enabled"))
        .output()
        .ok()?;
    match String::from_utf8_lossy(&out.stdout).trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn write_plugin(id: &str, enabled: bool) -> Result<(), String> {
    let status = Command::new("kwriteconfig6")
        .args(["--file", "kwinrc", "--group", "Plugins", "--key"])
        .arg(format!("{id}Enabled"))
        .arg("--type")
        .arg("bool")
        .arg(if enabled { "true" } else { "false" })
        .status()
        .map_err(|e| format!("kwriteconfig6: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("kwriteconfig6 failed".into())
    }
}

pub fn is_enabled() -> bool {
    read_plugin(PLUGIN_ID) == Some(true)
}

fn kwin_call(
    path: &str,
    interface: &str,
    method: &str,
    arg: Option<&str>,
) -> Result<zbus::Message, String> {
    let connection = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let result = match arg {
        Some(arg) => {
            connection.call_method(Some("org.kde.KWin"), path, Some(interface), method, &(arg,))
        }
        None => connection.call_method(Some("org.kde.KWin"), path, Some(interface), method, &()),
    };
    result.map_err(|e| e.to_string())
}

pub fn is_loaded() -> bool {
    kwin_call(
        "/Scripting",
        "org.kde.kwin.Scripting",
        "isScriptLoaded",
        Some(PLUGIN_ID),
    )
    .ok()
    .and_then(|m| m.body().deserialize::<bool>().ok())
    .unwrap_or(false)
}

/// Enable the script for future logins and run the installed version now.
///
/// KWin's own loading of enabled scripts (`reconfigure`) works from metadata
/// it read when the session started, so a script installed or updated later
/// is loaded directly instead.
pub fn enable(reload: bool) -> Result<(), String> {
    if !is_enabled() {
        write_plugin(PLUGIN_ID, true)?;
    }
    if is_loaded() {
        if !reload {
            return Ok(());
        }
        let _ = kwin_call(
            "/Scripting",
            "org.kde.kwin.Scripting",
            "unloadScript",
            Some(PLUGIN_ID),
        );
    }
    let main = script_dir()
        .join("contents/ui")
        .join(main_file_name(&source()));
    let connection = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let id: i32 = connection
        .call_method(
            Some("org.kde.KWin"),
            "/Scripting",
            Some("org.kde.kwin.Scripting"),
            "loadDeclarativeScript",
            &(main.to_string_lossy().as_ref(), PLUGIN_ID),
        )
        .map_err(|e| e.to_string())?
        .body()
        .deserialize()
        .map_err(|e| e.to_string())?;
    if id < 0 {
        return Err("KWin refused to load the script".into());
    }
    connection
        .call_method(
            Some("org.kde.KWin"),
            &*format!("/Scripting/Script{id}"),
            Some("org.kde.kwin.Script"),
            "run",
            &(),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Install, enable and load. Called when the service starts.
pub fn ensure() -> Result<(), String> {
    let changed = install()?;
    enable(changed)
}

/// Turn the script off and remove it. Used by `tessera --uninstall-script`.
pub fn remove() -> Result<(), String> {
    let _ = Command::new("kwriteconfig6")
        .args(["--file", "kwinrc", "--group", "Plugins", "--key"])
        .arg(format!("{PLUGIN_ID}Enabled"))
        .arg("--delete")
        .status();
    let _ = kwin_call(
        "/Scripting",
        "org.kde.kwin.Scripting",
        "unloadScript",
        Some(PLUGIN_ID),
    );
    let _ = std::fs::remove_dir_all(script_dir());
    kwin_call("/KWin", "org.kde.KWin", "reconfigure", None).map(|_| ())
}

pub fn conflicts() -> Vec<Conflict> {
    CONFLICTS
        .iter()
        .filter(|(id, _, _)| read_plugin(id) == Some(true))
        .map(|(id, name, reason)| Conflict {
            plugin: id.to_string(),
            name: name.to_string(),
            reason: reason.to_string(),
        })
        .collect()
}

pub fn disable_plugin(id: &str) -> Result<(), String> {
    if !CONFLICTS.iter().any(|(known, _, _)| *known == id) {
        return Err(format!("{id} is not a known conflicting script"));
    }
    write_plugin(id, false)?;
    let _ = kwin_call(
        "/Scripting",
        "org.kde.kwin.Scripting",
        "unloadScript",
        Some(id),
    );
    kwin_call("/KWin", "org.kde.KWin", "reconfigure", None).map(|_| ())
}

/// Plasma's on-screen message, as used for volume changes.
pub fn osd(icon: &str, text: &str) {
    if let Ok(connection) = zbus::blocking::Connection::session() {
        let _ = connection.call_method(
            Some("org.kde.plasmashell"),
            "/org/kde/osdService",
            Some("org.kde.osdService"),
            "showText",
            &(icon, text),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_has_its_version_filled_in() {
        let source = source();
        assert!(!source.contains("@VERSION@"));
        assert!(source.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn main_file_changes_with_the_content() {
        assert_ne!(main_file_name("a"), main_file_name("b"));
        assert!(main_file_name("a").ends_with("/main.qml"));
        assert!(metadata("vx/main.qml").contains("\"ui/vx/main.qml\""));
    }
}
