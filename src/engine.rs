//! What to do with a window: the service's decisions, without any D-Bus.
//!
//! A program can have several windows open (browsers, file managers). Each is
//! given a *slot* of that program's record while it is open; the slot keeps
//! its last placement after it closes, and the next window of that program
//! takes the first free slot again. A slot whose title matches is preferred,
//! so a mail client's composer and main window do not swap places.

use std::collections::HashMap;

use crate::model::*;

/// Most windows remembered per program.
pub const MAX_SLOTS: usize = 6;
/// How long windows from an arrangement are waited for after a restore.
pub const PENDING_SECS: u64 = 120;

#[derive(Debug, Clone)]
pub struct Live {
    pub class: String,
    pub caption: String,
    pub slot: Option<usize>,
    /// Event counter when the window appeared.
    pub since: u64,
}

#[derive(Debug, Clone)]
pub struct Pending {
    pub class: String,
    pub caption_key: String,
    pub placement: Placement,
    pub until: u64,
}

#[derive(Debug, Default)]
pub struct Engine {
    pub config: Config,
    pub windows: WindowsFile,
    pub snapshots: SnapshotsFile,
    pub live: HashMap<String, Live>,
    pub pending: Vec<Pending>,
    pub topology: Topology,
    pub zones: Vec<ZoneState>,
    pub paused: bool,
    pub revision: u64,
    /// Counts events; orders slot writes against window lifetimes.
    tick: u64,
    /// When each slot was last written, by tick. A window may not take over a
    /// slot another window wrote while it was open: that slot belongs to the
    /// other window, which has just closed.
    written: HashMap<(String, usize), u64>,
    pub dirty_config: bool,
    pub dirty_windows: bool,
    pub dirty_snapshots: bool,
}

/// Window classes are compared in lower case: X11 programs report
/// `Firefox`, the same program under Wayland `firefox`.
pub fn class_key(class: &str) -> String {
    class.trim().to_lowercase()
}

/// The part of a window title a program keeps the same across documents:
/// whatever follows the last separator, or the whole title.
fn title_family(key: &str) -> &str {
    [" — ", " – ", " - ", " | "]
        .iter()
        .filter_map(|sep| key.rfind(sep).map(|i| &key[i + sep.len()..]))
        .min_by_key(|rest| rest.len())
        .unwrap_or(key)
        .trim()
}

/// Whether a window titled `key` may take a slot last used by a window
/// titled `slot`. An unknown title on either side does not count against it.
fn same_kind(key: &str, slot: &str) -> bool {
    key.is_empty() || slot.is_empty() || title_family(key) == title_family(slot)
}

/// What a slot keeps when a window reports in: size and position only change
/// when the user set them by hand. A program resizing itself, a zone or a
/// maximised state never replaces what the user chose.
pub fn merge_placement(old: &Placement, new: &Placement) -> Placement {
    let mut merged = new.clone();
    if !new.user_pos && old.user_pos {
        merged.x = old.x;
        merged.y = old.y;
        merged.output = old.output.clone();
        merged.user_pos = true;
    }
    if !new.user_size && old.user_size {
        merged.w = old.w;
        merged.h = old.h;
        merged.user_size = true;
    }
    merged
}

impl Engine {
    pub fn new(config: Config, windows: WindowsFile, snapshots: SnapshotsFile) -> Self {
        Self {
            config,
            windows,
            snapshots,
            ..Default::default()
        }
    }

    pub fn bump(&mut self) {
        self.revision += 1;
    }

    pub fn settings(&self) -> &Settings {
        &self.config.settings
    }

    pub fn is_ignored(&self, class: &str) -> bool {
        let key = class_key(class);
        key.is_empty()
            || self
                .settings()
                .ignore
                .iter()
                .any(|p| pattern_matches(p, &key))
            || self
                .windows
                .apps
                .get(&key)
                .is_some_and(|app| app.mode == AppMode::Ignore)
    }

    fn claimed(&self, class: &str, except: &str) -> Vec<usize> {
        self.live
            .iter()
            .filter(|(id, live)| id.as_str() != except && live.class == class)
            .filter_map(|(_, live)| live.slot)
            .collect()
    }

    /// The free slot for a window titled `caption`, if there is one.
    fn free_slot(&self, class: &str, caption: &str, id: &str, since: Option<u64>) -> Option<usize> {
        let app = self.windows.apps.get(class)?;
        let taken = self.claimed(class, id);
        let key = caption_key(caption);
        let free: Vec<usize> = (0..app.slots.len())
            .filter(|i| !taken.contains(i))
            .filter(|i| {
                since.is_none_or(|since| {
                    self.written
                        .get(&(class.to_string(), *i))
                        .is_none_or(|&at| at < since)
                })
            })
            .collect();
        // Same title first. Otherwise only a window of the same kind: the
        // part of the title a program keeps the same (" — Mozilla Firefox")
        // must match, so a launcher's splash or a friends list does not take
        // the main window's place.
        free.iter()
            .copied()
            .find(|&i| !key.is_empty() && app.slots[i].caption_key == key)
            .or_else(|| {
                free.iter()
                    .copied()
                    .find(|&i| same_kind(&key, &app.slots[i].caption_key))
            })
            // Titles that change all the time (a player showing the song) match
            // nothing; as the only window of its program it takes the first slot.
            .or_else(|| {
                let alone = !self
                    .live
                    .iter()
                    .any(|(other, live)| other != id && live.class == class);
                alone.then(|| free.first().copied()).flatten()
            })
    }

    fn restore_for(&self, placement: &Placement, everything: bool) -> Restore {
        let s = self.settings();
        Restore {
            placement: placement.clone(),
            geometry: everything || s.restore_geometry,
            screen: everything || s.restore_screen,
            desktop: everything || s.restore_desktop,
            maximized: everything || s.restore_maximized,
            minimized: s.restore_minimized,
            above: everything || s.restore_above,
            zone: everything || s.restore_zone,
            force_geometry: everything,
            pin: None,
        }
    }

    /// The script forgot everything it knew; so does the service.
    pub fn hello(&mut self) {
        self.live.clear();
        self.bump();
    }

    /// A window appeared. Returns what to do with it, if anything.
    pub fn opened(&mut self, info: &WindowInfo) -> Option<Restore> {
        let class = class_key(&info.class);
        let now = now();
        self.pending.retain(|p| p.until > now);
        self.tick += 1;
        self.live.insert(
            info.id.clone(),
            Live {
                class: class.clone(),
                caption: info.caption.clone(),
                slot: None,
                since: self.tick,
            },
        );
        self.bump();
        if self.is_ignored(&class) {
            return None;
        }

        // Windows from an arrangement that was just restored come first.
        let key = caption_key(&info.caption);
        let pending = self
            .pending
            .iter()
            .position(|p| p.class == class && p.caption_key == key)
            .or_else(|| self.pending.iter().position(|p| p.class == class));
        if let Some(index) = pending {
            let entry = self.pending.remove(index);
            self.claim(&info.id, &class, &info.caption);
            return Some(self.restore_for(&entry.placement, true));
        }

        let app = self.windows.apps.get(&class)?;
        match app.mode {
            AppMode::Ignore => None,
            AppMode::Zone => {
                let pin = app.pin.clone()?;
                (!self.paused).then(|| Restore {
                    pin: Some(pin),
                    ..Default::default()
                })
            }
            AppMode::Remember => {
                let slot = self.claim(&info.id, &class, &info.caption)?;
                if self.paused || !self.settings().remember {
                    return None;
                }
                let placement = self
                    .windows
                    .apps
                    .get(&class)?
                    .slots
                    .get(slot)?
                    .placement
                    .clone();
                if placement.w <= 0 || placement.h <= 0 {
                    return None;
                }
                Some(self.restore_for(&placement, false))
            }
        }
    }

    fn claim(&mut self, id: &str, class: &str, caption: &str) -> Option<usize> {
        let slot = self.free_slot(class, caption, id, None)?;
        if let Some(live) = self.live.get_mut(id) {
            live.slot = Some(slot);
        }
        Some(slot)
    }

    /// Like `claim`, for a window that is already open.
    fn claim_late(&mut self, id: &str, class: &str, caption: &str) -> Option<usize> {
        let since = self.live.get(id).map(|l| l.since);
        let slot = self.free_slot(class, caption, id, since)?;
        if let Some(live) = self.live.get_mut(id) {
            live.slot = Some(slot);
        }
        Some(slot)
    }

    /// A window moved, changed state, or was reported on start.
    pub fn changed(&mut self, info: &WindowInfo) {
        let class = class_key(&info.class);
        self.tick += 1;
        let tick = self.tick;
        let live = self.live.entry(info.id.clone()).or_insert_with(|| Live {
            class: class.clone(),
            caption: info.caption.clone(),
            slot: None,
            since: tick,
        });
        live.caption = info.caption.clone();
        self.bump();
        if self.is_ignored(&class) || !self.settings().remember {
            return;
        }

        let remember_new = self.settings().remember_new;
        if !self.windows.apps.contains_key(&class) {
            if !remember_new {
                return;
            }
            self.windows.apps.insert(
                class.clone(),
                AppRecord {
                    desktop_file: info.desktop_file.clone(),
                    ..Default::default()
                },
            );
        }

        let mut slot = self.live.get(&info.id).and_then(|l| l.slot);
        if slot.is_none() {
            slot = self.claim_late(&info.id, &class, &info.caption);
        }
        let taken = self.claimed(&class, &info.id);
        let Some(app) = self.windows.apps.get_mut(&class) else {
            return;
        };
        app.last_seen = now();
        if app.desktop_file.is_empty() {
            app.desktop_file = info.desktop_file.clone();
        }
        if app.mode != AppMode::Remember {
            self.dirty_windows = true;
            return;
        }
        let index = match slot {
            Some(index) if index < app.slots.len() => index,
            _ if app.slots.len() < MAX_SLOTS => {
                app.slots.push(Slot::default());
                app.slots.len() - 1
            }
            // All slots in use by other open windows: the oldest free one
            // would be the only candidate, and there is none.
            _ => match (0..app.slots.len())
                .filter(|i| !taken.contains(i))
                .min_by_key(|&i| app.slots[i].updated)
            {
                Some(index) => index,
                None => return,
            },
        };
        let slot = &mut app.slots[index];
        slot.caption_key = caption_key(&info.caption);
        slot.placement = merge_placement(&slot.placement, &info.placement);
        slot.updated = now();
        self.written.insert((class.clone(), index), self.tick);
        if let Some(live) = self.live.get_mut(&info.id) {
            live.slot = Some(index);
        }
        self.dirty_windows = true;
    }

    pub fn closed(&mut self, info: &WindowInfo) {
        self.changed(info);
        self.live.remove(&info.id);
        self.bump();
    }

    pub fn running(&self) -> Vec<String> {
        let mut classes: Vec<String> = self.live.values().map(|l| l.class.clone()).collect();
        classes.sort();
        classes.dedup();
        classes
    }

    /// Turn the script's list of windows into an arrangement.
    pub fn snapshot_from(&self, id: String, name: String, windows: &[WindowInfo]) -> Snapshot {
        Snapshot {
            id,
            name,
            created: now(),
            screens: self.topology.screens.clone(),
            windows: windows
                .iter()
                .filter(|w| !self.is_ignored(&w.class))
                .map(|w| SnapshotWindow {
                    class: class_key(&w.class),
                    desktop_file: w.desktop_file.clone(),
                    caption: w.caption.clone(),
                    placement: w.placement.clone(),
                })
                .collect(),
        }
    }

    /// Match an arrangement against the open windows. Returns the moves for
    /// open windows and the entries no open window was found for; those are
    /// kept as pending, so they are placed when their program starts.
    pub fn plan_restore(
        &mut self,
        snapshot: &Snapshot,
    ) -> (Vec<(String, Restore)>, Vec<SnapshotWindow>) {
        let mut used: Vec<String> = Vec::new();
        let mut moves = Vec::new();
        let mut missing = Vec::new();
        let mut candidates: Vec<(&String, &Live)> = self.live.iter().collect();
        candidates.sort_by(|a, b| a.0.cmp(b.0));
        for entry in &snapshot.windows {
            let key = caption_key(&entry.caption);
            let open = |exact: bool| {
                candidates
                    .iter()
                    .find(|(id, live)| {
                        live.class == entry.class
                            && !used.contains(id)
                            && (!exact || caption_key(&live.caption) == key)
                    })
                    .map(|(id, _)| (*id).clone())
            };
            match open(true).or_else(|| open(false)) {
                Some(id) => {
                    used.push(id.clone());
                    moves.push((id, self.restore_for(&entry.placement, true)));
                }
                None => missing.push(entry.clone()),
            }
        }
        let until = now() + PENDING_SECS;
        self.pending = missing
            .iter()
            .map(|entry| Pending {
                class: entry.class.clone(),
                caption_key: caption_key(&entry.caption),
                placement: entry.placement.clone(),
                until,
            })
            .collect();
        (moves, missing)
    }

    pub fn state_view(&self) -> StateView {
        StateView {
            revision: self.revision,
            version: env!("CARGO_PKG_VERSION").into(),
            config: self.config.clone(),
            apps: self.windows.apps.clone(),
            running: self.running(),
            snapshots: self.snapshots.items.clone(),
            topology: self.topology.clone(),
            zones: self.zones.clone(),
            script: ScriptStatus::default(),
            conflicts: Vec::new(),
            paused: self.paused,
            data_dir: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: &str, class: &str, caption: &str, x: i32) -> WindowInfo {
        WindowInfo {
            id: id.into(),
            class: class.into(),
            caption: caption.into(),
            placement: Placement {
                output: "DP-1".into(),
                x,
                y: 0,
                w: 800,
                h: 600,
                user_pos: true,
                user_size: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// The same window, sized and placed by the program rather than the user.
    fn by_program(mut info: WindowInfo, w: i32, h: i32) -> WindowInfo {
        info.placement.w = w;
        info.placement.h = h;
        info.placement.user_pos = false;
        info.placement.user_size = false;
        info
    }

    #[test]
    fn remembers_and_restores_a_window() {
        let mut engine = Engine::default();
        assert!(engine.opened(&window("a", "Kate", "x", 0)).is_none());
        engine.closed(&window("a", "Kate", "x", 300));
        let restore = engine.opened(&window("b", "kate", "x", 0)).unwrap();
        assert_eq!(restore.placement.x, 300);
        assert!(restore.geometry);
    }

    #[test]
    fn several_windows_get_their_own_slots() {
        let mut engine = Engine::default();
        engine.opened(&window("a", "firefox", "Mail — Mozilla Firefox", 0));
        engine.opened(&window("b", "firefox", "News — Mozilla Firefox", 0));
        engine.closed(&window("a", "firefox", "Mail — Mozilla Firefox", 100));
        engine.closed(&window("b", "firefox", "News — Mozilla Firefox", 900));
        assert_eq!(engine.windows.apps["firefox"].slots.len(), 2);

        // Title match wins over order.
        let r = engine
            .opened(&window("c", "firefox", "News — Mozilla Firefox", 0))
            .unwrap();
        assert_eq!(r.placement.x, 900);
        let r = engine
            .opened(&window("d", "firefox", "Other — Mozilla Firefox", 0))
            .unwrap();
        assert_eq!(r.placement.x, 100);
        // No third slot to restore from.
        assert!(
            engine
                .opened(&window("e", "firefox", "Third — Mozilla Firefox", 0))
                .is_none()
        );
    }

    #[test]
    fn ignored_programs_are_left_alone() {
        let mut engine = Engine::default();
        engine.closed(&window("a", "steam_app_42", "Game", 10));
        assert!(!engine.windows.apps.contains_key("steam_app_42"));
        assert!(
            engine
                .opened(&window("b", "steam_app_42", "Game", 0))
                .is_none()
        );
    }

    #[test]
    fn pinned_programs_go_to_their_zone() {
        let mut engine = Engine::default();
        engine.windows.apps.insert(
            "konsole".into(),
            AppRecord {
                mode: AppMode::Zone,
                pin: Some(ZonePin {
                    output: "DP-1".into(),
                    index: 2,
                }),
                ..Default::default()
            },
        );
        let r = engine.opened(&window("a", "konsole", "", 0)).unwrap();
        assert_eq!(r.pin.unwrap().index, 2);
        assert!(!r.geometry);
    }

    #[test]
    fn pause_keeps_saving_but_stops_restoring() {
        let mut engine = Engine::default();
        engine.closed(&window("a", "kate", "", 50));
        engine.paused = true;
        assert!(engine.opened(&window("b", "kate", "", 0)).is_none());
        engine.closed(&window("b", "kate", "", 70));
        engine.paused = false;
        assert_eq!(
            engine
                .opened(&window("c", "kate", "", 0))
                .unwrap()
                .placement
                .x,
            70
        );
    }

    #[test]
    fn arrangement_moves_open_windows_and_waits_for_the_rest() {
        let mut engine = Engine::default();
        engine.opened(&window("a", "kate", "notes", 0));
        let snapshot = engine.snapshot_from(
            "s".into(),
            "Work".into(),
            &[
                window("x", "kate", "notes", 500),
                window("y", "dolphin", "Home", 20),
            ],
        );
        let (moves, missing) = engine.plan_restore(&snapshot);
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].0, "a");
        assert_eq!(moves[0].1.placement.x, 500);
        assert_eq!(missing.len(), 1);
        let r = engine.opened(&window("z", "dolphin", "Home", 0)).unwrap();
        assert_eq!(r.placement.x, 20);
        assert!(engine.pending.is_empty());
    }

    #[test]
    fn slots_are_capped() {
        let mut engine = Engine::default();
        for i in 0..10 {
            engine.opened(&window(&format!("w{i}"), "konsole", "", 0));
            engine.changed(&window(&format!("w{i}"), "konsole", "", i));
        }
        assert_eq!(engine.windows.apps["konsole"].slots.len(), MAX_SLOTS);
    }

    #[test]
    fn a_program_sizing_itself_does_not_replace_what_the_user_set() {
        let mut engine = Engine::default();
        engine.opened(&window("a", "steam", "Steam", 0));
        engine.changed(&window("a", "steam", "Steam", 300));
        // The program grows its window later, or a zone sizes it.
        engine.closed(&by_program(window("a", "steam", "Steam", 0), 2400, 1440));
        let slot = &engine.windows.apps["steam"].slots[0].placement;
        assert_eq!((slot.x, slot.w, slot.h), (300, 800, 600));
        assert!(slot.user_pos && slot.user_size);
    }

    #[test]
    fn sizes_nobody_set_by_hand_are_not_marked_as_the_users() {
        let mut engine = Engine::default();
        engine.opened(&window("a", "vesktop", "Discord", 0));
        engine.closed(&by_program(
            window("a", "vesktop", "Discord", 0),
            1800,
            1200,
        ));
        let r = engine
            .opened(&by_program(window("b", "vesktop", "Discord", 0), 900, 700))
            .unwrap();
        assert!(!r.placement.user_pos && !r.placement.user_size);
        assert!(!r.force_geometry);
    }

    #[test]
    fn entries_saved_before_the_markers_count_as_not_set_by_hand() {
        let old: Placement =
            serde_json::from_str(r#"{"output":"DP-1","x":0,"y":0,"w":2402,"h":1440}"#).unwrap();
        assert!(!old.user_pos && !old.user_size);
    }

    #[test]
    fn a_second_window_with_another_title_gets_its_own_slot() {
        let mut engine = Engine::default();
        engine.opened(&window("main", "steam", "Steam", 0));
        engine.closed(&window("main", "steam", "Steam", 100));
        // Main window open again, then the friends list next to it.
        engine.opened(&window("main2", "steam", "Steam", 0));
        assert!(
            engine
                .opened(&window("friends", "steam", "Friends List", 0))
                .is_none()
        );
        engine.closed(&window("friends", "steam", "Friends List", 2000));
        assert_eq!(engine.windows.apps["steam"].slots.len(), 2);
        assert_eq!(engine.windows.apps["steam"].slots[0].placement.x, 100);
    }

    #[test]
    fn a_lone_window_with_a_changing_title_still_finds_its_slot() {
        let mut engine = Engine::default();
        engine.opened(&window("a", "spotify", "Spotify Premium", 0));
        engine.closed(&window("a", "spotify", "Artist - Song", 420));
        let r = engine
            .opened(&window("b", "spotify", "Spotify Premium", 0))
            .unwrap();
        assert_eq!(r.placement.x, 420);
    }

    #[test]
    fn arrangements_set_everything() {
        let mut engine = Engine::default();
        engine.opened(&window("a", "kate", "notes", 0));
        let snapshot = engine.snapshot_from(
            "s".into(),
            "Work".into(),
            &[by_program(window("x", "kate", "notes", 500), 900, 900)],
        );
        let (moves, _) = engine.plan_restore(&snapshot);
        assert!(moves[0].1.force_geometry);
    }

    #[test]
    fn title_families() {
        assert_eq!(title_family("inbox — mozilla firefox"), "mozilla firefox");
        assert_eq!(title_family("a - b — kate"), "kate");
        assert_eq!(title_family("steam"), "steam");
        assert!(same_kind("", "steam"));
        assert!(!same_kind("friends list", "steam"));
    }
}
