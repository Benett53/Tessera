//! Windows: remembered programs, what Tessera does with each.

use egui::{Id, Rect, RichText, Sense, vec2};

use super::TesseraApp;
use super::widgets::{self, Icon, Kind};
use crate::model::{AppMode, AppRecord, Placement, Request, StateView, TileRef, ZonePin};

#[derive(Default)]
pub struct WindowsState {
    search: String,
    expanded: Option<String>,
    confirm_forget_all: bool,
}

/// 1-based zone number of a remembered tile, if the layout still has it.
pub fn zone_number(state: &StateView, tile: &TileRef) -> Option<usize> {
    let zones = state
        .zones
        .iter()
        .find(|z| z.output == tile.output && z.desktop == tile.desktop)
        .or_else(|| state.zones.iter().find(|z| z.output == tile.output))?;
    zones
        .tree
        .zones([0.0, 0.0, 1.0, 1.0])
        .iter()
        .position(|(_, r)| r.iter().zip(&tile.rect).all(|(a, b)| (a - b).abs() < 0.02))
        .map(|i| i + 1)
}

fn describe(state: &StateView, p: &Placement) -> String {
    let mut parts = vec![
        if p.user_size {
            format!("{} × {}", p.w, p.h)
        } else {
            "its own size".into()
        },
        if p.user_pos {
            format!("at {}, {}", p.x, p.y)
        } else {
            "placed by KWin".into()
        },
    ];
    if !p.output.is_empty() && state.topology.screens.len() > 1 {
        parts.push(p.output.clone());
    }
    if state.topology.desktops.len() > 1 {
        if p.all_desktops {
            parts.push("all desktops".into());
        } else if let Some(name) = p
            .desktops
            .first()
            .and_then(|id| state.topology.desktops.iter().find(|d| &d.id == id))
            .map(|d| d.name.clone())
        {
            parts.push(name);
        }
    }
    if let Some(tile) = &p.tile {
        parts.push(match zone_number(state, tile) {
            Some(n) => format!("zone {n}"),
            None => "a zone".into(),
        });
    } else if p.full {
        parts.push("full screen".into());
    } else if p.max == 3 {
        parts.push("maximised".into());
    }
    if p.min {
        parts.push("minimised".into());
    }
    if p.above {
        parts.push("kept above".into());
    }
    parts.join(" · ")
}

fn mode_label(mode: AppMode) -> &'static str {
    match mode {
        AppMode::Remember => "Remember position",
        AppMode::Zone => "Always in a zone",
        AppMode::Ignore => "Leave alone",
    }
}

impl TesseraApp {
    pub(super) fn windows_page(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let Some(state) = self.state.clone() else {
            return;
        };
        widgets::page_title(
            ui,
            &palette,
            "Windows",
            "Programs Tessera has seen. Each opens where its window was last closed.",
        );

        widgets::card(ui, &palette, |ui| {
            let mut remember = state.config.settings.remember;
            widgets::setting_row(
                ui,
                &palette,
                "Restore windows when they open",
                "Position, size, screen, desktop and zone",
                |ui| {
                    if widgets::toggle(ui, &palette, &mut remember).changed() {
                        self.update_settings(|s| s.remember = remember);
                    }
                },
            );
            let mut remember_new = state.config.settings.remember_new;
            widgets::setting_row(
                ui,
                &palette,
                "Remember new programs",
                "Off: only programs already in this list",
                |ui| {
                    if widgets::toggle(ui, &palette, &mut remember_new).changed() {
                        self.update_settings(|s| s.remember_new = remember_new);
                    }
                },
            );
        });
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.windows.search)
                    .hint_text("Search programs")
                    .desired_width(260.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !state.apps.is_empty() {
                    if self.windows.confirm_forget_all {
                        if widgets::button(ui, &palette, "Cancel", Kind::Quiet).clicked() {
                            self.windows.confirm_forget_all = false;
                        }
                        if widgets::button(ui, &palette, "Forget all, really", Kind::Danger)
                            .clicked()
                        {
                            self.windows.confirm_forget_all = false;
                            self.send(Request::ForgetAll, Some("All windows forgotten"));
                        }
                    } else if widgets::button(ui, &palette, "Forget all", Kind::Quiet).clicked() {
                        self.windows.confirm_forget_all = true;
                    }
                }
            });
        });
        ui.add_space(6.0);

        let needle = self.windows.search.trim().to_lowercase();
        let mut rows: Vec<(String, AppRecord, String, bool)> = state
            .apps
            .iter()
            .map(|(class, record)| {
                let info = self.apps.get(ui.ctx(), class, &record.desktop_file);
                (
                    class.clone(),
                    record.clone(),
                    info.name,
                    state.running.contains(class),
                )
            })
            .filter(|(class, _, name, _)| {
                needle.is_empty()
                    || class.contains(&needle)
                    || name.to_lowercase().contains(&needle)
            })
            .collect();
        rows.sort_by(|a, b| {
            b.3.cmp(&a.3)
                .then(a.2.to_lowercase().cmp(&b.2.to_lowercase()))
        });

        if rows.is_empty() {
            widgets::card(ui, &palette, |ui| {
                if state.apps.is_empty() {
                    widgets::empty_state(
                        ui,
                        &palette,
                        "Nothing remembered yet",
                        "Move a window and close it; next time it opens in the same place.",
                    );
                } else {
                    widgets::empty_state(
                        ui,
                        &palette,
                        "No match",
                        "No remembered program matches the search.",
                    );
                }
            });
            return;
        }

        widgets::card(ui, &palette, |ui| {
            for (index, (class, record, name, running)) in rows.iter().enumerate() {
                if index > 0 {
                    widgets::separator(ui, &palette);
                }
                self.app_row(ui, &state, class, record, name, *running);
            }
        });
    }

    fn app_row(
        &mut self,
        ui: &mut egui::Ui,
        state: &StateView,
        class: &str,
        record: &AppRecord,
        name: &str,
        running: bool,
    ) {
        let palette = self.palette;
        let expanded = self.windows.expanded.as_deref() == Some(class);
        let info = self.apps.get(ui.ctx(), class, &record.desktop_file);
        let row = ui.horizontal(|ui| {
            ui.set_min_height(40.0);
            let (icon_rect, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
            match &info.icon {
                Some(texture) => {
                    egui::Image::new((texture.id(), vec2(28.0, 28.0))).paint_at(ui, icon_rect);
                }
                None => {
                    ui.painter()
                        .rect_filled(icon_rect, 6.0, palette.surface_hover);
                    ui.painter().text(
                        icon_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        name.chars()
                            .next()
                            .unwrap_or('?')
                            .to_uppercase()
                            .to_string(),
                        egui::FontId::proportional(14.0),
                        palette.text_dim,
                    );
                }
            }
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(name).size(14.5));
                    if running {
                        widgets::status_dot(ui, palette.ok);
                        ui.label(RichText::new("open").size(11.5).color(palette.text_dim));
                    }
                });
                let detail = match record.mode {
                    AppMode::Remember => {
                        let n = record.slots.len();
                        format!("{class} · {n} window{}", if n == 1 { "" } else { "s" })
                    }
                    AppMode::Zone => match &record.pin {
                        Some(pin) if state.topology.screens.len() > 1 => {
                            format!("{class} · zone {} on {}", pin.index, pin.output)
                        }
                        Some(pin) => format!("{class} · zone {}", pin.index),
                        None => format!("{class} · no zone chosen"),
                    },
                    AppMode::Ignore => format!("{class} · left alone"),
                };
                ui.label(RichText::new(detail).size(12.0).color(palette.text_dim));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_button(ui, &palette, Icon::Trash, "Forget this program").clicked()
                {
                    self.send(
                        Request::ForgetApp {
                            class: class.to_string(),
                        },
                        Some("Forgotten"),
                    );
                }
                if widgets::icon_button(ui, &palette, Icon::Chevron(expanded), "Details").clicked()
                {
                    self.windows.expanded = if expanded {
                        None
                    } else {
                        Some(class.to_string())
                    };
                }
                let mut mode = record.mode;
                egui::ComboBox::from_id_salt(("mode", class))
                    .width(170.0)
                    .selected_text(mode_label(mode))
                    .show_ui(ui, |ui| {
                        for option in [AppMode::Remember, AppMode::Zone, AppMode::Ignore] {
                            ui.selectable_value(&mut mode, option, mode_label(option));
                        }
                    });
                if mode != record.mode {
                    let pin = record.pin.clone().or_else(|| {
                        (mode == AppMode::Zone).then(|| ZonePin {
                            output: state
                                .topology
                                .screens
                                .first()
                                .map(|s| s.name.clone())
                                .unwrap_or_default(),
                            index: 1,
                        })
                    });
                    if mode == AppMode::Zone {
                        self.windows.expanded = Some(class.to_string());
                    }
                    self.send(
                        Request::SetApp {
                            class: class.to_string(),
                            mode,
                            pin,
                        },
                        None,
                    );
                }
            });
        });
        let _ = row;

        if !expanded {
            return;
        }
        ui.indent(Id::new(("details", class)), |ui| {
            ui.add_space(2.0);
            match record.mode {
                AppMode::Remember => {
                    if record.slots.is_empty() {
                        ui.label(RichText::new("Nothing set by hand yet. Move or resize the window yourself and Tessera keeps that; otherwise the program keeps its own size.").size(12.5).color(palette.text_dim));
                    }
                    for (i, slot) in record.slots.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("Window {}", i + 1)).size(13.0));
                            ui.label(RichText::new(describe(state, &slot.placement)).size(12.5).color(palette.text_dim));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if widgets::button(ui, &palette, "Forget", Kind::Quiet).clicked() {
                                    self.send(
                                        Request::ForgetSlot {
                                            class: class.to_string(),
                                            index: i,
                                        },
                                        None,
                                    );
                                }
                            });
                        });
                    }
                }
                AppMode::Zone => self.zone_pin_picker(ui, state, class, record),
                AppMode::Ignore => {
                    ui.label(
                        RichText::new("Tessera leaves this program's windows where they open and does not save them.")
                            .size(12.5)
                            .color(palette.text_dim),
                    );
                }
            }
            ui.add_space(6.0);
        });
    }

    fn zone_pin_picker(
        &mut self,
        ui: &mut egui::Ui,
        state: &StateView,
        class: &str,
        record: &AppRecord,
    ) {
        let palette = self.palette;
        let pin = record.pin.clone().unwrap_or(ZonePin {
            output: state
                .topology
                .screens
                .first()
                .map(|s| s.name.clone())
                .unwrap_or_default(),
            index: 1,
        });
        ui.label(
            RichText::new("Click the zone its windows should open in.")
                .size(12.5)
                .color(palette.text_dim),
        );
        let mut chosen: Option<ZonePin> = None;
        ui.horizontal_wrapped(|ui| {
            for screen in &state.topology.screens {
                let zones = state
                    .zones
                    .iter()
                    .find(|z| {
                        z.output == screen.name && z.desktop == state.topology.current_desktop
                    })
                    .or_else(|| state.zones.iter().find(|z| z.output == screen.name));
                let Some(zones) = zones else {
                    continue;
                };
                ui.vertical(|ui| {
                    if state.topology.screens.len() > 1 {
                        ui.label(
                            RichText::new(&screen.name)
                                .size(12.0)
                                .color(palette.text_dim),
                        );
                    }
                    let aspect = screen.w as f32 / screen.h.max(1) as f32;
                    let (rect, _) = ui
                        .allocate_exact_size(vec2(150.0 * aspect.min(2.4), 150.0), Sense::hover());
                    widgets::monitor_frame(ui.painter(), rect, &palette, false);
                    let inner = rect.shrink(3.0);
                    let active = (pin.output == screen.name).then(|| pin.index as usize - 1);
                    let painter = ui.painter().clone();
                    widgets::layout_preview(&painter, inner, &zones.tree, &palette, active, true);
                    for (i, (_, r)) in zones
                        .tree
                        .zones([
                            inner.left() as f64,
                            inner.top() as f64,
                            inner.width() as f64,
                            inner.height() as f64,
                        ])
                        .iter()
                        .enumerate()
                    {
                        let zone: Rect = widgets::rect_from(r);
                        let response = ui.interact(
                            zone,
                            Id::new(("pin", class, &screen.name, i)),
                            Sense::click(),
                        );
                        if response
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            chosen = Some(ZonePin {
                                output: screen.name.clone(),
                                index: i as u32 + 1,
                            });
                        }
                    }
                });
            }
        });
        if let Some(pin) = chosen {
            self.send(
                Request::SetApp {
                    class: class.to_string(),
                    mode: AppMode::Zone,
                    pin: Some(pin),
                },
                None,
            );
        }
    }
}
