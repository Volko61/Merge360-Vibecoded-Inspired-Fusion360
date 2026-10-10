//! Small reusable widgets: expression fields and command dialog frames.

use eframe::egui::{self, Color32, RichText, Stroke};
use m360_core::Units;

use crate::theme;

/// Text field for a length with unit expressions ("25", "1 in", "10mm+2").
/// Returns true when the value changed.
pub fn length_field(ui: &mut egui::Ui, id_salt: impl std::hash::Hash + std::fmt::Debug, value_mm: &mut f64, units: Units) -> bool {
    expr_field(ui, id_salt, value_mm, |v| units.format(v), |s| units.parse(s))
}

/// Text field for an angle in degrees.
pub fn angle_field(ui: &mut egui::Ui, id_salt: impl std::hash::Hash + std::fmt::Debug, deg: &mut f64) -> bool {
    expr_field(
        ui,
        id_salt,
        deg,
        |v| format!("{v:.1} deg"),
        |s| s.trim().trim_end_matches("deg").trim_end_matches('°').trim().parse().ok(),
    )
}

fn expr_field(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut f64,
    fmt: impl Fn(f64) -> String,
    parse: impl Fn(&str) -> Option<f64>,
) -> bool {
    let id = ui.make_persistent_id(id_salt);
    let mut text: String = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| fmt(*value));
    let has_focus = ui.memory(|m| m.has_focus(id));
    if !has_focus {
        let shown = fmt(*value);
        if parse(&text) != Some(*value) {
            text = shown;
        }
    }
    let ok = parse(&text).is_some();
    let resp = ui.add(
        egui::TextEdit::singleline(&mut text)
            .id(id)
            .desired_width(120.0)
            .text_color(if ok { theme::TEXT } else { theme::ERROR }),
    );
    let mut changed = false;
    if resp.changed() {
        if let Some(v) = parse(&text) {
            if (v - *value).abs() > 1e-12 {
                *value = v;
                changed = true;
            }
        }
    }
    if resp.lost_focus() {
        text = fmt(*value);
    }
    ui.data_mut(|d| d.insert_temp(id, text));
    changed
}

pub struct DialogResult {
    pub ok: bool,
    pub cancel: bool,
}

/// A Fusion-style command dialog anchored at the right of the canvas.
pub fn command_dialog(
    ctx: &egui::Context,
    title: &str,
    anchor: egui::Pos2,
    ok_enabled: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> DialogResult {
    let mut res = DialogResult { ok: false, cancel: false };
    egui::Area::new(egui::Id::new("command_dialog"))
        .order(egui::Order::Foreground)
        .fixed_pos(anchor)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme::PANEL_BG)
                .stroke(Stroke::new(1.0, theme::PANEL_BORDER))
                .corner_radius(3)
                .shadow(ui.visuals().window_shadow)
                .show(ui, |ui| {
                    ui.set_width(300.0);
                    // Header
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(300.0, 26.0), egui::Sense::drag());
                    ui.painter().rect_filled(rect, egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, theme::DIALOG_HEADER);
                    ui.painter().text(
                        rect.left_center() + egui::vec2(10.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        title,
                        egui::FontId::proportional(12.5),
                        Color32::WHITE,
                    );
                    egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                        egui::Grid::new("dialog_grid")
                            .num_columns(2)
                            .spacing([10.0, 6.0])
                            .min_col_width(90.0)
                            .show(ui, add_contents);
                    });
                    ui.separator();
                    egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                            ui.painter().circle_stroke(r.center(), 6.5, Stroke::new(1.2, theme::TEXT_DIM));
                            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "i", egui::FontId::proportional(10.0), theme::TEXT_DIM);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if theme::secondary_button(ui, "Cancel").clicked() {
                                    res.cancel = true;
                                }
                                ui.add_enabled_ui(ok_enabled, |ui| {
                                    if theme::primary_button(ui, "OK").clicked() {
                                        res.ok = true;
                                    }
                                });
                            });
                        });
                    });
                });
        });
    res
}

/// A selection-field button ("Select" / "1 selected") like Fusion's.
pub fn selection_field(ui: &mut egui::Ui, active: bool, text: &str) -> egui::Response {
    let fill = if active { theme::ACCENT } else { Color32::from_rgb(0xee, 0xef, 0xf1) };
    let color = if active { Color32::WHITE } else { theme::TEXT };
    let resp = ui.add(egui::Button::new(RichText::new(format!("     {text}")).color(color)).fill(fill).min_size(egui::vec2(120.0, 20.0)));
    // Small cursor arrow glyph, like Fusion's selection fields.
    let p = resp.rect.left_center() + egui::vec2(9.0, -5.0);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![p, p + egui::vec2(0.0, 9.0), p + egui::vec2(2.5, 6.5), p + egui::vec2(6.0, 6.0)],
        color,
        Stroke::NONE,
    ));
    resp
}

/// Label column of a dialog row.
pub fn row_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::TEXT_DIM));
}
