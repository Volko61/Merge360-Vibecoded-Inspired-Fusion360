//! The Preferences window (user menu ▸ Preferences).

use eframe::egui::{self, RichText};

use crate::app::Merge360App;
use crate::i18n::{tr, Language};
use crate::prefs::{action_label, KeyContext, KeymapPreset, NavPreset, Preferences, COMMANDS};
use crate::theme;

const PAGES: [&str; 4] = ["General", "Navigation", "Keyboard", "Display"];

impl Merge360App {
    pub(crate) fn preferences_ui(&mut self, ctx: &egui::Context) {
        if !self.prefs_ui.open {
            return;
        }
        let before = self.prefs.clone();
        let mut open = true;
        egui::Window::new(tr("Preferences"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([700.0, 480.0])
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(130.0);
                        for (i, p) in PAGES.iter().enumerate() {
                            if ui.selectable_label(self.prefs_ui.page == i, tr(p)).clicked() {
                                self.prefs_ui.page = i;
                            }
                        }
                        ui.add_space(20.0);
                        if ui.button(tr("Restore Defaults")).clicked() {
                            self.prefs = Preferences::default();
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_min_width(520.0);
                        egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| match self.prefs_ui.page {
                            0 => self.prefs_general(ui),
                            1 => self.prefs_navigation(ui),
                            2 => self.prefs_keyboard(ui),
                            _ => self.prefs_display(ui),
                        });
                    });
                });
            });
        if !open {
            self.prefs_ui.open = false;
            self.prefs_ui.capture = None;
        }
        if self.prefs != before {
            self.prefs.apply_globals();
            self.prefs.save();
            if (self.prefs.ui_scale - before.ui_scale).abs() > 1e-4 {
                ctx.set_zoom_factor(self.prefs.ui_scale);
            }
        }
    }

    fn prefs_general(&mut self, ui: &mut egui::Ui) {
        let p = &mut self.prefs;
        egui::Grid::new("prefs_general").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label(tr("Language"));
            egui::ComboBox::from_id_salt("pref_lang").selected_text(p.language.native_name()).show_ui(ui, |ui| {
                for l in Language::ALL {
                    ui.selectable_value(&mut p.language, l, l.native_name());
                }
            });
            ui.end_row();
            ui.label(tr("Default units"));
            egui::ComboBox::from_id_salt("pref_units").selected_text(tr(p.default_units.label())).show_ui(ui, |ui| {
                for u in m360_core::Units::ALL {
                    ui.selectable_value(&mut p.default_units, u, tr(u.label()));
                }
            });
            ui.end_row();
            ui.label(tr("Interface scale"));
            ui.vertical(|ui| {
                ui.add(egui::Slider::new(&mut p.ui_scale, 0.75..=3.0).step_by(0.05).custom_formatter(|v, _| format!("{:.0} %", v * 100.0)));
                ui.horizontal(|ui| {
                    for s in [1.0, 1.25, 1.5, 1.75, 2.0] {
                        if ui.small_button(format!("{:.0} %", s * 100.0)).clicked() {
                            p.ui_scale = s;
                        }
                    }
                });
            });
            ui.end_row();
            ui.label(tr("Auto-save recovery copy"));
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut p.autosave_minutes).range(0..=120).suffix(" min"));
                ui.label(RichText::new(tr("0 = off")).color(theme::TEXT_DIM));
            });
            ui.end_row();
        });
        ui.add_space(8.0);
        if let Some(d) = crate::files::config_dir() {
            ui.label(RichText::new(format!("{}: {}", tr("Settings are saved in"), d.join("preferences.ron").display())).small().color(theme::TEXT_DIM));
            ui.label(
                RichText::new(format!("{}: {}", tr("Custom translations"), d.join("lang").join("<code>.txt").display()))
                    .small()
                    .color(theme::TEXT_DIM),
            );
        }
    }

    fn prefs_navigation(&mut self, ui: &mut egui::Ui) {
        let p = &mut self.prefs;
        egui::Grid::new("prefs_nav").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label(tr("Pan, Zoom, Orbit shortcuts"));
            egui::ComboBox::from_id_salt("pref_nav").selected_text(p.navigation.label()).show_ui(ui, |ui| {
                for n in NavPreset::ALL {
                    ui.selectable_value(&mut p.navigation, n, n.label());
                }
            });
            ui.end_row();
            for (what, how) in p.navigation.describe() {
                ui.label(RichText::new(tr(what)).color(theme::TEXT_DIM));
                ui.label(tr(how));
                ui.end_row();
            }
            ui.label(tr("Zoom speed"));
            ui.add(egui::Slider::new(&mut p.zoom_speed, 0.2..=4.0).logarithmic(true));
            ui.end_row();
            ui.label(tr("Orbit speed"));
            ui.add(egui::Slider::new(&mut p.orbit_speed, 0.2..=4.0).logarithmic(true));
            ui.end_row();
            ui.label(tr("Pan speed"));
            ui.add(egui::Slider::new(&mut p.pan_speed, 0.2..=4.0).logarithmic(true));
            ui.end_row();
            ui.label(tr("Reverse zoom direction"));
            ui.checkbox(&mut p.invert_zoom, "");
            ui.end_row();
            ui.label(tr("Zoom towards the cursor"));
            ui.checkbox(&mut p.zoom_to_cursor, "");
            ui.end_row();
        });
    }

    fn prefs_keyboard(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(tr("Keyboard shortcuts"));
            let p = &mut self.prefs;
            egui::ComboBox::from_id_salt("pref_keys").selected_text(p.keymap.label()).show_ui(ui, |ui| {
                for k in KeymapPreset::ALL {
                    ui.selectable_value(&mut p.keymap, k, k.label());
                }
            });
            if !p.key_overrides.is_empty() && ui.button(tr("Reset to preset")).clicked() {
                p.key_overrides.clear();
            }
        });
        ui.label(
            RichText::new(tr("Click a shortcut, then press the new key combination (Backspace removes it, Esc cancels)."))
                .small()
                .color(theme::TEXT_DIM),
        );
        let filter_id = ui.make_persistent_id("key_filter");
        let mut filter: String = ui.data_mut(|d| d.get_temp(filter_id)).unwrap_or_default();
        ui.add(egui::TextEdit::singleline(&mut filter).hint_text(tr("Search actions…")).desired_width(240.0));
        ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
        let q = filter.to_lowercase();
        ui.add_space(4.0);

        // Every command and tool, in each context it can be bound in.
        let bindings = self.prefs.bindings();
        let mut rows: Vec<(&'static str, KeyContext)> = Vec::new();
        // Bound actions in each of their contexts, then unbound ones once.
        for b in &bindings {
            if !rows.contains(&(b.action, b.ctx)) {
                rows.push((b.action, b.ctx));
            }
        }
        let unbound = |id: &str| !bindings.iter().any(|b| b.action == id);
        for (c, _) in COMMANDS {
            if unbound(c) {
                rows.push((c, KeyContext::Global));
            }
        }
        for t in crate::tools::all_tools(true) {
            if crate::tools::is_implemented(t.id) && unbound(t.id) && !rows.iter().any(|r| r.0 == t.id) {
                let ctx = if t.id.starts_with("sk_") || t.id.starts_with("con_") { KeyContext::Sketch } else { KeyContext::Global };
                rows.push((t.id, ctx));
            }
        }
        rows.sort_by_key(|(a, c)| (bindings.iter().all(|b| b.action != *a), tr(action_label(a)).to_lowercase(), *c as u8));
        egui::Grid::new("prefs_keys").num_columns(3).striped(true).spacing([16.0, 4.0]).show(ui, |ui| {
            ui.label(RichText::new(tr("Action")).strong());
            ui.label(RichText::new(tr("Context")).strong());
            ui.label(RichText::new(tr("Shortcut")).strong());
            ui.end_row();
            for (action, kctx) in rows {
                let name = tr(action_label(action));
                if !q.is_empty() && !name.to_lowercase().contains(&q) && !action_label(action).to_lowercase().contains(&q) {
                    continue;
                }
                ui.label(name);
                ui.label(RichText::new(tr(kctx.label())).color(theme::TEXT_DIM));
                let keys: Vec<String> = bindings.iter().filter(|b| b.action == action && b.ctx == kctx).map(|b| b.chord.text()).collect();
                let capturing = self.prefs_ui.capture.as_ref().is_some_and(|(a, c)| a == action && *c == kctx);
                let text = if capturing {
                    tr("Press keys…").to_string()
                } else if keys.is_empty() {
                    "—".to_string()
                } else {
                    keys.join(" / ")
                };
                if ui.add(egui::Button::new(text).selected(capturing).min_size(egui::vec2(130.0, 0.0))).clicked() {
                    self.prefs_ui.capture = Some((action.to_string(), kctx));
                }
                ui.end_row();
            }
        });
    }

    fn prefs_display(&mut self, ui: &mut egui::Ui) {
        let p = &mut self.prefs;
        egui::Grid::new("prefs_display").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label(tr("Show grid in new designs"));
            ui.checkbox(&mut p.show_grid, "");
            ui.end_row();
            ui.label(tr("Snap to grid in new designs"));
            ui.checkbox(&mut p.snap_to_grid, "");
            ui.end_row();
            ui.label(tr("Show the ViewCube"));
            ui.checkbox(&mut p.show_view_cube, "");
            ui.end_row();
            ui.label(tr("Show frame rate"));
            ui.checkbox(&mut p.show_fps, "");
            ui.end_row();
        });
    }
}
