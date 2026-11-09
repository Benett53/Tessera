//! Tessera: window zones and remembered window positions for KDE Plasma.
//!
//! One executable, three roles:
//!   tessera            the window (starts the service when needed)
//!   tessera --daemon   the background service with the tray icon
//!   tessera --save …   command line, talks to the running service

mod autostart;
mod client;
mod daemon;
mod engine;
mod kwin;
mod layout;
mod model;
mod store;
mod tray;
mod ui;
mod update;

use std::time::{Duration, Instant};

use model::{Request, Response, StateView};

const HELP: &str = "\
Tessera – window zones and remembered window positions for KDE Plasma

Usage:
  tessera                     open the window
  tessera --daemon            run the background service
  tessera --save [NAME]       save an arrangement (default: the quick one)
  tessera --restore [NAME]    restore an arrangement (default: the quick one)
  tessera --pause | --resume  pause or resume restoring windows
  tessera --status            show what the service knows
  tessera --quit              stop the service
  tessera --remove-kwin-script  turn off and delete the KWin script
  tessera --version
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args.first().map(String::as_str).unwrap_or("");
    let rest = args.get(1..).map(|r| r.join(" ")).unwrap_or_default();

    let code = match first {
        "--help" | "-h" => {
            print!("{HELP}");
            0
        }
        "--version" | "-V" => {
            println!("tessera {}", env!("CARGO_PKG_VERSION"));
            0
        }
        "--daemon" => match daemon::run() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("tessera: {e}");
                1
            }
        },
        "--save" => cli(|| {
            let (name, replace) = if rest.is_empty() {
                ("Quick save".to_string(), Some(daemon::QUICK_ID.to_string()))
            } else {
                (rest.clone(), find_snapshot(&rest).ok())
            };
            match client::request(&Request::SaveSnapshot { name, replace })? {
                Response::Saved { count, .. } => Ok(format!("Saved {count} windows")),
                _ => Ok(String::new()),
            }
        }),
        "--restore" => cli(|| {
            let id = if rest.is_empty() {
                daemon::QUICK_ID.to_string()
            } else {
                find_snapshot(&rest)?
            };
            match client::request(&Request::RestoreSnapshot { id })? {
                Response::Restored { placed, missing } => {
                    Ok(format!("Restored {placed} windows, {missing} not open"))
                }
                _ => Ok(String::new()),
            }
        }),
        "--pause" | "--resume" => cli(|| {
            client::request(&Request::SetPaused {
                paused: first == "--pause",
            })?;
            Ok(if first == "--pause" {
                "Paused"
            } else {
                "Resumed"
            }
            .into())
        }),
        "--status" => cli(|| {
            let state = state()?;
            let mut out = format!(
                "service {}\nKWin script: {}{}\nscreens: {}\nprograms remembered: {}\narrangements: {}\nrestoring: {}",
                state.version,
                if state.script.connected {
                    "connected"
                } else {
                    "not connected"
                },
                if state.script.version.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", state.script.version)
                },
                state
                    .topology
                    .screens
                    .iter()
                    .map(|s| format!("{} {}x{}", s.name, s.w, s.h))
                    .collect::<Vec<_>>()
                    .join(", "),
                state.apps.len(),
                state.snapshots.len(),
                if state.paused {
                    "paused"
                } else if state.config.settings.remember {
                    "on"
                } else {
                    "off"
                },
            );
            for conflict in &state.conflicts {
                out.push_str(&format!(
                    "\nconflict: {} ({})",
                    conflict.name, conflict.reason
                ));
            }
            Ok(out)
        }),
        "--quit" => {
            client::stop_service();
            0
        }
        "--remove-kwin-script" => match kwin::remove() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("tessera: {e}");
                1
            }
        },
        "" | "--updated" | "--show" => run_window(first == "--updated"),
        other => {
            eprintln!("tessera: unknown option {other}\n\n{HELP}");
            2
        }
    };
    std::process::exit(code);
}

fn cli(f: impl FnOnce() -> Result<String, String>) -> i32 {
    match f() {
        Ok(text) => {
            if !text.is_empty() {
                println!("{text}");
            }
            0
        }
        Err(e) => {
            eprintln!("tessera: {e}");
            1
        }
    }
}

fn state() -> Result<StateView, String> {
    match client::request(&Request::State)? {
        Response::State { state } => Ok(*state),
        _ => Err("unexpected answer".into()),
    }
}

fn find_snapshot(name: &str) -> Result<String, String> {
    let state = state()?;
    state
        .snapshots
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(name.trim()))
        .map(|s| s.id.clone())
        .ok_or_else(|| format!("no arrangement named “{name}”"))
}

fn run_window(updated: bool) -> i32 {
    // The demo state is for screenshots: no service, sample data.
    let demo: Option<StateView> = std::env::var_os("TESSERA_DEMO").and_then(|path| {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    });

    // One window at a time. A second start brings the first one forward.
    let mut guard = None;
    if demo.is_none() {
        let start = Instant::now();
        loop {
            match zbus::blocking::connection::Builder::session()
                .and_then(|b| b.name(daemon::GUI_NAME))
                .and_then(|b| b.build())
            {
                Ok(connection) => {
                    guard = Some(connection);
                    break;
                }
                // After an update the old window may still be closing.
                Err(zbus::Error::NameTaken)
                    if updated && start.elapsed() < Duration::from_secs(10) =>
                {
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(zbus::Error::NameTaken) => {
                    let _ = client::request(&Request::ShowWindow);
                    return 0;
                }
                Err(e) => {
                    eprintln!("tessera: session bus unavailable: {e}");
                    break;
                }
            }
        }
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Tessera")
        .with_app_id(daemon::APP_ID)
        .with_inner_size([1080.0, 720.0])
        .with_min_inner_size([820.0, 560.0]);
    if let Some(icon) = ui::window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let result = eframe::run_native(
        "Tessera",
        options,
        Box::new(move |cc| Ok(Box::new(ui::TesseraApp::new(cc, demo)))),
    );
    drop(guard);
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("tessera: {e}");
            1
        }
    }
}
