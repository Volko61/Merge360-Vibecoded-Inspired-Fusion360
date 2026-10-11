//! Built-in sample design: a bracket made from a box, a sketch-based cut and
//! a few primitives — exercises most of the implemented features.

use m360_core::{sketch::rectangle, BooleanOp, Document, ExtrudeOp, FeatureKind, PlaneRef, SketchEntity};

pub fn sample_document() -> Document {
    let mut d = Document::new("Sample Bracket");
    let base = d.add_feature(FeatureKind::Box {
        plane: PlaneRef::XZ,
        center: [0.0, 0.0],
        length: 120.0,
        width: 80.0,
        height: 20.0,
    });
    let top = PlaneRef { base: m360_core::BasePlane::XZ, offset: 20.0, flip: false };
    let mut entities = vec![
        SketchEntity::Circle { c: [-40.0, 20.0], r: 8.0 },
        SketchEntity::Circle { c: [40.0, 20.0], r: 8.0 },
        SketchEntity::Circle { c: [-40.0, -20.0], r: 8.0 },
        SketchEntity::Circle { c: [40.0, -20.0], r: 8.0 },
    ];
    entities.extend(rectangle([-20.0, -12.0], [20.0, 12.0]));
    let sk = d.add_feature(FeatureKind::Sketch { plane: top, entities, constraints: vec![] });
    d.add_feature(FeatureKind::Extrude {
        sketch: sk,
        profiles: vec![0, 1, 2, 3],
        distance: -20.0,
        symmetric: false,
        op: ExtrudeOp::Cut,
        participants: vec![base],
    });
    d.add_feature(FeatureKind::Extrude {
        sketch: sk,
        profiles: vec![4],
        distance: 30.0,
        symmetric: false,
        op: ExtrudeOp::Join,
        participants: vec![base],
    });
    let cyl = d.add_feature(FeatureKind::Cylinder {
        plane: PlaneRef { base: m360_core::BasePlane::XZ, offset: 50.0, flip: false },
        center: [0.0, 0.0],
        diameter: 30.0,
        height: 25.0,
    });
    d.add_feature(FeatureKind::Sphere { plane: PlaneRef::XZ, center: [100.0, 0.0], diameter: 40.0 });
    d.sketch_visible.insert(sk, false);
    d.add_feature(FeatureKind::Torus { plane: PlaneRef::XZ, center: [-110.0, 0.0], inner_diameter: 40.0, torus_diameter: 12.0 });
    let _ = (cyl, BooleanOp::Join);
    d
}

#[cfg(test)]
mod tests {
    #[test]
    fn sample_regenerates_cleanly() {
        let d = super::sample_document();
        let m = m360_core::Regenerator::default().regenerate(&d);
        assert!(m.errors.is_empty(), "{:?}", m.errors);
        assert_eq!(m.bodies.len(), 4);

    }
}
