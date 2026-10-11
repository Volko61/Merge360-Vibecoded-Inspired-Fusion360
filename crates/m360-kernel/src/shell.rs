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
    for t in mesh.indices.chunks_exact(3) {
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
            let Some(n) = next.remove(&cur) else { return None };
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
    let na = Loop2::Polygon(out.clone()).signed_area();
    (na.signum() == area.signum() && na.abs() > 1e-9).then_some(out)
}

/// Hollow out a prismatic solid with walls of `thickness`.
pub fn shell(solid: &Solid, mesh: &BodyMesh, thickness: f64, open: OpenCaps) -> Result<Solid> {
    if !(thickness > 1e-6) {
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
    fn offset_square() {
        let sq = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let o = offset_polygon(&sq, -1.0).unwrap();
        assert!((Loop2::Polygon(o).area() - 64.0).abs() < 1e-9);
    }
}
