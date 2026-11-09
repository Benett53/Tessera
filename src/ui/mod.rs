//! The window.
//!
//! The window never touches files or KWin itself: it shows the service's
//! state and sends requests. A worker thread does the D-Bus calls, so a slow
//! answer never stalls drawing, and polls the service's revision counter to
//! notice changes made elsewhere (a shortcut, the tray menu, a window moved).

mod apps;
mod arrangements;
mod overview;
mod settings;
pub mod theme;
mod widgets;
mod windows;
mod zones;

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use egui::{Align2, Color32, FontId, Id, RichText, vec2};

use crate::model::{Request, Response, Settings, StateView};
use crate::update::Updater;
use crate::{autostart, client};
use theme::Palette;
use widgets::{Icon, Kind};

const SIDEBAR_WIDTH: f32 = 212.0;
pub const LOGO_PNG: &[u8] = include_bytes!("../../assets/icon-64.png");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Zones,
    Windows,
    Arrangements,
    Settings,
}

enum Job {
    Request {
        request: Request,
        ok: Option<String>,
    },
    StartService,
    StopService,
}

enum Msg {
    State(Box<StateView>),
    Done {
        ok: Option<String>,
        result: Result<Response, String>,
    },
    Offline(String),
}

pub struct TesseraApp {
    palette: Palette,
    page: Page,
    state: Option<StateView>,
    offline: Option<String>,
    jobs: Option<Sender<Job>>,
    messages: Option<Receiver<Msg>>,
    toasts: Vec<(String, bool, Instant)>,
    apps: apps::AppCache,
    logo: Option<egui::TextureHandle>,
    updater: Updater,
    autostart: bool,
    demo: bool,
    zones: zones::ZonesState,
    windows: windows::WindowsState,
    arrangements: arrangements::ArrangementsState,
    settings_ui: settings::SettingsState,
    applied_accent: (bool, String),
}

impl TesseraApp {
    pub fn new(cc: &eframe::CreationContext<'_>, demo: Option<StateView>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let settings = demo
            .as_ref()
            .map(|s| s.config.settings.clone())
            .unwrap_or_default();
        let palette = Palette::new(settings.oled_black, theme::parse_accent(&settings.accent));
        theme::apply(&ctx, &palette);

        let (jobs, messages) = if demo.is_none() {
            let (job_tx, job_rx) = channel();
            let (msg_tx, msg_rx) = channel();
            let wake = ctx.clone();
            std::thread::Builder::new()
                .name("tessera-worker".into())
                .spawn(move || worker(job_rx, msg_tx, wake))
                .ok();
            (Some(job_tx), Some(msg_rx))
        } else {
            (None, None)
        };

        let wake = ctx.clone();
        let updater = Updater::new(
            settings.auto_update && demo.is_none(),
            settings.last_update_check,
            move || wake.request_repaint(),
        );

        let logo = crate::tray::decode_png(LOGO_PNG).map(|(w, h, rgba)| {
            ctx.load_texture(
                "logo",
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                egui::TextureOptions::LINEAR,
            )
        });

        let page = std::env::var("TESSERA_PAGE")
            .ok()
            .and_then(|p| match p.as_str() {
                "zones" => Some(Page::Zones),
                "windows" => Some(Page::Windows),
                "arrangements" => Some(Page::Arrangements),
                "settings" => Some(Page::Settings),
                _ => None,
            })
            .unwrap_or(Page::Overview);

        Self {
            palette,
            page,
            state: demo.clone(),
            offline: None,
            jobs,
            messages,
            toasts: Vec::new(),
            apps: apps::AppCache::default(),
            logo,
            updater,
            autostart: autostart::is_enabled(),
            demo: demo.is_some(),
            zones: zones::ZonesState::default(),
            windows: windows::WindowsState::default(),
            arrangements: arrangements::ArrangementsState::default(),
            settings_ui: settings::SettingsState::default(),
            applied_accent: (settings.oled_black, settings.accent),
        }
    }

    pub fn settings(&self) -> Settings {
        self.state
            .as_ref()
            .map(|s| s.config.settings.clone())
            .unwrap_or_default()
    }

    /// Send a request; `ok` is shown when it succeeds.
    fn send(&mut self, request: Request, ok: Option<&str>) {
        if self.demo {
            if let (Request::SetSettings { settings }, Some(state)) =
                (&request, self.state.as_mut())
            {
                state.config.settings = settings.clone();
            }
            return;
        }
        if let Some(jobs) = &self.jobs {
            let _ = jobs.send(Job::Request {
                request,
                ok: ok.map(str::to_string),
            });
        }
    }

    fn job(&self, job: Job) {
        if let Some(jobs) = &self.jobs {
            let _ = jobs.send(job);
        }
    }

    /// Change settings now and tell the service.
    fn update_settings(&mut self, change: impl FnOnce(&mut Settings)) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        change(&mut state.config.settings);
        let settings = state.config.settings.clone();
        self.updater.set_automatic(settings.auto_update);
        self.send(Request::SetSettings { settings }, None);
    }

    fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toasts.push((text.into(), error, Instant::now()));
    }

    fn drain(&mut self) {
        let Some(messages) = &self.messages else {
            return;
        };
        let mut incoming = Vec::new();
        while let Ok(message) = messages.try_recv() {
            incoming.push(message);
        }
        for message in incoming {
            match message {
                Msg::State(state) => {
                    self.offline = None;
                    self.state = Some(*state);
                }
                Msg::Offline(reason) => {
                    self.offline = Some(reason);
                }
                Msg::Done { ok, result } => match result {
                    Ok(Response::Saved { count, .. }) if ok.is_some() => {
                        let text = ok.unwrap_or_default().replace("{n}", &count.to_string());
                        self.toast(text, false);
                    }
                    Ok(Response::Restored { placed, missing }) => {
                        let mut text = format!(
                            "Restored {placed} window{}",
                            if placed == 1 { "" } else { "s" }
                        );
                        if missing > 0 {
                            let launching = self.settings().launch_missing;
                            text.push_str(&format!(
                                "; {missing} not open{}",
                                if launching {
                                    ", starting them"
                                } else {
                                    ", placed when they open"
                                }
                            ));
                        }
                        self.toast(text, false);
                    }
                    Ok(_) => {
                        if let Some(text) = ok {
                            self.toast(text, false);
                        }
                    }
                    Err(error) => self.toast(error, true),
                },
            }
        }
    }

    fn follow_theme(&mut self, ctx: &egui::Context) {
        let settings = self.settings();
        let key = (settings.oled_black, settings.accent.clone());
        if key != self.applied_accent {
            self.applied_accent = key;
            self.palette = Palette::new(settings.oled_black, theme::parse_accent(&settings.accent));
            theme::apply(ctx, &self.palette);
        }
    }

    fn follow_updater(&mut self, ctx: &egui::Context) {
        let last = self.updater.last_check();
        if last != 0 && last != self.settings().last_update_check && self.state.is_some() {
            self.update_settings(|s| s.last_update_check = last);
        }
        if self.updater.wants_exit() {
            // The executable was replaced: restart the service and the window
            // from the new file.
            client::stop_service();
            crate::daemon::spawn_detached(&["--updated"]);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        egui::Panel::left(Id::new("tessera-sidebar"))
            .exact_size(SIDEBAR_WIDTH)
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(palette.sidebar)
                    .inner_margin(egui::Margin::symmetric(12, 14))
                    .stroke(egui::Stroke::new(1.0, palette.border)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(logo) = &self.logo {
                        ui.add(egui::Image::new((logo.id(), vec2(26.0, 26.0))));
                    }
                    ui.label(RichText::new("Tessera").size(18.0).color(palette.text));
                });
                ui.add_space(18.0);
                let update_dot = self.updater.available().is_some();
                for (page, icon, label) in [
                    (Page::Overview, Icon::Overview, "Overview"),
                    (Page::Zones, Icon::Zones, "Zones"),
                    (Page::Windows, Icon::Windows, "Windows"),
                    (Page::Arrangements, Icon::Arrangements, "Arrangements"),
                    (Page::Settings, Icon::Settings, "Settings"),
                ] {
                    let response = widgets::nav_item(ui, &palette, icon, label, self.page == page);
                    if page == Page::Settings && update_dot {
                        ui.painter().circle_filled(
                            egui::pos2(response.rect.right() - 14.0, response.rect.center().y),
                            3.5,
                            palette.accent,
                        );
                    }
                    if response.clicked() {
                        self.page = page;
                    }
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    let (color, text) = match (&self.offline, &self.state) {
                        (Some(_), _) | (None, None) => (palette.danger, "Service stopped"),
                        (None, Some(state)) if !state.script.connected => {
                            (palette.warn, "KWin script offline")
                        }
                        (None, Some(state)) if state.paused => (palette.warn, "Restoring paused"),
                        (None, Some(state)) if !state.config.settings.remember => {
                            (palette.text_dim, "Restoring off")
                        }
                        _ => (palette.ok, "Active"),
                    };
                    let text = if self.demo { "Active" } else { text };
                    let color = if self.demo { palette.ok } else { color };
                    ui.label(
                        RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                            .size(11.5)
                            .color(palette.text_faint),
                    );
                    ui.horizontal(|ui| {
                        widgets::status_dot(ui, color);
                        ui.label(RichText::new(text).size(13.0).color(palette.text_dim));
                    });
                });
            });
    }

    fn offline_banner(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(reason) = self.offline.clone() else {
            if self.state.is_none() && !self.demo {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new("Connecting to the Tessera service…")
                            .color(self.palette.text_dim),
                    );
                });
                return true;
            }
            return false;
        };
        let palette = self.palette;
        widgets::card(ui, &palette, |ui| {
            ui.horizontal(|ui| {
                widgets::status_dot(ui, palette.danger);
                ui.vertical(|ui| {
                    ui.label(RichText::new("The Tessera service is not running").size(15.0));
                    ui.label(RichText::new(reason).size(12.5).color(palette.text_dim));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::button(ui, &palette, "Start service", Kind::Primary).clicked() {
                        self.job(Job::StartService);
                    }
                });
            });
        });
        self.state.is_none()
    }

    fn draw_toasts(&mut self, ctx: &egui::Context) {
        self.toasts
            .retain(|(_, _, at)| at.elapsed() < Duration::from_secs(4));
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        let palette = self.palette;
        let screen = ctx.content_rect();
        let mut y = screen.bottom() - 18.0;
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            Id::new("toasts"),
        ));
        for (text, error, _) in self.toasts.iter().rev() {
            let font = FontId::proportional(13.5);
            let galley = painter.layout(text.clone(), font, palette.text, 420.0);
            let size = galley.size() + vec2(28.0, 18.0);
            let rect = egui::Rect::from_min_size(
                egui::pos2(screen.right() - size.x - 18.0, y - size.y),
                size,
            );
            painter.rect(
                rect,
                theme::CARD_CORNER,
                Color32::from_rgb(0x17, 0x17, 0x1C),
                egui::Stroke::new(
                    1.0,
                    if *error {
                        palette.danger
                    } else {
                        palette.accent.gamma_multiply(0.6)
                    },
                ),
                egui::StrokeKind::Inside,
            );
            painter.galley(rect.min + vec2(14.0, 9.0), galley, palette.text);
            y = rect.top() - 8.0;
        }
        let _ = Align2::CENTER_CENTER;
    }
}

impl eframe::App for TesseraApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain();
        self.follow_theme(ctx);
        self.follow_updater(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.sidebar(ui);
        let palette = self.palette;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(palette.background)
                    .inner_margin(egui::Margin {
                        left: 28,
                        right: 22,
                        top: 20,
                        bottom: 12,
                    }),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .id_salt(self.page as u8)
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width().min(1100.0));
                        if self.offline_banner(ui) {
                            return;
                        }
                        match self.page {
                            Page::Overview => self.overview_page(ui),
                            Page::Zones => self.zones_page(ui),
                            Page::Windows => self.windows_page(ui),
                            Page::Arrangements => self.arrangements_page(ui),
                            Page::Settings => self.settings_page(ui),
                        }
                        ui.add_space(16.0);
                    });
            });
        self.draw_toasts(&ctx);
    }
}

fn worker(jobs: Receiver<Job>, messages: Sender<Msg>, ctx: egui::Context) {
    let send = |m: Msg| {
        let _ = messages.send(m);
        ctx.request_repaint();
    };
    if !client::ensure_service() {
        send(Msg::Offline(
            "It could not be started. Run `tessera --daemon` in a terminal to see why.".into(),
        ));
    }
    let mut last_revision: Option<u64> = None;
    let mut last_full = Instant::now();
    loop {
        match jobs.recv_timeout(Duration::from_millis(700)) {
            Ok(Job::Request { request, ok }) => {
                let result = client::request(&request);
                send(Msg::Done { ok, result });
                last_revision = None;
            }
            Ok(Job::StartService) => {
                if !client::ensure_service() {
                    send(Msg::Offline(
                        "It could not be started. Run `tessera --daemon` in a terminal to see why."
                            .into(),
                    ));
                }
                last_revision = None;
            }
            Ok(Job::StopService) => {
                client::stop_service();
                last_revision = None;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // The script's connection and other scripts' settings change without
        // touching the revision, so a full refresh happens every few seconds.
        let revision = client::revision();
        let stale = last_full.elapsed() > Duration::from_secs(4);
        match revision {
            Some(r) if Some(r) == last_revision && !stale => {}
            Some(r) => match client::request(&Request::State) {
                Ok(Response::State { state }) => {
                    last_revision = Some(r);
                    last_full = Instant::now();
                    send(Msg::State(state));
                }
                Ok(_) => {}
                Err(e) => send(Msg::Offline(e)),
            },
            None => {
                if last_revision.is_some() || stale {
                    last_full = Instant::now();
                    send(Msg::Offline(
                        "Start it here, or log out and back in.".into(),
                    ));
                }
                last_revision = None;
            }
        }
    }
}

/// Window icon from the embedded PNG.
pub fn window_icon() -> Option<egui::IconData> {
    crate::tray::decode_png(LOGO_PNG).map(|(width, height, rgba)| egui::IconData {
        rgba,
        width,
        height,
    })
}

/// Open the folder or link with the desktop's default handler.
pub fn open_external(target: &str) {
    if let Ok(mut child) = std::process::Command::new("xdg-open").arg(target).spawn() {
        std::thread::spawn(move || child.wait());
    }
}

/// `5 min ago`, `yesterday`, a date.
pub fn ago(timestamp: u64) -> String {
    let now = crate::model::now();
    let diff = now.saturating_sub(timestamp);
    match diff {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", diff / 60),
        3600..86400 => format!("{} h ago", diff / 3600),
        86400..172800 => "yesterday".into(),
        _ => format!("{} days ago", diff / 86400),
    }
}
