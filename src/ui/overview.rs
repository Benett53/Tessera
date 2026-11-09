//! Overview: status, the main switches and quick actions.

use egui::{RichText, Sense, vec2};

use super::widgets::{self, Kind};
use super::{Page, TesseraApp};
use crate::model::Request;

pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Meta+Ctrl+Alt+1…9", "Window to zone 1–9"),
    ("Meta+Ctrl+Alt+Right", "Next zone"),
    ("Meta+Ctrl+Alt+Left", "Previous zone"),
    ("Meta+Ctrl+Alt+0", "Window out of its zone"),
    ("Meta+Ctrl+Alt+S", "Save quick arrangement"),
    ("Meta+Ctrl+Alt+R", "Restore quick arrangement"),
    ("Meta+Ctrl+Alt+T", "Open Tessera"),
    ("Shift while dragging", "Drop a window into a zone"),
];

impl TesseraApp {
    pub(super) fn overview_page(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let Some(state) = self.state.clone() else {
            return;
        };
        widgets::page_title(
            ui,
            &palette,
            "Overview",
            "Zones for your screens, and windows that open where you left them.",
        );

        // Problems first: they explain everything else on the page.
        let script = &state.script;
        if !self.demo && (!script.connected || !state.conflicts.is_empty()) {
            widgets::card(ui, &palette, |ui| {
                if !script.connected {
                    ui.horizontal(|ui| {
                        widgets::status_dot(ui, palette.warn);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("The KWin script is not connected").size(14.5));
                            let detail = if !script.installed {
                                "It is not installed yet."
                            } else if !script.enabled {
                                "It is turned off in System Settings > Window Management > KWin Scripts."
                            } else {
                                "KWin has not loaded it. Repairing usually helps; otherwise log out and back in."
                            };
                            ui.label(RichText::new(detail).size(12.5).color(palette.text_dim));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::button(ui, &palette, "Repair", Kind::Primary).clicked() {
                                self.send(Request::RepairScript, Some("KWin script reloaded"));
                            }
                        });
                    });
                }
                for conflict in &state.conflicts {
                    ui.horizontal(|ui| {
                        widgets::status_dot(ui, palette.warn);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(format!("{} is turned on", conflict.name)).size(14.5),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "This KWin script {}. Both at once move windows twice.",
                                    conflict.reason
                                ))
                                .size(12.5)
                                .color(palette.text_dim),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::button(ui, &palette, "Turn it off", Kind::Normal).clicked()
                            {
                                self.send(
                                    Request::DisableConflict {
                                        plugin: conflict.plugin.clone(),
                                    },
                                    Some("Turned off"),
                                );
                            }
                        });
                    });
                }
            });
            ui.add_space(8.0);
        }

        let settings = state.config.settings.clone();
        let total_width = ui.available_width();
        let column = ((total_width - 12.0) / 2.0).max(280.0);

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.vertical(|ui| {
                ui.set_width(column);
                // Zones card with a live preview of the first screen.
                widgets::card(ui, &palette, |ui| {
                    ui.label(RichText::new("Zones").size(16.0));
                    let screen = state.topology.screens.first().cloned();
                    let zone = screen.as_ref().and_then(|s| {
                        state
                            .zones
                            .iter()
                            .find(|z| {
                                z.output == s.name && z.desktop == state.topology.current_desktop
                            })
                            .or_else(|| state.zones.iter().find(|z| z.output == s.name))
                    });
                    let summary = match (&screen, zone) {
                        (Some(s), Some(z)) => {
                            format!("{} zones on {}", z.tree.zone_count(), s.name)
                        }
                        _ => "Waiting for KWin".into(),
                    };
                    ui.label(RichText::new(summary).size(12.5).color(palette.text_dim));
                    ui.add_space(6.0);
                    let aspect = screen
                        .as_ref()
                        .map(|s| s.w as f32 / s.h.max(1) as f32)
                        .unwrap_or(16.0 / 9.0);
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 130.0), Sense::click());
                    let monitor = widgets::fit(rect, aspect);
                    widgets::monitor_frame(ui.painter(), monitor, &palette, response.hovered());
                    if let Some(z) = zone {
                        widgets::layout_preview(
                            ui.painter(),
                            monitor.shrink(4.0),
                            &z.tree,
                            &palette,
                            None,
                            true,
                        );
                    }
                    if response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        self.page = Page::Zones;
                    }
                    ui.add_space(6.0);
                    if widgets::button(ui, &palette, "Edit zones", Kind::Normal).clicked() {
                        self.page = Page::Zones;
                    }
                });
                ui.add_space(12.0);
                widgets::card(ui, &palette, |ui| {
                    ui.label(RichText::new("Arrangements").size(16.0));
                    let count = state.snapshots.len();
                    ui.label(
                        RichText::new(if count == 0 {
                            "Save where every window is now and bring it back later.".to_string()
                        } else {
                            format!("{count} saved")
                        })
                        .size(12.5)
                        .color(palette.text_dim),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if widgets::button(ui, &palette, "Save current", Kind::Primary).clicked() {
                            self.send(
                                Request::SaveSnapshot {
                                    name: format!("Arrangement {}", state.snapshots.len() + 1),
                                    replace: None,
                                },
                                Some("Saved {n} windows"),
                            );
                        }
                        let newest = state.snapshots.iter().max_by_key(|s| s.created).cloned();
                        if let Some(newest) = newest
                            && widgets::button(
                                ui,
                                &palette,
                                &format!("Restore “{}”", newest.name),
                                Kind::Normal,
                            )
                            .clicked()
                        {
                            self.send(Request::RestoreSnapshot { id: newest.id }, None);
                        }
                    });
                });
            });
            ui.vertical(|ui| {
                ui.set_width(column);
                widgets::card(ui, &palette, |ui| {
                    ui.label(RichText::new("Remembered windows").size(16.0));
                    let apps = state.apps.len();
                    let open = state.running.len();
                    ui.label(
                        RichText::new(format!("{apps} programs remembered · {open} open now"))
                            .size(12.5)
                            .color(palette.text_dim),
                    );
                    ui.add_space(4.0);
                    let mut remember = settings.remember;
                    widgets::setting_row(
                        ui,
                        &palette,
                        "Restore windows",
                        "Open programs where they were last closed",
                        |ui| {
                            if widgets::toggle(ui, &palette, &mut remember).changed() {
                                self.update_settings(|s| s.remember = remember);
                            }
                        },
                    );
                    let mut paused = state.paused;
                    widgets::setting_row(
                        ui,
                        &palette,
                        "Pause for now",
                        "Until you resume or log out",
                        |ui| {
                            if widgets::toggle(ui, &palette, &mut paused).changed() {
                                self.send(Request::SetPaused { paused }, None);
                            }
                        },
                    );
                    let mut back = settings.zone_after_unmaximize;
                    widgets::setting_row(
                        ui,
                        &palette,
                        "Back into the zone",
                        "A maximised window returns to its zone when restored",
                        |ui| {
                            if widgets::toggle(ui, &palette, &mut back).changed() {
                                self.update_settings(|s| s.zone_after_unmaximize = back);
                            }
                        },
                    );
                    ui.add_space(4.0);
                    if widgets::button(ui, &palette, "Manage windows", Kind::Normal).clicked() {
                        self.page = Page::Windows;
                    }
                });
                ui.add_space(12.0);
                widgets::card(ui, &palette, |ui| {
                    ui.label(RichText::new("Shortcuts").size(16.0));
                    ui.add_space(4.0);
                    for (combo, action) in SHORTCUTS.iter().take(6) {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(*action).size(13.0).color(palette.text_dim));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    widgets::keys(ui, &palette, combo);
                                },
                            );
                        });
                    }
                });
            });
        });
    }
}
