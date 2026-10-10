//! Regeneration: replaying the timeline into B-rep bodies.

use std::collections::HashMap;
use std::sync::Arc;

use m360_kernel::{ops, ops::BooleanOp, primitives, profile, tessellate, BodyMesh, Solid};

use crate::document::Document;
use crate::feature::{ExtrudeOp, Feature, FeatureKind, SketchEntity};
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
}

#[derive(Clone)]
pub struct SketchGeom {
    pub uid: u64,
    pub name: String,
    pub plane: PlaneRef,
    pub entities: Vec<SketchEntity>,
}

/// Result of regenerating a document.
#[derive(Clone, Default)]
pub struct Model {
    pub bodies: Vec<Body>,
    pub sketches: Vec<SketchGeom>,
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
        let active = &doc.features[..doc.marker.min(doc.features.len())];
        let mut model = Model::default();
        let mut reuse = 0;
        for (i, f) in active.iter().enumerate() {
            match self.cache.get(i) {
                Some((fp, m)) if *fp == fingerprint(f) => {
                    model = m.clone();
                    reuse = i + 1;
                }
                _ => break,
            }
        }
        self.cache.truncate(reuse);
        for f in &active[reuse..] {
            if !f.suppressed {
                if let Err(e) = apply(&mut model, f) {
                    model.errors.insert(f.uid, e);
                }
            }
            self.cache.push((fingerprint(f), model.clone()));
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
    let mesh = Arc::new(tessellate(&solid));
    Body { id, solid, mesh, last_feature: feature }
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
            model.bodies[idx] = new_body(*target, result, f.uid);
            if !keep_tools {
                model.bodies.retain(|b| !tools.contains(&b.id));
            }
            Ok(())
        }
        FeatureKind::Move { bodies, translate, axis, angle_deg, copy } => {
            if bodies.is_empty() {
                return Err("no bodies selected".into());
            }
            let m = FeatureKind::move_matrix(*translate, *axis, *angle_deg);
            for (i, id) in bodies.iter().enumerate() {
                let b = model.body(*id).ok_or("a moved body no longer exists")?;
                let solid = ops::transform(&b.solid, m);
                if *copy {
                    model.bodies.push(new_body((f.uid << 16) | i as u64, solid, f.uid));
                } else {
                    let idx = model.bodies.iter().position(|x| x.id == *id).unwrap();
                    model.bodies[idx] = new_body(*id, solid, f.uid);
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
        FeatureKind::Sketch { plane, entities } => {
            model.sketches.push(SketchGeom {
                uid: f.uid,
                name: f.name.clone(),
                plane: *plane,
                entities: entities.clone(),
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
            match op {
                ExtrudeOp::NewBody => model.bodies.push(new_body(f.uid, tool, f.uid)),
                ExtrudeOp::Join if live.is_empty() => model.bodies.push(new_body(f.uid, tool, f.uid)),
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
                    replace(model, new_body(target, joined, f.uid));
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
                        replace(model, new_body(id, res, f.uid));
                    }
                }
            }
            Ok(())
        }
        _ => unreachable!("primitives handled above"),
    }
}
