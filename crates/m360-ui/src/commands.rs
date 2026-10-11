//! Command dialogs (BOX, EXTRUDE, COMBINE, MOVE/COPY, MEASURE …) and misc windows.

use eframe::egui::{self, vec2, Color32, RichText};
use glam::{DVec3, Vec3};
use m360_core::{Axis, BooleanOp, ExtrudeOp, FeatureKind, Units};
use m360_kernel::BodyMesh;

use crate::app::{Action, Merge360App};
use crate::state::{CombineField, Command, SelItem};
use crate::theme;
use crate::widgets::{angle_field, command_dialog, length_field, row_label, selection_field};

/// Volume (mm³) and centroid of a closed mesh.
pub fn mass_props(m: &BodyMesh) -> (f64, DVec3) {
    let mut vol = 0.0;
    let mut c = DVec3::ZERO;
    for (t, _) in m.triangles() {
        let a = t[0].as_dvec3();
        let b = t[1].as_dvec3();
        let d = t[2].as_dvec3();
        let v = a.dot(b.cross(d)) / 6.0;
        vol += v;
        c += (a + b + d) * v / 4.0;
    }
    if vol.abs() > 1e-12 {
        (vol, c / vol)
    } else {
        (0.0, DVec3::ZERO)
    }
}

fn face_area(m: &BodyMesh, face: u32) -> f64 {
    m.triangles()
        .filter(|(_, f)| *f == face)
        .map(|(t, _)| ((t[1] - t[0]).cross(t[2] - t[0]).length() / 2.0) as f64)
        .sum()
}

fn item_points(app: &Merge360App, item: &SelItem) -> Vec<Vec3> {
    let tab = &app.tabs[app.active];
    match item {
        SelItem::Body(b) => tab.model.body(*b).map(|b| b.mesh.positions.iter().map(|p| Vec3::from(*p)).collect()).unwrap_or_default(),
        SelItem::Face(b, f) | SelItem::FacePlane(b, f, _) => tab
            .model
            .body(*b)
            .map(|b| b.mesh.triangles().filter(|(_, id)| id == f).flat_map(|(t, _)| t).collect())
            .unwrap_or_default(),
        SelItem::Edge(b, e) => tab
            .model
            .body(*b)
            .and_then(|bd| bd.mesh.edges.get(*e as usize))
            .map(|pts| pts.iter().map(|p| Vec3::from(*p)).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn units_combo(ui: &mut egui::Ui, units: &mut Units) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt("units").selected_text(units.label()).show_ui(ui, |ui| {
        for u in Units::ALL {
            if ui.selectable_value(units, u, u.label()).changed() {
                changed = true;
            }
        }
    });
    changed
}

const MATERIALS: &[(&str, [f32; 3])] = &[
    ("Steel - Satin", [0.72, 0.74, 0.77]),
    ("Aluminum - Satin", [0.83, 0.84, 0.86]),
    ("Aluminum - Anodized Red", [0.70, 0.12, 0.12]),
    ("Aluminum - Anodized Blue", [0.15, 0.30, 0.70]),
    ("Brass - Polished", [0.80, 0.65, 0.30]),
    ("Copper", [0.78, 0.45, 0.30]),
    ("Gold - Polished", [0.90, 0.75, 0.30]),
    ("Plastic - Matte (White)", [0.93, 0.93, 0.92]),
    ("Plastic - Matte (Black)", [0.12, 0.12, 0.13]),
    ("Plastic - Glossy (Red)", [0.85, 0.15, 0.15]),
    ("Plastic - Glossy (Blue)", [0.15, 0.40, 0.85]),
    ("Plastic - Glossy (Green)", [0.20, 0.65, 0.30]),
    ("Plastic - Glossy (Yellow)", [0.95, 0.80, 0.15]),
    ("Plastic - Glossy (Orange)", [0.95, 0.50, 0.12]),
    ("Rubber - Black", [0.08, 0.08, 0.08]),
    ("Wood - Oak", [0.70, 0.52, 0.33]),
    ("Wood - Walnut", [0.42, 0.28, 0.18]),
    ("Glass - Clear", [0.80, 0.90, 0.95]),
    ("Concrete", [0.62, 0.62, 0.60]),
    ("Carbon Fiber", [0.20, 0.21, 0.23]),
];

impl Merge360App {
    pub(crate) fn dialogs(&mut self, ctx: &egui::Context) {
        self.file_dialog_ui(ctx);
        self.command_ui(ctx);
        self.misc_windows(ctx);
    }

    fn command_ui(&mut self, ctx: &egui::Context) {
        let Some(mut cmd) = self.tab().command.take() else {
            self.canvas.com = None;
            return;
        };
        let anchor = egui::pos2(self.canvas_rect.right() - 318.0, self.canvas_rect.top() + 150.0);
        let units = self.tabs[self.active].doc.units;
        let title = cmd.title();
        let preview_err = self.tabs[self.active].preview.as_ref().and_then(|p| p.error.clone());
        let mut finish: Option<bool> = None; // Some(true)=ok, Some(false)=cancel
        let mut enabled = true;

        match &mut cmd {
            Command::Primitive { kind, plane_chosen, .. } => {
                let ok = *plane_chosen && preview_err.is_none();
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Plane");
                    let plane_text = match kind {
                        FeatureKind::Box { plane, .. }
                        | FeatureKind::Cylinder { plane, .. }
                        | FeatureKind::Sphere { plane, .. }
                        | FeatureKind::Torus { plane, .. }
                        | FeatureKind::Coil { plane, .. } => {
                            if *plane_chosen { plane.label() } else { "Select".into() }
                        }
                        _ => String::new(),
                    };
                    if selection_field(ui, !*plane_chosen, &plane_text).clicked() {
                        *plane_chosen = false;
                    }
                    ui.end_row();
                    if *plane_chosen {
                        match kind {
                            FeatureKind::Box { center, length, width, height, .. } => {
                                center_rows(ui, center, units);
                                row_label(ui, "Length");
                                length_field(ui, "len", length, units);
                                ui.end_row();
                                row_label(ui, "Width");
                                length_field(ui, "wid", width, units);
                                ui.end_row();
                                row_label(ui, "Height");
                                length_field(ui, "hei", height, units);
                                ui.end_row();
                            }
                            FeatureKind::Cylinder { center, diameter, height, .. } => {
                                center_rows(ui, center, units);
                                row_label(ui, "Diameter");
                                length_field(ui, "dia", diameter, units);
                                ui.end_row();
                                row_label(ui, "Height");
                                length_field(ui, "hei", height, units);
                                ui.end_row();
                            }
                            FeatureKind::Sphere { center, diameter, .. } => {
                                center_rows(ui, center, units);
                                row_label(ui, "Diameter");
                                length_field(ui, "dia", diameter, units);
                                ui.end_row();
                            }
                            FeatureKind::Torus { center, inner_diameter, torus_diameter, .. } => {
                                center_rows(ui, center, units);
                                row_label(ui, "Inner Diameter");
                                length_field(ui, "idia", inner_diameter, units);
                                ui.end_row();
                                row_label(ui, "Torus Diameter");
                                length_field(ui, "tdia", torus_diameter, units);
                                ui.end_row();
                            }
                            FeatureKind::Coil { center, diameter, turns, height, taper, section, size, position, clockwise, .. } => {
                                center_rows(ui, center, units);
                                row_label(ui, "Diameter");
                                length_field(ui, "dia", diameter, units);
                                ui.end_row();
                                row_label(ui, "Rotation");
                                egui::ComboBox::from_id_salt("coil_rot")
                                    .selected_text(if *clockwise { "Clockwise" } else { "Counterclockwise" })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(clockwise, true, "Clockwise");
                                        ui.selectable_value(clockwise, false, "Counterclockwise");
                                    });
                                ui.end_row();
                                row_label(ui, "Revolutions");
                                ui.add(egui::DragValue::new(turns).speed(0.05).range(0.05..=200.0).max_decimals(2));
                                ui.end_row();
                                row_label(ui, "Height");
                                length_field(ui, "hei", height, units);
                                ui.end_row();
                                row_label(ui, "Pitch");
                                ui.label(units.format(*height / turns.max(1e-9)));
                                ui.end_row();
                                row_label(ui, "Angle");
                                angle_field(ui, "coil_ang", taper);
                                ui.end_row();
                                row_label(ui, "Section");
                                egui::ComboBox::from_id_salt("coil_sec").selected_text(section.label()).show_ui(ui, |ui| {
                                    for s in m360_core::CoilSectionType::ALL {
                                        ui.selectable_value(section, s, s.label());
                                    }
                                });
                                ui.end_row();
                                row_label(ui, "Section Position");
                                egui::ComboBox::from_id_salt("coil_pos").selected_text(position.label()).show_ui(ui, |ui| {
                                    for p in m360_core::CoilPosition::ALL {
                                        ui.selectable_value(position, p, p.label());
                                    }
                                });
                                ui.end_row();
                                row_label(ui, "Section Size");
                                length_field(ui, "coil_size", size, units);
                                ui.end_row();
                            }
                            _ => {}
                        }
                        row_label(ui, "Operation");
                        egui::ComboBox::from_id_salt("prim_op").selected_text("New Body").show_ui(ui, |ui| {
                            let _ = ui.selectable_label(true, "New Body");
                        });
                        ui.end_row();
                    }
                    if let Some(e) = &preview_err {
                        ui.colored_label(theme::ERROR, "⚠");
                        ui.colored_label(theme::ERROR, e);
                        ui.end_row();
                    }
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Extrude { sketch, profiles, distance, symmetric, taper, op, .. } => {
                let ok = sketch.is_some() && !profiles.is_empty() && preview_err.is_none();
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Profiles");
                    ui.horizontal(|ui| {
                        let text = if profiles.is_empty() { "Select".to_string() } else { format!("{} selected", profiles.len()) };
                        if selection_field(ui, true, &text).clicked() {
                            profiles.clear();
                        }
                        if let Some(sk) = *sketch {
                            if ui.button("All").on_hover_text("Select all solid profiles of this sketch (e.g. text)").clicked() {
                                if let Some((_, ents)) = self.tabs[self.active].sketch_entities(sk) {
                                    *profiles = m360_core::sketch::solid_profiles(&ents);
                                }
                            }
                        }
                    });
                    ui.end_row();
                    row_label(ui, "Start");
                    ui.label("Profile Plane");
                    ui.end_row();
                    row_label(ui, "Direction");
                    egui::ComboBox::from_id_salt("ex_dir").selected_text(if *symmetric { "Symmetric" } else { "One Side" }).show_ui(ui, |ui| {
                        ui.selectable_value(symmetric, false, "One Side");
                        ui.selectable_value(symmetric, true, "Symmetric");
                    });
                    ui.end_row();
                    row_label(ui, "Extent Type");
                    ui.label("Distance");
                    ui.end_row();
                    row_label(ui, "Distance");
                    ui.horizontal(|ui| {
                        length_field(ui, "ex_dist", distance, units);
                        if ui.button("Flip").on_hover_text("Flip direction").clicked() {
                            *distance = -*distance;
                        }
                    });
                    ui.end_row();
                    row_label(ui, "Taper Angle");
                    angle_field(ui, "ex_taper", taper);
                    ui.end_row();
                    row_label(ui, "Operation");
                    egui::ComboBox::from_id_salt("ex_op").selected_text(op.label()).show_ui(ui, |ui| {
                        for o in ExtrudeOp::ALL {
                            ui.selectable_value(op, o, o.label());
                        }
                    });
                    ui.end_row();
                    if let Some(e) = &preview_err {
                        ui.colored_label(theme::ERROR, "⚠");
                        ui.colored_label(theme::ERROR, e);
                        ui.end_row();
                    }
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Combine { target, tools, op, keep_tools, field, .. } => {
                let ok = target.is_some() && !tools.is_empty();
                let names = |id: u64, app: &Self| app.tabs[app.active].doc.body_meta(id).name;
                let target_text = target.map_or("Select".into(), |t| names(t, self));
                let tools_text = if tools.is_empty() { "Select".into() } else { format!("{} selected", tools.len()) };
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Target Body");
                    if selection_field(ui, *field == CombineField::Target, &target_text).clicked() {
                        *field = CombineField::Target;
                    }
                    ui.end_row();
                    row_label(ui, "Tool Bodies");
                    if selection_field(ui, *field == CombineField::Tools, &tools_text).clicked() {
                        *field = CombineField::Tools;
                    }
                    ui.end_row();
                    row_label(ui, "Operation");
                    egui::ComboBox::from_id_salt("cb_op").selected_text(format!("{op:?}")).show_ui(ui, |ui| {
                        for o in [BooleanOp::Join, BooleanOp::Cut, BooleanOp::Intersect] {
                            ui.selectable_value(op, o, format!("{o:?}"));
                        }
                    });
                    ui.end_row();
                    row_label(ui, "Keep Tools");
                    ui.checkbox(keep_tools, "");
                    ui.end_row();
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Move { bodies, translate, axis, angle_deg, copy, .. } => {
                let ok = !bodies.is_empty();
                let text = if bodies.is_empty() { "Select".into() } else { format!("{} Bodies", bodies.len()) };
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Move Object");
                    ui.label("Bodies");
                    ui.end_row();
                    row_label(ui, "Objects");
                    if selection_field(ui, true, &text).clicked() {
                        bodies.clear();
                    }
                    ui.end_row();
                    row_label(ui, "Move Type");
                    ui.label("Translate + Rotate");
                    ui.end_row();
                    for (i, name) in ["X Distance", "Y Distance", "Z Distance"].iter().enumerate() {
                        row_label(ui, name);
                        length_field(ui, ("mv", i), &mut translate[i], units);
                        ui.end_row();
                    }
                    row_label(ui, "Rotation Axis");
                    egui::ComboBox::from_id_salt("mv_axis").selected_text(format!("{axis:?}")).show_ui(ui, |ui| {
                        for a in [Axis::X, Axis::Y, Axis::Z] {
                            ui.selectable_value(axis, a, format!("{a:?}"));
                        }
                    });
                    ui.end_row();
                    row_label(ui, "Angle");
                    angle_field(ui, "mv_ang", angle_deg);
                    ui.end_row();
                    row_label(ui, "Create Copy");
                    ui.checkbox(copy, "");
                    ui.end_row();
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Revolve { sketch, profiles, axis, angle_deg, op, picking_axis, .. } => {
                let ok = sketch.is_some() && !profiles.is_empty() && axis.is_some() && preview_err.is_none();
                let lines: Vec<usize> = sketch
                    .and_then(|sk| self.tabs[self.active].sketch_entities(sk))
                    .map(|(_, ents)| {
                        ents.iter().enumerate().filter(|(_, e)| matches!(e, m360_core::SketchEntity::Line { .. })).map(|(i, _)| i).collect()
                    })
                    .unwrap_or_default();
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Profile");
                    let text = if profiles.is_empty() { "Select".to_string() } else { format!("{} selected", profiles.len()) };
                    if selection_field(ui, !*picking_axis, &text).clicked() {
                        *picking_axis = false;
                    }
                    ui.end_row();
                    row_label(ui, "Axis");
                    ui.horizontal(|ui| {
                        let text = axis.map_or("Select".to_string(), |a| a.label());
                        if selection_field(ui, *picking_axis, &text).clicked() {
                            *picking_axis = true;
                        }
                        egui::ComboBox::from_id_salt("rev_axis").selected_text("").width(20.0).show_ui(ui, |ui| {
                            for a in [m360_core::RevolveAxis::X, m360_core::RevolveAxis::Y, m360_core::RevolveAxis::Z] {
                                if ui.selectable_label(*axis == Some(a), a.label()).clicked() {
                                    *axis = Some(a);
                                    *picking_axis = false;
                                }
                            }
                            for i in &lines {
                                let a = m360_core::RevolveAxis::SketchLine(*i);
                                if ui.selectable_label(*axis == Some(a), a.label()).clicked() {
                                    *axis = Some(a);
                                    *picking_axis = false;
                                }
                            }
                        });
                    });
                    ui.end_row();
                    row_label(ui, "Type");
                    ui.label("Angle");
                    ui.end_row();
                    row_label(ui, "Angle");
                    ui.horizontal(|ui| {
                        angle_field(ui, "rev_angle", angle_deg);
                        if ui.button("Full").clicked() {
                            *angle_deg = 360.0;
                        }
                    });
                    ui.end_row();
                    row_label(ui, "Operation");
                    egui::ComboBox::from_id_salt("rev_op").selected_text(op.label()).show_ui(ui, |ui| {
                        for o in ExtrudeOp::ALL {
                            ui.selectable_value(op, o, o.label());
                        }
                    });
                    ui.end_row();
                    if let Some(e) = &preview_err {
                        ui.colored_label(theme::ERROR, "⚠");
                        ui.colored_label(theme::ERROR, e);
                        ui.end_row();
                    }
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Sweep { sketch, profiles, path_sketch, path, picking_path, op, .. } => {
                let ok = sketch.is_some() && !profiles.is_empty() && path_sketch.is_some() && !path.is_empty() && preview_err.is_none();
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Type");
                    ui.label("Single path");
                    ui.end_row();
                    row_label(ui, "Profile");
                    let t = if profiles.is_empty() { "Select".to_string() } else { format!("{} selected", profiles.len()) };
                    if selection_field(ui, !*picking_path, &t).clicked() {
                        *picking_path = false;
                    }
                    ui.end_row();
                    row_label(ui, "Path");
                    let t = if path.is_empty() { "Select".to_string() } else { format!("{} curves", path.len()) };
                    if selection_field(ui, *picking_path, &t).clicked() {
                        *picking_path = true;
                    }
                    ui.end_row();
                    row_label(ui, "Operation");
                    egui::ComboBox::from_id_salt("sw_op").selected_text(op.label()).show_ui(ui, |ui| {
                        for o in ExtrudeOp::ALL {
                            ui.selectable_value(op, o, o.label());
                        }
                    });
                    ui.end_row();
                    if let Some(e) = &preview_err {
                        ui.colored_label(theme::ERROR, "⚠");
                        ui.colored_label(theme::ERROR, e);
                        ui.end_row();
                    }
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Pipe { path, diameter, wall, hollow, op, .. } => {
                let ok = !path.is_empty() && preview_err.is_none();
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Path");
                    let t = if path.is_empty() { "Select".to_string() } else { format!("{} curves", path.len()) };
                    if selection_field(ui, true, &t).clicked() {
                        path.clear();
                    }
                    ui.end_row();
                    row_label(ui, "Section");
                    ui.label("Circular");
                    ui.end_row();
                    row_label(ui, "Section Size");
                    length_field(ui, "pipe_d", diameter, units);
                    ui.end_row();
                    row_label(ui, "Hollow");
                    ui.checkbox(hollow, "");
                    ui.end_row();
                    if *hollow {
                        row_label(ui, "Section Thickness");
                        length_field(ui, "pipe_w", wall, units);
                        ui.end_row();
                    }
                    row_label(ui, "Operation");
                    egui::ComboBox::from_id_salt("pipe_op").selected_text(op.label()).show_ui(ui, |ui| {
                        for o in ExtrudeOp::ALL {
                            ui.selectable_value(op, o, o.label());
                        }
                    });
                    ui.end_row();
                    if let Some(e) = &preview_err {
                        ui.colored_label(theme::ERROR, "⚠");
                        ui.colored_label(theme::ERROR, e);
                        ui.end_row();
                    }
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Loft { sections, op, .. } => {
                let ok = sections.len() >= 2 && preview_err.is_none();
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Profiles");
                    let t = if sections.is_empty() { "Select".to_string() } else { format!("{} profiles", sections.len()) };
                    if selection_field(ui, true, &t).clicked() {
                        sections.clear();
                    }
                    ui.end_row();
                    row_label(ui, "");
                    ui.label(egui::RichText::new("Select profiles in order").small().color(theme::TEXT_DIM));
                    ui.end_row();
                    row_label(ui, "Operation");
                    egui::ComboBox::from_id_salt("loft_op").selected_text(op.label()).show_ui(ui, |ui| {
                        for o in ExtrudeOp::ALL {
                            ui.selectable_value(op, o, o.label());
                        }
                    });
                    ui.end_row();
                    if let Some(e) = &preview_err {
                        ui.colored_label(theme::ERROR, "⚠");
                        ui.colored_label(theme::ERROR, e);
                        ui.end_row();
                    }
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Interference { bodies, results } => {
                let names: Vec<String> = bodies.iter().map(|b| self.tabs[self.active].doc.body_meta(*b).name).collect();
                let mut compute = false;
                let r = command_dialog(ctx, title, anchor, true, |ui| {
                    row_label(ui, "Bodies");
                    let t = if bodies.is_empty() { "Select".to_string() } else { format!("{} selected", bodies.len()) };
                    if selection_field(ui, bodies.is_empty(), &t).clicked() {
                        bodies.clear();
                        *results = None;
                    }
                    ui.end_row();
                    ui.label("");
                    compute = ui.add_enabled(bodies.len() >= 2, egui::Button::new("Compute")).clicked();
                    ui.end_row();
                    if let Some(rs) = results {
                        if rs.is_empty() {
                            ui.label("");
                            ui.label(RichText::new("No interference").color(theme::SUCCESS));
                            ui.end_row();
                        }
                        let name = |id: u64| bodies.iter().position(|b| *b == id).map_or(String::new(), |k| names[k].clone());
                        for i in rs.iter() {
                            ui.colored_label(theme::ERROR, "⚠");
                            ui.label(format!("{} ↔ {}: {} mm³", name(i.a), name(i.b), (i.volume * 100.0).round() / 100.0));
                            ui.end_row();
                        }
                    }
                });
                if compute {
                    let res = crate::state::interferences(&self.tabs[self.active].model, bodies);
                    *results = Some(res);
                }
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Thread { body, cyl, size, pitch, full_length, length, .. } => {
                use crate::state::ISO_THREADS;
                let ok = body.is_some() && cyl.is_some() && *pitch > 1e-3;
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Faces");
                    let t = match cyl {
                        Some(c) => format!("1 selected ({})", if c.internal { "internal" } else { "external" }),
                        None => "Select".into(),
                    };
                    if selection_field(ui, cyl.is_none(), &t).clicked() {
                        *cyl = None;
                        *body = None;
                    }
                    ui.end_row();
                    row_label(ui, "Thread Type");
                    ui.label("ISO Metric profile");
                    ui.end_row();
                    row_label(ui, "Size");
                    let before = *size;
                    egui::ComboBox::from_id_salt("thread_size").selected_text(ISO_THREADS[*size].0).show_ui(ui, |ui| {
                        for (i, t) in ISO_THREADS.iter().enumerate() {
                            ui.selectable_value(size, i, t.0);
                        }
                    });
                    if *size != before {
                        *pitch = ISO_THREADS[*size].2;
                    }
                    ui.end_row();
                    row_label(ui, "Pitch");
                    length_field(ui, "thread_pitch", pitch, units);
                    ui.end_row();
                    row_label(ui, "Full Length");
                    ui.checkbox(full_length, "");
                    ui.end_row();
                    if !*full_length {
                        row_label(ui, "Length");
                        length_field(ui, "thread_len", length, units);
                        ui.end_row();
                    }
                    row_label(ui, "Modeled");
                    ui.add_enabled(false, egui::Checkbox::new(&mut false, "")).on_disabled_hover_text("Cosmetic threads only for now");
                    ui.end_row();
                    if let Some(c) = cyl {
                        let nominal = ISO_THREADS[*size].1;
                        let face_d = 2.0 * c.radius;
                        let expect = if c.internal { nominal - 1.0825 * ISO_THREADS[*size].2 } else { nominal };
                        if (face_d - expect).abs() > 0.5 {
                            ui.colored_label(theme::ERROR, "⚠");
                            ui.colored_label(theme::ERROR, format!("Face ⌀ {} does not match {}", units.format(face_d), ISO_THREADS[*size].0));
                            ui.end_row();
                        }
                    }
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Shell { body, faces, thickness, .. } => {
                let ok = body.is_some() && preview_err.is_none();
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Faces/Body");
                    let t = match (body.is_some(), faces.len()) {
                        (false, _) => "Select".to_string(),
                        (true, 0) => "1 Body (closed)".to_string(),
                        (true, n) => format!("{n} Faces removed"),
                    };
                    if selection_field(ui, true, &t).clicked() {
                        faces.clear();
                        *body = None;
                    }
                    ui.end_row();
                    row_label(ui, "Inside Thickness");
                    length_field(ui, "shell_t", thickness, units);
                    ui.end_row();
                    row_label(ui, "Direction");
                    ui.label("Inside");
                    ui.end_row();
                    if let Some(e) = &preview_err {
                        ui.colored_label(theme::ERROR, "⚠");
                        ui.colored_label(theme::ERROR, e);
                        ui.end_row();
                    }
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::SplitBody { body, plane, picking_plane, .. } => {
                let ok = body.is_some() && plane.is_some();
                let bname = body.map_or("Select".to_string(), |b| self.tabs[self.active].doc.body_meta(b).name);
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Body to Split");
                    if selection_field(ui, !*picking_plane, &bname).clicked() {
                        *picking_plane = false;
                    }
                    ui.end_row();
                    row_label(ui, "Splitting Tool");
                    let t = plane.map_or("Select".to_string(), |p| p.label());
                    if selection_field(ui, *picking_plane, &t).clicked() {
                        *picking_plane = true;
                    }
                    ui.end_row();
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Scale { bodies, point, factor, .. } => {
                let ok = !bodies.is_empty();
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Entities");
                    let t = if bodies.is_empty() { "Select".to_string() } else { format!("{} Bodies", bodies.len()) };
                    if selection_field(ui, true, &t).clicked() {
                        bodies.clear();
                    }
                    ui.end_row();
                    for (i, n) in ["Point X", "Point Y", "Point Z"].iter().enumerate() {
                        row_label(ui, n);
                        length_field(ui, ("sc_p", i), &mut point[i], units);
                        ui.end_row();
                    }
                    row_label(ui, "Scale Factor");
                    ui.add(egui::DragValue::new(factor).speed(0.01).range(0.001..=1000.0));
                    ui.end_row();
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Joint { b, a, motion, .. } => {
                let ok = b.is_some() && a.is_some();
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Component 1");
                    let t = b.as_ref().map_or("Select face".to_string(), |(bs, _)| format!("{} bodies", bs.len()));
                    if selection_field(ui, b.is_none(), &t).clicked() {
                        *b = None;
                        *a = None;
                    }
                    ui.end_row();
                    row_label(ui, "Component 2");
                    let t = if a.is_some() { "1 face".to_string() } else { "Select face".to_string() };
                    if selection_field(ui, b.is_some() && a.is_none(), &t).clicked() {
                        *a = None;
                    }
                    ui.end_row();
                    row_label(ui, "Motion");
                    egui::ComboBox::from_id_salt("joint_motion").selected_text(motion.label()).show_ui(ui, |ui| {
                        for m in [m360_core::JointMotion::Rigid, m360_core::JointMotion::Revolute { angle_deg: 0.0 }, m360_core::JointMotion::Slider { offset: 0.0 }] {
                            if ui.selectable_label(motion.label() == m.label(), m.label()).clicked() {
                                *motion = m;
                            }
                        }
                    });
                    ui.end_row();
                    match motion {
                        m360_core::JointMotion::Rigid => {}
                        m360_core::JointMotion::Revolute { angle_deg } => {
                            row_label(ui, "Angle");
                            angle_field(ui, "joint_angle", angle_deg);
                            ui.end_row();
                        }
                        m360_core::JointMotion::Slider { offset } => {
                            row_label(ui, "Distance");
                            length_field(ui, "joint_offset", offset, units);
                            ui.end_row();
                        }
                    }
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::OffsetPlane { base, distance, .. } => {
                let ok = base.is_some();
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Plane");
                    let text = base.map_or("Select".to_string(), |p| p.label());
                    if selection_field(ui, base.is_none(), &text).clicked() {
                        *base = None;
                    }
                    ui.end_row();
                    row_label(ui, "Distance");
                    length_field(ui, "off_plane", distance, units);
                    ui.end_row();
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::RefPlane { method, picks, angle } => {
                use crate::state::{ref_plane, RefPick, RefPlaneMethod};
                let ok = ref_plane(*method, picks, *angle).is_some();
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    let names: &[&str] = match method {
                        RefPlaneMethod::Angle => &["Line"],
                        RefPlaneMethod::Mid => &["Plane 1", "Plane 2"],
                        RefPlaneMethod::ThreePoints => &["Point 1", "Point 2", "Point 3"],
                    };
                    for (k, name) in names.iter().enumerate() {
                        row_label(ui, name);
                        let text = match picks.get(k) {
                            Some(RefPick::Plane(p)) => p.label(),
                            Some(RefPick::Line { a, b, .. }) => format!("Line ({})", units.format((*b - *a).length())),
                            Some(RefPick::Point(p)) => format!("({:.1}, {:.1}, {:.1})", p.x, p.y, p.z),
                            None => "Select".into(),
                        };
                        if selection_field(ui, picks.len() == k, &text).clicked() {
                            picks.truncate(k);
                        }
                        ui.end_row();
                    }
                    if *method == RefPlaneMethod::Angle {
                        row_label(ui, "Angle");
                        angle_field(ui, "plane_angle", angle);
                        ui.end_row();
                    }
                    if *method == RefPlaneMethod::ThreePoints {
                        ui.label("");
                        ui.label(RichText::new("Click near the ends of edges").small().color(theme::TEXT_DIM));
                        ui.end_row();
                    }
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Section { plane, offset, flip } => {
                let ok = plane.is_some();
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Face / Plane");
                    let text = plane.map_or("Select".to_string(), |p| p.label());
                    if selection_field(ui, plane.is_none(), &text).clicked() {
                        *plane = None;
                    }
                    ui.end_row();
                    row_label(ui, "Distance");
                    length_field(ui, "sec_off", offset, units);
                    ui.end_row();
                    row_label(ui, "Flip");
                    ui.checkbox(flip, "");
                    ui.end_row();
                });
                // Live section while the dialog is open.
                let live = plane.map(|p| crate::state::SectionState {
                    plane: m360_core::PlaneRef { offset: p.offset + if p.flip { -*offset } else { *offset }, ..p },
                    flip: *flip,
                    visible: true,
                });
                self.tab().section = live;
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Pattern { bodies, pattern, .. } => {
                let ok = !bodies.is_empty();
                let text = if bodies.is_empty() { "Select".to_string() } else { format!("{} Bodies", bodies.len()) };
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Pattern Type");
                    ui.label("Bodies");
                    ui.end_row();
                    row_label(ui, "Objects");
                    if selection_field(ui, true, &text).clicked() {
                        bodies.clear();
                    }
                    ui.end_row();
                    let axis_combo = |ui: &mut egui::Ui, id: &str, a: &mut Axis| {
                        egui::ComboBox::from_id_salt(id.to_string()).selected_text(format!("{a:?} Axis")).show_ui(ui, |ui| {
                            for x in [Axis::X, Axis::Y, Axis::Z] {
                                ui.selectable_value(a, x, format!("{x:?} Axis"));
                            }
                        });
                    };
                    match pattern {
                        m360_core::PatternKind::Rectangular { dir1, count1, spacing1, dir2, count2, spacing2 } => {
                            row_label(ui, "Direction 1");
                            axis_combo(ui, "pd1", dir1);
                            ui.end_row();
                            row_label(ui, "Quantity");
                            ui.add(egui::DragValue::new(count1).range(1..=100));
                            ui.end_row();
                            row_label(ui, "Spacing");
                            length_field(ui, "ps1", spacing1, units);
                            ui.end_row();
                            row_label(ui, "Direction 2");
                            axis_combo(ui, "pd2", dir2);
                            ui.end_row();
                            row_label(ui, "Quantity");
                            ui.add(egui::DragValue::new(count2).range(1..=100));
                            ui.end_row();
                            row_label(ui, "Spacing");
                            length_field(ui, "ps2", spacing2, units);
                            ui.end_row();
                        }
                        m360_core::PatternKind::Circular { axis, count, total_deg } => {
                            row_label(ui, "Axis");
                            axis_combo(ui, "pca", axis);
                            ui.end_row();
                            row_label(ui, "Quantity");
                            ui.add(egui::DragValue::new(count).range(1..=360));
                            ui.end_row();
                            row_label(ui, "Total Angle");
                            angle_field(ui, "pct", total_deg);
                            ui.end_row();
                        }
                    }
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Mirror { bodies, plane, picking_plane, .. } => {
                let ok = !bodies.is_empty() && plane.is_some();
                let text = if bodies.is_empty() { "Select".to_string() } else { format!("{} Bodies", bodies.len()) };
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Pattern Type");
                    ui.label("Bodies");
                    ui.end_row();
                    row_label(ui, "Objects");
                    if selection_field(ui, !*picking_plane, &text).clicked() {
                        *picking_plane = false;
                    }
                    ui.end_row();
                    row_label(ui, "Mirror Plane");
                    let ptext = plane.map_or("Select".to_string(), |p| p.label());
                    if selection_field(ui, *picking_plane, &ptext).clicked() {
                        *picking_plane = true;
                    }
                    ui.end_row();
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Hole { plane, points, diameter, depth, through_all, kind, .. } => {
                let ok = plane.is_some() && !points.is_empty() && preview_err.is_none();
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Placement");
                    ui.label("Single / multiple points");
                    ui.end_row();
                    row_label(ui, "Face / Points");
                    let text = if points.is_empty() { "Select".to_string() } else { format!("{} Points", points.len()) };
                    if selection_field(ui, true, &text).clicked() {
                        points.clear();
                        *plane = None;
                    }
                    ui.end_row();
                    row_label(ui, "Extents");
                    egui::ComboBox::from_id_salt("hole_ext").selected_text(if *through_all { "All" } else { "Distance" }).show_ui(ui, |ui| {
                        ui.selectable_value(through_all, false, "Distance");
                        ui.selectable_value(through_all, true, "All");
                    });
                    ui.end_row();
                    row_label(ui, "Hole Type");
                    egui::ComboBox::from_id_salt("hole_type").selected_text(kind.label()).show_ui(ui, |ui| {
                        let d = *diameter;
                        for k in [
                            m360_core::HoleType::Simple,
                            m360_core::HoleType::Counterbore { diameter: d * 1.8, depth: d * 0.6 },
                            m360_core::HoleType::Countersink { diameter: d * 2.0, angle_deg: 90.0 },
                        ] {
                            if ui.selectable_label(kind.label() == k.label(), k.label()).clicked() {
                                *kind = k;
                            }
                        }
                    });
                    ui.end_row();
                    row_label(ui, "Diameter");
                    length_field(ui, "hole_d", diameter, units);
                    ui.end_row();
                    if !*through_all {
                        row_label(ui, "Depth");
                        length_field(ui, "hole_depth", depth, units);
                        ui.end_row();
                    }
                    match kind {
                        m360_core::HoleType::Simple => {}
                        m360_core::HoleType::Counterbore { diameter, depth } => {
                            row_label(ui, "Counterbore ⌀");
                            length_field(ui, "cb_d", diameter, units);
                            ui.end_row();
                            row_label(ui, "Counterbore Depth");
                            length_field(ui, "cb_depth", depth, units);
                            ui.end_row();
                        }
                        m360_core::HoleType::Countersink { diameter, angle_deg } => {
                            row_label(ui, "Countersink ⌀");
                            length_field(ui, "cs_d", diameter, units);
                            ui.end_row();
                            row_label(ui, "Countersink Angle");
                            angle_field(ui, "cs_a", angle_deg);
                            ui.end_row();
                        }
                    }
                    if let Some(e) = &preview_err {
                        ui.colored_label(theme::ERROR, "⚠");
                        ui.colored_label(theme::ERROR, e);
                        ui.end_row();
                    }
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Fillet { body, edges, size, chamfer, .. } => {
                let ok = body.is_some() && !edges.is_empty() && preview_err.is_none();
                let text = if edges.is_empty() { "Select".to_string() } else { format!("{} Edges", edges.len()) };
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Type");
                    ui.label(if *chamfer { "Equal distance" } else { "Constant Radius" });
                    ui.end_row();
                    row_label(ui, "Edges");
                    if selection_field(ui, true, &text).clicked() {
                        edges.clear();
                        *body = None;
                    }
                    ui.end_row();
                    row_label(ui, if *chamfer { "Distance" } else { "Radius" });
                    length_field(ui, "fillet_size", size, units);
                    ui.end_row();
                    row_label(ui, "Kind");
                    egui::ComboBox::from_id_salt("fillet_kind").selected_text(if *chamfer { "Chamfer" } else { "Fillet" }).show_ui(ui, |ui| {
                        ui.selectable_value(chamfer, false, "Fillet");
                        ui.selectable_value(chamfer, true, "Chamfer");
                    });
                    ui.end_row();
                    if let Some(e) = &preview_err {
                        ui.colored_label(theme::ERROR, "⚠");
                        ui.colored_label(theme::ERROR, e);
                        ui.end_row();
                    }
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::SketchPlane => {
                let r = command_dialog(ctx, title, anchor, false, |ui| {
                    ui.label("Select a plane or a planar face.");
                    ui.end_row();
                    for (name, b) in [("XY Plane", m360_core::BasePlane::XY), ("XZ Plane", m360_core::BasePlane::XZ), ("YZ Plane", m360_core::BasePlane::YZ)] {
                        if ui.button(name).clicked() {
                            self.pending_sketch_plane = Some(m360_core::PlaneRef { base: b, offset: 0.0, flip: false, custom: None });
                        }
                    }
                    ui.end_row();
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Measure { picks } => {
                let picks = picks.clone();
                let r = command_dialog(ctx, title, anchor, true, |ui| {
                    row_label(ui, "Selection");
                    ui.label(if picks.is_empty() { "Select faces or bodies".to_string() } else { format!("{} selected", picks.len()) });
                    ui.end_row();
                    self.measure_rows(ui, &picks, units);
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Appearance => {
                let sel = self.tabs[self.active].selected_bodies();
                let r = command_dialog(ctx, title, anchor, true, |ui| {
                    row_label(ui, "Apply To");
                    ui.label(if sel.is_empty() { "Select bodies".to_string() } else { format!("{} Bodies", sel.len()) });
                    ui.end_row();
                    row_label(ui, "Library");
                    ui.label("Fusion Appearance Library");
                    ui.end_row();
                });
                // Swatch grid in its own area under the dialog.
                self.appearance_swatches(ctx, anchor + vec2(0.0, 150.0), &sel);
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::Parameters => {
                let mut open = true;
                egui::Window::new("Parameters")
                    .open(&mut open)
                    .default_size([520.0, 360.0])
                    .default_pos(self.canvas_rect.center() - egui::vec2(260.0, 180.0))
                    .collapsible(false)
                    .show(ctx, |ui| self.parameters_ui(ui));
                if !open {
                    finish = Some(true);
                }
            }
            Command::DocSettings => {
                let mut u = units;
                let r = command_dialog(ctx, title, anchor, true, |ui| {
                    row_label(ui, "Units");
                    units_combo(ui, &mut u);
                    ui.end_row();
                });
                if u != units {
                    self.tab().edit("Change Units", |d| d.units = u);
                }
                enabled = r.enabled;
                finish = resolve(r);
            }
            Command::CenterOfMass | Command::Properties => {
                let is_com = matches!(cmd, Command::CenterOfMass);
                let tab = &self.tabs[self.active];
                let mut ids = tab.selected_bodies();
                if ids.is_empty() && is_com {
                    ids = tab.model.bodies.iter().filter(|b| tab.body_visible(b.id)).map(|b| b.id).collect();
                }
                let mut vol = 0.0;
                let mut c = DVec3::ZERO;
                let mut area = 0.0;
                let mut min = Vec3::splat(f32::MAX);
                let mut max = Vec3::splat(f32::MIN);
                for id in &ids {
                    if let Some(b) = tab.model.body(*id) {
                        let (v, cc) = mass_props(&b.mesh);
                        vol += v;
                        c += cc * v;
                        area += b.mesh.area();
                        min = min.min(b.mesh.bbox_min);
                        max = max.max(b.mesh.bbox_max);
                    }
                }
                if vol.abs() > 1e-12 {
                    c /= vol;
                }
                self.canvas.com = (is_com && !ids.is_empty()).then(|| c.as_vec3());
                let name = if ids.len() == 1 { tab.doc.body_meta(ids[0]).name } else { format!("{} bodies", ids.len()) };
                let r = command_dialog(ctx, title, anchor, true, |ui| {
                    row_label(ui, "Selection");
                    ui.label(if ids.is_empty() { "Select bodies".into() } else { name });
                    ui.end_row();
                    if !ids.is_empty() {
                        let mm3 = |v: f64| format!("{:.3} {}³", units.from_mm(units.from_mm(units.from_mm(v))), units.suffix());
                        if !is_com {
                            row_label(ui, "Volume");
                            ui.label(mm3(vol));
                            ui.end_row();
                            row_label(ui, "Area");
                            ui.label(format!("{:.3} {}²", units.from_mm(units.from_mm(area)), units.suffix()));
                            ui.end_row();
                            row_label(ui, "Mass (steel)");
                            ui.label(format!("{:.3} g", vol / 1000.0 * 7.85));
                            ui.end_row();
                            let d = max - min;
                            row_label(ui, "Bounding Box");
                            ui.label(format!("{} × {} × {}", units.format(d.x as f64), units.format(d.y as f64), units.format(d.z as f64)));
                            ui.end_row();
                        }
                        row_label(ui, "Center of Mass");
                        ui.label(format!("{}, {}, {}", units.format(c.x), units.format(c.y), units.format(c.z)));
                        ui.end_row();
                    }
                });
                enabled = r.enabled;
                finish = resolve(r);
            }
        }

        if self.script.as_mut().is_some_and(|s| std::mem::take(&mut s.ok)) {
            if enabled {
                finish = Some(true);
            } else {
                eprintln!("script: OK is disabled for {title}");
            }
        }
        match finish {
            Some(true) => self.commit_command(cmd),
            Some(false) => {
                if matches!(cmd, Command::Section { .. }) {
                    self.tab().section = None;
                }
                self.tab().command = Some(cmd);
                self.cancel_command();
            }
            None => {
                let tab = self.tab();
                if tab.command.is_none() {
                    tab.command = Some(cmd);
                }
            }
        }
        if let Some(p) = self.pending_sketch_plane.take() {
            self.tab().command = None;
            self.start_sketch(p);
        }
    }

    fn commit_command(&mut self, cmd: Command) {
        match cmd {
            Command::Primitive { kind, editing, .. } => self.commit_feature(kind, editing),
            Command::Extrude { sketch: Some(sketch), profiles, distance, symmetric, taper, op, editing } => {
                let participants = self.overlapping_bodies(op);
                if matches!(op, ExtrudeOp::Cut | ExtrudeOp::Intersect) && participants.is_empty() {
                    self.toast(crate::app::ToastKind::Warning, "No bodies intersect the extrusion; nothing to cut.");
                }
                let new = editing.is_none();
                self.commit_feature(FeatureKind::Extrude { sketch, profiles, distance, symmetric, op, participants, taper }, editing);
                if new {
                    // Fusion hides a sketch once a feature consumes it.
                    self.tab().doc.sketch_visible.insert(sketch, false);
                }
            }
            Command::Combine { target: Some(target), tools, op, keep_tools, editing, .. } => {
                self.commit_feature(FeatureKind::Combine { target, tools, op, keep_tools }, editing)
            }
            Command::Move { bodies, translate, axis, angle_deg, copy, editing } => {
                self.commit_feature(FeatureKind::Move { bodies, translate, axis, angle_deg, copy }, editing)
            }
            Command::Revolve { sketch: Some(sketch), profiles, axis: Some(axis), angle_deg, op, editing, .. } => {
                let participants = self.overlapping_bodies(op);
                if matches!(op, ExtrudeOp::Cut | ExtrudeOp::Intersect) && participants.is_empty() {
                    self.toast(crate::app::ToastKind::Warning, "No bodies intersect the revolve; nothing to cut.");
                }
                let new = editing.is_none();
                self.commit_feature(FeatureKind::Revolve { sketch, profiles, axis, angle_deg, op, participants }, editing);
                if new {
                    self.tab().doc.sketch_visible.insert(sketch, false);
                }
            }
            Command::Pattern { bodies, pattern, editing } => {
                self.commit_feature(FeatureKind::Pattern { bodies, pattern }, editing);
            }
            Command::Sweep { sketch: Some(sketch), profiles, path_sketch: Some(path_sketch), path, op, editing, .. } => {
                let participants = self.overlapping_bodies(op);
                let new = editing.is_none();
                self.commit_feature(FeatureKind::Sweep { sketch, profiles, path_sketch, path, op, participants }, editing);
                if new {
                    self.tab().doc.sketch_visible.insert(sketch, false);
                }
            }
            Command::Pipe { path_sketch: Some(path_sketch), path, diameter, wall, hollow, op, editing } => {
                let participants = self.overlapping_bodies(op);
                self.commit_feature(FeatureKind::Pipe { path_sketch, path, diameter, wall: if hollow { wall } else { 0.0 }, op, participants }, editing);
            }
            Command::Loft { sections, op, editing } => {
                let participants = self.overlapping_bodies(op);
                self.commit_feature(FeatureKind::Loft { sections, op, participants }, editing);
            }
            Command::Thread { body: Some(body), cyl: Some(c), size, pitch, full_length, length, editing } => {
                self.commit_feature(
                    FeatureKind::Thread {
                        body,
                        origin: c.origin.into(),
                        axis: c.axis.into(),
                        radius: c.radius,
                        face_length: c.length,
                        length: if full_length { 0.0 } else { length },
                        pitch,
                        internal: c.internal,
                        designation: crate::state::ISO_THREADS[size].0.to_string(),
                    },
                    editing,
                );
            }
            Command::Shell { body: Some(body), faces, thickness, editing } => {
                let pts = self.tabs[self.active].model.body(body).map(|b| crate::state::face_points(&b.mesh, &faces)).unwrap_or_default();
                self.commit_feature(FeatureKind::Shell { body, open_points: pts, thickness }, editing);
            }
            Command::SplitBody { body: Some(body), plane: Some(plane), editing, .. } => {
                self.commit_feature(FeatureKind::SplitBody { body, plane }, editing);
            }
            Command::Scale { bodies, point, factor, editing } => {
                self.commit_feature(FeatureKind::Scale { bodies, point, factor }, editing);
            }
            Command::Joint { b: Some((moving, b)), a: Some(a), motion, editing } => {
                self.commit_feature(FeatureKind::Joint { moving, a, b, motion }, editing);
            }
            Command::OffsetPlane { base: Some(base), distance, editing } => {
                let plane = m360_core::PlaneRef { offset: base.offset + if base.flip { -distance } else { distance }, ..base };
                self.commit_feature(FeatureKind::ConstructionPlane { plane }, editing);
            }
            Command::RefPlane { method, picks, angle } => {
                if let Some(plane) = crate::state::ref_plane(method, &picks, angle) {
                    self.commit_feature(FeatureKind::ConstructionPlane { plane }, None);
                }
            }
            Command::Section { .. } => {} // the section stays active (see browser ▸ Analysis)
            Command::Mirror { bodies, plane: Some(plane), editing, .. } => {
                self.commit_feature(FeatureKind::Mirror { bodies, plane }, editing);
            }
            Command::Hole { plane: Some(plane), points, diameter, depth, through_all, kind, editing } => {
                let participants = self.overlapping_bodies(ExtrudeOp::Cut);
                if participants.is_empty() {
                    self.toast(crate::app::ToastKind::Warning, "The hole does not intersect any body.");
                    return;
                }
                self.commit_feature(FeatureKind::Hole { plane, points, diameter, depth, through_all, kind, participants }, editing);
            }
            Command::Fillet { body: Some(body), edges, size, chamfer, editing } => {
                let tab = &self.tabs[self.active];
                let Some(b) = tab.model.body(body) else { return };
                let ends = crate::state::edge_endpoints(&b.mesh, &edges);
                self.commit_feature(FeatureKind::Fillet { body, edges: ends, size, chamfer }, editing)
            }
            _ => {}
        }
        self.tab().preview = None;
        self.tab().selection.clear();
    }

    /// Bodies whose bounds overlap the current preview tool (Join/Cut targets).
    fn overlapping_bodies(&self, op: ExtrudeOp) -> Vec<u64> {
        let tab = &self.tabs[self.active];
        if op == ExtrudeOp::NewBody {
            return Vec::new();
        }
        let Some(m) = tab.preview.as_ref().and_then(|p| p.mesh.as_ref()) else { return Vec::new() };
        let pad = Vec3::splat(0.01);
        tab.model
            .bodies
            .iter()
            .filter(|b| tab.body_visible(b.id))
            .filter(|b| {
                let (a0, a1) = (b.mesh.bbox_min - pad, b.mesh.bbox_max + pad);
                a0.cmple(m.bbox_max).all() && m.bbox_min.cmple(a1).all()
            })
            .map(|b| b.id)
            .collect()
    }

    fn measure_rows(&self, ui: &mut egui::Ui, picks: &[SelItem], units: Units) {
        let tab = &self.tabs[self.active];
        for p in picks.iter().take(2) {
            match p {
                SelItem::Face(b, f) | SelItem::FacePlane(b, f, _) => {
                    if let Some(body) = tab.model.body(*b) {
                        row_label(ui, "Face Area");
                        ui.label(format!("{:.3} {}²", units.from_mm(units.from_mm(face_area(&body.mesh, *f))), units.suffix()));
                        ui.end_row();
                    }
                }
                SelItem::Edge(b, e) => {
                    if let Some(pts) = tab.model.body(*b).and_then(|bd| bd.mesh.edges.get(*e as usize)) {
                        let len: f64 = pts.windows(2).map(|w| (Vec3::from(w[1]) - Vec3::from(w[0])).length() as f64).sum();
                        row_label(ui, "Edge Length");
                        ui.label(units.format(len));
                        ui.end_row();
                    }
                }
                SelItem::Body(b) => {
                    if let Some(body) = tab.model.body(*b) {
                        let (v, _) = mass_props(&body.mesh);
                        row_label(ui, "Body Volume");
                        ui.label(format!("{:.3} {}³", units.from_mm(units.from_mm(units.from_mm(v))), units.suffix()));
                        ui.end_row();
                    }
                }
                _ => {}
            }
        }
        if picks.len() == 2 {
            let a = item_points(self, &picks[0]);
            let b = item_points(self, &picks[1]);
            if !a.is_empty() && !b.is_empty() {
                let sa = (a.len() / 600).max(1);
                let sb = (b.len() / 600).max(1);
                let mut best = (f32::MAX, Vec3::ZERO, Vec3::ZERO);
                for p in a.iter().step_by(sa) {
                    for q in b.iter().step_by(sb) {
                        let d = p.distance(*q);
                        if d < best.0 {
                            best = (d, *p, *q);
                        }
                    }
                }
                let delta = best.2 - best.1;
                row_label(ui, "Minimum Distance");
                ui.label(RichText::new(units.format(best.0 as f64)).strong());
                ui.end_row();
                row_label(ui, "Δ X / Y / Z");
                ui.label(format!("{}  {}  {}", units.format(delta.x as f64), units.format(delta.y as f64), units.format(delta.z as f64)));
                ui.end_row();
            }
        }
    }

    fn appearance_swatches(&mut self, ctx: &egui::Context, pos: egui::Pos2, sel: &[u64]) {
        egui::Area::new(egui::Id::new("appearance_swatches")).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(286.0);
                ui.label(RichText::new("In This Design / Library").small().color(theme::TEXT_DIM));
                egui::Grid::new("swatches").spacing([6.0, 6.0]).show(ui, |ui| {
                    for (i, (name, c)) in MATERIALS.iter().enumerate() {
                        let col = Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8);
                        let (r, resp) = ui.allocate_exact_size(vec2(48.0, 48.0), egui::Sense::click());
                        ui.painter().rect_filled(r, 4.0, Color32::WHITE);
                        ui.painter().circle_filled(r.center(), 19.0, col);
                        ui.painter().circle_filled(r.center() - vec2(6.0, 6.0), 5.0, Color32::from_white_alpha(90));
                        if resp.hovered() {
                            ui.painter().rect_stroke(r, 4.0, egui::Stroke::new(2.0, theme::ACCENT), egui::StrokeKind::Inside);
                        }
                        if resp.on_hover_text(*name).clicked() {
                            if sel.is_empty() {
                                self.toast(crate::app::ToastKind::Info, "Select bodies first, then click an appearance.");
                            } else {
                                let ids = sel.to_vec();
                                let color = *c;
                                self.tab().edit("Appearance", |d| {
                                    for id in &ids {
                                        let mut m = d.body_meta(*id);
                                        m.color = color;
                                        d.bodies.insert(*id, m);
                                    }
                                });
                            }
                        }
                        if i % 5 == 4 {
                            ui.end_row();
                        }
                    }
                });
            });
        });
    }

    fn parameters_ui(&mut self, ui: &mut egui::Ui) {
        use m360_core::expr::{eval, eval_params, is_symbolic, UserParam};
        let units = self.tabs[self.active].doc.units;
        let params = self.tabs[self.active].doc.params.clone();
        let (vars, perrs) = eval_params(&params, units);
        egui::ScrollArea::vertical().show(ui, |ui| {
            // ---- user parameters
            ui.horizontal(|ui| {
                ui.label(RichText::new("User Parameters").strong());
                if ui.small_button("+ Add").clicked() {
                    let mut k = params.len() + 1;
                    while params.iter().any(|p| p.name == format!("d{k}")) {
                        k += 1;
                    }
                    let name = format!("d{k}");
                    self.tab().edit("Add Parameter", |d| d.params.push(UserParam { name, expr: "10".into(), comment: String::new() }));
                }
            });
            egui::Grid::new("user_params").striped(true).num_columns(5).spacing([12.0, 4.0]).show(ui, |ui| {
                for h in ["Name", "Expression", "Value", "Comment", ""] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                let mut remove = None;
                for (i, p) in params.iter().enumerate() {
                    let mut name = p.name.clone();
                    let mut expr = p.expr.clone();
                    let mut comment = p.comment.clone();
                    let rn = ui.add(egui::TextEdit::singleline(&mut name).desired_width(80.0));
                    let err = perrs.iter().find(|(n, _)| *n == p.name).map(|e| e.1.clone());
                    let re = ui.add(egui::TextEdit::singleline(&mut expr).desired_width(140.0).text_color(if err.is_some() { theme::ERROR } else { theme::TEXT }));
                    match vars.get(&p.name) {
                        Some(v) => ui.label(units.format(*v)),
                        None => ui.colored_label(theme::ERROR, err.unwrap_or_default()),
                    };
                    let rc = ui.add(egui::TextEdit::singleline(&mut comment).desired_width(120.0));
                    if ui.small_button("✕").on_hover_text("Delete parameter").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                    let valid_name = !name.is_empty() && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                    if (rn.lost_focus() && valid_name && name != p.name) || (re.lost_focus() && expr != p.expr) || (rc.lost_focus() && comment != p.comment) {
                        let old = p.name.clone();
                        self.tab().edit("Change Parameter", |d| {
                            if let Some(q) = d.params.get_mut(i) {
                                q.name = if valid_name { name.clone() } else { q.name.clone() };
                                q.expr = expr;
                                q.comment = comment;
                            }
                            // Renaming a parameter updates expressions that use it.
                            if valid_name && name != old {
                                for f in &mut d.features {
                                    for e in f.exprs.values_mut() {
                                        *e = rename_ident(e, &old, &name);
                                    }
                                }
                                for q in &mut d.params {
                                    q.expr = rename_ident(&q.expr, &old, &name);
                                }
                            }
                        });
                    }
                }
                if let Some(i) = remove {
                    self.tab().edit("Delete Parameter", |d| {
                        d.params.remove(i);
                    });
                }
            });
            ui.add_space(10.0);
            ui.label(RichText::new("Model Parameters").strong());
            ui.label(RichText::new("Type a value or an expression using user parameters (e.g. width / 2 + 3 mm).").small().color(theme::TEXT_DIM));
            let features = self.tabs[self.active].doc.features.clone();
            egui::Grid::new("params").striped(true).num_columns(4).spacing([16.0, 4.0]).show(ui, |ui| {
                for h in ["Feature", "Parameter", "Expression", "Value"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for f in &features {
                    let mut kind = f.kind.clone();
                    for (name, angle, value) in kind.numeric_fields_mut() {
                        let u = if angle { m360_core::Units::Millimeter } else { units };
                        let expr = f.exprs.get(name).cloned();
                        let id = egui::Id::new(("param_expr", f.uid, name));
                        let shown = expr.clone().unwrap_or_else(|| if angle { format!("{:.4}", *value) } else { format!("{:.4}", units.from_mm(*value)) });
                        let mut text: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or(shown.clone());
                        ui.label(&f.name);
                        ui.label(name);
                        let result = eval(&text, u, &vars);
                        let r = ui.add(egui::TextEdit::singleline(&mut text).id(id.with("te")).desired_width(150.0).text_color(if result.is_ok() { theme::TEXT } else { theme::ERROR }));
                        match expr.as_ref().map(|e| eval(e, u, &vars)) {
                            Some(Ok(v)) => ui.label(if angle { format!("{v:.2}") } else { units.format(v) }),
                            Some(Err(e)) => ui.colored_label(theme::ERROR, e),
                            None => ui.label(if angle { format!("{:.2}", *value) } else { units.format(*value) }),
                        };
                        ui.end_row();
                        if r.has_focus() {
                            ui.data_mut(|d| d.insert_temp(id, text.clone()));
                        } else {
                            ui.data_mut(|d| d.remove::<String>(id));
                        }
                        if r.lost_focus() && text != shown {
                            if let Ok(v) = result {
                                let (uid, field, symbolic) = (f.uid, name.to_string(), is_symbolic(&text));
                                let t = text.clone();
                                self.tab().edit("Change Parameter", move |d| {
                                    if let Some(ff) = d.feature_mut(uid) {
                                        if symbolic {
                                            ff.exprs.insert(field.clone(), t);
                                        } else {
                                            ff.exprs.remove(&field);
                                        }
                                        for (n, _, val) in ff.kind.numeric_fields_mut() {
                                            if n == field {
                                                *val = v;
                                            }
                                        }
                                    }
                                });
                            }
                        }
                    }
                }
            });
        });
    }

    fn misc_windows(&mut self, ctx: &egui::Context) {
        if self.flags.about {
            egui::Window::new("About Merge360").collapsible(false).resizable(false).open(&mut self.flags.about).show(ctx, |ui| {
                ui.heading("Merge360");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label("A parametric 3D CAD application written in Rust,");
                ui.label("inspired by the Fusion 360 workflow. Offline-only.");
                ui.add_space(6.0);
                ui.label(RichText::new("Kernel: truck (B-rep) · UI: egui · Rendering: wgpu").color(theme::TEXT_DIM));
            });
        }
        if self.flags.shortcuts {
            egui::Window::new("Keyboard Shortcuts").collapsible(false).open(&mut self.flags.shortcuts).show(ctx, |ui| {
                egui::Grid::new("keys").striped(true).show(ui, |ui| {
                    for (k, d) in [
                        ("S", "Toolbox (search tools)"),
                        ("E", "Extrude"),
                        ("M", "Move/Copy"),
                        ("I", "Measure"),
                        ("A", "Appearance"),
                        ("L / R / C / P", "Line / Rectangle / Circle / Point (sketch)"),
                        ("Enter", "Repeat last command"),
                        ("Esc", "Cancel / clear selection"),
                        ("Delete", "Delete selection"),
                        ("F6", "Fit to view"),
                        ("Ctrl+Z / Ctrl+Y", "Undo / Redo"),
                        ("Ctrl+S / Ctrl+O / Ctrl+N", "Save / Open / New"),
                        ("Middle drag", "Pan"),
                        ("Shift + Middle drag", "Orbit"),
                        ("Wheel", "Zoom at cursor"),
                        ("Right click", "Marking menu"),
                    ] {
                        ui.label(RichText::new(k).strong());
                        ui.label(d);
                        ui.end_row();
                    }
                });
            });
        }
        if let Some(i) = self.flags.confirm_close {
            let name = self.tabs.get(i).map(|t| t.doc.name.clone()).unwrap_or_default();
            let mut decision = None;
            egui::Modal::new(egui::Id::new("confirm_close")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.heading("Save changes?");
                ui.label(format!("\"{name}\" has unsaved changes."));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if theme::primary_button(ui, "Save").clicked() {
                        decision = Some(0);
                    }
                    if theme::secondary_button(ui, "Don't Save").clicked() {
                        decision = Some(1);
                    }
                    if theme::secondary_button(ui, "Cancel").clicked() {
                        decision = Some(2);
                    }
                });
            });
            match decision {
                Some(0) => {
                    self.flags.confirm_close = None;
                    self.active = i;
                    self.actions.push(Action::Save);
                }
                Some(1) => {
                    self.flags.confirm_close = None;
                    self.close_tab(i);
                }
                Some(_) => self.flags.confirm_close = None,
                None => {}
            }
        }
    }
}

fn center_rows(ui: &mut egui::Ui, center: &mut [f64; 2], units: Units) {
    row_label(ui, "Center X");
    length_field(ui, "cx", &mut center[0], units);
    ui.end_row();
    row_label(ui, "Center Y");
    length_field(ui, "cy", &mut center[1], units);
    ui.end_row();
}

/// Replace whole-word occurrences of identifier `old` with `new`.
fn rename_ident(expr: &str, old: &str, new: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let flush = |w: &mut String, out: &mut String| {
        out.push_str(if w == old { new } else { w });
        w.clear();
    };
    for c in expr.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

fn resolve(r: crate::widgets::DialogResult) -> Option<bool> {
    if r.ok {
        Some(true)
    } else if r.cancel {
        Some(false)
    } else {
        None
    }
}
