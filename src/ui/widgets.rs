//! Small custom widgets and vector icons.

use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Layout, Painter, Rect, Response, RichText, Sense,
    Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};

use super::theme::{CARD_CORNER, Palette, ROW_CORNER};
use crate::layout::ZoneNode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Overview,
    Zones,
    Windows,
    Arrangements,
    Settings,
    Trash,
    Chevron(bool),
}

/// Line icons drawn into `rect`.
pub fn draw_icon(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.5;
    let stroke = Stroke::new(1.5, color);
    let r = |x0: f32, y0: f32, x1: f32, y1: f32| {
        Rect::from_min_max(
            pos2(c.x + x0 * s, c.y + y0 * s),
            pos2(c.x + x1 * s, c.y + y1 * s),
        )
    };
    match icon {
        Icon::Overview => {
            for (x, y) in [(-0.8, -0.8), (0.1, -0.8), (-0.8, 0.1), (0.1, 0.1)] {
                painter.rect_stroke(r(x, y, x + 0.7, y + 0.7), 2.0, stroke, StrokeKind::Middle);
            }
        }
        Icon::Zones => {
            painter.rect_stroke(r(-0.85, -0.7, 0.85, 0.7), 2.5, stroke, StrokeKind::Middle);
            painter.line_segment(
                [
                    pos2(c.x - 0.15 * s, c.y - 0.7 * s),
                    pos2(c.x - 0.15 * s, c.y + 0.7 * s),
                ],
                stroke,
            );
            painter.line_segment(
                [pos2(c.x - 0.15 * s, c.y), pos2(c.x + 0.85 * s, c.y)],
                stroke,
            );
        }
        Icon::Windows => {
            painter.rect_stroke(r(-0.85, -0.7, 0.85, 0.7), 2.5, stroke, StrokeKind::Middle);
            painter.line_segment(
                [
                    pos2(c.x - 0.85 * s, c.y - 0.35 * s),
                    pos2(c.x + 0.85 * s, c.y - 0.35 * s),
                ],
                stroke,
            );
            painter.circle_filled(pos2(c.x + 0.6 * s, c.y - 0.53 * s), 1.2, color);
        }
        Icon::Arrangements => {
            painter.rect_stroke(r(-0.85, -0.85, 0.35, 0.15), 2.0, stroke, StrokeKind::Middle);
            painter.rect_stroke(r(-0.35, -0.35, 0.85, 0.85), 2.0, stroke, StrokeKind::Middle);
        }
        Icon::Settings => {
            painter.circle_stroke(c, 0.32 * s, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (sin, cos) = a.sin_cos();
                painter.line_segment(
                    [
                        pos2(c.x + cos * 0.55 * s, c.y + sin * 0.55 * s),
                        pos2(c.x + cos * 0.85 * s, c.y + sin * 0.85 * s),
                    ],
                    Stroke::new(2.0, color),
                );
            }
            painter.circle_stroke(c, 0.62 * s, stroke);
        }
        Icon::Trash => {
            painter.line_segment(
                [
                    pos2(c.x - 0.7 * s, c.y - 0.55 * s),
                    pos2(c.x + 0.7 * s, c.y - 0.55 * s),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    pos2(c.x - 0.25 * s, c.y - 0.8 * s),
                    pos2(c.x + 0.25 * s, c.y - 0.8 * s),
                ],
                stroke,
            );
            painter.rect_stroke(r(-0.5, -0.55, 0.5, 0.85), 1.5, stroke, StrokeKind::Middle);
        }
        Icon::Chevron(open) => {
            let pts = if open {
                [
                    pos2(c.x - 0.45 * s, c.y - 0.2 * s),
                    pos2(c.x, c.y + 0.25 * s),
                    pos2(c.x + 0.45 * s, c.y - 0.2 * s),
                ]
            } else {
                [
                    pos2(c.x - 0.2 * s, c.y - 0.45 * s),
                    pos2(c.x + 0.25 * s, c.y),
                    pos2(c.x - 0.2 * s, c.y + 0.45 * s),
                ]
            };
            painter.line_segment([pts[0], pts[1]], stroke);
            painter.line_segment([pts[1], pts[2]], stroke);
        }
    }
}

pub fn nav_item(ui: &mut Ui, palette: &Palette, icon: Icon, label: &str, active: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if active {
            painter.rect_filled(rect, ROW_CORNER, palette.accent.gamma_multiply(0.14));
            painter.rect_filled(
                Rect::from_min_size(pos2(rect.left(), rect.top() + 9.0), vec2(3.0, 16.0)),
                CornerRadius::same(2),
                palette.accent,
            );
        } else if response.hovered() {
            painter.rect_filled(rect, ROW_CORNER, palette.surface_hover);
        }
        let color = if active {
            palette.text
        } else {
            palette.text_dim
        };
        draw_icon(
            painter,
            Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), Vec2::splat(16.0)),
            icon,
            if active { palette.accent } else { color },
        );
        painter.text(
            pos2(rect.left() + 40.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(14.0),
            color,
        );
    }
    response
}

/// An on/off switch.
pub fn toggle(ui: &mut Ui, palette: &Palette, on: &mut bool) -> Response {
    let size = vec2(38.0, 21.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        let painter = ui.painter();
        let radius = rect.height() / 2.0;
        let fill = if *on {
            palette.accent
        } else if response.hovered() {
            palette.surface_hover
        } else {
            palette.surface
        };
        painter.rect(
            rect,
            radius,
            fill,
            Stroke::new(
                1.0,
                if *on {
                    palette.accent
                } else {
                    palette.text_faint
                },
            ),
            StrokeKind::Inside,
        );
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
        let knob = if *on {
            palette.background
        } else {
            palette.text_dim
        };
        painter.circle_filled(pos2(x, rect.center().y), radius - 4.0, knob);
    }
    response
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Normal,
    Primary,
    Danger,
    Quiet,
}

pub fn button(ui: &mut Ui, palette: &Palette, label: &str, kind: Kind) -> Response {
    button_enabled(ui, palette, label, kind, true)
}

pub fn button_enabled(
    ui: &mut Ui,
    palette: &Palette,
    label: &str,
    kind: Kind,
    enabled: bool,
) -> Response {
    let font = FontId::proportional(13.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), palette.text);
    let size = vec2(galley.size().x + 26.0, 30.0);
    let (rect, response) = ui.allocate_exact_size(
        size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let hovered = enabled && response.hovered();
        let (fill, text, stroke) = match kind {
            Kind::Primary => (
                if hovered {
                    palette.accent
                } else {
                    palette.accent.gamma_multiply(0.88)
                },
                palette.background,
                Stroke::NONE,
            ),
            Kind::Danger => (
                if hovered {
                    palette.danger.gamma_multiply(0.25)
                } else {
                    palette.danger.gamma_multiply(0.12)
                },
                palette.danger,
                Stroke::new(1.0, palette.danger.gamma_multiply(0.5)),
            ),
            Kind::Normal => (
                if hovered {
                    palette.surface_hover
                } else {
                    palette.surface
                },
                palette.text,
                Stroke::new(1.0, palette.border),
            ),
            Kind::Quiet => (
                if hovered {
                    palette.surface_hover
                } else {
                    Color32::TRANSPARENT
                },
                palette.text_dim,
                Stroke::NONE,
            ),
        };
        let alpha = if enabled { 1.0 } else { 0.4 };
        painter.rect(
            rect,
            ROW_CORNER,
            fill.gamma_multiply(alpha),
            stroke,
            StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            font,
            text.gamma_multiply(alpha),
        );
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub fn icon_button(ui: &mut Ui, palette: &Palette, icon: Icon, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, ROW_CORNER, palette.surface_hover);
        }
        let color = if response.hovered() {
            if icon == Icon::Trash {
                palette.danger
            } else {
                palette.text
            }
        } else {
            palette.text_dim
        };
        draw_icon(ui.painter(), rect.shrink(7.0), icon, color);
    }
    response.on_hover_text(tooltip)
}

pub fn chip(ui: &mut Ui, palette: &Palette, label: &str, active: bool) -> Response {
    let font = FontId::proportional(13.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), palette.text);
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 20.0, 26.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if active {
            palette.accent.gamma_multiply(0.20)
        } else if response.hovered() {
            palette.surface_hover
        } else {
            palette.surface
        };
        let stroke = if active {
            Stroke::new(1.0, palette.accent.gamma_multiply(0.8))
        } else {
            Stroke::new(1.0, palette.border)
        };
        painter.rect(
            rect,
            CornerRadius::same(13),
            fill,
            stroke,
            StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            font,
            if active { palette.accent } else { palette.text },
        );
    }
    response
}

pub fn colour_swatch(ui: &mut Ui, palette: &Palette, colour: Color32, active: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.circle_filled(rect.center(), 9.0, colour);
        if active || response.hovered() {
            painter.circle_stroke(
                rect.center(),
                12.0,
                Stroke::new(1.5, if active { colour } else { palette.text_dim }),
            );
        }
    }
    response
}

pub fn page_title(ui: &mut Ui, palette: &Palette, title: &str, subtitle: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).size(24.0).color(palette.text));
    if !subtitle.is_empty() {
        ui.label(RichText::new(subtitle).size(13.5).color(palette.text_dim));
    }
    ui.add_space(10.0);
}

pub fn section(ui: &mut Ui, palette: &Palette, title: &str) {
    ui.add_space(10.0);
    ui.label(
        RichText::new(title.to_uppercase())
            .size(11.5)
            .color(palette.text_faint)
            .strong(),
    );
    ui.add_space(2.0);
}

/// A rounded panel around `add`.
pub fn card<R>(ui: &mut Ui, palette: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::NONE
        .fill(palette.surface)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CARD_CORNER)
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A row inside a card: title and description on the left, a control on the right.
pub fn setting_row(
    ui: &mut Ui,
    palette: &Palette,
    title: &str,
    description: &str,
    control: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        let control_width = 200.0;
        let text_width = (ui.available_width() - control_width).max(160.0);
        ui.vertical(|ui| {
            ui.set_width(text_width);
            ui.label(RichText::new(title).size(14.0).color(palette.text));
            if !description.is_empty() {
                ui.label(
                    RichText::new(description)
                        .size(12.5)
                        .color(palette.text_dim),
                );
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), control);
    });
}

pub fn separator(ui: &mut Ui, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, palette.border),
    );
}

pub fn status_dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// A key combination drawn as key caps.
pub fn keys(ui: &mut Ui, palette: &Palette, combo: &str) {
    // Inside a right-aligned row widgets are placed right to left, so the
    // keys go in reversed to read in the right order.
    let mut parts: Vec<&str> = combo.split('+').collect();
    if ui.layout().prefer_right_to_left() {
        parts.reverse();
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (i, key) in parts.into_iter().enumerate() {
            if i > 0 {
                ui.label(RichText::new("+").size(11.0).color(palette.text_faint));
            }
            let font = FontId::monospace(11.5);
            let galley = ui
                .painter()
                .layout_no_wrap(key.to_string(), font.clone(), palette.text);
            let (rect, _) =
                ui.allocate_exact_size(vec2(galley.size().x + 12.0, 22.0), Sense::hover());
            ui.painter().rect(
                rect,
                CornerRadius::same(5),
                palette.surface_hover,
                Stroke::new(1.0, palette.border),
                StrokeKind::Inside,
            );
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                key,
                font,
                palette.text,
            );
        }
    });
}

/// Zones of `tree` drawn into `rect`, numbered.
pub fn layout_preview(
    painter: &Painter,
    rect: Rect,
    tree: &ZoneNode,
    palette: &Palette,
    highlight: Option<usize>,
    numbers: bool,
) {
    let gap = (rect.width() / 80.0).clamp(1.0, 3.0);
    for (i, (_, r)) in tree
        .zones([
            rect.left() as f64,
            rect.top() as f64,
            rect.width() as f64,
            rect.height() as f64,
        ])
        .iter()
        .enumerate()
    {
        let zone = Rect::from_min_size(
            pos2(r[0] as f32, r[1] as f32),
            vec2(r[2] as f32, r[3] as f32),
        )
        .shrink(gap);
        let active = highlight == Some(i);
        painter.rect(
            zone,
            CornerRadius::same(3),
            if active {
                palette.accent.gamma_multiply(0.45)
            } else {
                palette.accent.gamma_multiply(0.12)
            },
            Stroke::new(
                1.0,
                palette
                    .accent
                    .gamma_multiply(if active { 1.0 } else { 0.55 }),
            ),
            StrokeKind::Inside,
        );
        if numbers && zone.height() > 14.0 {
            painter.text(
                zone.center(),
                Align2::CENTER_CENTER,
                (i + 1).to_string(),
                FontId::proportional((zone.height() * 0.4).clamp(9.0, 14.0)),
                palette.text.gamma_multiply(0.85),
            );
        }
    }
}

/// Monitor outline with a stand, for screen pickers and previews.
pub fn monitor_frame(painter: &Painter, rect: Rect, palette: &Palette, active: bool) {
    painter.rect(
        rect,
        CornerRadius::same(4),
        Color32::from_rgb(0x08, 0x08, 0x0B),
        Stroke::new(
            if active { 1.5 } else { 1.0 },
            if active {
                palette.accent
            } else {
                palette.border
            },
        ),
        StrokeKind::Outside,
    );
}

pub fn rect_from(r: &[f64; 4]) -> Rect {
    Rect::from_min_size(
        pos2(r[0] as f32, r[1] as f32),
        vec2(r[2] as f32, r[3] as f32),
    )
}

pub fn fit(outer: Rect, aspect: f32) -> Rect {
    let mut size = outer.size();
    if size.x / size.y > aspect {
        size.x = size.y * aspect;
    } else {
        size.y = size.x / aspect;
    }
    Rect::from_center_size(outer.center(), size)
}

pub fn empty_state(ui: &mut Ui, palette: &Palette, title: &str, text: &str) {
    ui.add_space(30.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(title).size(16.0).color(palette.text));
        ui.add_space(2.0);
        ui.label(RichText::new(text).size(13.0).color(palette.text_dim));
    });
    ui.add_space(30.0);
}
