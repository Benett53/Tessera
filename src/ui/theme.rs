//! Colours and egui styling.
//!
//! True black by default; everything above the background is built from low
//! alpha whites, so contrast steps stay the same whatever the accent.

use egui::{Color32, CornerRadius, Stroke, Visuals};

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub background: Color32,
    pub sidebar: Color32,
    pub surface: Color32,
    pub surface_hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub danger: Color32,
    pub ok: Color32,
    pub warn: Color32,
}

impl Palette {
    pub fn new(oled_black: bool, accent: Color32) -> Self {
        let background = if oled_black {
            Color32::BLACK
        } else {
            Color32::from_rgb(0x13, 0x13, 0x16)
        };
        Self {
            background,
            sidebar: if oled_black {
                Color32::from_rgb(0x07, 0x07, 0x09)
            } else {
                Color32::from_rgb(0x0E, 0x0E, 0x11)
            },
            surface: Color32::from_rgba_unmultiplied(255, 255, 255, 9),
            surface_hover: Color32::from_rgba_unmultiplied(255, 255, 255, 20),
            border: Color32::from_rgb(0x24, 0x24, 0x2A),
            text: Color32::from_rgb(0xEC, 0xEC, 0xF0),
            text_dim: Color32::from_rgb(0x8E, 0x8E, 0x9A),
            text_faint: Color32::from_rgb(0x5A, 0x5A, 0x66),
            accent,
            danger: Color32::from_rgb(0xFF, 0x6B, 0x6B),
            ok: Color32::from_rgb(0x4A, 0xD6, 0x8C),
            warn: Color32::from_rgb(0xFF, 0xB4, 0x54),
        }
    }
}

pub const CARD_CORNER: CornerRadius = CornerRadius::same(10);
pub const ROW_CORNER: CornerRadius = CornerRadius::same(7);

pub const DEFAULT_ACCENT: Color32 = Color32::from_rgb(0x7A, 0xA2, 0xF7);

pub fn parse_accent(text: &str) -> Color32 {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() == 6
        && let Ok(value) = u32::from_str_radix(hex, 16)
    {
        return Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8);
    }
    DEFAULT_ACCENT
}

#[cfg(test)]
pub fn accent_to_hex(color: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b())
}

pub const ACCENT_CHOICES: &[(&str, &str)] = &[
    ("Blue", "#7AA2F7"),
    ("Ice", "#4CC2FF"),
    ("Mint", "#3DDC97"),
    ("Amber", "#FFB454"),
    ("Magenta", "#FF6FB5"),
    ("Violet", "#A88BFF"),
    ("Grey", "#9AA3AD"),
];

pub fn apply(ctx: &egui::Context, palette: &Palette) {
    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(palette.text);
    visuals.panel_fill = palette.background;
    visuals.window_fill = Color32::from_rgb(0x15, 0x15, 0x19);
    visuals.extreme_bg_color = Color32::from_rgb(0x0C, 0x0C, 0x0F);
    visuals.faint_bg_color = palette.surface;
    visuals.window_stroke = Stroke::new(1.0, palette.border);
    visuals.window_corner_radius = CARD_CORNER;
    visuals.menu_corner_radius = CARD_CORNER;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.selection.bg_fill = palette.accent.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, palette.accent);
    visuals.hyperlink_color = palette.accent;

    visuals.widgets.noninteractive.bg_fill = palette.background;
    visuals.widgets.noninteractive.weak_bg_fill = palette.background;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text_dim);

    visuals.widgets.inactive.bg_fill = palette.surface_hover;
    visuals.widgets.inactive.weak_bg_fill = palette.surface;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, palette.text);
    visuals.widgets.inactive.corner_radius = ROW_CORNER;

    visuals.widgets.hovered.bg_fill = palette.surface_hover;
    visuals.widgets.hovered.weak_bg_fill = palette.surface_hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, palette.text);
    visuals.widgets.hovered.corner_radius = ROW_CORNER;

    visuals.widgets.active.bg_fill = palette.accent.gamma_multiply(0.30);
    visuals.widgets.active.weak_bg_fill = palette.accent.gamma_multiply(0.30);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, palette.text);
    visuals.widgets.active.corner_radius = ROW_CORNER;

    visuals.widgets.open = visuals.widgets.active;

    ctx.all_styles_mut(|style| {
        style.visuals = visuals.clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 28.0;
        style.spacing.scroll.bar_width = 7.0;
        style.spacing.scroll.floating = true;
        style.animation_time = 0.08;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_round_trip() {
        for (_, hex) in ACCENT_CHOICES {
            assert_eq!(&accent_to_hex(parse_accent(hex)), hex);
        }
        assert_eq!(parse_accent("nonsense"), DEFAULT_ACCENT);
    }
}
