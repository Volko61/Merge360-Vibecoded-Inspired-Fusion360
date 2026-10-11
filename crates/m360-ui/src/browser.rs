//! The BROWSER panel (floating tree at the top-left of the canvas).

use eframe::egui::{self, Color32, RichText, Sense};
use m360_core::{BasePlane, FeatureKind};

use crate::app::{Action, Merge360App};
use crate::icons::{self, Icon};
use crate::state::{Command, SelItem};
use crate::theme;

const ROW_H: f32 = 22.0;
const INDENT: f32 = 14.0;

struct Row<'a> {
    depth: usize,
    /// Default expanded state when the row has not been toggled yet.
    open_default: bool,
    expandable: bool,
    eye: Option<bool>,
    icon: Icon,
    label: &'a str,
    selected: bool,
    dim: bool,
    error: bool,
}

struct RowResponse {
    resp: egui::Response,
    toggled_eye: bool,
}

fn row(ui: &mut egui::Ui, id: egui::Id, r: Row) -> RowResponse {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, ROW_H), Sense::click());
    let painter = ui.painter();
    if r.selected {
        painter.rect_filled(rect, 0.0, theme::SELECT_BG);
    } else if resp.hovered() {
        painter.rect_filled(rect, 0.0, theme::HOVER_BG);
    }
    let mut x = rect.left() + 4.0 + r.depth as f32 * INDENT;
    let cy = rect.center().y;
    let mut toggled_eye = false;
    let open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(r.open_default);
    if r.expandable {
        let ar = egui::Rect::from_center_size(egui::pos2(x + 5.0, cy), egui::vec2(12.0, 12.0));
        let ar_resp = ui.interact(ar, id.with("exp"), Sense::click());
        let c = theme::TEXT_DIM;
        let pts = if open {
            vec![ar.left_top() + egui::vec2(2.0, 3.5), ar.right_top() + egui::vec2(-2.0, 3.5), ar.center_bottom() - egui::vec2(0.0, 3.0)]
        } else {
            vec![ar.left_top() + egui::vec2(3.5, 2.0), ar.right_center() - egui::vec2(3.0, 0.0), ar.left_bottom() + egui::vec2(3.5, -2.0)]
        };
        ui.painter().add(egui::Shape::convex_polygon(pts, c, egui::Stroke::NONE));
        if ar_resp.clicked() {
            ui.data_mut(|d| d.insert_temp(id, !open));
        }
    }
    x += 14.0;
    if let Some(visible) = r.eye {
        let er = egui::Rect::from_center_size(egui::pos2(x + 8.0, cy), egui::vec2(16.0, 16.0));
        let e_resp = ui.interact(er, id.with("eye"), Sense::click());
        icons::paint(ui.painter(), er, if visible { Icon::Eye } else { Icon::EyeOff }, None);
        if e_resp.clicked() {
            toggled_eye = true;
        }
        x += 20.0;
    }
    let ir = egui::Rect::from_center_size(egui::pos2(x + 8.0, cy), egui::vec2(16.0, 16.0));
    icons::paint(ui.painter(), ir, r.icon, None);
    x += 22.0;
    let color = if r.error {
        theme::ERROR
    } else if r.dim {
        theme::TEXT_DIM
    } else {
        theme::TEXT
    };
    ui.painter().text(egui::pos2(x, cy), egui::Align2::LEFT_CENTER, r.label, egui::FontId::proportional(12.5), color);
    RowResponse { resp, toggled_eye }
}

fn is_open(ui: &egui::Ui, id: egui::Id, default: bool) -> bool {
    ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(default)
}

impl Merge360App {
    pub(crate) fn browser(&mut self, ctx: &egui::Context) {
        if !self.flags.browser_open {
            return;
        }
        let pos = self.canvas_rect.min + egui::vec2(8.0, 8.0);
        let max_h = (self.canvas_rect.height() - 80.0).max(120.0);
        egui::Area::new(egui::Id::new("browser"))
            .fixed_pos(pos)
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(0xfb, 0xfb, 0xfb, 238))
                    .stroke(egui::Stroke::new(1.0, theme::PANEL_BORDER))
                    .corner_radius(3)
                    .shadow(egui::epaint::Shadow { offset: [0, 1], blur: 6, spread: 0, color: Color32::from_black_alpha(30) })
                    .show(ui, |ui| {
                        ui.set_width(250.0);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let header_id = egui::Id::new("browser_body_open");
                        let open = is_open(ui, header_id, true);
                        ui.horizontal(|ui| {
                            ui.add_space(6.0);
                            let arrow = if open { "◀" } else { "▶" };
                            if ui.add(egui::Button::new(RichText::new(arrow).size(9.0)).frame(false)).clicked() {
                                ui.data_mut(|d| d.insert_temp(header_id, !open));
                            }
                            ui.label(RichText::new("BROWSER").size(11.5).strong().color(theme::TEXT));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(egui::Button::new(RichText::new("✕").size(10.0)).frame(false)).on_hover_text("Hide Browser").clicked() {
                                    self.flags.browser_open = false;
                                }
                            });
                        });
                        if !open {
                            return;
                        }
                        ui.add_space(2.0);
                        egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
                            self.browser_tree(ui);
                        });
                        ui.add_space(4.0);
                    });
            });
    }

    fn browser_tree(&mut self, ui: &mut egui::Ui) {
        let active = self.active;
        let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.shift);
        let root_id = egui::Id::new(("br_root", active));
        let title = self.tabs[active].title();
        let r = row(ui, root_id, Row { open_default: true, depth: 0, expandable: true, eye: None, icon: Icon::Component, label: &title, selected: false, dim: false, error: false });
        r.resp.context_menu(|ui| {
            if ui.button("Save").clicked() {
                self.actions.push(Action::Save);
            }
            if ui.button("Export…").clicked() {
                self.actions.push(Action::Export(crate::files::FileMode::ExportStep));
            }
        });
        if !is_open(ui, root_id, true) {
            return;
        }

        // Document Settings
        let ds_id = egui::Id::new(("br_ds", active));
        row(ui, ds_id, Row { open_default: false, depth: 1, expandable: true, eye: None, icon: Icon::Gear, label: "Document Settings", selected: false, dim: false, error: false });
        if is_open(ui, ds_id, false) {
            let units = format!("Units: {}", self.tabs[active].doc.units.label());
            let r = row(ui, ds_id.with("u"), Row { open_default: true, depth: 2, expandable: false, eye: None, icon: Icon::Parameters, label: &units, selected: false, dim: false, error: false });
            if r.resp.clicked() {
                self.tabs[active].command = Some(Command::DocSettings);
            }
        }

        // Named Views
        let nv_id = egui::Id::new(("br_nv", active));
        row(ui, nv_id, Row { open_default: false, depth: 1, expandable: true, eye: None, icon: Icon::Camera, label: "Named Views", selected: false, dim: false, error: false });
        if is_open(ui, nv_id, false) {
            let views: [(&str, glam::Vec3); 4] = [
                ("TOP", glam::Vec3::Y),
                ("FRONT", glam::Vec3::Z),
                ("RIGHT", glam::Vec3::X),
                ("HOME", glam::Vec3::new(1.0, 0.8, 1.0)),
            ];
            for (name, dir) in views {
                let r = row(ui, nv_id.with(name), Row { open_default: true, depth: 2, expandable: false, eye: None, icon: Icon::Camera, label: name, selected: false, dim: false, error: false });
                if r.resp.clicked() {
                    let now = self.now();
                    let tab = &mut self.tabs[active];
                    let target = if name == "HOME" {
                        tab.home
                    } else {
                        let mut c = tab.camera;
                        c.look_from(dir);
                        if name == "TOP" {
                            c.yaw = 0.0;
                        }
                        c
                    };
                    tab.animate_to(target, now);
                }
            }
        }

        // Origin
        let or_id = egui::Id::new(("br_origin", active));
        let origin_visible = self.tabs[active].doc.origin_visible;
        let r = row(ui, or_id, Row { open_default: false, depth: 1, expandable: true, eye: Some(origin_visible), icon: Icon::Folder, label: "Origin", selected: false, dim: !origin_visible, error: false });
        if r.toggled_eye {
            self.tabs[active].doc.origin_visible = !origin_visible;
        }
        if is_open(ui, or_id, false) {
            row(ui, or_id.with("O"), Row { open_default: true, depth: 2, expandable: false, eye: None, icon: Icon::Point, label: "O", selected: false, dim: !origin_visible, error: false });
            for a in ["X", "Y", "Z"] {
                row(ui, or_id.with(a), Row { open_default: true, depth: 2, expandable: false, eye: None, icon: Icon::Axis, label: a, selected: false, dim: !origin_visible, error: false });
            }
            for (name, plane) in [("XY", BasePlane::XY), ("XZ", BasePlane::XZ), ("YZ", BasePlane::YZ)] {
                let sel = self.tabs[active].selection.contains(&SelItem::Plane(plane));
                let r = row(ui, or_id.with(name), Row { open_default: true, depth: 2, expandable: false, eye: None, icon: Icon::Plane, label: name, selected: sel, dim: !origin_visible, error: false });
                if r.resp.clicked() {
                    self.click_item(SelItem::Plane(plane), ctrl);
                }
            }
        }

        // Construction
        let planes: Vec<(u64, String)> = self.tabs[active].model.planes.iter().map(|p| (p.uid, p.name.clone())).collect();
        if !planes.is_empty() {
            let co_id = egui::Id::new(("br_construction", active));
            row(ui, co_id, Row { open_default: true, depth: 1, expandable: true, eye: None, icon: Icon::Folder, label: "Construction", selected: false, dim: false, error: false });
            if is_open(ui, co_id, true) {
                for (uid, name) in planes {
                    let tab = &self.tabs[active];
                    let visible = tab.doc.plane_visible.get(&uid).copied().unwrap_or(true);
                    let sel = tab.selection.contains(&SelItem::ConstructionPlane(uid));
                    let r = row(ui, co_id.with(uid), Row { open_default: true, depth: 2, expandable: false, eye: Some(visible), icon: Icon::Plane, label: &name, selected: sel, dim: !visible, error: false });
                    if r.toggled_eye {
                        self.tabs[active].doc.plane_visible.insert(uid, !visible);
                    } else if r.resp.double_clicked() {
                        self.actions.push(Action::EditFeature(uid));
                    } else if r.resp.clicked() {
                        self.click_item(SelItem::ConstructionPlane(uid), ctrl);
                    }
                    r.resp.context_menu(|ui| {
                        if ui.button("Create Sketch").clicked() {
                            self.tabs[active].selection = vec![SelItem::ConstructionPlane(uid)];
                            self.actions.push(Action::Tool("create_sketch"));
                        }
                        if ui.button("Edit Feature").clicked() {
                            self.actions.push(Action::EditFeature(uid));
                        }
                        if ui.button(RichText::new("Delete").color(theme::ERROR)).clicked() {
                            self.tabs[active].edit("Delete Plane", |d| d.delete_feature(uid));
                        }
                    });
                }
            }
        }

        // Analysis (section views)
        if self.tabs[active].section.is_some() {
            let an_id = egui::Id::new(("br_analysis", active));
            row(ui, an_id, Row { open_default: true, depth: 1, expandable: true, eye: None, icon: Icon::Folder, label: "Analysis", selected: false, dim: false, error: false });
            if is_open(ui, an_id, true) {
                let on = self.tabs[active].section.as_ref().is_some_and(|s| s.visible);
                let r = row(ui, an_id.with("sec"), Row { open_default: true, depth: 2, expandable: false, eye: Some(on), icon: Icon::Section, label: "Section1", selected: false, dim: !on, error: false });
                if r.toggled_eye {
                    if let Some(sec) = &mut self.tabs[active].section {
                        sec.visible = !on;
                    }
                } else if r.resp.double_clicked() {
                    self.actions.push(Action::Tool("section_analysis"));
                }
                r.resp.context_menu(|ui| {
                    if ui.button("Edit Section Analysis").clicked() {
                        self.actions.push(Action::Tool("section_analysis"));
                    }
                    if ui.button(RichText::new("Delete").color(theme::ERROR)).clicked() {
                        self.tabs[active].section = None;
                    }
                });
            }
        }

        // Bodies
        let bo_id = egui::Id::new(("br_bodies", active));
        let folder_visible = self.tabs[active].doc.bodies_folder_visible;
        let all_bodies: Vec<_> = self.tabs[active].model.bodies.iter().map(|b| b.id).collect();
        let bodies: Vec<_> = all_bodies.iter().copied().filter(|b| self.tabs[active].doc.body_meta(*b).component.is_none()).collect();
        if !bodies.is_empty() {
            let r = row(ui, bo_id, Row { open_default: true, depth: 1, expandable: true, eye: Some(folder_visible), icon: Icon::Folder, label: "Bodies", selected: false, dim: !folder_visible, error: false });
            if r.toggled_eye {
                self.tabs[active].edit("Toggle Bodies", |d| d.bodies_folder_visible = !folder_visible);
            }
            if is_open(ui, bo_id, true) {
                for id in bodies {
                    self.body_row(ui, bo_id, id, ctrl);
                }
            }
        }

        // Components
        let comps = self.tabs[active].doc.components.clone();
        for c in comps {
            let cid = egui::Id::new(("br_comp", active, c.id));
            let members: Vec<u64> = all_bodies.iter().copied().filter(|b| self.tabs[active].doc.body_meta(*b).component == Some(c.id)).collect();
            if let Some((SelItem::Feature(rid), text)) = &mut self.flags.rename {
                if *rid == c.id {
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(200.0));
                    r.request_focus();
                    if r.lost_focus() {
                        let name = text.trim().to_string();
                        self.flags.rename = None;
                        if !name.is_empty() {
                            let id = c.id;
                            self.tabs[active].edit("Rename Component", |d| {
                                if let Some(k) = d.components.iter_mut().find(|k| k.id == id) {
                                    k.name = name;
                                }
                            });
                        }
                    }
                    continue;
                }
            }
            let r = row(ui, cid, Row { open_default: true, depth: 1, expandable: true, eye: Some(c.visible), icon: Icon::Component, label: &c.name, selected: false, dim: !c.visible, error: false });
            if r.toggled_eye {
                let id = c.id;
                self.tabs[active].edit("Show/Hide Component", |d| {
                    if let Some(k) = d.components.iter_mut().find(|k| k.id == id) {
                        k.visible = !k.visible;
                    }
                });
            } else if r.resp.double_clicked() {
                self.flags.rename = Some((SelItem::Feature(c.id), c.name.clone()));
            } else if r.resp.clicked() {
                self.tabs[active].selection = members.iter().map(|b| SelItem::Body(*b)).collect();
            }
            r.resp.context_menu(|ui| {
                if ui.button("Rename").clicked() {
                    self.flags.rename = Some((SelItem::Feature(c.id), c.name.clone()));
                }
                if ui.button("Joint…").clicked() {
                    self.actions.push(Action::Tool("joint"));
                }
                if ui.button("Dissolve Component").clicked() {
                    let id = c.id;
                    self.tabs[active].edit("Dissolve Component", |d| {
                        d.components.retain(|k| k.id != id);
                        for m in d.bodies.values_mut() {
                            if m.component == Some(id) {
                                m.component = None;
                            }
                        }
                    });
                }
            });
            if is_open(ui, cid, true) {
                for id in members {
                    self.body_row(ui, cid, id, ctrl);
                }
            }
        }

        // Sketches
        let sk_id = egui::Id::new(("br_sketches", active));
        let sketches: Vec<(u64, String)> = self.tabs[active].model.sketches.iter().map(|s| (s.uid, s.name.clone())).collect();
        if !sketches.is_empty() {
            let folder = self.tabs[active].doc.sketches_folder_visible;
            let r = row(ui, sk_id, Row { open_default: true, depth: 1, expandable: true, eye: Some(folder), icon: Icon::Folder, label: "Sketches", selected: false, dim: !folder, error: false });
            if r.toggled_eye {
                self.tabs[active].edit("Toggle Sketches", |d| d.sketches_folder_visible = !folder);
            }
            if is_open(ui, sk_id, true) {
                for (uid, name) in sketches {
                    let tab = &self.tabs[active];
                    let visible = tab.doc.sketch_visible.get(&uid).copied().unwrap_or(true);
                    let sel = tab.selection.contains(&SelItem::Sketch(uid));
                    let editing = tab.sketch.as_ref().is_some_and(|s| s.uid == uid);
                    let r = row(ui, sk_id.with(uid), Row { open_default: true, depth: 2, expandable: false, eye: Some(visible), icon: Icon::SketchSmall, label: &name, selected: sel || editing, dim: !visible, error: false });
                    if r.toggled_eye {
                        self.tabs[active].doc.sketch_visible.insert(uid, !visible);
                    } else if r.resp.double_clicked() {
                        self.actions.push(Action::EditSketch(uid));
                    } else if r.resp.clicked() {
                        self.click_item(SelItem::Sketch(uid), ctrl);
                    }
                    r.resp.context_menu(|ui| {
                        if ui.button("Edit Sketch").clicked() {
                            self.actions.push(Action::EditSketch(uid));
                        }
                        if ui.button(if visible { "Hide" } else { "Show" }).clicked() {
                            self.tabs[active].doc.sketch_visible.insert(uid, !visible);
                        }
                        if ui.button("Delete").clicked() {
                            self.tabs[active].edit("Delete Sketch", |d| d.delete_feature(uid));
                        }
                    });
                }
            }
        }
    }

    fn body_row(&mut self, ui: &mut egui::Ui, parent: egui::Id, id: u64, ctrl: bool) {
        let active = self.active;
        let meta = self.tabs[active].doc.body_meta(id);
        let sel = self.tabs[active].selection.iter().any(|s| s.body() == Some(id));
        let row_id = parent.with(id);
        if let Some((SelItem::Body(rid), text)) = &mut self.flags.rename {
            if *rid == id {
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(200.0));
                r.request_focus();
                if r.lost_focus() {
                    let name = text.trim().to_string();
                    self.flags.rename = None;
                    if !name.is_empty() {
                        self.tabs[active].edit("Rename Body", |d| {
                            let mut m = d.body_meta(id);
                            m.name = name;
                            d.bodies.insert(id, m);
                        });
                    }
                }
                return;
            }
        }
        let r = row(ui, row_id, Row { open_default: true, depth: 2, expandable: false, eye: Some(meta.visible), icon: Icon::Body, label: &meta.name, selected: sel, dim: !meta.visible, error: false });
        let toggle = |app: &mut Self| {
            app.tabs[active].edit("Show/Hide Body", |d| {
                let mut m = d.body_meta(id);
                m.visible = !m.visible;
                d.bodies.insert(id, m);
            });
        };
        if r.toggled_eye {
            toggle(self);
        } else if r.resp.double_clicked() {
            self.flags.rename = Some((SelItem::Body(id), meta.name.clone()));
        } else if r.resp.clicked() {
            self.click_item(SelItem::Body(id), ctrl);
        }
        r.resp.context_menu(|ui| {
            if !sel {
                self.tabs[active].selection = vec![SelItem::Body(id)];
            }
            if ui.button("Move/Copy").clicked() {
                self.actions.push(Action::Tool("move"));
            }
            if ui.button("Appearance").clicked() {
                self.actions.push(Action::Tool("appearance"));
            }
            if ui.button("Properties").clicked() {
                self.actions.push(Action::Tool("properties"));
            }
            if ui.button("Create Components from Bodies").clicked() {
                self.actions.push(Action::Tool("new_component"));
            }
            ui.separator();
            if ui.button("Rename").clicked() {
                self.flags.rename = Some((SelItem::Body(id), meta.name.clone()));
            }
            if ui.button(if meta.visible { "Hide Body" } else { "Show Body" }).clicked() {
                toggle(self);
            }
            if ui.button("Find in Timeline").clicked() {
                let feat = self.tabs[active].model.body(id).map(|b| b.last_feature);
                self.tabs[active].timeline_hover = feat;
            }
            if ui.button("Save as STL").clicked() {
                self.actions.push(Action::Export(crate::files::FileMode::ExportStl));
            }
            ui.separator();
            if ui.button(RichText::new("Delete").color(theme::ERROR)).clicked() {
                self.actions.push(Action::DeleteSelection);
            }
        });
    }

    /// Click-select semantics shared by browser and canvas.
    pub(crate) fn click_item(&mut self, item: SelItem, additive: bool) {
        let tab = self.tab();
        if additive {
            if let Some(i) = tab.selection.iter().position(|s| *s == item) {
                tab.selection.remove(i);
            } else {
                tab.selection.push(item);
            }
        } else {
            tab.selection = vec![item];
        }
    }
}

/// Icon for a timeline feature.
pub fn feature_icon(kind: &FeatureKind) -> Icon {
    match kind {
        FeatureKind::Box { .. } => Icon::Box,
        FeatureKind::Cylinder { .. } => Icon::Cylinder,
        FeatureKind::Sphere { .. } => Icon::Sphere,
        FeatureKind::Torus { .. } => Icon::Torus,
        FeatureKind::Coil { .. } => Icon::Coil,
        FeatureKind::Combine { .. } => Icon::Combine,
        FeatureKind::Move { .. } => Icon::Move,
        FeatureKind::Remove { .. } => Icon::Delete,
        FeatureKind::Sketch { .. } => Icon::SketchSmall,
        FeatureKind::Extrude { .. } => Icon::Extrude,
        FeatureKind::Revolve { .. } => Icon::Revolve,
        FeatureKind::Hole { .. } => Icon::Hole,
        FeatureKind::Pattern { .. } => Icon::Pattern,
        FeatureKind::Mirror { .. } => Icon::Mirror,
        FeatureKind::ConstructionPlane { .. } => Icon::Plane,
        FeatureKind::Joint { .. } => Icon::Joint,
        FeatureKind::ImportMesh { .. } => Icon::Insert,
        FeatureKind::Sweep { .. } => Icon::Sweep,
        FeatureKind::Pipe { .. } => Icon::Pipe,
        FeatureKind::Loft { .. } => Icon::Loft,
        FeatureKind::SplitBody { .. } => Icon::Section,
        FeatureKind::Scale { .. } => Icon::Generic,
        FeatureKind::Shell { .. } => Icon::Shell,
        FeatureKind::Fillet { chamfer: true, .. } => Icon::Chamfer,
        FeatureKind::Fillet { .. } => Icon::Fillet,
    }
}
