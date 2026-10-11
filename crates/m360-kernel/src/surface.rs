//! Surface modeling: open shells made from sketch curves, patches,
//! stitching and face removal. Surface bodies are stored as (unchecked)
//! solids whose shells are not closed.

use glam::DVec3;
use truck_modeling::{builder, Point3, Rad, Shell, Vector3, Wire};
use truck_modeling::topology::ShellCondition;

use crate::primitives::to_truck;
use crate::profile::Region;
use crate::{Frame, KernelError, Result, Solid};

/// A curve of an open chain, in sketch-plane coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve2 {
    Line([f64; 2], [f64; 2]),
    /// Arc from the first point through the second to the third.
    Arc([f64; 2], [f64; 2], [f64; 2]),
    /// Polyline (sampled splines).
    Poly(Vec<[f64; 2]>),
}

/// Wire through ordered, connected curves (in `frame`).
pub fn chain_wire(frame: &Frame, curves: &[Curve2]) -> Result<Wire> {
    if curves.is_empty() {
        return Err(KernelError::InvalidParameter("select curves".into()));
    }
    let p3 = |p: [f64; 2]| {
        let w = frame.origin + frame.u * p[0] + frame.v * p[1];
        Point3::new(w.x, w.y, w.z)
    };
    let mut wire = Wire::new();
    let first = match &curves[0] {
        Curve2::Line(a, _) | Curve2::Arc(a, _, _) => *a,
        Curve2::Poly(p) => p[0],
    };
    let mut cur = builder::vertex(p3(first));
    let start = cur.clone();
    let n = curves.len();
    for (i, c) in curves.iter().enumerate() {
        let end_of = |c: &Curve2| match c {
            Curve2::Line(_, b) | Curve2::Arc(_, _, b) => *b,
            Curve2::Poly(p) => p[p.len() - 1],
        };
        // Close the loop on the start vertex.
        let closes = i == n - 1 && (end_of(c)[0] - first[0]).hypot(end_of(c)[1] - first[1]) < 1e-9;
        match c {
            Curve2::Line(_, b) => {
                let v = if closes { start.clone() } else { builder::vertex(p3(*b)) };
                wire.push_back(builder::line(&cur, &v));
                cur = v;
            }
            Curve2::Arc(_, m, b) => {
                let v = if closes { start.clone() } else { builder::vertex(p3(*b)) };
                wire.push_back(builder::circle_arc(&cur, &v, p3(*m)));
                cur = v;
            }
            Curve2::Poly(p) => {
                for (k, q) in p.iter().enumerate().skip(1) {
                    let v = if closes && k == p.len() - 1 { start.clone() } else { builder::vertex(p3(*q)) };
                    wire.push_back(builder::line(&cur, &v));
                    cur = v;
                }
            }
        }
    }
    Ok(wire)
}

fn tv(v: DVec3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}

fn open_solid(shell: Shell) -> Solid {
    if let Ok(s) = Solid::try_new(vec![shell.clone()]) {
        return s;
    }
    Solid::new_unchecked(vec![shell])
}

/// Surface swept by a curve chain along the frame normal from `z0` to `z1`.
pub fn extrude_curves(frame: &Frame, curves: &[Curve2], z0: f64, z1: f64) -> Result<Solid> {
    if z1 - z0 <= 1e-6 {
        return Err(KernelError::InvalidParameter("distance must not be zero".into()));
    }
    let w = chain_wire(frame, curves)?;
    let w = builder::translated(&w, tv(frame.n * z0));
    let shell: Shell = builder::tsweep(&w, tv(frame.n * (z1 - z0)));
    Ok(open_solid(shell))
}

/// Surface of revolution of a curve chain around a world axis.
pub fn revolve_curves(frame: &Frame, curves: &[Curve2], axis_point: DVec3, axis_dir: DVec3, angle_deg: f64) -> Result<Solid> {
    if angle_deg.abs() < 1e-3 {
        return Err(KernelError::InvalidParameter("angle must not be zero".into()));
    }
    let w = chain_wire(frame, curves)?;
    let shell: Shell = builder::rsweep(&w, Point3::new(axis_point.x, axis_point.y, axis_point.z), tv(axis_dir.normalize()), Rad(angle_deg.to_radians()));
    Ok(open_solid(shell))
}

/// Planar patch filling a closed region.
pub fn patch(frame: &Frame, region: &Region) -> Result<Solid> {
    let face = builder::transformed(&region.face_at_public(0.0)?, to_truck(frame.matrix()));
    Ok(open_solid(vec![face].into()))
}

/// Whether every shell of the body is closed (a solid, not a surface).
pub fn is_closed(solid: &Solid) -> bool {
    solid.boundaries().iter().all(|s| s.shell_condition() == ShellCondition::Closed)
}

/// Join the faces of several bodies into one shell (a solid when closed).
/// Faces are matched by geometry: shared boundary edges must coincide
/// within `tol`.
pub fn stitch(bodies: &[&Solid], tol: f64) -> Result<Solid> {
    use std::collections::HashMap;
    use truck_modeling::{Edge, Face, Vertex};
    let key = |p: Point3| [(p.x / tol).round() as i64, (p.y / tol).round() as i64, (p.z / tol).round() as i64];
    // Rebuild every face on shared vertices and edges.
    let mut verts: HashMap<[i64; 3], Vertex> = HashMap::new();
    // Edges keyed by (start, end, midpoint) grid cells.
    type EdgeKey = ([i64; 3], [i64; 3], [i64; 3]);
    let mut edges: HashMap<EdgeKey, Edge> = HashMap::new();
    let mut faces: Vec<Face> = Vec::new();
    for b in bodies {
        for shell in b.boundaries() {
            for f in shell.face_iter() {
                let mut wires = Vec::new();
                for w in f.boundaries() {
                    let mut nw = Wire::new();
                    for e in w.edge_iter() {
                        let (a, z) = (e.front().point(), e.back().point());
                        let mid = {
                            let c = e.oriented_curve();
                            use truck_modeling::{BoundedCurve, ParametricCurve};
                            let (t0, t1) = c.range_tuple();
                            c.subs((t0 + t1) / 2.0)
                        };
                        let (ka, kz, km) = (key(a), key(z), key(mid));
                        let va = verts.entry(ka).or_insert_with(|| builder::vertex(a)).clone();
                        let vz = verts.entry(kz).or_insert_with(|| builder::vertex(z)).clone();
                        let ne = if let Some(x) = edges.get(&(kz, ka, km)) {
                            x.inverse()
                        } else {
                            edges
                                .entry((ka, kz, km))
                                .or_insert_with(|| Edge::new(&va, &vz, e.oriented_curve()))
                                .clone()
                        };
                        nw.push_back(ne);
                    }
                    wires.push(nw);
                }
                let nf = Face::try_new(wires, f.oriented_surface()).map_err(|e| KernelError::OperationFailed(format!("stitch: {e:?}")))?;
                faces.push(nf);
            }
        }
    }
    let shell: Shell = faces.into();
    let mut out = open_solid(shell);
    if is_closed(&out) && crate::tessellate(&out).volume() < 0.0 {
        out.not();
    }
    Ok(out)
}

/// Every face of a body as a separate surface body.
pub fn unstitch(solid: &Solid) -> Vec<Solid> {
    solid
        .boundaries()
        .iter()
        .flat_map(|s| s.face_iter())
        .map(|f| open_solid(vec![builder::transformed(f, to_truck(glam::DMat4::IDENTITY))].into()))
        .collect()
}

/// Remove faces (indices in `face_iter` order); the result is a surface.
pub fn delete_faces(solid: &Solid, faces: &[usize]) -> Result<Solid> {
    let mut k = 0;
    let mut shells = Vec::new();
    for s in solid.boundaries() {
        let mut ns = Shell::new();
        for f in s.face_iter() {
            if !faces.contains(&k) {
                ns.push(f.clone());
            }
            k += 1;
        }
        if !ns.is_empty() {
            shells.push(ns);
        }
    }
    if shells.is_empty() {
        return Err(KernelError::InvalidParameter("no faces would remain".into()));
    }
    Ok(Solid::new_unchecked(shells))
}

/// Flip the orientation of all faces.
pub fn reverse(solid: &Solid) -> Solid {
    let mut s = solid.clone();
    s.not();
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tessellate;

    fn xy() -> Frame {
        Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y)
    }

    #[test]
    fn surfaces_and_stitching() {
        // An open "U" extruded: 3 faces, open.
        let u = [Curve2::Line([0.0, 10.0], [0.0, 0.0]), Curve2::Line([0.0, 0.0], [10.0, 0.0]), Curve2::Line([10.0, 0.0], [10.0, 10.0])];
        let s = extrude_curves(&xy(), &u, 0.0, 5.0).unwrap();
        assert!(!is_closed(&s));
        let m = tessellate(&s);
        assert!((m.area() - 3.0 * 10.0 * 5.0).abs() < 1e-6, "{}", m.area());
        // Box made of 6 patches, stitched into a closed solid.
        let sq = Region {
            outer: crate::profile::Loop2::Polygon(vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]),
            holes: vec![],
        };
        let b = crate::primitives::make_box(&xy(), 10.0, 10.0, 10.0).unwrap();
        let parts = unstitch(&b);
        assert_eq!(parts.len(), 6);
        let refs: Vec<&Solid> = parts.iter().collect();
        let st = stitch(&refs, 1e-6).unwrap();
        assert!(is_closed(&st));
        assert!((tessellate(&st).volume() - 1000.0).abs() < 1e-6);
        let open = delete_faces(&b, &[0]).unwrap();
        assert!(!is_closed(&open));
        let p = patch(&xy(), &sq).unwrap();
        assert!((tessellate(&p).area() - 100.0).abs() < 1e-9);
        // Revolve a line into a cone surface.
        let r = revolve_curves(&xy(), &[Curve2::Line([0.0, 0.0], [5.0, 10.0])], DVec3::ZERO, DVec3::Y, 360.0).unwrap();
        assert!(tessellate(&r).area() > 0.0);
    }
}
