//! Hole tool bodies (simple, counterbore, countersink, with drill point).

use glam::DVec3;

use crate::profile::{extrude_range, Loop2, Region};
use crate::{Frame, KernelError, Result, Solid};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HoleKind {
    Simple,
    Counterbore { diameter: f64, depth: f64 },
    Countersink { diameter: f64, angle_deg: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoleSpec {
    pub diameter: f64,
    /// Depth below the placement face (excluding the drill point).
    pub depth: f64,
    pub kind: HoleKind,
}

/// Tool solids of a hole whose entry point is `center` on a face with
/// outward normal `normal`; cut them from the body one after the other.
/// Built from extruded circles (robust in truck booleans) plus a revolved
/// ring for countersinks. Bottoms are flat.
pub fn hole_tools(center: DVec3, normal: DVec3, spec: &HoleSpec) -> Result<Vec<Solid>> {
    let r = spec.diameter / 2.0;
    if !(r > 1e-6) || !(spec.depth > 1e-6) {
        return Err(KernelError::InvalidParameter("diameter and depth must be greater than zero".into()));
    }
    let n = normal.normalize();
    let u = n.any_orthonormal_vector();
    let v = n.cross(u);
    let disk = Frame::new(center, u, v); // normal = n
    let over = (r * 0.5).max(0.5);
    let circle = |radius: f64| Region { outer: Loop2::Circle { c: [0.0, 0.0], r: radius }, holes: vec![] };
    let mut tools = vec![extrude_range(&disk, &circle(r), -spec.depth, over)?];
    match spec.kind {
        HoleKind::Simple => {}
        HoleKind::Counterbore { diameter, depth } => {
            let rc = diameter / 2.0;
            if rc <= r || depth <= 0.0 || depth >= spec.depth {
                return Err(KernelError::InvalidParameter("counterbore must be wider than the hole and shallower".into()));
            }
            // Rotated frame: keep the circle seams of the two cylinders apart.
            let (c, sn) = (0.7f64.cos(), 0.7f64.sin());
            let u2 = u * c + v * sn;
            let disk2 = Frame::new(center, u2, n.cross(u2));
            // Cutting the wide counterbore first is more robust in truck.
            tools.insert(0, extrude_range(&disk2, &circle(rc), -depth, over * 1.37)?);
        }
        HoleKind::Countersink { diameter, angle_deg } => {
            let rc = diameter / 2.0;
            let half = (angle_deg / 2.0).to_radians();
            if rc <= r || !(0.1..1.5).contains(&half) {
                return Err(KernelError::InvalidParameter("countersink must be wider than the hole".into()));
            }
            let h = (rc - r) / half.tan();
            if h >= spec.depth {
                return Err(KernelError::InvalidParameter("countersink is deeper than the hole".into()));
            }
            // Frustum reaching slightly below the cone start (inside the bore)
            // and above the face, so every intersection is transversal.
            let top = over * 1.37;
            let below = over * 0.5;
            let sink = crate::primitives::make_frustum(
                &disk,
                (r - below * half.tan()).max(r * 0.3),
                -h - below,
                rc + top * half.tan(),
                top,
            )?;
            tools.insert(0, sink);
        }
    }
    Ok(tools)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{boolean, BooleanOp};
    use crate::{primitives::make_box, tessellate};
    use std::f64::consts::PI;

    fn drill(body: &Solid, center: DVec3, n: DVec3, spec: &HoleSpec) -> Result<Solid> {
        let mut acc = body.clone();
        for t in hole_tools(center, n, spec)? {
            acc = boolean(&acc, &[t], BooleanOp::Cut)?;
        }
        Ok(acc)
    }

    fn cube() -> Solid {
        make_box(&Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y), 20.0, 20.0, 20.0).unwrap()
    }

    #[test]
    fn simple_blind_hole() {
        let spec = HoleSpec { diameter: 6.0, depth: 10.0, kind: HoleKind::Simple };
        let s = drill(&cube(), DVec3::new(0.0, 0.0, 20.0), DVec3::Z, &spec).unwrap();
        let v = tessellate(&s).volume();
        let expect = 8000.0 - PI * 9.0 * 10.0;
        assert!((v - expect).abs() < 6.0, "{v} vs {expect}");
    }

    #[test]
    fn counterbore_and_countersink() {
        for kind in [
            HoleKind::Counterbore { diameter: 10.0, depth: 3.0 },
            HoleKind::Countersink { diameter: 10.0, angle_deg: 90.0 },
        ] {
            let spec = HoleSpec { diameter: 5.0, depth: 12.0, kind };
            let s = drill(&cube(), DVec3::new(2.0, -3.0, 20.0), DVec3::Z, &spec);
            assert!(s.is_ok(), "{kind:?}: {:?}", s.err());
            let v = tessellate(&s.unwrap()).volume();
            assert!(v < 8000.0 - PI * 6.25 * 12.0 && v > 7000.0, "{kind:?} {v}");
        }
    }

    #[test]
    fn through_hole_on_side_face() {
        let spec = HoleSpec { diameter: 4.0, depth: 1000.0, kind: HoleKind::Simple };
        let s = drill(&cube(), DVec3::new(10.0, 0.0, 10.0), DVec3::X, &spec).unwrap();
        let v = tessellate(&s).volume();
        let expect = 8000.0 - PI * 4.0 * 20.0;
        assert!((v - expect).abs() < 3.0, "{v} vs {expect}");
    }
}
