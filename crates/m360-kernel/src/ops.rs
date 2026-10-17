//! Solid operations: booleans and rigid transforms.

use glam::DMat4;
use truck_modeling::builder;

use crate::primitives::to_truck;
use crate::{KernelError, Result, Solid};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    Join,
    Cut,
    Intersect,
}

/// Boolean tolerance in millimetres.
const BOOL_TOL: f64 = 0.05;

/// Combine `target` with each of `tools` in sequence.
pub fn boolean(target: &Solid, tools: &[Solid], op: BooleanOp) -> Result<Solid> {
    boolean_with_tol(target, tools, op, BOOL_TOL)
}

/// [`boolean`] with an explicit intersection tolerance (mm).
pub fn boolean_with_tol(target: &Solid, tools: &[Solid], op: BooleanOp, tol: f64) -> Result<Solid> {
    let mut acc = target.clone();
    for tool in tools {
        let res = guarded(|| match op {
            BooleanOp::Join => truck_shapeops::or(&acc, tool, tol),
            BooleanOp::Intersect => truck_shapeops::and(&acc, tool, tol),
            BooleanOp::Cut => {
                let mut inv = tool.clone();
                inv.not();
                truck_shapeops::and(&acc, &inv, tol)
            }
        })?;
        acc = match res {
            Some(r) => r,
            // truck reports disjoint bodies as a failure: handle them here.
            None if disjoint(&acc, tool) => match op {
                BooleanOp::Join => {
                    let mut shells = acc.boundaries().clone();
                    shells.extend(tool.boundaries().iter().cloned());
                    Solid::new(shells)
                }
                BooleanOp::Cut => acc,
                BooleanOp::Intersect => {
                    return Err(KernelError::OperationFailed("Intersect produced an empty body".into()))
                }
            },
            None => {
                return Err(KernelError::OperationFailed(format!("{op:?} failed: bodies could not be intersected")))
            }
        };
        if acc.boundaries().is_empty() {
            return Err(KernelError::OperationFailed(format!("{op:?} produced an empty body")));
        }
    }
    Ok(acc)
}

/// Conservative bounding-box test (from a coarse tessellation).
fn disjoint(a: &Solid, b: &Solid) -> bool {
    let ma = crate::mesh::tessellate_with(a, 0.5);
    let mb = crate::mesh::tessellate_with(b, 0.5);
    let pad = glam::Vec3::splat(0.6);
    let overlap = (ma.bbox_min - pad).cmple(mb.bbox_max).all() && (mb.bbox_min - pad).cmple(ma.bbox_max).all();
    !overlap
}

/// truck's boolean code can panic on degenerate input (e.g. coplanar faces);
/// turn such panics into a recoverable error.
fn guarded<T>(f: impl FnOnce() -> Option<T>) -> Result<Option<T>> {
    crate::quiet_catch(f).ok_or_else(|| KernelError::OperationFailed("boolean failed: degenerate geometry".into()))
}

/// Split a solid by a plane (point + normal). Returns (front, back) where
/// front lies on the side the normal points to.
pub fn split(solid: &Solid, point: glam::DVec3, normal: glam::DVec3) -> Result<(Solid, Solid)> {
    if !normal.is_finite() || normal.length() < 1e-12 || !point.is_finite() {
        return Err(KernelError::InvalidParameter("the split plane is undefined".into()));
    }
    let mesh = crate::mesh::tessellate_with(solid, 0.5);
    let size = ((mesh.bbox_max - mesh.bbox_min).length() as f64).max(1.0) * 4.0;
    let n = normal.normalize();
    let u = n.any_orthonormal_vector();
    let v = n.cross(u);
    let half = |sign: f64| -> Result<Solid> {
        let frame = crate::Frame::new(point, u, v);
        crate::primitives::make_box(&frame, size, size, sign * size)
    };
    let front = boolean(solid, &[half(1.0)?], BooleanOp::Intersect)
        .map_err(|_| KernelError::OperationFailed("the plane does not split the body (or touches a face)".into()))?;
    let back = boolean(solid, &[half(-1.0)?], BooleanOp::Intersect)
        .map_err(|_| KernelError::OperationFailed("the plane does not split the body (or touches a face)".into()))?;
    Ok((front, back))
}

/// Uniform scale about a point.
pub fn scale(solid: &Solid, point: glam::DVec3, factor: f64) -> Result<Solid> {
    if !factor.is_finite() || factor <= 1e-9 || factor > 1e6 {
        return Err(KernelError::InvalidParameter("scale factor must be positive (at most 1e6)".into()));
    }
    let m = DMat4::from_translation(point) * DMat4::from_scale(glam::DVec3::splat(factor)) * DMat4::from_translation(-point);
    Ok(transform(solid, m))
}

pub fn transform(solid: &Solid, m: DMat4) -> Solid {
    builder::transformed(solid, to_truck(m))
}

/// Sweep face number `face` (in `solid.face_iter()` order) of a solid
/// along the unit `normal` between offsets `z0` and `z1`.
pub fn extrude_face(solid: &Solid, face: usize, normal: glam::DVec3, z0: f64, z1: f64) -> Result<Solid> {
    if z1 - z0 <= 1e-6 || z0.is_nan() || z1.is_nan() {
        return Err(KernelError::InvalidParameter("distance must not be zero".into()));
    }
    let f = solid.face_iter().nth(face).cloned().ok_or_else(|| KernelError::InvalidParameter("the face no longer exists".into()))?;
    // `transformed` copies the topology, so the tool shares nothing with the body.
    let start = builder::transformed(&f, to_truck(DMat4::from_translation(normal * z0)));
    let d = normal * (z1 - z0);
    let mut out: Solid = builder::tsweep(&start, truck_modeling::Vector3::new(d.x, d.y, d.z));
    if crate::tessellate(&out).volume() < 0.0 {
        out.not();
    }
    Ok(out)
}

/// Move a planar face of a solid by `d` along its unit outward `normal`,
/// adding (d > 0) or removing (d < 0) the swept prism. Built directly from
/// the topology (no boolean), so faces coplanar with the body are fine; the
/// prism must not run into other parts of the body.
pub fn push_pull_face(solid: &Solid, face: usize, normal: glam::DVec3, d: f64) -> Result<Solid> {
    use truck_modeling::{Shell, Vector3};
    if d.abs() < 1e-6 || d.is_nan() {
        return Err(KernelError::InvalidParameter("distance must not be zero".into()));
    }
    let f = solid.face_iter().nth(face).cloned().ok_or_else(|| KernelError::InvalidParameter("the face no longer exists".into()))?;
    let v = normal * d;
    let mut prism: Solid = builder::tsweep(&f, Vector3::new(v.x, v.y, v.z));
    if crate::tessellate(&prism).volume() < 0.0 {
        prism.not();
    }
    let prism_vol = crate::tessellate(&prism).volume();
    if d < 0.0 {
        prism.not();
    }
    let mut out_shells = Vec::new();
    let mut found = false;
    for sh in solid.boundaries() {
        if !sh.face_iter().any(|g| g.id() == f.id()) {
            out_shells.push(sh.clone());
            continue;
        }
        found = true;
        let mut shell: Shell = sh.face_iter().filter(|g| g.id() != f.id()).cloned().collect();
        for g in prism.boundaries().iter().flat_map(|s| s.face_iter()) {
            if g.id() != f.id() {
                shell.push(g.clone());
            }
        }
        out_shells.push(shell);
    }
    if !found {
        return Err(KernelError::OperationFailed("the face is not on the body".into()));
    }
    let out = Solid::try_new(out_shells).map_err(|e| KernelError::OperationFailed(format!("push/pull: {e:?}")))?;
    let expect = crate::tessellate(solid).volume() + prism_vol * d.signum();
    let got = crate::tessellate(&out).volume();
    if expect <= 0.0 || (got - expect).abs() > 1e-3 * expect.abs().max(1.0) {
        return Err(KernelError::OperationFailed("push/pull produced an invalid body".into()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn extrude_box_face() {
        use crate::primitives::make_box;
        let f = crate::Frame::new(glam::DVec3::ZERO, glam::DVec3::X, glam::DVec3::Y);
        let b = make_box(&f, 10.0, 10.0, 10.0).unwrap();
        let m = crate::tessellate(&b);
        let top = (0..m.face_count).find(|i| m.planar_normal(*i).is_some_and(|n| n.z > 0.99)).unwrap();
        let t = super::extrude_face(&b, top as usize, glam::DVec3::Z, -0.1, 5.0).unwrap();
        assert!((crate::tessellate(&t).volume() - 510.0).abs() < 1.0);
        let up = super::push_pull_face(&b, top as usize, glam::DVec3::Z, 5.0).unwrap();
        assert!((crate::tessellate(&up).volume() - 1500.0).abs() < 1.0);
        // Edges between the old and new (coplanar) side faces are hidden.
        let um = crate::tessellate(&up);
        assert!(!um.edges.iter().any(|e| e.iter().all(|p| (p[2] - 10.0).abs() < 1e-6)), "seam at the old top is visible");
        let down = super::push_pull_face(&b, top as usize, glam::DVec3::Z, -4.0).unwrap();
        assert!((crate::tessellate(&down).volume() - 600.0).abs() < 1.0);
        // The result is a valid solid for later booleans.
        let c = crate::primitives::make_cylinder(&crate::Frame::new(glam::DVec3::new(0.0, 0.0, -5.0), glam::DVec3::X, glam::DVec3::Y), 4.0, 30.0).unwrap();
        let cut = super::boolean(&up, &[c], super::BooleanOp::Cut).unwrap();
        assert!(crate::tessellate(&cut).volume() < 1500.0 - 100.0);
    }

    use super::*;
    use crate::{primitives::*, tessellate};
    use glam::DVec3;

    fn xy() -> Frame {
        Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y)
    }

    #[test]
    fn cut_box_with_box() {
        let a = make_box(&xy(), 10.0, 10.0, 10.0).unwrap();
        let b = make_box(&Frame::new(DVec3::new(5.0, 0.0, -5.0), DVec3::X, DVec3::Y), 10.0, 20.0, 20.0)
            .unwrap();
        let c = boolean(&a, &[b], BooleanOp::Cut).unwrap();
        let v = tessellate(&c).volume();
        assert!((v - 500.0).abs() < 5.0, "{v}");
    }

    #[test]
    fn split_and_scale() {
        let a = make_box(&xy(), 10.0, 10.0, 10.0).unwrap();
        let (f, b) = split(&a, DVec3::new(2.0, 0.0, 0.0), DVec3::X).unwrap();
        assert!((tessellate(&f).volume() - 300.0).abs() < 1.0);
        assert!((tessellate(&b).volume() - 700.0).abs() < 1.0);
        let s = scale(&a, DVec3::ZERO, 2.0).unwrap();
        assert!((tessellate(&s).volume() - 8000.0).abs() < 1.0);
    }

    #[test]
    fn translate() {
        let a = make_box(&xy(), 10.0, 10.0, 10.0).unwrap();
        let t = transform(&a, DMat4::from_translation(DVec3::new(100.0, 0.0, 0.0)));
        let m = tessellate(&t);
        assert!((m.bbox_min.x - 95.0).abs() < 1e-3);
    }
}
