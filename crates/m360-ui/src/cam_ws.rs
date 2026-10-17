//! MANUFACTURE workspace: milling setups, 2D operations, toolpath display,
//! simulation and G-code post processing.

use eframe::egui::{self, RichText};
use glam::DVec3;
use m360_core::cam::{self, CamOp, CamOpKind, CamSetup, CamTool, Move, Stock};
use m360_render::LineInst;

use crate::app::{Merge360App, ToastKind};
use crate::i18n::tr;
use crate::theme;

/// Toolpath of one operation, or why it failed.
pub type OpPath = Result<Vec<Move>, String>;

/// UI state of the workspace.
#[derive(Default)]
pub struct CamUi {
    pub setup: usize,
    /// Operation being edited.
    pub op: Option<usize>,
    /// Simulation position (0..1 of the whole setup) and playback.
    pub sim: f32,
    pub playing: bool,
    pub show_paths: bool,
    /// Cached toolpaths: (key, per-operation result).
    pub cache: Option<(u64, Vec<OpPath>)>,
}

fn srgb(r: f32, g: f32, b: f32, a: f32) -> [f32; 4] {
    let f = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    [f(r), f(g), f(b), a]
}

impl Merge360App {
    fn cam_setup(&self) -> Option<&CamSetup> {
        self.tabs[self.active].doc.cam.get(self.cam.setup)
    }

    /// Toolpaths of the active setup (recomputed when the setup or the
    /// design changes).
    pub(crate) fn cam_paths(&mut self) -> Vec<OpPath> {
        use std::hash::{Hash, Hasher};
        let tab = &self.tabs[self.active];
        let Some(setup) = tab.doc.cam.get(self.cam.setup) else { return Vec::new() };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{setup:?}").hash(&mut h);
        format!("{:?}{}", tab.doc.features, tab.doc.marker).hash(&mut h);
        let key = h.finish();
        if let Some((k, paths)) = &self.cam.cache {
            if *k == key {
                return paths.clone();
            }
        }
        let paths: Vec<_> = setup.ops.iter().map(|op| cam::toolpath(setup, op, &tab.model)).collect();
        self.cam.cache = Some((key, paths.clone()));
        paths
    }

    fn new_op(&mut self, kind: &str) {
        let tab = &self.tabs[self.active];
        // Default geometry: the last sketch with all its profiles.
        let sketch = tab.model.sketches.last().map(|s| (s.uid, m360_core::sketch::solid_profiles(&s.entities)));
        let tool = CamTool::default();
        let (name, kind) = match (kind, sketch) {
            ("face", _) => ("Face", CamOpKind::Face { stepover: 0.6 }),
            ("contour", Some((sk, p))) => ("2D Contour", CamOpKind::Contour { sketch: sk, profiles: p, depth: 5.0, inside: false }),
            ("pocket", Some((sk, p))) => ("2D Pocket", CamOpKind::Pocket { sketch: sk, profiles: p, depth: 3.0, stepover: 0.4 }),
            ("drill", Some((sk, _))) => ("Drill", CamOpKind::Drill { sketch: sk, depth: 5.0, peck: 2.0 }),
            _ => {
                self.toast(ToastKind::Info, tr("Create a sketch with the geometry to machine first."));
                return;
            }
        };
        let si = self.cam.setup;
        let tab = self.tab();
        if tab.doc.cam.is_empty() {
            tab.doc.cam.push(CamSetup::default());
        }
        let si = si.min(tab.doc.cam.len() - 1);
        let setup = &mut tab.doc.cam[si];
        let n = setup.ops.iter().filter(|o| o.kind.label() == kind.label()).count() + 1;
        setup.ops.push(CamOp { name: format!("{}{n}", tr(name)), kind, tool, stepdown: 2.0, clearance: 5.0 });
        let i = setup.ops.len() - 1;
        tab.dirty = true;
        self.cam.setup = si;
        self.cam.op = Some(i);
        self.cam.show_paths = true;
    }

    /// Toolbar / toolbox / script actions of the workspace.
    pub(crate) fn cam_action(&mut self, id: &str) {
        self.flags.workspace = crate::tools::WS_MANUFACTURE;
        match id {
            "cam_setup" => {
                let tab = self.tab();
                let n = tab.doc.cam.len() + 1;
                tab.doc.cam.push(CamSetup { name: format!("Setup{n}"), ..Default::default() });
                tab.dirty = true;
                self.cam.setup = n - 1;
                self.cam.op = None;
            }
            "cam_face" => self.new_op("face"),
            "cam_contour" => self.new_op("contour"),
            "cam_pocket" => self.new_op("pocket"),
            "cam_drill" => self.new_op("drill"),
            "cam_simulate" => {
                self.cam.playing = !self.cam.playing;
                self.cam.show_paths = true;
                if self.cam.sim >= 1.0 {
                    self.cam.sim = 0.0;
                }
            }
            "cam_post" => self.post_process(),
            _ => {}
        }
    }

    pub(crate) fn manufacture_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(tr("SETUP")).small().strong().color(theme::TEXT_DIM));
            if ui.button(tr("New Setup")).clicked() {
                let tab = self.tab();
                let n = tab.doc.cam.len() + 1;
                tab.doc.cam.push(CamSetup { name: format!("Setup{n}"), ..Default::default() });
                tab.dirty = true;
                self.cam.setup = n - 1;
                self.cam.op = None;
            }
            ui.separator();
            ui.label(RichText::new(tr("MILLING")).small().strong().color(theme::TEXT_DIM));
            for (id, label) in [("face", "Face"), ("contour", "2D Contour"), ("pocket", "2D Pocket"), ("drill", "Drill")] {
                if ui.button(tr(label)).clicked() {
                    self.new_op(id);
                }
            }
            ui.separator();
            ui.label(RichText::new(tr("ACTIONS")).small().strong().color(theme::TEXT_DIM));
            ui.checkbox(&mut self.cam.show_paths, tr("Toolpaths"));
            if ui.button(if self.cam.playing { "⏸" } else { "▶" }).on_hover_text(tr("Simulate")).clicked() {
                self.cam.playing = !self.cam.playing;
                self.cam.show_paths = true;
                if self.cam.sim >= 1.0 {
                    self.cam.sim = 0.0;
                }
            }
            ui.add(egui::Slider::new(&mut self.cam.sim, 0.0..=1.0).show_value(false));
            if theme::primary_button(ui, tr("Post Process")).clicked() {
                self.post_process();
            }
        });
    }

    fn post_process(&mut self) {
        let tab = &self.tabs[self.active];
        let Some(setup) = tab.doc.cam.get(self.cam.setup) else {
            return self.toast(ToastKind::Info, tr("Create a setup and operations first."));
        };
        match cam::gcode(setup, &tab.model) {
            Ok(g) => {
                let name = format!("{}-{}", tab.doc.name, setup.name).replace(['/', '\\', ' '], "_");
                let path = self.default_dir().join(format!("{name}.nc"));
                match std::fs::write(&path, g) {
                    Ok(()) => self.toast(ToastKind::Info, format!("{} {}", tr("G-code saved to"), path.display())),
                    Err(e) => self.toast(ToastKind::Error, format!("{e}")),
                }
            }
            Err(e) => self.toast(ToastKind::Error, e),
        }
    }

    /// The CAM browser (setups and operations) and the operation editor.
    pub(crate) fn cam_browser(&mut self, ctx: &egui::Context) {
        let paths = self.cam_paths();
        let rect = self.canvas_rect;
        let mut select: Option<usize> = None;
        let mut delete: Option<usize> = None;
        let mut select_setup: Option<usize> = None;
        egui::Area::new(egui::Id::new("cam_browser")).fixed_pos(rect.min + egui::vec2(8.0, 8.0)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(250.0);
                ui.label(RichText::new(tr("CAM BROWSER")).small().strong());
                let tab = &self.tabs[self.active];
                if tab.doc.cam.is_empty() {
                    ui.label(RichText::new(tr("Add a setup or an operation from the toolbar.")).color(theme::TEXT_DIM));
                }
                for (si, setup) in tab.doc.cam.iter().enumerate() {
                    let active = si == self.cam.setup;
                    if ui.selectable_label(active, format!("⚙ {}", setup.name)).clicked() {
                        select_setup = Some(si);
                    }
                    if !active {
                        continue;
                    }
                    for (i, op) in setup.ops.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add_space(14.0);
                            let status = match paths.get(i) {
                                Some(Ok(m)) => format!("{:.1} min", cam::machining_time(m, &op.tool)),
                                Some(Err(_)) => "⚠".to_string(),
                                None => String::new(),
                            };
                            if ui.selectable_label(self.cam.op == Some(i), format!("{}   {status}", op.name)).clicked() {
                                select = Some(i);
                            }
                            if ui.small_button("🗑").on_hover_text(tr("Delete")).clicked() {
                                delete = Some(i);
                            }
                        });
                    }
                }
            });
        });
        if let Some(s) = select_setup {
            self.cam.setup = s;
            self.cam.op = None;
        }
        if let Some(i) = select {
            self.cam.op = Some(i);
        }
        if let Some(i) = delete {
            let s = self.cam.setup;
            let tab = self.tab();
            if let Some(setup) = tab.doc.cam.get_mut(s) {
                setup.ops.remove(i);
                tab.dirty = true;
            }
            self.cam.op = None;
        }
        self.cam_op_editor(ctx, &paths);
    }

    fn cam_op_editor(&mut self, ctx: &egui::Context, paths: &[OpPath]) {
        let (s, Some(i)) = (self.cam.setup, self.cam.op) else { return };
        let units = self.tabs[self.active].doc.units;
        let sketches: Vec<(u64, String)> = {
            let tab = &self.tabs[self.active];
            tab.model.sketches.iter().map(|k| (k.uid, tab.doc.feature(k.uid).map_or("Sketch".into(), |f| f.name.clone()))).collect()
        };
        let Some(mut op) = self.tabs[self.active].doc.cam.get(s).and_then(|x| x.ops.get(i)).cloned() else {
            self.cam.op = None;
            return;
        };
        let Some(mut setup_vals) = self.cam_setup().map(|x| (x.name.clone(), x.stock_side, x.stock_top)) else { return };
        let mut open = true;
        let pos = self.canvas_rect.right_top() + egui::vec2(-330.0, 12.0);
        egui::Window::new(tr("Operation")).id(egui::Id::new("cam_op")).default_pos(pos).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            egui::Grid::new("cam_op_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(tr("Setup"));
                ui.text_edit_singleline(&mut setup_vals.0);
                ui.end_row();
                ui.label(tr("Stock side offset"));
                crate::widgets::length_field(ui, "cam_ss", &mut setup_vals.1, units);
                ui.end_row();
                ui.label(tr("Stock top offset"));
                crate::widgets::length_field(ui, "cam_st", &mut setup_vals.2, units);
                ui.end_row();
                ui.separator();
                ui.separator();
                ui.end_row();
                ui.label(tr("Name"));
                ui.text_edit_singleline(&mut op.name);
                ui.end_row();
                ui.label(tr("Type"));
                ui.label(tr(op.kind.label()));
                ui.end_row();
                ui.label(tr("Tool"));
                ui.horizontal(|ui| {
                    ui.label("T");
                    ui.add(egui::DragValue::new(&mut op.tool.number).range(1..=99));
                    ui.label("Ø");
                    crate::widgets::length_field(ui, "cam_td", &mut op.tool.diameter, units);
                });
                ui.end_row();
                ui.label(tr("Spindle speed"));
                ui.add(egui::DragValue::new(&mut op.tool.rpm).range(100.0..=60000.0).suffix(" rpm"));
                ui.end_row();
                ui.label(tr("Cutting feed"));
                ui.add(egui::DragValue::new(&mut op.tool.feed).range(1.0..=20000.0).suffix(" mm/min"));
                ui.end_row();
                ui.label(tr("Plunge feed"));
                ui.add(egui::DragValue::new(&mut op.tool.plunge).range(1.0..=20000.0).suffix(" mm/min"));
                ui.end_row();
                ui.label(tr("Maximum stepdown"));
                crate::widgets::length_field(ui, "cam_sd", &mut op.stepdown, units);
                ui.end_row();
                ui.label(tr("Clearance height"));
                crate::widgets::length_field(ui, "cam_cl", &mut op.clearance, units);
                ui.end_row();
                let sketch_combo = |ui: &mut egui::Ui, sketch: &mut u64| {
                    let name = sketches.iter().find(|x| x.0 == *sketch).map_or("—".to_string(), |x| x.1.clone());
                    egui::ComboBox::from_id_salt("cam_sk").selected_text(name).show_ui(ui, |ui| {
                        for (uid, n) in &sketches {
                            ui.selectable_value(sketch, *uid, n);
                        }
                    });
                };
                match &mut op.kind {
                    CamOpKind::Face { stepover } => {
                        ui.label(tr("Stepover"));
                        ui.add(egui::Slider::new(stepover, 0.1..=0.9).custom_formatter(|v, _| format!("{:.0} %", v * 100.0)));
                        ui.end_row();
                    }
                    CamOpKind::Contour { sketch, profiles, depth, inside } => {
                        ui.label(tr("Sketch"));
                        sketch_combo(ui, sketch);
                        ui.end_row();
                        ui.label(tr("Profiles"));
                        ui.label(format!("{} — {}", profiles.len(), tr("click profiles in the view")));
                        ui.end_row();
                        ui.label(tr("Side"));
                        ui.horizontal(|ui| {
                            ui.selectable_value(inside, false, tr("Outside"));
                            ui.selectable_value(inside, true, tr("Inside"));
                        });
                        ui.end_row();
                        ui.label(tr("Depth"));
                        crate::widgets::length_field(ui, "cam_dp", depth, units);
                        ui.end_row();
                    }
                    CamOpKind::Pocket { sketch, profiles, depth, stepover } => {
                        ui.label(tr("Sketch"));
                        sketch_combo(ui, sketch);
                        ui.end_row();
                        ui.label(tr("Profiles"));
                        ui.label(format!("{} — {}", profiles.len(), tr("click profiles in the view")));
                        ui.end_row();
                        ui.label(tr("Depth"));
                        crate::widgets::length_field(ui, "cam_dp", depth, units);
                        ui.end_row();
                        ui.label(tr("Stepover"));
                        ui.add(egui::Slider::new(stepover, 0.1..=0.9).custom_formatter(|v, _| format!("{:.0} %", v * 100.0)));
                        ui.end_row();
                    }
                    CamOpKind::Drill { sketch, depth, peck } => {
                        ui.label(tr("Sketch"));
                        sketch_combo(ui, sketch);
                        ui.end_row();
                        ui.label(tr("Depth"));
                        crate::widgets::length_field(ui, "cam_dp", depth, units);
                        ui.end_row();
                        ui.label(tr("Pecking depth"));
                        crate::widgets::length_field(ui, "cam_pk", peck, units);
                        ui.end_row();
                    }
                }
            });
            match paths.get(i) {
                Some(Ok(m)) => {
                    let len: f64 = m.windows(2).map(|w| (DVec3::from(w[1].p) - DVec3::from(w[0].p)).length()).sum();
                    ui.label(
                        RichText::new(format!("{}: {:.0} mm · {:.1} min", tr("Toolpath"), len, cam::machining_time(m, &op.tool))).color(theme::TEXT_DIM),
                    );
                }
                Some(Err(e)) => {
                    ui.colored_label(theme::ERROR, format!("⚠ {e}"));
                }
                None => {}
            }
        });
        let tab = self.tab();
        if let Some(setup) = tab.doc.cam.get_mut(s) {
            if (setup.name.clone(), setup.stock_side, setup.stock_top) != setup_vals {
                setup.name = setup_vals.0;
                setup.stock_side = setup_vals.1;
                setup.stock_top = setup_vals.2;
                tab.dirty = true;
            }
            if let Some(o) = setup.ops.get_mut(i) {
                if *o != op {
                    *o = op;
                    tab.dirty = true;
                }
            }
        }
        if !open {
            self.cam.op = None;
        }
    }

    /// Toggle a sketch profile in the edited operation (viewport click).
    pub(crate) fn cam_pick_profile(&mut self, sketch: u64, profile: usize) -> bool {
        let (s, Some(i)) = (self.cam.setup, self.cam.op) else { return false };
        let tab = self.tab();
        let Some(op) = tab.doc.cam.get_mut(s).and_then(|x| x.ops.get_mut(i)) else { return false };
        match &mut op.kind {
            CamOpKind::Contour { sketch: sk, profiles, .. } | CamOpKind::Pocket { sketch: sk, profiles, .. } => {
                if *sk != sketch {
                    *sk = sketch;
                    profiles.clear();
                }
                if let Some(k) = profiles.iter().position(|p| *p == profile) {
                    profiles.remove(k);
                } else {
                    profiles.push(profile);
                }
                tab.dirty = true;
                true
            }
            CamOpKind::Drill { sketch: sk, .. } => {
                *sk = sketch;
                tab.dirty = true;
                true
            }
            CamOpKind::Face { .. } => false,
        }
    }

    /// Stock box, toolpaths and the simulated tool (viewport lines).
    pub(crate) fn cam_scene_lines(&mut self, out: &mut Vec<LineInst>, ctx_time: f64) {
        let paths = self.cam_paths();
        let tab = &self.tabs[self.active];
        let Some(setup) = tab.doc.cam.get(self.cam.setup) else { return };
        let Some(stock) = Stock::of(setup, &tab.model) else { return };
        let w = |p: [f64; 3]| stock.to_world(p).as_vec3();
        // Stock box.
        let (hx, hy, d) = (stock.size[0] / 2.0, stock.size[1] / 2.0, stock.size[2]);
        let c = [[-hx, -hy], [hx, -hy], [hx, hy], [-hx, hy]];
        let col = srgb(0.95, 0.6, 0.15, 0.9);
        for k in 0..4 {
            let (a, b) = (c[k], c[(k + 1) % 4]);
            out.push(LineInst::new(w([a[0], a[1], 0.0]), w([b[0], b[1], 0.0]), col, 1.2));
            out.push(LineInst::new(w([a[0], a[1], -d]), w([b[0], b[1], -d]), col, 1.2));
            out.push(LineInst::new(w([a[0], a[1], 0.0]), w([a[0], a[1], -d]), col, 1.2));
        }
        if !self.cam.show_paths {
            return;
        }
        // All moves of the setup, for the simulation.
        let all: Vec<(Move, f64)> = setup
            .ops
            .iter()
            .zip(&paths)
            .filter_map(|(op, p)| p.as_ref().ok().map(|m| m.iter().map(move |mv| (*mv, op.tool.diameter))))
            .flatten()
            .collect();
        let total: f64 = all.windows(2).map(|x| (DVec3::from(x[1].0.p) - DVec3::from(x[0].0.p)).length()).sum();
        let upto = total * self.cam.sim as f64;
        let mut walked = 0.0;
        let mut tool_at: Option<([f64; 3], f64)> = None;
        for (k, x) in all.windows(2).enumerate() {
            let (a, b) = (x[0].0, x[1].0);
            let seg = (DVec3::from(b.p) - DVec3::from(a.p)).length();
            let done = walked + seg <= upto || self.cam.sim >= 1.0 || self.cam.sim <= 0.0;
            let color = if b.rapid {
                srgb(0.9, 0.2, 0.15, if done { 0.5 } else { 0.15 })
            } else {
                srgb(0.1, 0.35, 0.95, if done { 1.0 } else { 0.25 })
            };
            out.push(LineInst::new(w(a.p), w(b.p), color, if b.rapid { 0.8 } else { 1.3 }));
            if tool_at.is_none() && walked + seg >= upto && self.cam.sim > 0.0 && self.cam.sim < 1.0 {
                let t = ((upto - walked) / seg.max(1e-9)).clamp(0.0, 1.0);
                let p = DVec3::from(a.p).lerp(DVec3::from(b.p), t);
                tool_at = Some((p.to_array(), all[k + 1].1));
            }
            walked += seg;
        }
        // The tool: a cylinder outline standing on the cut point.
        if let Some((p, dia)) = tool_at {
            let r = dia / 2.0;
            let tc = srgb(0.2, 0.2, 0.22, 1.0);
            let n = 24;
            for z in [0.0, 25.0] {
                for k in 0..n {
                    let (a0, a1) = (k as f64 / n as f64 * std::f64::consts::TAU, (k + 1) as f64 / n as f64 * std::f64::consts::TAU);
                    out.push(LineInst::new(
                        w([p[0] + r * a0.cos(), p[1] + r * a0.sin(), p[2] + z]),
                        w([p[0] + r * a1.cos(), p[1] + r * a1.sin(), p[2] + z]),
                        tc,
                        1.5,
                    ));
                }
            }
            for k in 0..4 {
                let a = k as f64 * std::f64::consts::FRAC_PI_2;
                out.push(LineInst::new(w([p[0] + r * a.cos(), p[1] + r * a.sin(), p[2]]), w([p[0] + r * a.cos(), p[1] + r * a.sin(), p[2] + 25.0]), tc, 1.5));
            }
        }
        if self.cam.playing {
            let _ = ctx_time;
            self.cam.sim = (self.cam.sim + 0.0025).min(1.0);
            if self.cam.sim >= 1.0 {
                self.cam.playing = false;
            }
        }
    }
}
