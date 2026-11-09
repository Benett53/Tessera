//! Settings.

use egui::RichText;

use super::overview::SHORTCUTS;
use super::widgets::{self, Kind};
use super::{Job, TesseraApp, open_external, theme};
use crate::autostart;
use crate::model::{Settings, default_ignore};
use crate::update::{CURRENT_VERSION, Status};

#[derive(Default)]
pub struct SettingsState {
    ignore_input: String,
}

const REPO_URL: &str = "https://github.com/Benett53/Tessera";

impl TesseraApp {
    pub(super) fn settings_page(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let Some(state) = self.state.clone() else {
            return;
        };
        let s = state.config.settings.clone();
        widgets::page_title(ui, &palette, "Settings", "");

        widgets::section(ui, &palette, "Restoring windows");
        widgets::card(ui, &palette, |ui| {
            type Field = fn(&mut Settings) -> &mut bool;
            let rows: [(&str, &str, Field); 7] = [
                ("Position and size", "", |s| &mut s.restore_geometry),
                (
                    "Screen",
                    "Falls back to the current screen when that one is gone",
                    |s| &mut s.restore_screen,
                ),
                ("Virtual desktop", "", |s| &mut s.restore_desktop),
                ("Zone", "Put the window back into the zone it was in", |s| {
                    &mut s.restore_zone
                }),
                ("Maximised and full screen", "", |s| {
                    &mut s.restore_maximized
                }),
                ("Keep above others", "", |s| &mut s.restore_above),
                ("Minimised", "Off: windows always open visible", |s| {
                    &mut s.restore_minimized
                }),
            ];
            for (i, (title, description, field)) in rows.into_iter().enumerate() {
                if i > 0 {
                    widgets::separator(ui, &palette);
                }
                let mut copy = s.clone();
                let mut value = *field(&mut copy);
                widgets::setting_row(ui, &palette, title, description, |ui| {
                    if widgets::toggle(ui, &palette, &mut value).changed() {
                        self.update_settings(|s| *field(s) = value);
                    }
                });
            }
        });

        widgets::section(ui, &palette, "Zones");
        widgets::card(ui, &palette, |ui| {
            let mut back = s.zone_after_unmaximize;
            widgets::setting_row(
                ui,
                &palette,
                "Back into the zone after maximising",
                "Restoring a maximised window puts it back into its zone",
                |ui| {
                    if widgets::toggle(ui, &palette, &mut back).changed() {
                        self.update_settings(|s| s.zone_after_unmaximize = back);
                    }
                },
            );
        });

        widgets::section(ui, &palette, "Programs left alone");
        widgets::card(ui, &palette, |ui| {
            ui.label(
                RichText::new("Window classes Tessera never moves or saves. * matches anything, as in steam_app_*.")
                    .size(12.5)
                    .color(palette.text_dim),
            );
            ui.add_space(4.0);
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                for (i, pattern) in s.ignore.iter().enumerate() {
                    if widgets::chip(ui, &palette, &format!("{pattern}  ×"), false)
                        .on_hover_text("Remove")
                        .clicked()
                    {
                        remove = Some(i);
                    }
                }
            });
            if let Some(i) = remove {
                self.update_settings(|s| {
                    s.ignore.remove(i);
                });
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.settings_ui.ignore_input)
                        .hint_text("Window class")
                        .desired_width(200.0),
                );
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (widgets::button(ui, &palette, "Add", Kind::Normal).clicked() || enter)
                    && !self.settings_ui.ignore_input.trim().is_empty()
                {
                    let pattern = self.settings_ui.ignore_input.trim().to_lowercase();
                    self.settings_ui.ignore_input.clear();
                    self.update_settings(|s| {
                        if !s.ignore.contains(&pattern) {
                            s.ignore.push(pattern);
                        }
                    });
                }
                let open: Vec<String> = state
                    .running
                    .iter()
                    .filter(|c| !s.ignore.contains(c))
                    .cloned()
                    .collect();
                if !open.is_empty() {
                    let mut picked = None;
                    egui::ComboBox::from_id_salt("ignore-running")
                        .selected_text("Add an open program")
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for class in &open {
                                if ui.selectable_label(false, class).clicked() {
                                    picked = Some(class.clone());
                                }
                            }
                        });
                    if let Some(class) = picked {
                        self.update_settings(|s| s.ignore.push(class));
                    }
                }
                if widgets::button(ui, &palette, "Defaults", Kind::Quiet).clicked() {
                    self.update_settings(|s| s.ignore = default_ignore());
                }
            });
        });

        widgets::section(ui, &palette, "Startup");
        widgets::card(ui, &palette, |ui| {
            let mut autostart_on = self.autostart;
            widgets::setting_row(
                ui,
                &palette,
                "Start at login",
                "Runs the Tessera service in the background",
                |ui| {
                    if widgets::toggle(ui, &palette, &mut autostart_on).changed() {
                        match autostart::set(autostart_on) {
                            Ok(()) => self.autostart = autostart_on,
                            Err(e) => self.toast(format!("Autostart: {e}"), true),
                        }
                    }
                },
            );
            widgets::separator(ui, &palette);
            let mut tray = s.tray;
            widgets::setting_row(
                ui,
                &palette,
                "Tray icon",
                "Quick access to arrangements and pausing",
                |ui| {
                    if widgets::toggle(ui, &palette, &mut tray).changed() {
                        self.update_settings(|s| s.tray = tray);
                    }
                },
            );
        });

        widgets::section(ui, &palette, "Appearance");
        widgets::card(ui, &palette, |ui| {
            let mut black = s.oled_black;
            widgets::setting_row(
                ui,
                &palette,
                "True black",
                "Pure black background, for OLED screens",
                |ui| {
                    if widgets::toggle(ui, &palette, &mut black).changed() {
                        self.update_settings(|s| s.oled_black = black);
                    }
                },
            );
            widgets::separator(ui, &palette);
            widgets::setting_row(ui, &palette, "Accent", "", |ui| {
                for (name, hex) in theme::ACCENT_CHOICES.iter().rev() {
                    let colour = theme::parse_accent(hex);
                    let active = s.accent.eq_ignore_ascii_case(hex);
                    if widgets::colour_swatch(ui, &palette, colour, active)
                        .on_hover_text(*name)
                        .clicked()
                    {
                        let hex = hex.to_string();
                        self.update_settings(|s| s.accent = hex);
                    }
                }
            });
        });

        widgets::section(ui, &palette, "Shortcuts");
        widgets::card(ui, &palette, |ui| {
            for (combo, action) in SHORTCUTS {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(*action).size(13.5));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        widgets::keys(ui, &palette, combo);
                    });
                });
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("They belong to KWin and can be changed under Shortcuts > KWin, search for Tessera.")
                        .size(12.5)
                        .color(palette.text_dim),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::button(ui, &palette, "Change shortcuts", Kind::Normal).clicked()
                        && let Ok(mut child) = std::process::Command::new("systemsettings")
                            .arg("kcm_keys")
                            .spawn()
                    {
                        std::thread::spawn(move || child.wait());
                    }
                });
            });
        });

        widgets::section(ui, &palette, "Updates");
        widgets::card(ui, &palette, |ui| {
            let status = self.updater.status();
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("Tessera {CURRENT_VERSION}")).size(14.0));
                    let line = match &status {
                        Status::Idle => match self.updater.last_check() {
                            0 => "Not checked yet".to_string(),
                            t => format!("Checked {}", super::ago(t)),
                        },
                        Status::Checking => "Checking…".into(),
                        Status::UpToDate => "This is the latest version".into(),
                        Status::Available(r) => format!("Version {} is available", r.version),
                        Status::Downloading(got, total) => match total {
                            Some(t) if *t > 0 => format!("Downloading… {}%", got * 100 / t),
                            _ => format!("Downloading… {} KB", got / 1024),
                        },
                        Status::Installing => "Installing, Tessera restarts…".into(),
                        Status::Failed(e) => e.clone(),
                    };
                    let colour = match status {
                        Status::Available(_) => palette.accent,
                        Status::Failed(_) => palette.danger,
                        _ => palette.text_dim,
                    };
                    ui.label(RichText::new(line).size(12.5).color(colour));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let wake = ui.ctx().clone();
                    match self.updater.available() {
                        Some(release) => {
                            if widgets::button(ui, &palette, "Download and install", Kind::Primary)
                                .clicked()
                            {
                                self.updater
                                    .install(release, move || wake.request_repaint());
                            }
                        }
                        None => {
                            let busy = matches!(
                                status,
                                Status::Checking | Status::Downloading(..) | Status::Installing
                            );
                            if widgets::button_enabled(
                                ui,
                                &palette,
                                "Check for updates",
                                Kind::Normal,
                                !busy && !self.demo,
                            )
                            .clicked()
                            {
                                self.updater.check(move || wake.request_repaint());
                            }
                        }
                    }
                });
            });
            widgets::separator(ui, &palette);
            let mut auto = s.auto_update;
            widgets::setting_row(
                ui,
                &palette,
                "Check automatically once a day",
                "Asks GitHub for the latest release, nothing else",
                |ui| {
                    if widgets::toggle(ui, &palette, &mut auto).changed() {
                        self.update_settings(|s| s.auto_update = auto);
                    }
                },
            );
        });

        widgets::section(ui, &palette, "Data");
        widgets::card(ui, &palette, |ui| {
            widgets::setting_row(
                ui,
                &palette,
                "Saved windows and arrangements",
                &state.data_dir,
                |ui| {
                    if widgets::button(ui, &palette, "Open folder", Kind::Normal).clicked() {
                        open_external(&state.data_dir);
                    }
                },
            );
            widgets::separator(ui, &palette);
            widgets::setting_row(
                ui,
                &palette,
                "Background service",
                "Quit stops restoring until the next login or until Tessera is opened again",
                |ui| {
                    if widgets::button(ui, &palette, "Quit service", Kind::Quiet).clicked() {
                        self.job(Job::StopService);
                    }
                },
            );
        });

        widgets::section(ui, &palette, "About");
        widgets::card(ui, &palette, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Tessera").size(14.0));
                ui.label(
                    RichText::new(format!("{CURRENT_VERSION} · MIT licence"))
                        .size(12.5)
                        .color(palette.text_dim),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::button(ui, &palette, "GitHub", Kind::Quiet).clicked() {
                        open_external(REPO_URL);
                    }
                });
            });
            if !state.script.version.is_empty() && state.script.version != CURRENT_VERSION {
                ui.label(
                    RichText::new(format!(
                        "KWin is still running script version {}; it updates on the next start of the service.",
                        state.script.version
                    ))
                    .size(12.5)
                    .color(palette.warn),
                );
            }
        });
    }
}
