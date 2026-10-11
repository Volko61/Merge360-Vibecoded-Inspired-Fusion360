//! Sweep (and pipe) of planar regions along paths made of lines and arcs,
//! and loft between profiles.

use glam::DVec3;
use truck_modeling::{builder, Point3, Rad, Shell, Vector3, Wire};

use crate::primitives::to_truck;
use crate::profile::Region;
use crate::{Frame, KernelError, Result, Solid};

/// One segment of a 3D sweep path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathSeg {
    Line { a: DVec3, b: DVec3 },
    /// Circular arc from `a` through `m` to `b`.
    Arc { a: DVec3, m: DVec3, b: DVec3 },
}

impl PathSeg {
    pub fn start(&self) -> DVec3 {
        match self {
            PathSeg::Line { a, .. } | PathSeg::Arc { a, .. } => *a,
        }
    }
    pub fn end(&self) -> DVec3 {
        match self {
            PathSeg::Line { b, .. } | PathSeg::Arc { b, .. } => *b,
        }
    }
    /// Unit tangent at the start.
    pub fn start_tangent(&self) -> DVec3 {
        match *self {
            PathSeg::Line { a, b } => (b - a).normalize(),
            PathSeg::Arc { a, m, b } => {
                let (c, n, _) = arc_geometry(a, m, b).unwrap_or((a, DVec3::Z, 0.0));
                n.cross(a - c).normalize()
            }
        }
    }
}

/// Center, axis (right-handed around the travel direction a → m → b) and
/// sweep angle of a 3-point arc.
pub fn arc_geometry(a: DVec3, m: DVec3, b: DVec3) -> Option<(DVec3, DVec3, f64)> {
    let u = b - a;
    let v = m - a;
    let w = u.cross(v);
    let ww = w.length_squared();
    if ww < 1e-18 {
        return None;
    }
    let center = a + (u.length_squared() * v.cross(w) + v.length_squared() * w.cross(u)) / (2.0 * ww);
    // The triangle a, m, b is traversed in the travel direction.
    let axis = (m - a).cross(b - m).normalize();
    let ang = |p: DVec3, q: DVec3| {
        let (pu, qu) = ((p - center).normalize(), (q - center).normalize());
        axis.dot(pu.cross(qu)).atan2(pu.dot(qu)).rem_euclid(std::f64::consts::TAU)
    };
    Some((center, axis, ang(a, m) + ang(m, b)))
}

fn tp(p: DVec3) -> Point3 {
    Point3::new(p.x, p.y, p.z)
}
fn tv(v: DVec3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}

/// Sweep a wire along one segment; returns the side shell and the end wire.
fn sweep_wire(wire: &Wire, seg: &PathSeg) -> Result<(Shell, Wire)> {
    let shell: Shell = match *seg {
        PathSeg::Line { a, b } => builder::tsweep(wire, tv(b - a)),
        PathSeg::Arc { a, m, b } => {
            let (c, axis, angle) = arc_geometry(a, m, b).ok_or_else(|| KernelError::InvalidParameter("degenerate arc in path".into()))?;
            builder::rsweep(wire, tp(c), tv(axis), Rad(angle))
        }
    };
    // The end wire is the shell boundary that is not the input wire.
    let ids: Vec<_> = wire.edge_iter().map(|e| e.id()).collect();
    let end = shell
        .extract_boundaries()
        .into_iter()
        .find(|w| !w.edge_iter().any(|e| ids.contains(&e.id())))
        .ok_or_else(|| KernelError::OperationFailed("sweep: could not find the end profile".into()))?;
    // Shell boundaries run opposite to the profile's own orientation.
    Ok((shell, end.inverse()))
}

/// Sweep a region (given in `frame`) along a connected path.
pub fn sweep_region(frame: &Frame, region: &Region, path: &[PathSeg]) -> Result<Solid> {
    if path.is_empty() {
        return Err(KernelError::InvalidParameter("empty path".into()));
    }
    for w in path.windows(2) {
        if (w[0].end() - w[1].start()).length() > 1e-6 {
            return Err(KernelError::InvalidParameter("the path is not connected".into()));
        }
    }
    let face = builder::transformed(&region.face_at_public(0.0)?, to_truck(frame.matrix()));
    let start_wires: Vec<Wire> = face.boundaries();
    let mut faces: Vec<truck_modeling::Face> = Vec::new();
    let mut end_wires = Vec::new();
    for w in &start_wires {
        let mut cur = w.clone();
        for seg in path {
            let (shell, next) = sweep_wire(&cur, seg)?;
            faces.extend(shell.face_iter().cloned());
            cur = next;
        }
        end_wires.push(cur);
    }
    let mut shell: Shell = faces.into();
    shell.push(face.inverse());
    let end_cap = builder::try_attach_plane(&end_wires).map_err(|e| KernelError::OperationFailed(format!("sweep end cap: {e:?}")))?;
    shell.push(end_cap);
    let mut solid = Solid::try_new(vec![shell]).map_err(|e| KernelError::OperationFailed(format!("sweep: {e:?}")))?;
    if crate::tessellate(&solid).volume() < 0.0 {
        solid.not();
    }
    Ok(solid)
}

/// Loft between two or more profile wires (each given as a region in its
/// frame). Profiles must have the same number of boundary edges.
pub fn loft(sections: &[(Frame, Region)]) -> Result<Solid> {
    if sections.len() < 2 {
        return Err(KernelError::InvalidParameter("select at least two profiles".into()));
    }
    let faces: Vec<truck_modeling::Face> = sections
        .iter()
        .map(|(f, r)| Ok(builder::transformed(&r.face_at_public(0.0)?, to_truck(f.matrix()))))
        .collect::<Result<_>>()?;
    let wires: Vec<Wire> = faces.iter().map(|f| f.boundaries()[0].clone()).collect();
    for (i, f) in faces.iter().enumerate() {
        if f.boundaries().len() != 1 {
            return Err(KernelError::InvalidParameter(format!("profile {} has holes (not supported by loft)", i + 1)));
        }
        if wires[i].len() != wires[0].len() {
            return Err(KernelError::InvalidParameter(
                "profiles must have the same number of edges (e.g. circle to circle, rectangle to rectangle)".into(),
            ));
        }
    }
    let mut shell = Shell::new();
    for w in wires.windows(2) {
        let side = builder::try_wire_homotopy(&w[0], &w[1]).map_err(|e| KernelError::OperationFailed(format!("loft: {e:?}")))?;
        shell.extend(side);
    }
    shell.push(faces[0].inverse());
    shell.push(faces[faces.len() - 1].clone());
    let mut solid = Solid::try_new(vec![shell]).map_err(|e| KernelError::OperationFailed(format!("loft: {e:?}")))?;
    if crate::tessellate(&solid).volume() < 0.0 {
        solid.not();
    }
    Ok(solid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Loop2;
    use crate::tessellate;
    use std::f64::consts::PI;

    fn square(s: f64) -> Region {
        Region { outer: Loop2::Polygon(vec![[-s, -s], [s, -s], [s, s], [-s, s]]), holes: vec![] }
    }

    #[test]
    fn sweep_line_arc_line() {
        // Profile in the YZ plane at the origin, path along +X then a quarter
        // arc turning to +Y, then a straight line.
        let frame = Frame::new(DVec3::ZERO, DVec3::Y, DVec3::Z);
        let r = 20.0;
        let k = std::f64::consts::FRAC_1_SQRT_2;
        let path = [
            PathSeg::Line { a: DVec3::ZERO, b: DVec3::new(30.0, 0.0, 0.0) },
            PathSeg::Arc {
                a: DVec3::new(30.0, 0.0, 0.0),
                m: DVec3::new(30.0 + r * k, r - r * k, 0.0),
                b: DVec3::new(30.0 + r, r, 0.0),
            },
            PathSeg::Line { a: DVec3::new(50.0, 20.0, 0.0), b: DVec3::new(50.0, 40.0, 0.0) },
        ];
        let s = sweep_region(&frame, &square(2.0), &path).unwrap();
        let v = tessellate(&s).volume();
        let area = 16.0;
        let len = 30.0 + PI * r / 2.0 + 20.0;
        assert!((v - area * len).abs() / (area * len) < 0.01, "{v} vs {}", area * len);
    }

    #[test]
    fn pipe_with_hole() {
        let frame = Frame::new(DVec3::ZERO, DVec3::Y, DVec3::Z);
        let tube = Region { outer: Loop2::Circle { c: [0.0, 0.0], r: 3.0 }, holes: vec![Loop2::Circle { c: [0.0, 0.0], r: 2.0 }] };
        let s = sweep_region(&frame, &tube, &[PathSeg::Line { a: DVec3::ZERO, b: DVec3::new(10.0, 0.0, 0.0) }]).unwrap();
        let v = tessellate(&s).volume();
        assert!((v - PI * 5.0 * 10.0).abs() < 1.0, "{v}");
    }

    #[test]
    fn loft_square_to_square() {
        let a = (Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y), square(10.0));
        let b = (Frame::new(DVec3::new(0.0, 0.0, 20.0), DVec3::X, DVec3::Y), square(5.0));
        let s = loft(&[a, b]).unwrap();
        let v = tessellate(&s).volume();
        // Frustum of a square pyramid: h/3 (A1 + A2 + sqrt(A1 A2))
        let expect = 20.0 / 3.0 * (400.0 + 100.0 + 200.0);
        assert!((v - expect).abs() / expect < 0.01, "{v} vs {expect}");
    }
}
