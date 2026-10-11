//! Coil: a section profile swept along a (possibly tapered) helix.
//!
//! Like Fusion, the section lies in the axial plane through the coil axis.
//! The helix is sampled at stations; consecutive section wires are joined by
//! ruled faces and the ends are closed with planar caps.

use std::f64::consts::TAU;

use glam::DVec3;
use truck_modeling::{builder, Point3, Rad, Shell, Vector3, Wire};

use crate::{Frame, KernelError, Result, Solid};

/// Shape of the coil section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoilSection {
    #[default]
    Circular,
    Square,
    /// Triangle pointing away from the axis.
    TriangularExternal,
    /// Triangle pointing towards the axis.
    TriangularInternal,
}

/// Where the section sits relative to the coil diameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SectionPosition {
    Inside,
    #[default]
    Center,
    Outside,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoilSpec {
    pub diameter: f64,
    pub turns: f64,
    pub height: f64,
    /// Taper of the helix in degrees (positive = the coil widens upwards).
    pub taper_deg: f64,
    pub section: CoilSection,
    pub size: f64,
    pub position: SectionPosition,
    pub clockwise: bool,
}

/// Stations per full turn.
const PER_TURN: f64 = 24.0;

fn tp(p: DVec3) -> Point3 {
    Point3::new(p.x, p.y, p.z)
}

/// Section loop in (radial, axial) coordinates around the section center.
fn section_points(section: CoilSection, s: f64) -> Option<Vec<[f64; 2]>> {
    let h = s * 3f64.sqrt() / 2.0;
    match section {
        CoilSection::Circular => None,
        CoilSection::Square => Some(vec![[-s / 2.0, -s / 2.0], [s / 2.0, -s / 2.0], [s / 2.0, s / 2.0], [-s / 2.0, s / 2.0]]),
        CoilSection::TriangularExternal => Some(vec![[-h / 3.0, -s / 2.0], [2.0 * h / 3.0, 0.0], [-h / 3.0, s / 2.0]]),
        CoilSection::TriangularInternal => Some(vec![[h / 3.0, -s / 2.0], [h / 3.0, s / 2.0], [-2.0 * h / 3.0, 0.0]]),
    }
}

pub fn make_coil(frame: &Frame, spec: &CoilSpec) -> Result<Solid> {
    let CoilSpec { diameter, turns, height, taper_deg, section, size, position, clockwise } = *spec;
    let bad = |m: &str| Err(KernelError::InvalidParameter(m.into()));
    if diameter.is_nan() || size.is_nan() || diameter <= 1e-6 || size <= 1e-6 {
        return bad("diameter and section size must be greater than zero");
    }
    if turns.is_nan() || !(0.05..=200.0).contains(&turns) {
        return bad("revolutions must be between 0.05 and 200");
    }
    if !height.is_finite() || height < 0.0 {
        return bad("height must not be negative");
    }
    if !taper_deg.is_finite() || taper_deg.abs() >= 80.0 {
        return bad("the taper angle must be between -80° and 80°");
    }
    if turns > 1.0 && height / turns < size * 1.001 {
        return bad("the pitch must be larger than the section size (the coil would intersect itself)");
    }
    let shift = match position {
        SectionPosition::Inside => -size / 2.0,
        SectionPosition::Center => 0.0,
        SectionPosition::Outside => size / 2.0,
    };
    let k = taper_deg.to_radians().tan();
    let radius = |z: f64| diameter / 2.0 + k * z + shift;
    let reach = if section == CoilSection::Square { size / 2.0 * 2f64.sqrt() } else { size / 2.0 };
    if radius(0.0) - reach < 1e-6 || radius(height) - reach < 1e-6 {
        return bad("the section crosses the coil axis");
    }
    let n = (turns * PER_TURN).ceil().max(2.0) as usize;
    let dir = if clockwise { -1.0 } else { 1.0 };
    let pts2 = section_points(section, size);
    let wires: Vec<Wire> = (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let theta = dir * turns * TAU * t;
            let z = height * t;
            let rhat = frame.u * theta.cos() + frame.v * theta.sin();
            let center = frame.origin + rhat * radius(z) + frame.n * z;
            match &pts2 {
                None => {
                    let v = builder::vertex(tp(center + rhat * size / 2.0));
                    let axis = frame.n.cross(rhat);
                    builder::rsweep(&v, tp(center), Vector3::new(axis.x, axis.y, axis.z), Rad(TAU))
                }
                Some(ps) => {
                    let vs: Vec<_> = ps.iter().map(|p| builder::vertex(tp(center + rhat * p[0] + frame.n * p[1]))).collect();
                    (0..vs.len()).map(|j| builder::line(&vs[j], &vs[(j + 1) % vs.len()])).collect()
                }
            }
        })
        .collect();
    let mut shell = Shell::new();
    for w in wires.windows(2) {
        shell.extend(builder::try_wire_homotopy(&w[0], &w[1]).map_err(|e| KernelError::OperationFailed(format!("coil: {e:?}")))?);
    }
    let cap = |w: &Wire| {
        builder::try_attach_plane(std::slice::from_ref(w)).map_err(|e| KernelError::OperationFailed(format!("coil cap: {e:?}")))
    };
    shell.push(cap(&wires[0])?.inverse());
    shell.push(cap(&wires[n])?);
    let mut solid = Solid::try_new(vec![shell]).map_err(|e| KernelError::OperationFailed(format!("coil: {e:?}")))?;
    if crate::tessellate(&solid).volume() < 0.0 {
        solid.not();
    }
    Ok(solid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tessellate;
    use std::f64::consts::PI;

    fn spec(section: CoilSection) -> CoilSpec {
        CoilSpec {
            diameter: 40.0,
            turns: 3.0,
            height: 30.0,
            taper_deg: 0.0,
            section,
            size: 4.0,
            position: SectionPosition::Center,
            clockwise: false,
        }
    }

    #[test]
    fn coil_volumes() {
        let f = Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y);
        // Pappus (the axial shear of the helix preserves volume): section area
        // × circumference of the section centroid × turns; chords lose a bit.
        for (sec, area) in [
            (CoilSection::Circular, PI * 4.0),
            (CoilSection::Square, 16.0),
            (CoilSection::TriangularExternal, 3f64.sqrt() / 4.0 * 16.0),
        ] {
            let s = make_coil(&f, &spec(sec)).unwrap();
            let v = tessellate(&s).volume();
            let expect = area * TAU * 20.0 * 3.0;
            assert!((v - expect).abs() / expect < 0.05, "{sec:?}: {v} vs {expect}");
        }
        let mut tight = spec(CoilSection::Circular);
        tight.height = 6.0;
        assert!(make_coil(&f, &tight).is_err());
        let mut tapered = spec(CoilSection::Circular);
        tapered.taper_deg = 10.0;
        tapered.clockwise = true;
        assert!(tessellate(&make_coil(&f, &tapered).unwrap()).volume() > 0.0);
    }
}
