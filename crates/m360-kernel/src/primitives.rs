//! Primitive solids (Create ▸ Box / Cylinder / Sphere / Torus).
//!
//! Primitives are built in a local frame whose Z axis is the sketch-plane
//! normal and then mapped into world space with [`Frame::matrix`].

use std::f64::consts::PI;

use glam::{DMat4, DVec3};
use truck_modeling::{builder, EuclideanSpace, Matrix4, Point3, Rad, Shell, Vector3, Wire};

use crate::{KernelError, Result, Solid};

/// A placement frame: origin plus orthonormal (u, v, n) axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub origin: DVec3,
    pub u: DVec3,
    pub v: DVec3,
    pub n: DVec3,
}

impl Frame {
    pub fn new(origin: DVec3, u: DVec3, v: DVec3) -> Self {
        let u = u.normalize();
        let n = u.cross(v).normalize();
        let v = n.cross(u);
        Self { origin, u, v, n }
    }

    /// Local -> world transform.
    pub fn matrix(&self) -> DMat4 {
        DMat4::from_cols(
            self.u.extend(0.0),
            self.v.extend(0.0),
            self.n.extend(0.0),
            self.origin.extend(1.0),
        )
    }

    pub fn to_world(&self, local: DVec3) -> DVec3 {
        self.origin + self.u * local.x + self.v * local.y + self.n * local.z
    }
}

pub(crate) fn to_truck(m: DMat4) -> Matrix4 {
    let c = m.to_cols_array_2d();
    Matrix4::new(
        c[0][0], c[0][1], c[0][2], c[0][3], c[1][0], c[1][1], c[1][2], c[1][3], c[2][0], c[2][1],
        c[2][2], c[2][3], c[3][0], c[3][1], c[3][2], c[3][3],
    )
}

fn positive(name: &str, value: f64) -> Result<()> {
    if value.is_finite() && value > 1e-6 {
        Ok(())
    } else {
        Err(KernelError::InvalidParameter(format!(
            "{name} must be greater than zero"
        )))
    }
}

fn nonzero(name: &str, value: f64) -> Result<()> {
    if value.is_finite() && value.abs() > 1e-6 {
        Ok(())
    } else {
        Err(KernelError::InvalidParameter(format!("{name} must not be zero")))
    }
}

fn place(solid: Solid, frame: &Frame) -> Solid {
    builder::transformed(&solid, to_truck(frame.matrix()))
}

/// Box centered on the frame origin in (u, v), extruded `height` along n
/// (negative heights extrude the other way).
pub fn make_box(frame: &Frame, length: f64, width: f64, height: f64) -> Result<Solid> {
    positive("Length", length)?;
    positive("Width", width)?;
    nonzero("Height", height)?;
    let z0 = height.min(0.0);
    let v = builder::vertex(Point3::new(-length / 2.0, -width / 2.0, z0));
    let e = builder::tsweep(&v, Vector3::new(length, 0.0, 0.0));
    let f = builder::tsweep(&e, Vector3::new(0.0, width, 0.0));
    let solid = builder::tsweep(&f, Vector3::new(0.0, 0.0, height.abs()));
    Ok(place(solid, frame))
}

/// Cylinder whose base circle is centered on the frame origin.
pub fn make_cylinder(frame: &Frame, diameter: f64, height: f64) -> Result<Solid> {
    positive("Diameter", diameter)?;
    nonzero("Height", height)?;
    let r = diameter / 2.0;
    let z0 = height.min(0.0);
    let v = builder::vertex(Point3::new(r, 0.0, z0));
    let circle: Wire = builder::rsweep(&v, Point3::new(0.0, 0.0, z0), Vector3::unit_z(), Rad(2.0 * PI));
    let disk = builder::try_attach_plane(&[circle])
        .map_err(|e| KernelError::OperationFailed(format!("cylinder base: {e:?}")))?;
    let solid = builder::tsweep(&disk, Vector3::new(0.0, 0.0, height.abs()));
    Ok(place(solid, frame))
}

/// Sphere centered on the frame origin.
pub fn make_sphere(frame: &Frame, diameter: f64) -> Result<Solid> {
    positive("Diameter", diameter)?;
    let r = diameter / 2.0;
    let top = builder::vertex(Point3::new(0.0, 0.0, r));
    let meridian: Wire = builder::rsweep(&top, Point3::origin(), Vector3::unit_y(), Rad(PI));
    let shell: Shell = builder::cone(&meridian, Vector3::unit_z(), Rad(2.0 * PI));
    Ok(place(Solid::new(vec![shell]), frame))
}

/// Torus lying in the frame plane, centered on the frame origin.
pub fn make_torus(frame: &Frame, major_diameter: f64, minor_diameter: f64) -> Result<Solid> {
    positive("Inner diameter", major_diameter)?;
    positive("Torus diameter", minor_diameter)?;
    let big_r = major_diameter / 2.0;
    let small_r = minor_diameter / 2.0;
    if small_r >= big_r {
        return Err(KernelError::InvalidParameter(
            "torus diameter must be smaller than the major diameter".into(),
        ));
    }
    let v = builder::vertex(Point3::new(big_r + small_r, 0.0, 0.0));
    let section: Wire = builder::rsweep(&v, Point3::new(big_r, 0.0, 0.0), Vector3::unit_y(), Rad(2.0 * PI));
    let shell: Shell = builder::rsweep(&section, Point3::origin(), Vector3::unit_z(), Rad(2.0 * PI));
    Ok(place(Solid::new(vec![shell]), frame))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::tessellate;

    fn xy() -> Frame {
        Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y)
    }

    fn rel_close(a: f64, b: f64, tol: f64) -> bool {
        ((a - b) / b).abs() < tol
    }

    #[test]
    fn box_volume() {
        let s = make_box(&xy(), 10.0, 20.0, 30.0).unwrap();
        let m = tessellate(&s);
        assert!(rel_close(m.volume(), 6000.0, 1e-3), "{}", m.volume());
    }

    #[test]
    fn negative_height_box() {
        let s = make_box(&xy(), 10.0, 10.0, -10.0).unwrap();
        let m = tessellate(&s);
        assert!(rel_close(m.volume(), 1000.0, 1e-3), "{}", m.volume());
        assert!(m.bbox_max.z <= 1e-4);
    }

    #[test]
    fn cylinder_volume() {
        let s = make_cylinder(&xy(), 20.0, 10.0).unwrap();
        let m = tessellate(&s);
        assert!(rel_close(m.volume(), PI * 100.0 * 10.0, 1e-2), "{}", m.volume());
    }

    #[test]
    fn sphere_volume() {
        let s = make_sphere(&xy(), 20.0).unwrap();
        let m = tessellate(&s);
        assert!(rel_close(m.volume(), 4.0 / 3.0 * PI * 1000.0, 2e-2), "{}", m.volume());
    }

    #[test]
    fn torus_volume() {
        let s = make_torus(&xy(), 40.0, 10.0).unwrap();
        let m = tessellate(&s);
        let expect = 2.0 * PI * PI * 20.0 * 25.0;
        assert!(rel_close(m.volume(), expect, 2e-2), "{}", m.volume());
    }

    #[test]
    fn visible_edges() {
        let b = tessellate(&make_box(&xy(), 10.0, 10.0, 10.0).unwrap());
        assert_eq!(b.edges.len(), 12);
        let s = tessellate(&make_sphere(&xy(), 10.0).unwrap());
        assert_eq!(s.edges.len(), 0, "sphere seams should be hidden");
        let c = tessellate(&make_cylinder(&xy(), 10.0, 5.0).unwrap());
        assert!(!c.edges.is_empty(), "cylinder rims must be visible");
        // Only the two rims (possibly split into arcs) remain: no side seam.
        for e in &c.edges {
            let z0 = e[0][2];
            assert!(e.iter().all(|p| (p[2] - z0).abs() < 1e-4), "unexpected side edge");
        }
    }

    #[test]
    fn rejects_bad_params() {
        assert!(make_box(&xy(), 0.0, 1.0, 1.0).is_err());
        assert!(make_torus(&xy(), 10.0, 20.0).is_err());
    }
}
