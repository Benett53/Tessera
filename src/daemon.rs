//! The background service: owns the data, answers the KWin script and the
//! window, shows the tray icon.
//!
//! D-Bus name `app.tessera.Daemon`, object `/app/tessera/Daemon`, interface
//! `app.tessera.Daemon1`. Every payload is a JSON string.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::engine::{Engine, class_key};
use crate::model::*;
use crate::{autostart, kwin, store, tray};

pub const BUS_NAME: &str = "app.tessera.Daemon";
pub const OBJECT_PATH: &str = "/app/tessera/Daemon";
pub const INTERFACE: &str = "app.tessera.Daemon1";
/// Owned by the open window, so a second start can bring it forward instead.
pub const GUI_NAME: &str = "app.tessera.Gui";
/// Window class of Tessera's own window (Wayland app id).
pub const APP_ID: &str = "tessera";
/// Id of the arrangement the save shortcut writes to.
pub const QUICK_ID: &str = "quick";

const POLL_SECS: u64 = 20;
const REPLY_TIMEOUT: Duration = Duration::from_secs(4);

static TERMINATE: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    TERMINATE.store(true, Ordering::Relaxed);
}

pub struct Shared {
    pub engine: Mutex<Engine>,
    commands_tx: async_channel::Sender<String>,
    commands_rx: async_channel::Receiver<String>,
    replies: Mutex<HashMap<String, async_channel::Sender<String>>>,
    polling: AtomicUsize,
    last_poll: Mutex<Option<Instant>>,
    script_version: Mutex<String>,
    conflicts: Mutex<Vec<Conflict>>,
    script_flags: Mutex<(bool, bool)>,
    counter: AtomicU64,
    pub quit: AtomicBool,
}

type Result<T> = std::result::Result<T, String>;

impl Shared {
    fn new(engine: Engine) -> Self {
        let (commands_tx, commands_rx) = async_channel::unbounded();
        Self {
            engine: Mutex::new(engine),
            commands_tx,
            commands_rx,
            replies: Mutex::default(),
            polling: AtomicUsize::new(0),
            last_poll: Mutex::new(None),
            script_version: Mutex::default(),
            conflicts: Mutex::default(),
            script_flags: Mutex::default(),
            counter: AtomicU64::new(0),
            quit: AtomicBool::new(false),
        }
    }

    pub fn engine(&self) -> std::sync::MutexGuard<'_, Engine> {
        self.engine.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn send(&self, command: serde_json::Value) {
        let _ = self.commands_tx.try_send(command.to_string());
    }

    fn new_id(&self, prefix: &str) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("{prefix}{:x}{n}", now())
    }

    pub fn connected(&self) -> bool {
        self.polling.load(Ordering::Relaxed) > 0
            || self
                .last_poll
                .lock()
                .ok()
                .and_then(|t| *t)
                .is_some_and(|t| t.elapsed() < Duration::from_secs(POLL_SECS + 10))
    }

    /// Send a command and wait for the script's `Report` with the same key.
    async fn ask(&self, command: serde_json::Value, key: String) -> Result<String> {
        if !self.connected() {
            return Err("The KWin script is not connected.".into());
        }
        let (tx, rx) = async_channel::bounded(1);
        if let Ok(mut replies) = self.replies.lock() {
            replies.insert(key.clone(), tx);
        }
        self.send(command);
        let answer = futures_lite::future::or(async { rx.recv().await.ok() }, async {
            async_io::Timer::after(REPLY_TIMEOUT).await;
            None
        })
        .await;
        if let Ok(mut replies) = self.replies.lock() {
            replies.remove(&key);
        }
        answer.ok_or_else(|| "KWin did not answer in time.".to_string())
    }

    fn script_config(&self) -> serde_json::Value {
        let engine = self.engine();
        json!({ "zone_after_unmaximize": engine.settings().zone_after_unmaximize })
    }

    pub fn refresh_caches(&self) {
        let conflicts = kwin::conflicts();
        if let Ok(mut c) = self.conflicts.lock() {
            *c = conflicts;
        }
        let flags = (kwin::is_installed(), kwin::is_enabled());
        if let Ok(mut f) = self.script_flags.lock() {
            *f = flags;
        }
    }

    pub fn state_view(&self) -> StateView {
        let mut view = self.engine().state_view();
        let (installed, enabled) = self.script_flags.lock().map(|f| *f).unwrap_or_default();
        view.script = ScriptStatus {
            installed,
            enabled,
            connected: self.connected(),
            version: self
                .script_version
                .lock()
                .map(|v| v.clone())
                .unwrap_or_default(),
        };
        view.conflicts = self.conflicts.lock().map(|c| c.clone()).unwrap_or_default();
        view.data_dir = store::data_dir().display().to_string();
        view
    }

    pub fn flush(&self) {
        let mut engine = self.engine();
        if engine.dirty_config {
            engine.dirty_config = false;
            if let Err(e) = store::save(&store::config_path(), &engine.config) {
                eprintln!("tessera: saving settings failed: {e}");
            }
        }
        if engine.dirty_windows {
            engine.dirty_windows = false;
            if let Err(e) = store::save(&store::windows_path(), &engine.windows) {
                eprintln!("tessera: saving windows failed: {e}");
            }
        }
        if engine.dirty_snapshots {
            engine.dirty_snapshots = false;
            if let Err(e) = store::save(&store::snapshots_path(), &engine.snapshots) {
                eprintln!("tessera: saving arrangements failed: {e}");
            }
        }
    }

    pub async fn save_snapshot(
        &self,
        name: String,
        replace: Option<String>,
    ) -> Result<(String, usize)> {
        let req = self.new_id("s");
        let text = self
            .ask(
                json!({ "cmd": "snapshot", "req": req }),
                format!("snapshot:{req}"),
            )
            .await?;
        let windows: Vec<WindowInfo> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let mut engine = self.engine();
        let id = replace.unwrap_or_else(|| self.new_id("a"));
        let snapshot = engine.snapshot_from(id.clone(), name, &windows);
        let count = snapshot.windows.len();
        let items = &mut engine.snapshots.items;
        match items.iter_mut().find(|s| s.id == id) {
            Some(existing) => {
                let name = if snapshot.name.is_empty() {
                    existing.name.clone()
                } else {
                    snapshot.name.clone()
                };
                *existing = Snapshot { name, ..snapshot };
            }
            None => items.push(snapshot),
        }
        engine.dirty_snapshots = true;
        engine.bump();
        Ok((id, count))
    }

    pub fn restore_snapshot(&self, id: &str) -> Result<(usize, usize)> {
        if !self.connected() {
            return Err("The KWin script is not connected.".into());
        }
        let mut engine = self.engine();
        let snapshot = engine
            .snapshots
            .items
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or("This arrangement no longer exists.")?;
        let (moves, missing) = engine.plan_restore(&snapshot);
        let launch = engine.settings().launch_missing;
        drop(engine);
        let items: Vec<_> = moves
            .iter()
            .map(|(id, restore)| json!({ "id": id, "restore": restore }))
            .collect();
        self.send(json!({ "cmd": "place", "items": items }));
        if launch {
            let mut started: Vec<&str> = Vec::new();
            for entry in &missing {
                let app = if entry.desktop_file.is_empty() {
                    entry.class.as_str()
                } else {
                    entry.desktop_file.as_str()
                };
                if !started.contains(&app) {
                    started.push(app);
                    launch_app(app);
                }
            }
        }
        Ok((moves.len(), missing.len()))
    }

    /// The arrangement the restore shortcut uses: the quick one, else the newest.
    fn default_snapshot(&self) -> Option<String> {
        let engine = self.engine();
        let items = &engine.snapshots.items;
        items
            .iter()
            .find(|s| s.id == QUICK_ID)
            .or_else(|| items.iter().max_by_key(|s| s.created))
            .map(|s| s.id.clone())
    }

    pub fn set_paused(&self, paused: bool) {
        let mut engine = self.engine();
        engine.paused = paused;
        engine.bump();
    }

    pub fn show_window(&self) {
        if gui_running() {
            self.send(json!({ "cmd": "activate", "class": APP_ID }));
        } else {
            spawn_detached(&[]);
        }
    }

    pub async fn shortcut(&self, name: &str) {
        match name {
            "open" => self.show_window(),
            "save" => match self
                .save_snapshot("Quick save".into(), Some(QUICK_ID.into()))
                .await
            {
                Ok((_, count)) => {
                    kwin::osd("view-grid", &format!("Arrangement saved · {count} windows"))
                }
                Err(e) => kwin::osd("dialog-error", &e),
            },
            "restore" => match self.default_snapshot() {
                Some(id) => match self.restore_snapshot(&id) {
                    Ok((placed, _)) => kwin::osd(
                        "view-grid",
                        &format!("Arrangement restored · {placed} windows"),
                    ),
                    Err(e) => kwin::osd("dialog-error", &e),
                },
                None => kwin::osd("dialog-information", "No arrangement saved yet"),
            },
            "pause" => {
                let paused = !self.engine().paused;
                self.set_paused(paused);
                kwin::osd(
                    if paused {
                        "media-playback-pause"
                    } else {
                        "media-playback-start"
                    },
                    if paused {
                        "Restoring windows paused"
                    } else {
                        "Restoring windows resumed"
                    },
                );
            }
            _ => {}
        }
    }

    pub async fn handle(&self, request: Request) -> Response {
        match self.handle_inner(request).await {
            Ok(response) => response,
            Err(message) => Response::Error { message },
        }
    }

    async fn handle_inner(&self, request: Request) -> Result<Response> {
        match request {
            Request::State => {
                return Ok(Response::State {
                    state: Box::new(self.state_view()),
                });
            }
            Request::SetSettings { settings } => {
                let mut engine = self.engine();
                engine.config.settings = settings;
                engine.dirty_config = true;
                engine.bump();
                drop(engine);
                self.send(json!({ "cmd": "config", "config": self.script_config() }));
            }
            Request::SetApp { class, mode, pin } => {
                let mut engine = self.engine();
                let app = engine.windows.apps.entry(class_key(&class)).or_default();
                app.mode = mode;
                app.pin = pin;
                engine.dirty_windows = true;
                engine.bump();
            }
            Request::ForgetApp { class } => {
                let mut engine = self.engine();
                engine.windows.apps.remove(&class_key(&class));
                let key = class_key(&class);
                for live in engine.live.values_mut().filter(|l| l.class == key) {
                    live.slot = None;
                }
                engine.dirty_windows = true;
                engine.bump();
            }
            Request::ForgetSlot { class, index } => {
                let mut engine = self.engine();
                let key = class_key(&class);
                if let Some(app) = engine.windows.apps.get_mut(&key)
                    && index < app.slots.len()
                {
                    app.slots.remove(index);
                }
                for live in engine.live.values_mut().filter(|l| l.class == key) {
                    live.slot = match live.slot {
                        Some(s) if s == index => None,
                        Some(s) if s > index => Some(s - 1),
                        other => other,
                    };
                }
                engine.dirty_windows = true;
                engine.bump();
            }
            Request::ForgetAll => {
                let mut engine = self.engine();
                engine.windows.apps.clear();
                for live in engine.live.values_mut() {
                    live.slot = None;
                }
                engine.dirty_windows = true;
                engine.bump();
            }
            Request::ApplyZones {
                output,
                desktop,
                mut tree,
                padding,
            } => {
                if !self.connected() {
                    return Err("The KWin script is not connected.".into());
                }
                tree.normalize();
                self.send(json!({
                    "cmd": "apply_zones", "output": output, "desktop": desktop,
                    "tree": tree, "padding": padding.min(64),
                }));
            }
            Request::RefreshZones => self.send(json!({ "cmd": "zones" })),
            Request::SaveLayout { name, mut tree } => {
                tree.normalize();
                let id = self.new_id("l");
                let mut engine = self.engine();
                engine.config.layouts.push(SavedLayout {
                    id: id.clone(),
                    name,
                    tree,
                });
                engine.dirty_config = true;
                engine.bump();
                return Ok(Response::Saved { id, count: 1 });
            }
            Request::DeleteLayout { id } => {
                let mut engine = self.engine();
                engine.config.layouts.retain(|l| l.id != id);
                engine.dirty_config = true;
                engine.bump();
            }
            Request::SaveSnapshot { name, replace } => {
                let (id, count) = self.save_snapshot(name, replace).await?;
                return Ok(Response::Saved { id, count });
            }
            Request::RestoreSnapshot { id } => {
                let (placed, missing) = self.restore_snapshot(&id)?;
                return Ok(Response::Restored { placed, missing });
            }
            Request::RenameSnapshot { id, name } => {
                let mut engine = self.engine();
                if let Some(s) = engine.snapshots.items.iter_mut().find(|s| s.id == id) {
                    s.name = name;
                }
                engine.dirty_snapshots = true;
                engine.bump();
            }
            Request::DeleteSnapshot { id } => {
                let mut engine = self.engine();
                engine.snapshots.items.retain(|s| s.id != id);
                engine.dirty_snapshots = true;
                engine.bump();
            }
            Request::SetPaused { paused } => self.set_paused(paused),
            Request::DisableConflict { plugin } => {
                kwin::disable_plugin(&plugin)?;
                self.refresh_caches();
                self.engine().bump();
            }
            Request::RepairScript => {
                kwin::install()?;
                kwin::enable(true)?;
                self.refresh_caches();
                self.engine().bump();
            }
            Request::ShowWindow => self.show_window(),
            Request::Quit => self.quit.store(true, Ordering::Relaxed),
        }
        Ok(Response::Ok)
    }
}

/// Start a program by its desktop entry, the way the launcher would.
fn launch_app(app: &str) {
    let spawned = std::process::Command::new("kstart")
        .args(["--application", app])
        .spawn()
        .or_else(|_| std::process::Command::new("gtk-launch").arg(app).spawn());
    if let Ok(mut child) = spawned {
        std::thread::spawn(move || child.wait());
    }
}

/// Start another copy of this program in its own session, so it outlives us.
pub fn spawn_detached(args: &[&str]) {
    use std::os::unix::process::CommandExt;
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut command = std::process::Command::new(exe);
    command.args(args);
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    if let Ok(mut child) = command.spawn() {
        std::thread::spawn(move || child.wait());
    }
}

pub fn gui_running() -> bool {
    name_has_owner(GUI_NAME)
}

pub fn name_has_owner(name: &str) -> bool {
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return false;
    };
    connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &(name,),
        )
        .ok()
        .and_then(|m| m.body().deserialize::<bool>().ok())
        .unwrap_or(false)
}

struct Iface {
    shared: Arc<Shared>,
}

fn parse<T: serde::de::DeserializeOwned>(text: &str) -> Option<T> {
    match serde_json::from_str(text) {
        Ok(value) => Some(value),
        Err(error) => {
            eprintln!("tessera: bad message from KWin: {error}");
            None
        }
    }
}

#[zbus::interface(name = "app.tessera.Daemon1")]
impl Iface {
    /// The script started. Returns its settings.
    async fn hello(&self, version: String) -> String {
        let s = &self.shared;
        while s.commands_rx.try_recv().is_ok() {}
        if let Ok(mut v) = s.script_version.lock() {
            *v = version;
        }
        if let Ok(mut t) = s.last_poll.lock() {
            *t = Some(Instant::now());
        }
        s.engine().hello();
        s.send(json!({ "cmd": "zones" }));
        s.script_config().to_string()
    }

    /// Long-poll for the next command; empty after 20 seconds.
    async fn next(&self) -> String {
        let s = &self.shared;
        s.polling.fetch_add(1, Ordering::Relaxed);
        let rx = s.commands_rx.clone();
        let command =
            futures_lite::future::or(async { rx.recv().await.unwrap_or_default() }, async {
                async_io::Timer::after(Duration::from_secs(POLL_SECS)).await;
                String::new()
            })
            .await;
        s.polling.fetch_sub(1, Ordering::Relaxed);
        if let Ok(mut t) = s.last_poll.lock() {
            *t = Some(Instant::now());
        }
        command
    }

    async fn topology(&self, json: String) {
        if let Some(topology) = parse::<Topology>(&json) {
            let mut engine = self.shared.engine();
            engine.topology = topology;
            engine.bump();
        }
        self.shared.send(json!({ "cmd": "zones" }));
    }

    async fn existing(&self, json: String) {
        if let Some(list) = parse::<Vec<WindowInfo>>(&json) {
            let mut engine = self.shared.engine();
            for info in &list {
                engine.changed(info);
            }
        }
    }

    async fn opened(&self, json: String) -> String {
        let Some(info) = parse::<WindowInfo>(&json) else {
            return String::new();
        };
        self.shared
            .engine()
            .opened(&info)
            .and_then(|r| serde_json::to_string(&r).ok())
            .unwrap_or_default()
    }

    async fn changed(&self, json: String) {
        if let Some(info) = parse::<WindowInfo>(&json) {
            self.shared.engine().changed(&info);
        }
    }

    async fn closed(&self, json: String) {
        if let Some(info) = parse::<WindowInfo>(&json) {
            self.shared.engine().closed(&info);
        }
    }

    async fn report(&self, kind: String, json: String) {
        if kind == "zones" {
            if let Some(zones) = parse::<Vec<ZoneState>>(&json) {
                let mut engine = self.shared.engine();
                engine.zones = zones;
                engine.bump();
            }
            return;
        }
        let sender = self
            .shared
            .replies
            .lock()
            .ok()
            .and_then(|mut r| r.remove(&kind));
        if let Some(sender) = sender {
            let _ = sender.try_send(json);
        }
    }

    async fn shortcut(&self, name: String) {
        self.shared.shortcut(&name).await;
    }

    async fn request(&self, json: String) -> String {
        let response = match serde_json::from_str::<Request>(&json) {
            Ok(request) => self.shared.handle(request).await,
            Err(e) => Response::Error {
                message: format!("bad request: {e}"),
            },
        };
        serde_json::to_string(&response).unwrap_or_default()
    }

    async fn revision(&self) -> u64 {
        self.shared.engine().revision
    }
}

/// Run the service until it is asked to quit or the session ends.
pub fn run() -> std::result::Result<(), String> {
    let config: Config = store::load(&store::config_path());
    let windows: WindowsFile = store::load(&store::windows_path());
    let snapshots: SnapshotsFile = store::load(&store::snapshots_path());
    let mut engine = Engine::new(config, windows, snapshots);
    // Fresh installs start with the defaults written out, so the file can be
    // found and edited.
    let fresh = !store::config_path().exists();
    if fresh {
        engine.dirty_config = true;
    }
    let shared = Arc::new(Shared::new(engine));

    let _connection = match zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(BUS_NAME))
        .and_then(|b| {
            b.serve_at(
                OBJECT_PATH,
                Iface {
                    shared: shared.clone(),
                },
            )
        })
        .and_then(|b| b.build())
    {
        Ok(connection) => connection,
        Err(zbus::Error::NameTaken) => {
            eprintln!("tessera: the service is already running");
            return Ok(());
        }
        Err(e) => return Err(format!("cannot register on the session bus: {e}")),
    };

    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGHUP, on_signal as *const () as libc::sighandler_t);
    }

    if let Err(e) = kwin::ensure() {
        eprintln!("tessera: KWin script: {e}");
    }
    shared.refresh_caches();
    // Start at login is on by default; afterwards it is the user's choice.
    if fresh && std::env::var_os("TESSERA_HOME").is_none() {
        let _ = autostart::set(true);
    }
    autostart::repair();

    let mut tray_handle = None;
    let mut tray_revision = u64::MAX;
    let mut last_flush = Instant::now();
    let mut last_refresh = Instant::now();
    loop {
        if TERMINATE.load(Ordering::Relaxed) || shared.quit.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));

        let (want_tray, revision) = {
            let engine = shared.engine();
            (engine.settings().tray, engine.revision)
        };
        match (want_tray, tray_handle.is_some()) {
            (true, false) => {
                tray_handle = tray::spawn(shared.clone());
                tray_revision = u64::MAX;
            }
            (false, true) => {
                if let Some(handle) = tray_handle.take() {
                    tray::shutdown(handle);
                }
            }
            _ => {}
        }
        if let Some(handle) = &tray_handle
            && revision != tray_revision
        {
            tray_revision = revision;
            tray::refresh(handle);
        }

        if last_flush.elapsed() > Duration::from_secs(2) {
            last_flush = Instant::now();
            shared.flush();
        }
        if last_refresh.elapsed() > Duration::from_secs(30) {
            last_refresh = Instant::now();
            shared.refresh_caches();
        }
    }
    shared.flush();
    if let Some(handle) = tray_handle {
        tray::shutdown(handle);
    }
    Ok(())
}
