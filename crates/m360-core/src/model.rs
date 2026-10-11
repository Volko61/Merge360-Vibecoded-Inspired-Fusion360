//! Regeneration: replaying the timeline into B-rep bodies.

use std::collections::HashMap;
use std::sync::Arc;

use m360_kernel::{fillet, ops, ops::BooleanOp, primitives, profile, tessellate, tessellate_smooth, BodyMesh, Solid, SMOOTH_COS};

use crate::document::Document;
use crate::feature::{ExtrudeOp, Feature, FeatureKind, HoleType, RevolveAxis, SketchEntity};
use crate::PlaneRef;

/// Stable body identifier. A body created by a feature takes that feature's
/// uid; copies get `uid << 16 | index`.
pub type BodyId = u64;

#[derive(Clone)]
pub struct Body {
    pub id: BodyId,
    pub solid: Solid,
    pub mesh: Arc<BodyMesh>,
    /// Feature that last modified this body.
    pub last_feature: u64,
    /// Edge smoothing threshold (lower for bodies with faceted fillets).
    pub smooth_cos: f32,
}

#[derive(Clone)]
pub struct SketchGeom {
    pub uid: u64,
    pub name: String,
    pub plane: PlaneRef,
    pub entities: Vec<SketchEntity>,
    pub constraints: Vec<crate::solver::SketchConstraint>,
}

#[derive(Clone)]
pub struct PlaneGeom {
    pub uid: u64,
    pub name: String,
    pub plane: PlaneRef,
}

/// Result of regenerating a document.
#[derive(Clone, Default)]
pub struct Model {
    pub bodies: Vec<Body>,
    pub sketches: Vec<SketchGeom>,
    pub planes: Vec<PlaneGeom>,
    /// Feature uid -> error message (shown in red on the timeline).
    pub errors: HashMap<u64, String>,
}

impl Model {
    pub fn body(&self, id: BodyId) -> Option<&Body> {
        self.bodies.iter().find(|b| b.id == id)
    }
}

/// Incremental regenerator. It caches the model state after each feature,
/// keyed by a fingerprint of the feature prefix, so editing late features
/// does not recompute earlier ones.
#[derive(Default)]
pub struct Regenerator {
    cache: Vec<(String, Model)>,
}

fn fingerprint(f: &Feature) -> String {
    ron::to_string(f).unwrap_or_default()
}

impl Regenerator {
    pub fn regenerate(&mut self, doc: &Document) -> Model {
        let (vars, _) = crate::expr::eval_params(&doc.params, doc.units);
        // Evaluate expression-driven fields first; the fingerprint then also
        // captures parameter changes.
        let active: Vec<Result<Feature, (u64, String)>> = doc.features[..doc.marker.min(doc.features.len())]
            .iter()
            .map(|f| f.evaluated(doc.units, &vars).map_err(|e| (f.uid, e)))
            .collect();
        let fp = |r: &Result<Feature, (u64, String)>| match r {
            Ok(f) => fingerprint(f),
            Err((uid, e)) => format!("error{uid}{e}"),
        };
        let mut model = Model::default();
        let mut reuse = 0;
        for (i, f) in active.iter().enumerate() {
            match self.cache.get(i) {
                Some((k, m)) if *k == fp(f) => {
                    model = m.clone();
                    reuse = i + 1;
                }
                _ => break,
            }
        }
        self.cache.truncate(reuse);
        for r in &active[reuse..] {
            let f = match r {
                Ok(f) => f,
                Err((uid, e)) => {
                    model.errors.insert(*uid, e.clone());
                    self.cache.push((fp(r), model.clone()));
                    continue;
                }
            };
            if !f.suppressed {
                // Kernel panics must never take the application down.
                let mut next = model.clone();
                match m360_kernel::quiet_catch(|| apply(&mut next, f)) {
                    Some(Ok(())) => model = next,
                    Some(Err(e)) => {
                        model.errors.insert(f.uid, e);
                    }
                    None => {
                        model.errors.insert(f.uid, "the geometry kernel failed on this feature".into());
                    }
                }
            }
            self.cache.push((fp(r), model.clone()));
        }
        model
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

fn take(model: &mut Model, id: BodyId) -> Result<Body, String> {
    let idx = model
        .bodies
        .iter()
        .position(|b| b.id == id)
        .ok_or_else(|| format!("referenced body #{id} no longer exists"))?;
    Ok(model.bodies.remove(idx))
}

fn new_body(id: BodyId, solid: Solid, feature: u64) -> Body {
    new_body_smooth(id, solid, feature, SMOOTH_COS)
}

fn new_body_smooth(id: BodyId, solid: Solid, feature: u64, smooth_cos: f32) -> Body {
    let mesh = Arc::new(if smooth_cos < SMOOTH_COS { tessellate_smooth(&solid, smooth_cos) } else { tessellate(&solid) });
    Body { id, solid, mesh, last_feature: feature, smooth_cos }
}

/// A modified version of an existing body (keeps its display settings).
fn rebuilt(model: &Model, id: BodyId, solid: Solid, feature: u64) -> Body {
    let cos = model.body(id).map_or(SMOOTH_COS, |b| b.smooth_cos);
    new_body_smooth(id, solid, feature, cos)
}

/// Build a primitive feature's solid (also used for live command previews).
pub fn build_primitive(kind: &FeatureKind) -> Result<Option<Solid>, String> {
    let r = match kind {
        FeatureKind::Box { plane, center, length, width, height } => {
            primitives::make_box(&plane.frame(*center), *length, *width, *height)
        }
        FeatureKind::Cylinder { plane, center, diameter, height } => {
            primitives::make_cylinder(&plane.frame(*center), *diameter, *height)
        }
        FeatureKind::Sphere { plane, center, diameter } => {
            primitives::make_sphere(&plane.frame(*center), *diameter)
        }
        FeatureKind::Torus { plane, center, inner_diameter, torus_diameter } => {
            primitives::make_torus(&plane.frame(*center), *inner_diameter, *torus_diameter)
        }
        _ => return Ok(None),
    };
    r.map(Some).map_err(|e| e.to_string())
}

fn replace(model: &mut Model, body: Body) {
    if let Some(slot) = model.bodies.iter_mut().find(|b| b.id == body.id) {
        *slot = body;
    }
}

/// Resolve the selected profile indices into regions, merging adjacent
/// profiles (a region plus the profile filling one of its holes).
fn selected_regions(entities: &[SketchEntity], profiles: &[usize]) -> Result<Vec<profile::Region>, String> {
    if profiles.is_empty() {
        return Err("no profile selected".into());
    }
    let regions = crate::sketch::profiles(entities);
    let mut selected = Vec::new();
    for &i in profiles {
        selected.push(regions.get(i).ok_or("a selected profile no longer exists")?.clone());
    }
    let outers: Vec<_> = selected.iter().map(|r| r.outer.clone()).collect();
    let mut fillers = Vec::new();
    for r in &mut selected {
        r.holes.retain(|h| {
            let filled = outers.contains(h);
            if filled {
                fillers.push(h.clone());
            }
            !filled
        });
    }
    selected.retain(|r| !fillers.contains(&r.outer));
    Ok(selected)
}

/// Build the extrusion tool body of the selected sketch profiles over the
/// normal range `[z0, z1]`.
pub fn extrude_tool_range(
    plane: &PlaneRef,
    entities: &[SketchEntity],
    profiles: &[usize],
    z0: f64,
    z1: f64,
) -> Result<Solid, String> {
    let frame = plane.frame([0.0, 0.0]);
    let mut shells = Vec::new();
    for r in &selected_regions(entities, profiles)? {
        let s = profile::extrude_range(&frame, r, z0, z1).map_err(|e| e.to_string())?;
        shells.extend(s.into_boundaries());
    }
    Ok(Solid::new(shells))
}

pub fn extrude_tool(
    plane: &PlaneRef,
    entities: &[SketchEntity],
    profiles: &[usize],
    distance: f64,
    symmetric: bool,
) -> Result<Solid, String> {
    let (z0, z1) = profile::extent(distance, symmetric);
    extrude_tool_range(plane, entities, profiles, z0, z1)
}

/// Extent of a body along the plane normal, measured from the plane.
fn normal_extent(body: &Body, plane: &PlaneRef) -> (f64, f64) {
    let f = plane.frame([0.0, 0.0]);
    let mut lo = f64::MAX;
    let mut hi = f64::MIN;
    for p in &body.mesh.positions {
        let d = (glam::DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64) - f.origin).dot(f.n);
        lo = lo.min(d);
        hi = hi.max(d);
    }
    (lo, hi)
}

/// True when the selected profiles contain curves approximated by many
/// short, almost collinear line segments (text, imported outlines).
fn faceted_profiles(entities: &[SketchEntity], profiles: &[usize]) -> bool {
    let regions = crate::sketch::profiles(entities);
    let mut smooth_vertices = 0;
    for i in profiles {
        let Some(r) = regions.get(*i) else { continue };
        for l in std::iter::once(&r.outer).chain(r.holes.iter()) {
            if let profile::Loop2::Polygon(p) = l {
                let n = p.len();
                for k in 0..n {
                    let (a, b, c) = (p[(k + n - 1) % n], p[k], p[(k + 1) % n]);
                    let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
                    let cosang = (u[0] * v[0] + u[1] * v[1]) / (u[0].hypot(u[1]) * v[0].hypot(v[1])).max(1e-18);
                    if cosang > 0.94 && cosang < 0.99995 {
                        smooth_vertices += 1;
                    }
                }
            }
        }
    }
    smooth_vertices >= 6
}

/// World-space axis (point, direction) of a revolve.
pub fn revolve_axis(plane: &PlaneRef, entities: &[SketchEntity], axis: &RevolveAxis) -> Result<(glam::DVec3, glam::DVec3), String> {
    Ok(match axis {
        RevolveAxis::X => (glam::DVec3::ZERO, glam::DVec3::X),
        RevolveAxis::Y => (glam::DVec3::ZERO, glam::DVec3::Y),
        RevolveAxis::Z => (glam::DVec3::ZERO, glam::DVec3::Z),
        RevolveAxis::SketchLine(i) => match entities.get(*i) {
            Some(SketchEntity::Line { a, b }) => {
                let f = plane.frame([0.0, 0.0]);
                let pa = f.to_world(glam::DVec3::new(a[0], a[1], 0.0));
                let pb = f.to_world(glam::DVec3::new(b[0], b[1], 0.0));
                (pa, (pb - pa).normalize())
            }
            _ => return Err("the revolve axis line no longer exists".into()),
        },
    })
}

/// Revolve the selected profiles of a sketch.
pub fn revolve_tool(plane: &PlaneRef, entities: &[SketchEntity], profiles: &[usize], axis: &RevolveAxis, angle_deg: f64) -> Result<Solid, String> {
    let (p, d) = revolve_axis(plane, entities, axis)?;
    let frame = plane.frame([0.0, 0.0]);
    let mut shells = Vec::new();
    for r in &selected_regions(entities, profiles)? {
        let s = profile::revolve(&frame, r, p, d, angle_deg).map_err(|e| e.to_string())?;
        shells.extend(s.into_boundaries());
    }
    Ok(Solid::new(shells))
}

/// Tool solids of a Hole feature (all points), to be cut in order.
pub fn hole_tools(plane: &PlaneRef, points: &[[f64; 2]], diameter: f64, depth: f64, through_all: bool, kind: &HoleType) -> Result<Vec<Solid>, String> {
    use m360_kernel::hole::{HoleKind, HoleSpec};
    if points.is_empty() {
        return Err("no hole positions".into());
    }
    let spec = HoleSpec {
        diameter,
        depth: if through_all { 1.0e4 } else { depth },
        kind: match *kind {
            HoleType::Simple => HoleKind::Simple,
            HoleType::Counterbore { diameter, depth } => HoleKind::Counterbore { diameter, depth },
            HoleType::Countersink { diameter, angle_deg } => HoleKind::Countersink { diameter, angle_deg },
        },
    };
    let mut out = Vec::new();
    for p in points {
        let f = plane.frame(*p);
        out.extend(m360_kernel::hole::hole_tools(f.origin, f.n, &spec).map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Which prism caps contain the given points.
pub fn shell_open_caps(mesh: &BodyMesh, points: &[[f64; 3]]) -> m360_kernel::shell::OpenCaps {
    let mut open = m360_kernel::shell::OpenCaps::default();
    if let Some(p) = m360_kernel::shell::detect_prism(mesh) {
        for q in points {
            let h = (glam::DVec3::from(*q) - p.frame.origin).dot(p.frame.n);
            if h.abs() < 1e-3 {
                open.bottom = true;
            } else if (h - p.height).abs() < 1e-3 {
                open.top = true;
            }
        }
    }
    open
}

fn sketch_of(model: &Model, uid: u64) -> Result<(PlaneRef, Vec<SketchEntity>), String> {
    model
        .sketches
        .iter()
        .find(|s| s.uid == uid)
        .map(|s| (s.plane, s.entities.clone()))
        .ok_or_else(|| "a sketch used by this feature no longer exists".to_string())
}

/// Order sketch lines/arcs into a connected 3D path.
pub fn path_segments(model: &Model, sketch: u64, path: &[usize]) -> Result<Vec<m360_kernel::sweep::PathSeg>, String> {
    use m360_kernel::sweep::PathSeg;
    let (plane, ents) = sketch_of(model, sketch)?;
    // (start, end, arc midpoint) in plane coordinates.
    let mut segs: Vec<crate::sketch::ChainSeg> = Vec::new();
    for &i in path {
        match ents.get(i) {
            Some(SketchEntity::Line { a, b }) => segs.push((*a, *b, None)),
            Some(SketchEntity::Arc { a, m, b }) => segs.push((*a, *b, Some(*m))),
            _ => return Err("the path must consist of sketch lines and arcs".into()),
        }
    }
    if segs.is_empty() {
        return Err("no path selected".into());
    }
    let same = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-6;
    let degree = |p: [f64; 2], segs: &[crate::sketch::ChainSeg]| {
        segs.iter().filter(|s| same(s.0, p) || same(s.1, p)).count()
    };
    // Start at an open end if there is one.
    let start_idx = segs.iter().position(|s| degree(s.0, &segs) == 1 || degree(s.1, &segs) == 1).unwrap_or(0);
    let mut first = segs.remove(start_idx);
    if degree(first.0, &segs) > 0 && degree(first.1, &segs) == 0 {
        first = (first.1, first.0, first.2);
    }
    let mut ordered = vec![first];
    while !segs.is_empty() {
        let tail = ordered.last().unwrap().1;
        let Some(k) = segs.iter().position(|s| same(s.0, tail) || same(s.1, tail)) else {
            return Err("the path is not connected".into());
        };
        let s = segs.remove(k);
        ordered.push(if same(s.0, tail) { s } else { (s.1, s.0, s.2) });
    }
    let f = plane.frame([0.0, 0.0]);
    let w = |p: [f64; 2]| f.to_world(glam::DVec3::new(p[0], p[1], 0.0));
    Ok(ordered
        .into_iter()
        .map(|(a, b, m)| match m {
            None => PathSeg::Line { a: w(a), b: w(b) },
            Some(m) => PathSeg::Arc { a: w(a), m: w(m), b: w(b) },
        })
        .collect())
}

/// Tool solid of a Pipe feature.
pub fn pipe_tool(segs: &[m360_kernel::sweep::PathSeg], diameter: f64, wall: f64) -> Result<Solid, String> {
    use m360_kernel::profile::{Loop2, Region};
    let r = diameter / 2.0;
    if r <= 1e-6 {
        return Err("diameter must be greater than zero".into());
    }
    let t = segs.first().ok_or("no path selected")?.start_tangent();
    let u = t.any_orthonormal_vector();
    let frame = m360_kernel::Frame::new(segs[0].start(), u, t.cross(u));
    let holes = if wall > 1e-6 && wall < r { vec![Loop2::Circle { c: [0.0, 0.0], r: r - wall }] } else { vec![] };
    let region = Region { outer: Loop2::Circle { c: [0.0, 0.0], r }, holes };
    m360_kernel::sweep::sweep_region(&frame, &region, segs).map_err(|e| e.to_string())
}

/// Apply a tool body with a Fusion-style operation to the participants.
fn apply_op(model: &mut Model, uid: u64, tool: Solid, op: ExtrudeOp, participants: &[BodyId]) -> Result<(), String> {
    let live: Vec<BodyId> = participants.iter().copied().filter(|id| model.body(*id).is_some()).collect();
    match op {
        ExtrudeOp::NewBody => model.bodies.push(new_body(uid, tool, uid)),
        ExtrudeOp::Join if live.is_empty() => model.bodies.push(new_body(uid, tool, uid)),
        ExtrudeOp::Join => {
            let target = live[0];
            let t = model.body(target).unwrap();
            let joined = ops::boolean(&t.solid, std::slice::from_ref(&tool), BooleanOp::Join).unwrap_or_else(|_| {
                let mut shells = t.solid.boundaries().clone();
                shells.extend(tool.into_boundaries());
                Solid::new(shells)
            });
            replace(model, rebuilt(model, target, joined, uid));
        }
        ExtrudeOp::Cut | ExtrudeOp::Intersect => {
            if live.is_empty() {
                return Err("no bodies to cut/intersect".into());
            }
            let bop = if op == ExtrudeOp::Cut { BooleanOp::Cut } else { BooleanOp::Intersect };
            for id in live {
                let t = model.body(id).unwrap();
                let res = ops::boolean(&t.solid, std::slice::from_ref(&tool), bop).map_err(|e| e.to_string())?;
                replace(model, rebuilt(model, id, res, uid));
            }
        }
    }
    Ok(())
}

/// Match stored edge endpoints against the straight edges of a body.
pub fn resolve_edges(mesh: &BodyMesh, edges: &[[[f64; 3]; 2]]) -> Result<Vec<fillet::EdgeSpec>, String> {
    if edges.is_empty() {
        return Err("no edges selected".into());
    }
    let specs: Vec<fillet::EdgeSpec> = fillet::edge_specs(mesh).into_iter().flatten().collect();
    edges
        .iter()
        .map(|[a, b]| {
            let (a, b) = (glam::DVec3::from(*a), glam::DVec3::from(*b));
            let len = (b - a).length();
            specs
                .iter()
                .map(|s| (((s.a - a).length() + (s.b - b).length()).min((s.a - b).length() + (s.b - a).length()), s))
                .filter(|(d, _)| *d < (0.25 * len).max(0.5))
                .min_by(|x, y| x.0.total_cmp(&y.0))
                .map(|(_, s)| *s)
                .ok_or_else(|| "a filleted edge no longer exists (or is not a straight edge between flat faces)".to_string())
        })
        .collect()
}

fn apply(model: &mut Model, f: &Feature) -> Result<(), String> {
    if let Some(solid) = build_primitive(&f.kind)? {
        model.bodies.push(new_body(f.uid, solid, f.uid));
        return Ok(());
    }
    match &f.kind {
        FeatureKind::Combine { target, tools, op, keep_tools } => {
            if tools.is_empty() {
                return Err("no tool bodies selected".into());
            }
            if tools.contains(target) {
                return Err("target body cannot also be a tool".into());
            }
            let t = model.body(*target).ok_or("target body no longer exists")?;
            let tool_solids = tools
                .iter()
                .map(|id| model.body(*id).map(|b| b.solid.clone()))
                .collect::<Option<Vec<_>>>()
                .ok_or("a tool body no longer exists")?;
            let result = match ops::boolean(&t.solid, &tool_solids, *op) {
                Ok(r) => r,
                // Touching bodies (coplanar faces) cannot be fused by truck;
                // keep their shells together in one body instead.
                Err(_) if *op == BooleanOp::Join => {
                    let mut shells = t.solid.boundaries().clone();
                    for s in &tool_solids {
                        shells.extend(s.boundaries().iter().cloned());
                    }
                    Solid::new(shells)
                }
                Err(e) => return Err(e.to_string()),
            };
            let idx = model.bodies.iter().position(|b| b.id == *target).unwrap();
            model.bodies[idx] = rebuilt(model, *target, result, f.uid);
            if !keep_tools {
                model.bodies.retain(|b| !tools.contains(&b.id));
            }
            Ok(())
        }
        FeatureKind::Move { .. } | FeatureKind::Pattern { .. } | FeatureKind::Mirror { .. } if f.kind.copy_transforms().is_some() => {
            let (bodies, mats) = f.kind.copy_transforms().unwrap();
            if bodies.is_empty() {
                return Err("no bodies selected".into());
            }
            if mats.is_empty() {
                return Err("the pattern has a single instance".into());
            }
            for (i, id) in bodies.iter().enumerate() {
                let b = model.body(*id).ok_or("a source body no longer exists")?.clone();
                for (k, m) in mats.iter().enumerate() {
                    let mut solid = ops::transform(&b.solid, *m);
                    if m.determinant() < 0.0 {
                        solid.not(); // reflections turn solids inside out
                    }
                    let id = (f.uid << 16) | (i * mats.len() + k) as u64;
                    model.bodies.push(new_body_smooth(id, solid, f.uid, b.smooth_cos));
                }
            }
            Ok(())
        }
        FeatureKind::Move { bodies, translate, axis, angle_deg, .. } => {
            if bodies.is_empty() {
                return Err("no bodies selected".into());
            }
            let m = FeatureKind::move_matrix(*translate, *axis, *angle_deg);
            for id in bodies.iter() {
                let b = model.body(*id).ok_or("a moved body no longer exists")?;
                let solid = ops::transform(&b.solid, m);
                {
                    let idx = model.bodies.iter().position(|x| x.id == *id).unwrap();
                    model.bodies[idx] = rebuilt(model, *id, solid, f.uid);
                }
            }
            Ok(())
        }
        FeatureKind::Remove { bodies } => {
            for id in bodies {
                take(model, *id)?;
            }
            Ok(())
        }
        FeatureKind::Sketch { plane, entities, constraints } => {
            model.sketches.push(SketchGeom {
                uid: f.uid,
                name: f.name.clone(),
                plane: *plane,
                entities: entities.clone(),
                constraints: constraints.clone(),
            });
            Ok(())
        }
        FeatureKind::Extrude { sketch, profiles, distance, symmetric, op, participants } => {
            let sk = model
                .sketches
                .iter()
                .find(|s| s.uid == *sketch)
                .ok_or("the sketch used by this feature no longer exists")?;
            let (plane, entities) = (sk.plane, sk.entities.clone());
            let tool = extrude_tool(&plane, &entities, profiles, *distance, *symmetric)?;
            let live: Vec<BodyId> =
                participants.iter().copied().filter(|id| model.body(*id).is_some()).collect();
            // Curves approximated by many short lines (e.g. text): hide facet edges.
            let cos = if faceted_profiles(&entities, profiles) { 0.97 } else { SMOOTH_COS };
            match op {
                ExtrudeOp::NewBody => model.bodies.push(new_body_smooth(f.uid, tool, f.uid, cos)),
                ExtrudeOp::Join if live.is_empty() => model.bodies.push(new_body_smooth(f.uid, tool, f.uid, cos)),
                ExtrudeOp::Join => {
                    let target = live[0];
                    let t = model.body(target).unwrap();
                    // truck cannot fuse bodies that only touch along coplanar
                    // faces; keep both shells in one body in that case.
                    let joined = ops::boolean(&t.solid, std::slice::from_ref(&tool), BooleanOp::Join)
                        .unwrap_or_else(|_| {
                            let mut shells = t.solid.boundaries().clone();
                            shells.extend(tool.into_boundaries());
                            Solid::new(shells)
                        });
                    replace(model, rebuilt(model, target, joined, f.uid));
                }
                ExtrudeOp::Cut | ExtrudeOp::Intersect => {
                    if live.is_empty() {
                        return Err("no bodies to cut/intersect".into());
                    }
                    let bop = if *op == ExtrudeOp::Cut { BooleanOp::Cut } else { BooleanOp::Intersect };
                    let (z0, z1) = profile::extent(*distance, *symmetric);
                    for id in live {
                        let t = model.body(id).unwrap();
                        // Overshoot tool ends that lie outside the body so the
                        // boolean never sees coplanar faces.
                        let (lo, hi) = normal_extent(t, &plane);
                        let eps = 1e-3;
                        let pad = 1.0 + 0.05 * (z1 - z0);
                        let t0 = if lo >= z0 - eps { z0 - pad } else { z0 };
                        let t1 = if hi <= z1 + eps { z1 + pad } else { z1 };
                        let tool = if bop == BooleanOp::Cut {
                            extrude_tool_range(&plane, &entities, profiles, t0, t1)?
                        } else {
                            tool.clone()
                        };
                        let res = ops::boolean(&t.solid, &[tool], bop).map_err(|e| e.to_string())?;
                        replace(model, rebuilt(model, id, res, f.uid));
                    }
                }
            }
            Ok(())
        }
        FeatureKind::Sweep { sketch, profiles, path_sketch, path, op, participants } => {
            let (plane, ents) = sketch_of(model, *sketch)?;
            let segs = path_segments(model, *path_sketch, path)?;
            let frame = plane.frame([0.0, 0.0]);
            let mut shells = Vec::new();
            for r in &selected_regions(&ents, profiles)? {
                let s = m360_kernel::sweep::sweep_region(&frame, r, &segs).map_err(|e| e.to_string())?;
                shells.extend(s.into_boundaries());
            }
            apply_op(model, f.uid, Solid::new(shells), *op, participants)
        }
        FeatureKind::Pipe { path_sketch, path, diameter, wall, op, participants } => {
            let segs = path_segments(model, *path_sketch, path)?;
            let tool = pipe_tool(&segs, *diameter, *wall)?;
            apply_op(model, f.uid, tool, *op, participants)
        }
        FeatureKind::Loft { sections, op, participants } => {
            let mut secs = Vec::new();
            for (sk, i) in sections {
                let (plane, ents) = sketch_of(model, *sk)?;
                let r = crate::sketch::profiles(&ents).get(*i).cloned().ok_or("a loft profile no longer exists")?;
                secs.push((plane.frame([0.0, 0.0]), r));
            }
            let tool = m360_kernel::sweep::loft(&secs).map_err(|e| e.to_string())?;
            apply_op(model, f.uid, tool, *op, participants)
        }
        FeatureKind::Shell { body, open_points, thickness } => {
            let b = model.body(*body).ok_or("the shelled body no longer exists")?;
            let open = shell_open_caps(&b.mesh, open_points);
            let s = m360_kernel::shell::shell(&b.solid, &b.mesh, *thickness, open).map_err(|e| e.to_string())?;
            // Curved walls are offset as fine polygons: hide their facet edges.
            let cos = b.smooth_cos.min(0.985);
            replace(model, new_body_smooth(*body, s, f.uid, cos));
            Ok(())
        }
        FeatureKind::SplitBody { body, plane } => {
            let b = model.body(*body).ok_or("the body to split no longer exists")?;
            let fr = plane.frame([0.0, 0.0]);
            let (front, back) = ops::split(&b.solid, fr.origin, fr.n).map_err(|e| e.to_string())?;
            let cos = b.smooth_cos;
            replace(model, rebuilt(model, *body, front, f.uid));
            model.bodies.push(new_body_smooth(f.uid, back, f.uid, cos));
            Ok(())
        }
        FeatureKind::Scale { bodies, point, factor } => {
            for id in bodies {
                let b = model.body(*id).ok_or("a scaled body no longer exists")?;
                let s = ops::scale(&b.solid, glam::DVec3::from(*point), *factor).map_err(|e| e.to_string())?;
                replace(model, rebuilt(model, *id, s, f.uid));
            }
            Ok(())
        }
        FeatureKind::Joint { moving, a, b, motion } => {
            let m = crate::feature::JointGeo::mate(a, b, motion);
            for id in moving {
                let body = model.body(*id).ok_or("a jointed body no longer exists")?;
                let s = ops::transform(&body.solid, m);
                replace(model, rebuilt(model, *id, s, f.uid));
            }
            Ok(())
        }
        FeatureKind::ConstructionPlane { plane } => {
            model.planes.push(PlaneGeom { uid: f.uid, name: f.name.clone(), plane: *plane });
            Ok(())
        }
        FeatureKind::Revolve { sketch, profiles, axis, angle_deg, op, participants } => {
            let sk = model
                .sketches
                .iter()
                .find(|s| s.uid == *sketch)
                .ok_or("the sketch used by this feature no longer exists")?;
            let (plane, entities) = (sk.plane, sk.entities.clone());
            let tool = revolve_tool(&plane, &entities, profiles, axis, *angle_deg)?;
            apply_op(model, f.uid, tool, *op, participants)
        }
        FeatureKind::Hole { plane, points, diameter, depth, through_all, kind, participants } => {
            let live: Vec<BodyId> = participants.iter().copied().filter(|id| model.body(*id).is_some()).collect();
            if live.is_empty() {
                return Err("the hole does not intersect any body".into());
            }
            // Through All: just past the deepest point of the bodies.
            let depth = if *through_all {
                let n = plane.normal();
                let o = plane.frame([0.0, 0.0]).origin;
                let deepest = live
                    .iter()
                    .filter_map(|id| model.body(*id))
                    .flat_map(|b| b.mesh.positions.iter())
                    .map(|p| (o - glam::Vec3::from(*p).as_dvec3()).dot(n))
                    .fold(0.0, f64::max);
                deepest + 1.0 + diameter * 0.1
            } else {
                *depth
            };
            let tools = hole_tools(plane, points, *diameter, depth, false, kind)?;
            for id in live {
                let mut acc = model.body(id).unwrap().solid.clone();
                for t in &tools {
                    acc = ops::boolean(&acc, std::slice::from_ref(t), BooleanOp::Cut).map_err(|e| e.to_string())?;
                }
                replace(model, rebuilt(model, id, acc, f.uid));
            }
            Ok(())
        }
        FeatureKind::Fillet { body, edges, size, chamfer } => {
            let b = model.body(*body).ok_or("the filleted body no longer exists")?;
            let specs = resolve_edges(&b.mesh, edges)?;
            let t = if *chamfer {
                fillet::Treatment::Chamfer { distance: *size }
            } else {
                fillet::Treatment::Fillet { radius: *size }
            };
            let (solid, faceted) = fillet::treat_edges_report(&b.solid, &specs, t).map_err(|e| e.to_string())?;
            let cos = if faceted { b.smooth_cos.min(0.97) } else { b.smooth_cos };
            replace(model, new_body_smooth(*body, solid, f.uid, cos));
            Ok(())
        }
        _ => unreachable!("primitives handled above"),
    }
}
