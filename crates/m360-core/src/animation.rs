//! ANIMATION workspace storyboard: timed moves of bodies (exploded views,
//! assembly instructions).

use glam::{DMat4, DVec3};
use serde::{Deserialize, Serialize};

use crate::feature::Axis;
use crate::model::{BodyId, Model};

/// One storyboard action: bodies move and/or rotate over a time interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimStep {
    pub bodies: Vec<BodyId>,
    /// Start time and duration (seconds).
    pub start: f64,
    pub duration: f64,
    pub translate: [f64; 3],
    pub axis: Axis,
    /// Rotation about the body's center (degrees).
    pub angle_deg: f64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Storyboard {
    pub steps: Vec<AnimStep>,
}

fn ease(f: f64) -> f64 {
    let f = f.clamp(0.0, 1.0);
    f * f * (3.0 - 2.0 * f)
}

impl Storyboard {
    /// Total length (seconds).
    pub fn length(&self) -> f64 {
        self.steps.iter().map(|s| s.start + s.duration).fold(0.0, f64::max)
    }

    /// Transform of a body at time `t`; `center` is its rest-position center.
    pub fn transform(&self, body: BodyId, center: DVec3, t: f64) -> DMat4 {
        let mut steps: Vec<&AnimStep> = self.steps.iter().filter(|s| s.bodies.contains(&body)).collect();
        steps.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut m = DMat4::IDENTITY;
        for s in steps {
            let f = if s.duration <= 1e-9 { if t >= s.start { 1.0 } else { 0.0 } } else { ease((t - s.start) / s.duration) };
            if f <= 0.0 {
                continue;
            }
            let c = m.transform_point3(center);
            let rot = DMat4::from_translation(c) * DMat4::from_axis_angle(s.axis.vec(), (s.angle_deg * f).to_radians()) * DMat4::from_translation(-c);
            m = DMat4::from_translation(DVec3::from(s.translate) * f) * rot * m;
        }
        m
    }

    /// Steps of a one-shot exploded view: every body moves away from the
    /// assembly center by `scale` times the assembly size.
    pub fn auto_explode(model: &Model, bodies: &[BodyId], start: f64, scale: f64) -> Vec<AnimStep> {
        let centers: Vec<(BodyId, DVec3)> = bodies
            .iter()
            .filter_map(|id| model.body(*id))
            .map(|b| (b.id, ((b.mesh.bbox_min + b.mesh.bbox_max) * 0.5).as_dvec3()))
            .collect();
        if centers.len() < 2 {
            return Vec::new();
        }
        let center = centers.iter().map(|c| c.1).sum::<DVec3>() / centers.len() as f64;
        let (mut lo, mut hi) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
        for id in bodies {
            if let Some(b) = model.body(*id) {
                lo = lo.min(b.mesh.bbox_min.as_dvec3());
                hi = hi.max(b.mesh.bbox_max.as_dvec3());
            }
        }
        let size = (hi - lo).length().max(1.0);
        centers
            .into_iter()
            .map(|(id, c)| {
                let d = c - center;
                let dir = if d.length() < 1e-6 * size { DVec3::Y } else { d.normalize() };
                AnimStep { bodies: vec![id], start, duration: 1.5, translate: (dir * size * scale).to_array(), axis: Axis::Y, angle_deg: 0.0 }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_compose() {
        let sb = Storyboard {
            steps: vec![
                AnimStep { bodies: vec![1], start: 0.0, duration: 1.0, translate: [10.0, 0.0, 0.0], axis: Axis::Y, angle_deg: 0.0 },
                AnimStep { bodies: vec![1], start: 1.0, duration: 1.0, translate: [0.0, 0.0, 0.0], axis: Axis::Y, angle_deg: 90.0 },
            ],
        };
        assert_eq!(sb.length(), 2.0);
        assert_eq!(sb.transform(1, DVec3::ZERO, -1.0), DMat4::IDENTITY);
        let half = sb.transform(1, DVec3::ZERO, 0.5).transform_point3(DVec3::ZERO);
        assert!((half.x - 5.0).abs() < 1e-9);
        // After both: moved by 10 and rotated about its own (moved) center.
        let end = sb.transform(1, DVec3::ZERO, 5.0);
        assert!((end.transform_point3(DVec3::ZERO) - DVec3::new(10.0, 0.0, 0.0)).length() < 1e-9);
        assert!((end.transform_vector3(DVec3::X) - DVec3::NEG_Z).length() < 1e-9);
        assert_eq!(sb.transform(2, DVec3::ZERO, 5.0), DMat4::IDENTITY);
    }
}
