//! Planar profiles (closed 2D loops) and their extrusion.

use std::f64::consts::PI;

use truck_modeling::{builder, Point3, Rad, Vector3, Wire};
use truck_meshalgo::prelude::*;

use crate::primitives::to_truck;
use crate::{Frame, KernelError, Result, Solid};

/// A closed loop in plane coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum Loop2 {
    Polygon(Vec<[f64; 2]>),
    Circle { c: [f64; 2], r: f64 },
}

impl Loop2 {
    /// Signed area (positive = counter-clockwise).
    pub fn signed_area(&self) -> f64 {
        match self {
            Loop2::Circle { r, .. } => PI * r * r,
            Loop2::Polygon(p) => {
                let n = p.len();
                (0..n)
                    .map(|i| {
                        let a = p[i];
                        let b = p[(i + 1) % n];
                        a[0] * b[1] - b[0] * a[1]
                    })
                    .sum::<f64>()
                    / 2.0
            }
        }
    }

    pub fn area(&self) -> f64 {
        self.signed_area().abs()
    }

    /// Polyline approximation (closed, last point != first).
    pub fn points(&self, segments: usize) -> Vec<[f64; 2]> {
        match self {
            Loop2::Polygon(p) => p.clone(),
            Loop2::Circle { c, r } => (0..segments)
                .map(|i| {
                    let a = i as f64 / segments as f64 * 2.0 * PI;
                    [c[0] + r * a.cos(), c[1] + r * a.sin()]
                })
                .collect(),
        }
    }

    pub fn contains(&self, q: [f64; 2]) -> bool {
        match self {
            Loop2::Circle { c, r } => (q[0] - c[0]).hypot(q[1] - c[1]) < *r,
            Loop2::Polygon(p) => {
                let mut inside = false;
                let n = p.len();
                let mut j = n - 1;
                for i in 0..n {
                    let (a, b) = (p[i], p[j]);
                    if (a[1] > q[1]) != (b[1] > q[1])
                        && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0]
                    {
                        inside = !inside;
                    }
                    j = i;
                }
                inside
            }
        }
    }

    /// A point guaranteed to lie on the loop (used for containment tests).
    pub fn sample(&self) -> [f64; 2] {
        match self {
            Loop2::Polygon(p) => p[0],
            Loop2::Circle { c, r } => [c[0] + r, c[1]],
        }
    }

    fn wire(&self, z: f64, ccw: bool) -> Result<Wire> {
        let mut w = match self {
            Loop2::Circle { c, r } => {
                let v = builder::vertex(Point3::new(c[0] + r, c[1], z));
                builder::rsweep(&v, Point3::new(c[0], c[1], z), Vector3::unit_z(), Rad(2.0 * PI))
            }
            Loop2::Polygon(p) => {
                if p.len() < 3 {
                    return Err(KernelError::InvalidParameter("profile needs at least 3 points".into()));
                }
                let mut pts = p.clone();
                if self.signed_area() < 0.0 {
                    pts.reverse();
                }
                let vs: Vec<_> = pts.iter().map(|q| builder::vertex(Point3::new(q[0], q[1], z))).collect();
                (0..vs.len()).map(|i| builder::line(&vs[i], &vs[(i + 1) % vs.len()])).collect()
            }
        };
        if !ccw {
            w.invert();
        }
        Ok(w)
    }
}

/// A region: an outer loop minus holes.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub outer: Loop2,
    pub holes: Vec<Loop2>,
}

impl Region {
    pub fn contains(&self, q: [f64; 2]) -> bool {
        self.outer.contains(q) && !self.holes.iter().any(|h| h.contains(q))
    }

    pub fn area(&self) -> f64 {
        self.outer.area() - self.holes.iter().map(|h| h.area()).sum::<f64>()
    }

    fn face_at(&self, z: f64) -> Result<truck_modeling::Face> {
        let mut wires = vec![self.outer.wire(z, true)?];
        for h in &self.holes {
            wires.push(h.wire(z, false)?);
        }
        builder::try_attach_plane(&wires)
            .map_err(|e| KernelError::OperationFailed(format!("profile is not planar/closed: {e:?}")))
    }
}

/// Extrude a region along the frame normal. `distance` may be negative.
/// With `symmetric`, the region is extruded `distance` to both sides.
pub fn extrude(frame: &Frame, region: &Region, distance: f64, symmetric: bool) -> Result<Solid> {
    if !distance.is_finite() || distance.abs() < 1e-6 {
        return Err(KernelError::InvalidParameter("distance must not be zero".into()));
    }
    let (z0, z1) = extent(distance, symmetric);
    extrude_range(frame, region, z0, z1)
}

/// Normal-direction range `[z0, z1]` covered by an extrusion.
pub fn extent(distance: f64, symmetric: bool) -> (f64, f64) {
    if symmetric {
        (-distance.abs(), distance.abs())
    } else {
        (distance.min(0.0), distance.max(0.0))
    }
}

/// Extrude a region between two offsets along the frame normal.
pub fn extrude_range(frame: &Frame, region: &Region, z0: f64, z1: f64) -> Result<Solid> {
    if z1 - z0 <= 1e-6 || z1.is_nan() || z0.is_nan() {
        return Err(KernelError::InvalidParameter("distance must not be zero".into()));
    }
    let face = region.face_at(z0)?;
    let solid = builder::tsweep(&face, Vector3::new(0.0, 0.0, z1 - z0));
    Ok(builder::transformed(&solid, to_truck(frame.matrix())))
}

/// Triangulate a region (world space) for profile shading.
pub fn region_triangles(frame: &Frame, region: &Region) -> Vec<[f32; 3]> {
    let Ok(face) = region.face_at(0.0) else { return Vec::new() };
    let face = builder::transformed(&face, to_truck(frame.matrix()));
    let shell: truck_modeling::Shell = vec![face].into();
    let meshed = shell.triangulation(0.05);
    let mut out = Vec::new();
    for f in meshed.face_iter() {
        if let Some(poly) = f.surface() {
            let pos = poly.positions();
            for tri in poly.faces().triangle_iter() {
                for v in tri {
                    let p = pos[v.pos];
                    out.push([p.x as f32, p.y as f32, p.z as f32]);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tessellate;
    use glam::DVec3;

    fn xy() -> Frame {
        Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y)
    }

    #[test]
    fn extrude_rect_cw_and_ccw() {
        for pts in [
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0]],
            vec![[0.0, 0.0], [0.0, 5.0], [10.0, 5.0], [10.0, 0.0]],
        ] {
            let r = Region { outer: Loop2::Polygon(pts), holes: vec![] };
            let s = extrude(&xy(), &r, 2.0, false).unwrap();
            let v = tessellate(&s).volume();
            assert!((v - 100.0).abs() < 0.5, "{v}");
        }
    }

    #[test]
    fn extrude_with_hole_and_negative() {
        let r = Region {
            outer: Loop2::Polygon(vec![[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]]),
            holes: vec![Loop2::Circle { c: [0.0, 0.0], r: 5.0 }],
        };
        let s = extrude(&xy(), &r, -4.0, false).unwrap();
        let m = tessellate(&s);
        let expect = (400.0 - PI * 25.0) * 4.0;
        assert!((m.volume() - expect).abs() / expect < 0.01, "{}", m.volume());
        assert!(m.bbox_max.z < 1e-4);
    }

    #[test]
    fn symmetric_circle() {
        let r = Region { outer: Loop2::Circle { c: [3.0, 3.0], r: 2.0 }, holes: vec![] };
        let s = extrude(&xy(), &r, 5.0, true).unwrap();
        let v = tessellate(&s).volume();
        let expect = PI * 4.0 * 10.0;
        assert!((v - expect).abs() / expect < 0.01, "{v}");
    }

    #[test]
    fn contains() {
        let r = Region {
            outer: Loop2::Polygon(vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]),
            holes: vec![Loop2::Circle { c: [5.0, 5.0], r: 2.0 }],
        };
        assert!(r.contains([1.0, 1.0]));
        assert!(!r.contains([5.0, 5.0]));
        assert!(!r.contains([11.0, 5.0]));
        assert!(!region_triangles(&xy(), &r).is_empty());
    }
}
