//! Zones: pick a screen, pick or draw a layout, apply it to KWin.

use std::time::{Duration, Instant};

use egui::{
    Align2, CornerRadius, FontId, Id, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2,
};

use super::TesseraApp;
use super::widgets::{self, Kind};
use crate::layout::{COLUMNS, Divider, ROWS, Template, ZoneNode};
use crate::model::{Request, Screen};

/// Shares a split or divider snaps to, as in most window managers.
const SNAPS: &[f64] = &[0.5, 1.0 / 3.0, 2.0 / 3.0, 0.25, 0.75];

#[derive(Default)]
pub struct ZonesState {
    screen: Option<String>,
    /// Empty: all desktops.
    desktop: String,
    draft: Option<ZoneNode>,
    padding: u32,
    source: Option<(String, String)>,
    dirty: bool,
    undo: Vec<(ZoneNode, u32)>,
    template: Option<(Template, usize)>,
    naming: bool,
    layout_name: String,
    applied: Option<Instant>,
}

fn snap(value: f64, tolerance: f64) -> f64 {
    SNAPS
        .iter()
        .copied()
        .find(|s| (s - value).abs() < tolerance)
        .unwrap_or(value)
}

impl TesseraApp {
    pub(super) fn zones_page(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let Some(state) = self.state.clone() else {
            return;
        };
        widgets::page_title(
            ui,
            &palette,
            "Zones",
            "Split a screen into zones. Hold Shift while dragging a window to drop it into one.",
        );
        let screens = state.topology.screens.clone();
        if screens.is_empty() {
            widgets::card(ui, &palette, |ui| {
                widgets::empty_state(
                    ui,
                    &palette,
                    "No screens yet",
                    "Tessera is waiting for KWin to report the screens.",
                );
            });
            return;
        }

        let z = &mut self.zones;
        if z.screen
            .as_ref()
            .is_none_or(|s| !screens.iter().any(|x| &x.name == s))
        {
            z.screen = Some(screens[0].name.clone());
        }
        if !z.desktop.is_empty() && !state.topology.desktops.iter().any(|d| d.id == z.desktop) {
            z.desktop.clear();
        }
        let screen_name = z.screen.clone().unwrap_or_default();
        let screen = screens
            .iter()
            .find(|s| s.name == screen_name)
            .cloned()
            .unwrap_or_default();
        let shown_desktop = if z.desktop.is_empty() {
            state.topology.current_desktop.clone()
        } else {
            z.desktop.clone()
        };
        let current = state
            .zones
            .iter()
            .find(|zs| zs.output == screen_name && zs.desktop == shown_desktop)
            .or_else(|| state.zones.iter().find(|zs| zs.output == screen_name))
            .cloned();

        // Load what KWin has, unless the user is in the middle of an edit.
        let key = (screen_name.clone(), z.desktop.clone());
        let settling = z
            .applied
            .is_some_and(|t| t.elapsed() < Duration::from_millis(1500));
        let differs = match (&z.draft, &current) {
            (Some(draft), Some(cur)) => !draft.same_as(&cur.tree) || z.padding != cur.padding,
            (None, _) => true,
            _ => false,
        };
        if z.source.as_ref() != Some(&key) || (!z.dirty && differs && !settling) {
            z.draft = Some(current.as_ref().map(|c| c.tree.clone()).unwrap_or_default());
            z.padding = current.as_ref().map(|c| c.padding).unwrap_or(0);
            z.source = Some(key);
            z.dirty = false;
            z.undo.clear();
            z.template = None;
        }

        // Screen and desktop pickers.
        if screens.len() > 1 {
            screen_picker(ui, &palette, &screens, &mut z.screen);
            ui.add_space(6.0);
        }
        if state.topology.desktops.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Desktop").size(13.0).color(palette.text_dim));
                if widgets::chip(ui, &palette, "All desktops", z.desktop.is_empty()).clicked() {
                    z.desktop.clear();
                }
                for d in &state.topology.desktops {
                    if widgets::chip(ui, &palette, &d.name, z.desktop == d.id).clicked() {
                        z.desktop = d.id.clone();
                    }
                }
            });
            if z.desktop.is_empty() {
                let trees: Vec<_> = state
                    .zones
                    .iter()
                    .filter(|zs| zs.output == screen_name)
                    .collect();
                if trees.windows(2).any(|w| !w[0].tree.same_as(&w[1].tree)) {
                    ui.label(
                        RichText::new("The desktops on this screen have different zones; applying makes them all the same.")
                            .size(12.5)
                            .color(palette.warn),
                    );
                }
            }
            ui.add_space(6.0);
        }

        let side_width = 270.0;
        let mut apply = false;
        let mut save_layout = None;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            let canvas_width = (ui.available_width() - side_width - 16.0).max(320.0);
            ui.vertical(|ui| {
                ui.set_width(canvas_width);
                widgets::card(ui, &palette, |ui| {
                    editor(ui, &palette, &mut self.zones, &screen);
                    let z = &mut self.zones;
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Click a zone to split it into columns, Shift+click for rows. Drag a line to resize. Right-click for more.")
                            .size(12.0)
                            .color(palette.text_dim),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if widgets::button_enabled(ui, &palette, "Undo", Kind::Normal, !z.undo.is_empty()).clicked() {
                            undo(z);
                        }
                        if widgets::button_enabled(ui, &palette, "Revert", Kind::Normal, z.dirty).clicked() {
                            z.source = None;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let label = if z.desktop.is_empty() && state.topology.desktops.len() > 1 {
                                "Apply to all desktops"
                            } else {
                                "Apply"
                            };
                            if widgets::button_enabled(ui, &palette, label, Kind::Primary, z.dirty).clicked() {
                                apply = true;
                            }
                            if widgets::button(ui, &palette, "Save as layout", Kind::Normal).clicked() {
                                z.naming = !z.naming;
                                if z.layout_name.is_empty() {
                                    z.layout_name = format!("Layout {}", state.config.layouts.len() + 1);
                                }
                            }
                        });
                    });
                    if z.naming {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let edit = ui.add(egui::TextEdit::singleline(&mut z.layout_name).desired_width(220.0).hint_text("Name"));
                            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if (widgets::button(ui, &palette, "Save", Kind::Primary).clicked() || enter)
                                && !z.layout_name.trim().is_empty()
                                && let Some(tree) = z.draft.clone()
                            {
                                let name = z.layout_name.trim().to_string();
                                z.naming = false;
                                z.layout_name.clear();
                                save_layout = Some(Request::SaveLayout { name, tree });
                            }
                        });
                    }
                });
            });
            ui.vertical(|ui| {
                ui.set_width(side_width);
                self.zones_side(ui, &state.config.layouts);
            });
        });

        if let Some(request) = save_layout {
            self.send(request, Some("Layout saved"));
        }
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
            undo(&mut self.zones);
        }

        if apply && let Some(tree) = self.zones.draft.clone() {
            let z = &mut self.zones;
            z.dirty = false;
            z.applied = Some(Instant::now());
            let request = Request::ApplyZones {
                output: screen_name,
                desktop: z.desktop.clone(),
                tree,
                padding: z.padding,
            };
            self.send(request, Some("Zones applied"));
        }
    }

    fn zones_side(&mut self, ui: &mut egui::Ui, layouts: &[crate::model::SavedLayout]) {
        let palette = self.palette;
        widgets::card(ui, &palette, |ui| {
            ui.label(RichText::new("Templates").size(14.5));
            ui.add_space(4.0);
            let z = &mut self.zones;
            egui::Grid::new("templates")
                .spacing(vec2(10.0, 10.0))
                .show(ui, |ui| {
                    for (i, template) in Template::ALL.iter().enumerate() {
                        let active = z.template.is_some_and(|(t, _)| t == *template);
                        let count = z
                            .template
                            .filter(|(t, _)| t == template)
                            .map(|(_, c)| c)
                            .unwrap_or(template.default_count());
                        if thumbnail(
                            ui,
                            &palette,
                            &template.build(count),
                            template.name(),
                            active,
                        )
                        .clicked()
                        {
                            push_undo(z);
                            z.draft = Some(template.build(count));
                            z.template = Some((*template, count));
                            z.dirty = true;
                        }
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
            if let Some((template, count)) = z.template {
                let (lo, hi) = template.count_range();
                if lo != hi {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Zones").size(13.0).color(palette.text_dim));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let mut next = count;
                            if widgets::button_enabled(ui, &palette, "+", Kind::Normal, count < hi)
                                .clicked()
                            {
                                next += 1;
                            }
                            ui.label(RichText::new(count.to_string()).size(15.0));
                            if widgets::button_enabled(ui, &palette, "−", Kind::Normal, count > lo)
                                .clicked()
                            {
                                next -= 1;
                            }
                            if next != count {
                                push_undo(z);
                                z.draft = Some(template.build(next));
                                z.template = Some((template, next));
                                z.dirty = true;
                            }
                        });
                    });
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Space between zones")
                        .size(13.0)
                        .color(palette.text_dim),
                );
            });
            let mut padding = z.padding;
            if ui
                .add(
                    egui::Slider::new(&mut padding, 0..=32)
                        .suffix(" px")
                        .trailing_fill(true),
                )
                .changed()
            {
                z.padding = padding;
                z.dirty = true;
            }
        });

        ui.add_space(10.0);
        widgets::card(ui, &palette, |ui| {
            ui.label(RichText::new("My layouts").size(14.5));
            if layouts.is_empty() {
                ui.label(
                    RichText::new("Layouts you save appear here, ready for any screen.")
                        .size(12.5)
                        .color(palette.text_dim),
                );
            }
            let mut delete = None;
            for layout in layouts {
                ui.horizontal(|ui| {
                    let (rect, response) = ui.allocate_exact_size(vec2(64.0, 36.0), Sense::click());
                    widgets::monitor_frame(ui.painter(), rect, &palette, response.hovered());
                    widgets::layout_preview(
                        ui.painter(),
                        rect.shrink(2.0),
                        &layout.tree,
                        &palette,
                        None,
                        false,
                    );
                    let name = ui.add(
                        egui::Label::new(RichText::new(&layout.name).size(13.5))
                            .sense(Sense::click()),
                    );
                    if response.clicked() || name.clicked() {
                        let z = &mut self.zones;
                        push_undo(z);
                        z.draft = Some(layout.tree.clone());
                        z.template = None;
                        z.dirty = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon_button(ui, &palette, widgets::Icon::Trash, "Delete layout")
                            .clicked()
                        {
                            delete = Some(layout.id.clone());
                        }
                    });
                });
            }
            if let Some(id) = delete {
                self.send(Request::DeleteLayout { id }, Some("Layout deleted"));
            }
        });
    }
}

fn push_undo(z: &mut ZonesState) {
    if let Some(draft) = &z.draft {
        z.undo.push((draft.clone(), z.padding));
        if z.undo.len() > 50 {
            z.undo.remove(0);
        }
    }
}

fn undo(z: &mut ZonesState) {
    if let Some((tree, padding)) = z.undo.pop() {
        z.draft = Some(tree);
        z.padding = padding;
        z.dirty = true;
        z.template = None;
    }
}

fn thumbnail(
    ui: &mut egui::Ui,
    palette: &super::theme::Palette,
    tree: &ZoneNode,
    name: &str,
    active: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(112.0, 78.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if active {
            palette.accent.gamma_multiply(0.14)
        } else if response.hovered() {
            palette.surface_hover
        } else {
            egui::Color32::TRANSPARENT
        };
        painter.rect(
            rect,
            CornerRadius::same(8),
            fill,
            Stroke::new(
                1.0,
                if active {
                    palette.accent
                } else {
                    palette.border
                },
            ),
            StrokeKind::Inside,
        );
        let preview =
            Rect::from_min_size(rect.min + vec2(14.0, 8.0), vec2(rect.width() - 28.0, 44.0));
        widgets::layout_preview(painter, preview, tree, palette, None, false);
        painter.text(
            pos2(rect.center().x, rect.bottom() - 12.0),
            Align2::CENTER_CENTER,
            name,
            FontId::proportional(12.0),
            if active {
                palette.accent
            } else {
                palette.text_dim
            },
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn screen_picker(
    ui: &mut egui::Ui,
    palette: &super::theme::Palette,
    screens: &[Screen],
    selected: &mut Option<String>,
) {
    let min_x = screens.iter().map(|s| s.x).min().unwrap_or(0) as f32;
    let min_y = screens.iter().map(|s| s.y).min().unwrap_or(0) as f32;
    let max_x = screens.iter().map(|s| s.x + s.w).max().unwrap_or(1) as f32;
    let max_y = screens.iter().map(|s| s.y + s.h).max().unwrap_or(1) as f32;
    let (area, _) =
        ui.allocate_exact_size(vec2(ui.available_width().min(520.0), 96.0), Sense::hover());
    let scale = (area.width() / (max_x - min_x)).min(area.height() / (max_y - min_y));
    for screen in screens {
        let rect = Rect::from_min_size(
            pos2(
                area.left() + (screen.x as f32 - min_x) * scale,
                area.top() + (screen.y as f32 - min_y) * scale,
            ),
            vec2(screen.w as f32 * scale, screen.h as f32 * scale),
        )
        .shrink(3.0);
        let response = ui.interact(rect, Id::new(("screen", &screen.name)), Sense::click());
        let active = selected.as_deref() == Some(screen.name.as_str());
        widgets::monitor_frame(ui.painter(), rect, palette, active || response.hovered());
        ui.painter().text(
            rect.center() - vec2(0.0, 7.0),
            Align2::CENTER_CENTER,
            &screen.name,
            FontId::proportional(13.0),
            if active { palette.accent } else { palette.text },
        );
        ui.painter().text(
            rect.center() + vec2(0.0, 9.0),
            Align2::CENTER_CENTER,
            format!("{} × {}", screen.w, screen.h),
            FontId::proportional(11.0),
            palette.text_dim,
        );
        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            *selected = Some(screen.name.clone());
        }
    }
}

/// The big interactive preview.
fn editor(ui: &mut egui::Ui, palette: &super::theme::Palette, z: &mut ZonesState, screen: &Screen) {
    let aspect = if screen.h > 0 {
        screen.w as f32 / screen.h as f32
    } else {
        16.0 / 9.0
    };
    let width = ui.available_width();
    let height = (width / aspect).clamp(220.0, 470.0);
    let (outer, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let monitor = widgets::fit(outer, aspect);
    widgets::monitor_frame(ui.painter(), monitor, palette, false);
    let inner = monitor.shrink(6.0);
    let Some(draft) = z.draft.clone() else {
        return;
    };
    let as_f64 = [
        inner.left() as f64,
        inner.top() as f64,
        inner.width() as f64,
        inner.height() as f64,
    ];
    let px_per_unit = (
        screen.w.max(1) as f32 / inner.width(),
        screen.h.max(1) as f32 / inner.height(),
    );
    let gap = (z.padding as f32 / px_per_unit.0).clamp(1.5, 10.0);
    let shift = ui.input(|i| i.modifiers.shift);
    let pointer = ui.input(|i| i.pointer.hover_pos());

    // Dividers take priority over the zones under them.
    let dividers: Vec<Divider> = draft.dividers(as_f64);
    let mut divider_hovered = false;
    let mut changed: Option<ZoneNode> = None;
    for divider in &dividers {
        let line = widgets::rect_from(&divider.rect);
        let hit = if divider.dir == COLUMNS {
            line.expand2(vec2(6.0, 0.0))
        } else {
            line.expand2(vec2(0.0, 6.0))
        };
        let id = Id::new(("divider", &divider.path, divider.index));
        let response = ui.interact(hit, id, Sense::drag());
        let cursor = if divider.dir == COLUMNS {
            egui::CursorIcon::ResizeHorizontal
        } else {
            egui::CursorIcon::ResizeVertical
        };
        if response.hovered() || response.dragged() {
            divider_hovered = true;
            ui.ctx().set_cursor_icon(cursor);
        }
        if response.drag_started() {
            push_undo(z);
        }
        if response.dragged()
            && let Some(p) = response.interact_pointer_pos()
        {
            let raw = if divider.dir == COLUMNS { p.x } else { p.y } as f64;
            let (start, length) = if divider.dir == COLUMNS {
                (as_f64[0], as_f64[2])
            } else {
                (as_f64[1], as_f64[3])
            };
            let screen_share = snap((raw - start) / length, 6.0 / length);
            let mut next = changed.clone().unwrap_or_else(|| draft.clone());
            next.move_divider(divider, start + screen_share * length);
            changed = Some(next);
        }
    }

    let zones = draft.zones(as_f64);
    let count = zones.len();
    let painter = ui.painter().clone();
    for (i, (path, r)) in zones.iter().enumerate() {
        let rect = widgets::rect_from(r).shrink(gap / 2.0);
        let response = ui.interact(rect, Id::new(("zone", path)), Sense::click());
        let hovered = response.hovered() && !divider_hovered;
        painter.rect(
            rect,
            CornerRadius::same(6),
            if hovered {
                palette.accent.gamma_multiply(0.26)
            } else {
                palette.accent.gamma_multiply(0.11)
            },
            Stroke::new(
                1.0,
                palette
                    .accent
                    .gamma_multiply(if hovered { 0.95 } else { 0.5 }),
            ),
            StrokeKind::Inside,
        );
        painter.text(
            rect.center() - vec2(0.0, 8.0),
            Align2::CENTER_CENTER,
            (i + 1).to_string(),
            FontId::proportional((rect.height() * 0.22).clamp(13.0, 34.0)),
            palette.text,
        );
        let px = (
            (r[2] as f32 * px_per_unit.0).round() as i32,
            (r[3] as f32 * px_per_unit.1).round() as i32,
        );
        if rect.height() > 70.0 && rect.width() > 70.0 {
            painter.text(
                rect.center() + vec2(0.0, (rect.height() * 0.11).clamp(12.0, 22.0)),
                Align2::CENTER_CENTER,
                format!("{} × {}", px.0, px.1),
                FontId::proportional(11.5),
                palette.text_dim,
            );
        }
        let dir = if shift { ROWS } else { COLUMNS };
        let at = |p: egui::Pos2| {
            let raw = if dir == COLUMNS {
                (p.x - rect.left()) / rect.width()
            } else {
                (p.y - rect.top()) / rect.height()
            };
            snap(raw as f64, 0.03)
        };
        if hovered && let Some(p) = pointer {
            let share = at(p) as f32;
            let stroke = Stroke::new(1.5, palette.accent);
            if dir == COLUMNS {
                let x = rect.left() + share * rect.width();
                painter.line_segment(
                    [pos2(x, rect.top() + 4.0), pos2(x, rect.bottom() - 4.0)],
                    stroke,
                );
            } else {
                let y = rect.top() + share * rect.height();
                painter.line_segment(
                    [pos2(rect.left() + 4.0, y), pos2(rect.right() - 4.0, y)],
                    stroke,
                );
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        if response.clicked()
            && !divider_hovered
            && let Some(p) = response.interact_pointer_pos()
        {
            let mut next = draft.clone();
            next.split(path, dir, at(p));
            changed = Some(next);
            push_undo(z);
        }
        response.context_menu(|ui| {
            if ui.button("Split into columns").clicked() {
                push_undo(z);
                let mut next = draft.clone();
                next.split(path, COLUMNS, 0.5);
                z.draft = Some(next);
                z.dirty = true;
                z.template = None;
                ui.close();
            }
            if ui.button("Split into rows").clicked() {
                push_undo(z);
                let mut next = draft.clone();
                next.split(path, ROWS, 0.5);
                z.draft = Some(next);
                z.dirty = true;
                z.template = None;
                ui.close();
            }
            if ui
                .add_enabled(count > 1, egui::Button::new("Remove zone"))
                .clicked()
            {
                push_undo(z);
                let mut next = draft.clone();
                next.remove(path);
                z.draft = Some(next);
                z.dirty = true;
                z.template = None;
                ui.close();
            }
        });
    }

    // Dividers drawn last, above the zones.
    for divider in &dividers {
        let line = widgets::rect_from(&divider.rect);
        let hovered = pointer.is_some_and(|p| {
            if divider.dir == COLUMNS {
                line.expand2(vec2(6.0, 0.0)).contains(p)
            } else {
                line.expand2(vec2(0.0, 6.0)).contains(p)
            }
        });
        if hovered {
            let handle = if divider.dir == COLUMNS {
                Rect::from_center_size(line.center(), vec2(4.0, 28.0_f32.min(line.height())))
            } else {
                Rect::from_center_size(line.center(), vec2(28.0_f32.min(line.width()), 4.0))
            };
            painter.rect_filled(handle, CornerRadius::same(2), palette.accent);
        }
    }

    if let Some(next) = changed {
        z.draft = Some(next);
        z.dirty = true;
        z.template = None;
    }
}
