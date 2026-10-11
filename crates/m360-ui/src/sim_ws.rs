//! SIMULATION workspace: static stress study (supports, forces, solve,
//! color-mapped results on the deformed body).

use eframe::egui::{self, Color32, RichText};
use glam::{DVec3, Vec3};
use m360_core::simulation::{self, LoadDir, SimLoad, SimResult, SimStudy};
use m360_render::{srgb, FlatVertex, LineInst};

use crate::app::{Merge360App, ToastKind};
use crate::i18n::tr;
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimPick {
    Fixed,
    Load,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimView {
    VonMises,
    Displacement,
    Safety,
}

impl SimView {
    const ALL: [SimView; 3] = [SimView::VonMises, SimView::Displacement, SimView::Safety];
    fn label(self) -> &'static str {
        match self {
            SimView::VonMises => "Stress (von Mises)",
            SimView::Displacement => "Displacement",
            SimView::Safety => "Safety Factor",
        }
    }
}

pub struct SimUi {
    pub pick: Option<SimPick>,
    pub force: f64,
    pub dir: LoadDir,
    pub view: SimView,
    /// Deformation display scale (multiple of the automatic scale).
    pub deform: f32,
    pub result: Option<Result<SimResult, String>>,
}

impl Default for SimUi {
    fn default() -> Self {
        Self { pick: None, force: 100.0, dir: LoadDir::Normal, view: SimView::VonMises, deform: 1.0, result: None }
    }
}

/// Blue → cyan → green → yellow → red.
fn color_map(t: f64) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0) as f32;
    let stops = [[0.1, 0.2, 0.85], [0.1, 0.75, 0.9], [0.2, 0.8, 0.25], [0.95, 0.85, 0.15], [0.9, 0.15, 0.1]];
    let x = t * 4.0;
    let i = (x.floor() as usize).min(3);
    let f = x - i as f32;
    let (a, b) = (stops[i], stops[i + 1]);
    [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f]
}

impl Merge360App {
    pub(crate) fn sim_action(&mut self, id: &str) {
        self.flags.workspace = crate::tools::WS_SIMULATION;
        match id {
            "sim_study" => {
                let tab = &self.tabs[self.active];
                let body = tab.selected_bodies().first().copied().or_else(|| tab.model.bodies.iter().find(|b| tab.body_visible(b.id)).map(|b| b.id));
                let Some(body) = body else { return self.toast(ToastKind::Info, tr("There is no body to study.")) };
                let tab = self.tab();
                tab.doc.simulation = Some(SimStudy { body, fixed: Vec::new(), loads: Vec::new(), resolution: 30 });
                tab.dirty = true;
                tab.selection.clear();
                self.sim.result = None;
                self.sim.pick = Some(SimPick::Fixed);
            }
            "sim_fixed" => self.sim.pick = Some(SimPick::Fixed),
            "sim_force" => {
                if let Some(st) = &mut self.tab().doc.simulation {
                    st.loads.push(SimLoad { faces: Vec::new(), force: 100.0, dir: LoadDir::Normal });
                }
                self.sim.pick = Some(SimPick::Load);
            }
            "sim_solve" => {
                self.sim.pick = None;
                let tab = &self.tabs[self.active];
                let Some(st) = tab.doc.simulation.clone() else { return self.toast(ToastKind::Info, tr("Create a study first.")) };
                let mut st = st;
                st.loads.retain(|l| !l.faces.is_empty());
                let r = simulation::solve(&st, &tab.model, &tab.doc);
                if let Err(e) = &r {
                    self.toast(ToastKind::Warning, e.clone());
                }
                self.sim.result = Some(r);
            }
            _ => {}
        }
    }

    /// Face clicks while picking supports / loads. Returns true when used.
    pub(crate) fn sim_pick_face(&mut self, body: u64, face: u32) -> bool {
        let Some(pick) = self.sim.pick else { return false };
        let (force, dir) = (self.sim.force, self.sim.dir);
        let tab = self.tab();
        let Some(st) = &mut tab.doc.simulation else { return false };
        if st.body != body {
            return false;
        }
        let Some(b) = tab.model.body(body) else { return false };
        let Some(pt) = m360_core::model::face_sample_points(&b.mesh, &[face]).pop() else { return false };
        let toggle = |list: &mut Vec<[f64; 3]>| {
            let same = |p: &[f64; 3]| m360_core::model::face_at_point(&b.mesh, DVec3::from(*p)) == Some(face);
            if let Some(k) = list.iter().position(same) {
                list.remove(k);
            } else {
                list.push(pt);
            }
        };
        match pick {
            SimPick::Fixed => toggle(&mut st.fixed),
            SimPick::Load => {
                if st.loads.is_empty() {
                    st.loads.push(SimLoad { faces: Vec::new(), force, dir });
                }
                let l = st.loads.last_mut().unwrap();
                toggle(&mut l.faces);
            }
        }
        tab.dirty = true;
        self.sim.result = None;
        true
    }

    pub(crate) fn simulation_toolbar(&mut self, ui: &mut egui::Ui) {
        let units = self.tabs[self.active].doc.units;
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(tr("STUDY")).small().strong().color(theme::TEXT_DIM));
            if ui.button(tr("New Static Stress")).clicked() {
                self.sim_action("sim_study");
            }
            ui.separator();
            ui.label(RichText::new(tr("CONSTRAINTS")).small().strong().color(theme::TEXT_DIM));
            if ui.selectable_label(self.sim.pick == Some(SimPick::Fixed), tr("Fixed")).clicked() {
                self.sim_action("sim_fixed");
            }
            ui.separator();
            ui.label(RichText::new(tr("LOADS")).small().strong().color(theme::TEXT_DIM));
            if ui.selectable_label(self.sim.pick == Some(SimPick::Load), tr("Force")).clicked() {
                self.sim_action("sim_force");
            }
            ui.separator();
            ui.label(RichText::new(tr("SOLVE")).small().strong().color(theme::TEXT_DIM));
            if theme::primary_button(ui, tr("Solve")).clicked() {
                self.sim_action("sim_solve");
            }
            ui.separator();
            ui.label(RichText::new(tr("RESULTS")).small().strong().color(theme::TEXT_DIM));
            egui::ComboBox::from_id_salt("sim_view").selected_text(tr(self.sim.view.label())).show_ui(ui, |ui| {
                for v in SimView::ALL {
                    ui.selectable_value(&mut self.sim.view, v, tr(v.label()));
                }
            });
            ui.label(tr("Deformation"));
            ui.add(egui::Slider::new(&mut self.sim.deform, 0.0..=5.0).max_decimals(1));
            let _ = units;
        });
    }

    /// Study panel: setup summary, load values and results.
    pub(crate) fn sim_panel(&mut self, ctx: &egui::Context) {
        let rect = self.canvas_rect;
        let tab = &self.tabs[self.active];
        let Some(st) = tab.doc.simulation.clone() else { return };
        let body = tab.doc.body_meta(st.body);
        let mat = m360_core::materials::material(body.material.as_deref());
        let mut study = st.clone();
        egui::Area::new(egui::Id::new("sim_panel")).fixed_pos(rect.min + egui::vec2(8.0, 8.0)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(270.0);
                ui.label(RichText::new(tr("STATIC STRESS")).small().strong());
                egui::Grid::new("sim_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label(tr("Body"));
                    ui.label(&body.name);
                    ui.end_row();
                    ui.label(tr("Material"));
                    ui.label(format!("{} (E = {:.0} MPa)", tr(mat.name), mat.youngs));
                    ui.end_row();
                    ui.label(tr("Fixed"));
                    ui.label(format!("{} {}", study.fixed.len(), tr("faces")));
                    ui.end_row();
                    for (i, l) in study.loads.iter_mut().enumerate() {
                        ui.label(format!("{} {}", tr("Force"), i + 1));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut l.force).speed(1.0).suffix(" N"));
                            egui::ComboBox::from_id_salt(("sim_dir", i)).selected_text(tr(l.dir.label())).show_ui(ui, |ui| {
                                for d in LoadDir::ALL {
                                    ui.selectable_value(&mut l.dir, d, tr(d.label()));
                                }
                            });
                        });
                        ui.end_row();
                    }
                    ui.label(tr("Mesh resolution"));
                    ui.add(egui::DragValue::new(&mut study.resolution).range(6..=100));
                    ui.end_row();
                });
                if let Some(pick) = self.sim.pick {
                    let what = if pick == SimPick::Fixed { tr("Click faces to fix") } else { tr("Click faces to load") };
                    ui.label(RichText::new(what).color(theme::ACCENT));
                }
                match &self.sim.result {
                    Some(Ok(r)) => {
                        ui.separator();
                        let s = &r.solution;
                        ui.label(RichText::new(tr("RESULTS")).small().strong());
                        ui.label(format!("{}: {:.2} MPa", tr("Max. von Mises stress"), s.max_vm));
                        ui.label(format!("{}: {:.4} mm", tr("Max. displacement"), s.max_disp));
                        let sf = r.min_safety();
                        let col = if sf < 1.0 { theme::ERROR } else if sf < 2.0 { Color32::from_rgb(200, 140, 0) } else { theme::SUCCESS };
                        ui.colored_label(col, format!("{}: {:.2}", tr("Min. safety factor"), sf));
                        ui.label(RichText::new(format!("{} {} · {} {}", s.elements, tr("elements"), s.iterations, tr("iterations"))).small().color(theme::TEXT_DIM));
                    }
                    Some(Err(e)) => {
                        ui.colored_label(theme::ERROR, format!("⚠ {e}"));
                    }
                    None => {}
                }
            });
        });
        if study != st {
            self.tab().doc.simulation = Some(study);
            self.sim.result = None;
        }
        self.sim_legend(ctx);
    }

    fn sim_legend(&self, ctx: &egui::Context) {
        let Some(Ok(r)) = &self.sim.result else { return };
        let (lo, hi, unit) = match self.sim.view {
            SimView::VonMises => (0.0, r.solution.max_vm, "MPa"),
            SimView::Displacement => (0.0, r.solution.max_disp, "mm"),
            SimView::Safety => (0.0, 8.0, ""),
        };
        let rect = self.canvas_rect;
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("sim_legend")));
        let x = rect.right() - 90.0;
        let (top, h) = (rect.top() + 280.0, 220.0);
        for k in 0..44 {
            let t = 1.0 - k as f64 / 43.0;
            let c = color_map(if self.sim.view == SimView::Safety { 1.0 - t } else { t });
            let y = top + h * k as f32 / 44.0;
            painter.rect_filled(egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(18.0, h / 44.0 + 0.5)), 0.0, Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8));
        }
        for k in 0..=4 {
            let t = 1.0 - k as f64 / 4.0;
            let v = lo + (hi - lo) * t;
            let y = top + h * k as f32 / 4.0;
            painter.text(egui::pos2(x + 24.0, y), egui::Align2::LEFT_CENTER, format!("{v:.3} {unit}"), egui::FontId::proportional(11.0), theme::TEXT);
        }
        painter.text(egui::pos2(x, top - 14.0), egui::Align2::LEFT_CENTER, tr(self.sim.view.label()), egui::FontId::proportional(11.0), theme::TEXT);
    }

    /// Supports, loads and the result color map (scene geometry).
    pub(crate) fn sim_scene(&self, lines: &mut Vec<LineInst>, tris: &mut Vec<FlatVertex>) -> Option<u64> {
        let tab = &self.tabs[self.active];
        let st = tab.doc.simulation.as_ref()?;
        let body = tab.model.body(st.body)?;
        let mesh = &body.mesh;
        let size = (mesh.bbox_max - mesh.bbox_min).length();
        // Support markers (small blue ticks) and load arrows.
        let fixed_faces: Vec<u32> = st.fixed.iter().filter_map(|p| m360_core::model::face_at_point(mesh, DVec3::from(*p))).collect();
        for (t, f) in mesh.triangles() {
            if fixed_faces.contains(&f) {
                let c = (t[0] + t[1] + t[2]) / 3.0;
                let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
                lines.push(LineInst::new(c, c + n * size * 0.02, srgb(0.1, 0.35, 0.95, 1.0), 2.0));
            }
        }
        for l in &st.loads {
            for p in &l.faces {
                let Some(f) = m360_core::model::face_at_point(mesh, DVec3::from(*p)) else { continue };
                let n = mesh.planar_normal(f).unwrap_or(Vec3::Y);
                let dir = match l.dir {
                    LoadDir::Normal => -n,
                    LoadDir::X => Vec3::X,
                    LoadDir::Y => Vec3::Y,
                    LoadDir::Z => Vec3::Z,
                } * l.force.signum() as f32;
                let tip = DVec3::from(*p).as_vec3();
                let tail = tip - dir * size * 0.15;
                let red = srgb(0.9, 0.15, 0.1, 1.0);
                lines.push(LineInst::new(tail, tip, red, 2.5));
                let side = dir.any_orthonormal_vector() * size * 0.02;
                lines.push(LineInst::new(tip, tip - dir * size * 0.04 + side, red, 2.5));
                lines.push(LineInst::new(tip, tip - dir * size * 0.04 - side, red, 2.5));
            }
        }
        let Some(Ok(r)) = &self.sim.result else { return None };
        let s = &r.solution;
        // Exaggerate the deformation so that it is visible (Fusion-style).
        let auto = if s.max_disp > 1e-12 { size as f64 * 0.05 / s.max_disp } else { 0.0 };
        let scale = auto * self.sim.deform as f64;
        let light = Vec3::new(0.4, 0.8, 0.45).normalize();
        // Subdivide the surface to about the element size so that the colors
        // follow the field instead of blending across large triangles.
        let target = size / st.resolution.max(4) as f32;
        let tri_count = mesh.indices.len() / 3;
        let budget = (300_000 / tri_count.max(1)).max(1);
        for (t, _) in mesh.triangles() {
            let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
            let shade = 0.6 + 0.4 * n.dot(light).abs();
            let longest = (t[1] - t[0]).length().max((t[2] - t[1]).length()).max((t[0] - t[2]).length());
            let k = ((longest / target).ceil() as usize).clamp(1, (budget as f64).sqrt().max(1.0) as usize);
            let at = |i: usize, j: usize| t[0] + (t[1] - t[0]) * (i as f32 / k as f32) + (t[2] - t[0]) * (j as f32 / k as f32);
            let mut sub: Vec<[Vec3; 3]> = Vec::with_capacity(k * k);
            for i in 0..k {
                for j in 0..k - i {
                    sub.push([at(i, j), at(i + 1, j), at(i, j + 1)]);
                    if i + j + 1 < k {
                        sub.push([at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)]);
                    }
                }
            }
            for p in sub.into_iter().flatten() {
                let pd = p.as_dvec3();
                let v = match self.sim.view {
                    SimView::VonMises => s.stress_at(pd) / s.max_vm.max(1e-12),
                    SimView::Displacement => s.displacement_at(pd).length() / s.max_disp.max(1e-12),
                    SimView::Safety => 1.0 - (r.material.yield_strength / s.stress_at(pd).max(1e-9) / 8.0).min(1.0),
                };
                let c = color_map(v);
                let q = pd + s.displacement_at(pd) * scale;
                tris.push(FlatVertex::new(q.as_vec3(), srgb(c[0] * shade, c[1] * shade, c[2] * shade, 1.0)));
            }
        }
        Some(st.body)
    }
}
