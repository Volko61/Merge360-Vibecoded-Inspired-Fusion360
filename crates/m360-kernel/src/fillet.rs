//! Fillet and chamfer of straight edges between planar faces.
//!
//! truck has no blending operations, so edges are treated with booleans: a
//! prismatic tool whose cross-section is the region between the edge corner
//! and the fillet arc (or chamfer line) is cut from convex edges and joined to
//! concave ones. The tool overshoots away from the kept material so that the
//! boolean never sees coplanar faces.

use glam::DVec3;

use crate::ops::{boolean_with_tol, BooleanOp};
use crate::profile::{extrude_range, Loop2, Region, Seg2};
use crate::{Frame, KernelError, Result, Solid};

/// Geometry of a straight edge and its two planar faces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeSpec {
    pub a: DVec3,
    pub b: DVec3,
    /// Outward normals of the two faces.
    pub normals: [DVec3; 2],
    /// Directions from the edge into each face (perpendicular to the edge).
    pub dirs: [DVec3; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Treatment {
    Fillet { radius: f64 },
    Chamfer { distance: f64 },
}

impl EdgeSpec {
    pub fn convex(&self) -> bool {
        self.dirs[1].dot(self.normals[0]) < 0.0
    }
}

/// Reject treatments that reach past the body: the setback along each face must
/// stay within the body's extent in that direction.
fn check_size(solid: &Solid, edges: &[EdgeSpec], t: Treatment) -> Result<()> {
    let mesh = crate::tessellate(solid);
    for e in edges {
        let cos = e.dirs[0].dot(e.dirs[1]).clamp(-1.0, 1.0);
        let setback = match t {
            Treatment::Fillet { radius } => radius / (cos.acos() / 2.0).tan(),
            Treatment::Chamfer { distance } => distance,
        };
        for d in e.dirs {
            let extent = mesh.positions.iter().map(|p| (DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64) - e.a).dot(d)).fold(0.0, f64::max);
            if setback.is_finite() && setback > extent + 1e-6 {
                let what = if matches!(t, Treatment::Fillet { .. }) { "radius" } else { "distance" };
                return Err(KernelError::InvalidParameter(format!("the {what} is too large for the adjacent faces")));
            }
        }
    }
    Ok(())
}

/// Build the tool solid for one edge. Returns (tool, operation).
pub fn edge_tool(e: &EdgeSpec, t: Treatment) -> Result<(Solid, BooleanOp)> {
    edge_tool_variant(e, t, 0, &VARIANTS[0])
}

/// Robustness strategy for one attempt.
#[derive(Debug, Clone, Copy)]
struct Variant {
    /// How much the fillet circle is flattened (fraction of the tool size).
    flat: f64,
    /// Approximate the fillet arc with flat facets.
    faceted: bool,
    /// Boolean tolerance.
    tol: f64,
}

const VARIANTS: [Variant; 3] = [
    Variant { flat: 0.004, faceted: false, tol: 0.05 },
    Variant { flat: 0.05, faceted: false, tol: 0.01 },
    Variant { flat: 0.004, faceted: true, tol: 0.05 },
];

/// Number of facets of an approximated fillet arc.
pub const FILLET_FACETS: usize = 12;

/// `index` varies the overshoot so tools of different edges never share
/// coplanar faces (which would break their union).
fn edge_tool_variant(e: &EdgeSpec, t: Treatment, index: usize, var: &Variant) -> Result<(Solid, BooleanOp)> {
    let jitter = 1.0 + ((index as f64 * 0.618_034) % 1.0) * 0.79;
    let len = (e.b - e.a).length();
    if len < 1e-6 {
        return Err(KernelError::InvalidParameter("degenerate edge".into()));
    }
    let dir = (e.b - e.a) / len;
    let [d1, d2] = e.dirs;
    let [n1, n2] = e.normals;
    let cos = d1.dot(d2).clamp(-1.0, 1.0);
    let theta = cos.acos();
    if !(0.05..=std::f64::consts::PI - 0.05).contains(&theta) {
        return Err(KernelError::InvalidParameter("faces are (nearly) tangent at this edge".into()));
    }
    let convex = e.convex();
    let s = if convex { 1.0 } else { -1.0 };

    let c = DVec3::ZERO; // corner, in edge-local 3D coordinates (origin at a)
    let (t1, t2, mid, size) = match t {
        Treatment::Fillet { radius } => {
            if radius <= 1e-6 {
                return Err(KernelError::InvalidParameter("radius must be greater than zero".into()));
            }
            let tl = radius / (theta / 2.0).tan();
            let bis = (d1 + d2).normalize();
            let center = c + bis * radius / (theta / 2.0).sin();
            (c + d1 * tl, c + d2 * tl, Some(center - bis * radius), tl.max(radius))
        }
        Treatment::Chamfer { distance } => {
            if distance <= 1e-6 {
                return Err(KernelError::InvalidParameter("distance must be greater than zero".into()));
            }
            (c + d1 * distance, c + d2 * distance, None, distance)
        }
    };
    let over = (size * 0.5 + 0.2) * jitter;

    // Local 2D frame perpendicular to the edge.
    let ext = (len * 0.02 + 0.5) * jitter;
    let origin = e.a - dir * ext;
    let frame = Frame::new(origin, d1, dir.cross(d1).normalize());
    let to2 = |p: DVec3| {
        let w = e.a + p - origin;
        [w.dot(frame.u), w.dot(frame.v)]
    };
    let from2 = |q: [f64; 2]| frame.u * q[0] + frame.v * q[1] + origin - e.a;

    // The treated curve must cross both faces transversally (truck cannot
    // intersect tangent or coincident geometry), so it is extended slightly
    // past the faces, and for fillets the circle is flattened a tiny bit.
    let k = (size * 0.06).max(0.25) * jitter;
    let (c1, cm, c2) = match mid {
        Some(m) => {
            let bis = (d1 + d2).normalize();
            let m2 = to2(m - bis * size * var.flat * jitter);
            let (ctr, r) = crate::profile::circumcircle(to2(t1), m2, to2(t2))
                .ok_or_else(|| KernelError::OperationFailed("fillet arc".into()))?;
            let ang = |q: [f64; 2]| (q[1] - ctr[1]).atan2(q[0] - ctr[0]);
            let rot = |q: [f64; 2], away_from: [f64; 2]| {
                // Rotate q along the circle by a small angle, away from `away_from`.
                let a = ang(q);
                let da = (k / r).min(0.2);
                let cand = |s: f64| [ctr[0] + r * (a + s * da).cos(), ctr[1] + r * (a + s * da).sin()];
                let dist = |p: [f64; 2]| (p[0] - away_from[0]).hypot(p[1] - away_from[1]);
                if dist(cand(1.0)) > dist(cand(-1.0)) { cand(1.0) } else { cand(-1.0) }
            };
            (rot(to2(t1), m2), Some(m2), rot(to2(t2), m2))
        }
        None => {
            let u = (t1 - t2).normalize() * k;
            (to2(t1 + u), None, to2(t2 - u))
        }
    };
    let (c1w, c2w) = (from2(c1), from2(c2));
    let p2 = c2w + n2 * over * s;
    let pc = c + (n1 + n2) * over * s;
    let p1 = c1w + n1 * over * s;
    let mut segs = match cm {
        Some(m) if var.faceted => crate::profile::arc_points(c1, m, c2, FILLET_FACETS).into_iter().map(Seg2::line).collect(),
        Some(m) => vec![Seg2::arc(m, c2)],
        None => vec![Seg2::line(c2)],
    };
    segs.extend([Seg2::line(to2(p2)), Seg2::line(to2(pc)), Seg2::line(to2(p1)), Seg2::line(c1)]);
    let path = Loop2::Path(segs);
    let region = Region { outer: path, holes: vec![] };
    // `frame.n` may point along -dir depending on handedness; extrude both ways as needed.
    let along = frame.n.dot(dir);
    let (z0, z1) = if along > 0.0 { (0.0, len + 2.0 * ext) } else { (-(len + 2.0 * ext), 0.0) };
    let tool = extrude_range(&frame, &region, z0, z1)?;
    Ok((tool, if convex { BooleanOp::Cut } else { BooleanOp::Join }))
}

/// Fillet or chamfer several edges of a solid.
///
/// Cutters of convex edges are first united into one tool and removed in a
/// single boolean (this behaves much better at corners where several treated
/// edges meet); concave fillers are joined afterwards. If a combined boolean
/// fails, edges are processed one by one as a fallback.
pub fn treat_edges(solid: &Solid, edges: &[EdgeSpec], t: Treatment) -> Result<Solid> {
    treat_edges_report(solid, edges, t).map(|r| r.0)
}

/// Like [`treat_edges`]; also reports whether the fillet had to be
/// approximated with facets (the caller may then hide the facet edges).
pub fn treat_edges_report(solid: &Solid, edges: &[EdgeSpec], t: Treatment) -> Result<(Solid, bool)> {
    if edges.is_empty() {
        return Err(KernelError::InvalidParameter("no edges selected".into()));
    }
    check_size(solid, edges, t)?;
    // truck's booleans are sensitive to near-degenerate configurations where
    // tools meet; try progressively more robust strategies and perturbations.
    let variants: &[Variant] = if matches!(t, Treatment::Fillet { .. }) { &VARIANTS } else { &VARIANTS[..1] };
    let mut last_err = None;
    for var in variants {
        for seed in 0..3 {
            let attempt = crate::quiet_catch(|| {
                let r = treat_edges_once(solid, edges, t, seed, var)?;
                // Validate: the result must tessellate into a closed, positive volume.
                let v = crate::tessellate(&r).volume();
                if v.is_finite() && v > 1e-6 {
                    Ok(r)
                } else {
                    Err(KernelError::OperationFailed("invalid result".into()))
                }
            })
            .unwrap_or_else(|| Err(KernelError::OperationFailed("kernel panic".into())));
            match attempt {
                Ok(s) => return Ok((s, var.faceted)),
                Err(e @ KernelError::InvalidParameter(_)) => return Err(e),
                Err(e) => last_err = Some(e),
            }
        }
    }
    let e = last_err.unwrap();
    Err(KernelError::OperationFailed(format!(
        "could not compute the fillet/chamfer ({e}). Try fewer edges at once or a smaller size."
    )))
}

fn treat_edges_once(solid: &Solid, edges: &[EdgeSpec], t: Treatment, seed: usize, var: &Variant) -> Result<Solid> {
    let mut cuts = Vec::new();
    let mut joins = Vec::new();
    let n = edges.len();
    for (i, e) in edges.iter().enumerate() {
        // Rotate the perturbation assignment for each seed.
        let (tool, op) = edge_tool_variant(e, t, (i + seed * 3) % (n + 7), var)?;
        if op == BooleanOp::Cut { cuts.push(tool) } else { joins.push(tool) }
    }
    let tol = var.tol;
    let combined = || -> Result<Solid> {
        let mut acc = solid.clone();
        if !cuts.is_empty() {
            let tool = boolean_with_tol(&cuts[0], &cuts[1..], BooleanOp::Join, tol)?;
            acc = boolean_with_tol(&acc, &[tool], BooleanOp::Cut, tol)?;
        }
        if !joins.is_empty() {
            let tool = boolean_with_tol(&joins[0], &joins[1..], BooleanOp::Join, tol)?;
            acc = boolean_with_tol(&acc, &[tool], BooleanOp::Join, tol)?;
        }
        Ok(acc)
    };
    combined().or_else(|first_err| {
        let mut acc = solid.clone();
        for tool in &cuts {
            acc = boolean_with_tol(&acc, std::slice::from_ref(tool), BooleanOp::Cut, tol).map_err(|_| first_err.clone())?;
        }
        for tool in &joins {
            acc = boolean_with_tol(&acc, std::slice::from_ref(tool), BooleanOp::Join, tol).map_err(|_| first_err.clone())?;
        }
        Ok(acc)
    })
}

/// Edge specs of a tessellated body, for every straight edge between planar faces.
pub fn edge_specs(mesh: &crate::BodyMesh) -> Vec<Option<EdgeSpec>> {
    mesh.edges
        .iter()
        .zip(&mesh.edge_info)
        .map(|(pts, info)| {
            let (n, d) = (info.normals?, info.dirs?);
            Some(EdgeSpec {
                a: glam::Vec3::from(pts[0]).as_dvec3(),
                b: glam::Vec3::from(pts[pts.len() - 1]).as_dvec3(),
                normals: [n[0].as_dvec3(), n[1].as_dvec3()],
                dirs: [d[0].as_dvec3(), d[1].as_dvec3()],
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{primitives::make_box, tessellate};

    fn cube() -> Solid {
        make_box(&Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y), 20.0, 20.0, 20.0).unwrap()
    }

    fn vertical_edges(s: &Solid) -> Vec<EdgeSpec> {
        let m = tessellate(s);
        edge_specs(&m)
            .into_iter()
            .flatten()
            .filter(|e| ((e.b - e.a).normalize().z).abs() > 0.99)
            .collect()
    }

    #[test]
    fn edge_info_of_cube() {
        let m = tessellate(&cube());
        let specs: Vec<_> = edge_specs(&m).into_iter().flatten().collect();
        assert_eq!(specs.len(), 12);
        assert!(specs.iter().all(|e| e.convex()));
    }

    #[test]
    fn fillet_one_edge() {
        let c = cube();
        let e = vertical_edges(&c)[0];
        let r = 4.0;
        let s = treat_edges(&c, &[e], Treatment::Fillet { radius: r }).unwrap();
        let v = tessellate(&s).volume();
        let expect = 8000.0 - (1.0 - std::f64::consts::PI / 4.0) * r * r * 20.0;
        assert!((v - expect).abs() < 3.0, "{v} vs {expect}");
    }

    #[test]
    fn chamfer_four_edges() {
        let c = cube();
        let es = vertical_edges(&c);
        assert_eq!(es.len(), 4);
        let s = treat_edges(&c, &es, Treatment::Chamfer { distance: 3.0 }).unwrap();
        let v = tessellate(&s).volume();
        let expect = 8000.0 - 4.0 * 4.5 * 20.0;
        assert!((v - expect).abs() < 3.0, "{v} vs {expect}");
    }

    #[test]
    fn fillet_and_chamfer_face_loop() {
        let c = cube();
        let top: Vec<_> = edge_specs(&tessellate(&c))
            .into_iter()
            .flatten()
            .filter(|e| e.a.z > 19.9 && e.b.z > 19.9)
            .collect();
        assert_eq!(top.len(), 4);
        for t in [Treatment::Fillet { radius: 2.0 }, Treatment::Chamfer { distance: 2.0 }] {
            let s = treat_edges(&c, &top, t);
            assert!(s.is_ok(), "{t:?}: {:?}", s.err());
            let v = tessellate(&s.unwrap()).volume();
            assert!(v < 8000.0 && v > 7800.0, "{v}");
        }
    }

    #[test]
    fn fillet_all_twelve() {
        let c = cube();
        let all: Vec<_> = edge_specs(&tessellate(&c)).into_iter().flatten().collect();
        // Chamfers meeting three at a corner work; such fillet corners
        // (vertex blends) are beyond the boolean approach and must fail cleanly.
        let r = treat_edges(&c, &all, Treatment::Chamfer { distance: 2.0 });
        let v = tessellate(&r.unwrap()).volume();
        assert!(v > 7300.0 && v < 8000.0, "{v}");
        if let Err(e) = treat_edges(&c, &all, Treatment::Fillet { radius: 2.0 }) {
            assert!(matches!(e, KernelError::OperationFailed(_)));
        }
    }

    #[test]
    fn fillet_four_edges() {
        let c = cube();
        let es = vertical_edges(&c);
        let s = treat_edges(&c, &es, Treatment::Fillet { radius: 3.0 }).unwrap();
        let v = tessellate(&s).volume();
        let expect = 8000.0 - 4.0 * (1.0 - std::f64::consts::PI / 4.0) * 9.0 * 20.0;
        assert!((v - expect).abs() < 4.0, "{v} vs {expect}");
    }

    #[test]
    fn oversized_fillet_is_rejected() {
        let cube = crate::primitives::make_box(&crate::Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y), 20.0, 20.0, 20.0).unwrap();
        let spec = edge_specs(&crate::tessellate(&cube)).into_iter().flatten().next().unwrap();
        assert!(matches!(treat_edges(&cube, &[spec], Treatment::Fillet { radius: 50.0 }), Err(KernelError::InvalidParameter(_))));
        assert!(matches!(treat_edges(&cube, &[spec], Treatment::Chamfer { distance: 25.0 }), Err(KernelError::InvalidParameter(_))));
    }
}
