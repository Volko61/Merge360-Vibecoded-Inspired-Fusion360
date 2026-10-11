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
    /// Closed path of lines and arcs. Each segment ends at `end`; the start is
    /// the previous segment's end (wrapping around).
    Path(Vec<Seg2>),
}

/// One segment of a [`Loop2::Path`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seg2 {
    pub end: [f64; 2],
    /// For arcs: a point on the arc strictly between its start and end.
    pub transit: Option<[f64; 2]>,
}

impl Seg2 {
    pub fn line(end: [f64; 2]) -> Self {
        Self { end, transit: None }
    }
    pub fn arc(transit: [f64; 2], end: [f64; 2]) -> Self {
        Self { end, transit: Some(transit) }
    }
}

/// Points along the circular arc through a, m, b (excluding a, including b).
pub fn arc_points(a: [f64; 2], m: [f64; 2], b: [f64; 2], n: usize) -> Vec<[f64; 2]> {
    let Some((c, r)) = circumcircle(a, m, b) else { return vec![m, b] };
    let ang = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]);
    let (a0, am, a1) = (ang(a), ang(m), ang(b));
    let tau = 2.0 * PI;
    // Sweep from a0 to a1 in the direction that passes am.
    let ccw = |x: f64| (x - a0).rem_euclid(tau);
    let sweep = if ccw(am) < ccw(a1) { ccw(a1) } else { ccw(a1) - tau };
    (1..=n).map(|i| {
        let t = a0 + sweep * i as f64 / n as f64;
        [c[0] + r * t.cos(), c[1] + r * t.sin()]
    }).collect()
}

/// Center and radius of the circle through three points.
pub fn circumcircle(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<([f64; 2], f64)> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return None;
    }
    let sq = |p: [f64; 2]| p[0] * p[0] + p[1] * p[1];
    let ux = (sq(a) * (b[1] - c[1]) + sq(b) * (c[1] - a[1]) + sq(c) * (a[1] - b[1])) / d;
    let uy = (sq(a) * (c[0] - b[0]) + sq(b) * (a[0] - c[0]) + sq(c) * (b[0] - a[0])) / d;
    Some(([ux, uy], (a[0] - ux).hypot(a[1] - uy)))
}

impl Loop2 {
    /// Signed area (positive = counter-clockwise).
    pub fn signed_area(&self) -> f64 {
        match self {
            Loop2::Circle { r, .. } => PI * r * r,
            Loop2::Path(_) => Loop2::Polygon(self.points(64)).signed_area(),
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
            Loop2::Path(segs) => {
                let mut out = Vec::new();
                let mut prev = segs.last().map(|s| s.end).unwrap_or_default();
                for s in segs {
                    match s.transit {
                        None => out.push(s.end),
                        Some(m) => out.extend(arc_points(prev, m, s.end, (segments / 4).max(4))),
                    }
                    prev = s.end;
                }
                out
            }
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
            Loop2::Path(_) => Loop2::Polygon(self.points(64)).contains(q),
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
            Loop2::Path(s) => s[0].end,
            Loop2::Circle { c, r } => [c[0] + r, c[1]],
        }
    }

    /// Offset the loop by `t` (positive = grow, negative = shrink). Corners
    /// are mitered; arcs keep their centers. Returns `None` when the loop
    /// collapses.
    pub fn offset(&self, t: f64) -> Option<Loop2> {
        if t == 0.0 {
            return Some(self.clone());
        }
        match self {
            Loop2::Circle { c, r } => (r + t > 1e-9).then_some(Loop2::Circle { c: *c, r: r + t }),
            Loop2::Polygon(p) => crate::shell::offset_polygon(p, t).map(Loop2::Polygon),
            Loop2::Path(segs) => {
                let n = segs.len();
                let s = self.signed_area().signum();
                let start = |i: usize| segs[(i + n - 1) % n].end;
                // Unit tangent of segment i at point p (in travel direction).
                let tangent = |i: usize, p: [f64; 2]| -> [f64; 2] {
                    let (a, b) = (start(i), segs[i].end);
                    let d = match segs[i].transit.and_then(|m| circumcircle(a, m, b).map(|c| (m, c))) {
                        Some((m, (c, _))) => {
                            let ccw = (m[0] - a[0]) * (b[1] - m[1]) - (m[1] - a[1]) * (b[0] - m[0]) > 0.0;
                            let r = [p[0] - c[0], p[1] - c[1]];
                            if ccw { [-r[1], r[0]] } else { [r[1], -r[0]] }
                        }
                        None => [b[0] - a[0], b[1] - a[1]],
                    };
                    let l = d[0].hypot(d[1]).max(1e-12);
                    [d[0] / l, d[1] / l]
                };
                let normal = |d: [f64; 2]| [s * d[1], -s * d[0]];
                let mut out = Vec::with_capacity(n);
                for (i, seg) in segs.iter().enumerate() {
                    let v = seg.end;
                    let (n1, n2) = (normal(tangent(i, v)), normal(tangent((i + 1) % n, v)));
                    let bis = [n1[0] + n2[0], n1[1] + n2[1]];
                    let bl = bis[0].hypot(bis[1]);
                    if bl < 1e-9 {
                        return None;
                    }
                    let k = t / (bl / 2.0).max(0.2);
                    let end = [v[0] + bis[0] / bl * k, v[1] + bis[1] / bl * k];
                    let transit = seg.transit.map(|m| {
                        let nm = normal(tangent(i, m));
                        [m[0] + nm[0] * t, m[1] + nm[1] * t]
                    });
                    out.push(Seg2 { end, transit });
                }
                // Straight segments must keep their direction.
                let flipped = (0..n).any(|i| {
                    let (a0, b0) = (start(i), segs[i].end);
                    let (a1, b1) = (out[(i + n - 1) % n].end, out[i].end);
                    (b0[0] - a0[0]) * (b1[0] - a1[0]) + (b0[1] - a0[1]) * (b1[1] - a1[1]) <= 0.0
                });
                let l = Loop2::Path(out);
                let na = l.signed_area();
                (!flipped && na.signum() == s && na.abs() > 1e-9).then_some(l)
            }
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
            Loop2::Path(segs) => {
                if segs.len() < 2 {
                    return Err(KernelError::InvalidParameter("profile path needs at least 2 segments".into()));
                }
                // (start, end, transit) triples, reversed if clockwise.
                let n = segs.len();
                let mut tri: Vec<PathTriple> =
                    (0..n).map(|i| (segs[(i + n - 1) % n].end, segs[i].end, segs[i].transit)).collect();
                if self.signed_area() < 0.0 {
                    tri = tri.into_iter().rev().map(|(a, b, m)| (b, a, m)).collect();
                }
                let vs: Vec<_> = tri.iter().map(|t| builder::vertex(Point3::new(t.0[0], t.0[1], z))).collect();
                (0..n)
                    .map(|i| {
                        let (v0, v1) = (&vs[i], &vs[(i + 1) % n]);
                        match tri[i].2 {
                            None => builder::line(v0, v1),
                            Some(m) => builder::circle_arc(v0, v1, Point3::new(m[0], m[1], z)),
                        }
                    })
                    .collect()
            }
        };
        if !ccw {
            w.invert();
        }
        Ok(w)
    }
}

/// (start, end, arc transit) of a path segment.
type PathTriple = ([f64; 2], [f64; 2], Option<[f64; 2]>);

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

    /// Planar face of the region at height `z` (outer loop CCW, holes CW).
    pub fn face_at_public(&self, z: f64) -> Result<truck_modeling::Face> {
        self.face_at(z)
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

/// Solid lofted through the region at several heights, each loop offset by
/// `offset(z)` (holes the opposite way), in frame-local coordinates.
fn loft_region(region: &Region, heights: &[f64], offset: impl Fn(f64) -> f64) -> Result<Solid> {
    let too_large = || KernelError::InvalidParameter("the taper angle is too large for this profile".into());
    let sections = heights
        .iter()
        .map(|&z| {
            let o = offset(z);
            let outer = region.outer.offset(o).ok_or_else(too_large)?;
            let holes = region.holes.iter().map(|h| h.offset(-o).ok_or_else(too_large)).collect::<Result<_>>()?;
            Ok((z, Region { outer, holes }))
        })
        .collect::<Result<Vec<_>>>()?;
    loft_sections(&sections)
}

/// Solid through planar sections at heights `z` (frame-local), whose loops
/// correspond one to one (same loop count and edges per loop).
pub fn loft_sections(sections: &[(f64, Region)]) -> Result<Solid> {
    if sections.len() < 2 {
        return Err(KernelError::InvalidParameter("a loft needs two sections".into()));
    }
    let n_loops = 1 + sections[0].1.holes.len();
    let mut shell = truck_modeling::Shell::new();
    let (mut bottom, mut top) = (Vec::new(), Vec::new());
    for k in 0..n_loops {
        let wires: Vec<Wire> = sections
            .iter()
            .map(|(z, r)| {
                let l = if k == 0 { &r.outer } else { r.holes.get(k - 1).ok_or_else(|| KernelError::InvalidParameter("sections differ".into()))? };
                l.wire(*z, k == 0)
            })
            .collect::<Result<_>>()?;
        for w in wires.windows(2) {
            shell.extend(builder::try_wire_homotopy(&w[0], &w[1]).map_err(|e| KernelError::OperationFailed(format!("loft: {e:?}")))?);
        }
        bottom.push(wires[0].clone());
        top.push(wires[wires.len() - 1].clone());
    }
    let cap = |ws: &[Wire]| builder::try_attach_plane(ws).map_err(|e| KernelError::OperationFailed(format!("loft cap: {e:?}")));
    shell.push(cap(&bottom)?.inverse());
    shell.push(cap(&top)?);
    let mut solid = Solid::try_new(vec![shell]).map_err(|e| KernelError::OperationFailed(format!("loft: {e:?}")))?;
    if crate::tessellate(&solid).volume() < 0.0 {
        solid.not();
    }
    Ok(solid)
}

/// Extrude a region between `z0` and `z1` with side walls inclined by
/// `taper_deg` (positive = the profile grows away from the sketch plane).
pub fn extrude_tapered(frame: &Frame, region: &Region, z0: f64, z1: f64, taper_deg: f64) -> Result<Solid> {
    if taper_deg.abs() < 1e-9 {
        return extrude_range(frame, region, z0, z1);
    }
    if z1 - z0 <= 1e-6 || z1.is_nan() || z0.is_nan() {
        return Err(KernelError::InvalidParameter("distance must not be zero".into()));
    }
    if taper_deg.abs() >= 89.0 || !taper_deg.is_finite() {
        return Err(KernelError::InvalidParameter("the taper angle must be between -89° and 89°".into()));
    }
    let k = taper_deg.to_radians().tan();
    // The profile is unchanged on the sketch plane (z = 0).
    let heights = if z0 < 0.0 && z1 > 0.0 { vec![z0, 0.0, z1] } else { vec![z0, z1] };
    let solid = loft_region(region, &heights, |z| k * z.abs())?;
    Ok(builder::transformed(&solid, to_truck(frame.matrix())))
}

/// Revolve a region (in `frame`) around a world-space axis by `angle_deg`
/// (positive = right-handed around `axis_dir`; 360 for a full revolution).
pub fn revolve(frame: &Frame, region: &Region, axis_point: glam::DVec3, axis_dir: glam::DVec3, angle_deg: f64) -> Result<Solid> {
    if !angle_deg.is_finite() || angle_deg.abs() < 1e-3 {
        return Err(KernelError::InvalidParameter("angle must not be zero".into()));
    }
    let dir = axis_dir.normalize_or_zero();
    if dir == glam::DVec3::ZERO {
        return Err(KernelError::InvalidParameter("invalid axis".into()));
    }
    if dir.dot(frame.n).abs() > 1e-6 {
        return Err(KernelError::InvalidParameter("the axis must lie in the sketch plane".into()));
    }
    // The profile must stay on one side of the axis.
    let side: Vec<f64> = region
        .outer
        .points(64)
        .iter()
        .map(|p| {
            let w = frame.to_world(glam::DVec3::new(p[0], p[1], 0.0)) - axis_point;
            w.cross(dir).dot(frame.n)
        })
        .collect();
    let max = side.iter().cloned().fold(f64::MIN, f64::max);
    let min = side.iter().cloned().fold(f64::MAX, f64::min);
    let scale = max.abs().max(min.abs()).max(1e-9);
    if max > 1e-6 * scale && min < -1e-6 * scale && max.min(-min) > 1e-4 * scale {
        return Err(KernelError::InvalidParameter("the profile crosses the revolve axis".into()));
    }
    let face = builder::transformed(&region.face_at(0.0)?, to_truck(frame.matrix()));
    let o = Point3::new(axis_point.x, axis_point.y, axis_point.z);
    let a = Vector3::new(dir.x, dir.y, dir.z);
    let angle = angle_deg.to_radians();
    let full = angle.abs() >= 2.0 * PI - 1e-9;
    let solid = builder::rsweep(&face, o, a, Rad(if full { 2.0 * PI } else { angle }));
    // A negative sweep produces an inside-out solid; fix orientation via volume.
    let mut solid = solid;
    if crate::tessellate(&solid).volume() < 0.0 {
        solid.not();
    }
    Ok(solid)
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
    fn tapered_extrusions() {
        let k = 10f64.to_radians().tan();
        // Square, inward taper: frustum of a pyramid.
        let sq = Region { outer: Loop2::Polygon(vec![[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]]), holes: vec![] };
        let v = tessellate(&extrude_tapered(&xy(), &sq, 0.0, 20.0, -10.0).unwrap()).volume();
        let (a1, a2) = (400.0, (20.0 - 2.0 * 20.0 * k).powi(2));
        let expect = 20.0 / 3.0 * (a1 + a2 + (a1 * a2).sqrt());
        assert!((v - expect).abs() / expect < 0.01, "square {v} vs {expect}");
        // Circle, outward, symmetric: two cone frustums.
        let c = Region { outer: Loop2::Circle { c: [0.0, 0.0], r: 5.0 }, holes: vec![] };
        let v = tessellate(&extrude_tapered(&xy(), &c, -10.0, 10.0, 10.0).unwrap()).volume();
        let r1 = 5.0 + 10.0 * k;
        let expect = 2.0 * PI * 10.0 / 3.0 * (25.0 + r1 * r1 + 5.0 * r1);
        assert!((v - expect).abs() / expect < 0.02, "circle {v} vs {expect}");
        // Square with a round hole: the hole shrinks as the outside grows.
        let holed = Region { holes: vec![Loop2::Circle { c: [0.0, 0.0], r: 4.0 }], ..sq.clone() };
        let v = tessellate(&extrude_tapered(&xy(), &holed, 0.0, 5.0, 5.0).unwrap()).volume();
        assert!(v > 5.0 * holed.area(), "holed {v}");
        // Rounded slot (lines + arcs).
        let slot = Region {
            outer: Loop2::Path(vec![
                Seg2::line([10.0, -5.0]),
                Seg2::arc([15.0, 0.0], [10.0, 5.0]),
                Seg2::line([-10.0, 5.0]),
                Seg2::arc([-15.0, 0.0], [-10.0, -5.0]),
            ]),
            holes: vec![],
        };
        let v = tessellate(&extrude_tapered(&xy(), &slot, 0.0, 10.0, -5.0).unwrap()).volume();
        let base = 20.0 * 10.0 + PI * 25.0;
        assert!(v < 10.0 * base && v > 0.6 * 10.0 * base, "slot {v} vs {}", 10.0 * base);
        // Taper that collapses the profile.
        assert!(extrude_tapered(&xy(), &sq, 0.0, 100.0, -45.0).is_err());
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
    fn revolve_rectangle_into_tube() {
        // Rectangle x in [5, 10], y in [0, 20] revolved around the Y axis.
        let r = Region { outer: Loop2::Polygon(vec![[5.0, 0.0], [10.0, 0.0], [10.0, 20.0], [5.0, 20.0]]), holes: vec![] };
        let s = revolve(&xy(), &r, DVec3::ZERO, DVec3::Y, 360.0).unwrap();
        let v = tessellate(&s).volume();
        let expect = PI * (100.0 - 25.0) * 20.0;
        assert!((v - expect).abs() / expect < 0.01, "{v} vs {expect}");
        let half = revolve(&xy(), &r, DVec3::ZERO, DVec3::Y, -180.0).unwrap();
        let v = tessellate(&half).volume();
        assert!((v - expect / 2.0).abs() / expect < 0.01, "{v}");
        assert!(revolve(&xy(), &r, DVec3::new(7.0, 0.0, 0.0), DVec3::Y, 90.0).is_err());
    }

    #[test]
    fn revolve_profile_touching_axis() {
        // Drill-like profile in the XY plane touching the Y axis: x = radius, y = height.
        let r = Region {
            outer: Loop2::Polygon(vec![[0.0, -12.0], [5.0, -10.0], [5.0, 0.0], [8.0, 0.0], [8.0, 3.0], [0.0, 3.0]]),
            holes: vec![],
        };
        let s = revolve(&xy(), &r, DVec3::ZERO, DVec3::Y, 360.0).unwrap();
        let v = tessellate(&s).volume();
        let expect = PI * (25.0 * 10.0 + 64.0 * 3.0 + 25.0 * 2.0 / 3.0);
        assert!((v - expect).abs() / expect < 0.02, "{v} vs {expect}");
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
