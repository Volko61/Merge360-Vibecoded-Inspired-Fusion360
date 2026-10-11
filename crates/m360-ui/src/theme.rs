//! Fusion-like light theme.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(0x06, 0x96, 0xd7);
pub const ACCENT_DARK: Color32 = Color32::from_rgb(0x04, 0x6f, 0xa3);
pub const APP_BAR: Color32 = Color32::from_rgb(0x2e, 0x2f, 0x33);
pub const APP_BAR_HOVER: Color32 = Color32::from_rgb(0x46, 0x48, 0x4d);
pub const APP_BAR_TEXT: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);
pub const TOOLBAR_BG: Color32 = Color32::from_rgb(0xf4, 0xf4, 0xf5);
pub const TOOLBAR_TABS_BG: Color32 = Color32::from_rgb(0xe6, 0xe7, 0xe9);
pub const PANEL_BG: Color32 = Color32::from_rgb(0xfb, 0xfb, 0xfb);
pub const PANEL_BORDER: Color32 = Color32::from_rgb(0xc9, 0xcb, 0xce);
pub const TEXT: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x6b, 0x6e, 0x73);
pub const HOVER_BG: Color32 = Color32::from_rgb(0xdd, 0xee, 0xf8);
pub const SELECT_BG: Color32 = Color32::from_rgb(0xc4, 0xe2, 0xf5);
pub const DIALOG_HEADER: Color32 = Color32::from_rgb(0x3a, 0x3c, 0x40);
pub const SKETCH_GREEN: Color32 = Color32::from_rgb(0x2f, 0xa8, 0x4f);
pub const ERROR: Color32 = Color32::from_rgb(0xd6, 0x3b, 0x30);
pub const SUCCESS: Color32 = Color32::from_rgb(40, 150, 70);
pub const WARNING: Color32 = Color32::from_rgb(0xf2, 0xb1, 0x27);
pub const TIMELINE_BG: Color32 = Color32::from_rgb(0xef, 0xf0, 0xf1);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();
    style.text_styles = [
        (TextStyle::Small, FontId::new(10.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(12.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(12.5, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(6.0, 3.0);
    style.spacing.interact_size.y = 20.0;
    style.spacing.menu_margin = egui::Margin::same(4);

    let mut v = Visuals::light();
    v.override_text_color = Some(TEXT);
    v.panel_fill = PANEL_BG;
    v.window_fill = PANEL_BG;
    v.window_stroke = Stroke::new(1.0, PANEL_BORDER);
    v.window_corner_radius = CornerRadius::same(3);
    v.menu_corner_radius = CornerRadius::same(3);
    v.selection.bg_fill = SELECT_BG;
    v.selection.stroke = Stroke::new(1.0, ACCENT_DARK);
    v.hyperlink_color = ACCENT;
    v.extreme_bg_color = Color32::WHITE;
    v.faint_bg_color = Color32::from_rgb(0xf3, 0xf4, 0xf6);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, PANEL_BORDER);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0xee, 0xef, 0xf1);
    v.widgets.inactive.bg_fill = Color32::from_rgb(0xee, 0xef, 0xf1);
    v.widgets.inactive.corner_radius = CornerRadius::same(2);
    v.widgets.hovered.weak_bg_fill = HOVER_BG;
    v.widgets.hovered.bg_fill = HOVER_BG;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.hovered.corner_radius = CornerRadius::same(2);
    v.widgets.active.weak_bg_fill = SELECT_BG;
    v.widgets.active.bg_fill = SELECT_BG;
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT_DARK);
    v.widgets.active.corner_radius = CornerRadius::same(2);
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(40) };
    v.window_shadow = egui::epaint::Shadow { offset: [0, 3], blur: 12, spread: 0, color: Color32::from_black_alpha(50) };
    style.visuals = v;
    ctx.set_global_style(style);
}

/// The primary blue button (Fusion's "OK").
pub fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).color(Color32::WHITE))
            .fill(ACCENT)
            .min_size(egui::vec2(64.0, 22.0)),
    )
}

pub fn secondary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(egui::vec2(64.0, 22.0)))
}
