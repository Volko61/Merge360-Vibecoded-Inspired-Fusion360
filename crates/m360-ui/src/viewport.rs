//! The 3D canvas: navigation, picking, scene assembly, manipulators.

use std::sync::Arc;

use eframe::egui::{self, Color32, Pos2, Sense};
use glam::{DVec3, Vec2, Vec3};
use m360_core::{BasePlane, ExtrudeOp, FeatureKind, PlaneRef, SketchEntity};
use m360_render::{srgb, BodyDraw, Camera, FlatVertex, LineInst, Ray, Scene};

use crate::app::Merge360App;
use crate::state::{CombineField, Command, DocTab, NavMode, SelItem, SketchTool, VisualStyle};
use crate::theme;

pub const SKETCH_BLUE: [f32; 3] = [0.12, 0.42, 0.85];

/// Interactive drag handle drawn on the canvas.
#[derive(Debug, Clone, Copy)]
pub struct Manip {
    pub origin: Vec3,
    pub dir: Vec3,
    pub color: [f32; 3],
    /// Which parameter it drives (interpreted per command).
    pub slot: usize,
}

#[derive(Default)]
pub struct CanvasState {
    pub manip_drag: Option<usize>,
    pub window_sel: Option<Pos2>,
    pub last_pointer: Option<Pos2>,
    pub cursor_world: Option<Vec3>,
    pub sketch_cursor: Option<[f64; 2]>,
    /// Unsnapped cursor position on the sketch plane.
    pub sketch_raw: Option<[f64; 2]>,
    pub snapped: bool,
    /// Document state at the start of a sketch point drag.
    pub drag_before: Option<m360_core::Document>,
    /// Screen rectangles of constraint/dimension labels: (rect, sketch, index).
    pub labels: Vec<(egui::Rect, u64, usize)>,
    pub com: Option<Vec3>,
}

pub fn ray_triangle(r: &Ray, t: &[Vec3; 3]) -> Option<f32> {
    let e1 = t[1] - t[0];
    let e2 = t[2] - t[0];
    let p = r.dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = r.origin - t[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = r.dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

fn ray_aabb(r: &Ray, min: Vec3, max: Vec3) -> bool {
    let inv = r.dir.recip();
    let t1 = (min - r.origin) * inv;
    let t2 = (max - r.origin) * inv;
    let tmin = t1.min(t2).max_element();
    let tmax = t1.max(t2).min_element();
    tmax >= tmin.max(0.0)
}

pub fn ray_plane(r: &Ray, origin: Vec3, n: Vec3) -> Option<f32> {
    let d = r.dir.dot(n);
    if d.abs() < 1e-9 {
        return None;
    }
    let t = (origin - r.origin).dot(n) / d;
    (t > 0.0).then_some(t)
}

/// Size of the origin-plane squares for the current view.
pub fn origin_plane_size(cam: &Camera) -> f32 {
    cam.distance * 0.18
}

/// Adaptive grid spacing for the camera distance.
pub fn grid_step(cam: &Camera) -> f32 {
    10f32.powf((cam.distance / 15.0).max(1e-3).log10().floor())
}

fn plane_frame(p: &PlaneRef) -> (Vec3, Vec3, Vec3, Vec3) {
    let f = p.frame([0.0, 0.0]);
    (f.origin.as_vec3(), f.u.as_vec3(), f.v.as_vec3(), f.n.as_vec3())
}

fn to_local(p: &PlaneRef, w: Vec3) -> [f64; 2] {
    let f = p.frame([0.0, 0.0]);
    let d = w.as_dvec3() - f.origin;
    [d.dot(f.u), d.dot(f.v)]
}

pub fn to_world(p: &PlaneRef, l: [f64; 2]) -> Vec3 {
    p.frame([0.0, 0.0]).to_world(DVec3::new(l[0], l[1], 0.0)).as_vec3()
}

fn base_plane_ref(b: BasePlane) -> PlaneRef {
    PlaneRef { base: b, offset: 0.0, flip: false }
}

/// Face planarity test: returns a PlaneRef if the face is planar and axis aligned.
fn face_plane(tab: &DocTab, body: u64, face: u32, hit: Vec3) -> Option<PlaneRef> {
    let b = tab.model.body(body)?;
    let m = &b.mesh;
    let mut n0: Option<Vec3> = None;
    for (tri, f) in m.triangles() {
        if f != face {
            continue;
        }
        let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
        if n == Vec3::ZERO {
            continue;
        }
        match n0 {
            None => n0 = Some(n),
            Some(a) if a.dot(n) < 0.9999 => return None,
            _ => {}
        }
    }
    PlaneRef::from_point_normal(hit.as_dvec3(), n0?.as_dvec3())
}

/// Screen distance from point to a segment.
fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t - p).length()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PickMode {
    Default,
    Bodies,
    Planes,
    Profiles,
    Measure,
    Sketch,
    Edges,
    /// Lines of a given sketch (revolve axis).
    Axis(u64),
    /// Lines/arcs of visible sketches (sweep paths).
    Curves,
}

impl Merge360App {
    fn pick_mode(&self) -> PickMode {
        let tab = &self.tabs[self.active];
        if tab.sketch.is_some() {
            return PickMode::Sketch;
        }
        match &tab.command {
            Some(Command::Primitive { plane_chosen: false, .. }) | Some(Command::SketchPlane) => PickMode::Planes,
            Some(Command::Extrude { .. }) => PickMode::Profiles,
            Some(Command::Revolve { picking_axis: true, sketch: Some(sk), .. }) => PickMode::Axis(*sk),
            Some(Command::Revolve { .. }) => PickMode::Profiles,
            Some(Command::Combine { .. })
            | Some(Command::Move { .. })
            | Some(Command::Appearance)
            | Some(Command::CenterOfMass)
            | Some(Command::Properties) => PickMode::Bodies,
            Some(Command::Measure { .. }) => PickMode::Measure,
            Some(Command::Fillet { .. }) => PickMode::Edges,
            Some(Command::Hole { .. }) => PickMode::Planes,
            Some(Command::OffsetPlane { base: None, .. }) | Some(Command::Section { plane: None, .. }) => PickMode::Planes,
            Some(Command::Mirror { picking_plane: true, .. }) => PickMode::Planes,
            Some(Command::Sweep { picking_path: true, .. }) | Some(Command::Pipe { .. }) => PickMode::Curves,
            Some(Command::Sweep { .. }) | Some(Command::Loft { .. }) => PickMode::Profiles,
            Some(Command::SplitBody { picking_plane: true, .. }) => PickMode::Planes,
            Some(Command::SplitBody { .. }) | Some(Command::Scale { .. }) => PickMode::Bodies,
            Some(Command::Mirror { .. }) | Some(Command::Pattern { .. }) => PickMode::Bodies,
            _ => PickMode::Default,
        }
    }

    /// Project a world point to canvas screen coordinates.
    pub(crate) fn to_screen(&self, cam: &Camera, p: Vec3) -> Option<Pos2> {
        let r = self.canvas_rect;
        let ndc = cam.project(p, r.aspect_ratio())?;
        Some(egui::pos2(r.left() + (ndc.x + 1.0) * 0.5 * r.width(), r.top() + (1.0 - ndc.y) * 0.5 * r.height()))
    }

    fn ray_at(&self, cam: &Camera, pos: Pos2) -> Ray {
        let r = self.canvas_rect;
        let ndc = Vec2::new((pos.x - r.left()) / r.width() * 2.0 - 1.0, 1.0 - (pos.y - r.top()) / r.height() * 2.0);
        cam.ray(ndc, r.aspect_ratio())
    }

    fn pick(&mut self, pos: Pos2) -> Option<(SelItem, Vec3)> {
        let mode = self.pick_mode();
        let active = self.active;
        let cam = self.tabs[active].camera;
        let ray = self.ray_at(&cam, pos);
        let mut best: Option<(f32, SelItem)> = None;
        let consider = |t: f32, item: SelItem, best: &mut Option<(f32, SelItem)>| {
            if best.is_none_or(|(bt, _)| t < bt) {
                *best = Some((t, item));
            }
        };

        // Sketch entities (screen space, highest priority in sketch mode).
        if mode == PickMode::Sketch {
            let tab = &self.tabs[active];
            if let Some(s) = &tab.sketch {
                // Constraint / dimension labels.
                if let Some((r, uid, i)) = self.canvas.labels.iter().find(|(r, u, _)| *u == s.uid && r.contains(pos)) {
                    let _ = r;
                    return Some((SelItem::SketchConstraint(*uid, *i), Vec3::ZERO));
                }
                if let Some((plane, ents)) = tab.sketch_entities(s.uid) {
                    // Entity points (only for tools that act on points).
                    let wants_points = matches!(
                        s.tool,
                        SketchTool::Select | SketchTool::Dimension | SketchTool::Constrain(_)
                    );
                    if wants_points {
                        let mut best_p: Option<(f32, m360_core::solver::PointRef)> = None;
                        for e in 0..ents.len() {
                            for p in m360_core::solver::points_of(&ents, e) {
                                let Some(q) = m360_core::solver::point_of(&ents, p) else { continue };
                                if let Some(sp) = self.to_screen(&cam, to_world(&plane, q)) {
                                    let d = (sp - pos).length();
                                    if d < 8.0 && best_p.is_none_or(|b| d < b.0) {
                                        best_p = Some((d, p));
                                    }
                                }
                            }
                        }
                        if let Some((_, p)) = best_p {
                            let q = m360_core::solver::point_of(&ents, p).unwrap();
                            return Some((SelItem::SketchPoint(s.uid, p), to_world(&plane, q)));
                        }
                    }
                    let mut best_d = 7.0;
                    let mut hit = None;
                    for (i, e) in ents.iter().enumerate() {
                        let d = self.entity_screen_dist(&cam, &plane, e, pos);
                        if d < best_d {
                            best_d = d;
                            hit = Some(i);
                        }
                    }
                    if let Some(i) = hit {
                        let w = ray_plane(&ray, plane_frame(&plane).0, plane_frame(&plane).3).map(|t| ray.origin + ray.dir * t).unwrap_or(Vec3::ZERO);
                        return Some((SelItem::SketchEntity(s.uid, i), w));
                    }
                }
            }
        }

        // Sweep paths: lines and arcs of visible sketches.
        if mode == PickMode::Curves {
            let tab = &self.tabs[active];
            let mut best_c: Option<(f32, SelItem, Vec3)> = None;
            for sk in &tab.model.sketches {
                let visible = tab.doc.sketches_folder_visible && tab.doc.sketch_visible.get(&sk.uid).copied().unwrap_or(true);
                if !visible {
                    continue;
                }
                for (i, e) in sk.entities.iter().enumerate() {
                    if !matches!(e, SketchEntity::Line { .. } | SketchEntity::Arc { .. }) {
                        continue;
                    }
                    let d = self.entity_screen_dist(&cam, &sk.plane, e, pos);
                    if d < 8.0 && best_c.as_ref().is_none_or(|b| d < b.0) {
                        let (o, _, _, n) = plane_frame(&sk.plane);
                        let w = ray_plane(&ray, o, n).map(|t| ray.origin + ray.dir * t).unwrap_or(Vec3::ZERO);
                        best_c = Some((d, SelItem::SketchEntity(sk.uid, i), w));
                    }
                }
            }
            return best_c.map(|(_, i, w)| (i, w));
        }

        // Revolve axis: lines of the profile's sketch.
        if let PickMode::Axis(uid) = mode {
            if let Some((plane, ents)) = self.tabs[active].sketch_entities(uid) {
                let hit = ents
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| matches!(e, SketchEntity::Line { .. }))
                    .map(|(i, e)| (i, self.entity_screen_dist(&cam, &plane, e, pos)))
                    .filter(|(_, d)| *d < 8.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                let (o, _, _, n) = plane_frame(&plane);
                let w = ray_plane(&ray, o, n).map(|t| ray.origin + ray.dir * t).unwrap_or(Vec3::ZERO);
                return hit.map(|(i, _)| (SelItem::SketchEntity(uid, i), w));
            }
            return None;
        }

        // Body faces.
        let tab = &self.tabs[active];
        if mode != PickMode::Sketch || tab.sketch.is_none() {
            for b in &tab.model.bodies {
                if !tab.body_visible(b.id) {
                    continue;
                }
                let m = &b.mesh;
                if !ray_aabb(&ray, m.bbox_min - Vec3::splat(1e-3), m.bbox_max + Vec3::splat(1e-3)) {
                    continue;
                }
                for (tri, face) in m.triangles() {
                    if let Some(t) = ray_triangle(&ray, &tri) {
                        let item = match mode {
                            PickMode::Bodies => SelItem::Body(b.id),
                            _ => SelItem::Face(b.id, face),
                        };
                        consider(t, item, &mut best);
                    }
                }
            }
        }

        // Sketch profiles.
        if matches!(mode, PickMode::Default | PickMode::Profiles | PickMode::Sketch) {
            let uids: Vec<u64> = if let Some(s) = &self.tabs[active].sketch {
                vec![s.uid]
            } else {
                let tab = &self.tabs[active];
                tab.model
                    .sketches
                    .iter()
                    .filter(|s| tab.doc.sketches_folder_visible && tab.doc.sketch_visible.get(&s.uid).copied().unwrap_or(true))
                    .map(|s| s.uid)
                    .collect()
            };
            for uid in uids {
                let Some((plane, _)) = self.tabs[active].sketch_entities(uid) else { continue };
                let (o, _, _, n) = plane_frame(&plane);
                let Some(t) = ray_plane(&ray, o, n) else { continue };
                let local = to_local(&plane, ray.origin + ray.dir * t);
                if let Some(cache) = self.tabs[active].sketch_profiles(uid) {
                    if let Some(i) = cache.regions.iter().position(|r| r.contains(local)) {
                        // Profiles win ties with the face they lie on.
                        consider(t - cam.distance * 1e-4, SelItem::Profile(uid, i), &mut best);
                    }
                }
            }
        }

        // Origin planes.
        let tab = &self.tabs[active];
        if mode == PickMode::Planes || (mode == PickMode::Default && tab.doc.origin_visible) {
            let s = origin_plane_size(&cam);
            for b in [BasePlane::XY, BasePlane::XZ, BasePlane::YZ] {
                let p = base_plane_ref(b);
                let (o, _, _, n) = plane_frame(&p);
                if let Some(t) = ray_plane(&ray, o, n) {
                    let l = to_local(&p, ray.origin + ray.dir * t);
                    if l[0].abs() < s as f64 && l[1].abs() < s as f64 {
                        consider(t + cam.distance * 1e-3, SelItem::Plane(b), &mut best);
                    }
                }
            }
        }

        // Construction planes.
        if matches!(mode, PickMode::Planes | PickMode::Default) {
            let s = origin_plane_size(&cam);
            let tab = &self.tabs[active];
            for pg in &tab.model.planes {
                if !tab.doc.plane_visible.get(&pg.uid).copied().unwrap_or(true) {
                    continue;
                }
                let (o, _, _, n) = plane_frame(&pg.plane);
                if let Some(t) = ray_plane(&ray, o, n) {
                    let l = to_local(&pg.plane, ray.origin + ray.dir * t);
                    if l[0].abs() < s as f64 && l[1].abs() < s as f64 {
                        consider(t + cam.distance * 1e-3, SelItem::ConstructionPlane(pg.uid), &mut best);
                    }
                }
            }
        }

        // Edges (screen-space proximity, only if not hidden behind a face).
        if matches!(mode, PickMode::Default | PickMode::Measure | PickMode::Edges) {
            let face_t = best.map(|b| b.0);
            let tab = &self.tabs[active];
            let mut best_edge: Option<(f32, SelItem, Vec3)> = None;
            for b in &tab.model.bodies {
                if !tab.body_visible(b.id) {
                    continue;
                }
                for (ei, pts) in b.mesh.edges.iter().enumerate() {
                    for w in pts.windows(2) {
                        let (pa, pb) = (Vec3::from(w[0]), Vec3::from(w[1]));
                        let (Some(sa), Some(sb)) = (self.to_screen(&cam, pa), self.to_screen(&cam, pb)) else { continue };
                        let d = seg_dist(pos, sa, sb);
                        if d > 6.0 || best_edge.as_ref().is_some_and(|x| x.0 <= d) {
                            continue;
                        }
                        let ab = sb - sa;
                        let k = ((pos - sa).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                        let wp = pa.lerp(pb, k);
                        let te = (wp - ray.origin).dot(ray.dir);
                        if face_t.is_none_or(|ft| te <= ft + cam.distance * 0.004) {
                            best_edge = Some((d, SelItem::Edge(b.id, ei as u32), wp));
                        }
                    }
                }
            }
            if let Some((_, item, wp)) = best_edge {
                return Some((item, wp));
            }
        }

        let (t, item) = best?;
        let hit = ray.origin + ray.dir * t;
        // Upgrade faces to sketchable planes when choosing a plane.
        if mode == PickMode::Planes {
            if let SelItem::Face(b, f) = item {
                return face_plane(&self.tabs[active], b, f, hit).map(|p| (SelItem::FacePlane(b, f, p), hit));
            }
        }
        Some((item, hit))
    }

    fn entity_screen_dist(&self, cam: &Camera, plane: &PlaneRef, e: &SketchEntity, pos: Pos2) -> f32 {
        let s = |l: [f64; 2]| self.to_screen(cam, to_world(plane, l));
        match e {
            SketchEntity::Line { a, b } => match (s(*a), s(*b)) {
                (Some(a), Some(b)) => seg_dist(pos, a, b),
                _ => f32::MAX,
            },
            SketchEntity::Point { p } => s(*p).map_or(f32::MAX, |q| (q - pos).length()),
            SketchEntity::Circle { c, r } => {
                let pts: Vec<Pos2> = (0..=48)
                    .filter_map(|i| {
                        let a = i as f64 / 48.0 * std::f64::consts::TAU;
                        s([c[0] + r * a.cos(), c[1] + r * a.sin()])
                    })
                    .collect();
                pts.windows(2).map(|w| seg_dist(pos, w[0], w[1])).fold(f32::MAX, f32::min)
            }
            SketchEntity::Arc { a, m, b } => {
                let mut pts: Vec<Pos2> = s(*a).into_iter().collect();
                pts.extend(m360_kernel::profile::arc_points(*a, *m, *b, 32).into_iter().filter_map(s));
                pts.windows(2).map(|w| seg_dist(pos, w[0], w[1])).fold(f32::MAX, f32::min)
            }
        }
    }

    // ------------------------------------------------------------------ manipulators

    fn manipulators(&self) -> Vec<Manip> {
        let tab = &self.tabs[self.active];
        let mut out = Vec::new();
        match &tab.command {
            Some(Command::Primitive { kind, plane_chosen: true, .. }) => {
                let green = [0.18, 0.66, 0.31];
                match kind {
                    FeatureKind::Box { plane, center, height, .. } | FeatureKind::Cylinder { plane, center, height, .. } => {
                        let f = plane.frame(*center);
                        let top = f.to_world(DVec3::new(0.0, 0.0, *height)).as_vec3();
                        out.push(Manip { origin: top, dir: f.n.as_vec3() * height.signum() as f32, color: green, slot: 0 });
                    }
                    FeatureKind::Sphere { plane, center, diameter } => {
                        let f = plane.frame(*center);
                        let p = f.to_world(DVec3::new(diameter / 2.0, 0.0, 0.0)).as_vec3();
                        out.push(Manip { origin: p, dir: f.u.as_vec3(), color: green, slot: 0 });
                    }
                    FeatureKind::Torus { plane, center, inner_diameter, .. } => {
                        let f = plane.frame(*center);
                        let p = f.to_world(DVec3::new(inner_diameter / 2.0, 0.0, 0.0)).as_vec3();
                        out.push(Manip { origin: p, dir: f.u.as_vec3(), color: green, slot: 0 });
                    }
                    _ => {}
                }
            }
            Some(Command::Extrude { sketch: Some(sk), profiles, distance, symmetric, .. }) if !profiles.is_empty() => {
                if let Some((plane, ents)) = tab.sketch_entities(*sk) {
                    let regions = m360_core::sketch::profiles(&ents);
                    let mut c = [0.0, 0.0];
                    let mut n = 0.0;
                    for &i in profiles {
                        if let Some(r) = regions.get(i) {
                            let pts = r.outer.points(16);
                            for p in &pts {
                                c[0] += p[0];
                                c[1] += p[1];
                                n += 1.0;
                            }
                        }
                    }
                    if n > 0.0 {
                        c = [c[0] / n, c[1] / n];
                        let f = plane.frame(c);
                        let d = if *symmetric { distance.abs() } else { *distance };
                        let top = f.to_world(DVec3::new(0.0, 0.0, d)).as_vec3();
                        let dir = f.n.as_vec3() * if d < 0.0 { -1.0 } else { 1.0 };
                        out.push(Manip { origin: top, dir, color: [0.18, 0.66, 0.31], slot: 0 });
                    }
                }
            }
            Some(Command::Fillet { body: Some(b), edges, size, .. }) if !edges.is_empty() => {
                if let Some(bd) = tab.model.body(*b) {
                    let i = edges[0] as usize;
                    if let (Some(pts), Some(info)) = (bd.mesh.edges.get(i), bd.mesh.edge_info.get(i)) {
                        if let (Some(n), Some(d)) = (info.normals, info.dirs) {
                            let mid = (Vec3::from(pts[0]) + Vec3::from(pts[pts.len() - 1])) * 0.5;
                            // Handle sits on the first face at the fillet tangent line, pointing into it.
                            let origin = mid + d[0] * *size as f32 + n[0] * 0.0;
                            out.push(Manip { origin, dir: d[0], color: [0.18, 0.66, 0.31], slot: 0 });
                        }
                    }
                }
            }
            Some(Command::Move { bodies, translate, .. }) if !bodies.is_empty() => {
                let mut min = Vec3::splat(f32::MAX);
                let mut max = Vec3::splat(f32::MIN);
                for id in bodies {
                    if let Some(b) = tab.model.body(*id) {
                        min = min.min(b.mesh.bbox_min);
                        max = max.max(b.mesh.bbox_max);
                    }
                }
                if min.x <= max.x {
                    let c = (min + max) * 0.5 + Vec3::from(translate.map(|v| v as f32));
                    out.push(Manip { origin: c, dir: Vec3::X, color: [0.84, 0.23, 0.19], slot: 0 });
                    out.push(Manip { origin: c, dir: Vec3::Y, color: [0.18, 0.66, 0.31], slot: 1 });
                    out.push(Manip { origin: c, dir: Vec3::Z, color: [0.18, 0.43, 0.84], slot: 2 });
                }
            }
            _ => {}
        }
        out
    }

    fn manip_screen(&self, m: &Manip) -> Option<(Pos2, Pos2, f32)> {
        let cam = &self.tabs[self.active].camera;
        let len_world = cam.world_per_pixel(self.canvas_rect.height()) * 70.0;
        let a = self.to_screen(cam, m.origin)?;
        let b = self.to_screen(cam, m.origin + m.dir * len_world)?;
        Some((a, b, len_world))
    }

    fn apply_manip(&mut self, slot: usize, delta: f64) {
        let tab = self.tab();
        match &mut tab.command {
            Some(Command::Primitive { kind, .. }) => match kind {
                FeatureKind::Box { height, .. } | FeatureKind::Cylinder { height, .. } => {
                    let sign = height.signum();
                    *height += delta * sign;
                    if height.abs() < 0.01 {
                        *height = 0.01 * sign;
                    }
                }
                FeatureKind::Sphere { diameter, .. } => *diameter = (*diameter + 2.0 * delta).max(0.01),
                FeatureKind::Torus { inner_diameter, torus_diameter, .. } => {
                    *inner_diameter = (*inner_diameter + 2.0 * delta).max(*torus_diameter + 0.01)
                }
                _ => {}
            },
            Some(Command::Extrude { distance, symmetric, .. }) => {
                if *symmetric {
                    *distance = (distance.abs() + delta).max(0.01);
                } else {
                    let sign = if *distance < 0.0 { -1.0 } else { 1.0 };
                    *distance += delta * sign;
                }
            }
            Some(Command::Move { translate, .. }) => translate[slot] += delta,
            Some(Command::Fillet { size, .. }) => *size = (*size + delta).max(0.01),
            _ => {}
        }
    }

    // ------------------------------------------------------------------ canvas

    pub(crate) fn canvas(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let rect = ui.max_rect();
        self.canvas_rect = rect;
        let now = self.now();
        let aspect = rect.aspect_ratio().max(0.01);

        // Camera animation and fitting.
        {
            let tab = self.tab();
            if let Some((from, to, start)) = tab.anim {
                let t = ((now - start) / 0.35) as f32;
                if t >= 1.0 {
                    tab.camera = to;
                    tab.anim = None;
                } else {
                    tab.camera = Camera::lerp(&from, &to, t.max(0.0));
                }
            }
            if tab.fit_pending {
                tab.fit_pending = false;
                let mut cam = tab.camera;
                let (min, max) = scene_bounds(tab);
                cam.fit(min, max, aspect);
                tab.animate_to(cam, now);
            }
        }

        let resp = ui.interact(rect, ui.id().with("canvas"), Sense::click_and_drag());
        self.handle_input(ui, &resp, aspect);
        self.tab().update_preview();

        // Render.
        let scene = self.build_scene(ui.ctx().pixels_per_point());
        let ppp = ui.ctx().pixels_per_point();
        let size = [(rect.width() * ppp).round() as u32, (rect.height() * ppp).round() as u32];
        match (self.viewport.as_mut(), frame.wgpu_render_state()) {
            (Some(vp), Some(rs)) if size[0] > 0 && size[1] > 0 => {
                let tex = vp.render(rs, size, &scene);
                ui.painter().image(tex, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            _ => {
                ui.painter().rect_filled(rect, 0.0, Color32::from_gray(200));
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "3D viewport unavailable (no wgpu device)", egui::FontId::proportional(14.0), theme::TEXT);
            }
        }
        self.paint_canvas_2d(ui);
    }

    fn handle_input(&mut self, ui: &mut egui::Ui, resp: &egui::Response, aspect: f32) {
        let ctx = ui.ctx().clone();
        let rect = self.canvas_rect;
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        if pointer.is_some() {
            self.canvas.last_pointer = pointer;
        }
        let inside = pointer.is_some_and(|p| rect.contains(p)) && resp.hovered() || resp.dragged();
        let (shift, ctrl) = ctx.input(|i| (i.modifiers.shift, i.modifiers.command));
        let middle_down = ctx.input(|i| i.pointer.middle_down());
        let primary_down = ctx.input(|i| i.pointer.primary_down());
        let delta = ctx.input(|i| i.pointer.delta());
        let nav = self.tabs[self.active].nav;

        // Zoom (wheel).
        if resp.hovered() {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.0 {
                if let Some(p) = pointer {
                    let ndc = Vec2::new((p.x - rect.left()) / rect.width() * 2.0 - 1.0, 1.0 - (p.y - rect.top()) / rect.height() * 2.0);
                    let factor = (-scroll * 0.0025).exp();
                    let tab = self.tab();
                    tab.anim = None;
                    tab.camera.zoom_at(factor, ndc, aspect);
                }
            }
            // Pinch zoom on touchpads.
            let zoom = ctx.input(|i| i.zoom_delta());
            if (zoom - 1.0).abs() > 1e-4 {
                let tab = self.tab();
                tab.camera.zoom_at(1.0 / zoom, Vec2::ZERO, aspect);
            }
        }

        // Middle mouse: pan, Shift+middle: orbit (Fusion defaults).
        if middle_down && inside {
            let tab = self.tab();
            tab.anim = None;
            if shift {
                tab.camera.orbit(Vec2::new(delta.x, delta.y));
            } else {
                tab.camera.pan(Vec2::new(delta.x, delta.y), rect.height());
            }
            return;
        }

        // Manipulator drag.
        if let Some(i) = self.canvas.manip_drag {
            if primary_down {
                let ms = self.manipulators();
                if let Some(m) = ms.get(i).copied() {
                    if let Some((a, b, len)) = self.manip_screen(&m) {
                        let axis = b - a;
                        let along = delta.dot(axis.normalized());
                        let world = along / axis.length().max(1e-3) * len;
                        self.apply_manip(m.slot, world as f64);
                    }
                }
                return;
            }
            self.canvas.manip_drag = None;
        }

        // Navigation tools from the nav bar (left drag).
        if resp.dragged_by(egui::PointerButton::Primary) && nav != NavMode::Select {
            let tab = self.tab();
            tab.anim = None;
            match nav {
                NavMode::Orbit => tab.camera.orbit(Vec2::new(delta.x, delta.y)),
                NavMode::Pan => tab.camera.pan(Vec2::new(delta.x, delta.y), rect.height()),
                NavMode::Zoom => tab.camera.zoom_at(1.0 + delta.y * 0.005, Vec2::ZERO, aspect),
                NavMode::Select => {}
            }
            return;
        }

        // Dragging sketch points with the Select tool.
        let sketch_select = self.tabs[self.active].sketch.as_ref().is_some_and(|s| s.tool == SketchTool::Select);
        if sketch_select {
            let dragging = self.tabs[self.active].sketch.as_ref().is_some_and(|s| s.drag.is_some());
            if dragging {
                if let Some(p) = pointer {
                    self.update_sketch_cursor(p);
                }
                if primary_down {
                    self.sketch_drag(false, false);
                } else {
                    self.sketch_drag(false, true);
                }
                return;
            }
            if resp.drag_started_by(egui::PointerButton::Primary) {
                if let Some(SelItem::SketchPoint(_, p)) = self.tabs[self.active].hover {
                    if let Some(s) = &mut self.tab().sketch {
                        s.drag = Some(p);
                    }
                    self.sketch_drag(true, false);
                    return;
                }
            }
        }

        // Start of a primary drag: manipulator, or window selection.
        if resp.drag_started_by(egui::PointerButton::Primary) {
            if let Some(press) = ctx.input(|i| i.pointer.press_origin()) {
                let ms = self.manipulators();
                let hit = ms.iter().position(|m| self.manip_screen(m).is_some_and(|(a, b, _)| seg_dist(press, a, b) < 9.0));
                if let Some(i) = hit {
                    self.canvas.manip_drag = Some(i);
                    return;
                }
                if self.tabs[self.active].sketch.as_ref().is_none_or(|s| s.tool == SketchTool::Select) {
                    self.canvas.window_sel = Some(press);
                }
            }
        }
        if let Some(start) = self.canvas.window_sel {
            if !primary_down {
                if let Some(end) = pointer {
                    self.window_select(start, end, shift || ctrl);
                }
                self.canvas.window_sel = None;
            }
            return;
        }

        // Hover.
        if let Some(p) = pointer.filter(|_| inside) {
            let hovered_manip = self.manipulators().iter().any(|m| self.manip_screen(m).is_some_and(|(a, b, _)| seg_dist(p, a, b) < 9.0));
            let picked = if hovered_manip { None } else { self.pick(p) };
            self.canvas.cursor_world = picked.map(|x| x.1);
            self.tab().hover = picked.map(|x| x.0);
            if hovered_manip {
                ctx.set_cursor_icon(egui::CursorIcon::Grab);
            }
            if self.tabs[self.active].sketch.is_some() {
                self.update_sketch_cursor(p);
            }
        } else {
            self.tab().hover = None;
            self.canvas.sketch_cursor = None;
        }

        // Right click: marking menu.
        if resp.secondary_clicked() {
            if let Some(p) = pointer {
                self.flags.marking = Some(crate::marking::MarkingMenu::new(p));
            }
            return;
        }

        if resp.double_clicked() {
            if let Some(SelItem::SketchConstraint(uid, i)) = self.tabs[self.active].hover {
                let c = self.tabs[self.active].model.sketches.iter().find(|k| k.uid == uid).and_then(|k| k.constraints.get(i).cloned());
                if let Some(c) = c {
                    let units = self.tabs[self.active].doc.units;
                    self.open_dim_edit(Some(i), c, pointer.unwrap_or_default(), units);
                }
                return;
            }
            if let Some(s) = &self.tabs[self.active].sketch {
                if s.tool == SketchTool::Line {
                    if let Some(s) = &mut self.tab().sketch {
                        s.pending.clear();
                    }
                    return;
                }
            }
            if let Some(SelItem::Face(b, _)) = self.tabs[self.active].hover {
                self.click_item(SelItem::Body(b), ctrl || shift);
                return;
            }
        }
        if resp.clicked() {
            let hover = self.tabs[self.active].hover;
            self.canvas_click(hover, ctrl || shift);
        }
    }

    fn canvas_click(&mut self, hover: Option<SelItem>, additive: bool) {
        // Sketch drawing consumes clicks.
        if self.tabs[self.active].sketch.as_ref().is_some_and(|s| s.tool != SketchTool::Select) {
            self.sketch_click(hover);
            return;
        }
        if matches!(self.tabs[self.active].command, Some(Command::Fillet { .. })) {
            self.fillet_click(hover);
            return;
        }
        let point = self.canvas.cursor_world;
        let hover_plane = hover.and_then(|h| self.tabs[self.active].plane_of(&h));
        let tab = self.tab();
        match &mut tab.command {
            Some(Command::Primitive { kind, plane_chosen, .. }) if !*plane_chosen => {
                let plane = hover_plane;
                if let (Some(p), Some(w)) = (plane, point) {
                    let mut c = to_local(&p, w);
                    let step = grid_step(&tab.camera) as f64 / 10.0;
                    c = [(c[0] / step).round() * step, (c[1] / step).round() * step];
                    match kind {
                        FeatureKind::Box { plane, center, .. }
                        | FeatureKind::Cylinder { plane, center, .. }
                        | FeatureKind::Sphere { plane, center, .. }
                        | FeatureKind::Torus { plane, center, .. } => {
                            *plane = p;
                            *center = c;
                        }
                        _ => {}
                    }
                    *plane_chosen = true;
                }
            }
            Some(Command::Sweep { sketch, profiles, path_sketch, path, picking_path, .. }) => match hover {
                Some(SelItem::SketchEntity(uid, i)) if *picking_path => {
                    *path_sketch = Some(uid);
                    *path = chain_of(&tab.model, uid, i);
                    *picking_path = false;
                }
                Some(SelItem::Profile(uid, i)) => {
                    if *sketch != Some(uid) {
                        *sketch = Some(uid);
                        profiles.clear();
                    }
                    if let Some(k) = profiles.iter().position(|x| *x == i) {
                        profiles.remove(k);
                    } else {
                        profiles.push(i);
                    }
                    if path.is_empty() {
                        *picking_path = true;
                    }
                }
                _ => {}
            },
            Some(Command::Pipe { path_sketch, path, .. }) => {
                if let Some(SelItem::SketchEntity(uid, i)) = hover {
                    *path_sketch = Some(uid);
                    *path = chain_of(&tab.model, uid, i);
                }
            }
            Some(Command::Loft { sections, .. }) => {
                if let Some(SelItem::Profile(uid, i)) = hover {
                    if let Some(k) = sections.iter().position(|x| *x == (uid, i)) {
                        sections.remove(k);
                    } else {
                        sections.push((uid, i));
                    }
                }
            }
            Some(Command::SplitBody { body, plane, picking_plane, .. }) => {
                if *picking_plane {
                    if hover_plane.is_some() {
                        *plane = hover_plane;
                        *picking_plane = false;
                    }
                } else if let Some(b) = hover.and_then(|h| h.body()) {
                    *body = Some(b);
                    if plane.is_none() {
                        *picking_plane = true;
                    }
                }
            }
            Some(Command::Shell { body, faces, .. }) => {
                if let Some(SelItem::Face(b, f)) = hover {
                    if *body != Some(b) {
                        *body = Some(b);
                        faces.clear();
                    }
                    if let Some(k) = faces.iter().position(|x| *x == f) {
                        faces.remove(k);
                    } else {
                        faces.push(f);
                    }
                }
            }
            Some(Command::Scale { bodies, .. }) => {
                if let Some(b) = hover.and_then(|h| h.body()) {
                    if let Some(k) = bodies.iter().position(|x| *x == b) {
                        bodies.remove(k);
                    } else {
                        bodies.push(b);
                    }
                }
            }
            Some(Command::Mirror { plane, picking_plane: true, .. }) => {
                if hover_plane.is_some() {
                    *plane = hover_plane;
                }
            }
            Some(Command::Mirror { bodies, picking_plane, plane, .. }) => {
                if let Some(b) = hover.and_then(|h| h.body()) {
                    if let Some(k) = bodies.iter().position(|x| *x == b) {
                        bodies.remove(k);
                    } else {
                        bodies.push(b);
                    }
                    if plane.is_none() {
                        *picking_plane = true;
                    }
                }
            }
            Some(Command::Pattern { bodies, .. }) => {
                if let Some(b) = hover.and_then(|h| h.body()) {
                    if let Some(k) = bodies.iter().position(|x| *x == b) {
                        bodies.remove(k);
                    } else {
                        bodies.push(b);
                    }
                }
            }
            Some(Command::Hole { plane, points, .. }) => {
                if let (Some(SelItem::FacePlane(_, _, p)), Some(w)) = (hover, point) {
                    let same = plane.is_some_and(|q| q.base == p.base && q.flip == p.flip && (q.offset - p.offset).abs() < 1e-3);
                    if !same {
                        *plane = Some(p);
                        points.clear();
                    }
                    let mut c = to_local(&p, w);
                    let step = grid_step(&tab.camera) as f64 / 10.0;
                    c = [(c[0] / step).round() * step, (c[1] / step).round() * step];
                    points.push(c);
                }
            }
            Some(Command::OffsetPlane { base, .. }) => {
                if hover_plane.is_some() {
                    *base = hover_plane;
                }
            }
            Some(Command::Section { plane, .. }) => {
                if hover_plane.is_some() {
                    *plane = hover_plane;
                }
            }
            Some(Command::SketchPlane) => {
                let plane = hover_plane;
                if let Some(p) = plane {
                    self.start_sketch(p);
                }
            }
            Some(Command::Revolve { sketch, profiles, axis, picking_axis, .. }) => match hover {
                Some(SelItem::SketchEntity(_, i)) if *picking_axis => {
                    *axis = Some(m360_core::RevolveAxis::SketchLine(i));
                    *picking_axis = false;
                }
                Some(SelItem::Profile(uid, i)) => {
                    if *sketch != Some(uid) {
                        *sketch = Some(uid);
                        profiles.clear();
                    }
                    if let Some(k) = profiles.iter().position(|x| *x == i) {
                        profiles.remove(k);
                    } else {
                        profiles.push(i);
                    }
                    if axis.is_none() {
                        *picking_axis = true;
                    }
                }
                _ => {}
            },
            Some(Command::Extrude { sketch, profiles, .. }) => {
                if let Some(SelItem::Profile(uid, i)) = hover {
                    if *sketch != Some(uid) {
                        *sketch = Some(uid);
                        profiles.clear();
                    }
                    if let Some(k) = profiles.iter().position(|x| *x == i) {
                        profiles.remove(k);
                    } else {
                        profiles.push(i);
                    }
                    self.auto_extrude_op();
                }
            }
            Some(Command::Combine { target, tools, field, .. }) => {
                if let Some(b) = hover.and_then(|h| h.body()) {
                    match field {
                        CombineField::Target => {
                            *target = Some(b);
                            tools.retain(|t| *t != b);
                            *field = CombineField::Tools;
                        }
                        CombineField::Tools => {
                            if Some(b) != *target {
                                if let Some(k) = tools.iter().position(|t| *t == b) {
                                    tools.remove(k);
                                } else {
                                    tools.push(b);
                                }
                            }
                        }
                    }
                }
            }
            Some(Command::Move { bodies, .. }) => {
                if let Some(b) = hover.and_then(|h| h.body()) {
                    if let Some(k) = bodies.iter().position(|x| *x == b) {
                        bodies.remove(k);
                    } else {
                        bodies.push(b);
                    }
                }
            }
            Some(Command::Measure { picks }) => {
                if let Some(h) = hover {
                    if picks.len() >= 2 {
                        picks.clear();
                    }
                    picks.push(h);
                } else {
                    picks.clear();
                }
            }
            Some(Command::Appearance) | Some(Command::CenterOfMass) | Some(Command::Properties) => match hover {
                Some(h) => self.click_item(h, true),
                None => self.tab().selection.clear(),
            },
            _ => match hover {
                Some(h) => self.click_item(h, additive),
                None => {
                    if !additive {
                        self.tab().selection.clear();
                    }
                }
            },
        }
    }


    fn fillet_click(&mut self, hover: Option<SelItem>) {
                let tab = &self.tabs[self.active];
                // A face adds all of its eligible edges; an edge toggles itself.
                let picked: Vec<(u64, u32)> = match hover {
                    Some(SelItem::Edge(b, e)) => vec![(b, e)],
                    Some(SelItem::Face(b, f)) => tab
                        .model
                        .body(b)
                        .map(|bd| {
                            bd.mesh
                                .edge_info
                                .iter()
                                .enumerate()
                                .filter(|(_, info)| info.faces.contains(&f) && info.normals.is_some())
                                .map(|(i, _)| (b, i as u32))
                                .collect()
                        })
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                let eligible = |b: u64, e: u32| {
                    tab.model.body(b).and_then(|bd| bd.mesh.edge_info.get(e as usize)).is_some_and(|i| i.normals.is_some())
                };
                let mut rejected = false;
                let (mut nb, mut ne) = match &tab.command {
                    Some(Command::Fillet { body, edges, .. }) => (*body, edges.clone()),
                    _ => return,
                };
                for (b, e) in picked {
                    if !eligible(b, e) {
                        rejected = true;
                        continue;
                    }
                    if nb != Some(b) {
                        nb = Some(b);
                        ne.clear();
                    }
                    if let Some(k) = ne.iter().position(|x| *x == e) {
                        if !matches!(hover, Some(SelItem::Face(..))) {
                            ne.remove(k);
                        }
                    } else {
                        ne.push(e);
                    }
                }
                let tab = self.tab();
                if let Some(Command::Fillet { body, edges, .. }) = &mut tab.command {
                    *body = nb;
                    *edges = ne;
                }
                if rejected {
                    self.toast(crate::app::ToastKind::Info, "Only straight edges between flat faces can be filleted for now.");
                }
                }

    fn window_select(&mut self, a: Pos2, b: Pos2, additive: bool) {
        let r = egui::Rect::from_two_pos(a, b);
        if r.width() < 3.0 && r.height() < 3.0 {
            return;
        }
        let crossing = b.x < a.x;
        let cam = self.tabs[self.active].camera;
        let mut hits = Vec::new();
        let tab = &self.tabs[self.active];
        for body in &tab.model.bodies {
            if !tab.body_visible(body.id) {
                continue;
            }
            let pts = &body.mesh.positions;
            let step = (pts.len() / 400).max(1);
            let mut any = false;
            let mut all = true;
            for p in pts.iter().step_by(step) {
                match self.to_screen(&cam, Vec3::from(*p)) {
                    Some(s) if r.contains(s) => any = true,
                    _ => all = false,
                }
            }
            if (crossing && any) || (!crossing && all) {
                hits.push(SelItem::Body(body.id));
            }
        }
        let tab = self.tab();
        if !additive {
            tab.selection.clear();
        }
        for h in hits {
            if !tab.selection.contains(&h) {
                tab.selection.push(h);
            }
        }
    }

    // ------------------------------------------------------------------ scene

    fn build_scene(&mut self, ppp: f32) -> Scene {
        let manips = self.manipulators();
        let active = self.active;
        // Profiles of visible sketches (cached triangulations).
        let sketch_uids: Vec<u64> = self.tabs[active].model.sketches.iter().map(|s| s.uid).collect();
        for uid in &sketch_uids {
            self.tabs[active].sketch_profiles(*uid);
        }
        let tab = &self.tabs[active];
        let cam = tab.camera;
        let (top, bottom) = tab.display.environment.gradient();
        let mut scene = Scene {
            camera: cam,
            bg_top: srgb(top[0], top[1], top[2], 1.0),
            bg_bottom: srgb(bottom[0], bottom[1], bottom[2], 1.0),
            pixels_per_point: ppp,
            ..Default::default()
        };
        let style = tab.display.style;
        let dark_env = matches!(tab.display.environment, crate::state::Environment::DarkSky);
        let sel_bodies = tab.selected_bodies();
        let hover_body = tab.hover.and_then(|h| h.body());
        let timeline_bodies: Vec<u64> = tab
            .timeline_hover
            .map(|uid| tab.model.bodies.iter().filter(|b| b.last_feature == uid || b.id == uid).map(|b| b.id).collect())
            .unwrap_or_default();

        // Combine role tints.
        let (combine_target, combine_tools, move_bodies, moving) = match &tab.command {
            Some(Command::Combine { target, tools, .. }) => (*target, tools.clone(), Vec::new(), false),
            Some(Command::Move { bodies, copy, .. }) => (None, Vec::new(), bodies.clone(), !*copy),
            Some(Command::Pattern { bodies, .. }) | Some(Command::Mirror { bodies, .. }) => (None, Vec::new(), bodies.clone(), false),
            _ => (None, Vec::new(), Vec::new(), false),
        };
        let pick_bodies = self.pick_mode() == PickMode::Bodies;

        for b in &tab.model.bodies {
            if !tab.body_visible(b.id) {
                continue;
            }
            if moving && move_bodies.contains(&b.id) && tab.preview.as_ref().is_some_and(|p| p.mesh.is_some()) {
                continue;
            }
            // Bodies being modified in place show their preview instead.
            let replaced = match &tab.command {
                Some(Command::Shell { body: Some(sb), .. }) | Some(Command::Fillet { body: Some(sb), .. }) => *sb == b.id,
                _ => false,
            };
            if replaced && tab.preview.as_ref().is_some_and(|p| p.mesh.is_some()) {
                continue;
            }
            let meta = tab.doc.body_meta(b.id);
            let mut d = BodyDraw::new(b.mesh.clone(), srgb(meta.color[0], meta.color[1], meta.color[2], 1.0));
            if dark_env {
                d.edge_color = srgb(0.0, 0.0, 0.0, 1.0);
            }
            match style {
                VisualStyle::Shaded => d.show_edges = false,
                VisualStyle::Wireframe => {
                    d.show_faces = false;
                    if dark_env {
                        d.edge_color = srgb(0.85, 0.85, 0.9, 1.0);
                    }
                }
                VisualStyle::ShadedHiddenEdges => {
                    d.color[3] = 0.55;
                }
                VisualStyle::ShadedVisibleEdges => {}
            }
            let blue = srgb(0.1, 0.5, 1.0, 1.0);
            if Some(b.id) == combine_target {
                d.highlight = [blue[0], blue[1], blue[2], 0.32];
            } else if combine_tools.contains(&b.id) {
                let red = srgb(0.9, 0.25, 0.2, 1.0);
                d.highlight = [red[0], red[1], red[2], 0.45];
            } else if sel_bodies.contains(&b.id) && tab.selection.contains(&SelItem::Body(b.id)) || move_bodies.contains(&b.id) {
                d.highlight = [blue[0], blue[1], blue[2], 0.28];
                d.edge_color = srgb(0.02, 0.35, 0.8, 1.0);
            } else if timeline_bodies.contains(&b.id) {
                d.highlight = [blue[0], blue[1], blue[2], 0.25];
            } else if pick_bodies && hover_body == Some(b.id) {
                d.highlight = [blue[0], blue[1], blue[2], 0.2];
            }
            d.selected_faces = tab
                .selection
                .iter()
                .filter_map(|s| match s {
                    SelItem::Face(bid, f) | SelItem::FacePlane(bid, f, _) if *bid == b.id => Some(*f),
                    _ => None,
                })
                .collect();
            if let Some(SelItem::Face(bid, f) | SelItem::FacePlane(bid, f, _)) = tab.hover {
                if bid == b.id {
                    d.hover_face = Some(f);
                }
            }
            if let Some(Command::Shell { body: Some(sb), faces, .. }) = &tab.command {
                if *sb == b.id {
                    d.selected_faces.extend(faces.iter().copied());
                }
            }
            if let Some(Command::Measure { picks }) = &tab.command {
                for p in picks {
                    match p {
                        SelItem::Face(bid, f) if *bid == b.id => d.selected_faces.push(*f),
                        SelItem::Body(bid) if *bid == b.id => d.highlight = [blue[0], blue[1], blue[2], 0.38],
                        _ => {}
                    }
                }
            }
            scene.bodies.push(d);
        }

        // Sweep / pipe paths.
        let path_hl = match &tab.command {
            Some(Command::Sweep { path_sketch: Some(ps), path, .. }) | Some(Command::Pipe { path_sketch: Some(ps), path, .. }) => Some((*ps, path.clone())),
            _ => None,
        };
        if let Some((ps, path)) = path_hl {
            if let Some((plane, ents)) = tab.sketch_entities(ps) {
                for i in path {
                    if let Some(e) = ents.get(i) {
                        entity_lines(&mut scene.lines, &plane, e, srgb(0.02, 0.35, 0.9, 1.0), 3.0);
                    }
                }
            }
        }
        if let Some(SelItem::SketchEntity(uid, i)) = tab.hover.filter(|_| self.pick_mode() == PickMode::Curves) {
            if let Some((plane, ents)) = tab.sketch_entities(uid) {
                if let Some(e) = ents.get(i) {
                    entity_lines(&mut scene.lines, &plane, e, srgb(0.25, 0.65, 1.0, 1.0), 3.0);
                }
            }
        }

        // Revolve axis line.
        if let Some(Command::Revolve { sketch: Some(sk), axis: Some(axis), .. }) = &tab.command {
            if let Some((plane, ents)) = tab.sketch_entities(*sk) {
                if let Ok((p, d)) = m360_core::model::revolve_axis(&plane, &ents, axis) {
                    let ext = cam.distance;
                    let (p, d) = (p.as_vec3(), d.as_vec3());
                    scene.lines.push(LineInst::new(p - d * ext, p + d * ext, srgb(0.05, 0.4, 0.95, 0.9), 1.8));
                }
            }
        }

        // Edge highlights (hover / selection / fillet edges).
        {
            let mut hl: Vec<(u64, u32, [f32; 4])> = Vec::new();
            for sitem in &tab.selection {
                if let SelItem::Edge(b, e) = sitem {
                    hl.push((*b, *e, srgb(0.02, 0.35, 0.9, 1.0)));
                }
            }
            if let Some(Command::Fillet { body: Some(b), edges, .. }) = &tab.command {
                for e in edges {
                    hl.push((*b, *e, srgb(0.02, 0.35, 0.9, 1.0)));
                }
            }
            if let Some(SelItem::Edge(b, e)) = tab.hover {
                hl.push((b, e, srgb(0.25, 0.65, 1.0, 1.0)));
            }
            for (b, e, c) in hl {
                if let Some(pts) = tab.model.body(b).and_then(|bd| bd.mesh.edges.get(e as usize)) {
                    for w in pts.windows(2) {
                        scene.lines.push(LineInst::new(Vec3::from(w[0]), Vec3::from(w[1]), c, 3.2));
                    }
                }
            }
        }

        // Command preview body.
        if let Some(p) = &tab.preview {
            if let Some(mesh) = &p.mesh {
                let (color, edges) = match &tab.command {
                    Some(Command::Extrude { op: ExtrudeOp::Cut, .. }) | Some(Command::Revolve { op: ExtrudeOp::Cut, .. }) | Some(Command::Hole { .. }) => (srgb(0.9, 0.25, 0.2, 0.45), true),
                    Some(Command::Revolve { op: ExtrudeOp::Join, .. }) => (srgb(0.55, 0.75, 0.95, 0.7), true),
                    Some(Command::Extrude { op: ExtrudeOp::Intersect, .. }) => (srgb(0.95, 0.6, 0.15, 0.45), true),
                    Some(Command::Extrude { op: ExtrudeOp::Join, .. }) => (srgb(0.55, 0.75, 0.95, 0.7), true),
                    Some(Command::Move { .. }) | Some(Command::Pattern { .. }) | Some(Command::Mirror { .. }) => (srgb(0.62, 0.78, 0.95, 1.0), true),
                    _ => (srgb(0.72, 0.74, 0.77, 1.0), true),
                };
                let mut d = BodyDraw::new(Arc::clone(mesh), color);
                d.show_edges = edges;
                scene.bodies.push(d);
            }
        }

        let in_sketch = tab.sketch.as_ref().map(|s| s.uid);
        let grid_alpha = if dark_env { 0.25 } else { 0.35 };

        // Ground grid (XZ plane) or sketch grid.
        if tab.display.grid {
            let plane = in_sketch
                .and_then(|u| tab.sketch_entities(u))
                .map(|(p, _)| p)
                .unwrap_or(PlaneRef::XZ);
            grid_lines(&mut scene.lines, &cam, &plane, grid_alpha, in_sketch.is_some());
        }

        // Origin axes.
        let ext = cam.distance * 3.0;
        let red = srgb(0.84, 0.23, 0.19, 0.9);
        let green = srgb(0.18, 0.66, 0.31, 0.9);
        let blue = srgb(0.18, 0.43, 0.84, 0.9);
        if tab.display.grid && in_sketch.is_none() {
            scene.lines.push(LineInst::new(Vec3::new(-ext, 0.0, 0.0), Vec3::new(ext, 0.0, 0.0), red, 1.4));
            scene.lines.push(LineInst::new(Vec3::new(0.0, 0.0, -ext), Vec3::new(0.0, 0.0, ext), blue, 1.4));
        }
        let show_planes = tab.doc.origin_visible || self.pick_mode() == PickMode::Planes;
        if show_planes {
            let s = origin_plane_size(&cam);
            scene.lines.push(LineInst::new(Vec3::ZERO, Vec3::X * s * 1.2, red, 2.0));
            scene.lines.push(LineInst::new(Vec3::ZERO, Vec3::Y * s * 1.2, green, 2.0));
            scene.lines.push(LineInst::new(Vec3::ZERO, Vec3::Z * s * 1.2, blue, 2.0));
            for b in [BasePlane::XY, BasePlane::XZ, BasePlane::YZ] {
                let p = base_plane_ref(b);
                let (o, u, v, _) = plane_frame(&p);
                let hovered = tab.hover == Some(SelItem::Plane(b));
                let selected = tab.selection.contains(&SelItem::Plane(b));
                let fill = if hovered || selected { srgb(0.25, 0.6, 1.0, 0.35) } else { srgb(0.95, 0.72, 0.38, 0.18) };
                let edge = if hovered || selected { srgb(0.1, 0.45, 0.95, 1.0) } else { srgb(0.85, 0.55, 0.2, 0.8) };
                let c = [o - u * s - v * s, o + u * s - v * s, o + u * s + v * s, o - u * s + v * s];
                for t in [[0, 1, 2], [0, 2, 3]] {
                    for k in t {
                        scene.tris.push(FlatVertex::new(c[k], fill));
                    }
                }
                for k in 0..4 {
                    scene.lines.push(LineInst::new(c[k], c[(k + 1) % 4], edge, 1.2));
                }
            }
        }
        // Construction planes (and the plane being created / section plane).
        {
            let s = origin_plane_size(&cam);
            let mut quads: Vec<(PlaneRef, bool, bool)> = tab
                .model
                .planes
                .iter()
                .filter(|p| tab.doc.plane_visible.get(&p.uid).copied().unwrap_or(true))
                .map(|p| {
                    let hl = tab.hover == Some(SelItem::ConstructionPlane(p.uid)) || tab.selection.contains(&SelItem::ConstructionPlane(p.uid));
                    (p.plane, hl, false)
                })
                .collect();
            if let Some(Command::OffsetPlane { base: Some(b), distance, .. }) = &tab.command {
                quads.push((PlaneRef { offset: b.offset + if b.flip { -distance } else { *distance }, ..*b }, true, true));
            }
            if let Some(sec) = tab.section.filter(|s| s.visible) {
                if matches!(tab.command, Some(Command::Section { .. })) {
                    quads.push((sec.plane, true, true));
                }
            }
            for (p, hl, preview) in quads {
                let (o, u, v, _) = plane_frame(&p);
                let fill = if preview { srgb(0.25, 0.6, 1.0, 0.25) } else if hl { srgb(0.25, 0.6, 1.0, 0.35) } else { srgb(0.95, 0.72, 0.38, 0.22) };
                let edge = if hl { srgb(0.1, 0.45, 0.95, 1.0) } else { srgb(0.85, 0.55, 0.2, 0.9) };
                let c = [o - u * s - v * s, o + u * s - v * s, o + u * s + v * s, o - u * s + v * s];
                for t in [[0, 1, 2], [0, 2, 3]] {
                    for k in t {
                        scene.tris.push(FlatVertex::new(c[k], fill));
                    }
                }
                for k in 0..4 {
                    scene.lines.push(LineInst::new(c[k], c[(k + 1) % 4], edge, 1.2));
                }
            }
        }
        scene.section = tab.section.filter(|s| s.visible).map(|s| s.clip());

        // Planar face hover while picking planes.
        if let Some(SelItem::FacePlane(..)) = tab.hover {
            // Face highlight already handled by hover_face.
        }

        // Sketches.
        if tab.doc.sketches_folder_visible || in_sketch.is_some() {
            for sk in &tab.model.sketches {
                let editing = in_sketch == Some(sk.uid);
                let visible = tab.doc.sketch_visible.get(&sk.uid).copied().unwrap_or(true);
                if !(visible && tab.doc.sketches_folder_visible) && !editing {
                    continue;
                }
                let base = if editing { SKETCH_BLUE } else { [0.25, 0.27, 0.3] };
                for (i, e) in sk.entities.iter().enumerate() {
                    let selected = tab.selection.contains(&SelItem::SketchEntity(sk.uid, i));
                    let hovered = tab.hover == Some(SelItem::SketchEntity(sk.uid, i));
                    let c = if selected {
                        srgb(0.02, 0.35, 0.9, 1.0)
                    } else if hovered {
                        srgb(0.25, 0.65, 1.0, 1.0)
                    } else {
                        srgb(base[0], base[1], base[2], 1.0)
                    };
                    let w = if selected || hovered { 2.6 } else { 1.6 };
                    entity_lines(&mut scene.lines, &sk.plane, e, c, w);
                    if editing {
                        for p in entity_points(e) {
                            let q = to_world(&sk.plane, p);
                            point_marker(&mut scene.overlay_lines, &cam, self.canvas_rect.height(), q, srgb(0.1, 0.1, 0.12, 1.0));
                        }
                    }
                }
                // Profiles
                let show_profiles = editing || in_sketch.is_none();
                if show_profiles {
                    if let Some(cache) = tab.profiles.get(&sk.uid) {
                        for (i, tris) in cache.tris.iter().enumerate() {
                            let item = SelItem::Profile(sk.uid, i);
                            let selected = tab.selection.contains(&item)
                                || matches!(&tab.command, Some(Command::Extrude { sketch: Some(s), profiles, .. }) | Some(Command::Revolve { sketch: Some(s), profiles, .. }) | Some(Command::Sweep { sketch: Some(s), profiles, .. }) if *s == sk.uid && profiles.contains(&i))
                                || matches!(&tab.command, Some(Command::Loft { sections, .. }) if sections.contains(&(sk.uid, i)));
                            let hovered = tab.hover == Some(item);
                            let fill = if selected {
                                srgb(0.25, 0.6, 1.0, 0.45)
                            } else if hovered {
                                srgb(0.45, 0.72, 1.0, 0.35)
                            } else if editing {
                                srgb(0.98, 0.75, 0.45, 0.28)
                            } else {
                                srgb(0.98, 0.75, 0.45, 0.16)
                            };
                            for p in tris {
                                scene.tris.push(FlatVertex::new(Vec3::from(*p), fill));
                            }
                        }
                    }
                }
            }
        }

        // Sketch rubber band.
        if let Some(s) = &tab.sketch {
            if let (Some(cur), Some((plane, _))) = (self.canvas.sketch_cursor, tab.sketch_entities(s.uid)) {
                let c = srgb(0.1, 0.5, 1.0, 1.0);
                for e in crate::sketch::rubber_band(s, cur) {
                    entity_lines(&mut scene.overlay_lines, &plane, &e, c, 1.6);
                }
                let marker = if self.canvas.snapped { srgb(0.18, 0.66, 0.31, 1.0) } else { srgb(0.1, 0.1, 0.12, 1.0) };
                point_marker(&mut scene.overlay_lines, &cam, self.canvas_rect.height(), to_world(&plane, cur), marker);
            }
        }

        // Center of mass marker.
        if let (Some(Command::CenterOfMass), Some(c)) = (&tab.command, self.canvas.com) {
            let s = cam.world_per_pixel(self.canvas_rect.height()) * 10.0;
            let k = srgb(0.1, 0.1, 0.12, 1.0);
            for d in [Vec3::X, Vec3::Y, Vec3::Z] {
                scene.overlay_lines.push(LineInst::new(c - d * s, c + d * s, k, 2.0));
            }
        }

        // Manipulators.
        let wpp = cam.world_per_pixel(self.canvas_rect.height());
        for (i, m) in manips.iter().enumerate() {
            let len = wpp * 70.0;
            let active_drag = self.canvas.manip_drag == Some(i);
            let col = if active_drag { srgb(0.1, 0.5, 1.0, 1.0) } else { srgb(m.color[0], m.color[1], m.color[2], 1.0) };
            let tip = m.origin + m.dir * len;
            scene.overlay_lines.push(LineInst::new(m.origin, tip - m.dir * wpp * 10.0, col, 2.5));
            // Arrow head as a cone of triangles.
            let side = if m.dir.cross(cam.forward()).length() > 1e-3 { m.dir.cross(cam.forward()).normalize() } else { m.dir.any_orthonormal_vector() };
            let up = m.dir.cross(side).normalize();
            let r = wpp * 6.0;
            let base = tip - m.dir * wpp * 16.0;
            let n = 12;
            for k in 0..n {
                let a0 = k as f32 / n as f32 * std::f32::consts::TAU;
                let a1 = (k + 1) as f32 / n as f32 * std::f32::consts::TAU;
                let p0 = base + (side * a0.cos() + up * a0.sin()) * r;
                let p1 = base + (side * a1.cos() + up * a1.sin()) * r;
                for p in [tip, p0, p1] {
                    scene.overlay_tris.push(FlatVertex::new(p, col));
                }
            }
            // Base knob.
            point_marker(&mut scene.overlay_lines, &cam, self.canvas_rect.height(), m.origin, col);
        }
        scene
    }

    /// 2D overlays drawn on top of the 3D image: window selection, labels.
    fn paint_canvas_2d(&mut self, ui: &mut egui::Ui) {
        let painter = ui.painter_at(self.canvas_rect);
        if let (Some(a), Some(b)) = (self.canvas.window_sel, ui.input(|i| i.pointer.hover_pos())) {
            let r = egui::Rect::from_two_pos(a, b);
            let crossing = b.x < a.x;
            let (fill, stroke) = if crossing {
                (Color32::from_rgba_unmultiplied(47, 168, 79, 30), Color32::from_rgb(47, 168, 79))
            } else {
                (Color32::from_rgba_unmultiplied(6, 150, 215, 30), theme::ACCENT)
            };
            painter.rect_filled(r, 0.0, fill);
            if crossing {
                let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                for w in pts.windows(2) {
                    painter.add(egui::Shape::dashed_line(&[w[0], w[1]], egui::Stroke::new(1.0, stroke), 5.0, 3.0));
                }
            } else {
                painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
            }
        }

        // Constraint glyphs and dimensions of the sketch being edited.
        self.canvas.labels.clear();
        let tab = &self.tabs[self.active];
        if let Some(sk) = tab.sketch.as_ref().and_then(|s| tab.model.sketches.iter().find(|k| k.uid == s.uid)) {
            let cam = tab.camera;
            let units = tab.doc.units;
            let plane = sk.plane;
            let scr = |q: [f64; 2]| self.to_screen(&cam, to_world(&plane, q));
            let mut per_entity: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
            let mut labels = Vec::new();
            for (ci, c) in sk.constraints.iter().enumerate() {
                use m360_core::solver::{point_of, SketchConstraint as C};
                let selected = tab.selection.contains(&SelItem::SketchConstraint(sk.uid, ci));
                let hovered = tab.hover == Some(SelItem::SketchConstraint(sk.uid, ci));
                let (text, pos, dim) = match c {
                    C::Distance { a, b, value } => {
                        let (Some(pa), Some(pb)) = (point_of(&sk.entities, *a), point_of(&sk.entities, *b)) else { continue };
                        let (Some(sa), Some(sb)) = (scr(pa), scr(pb)) else { continue };
                        let d = (sb - sa).normalized();
                        let n = egui::vec2(-d.y, d.x) * 18.0;
                        // Dimension line with extension lines.
                        let stroke = egui::Stroke::new(1.0, Color32::from_rgb(40, 40, 46));
                        painter.line_segment([sa, sa + n * 1.2], stroke);
                        painter.line_segment([sb, sb + n * 1.2], stroke);
                        painter.arrow(sa + n + (sb - sa) * 0.5, (sa - sb) * 0.5, stroke);
                        painter.arrow(sa + n + (sb - sa) * 0.5, (sb - sa) * 0.5, stroke);
                        (format!("{:.2}", units.from_mm(*value)), sa + (sb - sa) * 0.5 + n, true)
                    }
                    C::Diameter { entity, value } => {
                        let center = match sk.entities.get(*entity) {
                            Some(SketchEntity::Circle { c, r }) => Some((*c, *r)),
                            Some(SketchEntity::Arc { a, m, b }) => m360_kernel::profile::circumcircle(*a, *m, *b),
                            _ => None,
                        };
                        let Some((c, r)) = center else { continue };
                        let k = std::f64::consts::FRAC_1_SQRT_2;
                        let (Some(sc), Some(se)) = (scr(c), scr([c[0] + r * k, c[1] + r * k])) else { continue };
                        let stroke = egui::Stroke::new(1.0, Color32::from_rgb(40, 40, 46));
                        let dir = (se - sc).normalized();
                        painter.arrow(se + dir * 22.0, -dir * 22.0, stroke);
                        (format!("⌀{:.2}", units.from_mm(*value)), se + dir * 30.0, true)
                    }
                    other => {
                        let Some(&e) = other.entities().first() else { continue };
                        let anchor = match sk.entities.get(e) {
                            Some(SketchEntity::Line { a, b }) => [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0],
                            Some(SketchEntity::Circle { c, r }) => [c[0] + r, c[1]],
                            Some(SketchEntity::Arc { m, .. }) => *m,
                            Some(SketchEntity::Point { p }) => *p,
                            None => continue,
                        };
                        let anchor = match other {
                            C::Fix(p) | C::Coincident(p, _) => point_of(&sk.entities, *p).unwrap_or(anchor),
                            _ => anchor,
                        };
                        let Some(sa) = scr(anchor) else { continue };
                        let k = per_entity.entry(e).or_insert(0);
                        *k += 1;
                        (other.glyph().to_string(), sa + egui::vec2(10.0 + 16.0 * (*k - 1) as f32, -14.0), false)
                    }
                };
                let font = egui::FontId::proportional(if dim { 12.0 } else { 10.5 });
                let color = if selected { Color32::WHITE } else if dim { Color32::from_rgb(25, 25, 30) } else { Color32::from_rgb(30, 90, 30) };
                let galley = painter.layout_no_wrap(text, font, color);
                let r = egui::Rect::from_center_size(pos, galley.size() + egui::vec2(6.0, 2.0));
                let bg = if selected {
                    theme::ACCENT
                } else if hovered {
                    theme::HOVER_BG
                } else if dim {
                    Color32::from_white_alpha(225)
                } else {
                    Color32::from_rgba_unmultiplied(225, 245, 225, 235)
                };
                painter.rect_filled(r, 2.0, bg);
                if !dim {
                    painter.rect_stroke(r, 2.0, egui::Stroke::new(1.0, Color32::from_rgb(90, 150, 90)), egui::StrokeKind::Inside);
                }
                painter.galley(r.center() - galley.size() / 2.0, galley, color);
                labels.push((r, sk.uid, ci));
            }
            self.canvas.labels = labels;
        }

        // Sketch dimension label next to the cursor.
        let tab = &self.tabs[self.active];
        if let (Some(s), Some(cur)) = (tab.sketch.as_ref().filter(|s| !matches!(s.tool, SketchTool::Select | SketchTool::Dimension | SketchTool::Constrain(_))), self.canvas.sketch_cursor) {
            if let Some((plane, _)) = tab.sketch_entities(s.uid) {
                if let Some(text) = crate::sketch::rubber_band_label(s, cur, tab.doc.units) {
                    if let Some(p) = self.to_screen(&tab.camera, to_world(&plane, cur)) {
                        let pos = p + egui::vec2(14.0, -18.0);
                        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.0), theme::TEXT);
                        let r = egui::Rect::from_min_size(pos, galley.size()).expand(3.0);
                        painter.rect_filled(r, 2.0, Color32::from_white_alpha(230));
                        painter.rect_stroke(r, 2.0, egui::Stroke::new(1.0, theme::PANEL_BORDER), egui::StrokeKind::Inside);
                        painter.galley(pos, galley, theme::TEXT);
                    }
                }
            }
        }

        // Prompt for commands that wait for a selection.
        let prompt = match &tab.command {
            Some(Command::Primitive { plane_chosen: false, .. }) => Some("Select a plane or planar face"),
            Some(Command::SketchPlane) => Some("Select a plane or planar face to sketch on"),
            Some(Command::Hole { .. }) => Some("Click on a planar face to place holes"),
            Some(Command::Extrude { profiles, .. }) if profiles.is_empty() => Some("Select a sketch profile"),
            _ => None,
        };
        if let (Some(text), Some(p)) = (prompt, ui.input(|i| i.pointer.hover_pos()).filter(|p| self.canvas_rect.contains(*p))) {
            let pos = p + egui::vec2(16.0, 14.0);
            let galley = painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(11.5), Color32::WHITE);
            let r = egui::Rect::from_min_size(pos, galley.size()).expand(4.0);
            painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(40, 42, 46, 220));
            painter.galley(pos, galley, Color32::WHITE);
        }
    }
}

/// Connected chain of lines/arcs containing entity `start` (chain selection).
pub fn chain_of(model: &m360_core::Model, sketch: u64, start: usize) -> Vec<usize> {
    let Some(sk) = model.sketches.iter().find(|s| s.uid == sketch) else { return vec![start] };
    let ends = |e: &SketchEntity| match e {
        SketchEntity::Line { a, b } | SketchEntity::Arc { a, b, .. } => Some([*a, *b]),
        _ => None,
    };
    let same = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-6;
    let mut out = vec![start];
    let mut stack = vec![start];
    while let Some(i) = stack.pop() {
        let Some(ei) = sk.entities.get(i).and_then(ends) else { continue };
        for (j, e) in sk.entities.iter().enumerate() {
            if out.contains(&j) {
                continue;
            }
            if let Some(ej) = ends(e) {
                if ei.iter().any(|p| ej.iter().any(|q| same(*p, *q))) {
                    out.push(j);
                    stack.push(j);
                }
            }
        }
    }
    out
}

/// Bounds of all visible bodies (falls back to a default box).
pub fn scene_bounds(tab: &DocTab) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for b in &tab.model.bodies {
        if tab.body_visible(b.id) {
            min = min.min(b.mesh.bbox_min);
            max = max.max(b.mesh.bbox_max);
        }
    }
    for s in &tab.model.sketches {
        for e in &s.entities {
            for p in entity_points(e) {
                let w = to_world(&s.plane, p);
                min = min.min(w);
                max = max.max(w);
            }
            if let SketchEntity::Circle { c, r } = e {
                for d in [[*r, *r], [-*r, -*r]] {
                    let w = to_world(&s.plane, [c[0] + d[0], c[1] + d[1]]);
                    min = min.min(w);
                    max = max.max(w);
                }
            }
        }
    }
    if min.x > max.x {
        (Vec3::splat(-50.0), Vec3::splat(50.0))
    } else {
        (min, max)
    }
}

pub fn entity_points(e: &SketchEntity) -> Vec<[f64; 2]> {
    match e {
        SketchEntity::Line { a, b } => vec![*a, *b],
        SketchEntity::Circle { c, .. } => vec![*c],
        SketchEntity::Point { p } => vec![*p],
        SketchEntity::Arc { a, b, m } => {
            let mut v = vec![*a, *b];
            if let Some((c, _)) = m360_kernel::profile::circumcircle(*a, *m, *b) {
                v.push(c);
            }
            v
        }
    }
}

fn entity_lines(out: &mut Vec<LineInst>, plane: &PlaneRef, e: &SketchEntity, color: [f32; 4], width: f32) {
    match e {
        SketchEntity::Line { a, b } => out.push(LineInst::new(to_world(plane, *a), to_world(plane, *b), color, width)),
        SketchEntity::Circle { c, r } => {
            let n = 72;
            for i in 0..n {
                let a0 = i as f64 / n as f64 * std::f64::consts::TAU;
                let a1 = (i + 1) as f64 / n as f64 * std::f64::consts::TAU;
                out.push(LineInst::new(
                    to_world(plane, [c[0] + r * a0.cos(), c[1] + r * a0.sin()]),
                    to_world(plane, [c[0] + r * a1.cos(), c[1] + r * a1.sin()]),
                    color,
                    width,
                ));
            }
        }
        SketchEntity::Arc { a, m, b } => {
            let mut prev = *a;
            for p in m360_kernel::profile::arc_points(*a, *m, *b, 48) {
                out.push(LineInst::new(to_world(plane, prev), to_world(plane, p), color, width));
                prev = p;
            }
        }
        SketchEntity::Point { .. } => {}
    }
}

/// A small screen-sized square marker.
fn point_marker(out: &mut Vec<LineInst>, cam: &Camera, viewport_h: f32, p: Vec3, color: [f32; 4]) {
    let s = cam.world_per_pixel(viewport_h) * 3.0;
    let r = cam.right() * s;
    let u = cam.up() * s;
    let c = [p - r - u, p + r - u, p + r + u, p - r + u];
    for k in 0..4 {
        out.push(LineInst::new(c[k], c[(k + 1) % 4], color, 1.5));
    }
}

fn grid_lines(out: &mut Vec<LineInst>, cam: &Camera, plane: &PlaneRef, alpha: f32, sketch: bool) {
    let (o, u, v, n) = plane_frame(plane);
    let step = grid_step(cam);
    let major = step * 10.0;
    // Center the grid on the camera target projected onto the plane.
    let t = cam.target - n * (cam.target - o).dot(n);
    let cu = ((t - o).dot(u) / major).round() * major;
    let cv = ((t - o).dot(v) / major).round() * major;
    let half = (cam.distance * 1.6 / major).ceil() * major;
    let count = (2.0 * half / step).round() as i32;
    if count > 400 {
        return;
    }
    let minor_c = if sketch { [0.55, 0.62, 0.72] } else { [0.45, 0.47, 0.5] };
    let segs = 10;
    for axis in 0..2 {
        let (dir, across, c_across, c_along) = if axis == 0 { (v, u, cu, cv) } else { (u, v, cv, cu) };
        for i in 0..=count {
            let off = c_across - half + i as f32 * step;
            let is_major = (off / major).round() * major - off == 0.0 || ((off / major) - (off / major).round()).abs() < 1e-4;
            if !sketch && off.abs() < step * 0.01 {
                continue; // axis lines drawn separately
            }
            let a_base = if is_major { alpha * 1.0 } else { alpha * 0.45 };
            for s in 0..segs {
                let s0 = c_along - half + 2.0 * half * s as f32 / segs as f32;
                let s1 = c_along - half + 2.0 * half * (s + 1) as f32 / segs as f32;
                let mid = (s0 + s1) * 0.5 - c_along;
                let fade = (1.0 - (mid.abs().max((off - c_across).abs()) / half)).clamp(0.0, 1.0);
                let a = a_base * fade.powf(0.7);
                if a < 0.01 {
                    continue;
                }
                let p0 = o + across * off + dir * s0;
                let p1 = o + across * off + dir * s1;
                out.push(LineInst::new(p0, p1, srgb(minor_c[0], minor_c[1], minor_c[2], a), if is_major { 1.0 } else { 0.8 }));
            }
        }
    }
    if sketch {
        // Sketch axes through the plane origin.
        let ext = cam.distance * 3.0;
        out.push(LineInst::new(o - u * ext, o + u * ext, srgb(0.84, 0.23, 0.19, 0.6), 1.2));
        out.push(LineInst::new(o - v * ext, o + v * ext, srgb(0.18, 0.66, 0.31, 0.6), 1.2));
    }
}
