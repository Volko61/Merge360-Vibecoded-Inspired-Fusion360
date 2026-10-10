//! Solid operations: booleans and rigid transforms.

use glam::DMat4;
use truck_modeling::builder;

use crate::primitives::to_truck;
use crate::{KernelError, Result, Solid};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    Join,
    Cut,
    Intersect,
}

/// Boolean tolerance in millimetres.
const BOOL_TOL: f64 = 0.05;

/// Combine `target` with each of `tools` in sequence.
pub fn boolean(target: &Solid, tools: &[Solid], op: BooleanOp) -> Result<Solid> {
    let mut acc = target.clone();
    for tool in tools {
        let res = guarded(|| match op {
            BooleanOp::Join => truck_shapeops::or(&acc, tool, BOOL_TOL),
            BooleanOp::Intersect => truck_shapeops::and(&acc, tool, BOOL_TOL),
            BooleanOp::Cut => {
                let mut inv = tool.clone();
                inv.not();
                truck_shapeops::and(&acc, &inv, BOOL_TOL)
            }
        })?;
        acc = res.ok_or_else(|| {
            KernelError::OperationFailed(format!("{op:?} failed: bodies could not be intersected"))
        })?;
        if acc.boundaries().is_empty() {
            return Err(KernelError::OperationFailed(format!("{op:?} produced an empty body")));
        }
    }
    Ok(acc)
}

/// truck's boolean code can panic on degenerate input (e.g. coplanar faces);
/// turn such panics into a recoverable error.
fn guarded<T>(f: impl FnOnce() -> Option<T>) -> Result<Option<T>> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|e| {
        let msg = e
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| e.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "kernel panic".into());
        KernelError::OperationFailed(format!("boolean failed: {msg}"))
    })
}

pub fn transform(solid: &Solid, m: DMat4) -> Solid {
    builder::transformed(solid, to_truck(m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{primitives::*, tessellate};
    use glam::DVec3;

    fn xy() -> Frame {
        Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y)
    }

    #[test]
    fn cut_box_with_box() {
        let a = make_box(&xy(), 10.0, 10.0, 10.0).unwrap();
        let b = make_box(&Frame::new(DVec3::new(5.0, 0.0, -5.0), DVec3::X, DVec3::Y), 10.0, 20.0, 20.0)
            .unwrap();
        let c = boolean(&a, &[b], BooleanOp::Cut).unwrap();
        let v = tessellate(&c).volume();
        assert!((v - 500.0).abs() < 5.0, "{v}");
    }

    #[test]
    fn translate() {
        let a = make_box(&xy(), 10.0, 10.0, 10.0).unwrap();
        let t = transform(&a, DMat4::from_translation(DVec3::new(100.0, 0.0, 0.0)));
        let m = tessellate(&t);
        assert!((m.bbox_min.x - 95.0).abs() < 1e-3);
    }
}
