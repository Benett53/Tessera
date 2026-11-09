//! Types shared by the service, the window and the KWin script.
//!
//! Everything that crosses D-Bus is JSON. The script side reads and writes the
//! same field names, so renaming a field here means changing `kwin/main.qml`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::layout::ZoneNode;

/// User settings, stored in `config.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Master switch for restoring windows when they open.
    pub remember: bool,
    /// Programs not seen before are remembered without asking.
    pub remember_new: bool,
    pub restore_geometry: bool,
    pub restore_screen: bool,
    pub restore_desktop: bool,
    pub restore_maximized: bool,
    pub restore_minimized: bool,
    pub restore_above: bool,
    /// Put a window back into the zone it was in.
    pub restore_zone: bool,
    /// A maximised window returns to its zone when it is restored.
    pub zone_after_unmaximize: bool,
    /// Programs that are never moved. `*` matches any run of characters.
    pub ignore: Vec<String>,
    /// Start programs from an arrangement that are not running.
    pub launch_missing: bool,
    pub tray: bool,
    pub oled_black: bool,
    pub accent: String,
    pub auto_update: bool,
    pub last_update_check: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            remember: true,
            remember_new: true,
            restore_geometry: true,
            restore_screen: true,
            restore_desktop: true,
            restore_maximized: true,
            restore_minimized: false,
            restore_above: true,
            restore_zone: true,
            zone_after_unmaximize: true,
            ignore: default_ignore(),
            launch_missing: false,
            tray: true,
            oled_black: true,
            accent: "#7AA2F7".into(),
            auto_update: true,
            last_update_check: 0,
        }
    }
}

/// Shells, launchers and helpers that place their own windows.
pub fn default_ignore() -> Vec<String> {
    [
        "plasmashell",
        "org.kde.plasmashell",
        "krunner",
        "org.kde.krunner",
        "org.kde.spectacle",
        "spectacle",
        "org.kde.polkit-kde-authentication-agent-1",
        "polkit-kde-authentication-agent-1",
        "xwaylandvideobridge",
        "ksmserver-logout-greeter",
        "org.kde.ksmserver-logout-greeter",
        "steam_app_*",
        "gamescope",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// `*` wildcard match, case-insensitive.
pub fn pattern_matches(pattern: &str, text: &str) -> bool {
    fn inner(p: &[u8], t: &[u8]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some((b'*', rest)) => (0..=t.len()).any(|i| inner(rest, &t[i..])),
            Some((c, rest)) => t
                .split_first()
                .is_some_and(|(d, t_rest)| c.eq_ignore_ascii_case(d) && inner(rest, t_rest)),
        }
    }
    inner(pattern.trim().as_bytes(), text.as_bytes())
}

/// A custom layout in the library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedLayout {
    pub id: String,
    pub name: String,
    pub tree: ZoneNode,
}

/// `config.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub settings: Settings,
    pub layouts: Vec<SavedLayout>,
}

/// A KWin tile, identified by where it sits rather than by an object that
/// stops existing when the layout is rebuilt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TileRef {
    pub output: String,
    #[serde(default)]
    pub desktop: String,
    /// Relative to the screen: x, y, width, height in 0..1.
    pub rect: [f64; 4],
}

/// Where and how a window sits.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Placement {
    pub output: String,
    /// Frame geometry while not maximised, in global coordinates.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub desktops: Vec<String>,
    pub all_desktops: bool,
    /// KWin maximize mode: 0 none, 1 vertical, 2 horizontal, 3 full.
    pub max: u8,
    pub full: bool,
    pub min: bool,
    pub above: bool,
    pub tile: Option<TileRef>,
    /// The user moved the window by hand; `x`/`y` are theirs. Without it the
    /// program and KWin decide where it opens.
    pub user_pos: bool,
    /// The user resized the window by hand; `w`/`h` are theirs. Without it
    /// the program keeps its own size.
    pub user_size: bool,
}

/// What the script reports about a window.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowInfo {
    pub id: String,
    pub class: String,
    pub desktop_file: String,
    pub caption: String,
    pub pid: i64,
    #[serde(flatten)]
    pub placement: Placement,
}

/// What a remembered window slot restores, with switches for each part.
/// Sent to the script when a window opens.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Restore {
    pub placement: Placement,
    pub geometry: bool,
    pub screen: bool,
    pub desktop: bool,
    pub maximized: bool,
    pub minimized: bool,
    pub above: bool,
    pub zone: bool,
    /// Set size and position even where the user did not (arrangements,
    /// which save everything on purpose).
    pub force_geometry: bool,
    /// Pinned zone: put the window into zone `index` (1-based) on `output`.
    pub pin: Option<ZonePin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZonePin {
    pub output: String,
    pub index: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppMode {
    /// Remember where its windows were and put them back.
    #[default]
    Remember,
    /// Always open in a fixed zone.
    Zone,
    /// Leave alone.
    Ignore,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Slot {
    /// Caption with digits removed, used to tell windows of one program apart.
    pub caption_key: String,
    pub placement: Placement,
    pub updated: u64,
}

/// One program in `windows.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppRecord {
    pub desktop_file: String,
    pub mode: AppMode,
    pub pin: Option<ZonePin>,
    pub slots: Vec<Slot>,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowsFile {
    pub version: u32,
    /// Keyed by window class (Wayland app id or X11 WM_CLASS), lower case.
    pub apps: BTreeMap<String, AppRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapshotWindow {
    pub class: String,
    pub desktop_file: String,
    pub caption: String,
    pub placement: Placement,
}

/// A saved arrangement of all windows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub id: String,
    pub name: String,
    pub created: u64,
    pub screens: Vec<Screen>,
    pub windows: Vec<SnapshotWindow>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapshotsFile {
    pub version: u32,
    pub items: Vec<Snapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Screen {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub manufacturer: String,
    pub model: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Desktop {
    pub id: String,
    pub name: String,
}

/// Screens and virtual desktops, as KWin reports them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Topology {
    pub screens: Vec<Screen>,
    pub desktops: Vec<Desktop>,
    pub current_desktop: String,
}

/// The tile layout KWin currently has for one screen and desktop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneState {
    pub output: String,
    pub desktop: String,
    pub tree: ZoneNode,
    #[serde(default)]
    pub padding: u32,
}

/// The KWin script's connection, as far as the service can tell.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptStatus {
    pub installed: bool,
    pub enabled: bool,
    pub connected: bool,
    pub version: String,
}

/// Everything the window shows, in one answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StateView {
    pub revision: u64,
    pub version: String,
    pub config: Config,
    pub apps: BTreeMap<String, AppRecord>,
    /// Classes with a window open right now.
    pub running: Vec<String>,
    pub snapshots: Vec<Snapshot>,
    pub topology: Topology,
    pub zones: Vec<ZoneState>,
    pub script: ScriptStatus,
    /// Other KWin scripts that do the same job and would fight Tessera.
    pub conflicts: Vec<Conflict>,
    pub paused: bool,
    pub data_dir: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Conflict {
    pub plugin: String,
    pub name: String,
    pub reason: String,
}

/// Requests from the window and the command line to the service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    State,
    SetSettings {
        settings: Settings,
    },
    SetApp {
        class: String,
        mode: AppMode,
        pin: Option<ZonePin>,
    },
    ForgetApp {
        class: String,
    },
    ForgetSlot {
        class: String,
        index: usize,
    },
    ForgetAll,
    ApplyZones {
        output: String,
        /// Empty: every desktop.
        desktop: String,
        tree: ZoneNode,
        padding: u32,
    },
    RefreshZones,
    SaveLayout {
        name: String,
        tree: ZoneNode,
    },
    DeleteLayout {
        id: String,
    },
    SaveSnapshot {
        name: String,
        /// Overwrite this arrangement instead of adding one.
        replace: Option<String>,
    },
    RestoreSnapshot {
        id: String,
    },
    RenameSnapshot {
        id: String,
        name: String,
    },
    DeleteSnapshot {
        id: String,
    },
    SetPaused {
        paused: bool,
    },
    DisableConflict {
        plugin: String,
    },
    RepairScript,
    ShowWindow,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Error { message: String },
    State { state: Box<StateView> },
    Saved { id: String, count: usize },
    Restored { placed: usize, missing: usize },
}

/// Seconds since the Unix epoch.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Lower case, digits removed, whitespace collapsed. Window titles often carry
/// counters ("(3) Inbox") that should not make two windows look different.
pub fn caption_key(caption: &str) -> String {
    let cleaned: String = caption
        .chars()
        .filter(|c| !c.is_ascii_digit())
        .flat_map(char::to_lowercase)
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_patterns() {
        assert!(pattern_matches("steam_app_*", "steam_app_1234"));
        assert!(pattern_matches("krunner", "KRunner"));
        assert!(!pattern_matches("krunner", "krunner2"));
        assert!(pattern_matches("*fox", "firefox"));
        assert!(pattern_matches("*", ""));
        assert!(!pattern_matches("fire*x", "firefly"));
    }

    #[test]
    fn caption_keys_ignore_counters() {
        assert_eq!(
            caption_key("(3) Inbox  - Mail"),
            caption_key("(12) Inbox - Mail")
        );
        assert_ne!(caption_key("Inbox"), caption_key("Drafts"));
    }

    #[test]
    fn settings_tolerate_missing_fields() {
        let s: Settings = serde_json::from_str(r#"{"remember": false}"#).unwrap();
        assert!(!s.remember);
        assert!(s.restore_geometry);
    }

    #[test]
    fn requests_round_trip() {
        let r = Request::SetPaused { paused: true };
        let text = serde_json::to_string(&r).unwrap();
        assert_eq!(text, r#"{"op":"set_paused","paused":true}"#);
        assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), r);
    }

    #[test]
    fn window_info_reads_flat_json() {
        let info: WindowInfo = serde_json::from_str(
            r#"{"id":"{a}","class":"firefox","x":10,"y":20,"w":800,"h":600,"output":"DP-1","max":3,
                "tile":{"output":"DP-1","desktop":"d","rect":[0,0,0.5,1]}}"#,
        )
        .unwrap();
        assert_eq!(info.placement.x, 10);
        assert_eq!(info.placement.max, 3);
        assert_eq!(info.placement.tile.unwrap().rect[2], 0.5);
    }
}
