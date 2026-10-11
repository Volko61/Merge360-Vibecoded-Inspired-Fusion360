//! Shell (hollow out) for prismatic bodies.
//!
//! A body is prismatic along a direction `d` when every face is either a cap
//! (normal ±d) at one of two heights, or a side face (normals ⟂ d). The
//! cavity is the cross-section offset inwards by the wall thickness, extruded
//! between the caps (through a removed cap), and cut from the body.

use glam::{DVec3, Vec3};

use crate::ops::{boolean, BooleanOp};
use crate::profile::{extrude_range, Loop2, Region};
use crate::{BodyMesh, Frame, KernelError, Result, Solid};

/// Which caps of the prism are removed (left open).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpenCaps {
    pub top: bool,
    pub bottom: bool,
}

/// Prism description of a body.
#[derive(Debug, Clone)]
pub struct Prism {
    /// Frame on the bottom cap; its normal points from bottom to top.
    pub frame: Frame,
    pub height: f64,
    pub region: Region,
    /// Face ids of the bottom and top caps.
    pub bottom_faces: Vec<u32>,
    pub top_faces: Vec<u32>,
}

fn face_normals(mesh: &BodyMesh, face: u32) -> impl Iterator<Item = Vec3> + '_ {
    mesh.face_ids.iter().zip(&mesh.normals).filter(move |(f, _)| **f == face).map(|(_, n)| Vec3::from(*n))
}

/// Detect whether a tessellated body is a prism.
pub fn detect_prism(mesh: &BodyMesh) -> Option<Prism> {
    let faces: Vec<u32> = (0..mesh.face_count).collect();
    let candidates: Vec<Vec3> = faces.iter().filter_map(|f| mesh.planar_normal(*f)).collect();
    'cand: for d in candidates {
        let mut bottom = Vec::new();
        let mut top = Vec::new();
        for &f in &faces {
            let ns: Vec<Vec3> = face_normals(mesh, f).collect();
            if ns.is_empty() {
                continue;
            }
            if ns.iter().all(|n| n.dot(d) > 0.999) {
                top.push(f);
            } else if ns.iter().all(|n| n.dot(d) < -0.999) {
                bottom.push(f);
            } else if !ns.iter().all(|n| n.dot(d).abs() < 0.01) {
                continue 'cand;
            }
        }
        if top.is_empty() || bottom.is_empty() {
            continue;
        }
        // Caps must sit at two heights only.
        let height_of = |fs: &[u32]| -> Option<f32> {
            let hs: Vec<f32> = mesh
                .triangles()
                .filter(|(_, f)| fs.contains(f))
                .flat_map(|(t, _)| t.map(|p| p.dot(d)))
                .collect();
            let (lo, hi) = hs.iter().fold((f32::MAX, f32::MIN), |(a, b), h| (a.min(*h), b.max(*h)));
            ((hi - lo).abs() < 1e-3 * (1.0 + hi.abs())).then_some(lo)
        };
        let (Some(z0), Some(z1)) = (height_of(&bottom), height_of(&top)) else { continue };
        if z1 - z0 < 1e-4 {
            continue;
        }
        // Cross-section loops from the bottom cap's boundary edges.
        let dd = d.as_dvec3();
        let u = dd.any_orthonormal_vector();
        let v = dd.cross(u);
        let origin = dd * z0 as f64;
        let frame = Frame::new(origin, u, v);
        let loops = cap_loops(mesh, &bottom, &frame)?;
        let region = region_from_loops(loops)?;
        return Some(Prism { frame, height: (z1 - z0) as f64, region, bottom_faces: bottom, top_faces: top });
    }
    None
}

/// Boundary loops (2D, in `frame`) of the triangles of the given faces.
fn cap_loops(mesh: &BodyMesh, faces: &[u32], frame: &Frame) -> Option<Vec<Vec<[f64; 2]>>> {
    use std::collections::HashMap;
    let key = |p: [f32; 3]| [(p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64, (p[2] * 1e4).round() as i64];
    let mut pos: HashMap<[i64; 3], [f32; 3]> = HashMap::new();
    let mut count: HashMap<([i64; 3], [i64; 3]), i32> = HashMap::new();
    for t in mesh.indices.as_chunks::<3>().0 {
        if !faces.contains(&mesh.face_ids[t[0] as usize]) {
            continue;
        }
        let ps: Vec<[f32; 3]> = t.iter().map(|&i| mesh.positions[i as usize]).collect();
        for k in 0..3 {
            let (a, b) = (key(ps[k]), key(ps[(k + 1) % 3]));
            pos.insert(a, ps[k]);
            // Directed edge; an interior edge appears once in each direction.
            *count.entry((a, b)).or_default() += 1;
            *count.entry((b, a)).or_default() -= 1;
        }
    }
    let mut next: HashMap<[i64; 3], [i64; 3]> = HashMap::new();
    for ((a, b), c) in &count {
        if *c > 0 {
            next.insert(*a, *b);
        }
    }
    let mut loops = Vec::new();
    while let Some(&start) = next.keys().next() {
        let mut lp = Vec::new();
        let mut cur = start;
        loop {
            let p = Vec3::from(pos[&cur]).as_dvec3() - frame.origin;
            lp.push([p.dot(frame.u), p.dot(frame.v)]);
            let n = next.remove(&cur)?;
            cur = n;
            if cur == start {
                break;
            }
            if lp.len() > 100_000 {
                return None;
            }
        }
        if lp.len() >= 3 {
            loops.push(simplify(lp));
        }
    }
    (!loops.is_empty()).then_some(loops)
}

/// Remove collinear points.
fn simplify(p: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let n = p.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (a, b, c) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
        let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]) * (c[0] - b[0]).hypot(c[1] - b[1]);
        if cross.abs() > 1e-9 * len.max(1e-12) {
            out.push(b);
        }
    }
    out
}

fn region_from_loops(loops: Vec<Vec<[f64; 2]>>) -> Option<Region> {
    let mut ls: Vec<Loop2> = loops.into_iter().map(Loop2::Polygon).collect();
    ls.sort_by(|a, b| b.area().total_cmp(&a.area()));
    let outer = ls.remove(0);
    // Remaining loops must be holes inside the outer loop.
    for h in &ls {
        if !outer.contains(h.sample()) && !outer.contains(h.points(8)[1]) {
            return None;
        }
    }
    Some(Region { outer, holes: ls })
}

/// Offset a closed polygon by `t` (positive = grow) with mitered joins.
pub fn offset_polygon(p: &[[f64; 2]], t: f64) -> Option<Vec<[f64; 2]>> {
    let n = p.len();
    if n < 3 {
        return None;
    }
    let area = Loop2::Polygon(p.to_vec()).signed_area();
    let s = if area > 0.0 { 1.0 } else { -1.0 }; // CCW: outward normal is the right normal
    let normal = |a: [f64; 2], b: [f64; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = dx.hypot(dy).max(1e-12);
        [s * dy / l, -s * dx / l]
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (a, b, c) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
        let (n1, n2) = (normal(a, b), normal(b, c));
        let bis = [n1[0] + n2[0], n1[1] + n2[1]];
        let bl = bis[0].hypot(bis[1]);
        if bl < 1e-9 {
            return None;
        }
        let cosh = (bl / 2.0).max(0.2); // cos(half angle); limit very sharp miters
        let k = t / cosh;
        out.push([b[0] + bis[0] / bl * k, b[1] + bis[1] / bl * k]);
    }
    // Every edge must keep its direction (no inverted, collapsed edges).
    let flipped = (0..n).any(|i| {
        let j = (i + 1) % n;
        let (d0, d1) = ([p[j][0] - p[i][0], p[j][1] - p[i][1]], [out[j][0] - out[i][0], out[j][1] - out[i][1]]);
        d0[0] * d1[0] + d0[1] * d1[1] <= 0.0
    });
    let na = Loop2::Polygon(out.clone()).signed_area();
    (!flipped && na.signum() == area.signum() && na.abs() > 1e-9).then_some(out)
}

/// Hollow out a prismatic solid with walls of `thickness`.
pub fn shell(solid: &Solid, mesh: &BodyMesh, thickness: f64, open: OpenCaps) -> Result<Solid> {
    if thickness.is_nan() || thickness <= 1e-6 {
        return Err(KernelError::InvalidParameter("thickness must be greater than zero".into()));
    }
    let prism = detect_prism(mesh).ok_or_else(|| {
        KernelError::OperationFailed("Shell currently supports prismatic bodies (extrusions of a profile)".into())
    })?;
    let poly = |l: &Loop2| match l {
        Loop2::Polygon(p) => Some(p.clone()),
        other => Some(other.points(64)),
    };
    let outer = offset_polygon(&poly(&prism.region.outer).unwrap(), -thickness)
        .ok_or_else(|| KernelError::OperationFailed("the wall thickness is too large".into()))?;
    let mut holes = Vec::new();
    for h in &prism.region.holes {
        holes.push(Loop2::Polygon(
            offset_polygon(&poly(h).unwrap(), thickness).ok_or_else(|| KernelError::OperationFailed("the wall thickness is too large".into()))?,
        ));
    }
    let cavity = Region { outer: Loop2::Polygon(outer), holes };
    let over = thickness.max(1.0);
    let z0 = if open.bottom { -over } else { thickness };
    let z1 = if open.top { prism.height + over } else { prism.height - thickness };
    if z1 - z0 < 1e-6 {
        return Err(KernelError::OperationFailed("the wall thickness is too large".into()));
    }
    let tool = extrude_range(&prism.frame, &cavity, z0, z1)?;
    let _ = DVec3::ZERO;
    boolean(solid, &[tool], BooleanOp::Cut)
}

/// Offset a closed polygon edge by edge: edge `i` (from `p[i]` to
/// `p[i+1]`) moves `d[i]` along its outward normal (`outward_sign` picks the
/// outward side for the polygon's winding). Corners are the intersections
/// of adjacent offset edges.
fn offset_edges(p: &[[f64; 2]], d: &[f64]) -> Option<Vec<[f64; 2]>> {
    let n = p.len();
    let s = if Loop2::Polygon(p.to_vec()).signed_area() > 0.0 { 1.0 } else { -1.0 };
    let normal = |i: usize| {
        let (a, b) = (p[i], p[(i + 1) % n]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = dx.hypot(dy).max(1e-12);
        [s * dy / l, -s * dx / l]
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let j = (i + n - 1) % n; // previous edge
        let (n1, n2) = (normal(j), normal(i));
        let (c1, c2) = (n1[0] * p[i][0] + n1[1] * p[i][1] + d[j], n2[0] * p[i][0] + n2[1] * p[i][1] + d[i]);
        let det = n1[0] * n2[1] - n1[1] * n2[0];
        if det.abs() < 1e-9 {
            // Collinear edges: only valid when they move together.
            if (d[i] - d[j]).abs() > 1e-9 {
                return None;
            }
            out.push([p[i][0] + n2[0] * d[i], p[i][1] + n2[1] * d[i]]);
        } else {
            out.push([(c1 * n2[1] - c2 * n1[1]) / det, (n1[0] * c2 - n2[0] * c1) / det]);
        }
    }
    let (a0, a1) = (Loop2::Polygon(p.to_vec()).signed_area(), Loop2::Polygon(out.clone()).signed_area());
    (a0.signum() == a1.signum() && a1.abs() > 1e-9).then_some(out)
}

/// Draft (tilt) side faces of a prismatic body by `angle_deg` about its
/// bottom (or top) cap: positive angles make the part smaller away from the
/// fixed cap. `faces` are mesh face ids of side faces.
pub fn draft(mesh: &BodyMesh, faces: &[u32], angle_deg: f64, fixed_top: bool) -> Result<Solid> {
    if !angle_deg.is_finite() || angle_deg.abs() >= 60.0 {
        return Err(KernelError::InvalidParameter("the draft angle must be between -60° and 60°".into()));
    }
    let prism = detect_prism(mesh).ok_or_else(|| {
        KernelError::OperationFailed("Draft currently supports prismatic bodies (extrusions of a profile)".into())
    })?;
    let f = &prism.frame;
    let h = prism.height;
    // "Bottom" is the lower cap in the world (Y up), whichever way the prism
    // was detected.
    let up = [DVec3::Y, DVec3::Z, DVec3::X].into_iter().map(|a| f.n.dot(a)).find(|d| d.abs() > 1e-6).unwrap_or(1.0);
    let fixed_top = fixed_top != (up < 0.0);
    // Does the side face of edge (a, b) belong to the selection?
    let selected = |a: [f64; 2], b: [f64; 2]| -> bool {
        let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let w = (f.origin + f.u * m[0] + f.v * m[1] + f.n * (h / 2.0)).as_vec3();
        mesh.triangles().any(|(t, face)| faces.contains(&face) && on_triangle(&t, w))
    };
    let k = angle_deg.to_radians().tan() * h;
    let as_poly = |l: &Loop2| match l {
        Loop2::Polygon(p) => p.clone(),
        other => other.points(64),
    };
    let shifted = |l: &Loop2, outer: bool| -> Result<Loop2> {
        let p = as_poly(l);
        // Outer walls move in, hole walls move out (the part gets smaller).
        let d: Vec<f64> = (0..p.len()).map(|i| if selected(p[i], p[(i + 1) % p.len()]) { if outer { -k } else { k } } else { 0.0 }).collect();
        offset_edges(&p, &d).map(Loop2::Polygon).ok_or_else(|| KernelError::OperationFailed("the draft angle is too large for this body".into()))
    };
    let base = Region { outer: Loop2::Polygon(as_poly(&prism.region.outer)), holes: prism.region.holes.iter().map(|l| Loop2::Polygon(as_poly(l))).collect() };
    let moved = Region { outer: shifted(&prism.region.outer, true)?, holes: prism.region.holes.iter().map(|l| shifted(l, false)).collect::<Result<_>>()? };
    if base == moved {
        return Err(KernelError::InvalidParameter("select side faces of the body (faces parallel to the pull direction)".into()));
    }
    let sections = if fixed_top { [(0.0, moved), (h, base)] } else { [(0.0, base), (h, moved)] };
    let solid = crate::profile::loft_sections(&sections)?;
    Ok(truck_modeling::builder::transformed(&solid, crate::primitives::to_truck(f.matrix())))
}

/// Whether `p` lies on triangle `t` (within a small tolerance).
pub fn on_triangle(t: &[Vec3; 3], p: Vec3) -> bool {
    let n = (t[1] - t[0]).cross(t[2] - t[0]);
    let len = n.length();
    if len < 1e-12 {
        return false;
    }
    let n = n / len;
    let scale = (t[1] - t[0]).length().max((t[2] - t[0]).length()).max(1.0);
    if (p - t[0]).dot(n).abs() > 1e-3 * scale {
        return false;
    }
    let s = |a: Vec3, b: Vec3| (b - a).cross(p - a).dot(n);
    let (s1, s2, s3) = (s(t[0], t[1]), s(t[1], t[2]), s(t[2], t[0]));
    let tol = -1e-4 * scale * scale;
    s1 >= tol && s2 >= tol && s3 >= tol
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{make_box, make_cylinder};
    use crate::tessellate;

    fn xy() -> Frame {
        Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y)
    }

    #[test]
    fn open_box() {
        let b = make_box(&xy(), 40.0, 30.0, 20.0).unwrap();
        let m = tessellate(&b);
        let s = shell(&b, &m, 2.0, OpenCaps { top: true, bottom: false }).unwrap();
        let v = tessellate(&s).volume();
        let expect = 40.0 * 30.0 * 20.0 - 36.0 * 26.0 * 18.0;
        assert!((v - expect).abs() < 5.0, "{v} vs {expect}");
    }

    #[test]
    fn closed_cylinder_cup() {
        let c = make_cylinder(&xy(), 20.0, 30.0).unwrap();
        let m = tessellate(&c);
        let s = shell(&c, &m, 1.0, OpenCaps { top: true, bottom: false }).unwrap();
        let v = tessellate(&s).volume();
        let pi = std::f64::consts::PI;
        let expect = pi * 100.0 * 30.0 - pi * 81.0 * 29.0;
        assert!((v - expect).abs() / expect < 0.03, "{v} vs {expect}");
    }

    #[test]
    fn draft_two_faces() {
        let b = make_box(&xy(), 20.0, 20.0, 10.0);
        let b = b.unwrap();
        let m = tessellate(&b);
        // The +X and +Y side faces.
        let sides: Vec<u32> = (0..m.face_count)
            .filter(|f| m.planar_normal(*f).is_some_and(|n| n.x > 0.99 || n.y > 0.99))
            .collect();
        assert_eq!(sides.len(), 2);
        let d = draft(&m, &sides, 10.0, false).unwrap();
        let v = tessellate(&d).volume();
        // Each drafted wall removes a wedge of 20 x 10 x (10 tan 10°) / 2 (minus the corner overlap).
        let k = 10.0 * 10f64.to_radians().tan();
        let expect = 20.0 * 20.0 * 10.0 - 2.0 * 20.0 * 10.0 * k / 2.0 + 10.0 * k * k / 3.0;
        assert!((v - expect).abs() < 1.0, "{v} vs {expect}");
        assert!(draft(&m, &[], 10.0, false).is_err());
    }

    #[test]
    fn offset_square() {
        let sq = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let o = offset_polygon(&sq, -1.0).unwrap();
        assert!((Loop2::Polygon(o).area() - 64.0).abs() < 1e-9);
    }
}
