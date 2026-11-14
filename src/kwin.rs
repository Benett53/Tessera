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

/// One KWin script package that Tessera writes and loads.
struct Script {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    /// `declarativescript` or `javascript`.
    api: &'static str,
    /// Folder below `contents/` that holds the code.
    folder: &'static str,
    file: &'static str,
    /// The Scripting method that loads this kind of script.
    loader: &'static str,
    source: &'static str,
}

/// Reports windows, places them, builds tiles, registers shortcuts.
const MAIN: Script = Script {
    id: "tessera",
    name: "Tessera",
    description: "Window zones and remembered window positions. Managed by the Tessera app.",
    api: "declarativescript",
    folder: "ui",
    file: "main.qml",
    loader: "loadDeclarativeScript",
    source: include_str!("../kwin/main.qml"),
};

/// Entries in the window menu, which KWin offers only to JavaScript scripts.
const MENU: Script = Script {
    id: "tessera-menu",
    name: "Tessera window menu",
    description: "Tessera's entries in the window menu. Managed by the Tessera app.",
    api: "javascript",
    folder: "code",
    file: "main.js",
    loader: "loadScript",
    source: include_str!("../kwin/menu.js"),
};

const SCRIPTS: [&Script; 2] = [&MAIN, &MENU];

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

fn script_dir(script: &Script) -> PathBuf {
    scripts_dir().join(script.id)
}

fn source(script: &Script) -> String {
    script
        .source
        .replace("@VERSION@", env!("CARGO_PKG_VERSION"))
}

fn main_file_name(script: &Script, source: &str) -> String {
    let hash = Sha256::digest(source.as_bytes());
    let short: String = hash.iter().take(5).map(|b| format!("{b:02x}")).collect();
    format!("v{short}/{}", script.file)
}

fn metadata(script: &Script, main: &str) -> String {
    format!(
        r#"{{
    "KPlugin": {{
        "Id": "{id}",
        "Name": "{name}",
        "Description": "{description}",
        "Icon": "tessera",
        "Version": "{version}",
        "License": "MIT",
        "Authors": [{{ "Name": "Tessera" }}]
    }},
    "X-Plasma-API": "{api}",
    "X-Plasma-MainScript": "{folder}/{main}",
    "KPackageStructure": "KWin/Script"
}}
"#,
        id = script.id,
        name = script.name,
        description = script.description,
        api = script.api,
        folder = script.folder,
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// Write one script if it is missing or outdated. Returns whether anything
/// changed, in which case KWin needs to load it again.
fn install_script(script: &Script) -> Result<bool, String> {
    let dir = script_dir(script);
    let source = source(script);
    let main = main_file_name(script, &source);
    let code = dir.join("contents").join(script.folder);
    if code.join(&main).exists() && dir.join("metadata.json").exists() {
        return Ok(false);
    }
    // Older versions go; the one KWin is running stays loaded from memory.
    if let Ok(entries) = std::fs::read_dir(&code) {
        for entry in entries.flatten() {
            let path = entry.path();
            let _ = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
        }
    }
    let file = code.join(&main);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&file, source).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("metadata.json"), metadata(script, &main))
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// Write all scripts that are missing or outdated. Returns whether anything
/// changed.
pub fn install() -> Result<bool, String> {
    let mut changed = false;
    for script in SCRIPTS {
        changed |= install_script(script)?;
    }
    Ok(changed)
}

pub fn is_installed() -> bool {
    script_dir(&MAIN).join("metadata.json").exists()
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
    read_plugin(MAIN.id) == Some(true)
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

fn is_loaded(id: &str) -> bool {
    kwin_call(
        "/Scripting",
        "org.kde.kwin.Scripting",
        "isScriptLoaded",
        Some(id),
    )
    .ok()
    .and_then(|m| m.body().deserialize::<bool>().ok())
    .unwrap_or(false)
}

/// Enable all scripts for future logins and run the installed versions now.
pub fn enable(reload: bool) -> Result<(), String> {
    for script in SCRIPTS {
        enable_script(script, reload)?;
    }
    Ok(())
}

/// Enable one script for future logins and run the installed version now.
///
/// KWin's own loading of enabled scripts (`reconfigure`) works from metadata
/// it read when the session started, so a script installed or updated later
/// is loaded directly instead.
fn enable_script(script: &Script, reload: bool) -> Result<(), String> {
    if read_plugin(script.id) != Some(true) {
        write_plugin(script.id, true)?;
    }
    if is_loaded(script.id) {
        if !reload {
            return Ok(());
        }
        let _ = kwin_call(
            "/Scripting",
            "org.kde.kwin.Scripting",
            "unloadScript",
            Some(script.id),
        );
    }
    let main = script_dir(script)
        .join("contents")
        .join(script.folder)
        .join(main_file_name(script, &source(script)));
    let connection = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let id: i32 = connection
        .call_method(
            Some("org.kde.KWin"),
            "/Scripting",
            Some("org.kde.kwin.Scripting"),
            script.loader,
            &(main.to_string_lossy().as_ref(), script.id),
        )
        .map_err(|e| e.to_string())?
        .body()
        .deserialize()
        .map_err(|e| e.to_string())?;
    if id < 0 {
        return Err("KWin refused to load the script".into());
    }
    // `start` runs every loaded script that is not running yet. Calling
    // `run` on `/Scripting/Script<id>` is not reliable: KWin numbers scripts
    // by how many are loaded, so after an unload the id can belong to an
    // older script and the call reaches that one instead.
    connection
        .call_method(
            Some("org.kde.KWin"),
            "/Scripting",
            Some("org.kde.kwin.Scripting"),
            "start",
            &(),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Install, enable and load. Called when the service starts. The window
/// menu is tried even when the main script fails, and the other way round.
pub fn ensure() -> Result<(), String> {
    let mut errors = Vec::new();
    for script in SCRIPTS {
        if let Err(e) = install_script(script).and_then(|changed| enable_script(script, changed)) {
            errors.push(format!("{}: {e}", script.id));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Turn the scripts off and remove them. Used by `tessera --remove-kwin-script`.
pub fn remove() -> Result<(), String> {
    for script in SCRIPTS {
        let _ = Command::new("kwriteconfig6")
            .args(["--file", "kwinrc", "--group", "Plugins", "--key"])
            .arg(format!("{}Enabled", script.id))
            .arg("--delete")
            .status();
        let _ = kwin_call(
            "/Scripting",
            "org.kde.kwin.Scripting",
            "unloadScript",
            Some(script.id),
        );
        let _ = std::fs::remove_dir_all(script_dir(script));
    }
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
        let source = source(&MAIN);
        assert!(!source.contains("@VERSION@"));
        assert!(source.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn main_file_changes_with_the_content() {
        assert_ne!(main_file_name(&MAIN, "a"), main_file_name(&MAIN, "b"));
        assert!(main_file_name(&MAIN, "a").ends_with("/main.qml"));
        assert!(metadata(&MAIN, "vx/main.qml").contains("\"ui/vx/main.qml\""));
    }

    #[test]
    fn the_window_menu_is_a_javascript_package() {
        let main = main_file_name(&MENU, &source(&MENU));
        assert!(main.ends_with("/main.js"));
        let meta = metadata(&MENU, &main);
        assert!(meta.contains("\"X-Plasma-API\": \"javascript\""));
        assert!(meta.contains(&format!("\"code/{main}\"")));
        assert!(meta.contains("\"Id\": \"tessera-menu\""));
        assert!(source(&MENU).contains("registerUserActionsMenu"));
    }

    #[test]
    fn every_script_has_its_own_folder() {
        assert_ne!(script_dir(&MAIN), script_dir(&MENU));
    }
}
