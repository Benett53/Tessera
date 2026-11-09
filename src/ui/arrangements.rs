//! Arrangements: every window's place, saved under a name.

use egui::{Align2, CornerRadius, FontId, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};

use super::widgets::{self, Kind};
use super::{TesseraApp, ago};
use crate::daemon::QUICK_ID;
use crate::model::{Request, Snapshot};

#[derive(Default)]
pub struct ArrangementsState {
    name: String,
    renaming: Option<(String, String)>,
    confirm_delete: Option<String>,
}

impl TesseraApp {
    pub(super) fn arrangements_page(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let Some(state) = self.state.clone() else {
            return;
        };
        widgets::page_title(
            ui,
            &palette,
            "Arrangements",
            "Save where every window is, and put them all back with one click.",
        );

        widgets::card(ui, &palette, |ui| {
            ui.horizontal(|ui| {
                let a = &mut self.arrangements;
                if a.name.is_empty() {
                    a.name = format!(
                        "Arrangement {}",
                        state.snapshots.iter().filter(|s| s.id != QUICK_ID).count() + 1
                    );
                }
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut a.name)
                        .desired_width(240.0)
                        .hint_text("Name"),
                );
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if widgets::button(ui, &palette, "Save current arrangement", Kind::Primary)
                    .clicked()
                    || enter
                {
                    let name = a.name.trim().to_string();
                    a.name.clear();
                    self.send(
                        Request::SaveSnapshot {
                            name,
                            replace: None,
                        },
                        Some("Saved {n} windows"),
                    );
                }
            });
            ui.add_space(4.0);
            let mut launch = state.config.settings.launch_missing;
            widgets::setting_row(
                ui,
                &palette,
                "Start programs that are not running",
                "Off: their windows are placed when you open them within two minutes",
                |ui| {
                    if widgets::toggle(ui, &palette, &mut launch).changed() {
                        self.update_settings(|s| s.launch_missing = launch);
                    }
                },
            );
        });
        ui.add_space(10.0);

        let mut snapshots = state.snapshots.clone();
        snapshots.sort_by(|a, b| {
            (b.id == QUICK_ID)
                .cmp(&(a.id == QUICK_ID))
                .then(b.created.cmp(&a.created))
        });
        if snapshots.is_empty() {
            widgets::card(ui, &palette, |ui| {
                widgets::empty_state(
                    ui,
                    &palette,
                    "No arrangements yet",
                    "Save one above, or press Meta+Ctrl+Alt+S anywhere for the quick arrangement.",
                );
            });
            return;
        }

        let width = ui.available_width();
        let columns = if width > 760.0 { 2 } else { 1 };
        let card_width = (width - 12.0 * (columns as f32 - 1.0)) / columns as f32;
        for chunk in snapshots.chunks(columns) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for snapshot in chunk {
                    ui.vertical(|ui| {
                        ui.set_width(card_width);
                        self.snapshot_card(ui, snapshot);
                    });
                }
            });
            ui.add_space(12.0);
        }
    }

    fn snapshot_card(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        let palette = self.palette;
        widgets::card(ui, &palette, |ui| {
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 150.0), Sense::hover());
            self.preview(ui, rect, snapshot);
            ui.add_space(6.0);
            let renaming = self
                .arrangements
                .renaming
                .clone()
                .filter(|(id, _)| id == &snapshot.id);
            match renaming {
                Some((id, mut text)) => {
                    let mut done = false;
                    let mut cancel = false;
                    ui.horizontal(|ui| {
                        let edit =
                            ui.add(egui::TextEdit::singleline(&mut text).desired_width(200.0));
                        edit.request_focus();
                        if ui.input(|i| i.key_pressed(egui::Key::Enter))
                            || widgets::button(ui, &palette, "OK", Kind::Primary).clicked()
                        {
                            done = true;
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            cancel = true;
                        }
                    });
                    if cancel {
                        self.arrangements.renaming = None;
                    } else if done {
                        self.arrangements.renaming = None;
                        let name = text.trim().to_string();
                        if !name.is_empty() {
                            self.send(Request::RenameSnapshot { id, name }, None);
                        }
                    } else {
                        self.arrangements.renaming = Some((id, text));
                    }
                }
                None => {
                    ui.label(RichText::new(&snapshot.name).size(15.0));
                }
            }
            let mut detail = format!(
                "{} window{} · {}",
                snapshot.windows.len(),
                if snapshot.windows.len() == 1 { "" } else { "s" },
                ago(snapshot.created)
            );
            if snapshot.id == QUICK_ID {
                detail.push_str(" · Meta+Ctrl+Alt+S / R");
            }
            ui.label(RichText::new(detail).size(12.5).color(palette.text_dim));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if widgets::button(ui, &palette, "Restore", Kind::Primary).clicked() {
                    self.send(
                        Request::RestoreSnapshot {
                            id: snapshot.id.clone(),
                        },
                        None,
                    );
                }
                if widgets::button(ui, &palette, "Update", Kind::Normal)
                    .on_hover_text("Replace with where the windows are now")
                    .clicked()
                {
                    self.send(
                        Request::SaveSnapshot {
                            name: String::new(),
                            replace: Some(snapshot.id.clone()),
                        },
                        Some("Updated with {n} windows"),
                    );
                }
                if widgets::button(ui, &palette, "Rename", Kind::Quiet).clicked() {
                    self.arrangements.renaming = Some((snapshot.id.clone(), snapshot.name.clone()));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let a = &mut self.arrangements;
                    if a.confirm_delete.as_deref() == Some(snapshot.id.as_str()) {
                        if widgets::button(ui, &palette, "Delete", Kind::Danger).clicked() {
                            a.confirm_delete = None;
                            self.send(
                                Request::DeleteSnapshot {
                                    id: snapshot.id.clone(),
                                },
                                Some("Deleted"),
                            );
                        }
                    } else if widgets::icon_button(ui, &palette, widgets::Icon::Trash, "Delete")
                        .clicked()
                    {
                        a.confirm_delete = Some(snapshot.id.clone());
                    }
                });
            });
        });
    }

    /// Screens as outlines with each window as a labelled box.
    fn preview(&mut self, ui: &mut egui::Ui, area: Rect, snapshot: &Snapshot) {
        let palette = self.palette;
        let screens = if snapshot.screens.is_empty() {
            self.state
                .as_ref()
                .map(|s| s.topology.screens.clone())
                .unwrap_or_default()
        } else {
            snapshot.screens.clone()
        };
        if screens.is_empty() {
            return;
        }
        let min_x = screens.iter().map(|s| s.x).min().unwrap_or(0) as f32;
        let min_y = screens.iter().map(|s| s.y).min().unwrap_or(0) as f32;
        let max_x = screens.iter().map(|s| s.x + s.w).max().unwrap_or(1) as f32;
        let max_y = screens.iter().map(|s| s.y + s.h).max().unwrap_or(1) as f32;
        let total = vec2(max_x - min_x, max_y - min_y);
        let scale = (area.width() / total.x).min(area.height() / total.y);
        let offset = area.center() - total * scale / 2.0;
        let map = |x: f32, y: f32| {
            pos2(
                offset.x + (x - min_x) * scale,
                offset.y + (y - min_y) * scale,
            )
        };
        let painter = ui.painter().clone();
        for screen in &screens {
            let rect = Rect::from_min_max(
                map(screen.x as f32, screen.y as f32),
                map((screen.x + screen.w) as f32, (screen.y + screen.h) as f32),
            );
            widgets::monitor_frame(&painter, rect, &palette, false);
        }
        let clip = Rect::from_min_max(map(min_x, min_y), map(max_x, max_y));
        let painter = painter.with_clip_rect(clip);
        // Windows stacked in the same spot (a zone, maximised) share one box.
        let mut boxes: Vec<(Rect, Vec<String>)> = Vec::new();
        for window in snapshot.windows.iter().filter(|w| !w.placement.min) {
            let p = &window.placement;
            let (x, y, w, h) = match screens.iter().find(|s| s.name == p.output) {
                Some(s) if p.full || p.max == 3 => (s.x, s.y, s.w, s.h),
                _ => (p.x, p.y, p.w, p.h),
            };
            let rect =
                Rect::from_min_max(map(x as f32, y as f32), map((x + w) as f32, (y + h) as f32))
                    .shrink(1.5);
            let name = self
                .apps
                .get(ui.ctx(), &window.class, &window.desktop_file)
                .name;
            match boxes.iter_mut().find(|(r, _)| {
                (r.min - rect.min).length() < 4.0 && (r.max - rect.max).length() < 4.0
            }) {
                Some((_, names)) => {
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
                None => boxes.push((rect, vec![name])),
            }
        }
        for (rect, names) in boxes {
            painter.rect(
                rect,
                CornerRadius::same(3),
                palette.accent.gamma_multiply(0.14),
                Stroke::new(1.0, palette.accent.gamma_multiply(0.6)),
                StrokeKind::Inside,
            );
            if rect.width() > 40.0 && rect.height() > 16.0 {
                let label = match names.len() {
                    1 | 2 => names.join(", "),
                    n => format!("{}, {} +{}", names[0], names[1], n - 2),
                };
                painter.with_clip_rect(rect.shrink(2.0)).text(
                    rect.left_top() + vec2(5.0, 4.0),
                    Align2::LEFT_TOP,
                    label,
                    FontId::proportional(11.0),
                    palette.text,
                );
            }
        }
    }
}
