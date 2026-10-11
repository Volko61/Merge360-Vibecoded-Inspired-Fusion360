//! The design-history TIMELINE at the bottom of the window.

use eframe::egui::{self, Color32, RichText, Sense};

use crate::app::{Action, Merge360App};
use crate::browser::feature_icon;
use crate::icons::{self, Icon};
use crate::state::SelItem;
use crate::theme;
use crate::i18n::tr;

const ITEM_W: f32 = 30.0;
const ITEM_GAP: f32 = 3.0;

impl Merge360App {
    pub(crate) fn timeline(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("timeline")
            .exact_size(44.0)
            .frame(egui::Frame::new().fill(theme::TIMELINE_BG).stroke(egui::Stroke::new(1.0, theme::PANEL_BORDER)).inner_margin(egui::Margin::symmetric(8, 6)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let n = self.tabs[self.active].doc.features.len();
                    let marker = self.tabs[self.active].doc.marker;
                    let mut new_marker = None;
                    if icons::icon_button(ui, Icon::PlayStart, 24.0, "Move to Beginning").clicked() {
                        new_marker = Some(0);
                    }
                    if icons::icon_button(ui, Icon::PlayBack, 24.0, "Step Back").clicked() {
                        new_marker = Some(marker.saturating_sub(1));
                    }
                    if icons::icon_button(ui, Icon::Play, 24.0, "Step Forward").clicked() {
                        new_marker = Some((marker + 1).min(n));
                    }
                    if icons::icon_button(ui, Icon::PlayEnd, 24.0, "Move to End").clicked() {
                        new_marker = Some(n);
                    }
                    ui.add_space(10.0);
                    let width = ui.available_width() - 34.0;
                    let (strip, _) = ui.allocate_exact_size(egui::vec2(width.max(50.0), 32.0), Sense::hover());
                    ui.painter().rect_filled(strip, 3.0, Color32::from_rgb(0xe2, 0xe3, 0xe5));
                    if let Some(m) = self.timeline_strip(ui, strip) {
                        new_marker = Some(m);
                    }
                    let gear = icons::icon_button(ui, Icon::Gear, 24.0, "Timeline settings");
                    egui::Popup::menu(&gear).show(|ui| {
                        if ui.button(tr("Compute All")).clicked() {
                            self.actions.push(Action::Tool("compute_all"));
                        }
                        if ui.button(tr("Move to End")).clicked() {
                            let tab = self.tab();
                            let n = tab.doc.features.len();
                            tab.doc.set_marker(n);
                            tab.regenerate();
                        }
                    });
                    if let Some(m) = new_marker {
                        if m != marker {
                            let tab = self.tab();
                            tab.edit("Move Marker", |d| d.set_marker(m));
                        }
                    }
                });
            });
    }

    fn timeline_strip(&mut self, ui: &mut egui::Ui, strip: egui::Rect) -> Option<usize> {
        let active = self.active;
        let painter = ui.painter_at(strip);
        let n = self.tabs[active].doc.features.len();
        let marker = self.tabs[active].doc.marker;
        let x0 = strip.left() + 6.0;
        let item_x = |i: usize| x0 + i as f32 * (ITEM_W + ITEM_GAP);
        let mut new_marker = None;
        let mut hovered = None;

        for i in 0..n {
            let f = self.tabs[active].doc.features[i].clone();
            let rect = egui::Rect::from_min_size(egui::pos2(item_x(i), strip.top() + 3.0), egui::vec2(ITEM_W, strip.height() - 6.0));
            if rect.right() > strip.right() {
                break;
            }
            let id = ui.id().with(("tl", f.uid));
            let resp = ui.interact(rect, id, Sense::click_and_drag());
            let rolled = i >= marker;
            let err = self.tabs[active].model.errors.get(&f.uid).cloned();
            let sel = self.tabs[active].selection.contains(&SelItem::Feature(f.uid));
            let bg = if err.is_some() {
                Color32::from_rgb(0xf6, 0xd0, 0xcc)
            } else if sel {
                theme::SELECT_BG
            } else if resp.hovered() {
                theme::HOVER_BG
            } else {
                Color32::from_rgb(0xf7, 0xf7, 0xf8)
            };
            painter.rect_filled(rect, 2.0, bg);
            if err.is_some() {
                painter.rect_stroke(rect, 2.0, egui::Stroke::new(1.0, theme::ERROR), egui::StrokeKind::Inside);
            }
            let tint = (rolled || f.suppressed).then_some(Color32::from_gray(175));
            icons::paint(&painter, rect.shrink(4.0), feature_icon(&f.kind), tint);
            if f.suppressed {
                painter.line_segment([rect.left_bottom(), rect.right_top()], egui::Stroke::new(1.5, theme::ERROR));
            }
            if resp.hovered() {
                hovered = Some(f.uid);
            }
            let mut tip = f.name.clone();
            if f.suppressed {
                tip.push_str(" (suppressed)");
            }
            if let Some(e) = &err {
                tip.push_str(&format!("\n⚠ {e}"));
            }
            let resp = resp.on_hover_text(tip);
            if resp.double_clicked() {
                self.actions.push(Action::EditFeature(f.uid));
            } else if resp.clicked() {
                let additive = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                self.click_item(SelItem::Feature(f.uid), additive);
            }
            resp.context_menu(|ui| {
                let is_sketch = matches!(f.kind, m360_core::FeatureKind::Sketch { .. });
                if ui.button(tr(if is_sketch { "Edit Sketch" } else { "Edit Feature" })).clicked() {
                    self.actions.push(Action::EditFeature(f.uid));
                }
                if ui.button(tr("Roll History Marker Here")).clicked() {
                    new_marker = Some(i + 1);
                }
                if ui.button(tr(if f.suppressed { "Unsuppress Features" } else { "Suppress Features" })).clicked() {
                    let uid = f.uid;
                    self.tabs[active].edit("Suppress", |d| {
                        if let Some(f) = d.feature_mut(uid) {
                            f.suppressed = !f.suppressed;
                        }
                    });
                }
                if ui.button(tr("Rename")).clicked() {
                    self.flags.rename = Some((SelItem::Feature(f.uid), f.name.clone()));
                }
                ui.separator();
                if ui.button(RichText::new(tr("Delete")).color(theme::ERROR)).clicked() {
                    let uid = f.uid;
                    self.tabs[active].edit("Delete Feature", |d| d.delete_feature(uid));
                }
            });
        }
        self.tabs[active].timeline_hover = hovered;

        // Marker
        let mx = if marker == 0 { x0 - 3.0 } else { item_x(marker) - ITEM_GAP / 2.0 };
        let handle = egui::Rect::from_center_size(egui::pos2(mx, strip.center().y), egui::vec2(10.0, strip.height()));
        let mresp = ui.interact(handle, ui.id().with("tl_marker"), Sense::drag());
        let pos_x = if mresp.dragged() || mresp.drag_stopped() {
            ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos())).map_or(mx, |p| p.x).clamp(x0 - 3.0, strip.right())
        } else {
            mx
        };
        let col = if mresp.hovered() || mresp.dragged() { theme::ACCENT } else { Color32::from_rgb(0x55, 0x57, 0x5c) };
        painter.line_segment([egui::pos2(pos_x, strip.top() + 1.0), egui::pos2(pos_x, strip.bottom() - 1.0)], egui::Stroke::new(3.0, col));
        painter.add(egui::Shape::convex_polygon(
            vec![egui::pos2(pos_x - 5.0, strip.top()), egui::pos2(pos_x + 5.0, strip.top()), egui::pos2(pos_x, strip.top() + 6.0)],
            col,
            egui::Stroke::NONE,
        ));
        if mresp.drag_stopped() {
            let m = ((pos_x - x0 + ITEM_GAP / 2.0) / (ITEM_W + ITEM_GAP)).round().clamp(0.0, n as f32) as usize;
            new_marker = Some(m);
        }
        if mresp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }

        // Inline rename
        if let Some((SelItem::Feature(uid), text)) = &mut self.flags.rename {
            let uid = *uid;
            if let Some(i) = self.tabs[active].doc.feature_index(uid) {
                let rect = egui::Rect::from_min_size(egui::pos2(item_x(i), strip.top() - 28.0), egui::vec2(160.0, 22.0));
                let mut done = false;
                egui::Area::new(egui::Id::new("tl_rename")).fixed_pos(rect.min).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(150.0));
                    r.request_focus();
                    done = r.lost_focus();
                });
                if done {
                    let name = text.trim().to_string();
                    self.flags.rename = None;
                    if !name.is_empty() {
                        self.tabs[active].edit("Rename Feature", |d| {
                            if let Some(f) = d.feature_mut(uid) {
                                f.name = name;
                            }
                        });
                    }
                }
            }
        }
        new_marker
    }
}
