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
                        FeatureKind::Box { plane, .. } | FeatureKind::Cylinder { plane, .. } | FeatureKind::Sphere { plane, .. } | FeatureKind::Torus { plane, .. } => {
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
            Command::Extrude { sketch, profiles, distance, symmetric, op, .. } => {
                let ok = sketch.is_some() && !profiles.is_empty() && preview_err.is_none();
                let r = command_dialog(ctx, title, anchor, ok, |ui| {
                    row_label(ui, "Profiles");
                    let text = if profiles.is_empty() { "Select".to_string() } else { format!("{} selected", profiles.len()) };
                    if selection_field(ui, true, &text).clicked() {
                        profiles.clear();
                    }
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
                            self.pending_sketch_plane = Some(m360_core::PlaneRef { base: b, offset: 0.0, flip: false });
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
            Command::Extrude { sketch: Some(sketch), profiles, distance, symmetric, op, editing } => {
                let participants = self.overlapping_bodies(op);
                if matches!(op, ExtrudeOp::Cut | ExtrudeOp::Intersect) && participants.is_empty() {
                    self.toast(crate::app::ToastKind::Warning, "No bodies intersect the extrusion; nothing to cut.");
                }
                let new = editing.is_none();
                self.commit_feature(FeatureKind::Extrude { sketch, profiles, distance, symmetric, op, participants }, editing);
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
        let units = self.tabs[self.active].doc.units;
        ui.label(RichText::new("Model Parameters").strong());
        ui.separator();
        let features = self.tabs[self.active].doc.features.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("params").striped(true).num_columns(3).spacing([16.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new("Feature").strong());
                ui.label(RichText::new("Parameter").strong());
                ui.label(RichText::new("Value").strong());
                ui.end_row();
                for f in &features {
                    let mut kind = f.kind.clone();
                    let mut changed = false;
                    let mut row = |ui: &mut egui::Ui, name: &str, v: &mut f64, key: &str| {
                        ui.label(&f.name);
                        ui.label(name);
                        if length_field(ui, (f.uid, key), v, units) {
                            changed = true;
                        }
                        ui.end_row();
                    };
                    match &mut kind {
                        FeatureKind::Box { length, width, height, .. } => {
                            row(ui, "Length", length, "l");
                            row(ui, "Width", width, "w");
                            row(ui, "Height", height, "h");
                        }
                        FeatureKind::Cylinder { diameter, height, .. } => {
                            row(ui, "Diameter", diameter, "d");
                            row(ui, "Height", height, "h");
                        }
                        FeatureKind::Sphere { diameter, .. } => row(ui, "Diameter", diameter, "d"),
                        FeatureKind::Torus { inner_diameter, torus_diameter, .. } => {
                            row(ui, "Inner Diameter", inner_diameter, "i");
                            row(ui, "Torus Diameter", torus_diameter, "t");
                        }
                        FeatureKind::Extrude { distance, .. } => row(ui, "Distance", distance, "d"),
                        FeatureKind::Revolve { angle_deg, .. } => {
                            ui.label(&f.name);
                            ui.label("Angle");
                            if angle_field(ui, (f.uid, "a"), angle_deg) {
                                changed = true;
                            }
                            ui.end_row();
                        }
                        FeatureKind::Hole { diameter, depth, .. } => {
                            row(ui, "Diameter", diameter, "hd");
                            row(ui, "Depth", depth, "hp");
                        }
                        FeatureKind::Fillet { size, chamfer, .. } => row(ui, if *chamfer { "Distance" } else { "Radius" }, size, "s"),
                        FeatureKind::Move { translate, .. } => {
                            let [x, y, z] = translate;
                            row(ui, "X Distance", x, "x");
                            row(ui, "Y Distance", y, "y");
                            row(ui, "Z Distance", z, "z");
                        }
                        _ => {}
                    }
                    if changed {
                        let uid = f.uid;
                        self.tab().edit("Change Parameter", |d| {
                            if let Some(ff) = d.feature_mut(uid) {
                                ff.kind = kind;
                            }
                        });
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

fn resolve(r: crate::widgets::DialogResult) -> Option<bool> {
    if r.ok {
        Some(true)
    } else if r.cancel {
        Some(false)
    } else {
        None
    }
}
