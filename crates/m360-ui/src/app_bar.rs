//! Application bar (top dark strip) and toolbar ribbon.

use eframe::egui::{self, Color32, RichText, Sense};

use crate::app::{Action, Merge360App, ToastKind};
use crate::files::FileMode;
use crate::icons::{self, Icon};
use crate::theme;
use crate::tools::{self, egui_color::Accent, Tab};
use crate::i18n::tr;

fn bar_icon(ui: &mut egui::Ui, icon: Icon, tooltip: &str, enabled: bool) -> egui::Response {
    let size = egui::vec2(28.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    if resp.hovered() && enabled {
        ui.painter().rect_filled(rect, 3.0, theme::APP_BAR_HOVER);
    }
    let tint = (!enabled).then_some(Color32::from_gray(110));
    icons::paint(ui.painter(), rect.shrink(6.0), icon, tint);
    resp.on_hover_text(tooltip)
}

impl Merge360App {
    pub(crate) fn app_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("app_bar")
            .exact_size(36.0)
            .frame(egui::Frame::new().fill(theme::APP_BAR).inner_margin(egui::Margin::symmetric(6, 4)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if bar_icon(ui, Icon::DataPanel, "Show Data Panel", true).clicked() {
                        self.flags.data_panel = !self.flags.data_panel;
                    }
                    let file = bar_icon(ui, Icon::FileMenu, "File", true);
                    egui::Popup::menu(&file).show(|ui| self.file_menu(ui));
                    if bar_icon(ui, Icon::Save, "Save (Ctrl+S)", true).clicked() {
                        self.actions.push(Action::Save);
                    }
                    ui.add_space(6.0);
                    let tab = &self.tabs[self.active];
                    let (can_undo, can_redo) = (tab.history.can_undo(), tab.history.can_redo());
                    let undo_tip = tab.history.undo_label().map_or("Undo".into(), |l| format!("Undo {l} (Ctrl+Z)"));
                    let redo_tip = tab.history.redo_label().map_or("Redo".into(), |l| format!("Redo {l} (Ctrl+Y)"));
                    if bar_icon(ui, Icon::Undo, &undo_tip, can_undo).clicked() {
                        self.actions.push(Action::Undo);
                    }
                    if bar_icon(ui, Icon::Redo, &redo_tip, can_redo).clicked() {
                        self.actions.push(Action::Redo);
                    }
                    ui.add_space(16.0);

                    // Document tabs
                    for i in 0..self.tabs.len() {
                        self.doc_tab_chip(ui, i);
                    }
                    if bar_icon(ui, Icon::Plus, "New Design (Ctrl+N)", true).clicked() {
                        self.actions.push(Action::NewDesign);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let user = bar_icon(ui, Icon::User, "Local user (offline)", true);
                        egui::Popup::menu(&user).show(|ui| {
                            ui.label(RichText::new(tr("Merge360 — offline mode")).strong());
                            ui.label(tr("No account needed: everything is stored locally."));
                            if ui.button(tr("Preferences…")).clicked() {
                                self.actions.push(Action::Tool("preferences"));
                            }
                        });
                        let help = bar_icon(ui, Icon::Help, "Help", true);
                        egui::Popup::menu(&help).show(|ui| {
                            if ui.button(tr("Open Sample Design")).clicked() {
                                self.open_sample();
                            }
                            if ui.button(tr("Keyboard Shortcuts")).clicked() {
                                self.flags.shortcuts = true;
                            }
                            if ui.button(tr("About Merge360")).clicked() {
                                self.flags.about = true;
                            }
                        });
                        let bell = bar_icon(ui, Icon::Bell, "Notifications", true);
                        egui::Popup::menu(&bell).show(|ui| {
                            ui.set_min_width(220.0);
                            ui.label(RichText::new(tr("Notifications")).strong());
                            ui.label(RichText::new(tr("You're all caught up.")).color(theme::TEXT_DIM));
                        });
                    });
                });
            });
    }

    fn doc_tab_chip(&mut self, ui: &mut egui::Ui, i: usize) {
        let active = i == self.active;
        let title = self.tabs[i].title();
        let font = egui::FontId::proportional(12.5);
        let galley = ui.painter().layout_no_wrap(title, font, theme::APP_BAR_TEXT);
        let w = (galley.size().x + 52.0).clamp(110.0, 240.0);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 28.0), Sense::click());
        let bg = if active {
            Color32::from_rgb(0x55, 0x57, 0x5c)
        } else if resp.hovered() {
            theme::APP_BAR_HOVER
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 3.0, bg);
        icons::paint(ui.painter(), egui::Rect::from_min_size(rect.min + egui::vec2(6.0, 6.0), egui::vec2(16.0, 16.0)), Icon::Component, None);
        ui.painter().galley(rect.min + egui::vec2(28.0, (28.0 - galley.size().y) / 2.0), galley, theme::APP_BAR_TEXT);
        let close = egui::Rect::from_center_size(rect.right_center() - egui::vec2(12.0, 0.0), egui::vec2(16.0, 16.0));
        let close_resp = ui.interact(close, ui.id().with(("close_tab", i)), Sense::click());
        if close_resp.hovered() {
            ui.painter().rect_filled(close, 2.0, Color32::from_gray(110));
        }
        let c = theme::APP_BAR_TEXT;
        let s = close.shrink(4.5);
        ui.painter().line_segment([s.left_top(), s.right_bottom()], (1.2, c));
        ui.painter().line_segment([s.right_top(), s.left_bottom()], (1.2, c));
        if close_resp.clicked() {
            self.actions.push(Action::CloseTab(i));
        } else if resp.clicked() {
            self.active = i;
        }
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(220.0);
        if ui.add(egui::Button::new(tr("New Design")).shortcut_text("Ctrl+N")).clicked() {
            self.actions.push(Action::NewDesign);
        }
        if ui.add(egui::Button::new(tr("Open…")).shortcut_text("Ctrl+O")).clicked() {
            self.actions.push(Action::Open);
        }
        ui.menu_button("Recent Designs", |ui| {
            if self.recent.is_empty() {
                ui.label(RichText::new(tr("No recent designs")).color(theme::TEXT_DIM));
            }
            for p in self.recent.clone() {
                let name = p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
                if ui.button(name).on_hover_text(p.display().to_string()).clicked() {
                    self.actions.push(Action::OpenPath(p));
                }
            }
        });
        ui.separator();
        if ui.add(egui::Button::new(tr("Save")).shortcut_text("Ctrl+S")).clicked() {
            self.actions.push(Action::Save);
        }
        if ui.add(egui::Button::new(tr("Save As…")).shortcut_text("Ctrl+Shift+S")).clicked() {
            self.actions.push(Action::SaveAs);
        }
        ui.menu_button("Export", |ui| {
            if ui.button(tr("STL (*.stl)")).clicked() {
                self.actions.push(Action::Export(FileMode::ExportStl));
            }
            if ui.button(tr("STEP (*.step)")).clicked() {
                self.actions.push(Action::Export(FileMode::ExportStep));
            }
        });
        if ui.button(tr("3D Print")).clicked() {
            self.actions.push(Action::Export(FileMode::ExportStl));
        }
        if ui.button(tr("Capture Image…")).clicked() {
            self.toast(ToastKind::Info, tr("Capture Image is not available yet in Merge360."));
        }
        ui.separator();
        ui.menu_button("View", |ui| {
            ui.checkbox(&mut self.flags.browser_open, tr("Show Browser"));
            ui.checkbox(&mut self.flags.timeline_open, tr("Show Timeline"));
            ui.checkbox(&mut self.flags.comments_open, tr("Show Comments"));
            ui.checkbox(&mut self.flags.data_panel, tr("Show Data Panel"));
        });
        ui.separator();
        if ui.button(tr("Close Design")).clicked() {
            self.actions.push(Action::CloseTab(self.active));
        }
        if ui.button(tr("Exit")).clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    // ------------------------------------------------------------------ toolbar

    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui) {
        let in_sketch = self.tabs[self.active].sketch.is_some();
        egui::Panel::top("toolbar")
            .exact_size(92.0)
            .frame(egui::Frame::new().fill(theme::TOOLBAR_BG).stroke(egui::Stroke::new(1.0, theme::PANEL_BORDER)))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                // Tab strip
                let (strip, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), Sense::hover());
                ui.painter().rect_filled(strip, 0.0, theme::TOOLBAR_TABS_BG);
                let mut strip_ui = ui.new_child(egui::UiBuilder::new().max_rect(strip).layout(egui::Layout::left_to_right(egui::Align::Center)));
                self.tab_strip(&mut strip_ui, in_sketch);

                // Panels
                let tab: &Tab = if in_sketch {
                    &tools::SKETCH_TAB
                } else {
                    &tools::DESIGN_TABS[self.flags.toolbar_tab.min(tools::DESIGN_TABS.len() - 1)]
                };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    egui::ScrollArea::horizontal().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(6.0);
                            if self.flags.workspace == tools::WS_DRAWING {
                                ui.vertical(|ui| {
                                    ui.add_space(18.0);
                                    self.drawing_toolbar(ui);
                                });
                                return;
                            }
                            if self.flags.workspace == tools::WS_SIMULATION {
                                ui.vertical(|ui| {
                                    ui.add_space(18.0);
                                    self.simulation_toolbar(ui);
                                });
                                return;
                            }
                            if self.flags.workspace == tools::WS_ANIMATION {
                                ui.vertical(|ui| {
                                    ui.add_space(18.0);
                                    self.animation_toolbar(ui);
                                });
                                return;
                            }
                            if self.flags.workspace == tools::WS_MANUFACTURE {
                                ui.vertical(|ui| {
                                    ui.add_space(18.0);
                                    self.manufacture_toolbar(ui);
                                });
                                return;
                            }
                            if self.flags.workspace == tools::WS_RENDER {
                                ui.vertical(|ui| {
                                    ui.add_space(18.0);
                                    self.render_toolbar(ui);
                                });
                                return;
                            }
                            if self.flags.workspace != tools::WS_DESIGN {
                                let name = tools::WORKSPACES[self.flags.workspace].0;
                                ui.vertical(|ui| {
                                    ui.add_space(14.0);
                                    ui.label(RichText::new(format!("{}: {}", tr(name), tr("this workspace is not available yet."))).color(theme::TEXT_DIM));
                                });
                                return;
                            }
                            for panel in tab.panels {
                                self.ribbon_panel(ui, panel);
                            }
                        });
                    });
                });
            });
    }

    fn tab_strip(&mut self, ui: &mut egui::Ui, in_sketch: bool) {
        ui.add_space(6.0);
        // Workspace switcher
        let ws = tools::WORKSPACES[self.flags.workspace].0;
        let resp = ui.add(
            egui::Button::new(RichText::new(format!("{}     ", tr(ws))).strong().color(theme::TEXT))
                .fill(Color32::WHITE)
                .stroke(egui::Stroke::new(1.0, theme::PANEL_BORDER))
                .min_size(egui::vec2(110.0, 22.0)),
        );
        dropdown_triangle(ui.painter(), resp.rect.right_center() - egui::vec2(12.0, 0.0), theme::TEXT);
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(180.0);
            for (i, (name, available)) in tools::WORKSPACES.iter().enumerate() {
                let label = if *available { tr(name).to_string() } else { format!("{}  ({})", tr(name), tr("coming soon")) };
                if ui.selectable_label(self.flags.workspace == i, label).clicked() {
                    self.flags.workspace = i;
                    if !available {
                        self.toast(ToastKind::Info, format!("{}: {}", tr(name), tr("this workspace is not available yet.")));
                    }
                }
            }
        });
        ui.add_space(14.0);
        if self.flags.workspace != tools::WS_DESIGN {
            let name = tools::WORKSPACES[self.flags.workspace].0;
            tab_label(ui, tr(name), true, theme::ACCENT);
            return;
        }
        if in_sketch {
            tab_label(ui, tr("SKETCH"), true, theme::SKETCH_GREEN);
            for tab in tools::DESIGN_TABS.iter().take(1) {
                if tab_label(ui, tr(tab.name), false, theme::ACCENT).clicked() {
                    self.toast(ToastKind::Info, tr("Finish the sketch to return to the SOLID tools."));
                }
            }
            return;
        }
        for (i, tab) in tools::DESIGN_TABS.iter().enumerate() {
            if tab_label(ui, tr(tab.name), self.flags.toolbar_tab == i, theme::ACCENT).clicked() {
                self.flags.toolbar_tab = i;
            }
        }
    }

    fn ribbon_panel(&mut self, ui: &mut egui::Ui, panel: &tools::Panel) {
        let finish = panel.accent == Some(Accent::Finish);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(1.0, 0.0);
                ui.add_space(4.0);
                for tool in panel.tools.iter().take(panel.featured) {
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(38.0, 42.0), Sense::click());
                    let implemented = tools::is_implemented(tool.id);
                    if resp.hovered() {
                        ui.painter().rect_filled(rect, 3.0, theme::HOVER_BG);
                    }
                    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(30.0, 30.0));
                    icons::paint(ui.painter(), icon_rect, tool.icon, None);
                    if !implemented {
                        ui.painter().rect_filled(rect, 3.0, Color32::from_white_alpha(70));
                    }
                    let mut tip = tr(tool.label).to_string();
                    if let Some(k) = crate::prefs::shortcut_label(tool.id) {
                        tip.push_str(&format!("  ({k})"));
                    }
                    if !implemented {
                        tip.push('\n');
                        tip.push_str(tr("Not yet available in Merge360"));
                    }
                    if resp.on_hover_text(tip).clicked() {
                        self.actions.push(Action::Tool(tool.id));
                    }
                }
                ui.add_space(4.0);
            });
            // Panel dropdown label
            let width = ui.min_rect().width().max(60.0);
            let label_color = if finish { theme::SKETCH_GREEN } else { theme::TEXT };
            let galley = ui.painter().layout_no_wrap(tr(panel.name).to_string(), egui::FontId::proportional(10.5), label_color);
            let width = width.max(galley.size().x + 22.0);
            let (lrect, resp) = ui.allocate_exact_size(egui::vec2(width, 18.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(lrect, 2.0, theme::HOVER_BG);
            }
            let text_pos = lrect.center() - egui::vec2(galley.size().x / 2.0 + 5.0, galley.size().y / 2.0);
            let tri_x = text_pos.x + galley.size().x + 6.0;
            ui.painter().galley(text_pos, galley, label_color);
            dropdown_triangle(ui.painter(), egui::pos2(tri_x, lrect.center().y), label_color);
            egui::Popup::menu(&resp).show(|ui| {
                ui.set_min_width(230.0);
                for tool in panel.tools {
                    let implemented = tools::is_implemented(tool.id);
                    let text = RichText::new(tr(tool.label)).color(if implemented { theme::TEXT } else { theme::TEXT_DIM });
                    let (rect, _) = (ui.available_rect_before_wrap(), ());
                    let b = egui::Button::new(text).shortcut_text(crate::prefs::shortcut_label(tool.id).unwrap_or_default()).min_size(egui::vec2(220.0, 24.0));
                    let r = ui.horizontal(|ui| {
                        let (ir, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::hover());
                        icons::paint(ui.painter(), ir, tool.icon, None);
                        ui.add(b)
                    });
                    let _ = rect;
                    if r.inner.clicked() {
                        self.actions.push(Action::Tool(tool.id));
                    }
                }
            });
        });
        // Separator
        let (sep, _) = ui.allocate_exact_size(egui::vec2(1.0, 58.0), Sense::hover());
        ui.painter().vline(sep.center().x, sep.y_range().shrink(4.0), egui::Stroke::new(1.0, theme::PANEL_BORDER));
    }
}

/// Small downward triangle used by dropdown labels.
pub fn dropdown_triangle(painter: &egui::Painter, c: egui::Pos2, color: Color32) {
    painter.add(egui::Shape::convex_polygon(
        vec![c + egui::vec2(-3.5, -2.0), c + egui::vec2(3.5, -2.0), c + egui::vec2(0.0, 2.5)],
        color,
        egui::Stroke::NONE,
    ));
}

fn tab_label(ui: &mut egui::Ui, text: &str, active: bool, accent: Color32) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(11.5), theme::TEXT);
    let size = egui::vec2(galley.size().x + 22.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if active {
        ui.painter().rect_filled(rect, 0.0, theme::TOOLBAR_BG);
        ui.painter().hline(rect.x_range(), rect.bottom() - 1.5, egui::Stroke::new(3.0, accent));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(0xf0, 0xf1, 0xf2));
    }
    let color = if active { accent } else { theme::TEXT };
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(11.5), color);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    resp
}
