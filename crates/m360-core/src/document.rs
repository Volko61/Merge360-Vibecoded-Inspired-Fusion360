//! The persistent document (what gets saved to `.m360`).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::feature::{Feature, FeatureKind};
use crate::model::BodyId;
use crate::units::Units;

/// Per-body user attributes that are not part of the geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BodyMeta {
    pub name: String,
    pub visible: bool,
    /// Appearance color (linear-ish sRGB, 0..1).
    pub color: [f32; 3],
    /// Owning component (None = root component).
    #[serde(default)]
    pub component: Option<u64>,
    /// Physical material name (None = Steel).
    #[serde(default)]
    pub material: Option<String>,
}

/// A component (organises bodies; joints move whole components).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComponentDef {
    pub id: u64,
    pub name: String,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedView {
    pub name: String,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub name: String,
    pub units: Units,
    pub features: Vec<Feature>,
    /// Timeline marker: features at index >= marker are rolled back.
    pub marker: usize,
    pub next_uid: u64,
    pub bodies: BTreeMap<BodyId, BodyMeta>,
    pub sketch_visible: BTreeMap<u64, bool>,
    #[serde(default)]
    pub plane_visible: BTreeMap<u64, bool>,
    pub origin_visible: bool,
    pub bodies_folder_visible: bool,
    pub sketches_folder_visible: bool,
    pub named_views: Vec<NamedView>,
    #[serde(default)]
    pub components: Vec<ComponentDef>,
    #[serde(default)]
    pub params: Vec<crate::expr::UserParam>,
    /// Manufacture workspace setups.
    #[serde(default)]
    pub cam: Vec<crate::cam::CamSetup>,
    /// Animation workspace storyboard.
    #[serde(default)]
    pub animation: crate::animation::Storyboard,
    /// Simulation workspace study.
    #[serde(default)]
    pub simulation: Option<crate::simulation::SimStudy>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            name: "Untitled".into(),
            units: Units::Millimeter,
            features: Vec::new(),
            marker: 0,
            next_uid: 1,
            bodies: BTreeMap::new(),
            sketch_visible: BTreeMap::new(),
            plane_visible: BTreeMap::new(),
            origin_visible: false,
            bodies_folder_visible: true,
            sketches_folder_visible: true,
            named_views: Vec::new(),
            components: Vec::new(),
            params: Vec::new(),
            cam: Vec::new(),
            animation: Default::default(),
            simulation: None,
        }
    }
}

/// Default palette Fusion-like "Steel - Satin" grey plus a few accents for
/// copied bodies, so they can be told apart.
const BODY_COLOR: [f32; 3] = [0.72, 0.74, 0.77];

impl Document {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Default::default() }
    }

    fn next_index(&self, base: &str) -> usize {
        self.features
            .iter()
            .filter_map(|f| f.name.strip_prefix(base)?.parse::<usize>().ok())
            .max()
            .unwrap_or(0)
            + 1
    }

    /// Insert a feature at the marker (like Fusion, features are added where
    /// the marker is) and advance the marker. Returns the new uid.
    pub fn add_feature(&mut self, kind: FeatureKind) -> u64 {
        let uid = self.next_uid;
        self.next_uid += 1;
        let base = kind.base_name();
        let name = format!("{base}{}", self.next_index(base));
        if kind.creates_body() {
            let n = self.bodies.len() + 1;
            self.bodies.insert(
                uid,
                BodyMeta { name: format!("Body{n}"), visible: true, color: BODY_COLOR, component: None, material: None },
            );
        }
        if let Some((bodies, mats)) = kind.copy_transforms() {
            for i in 0..bodies.len() * mats.len() {
                let n = self.bodies.len() + 1;
                self.bodies.insert(
                    (uid << 16) | i as u64,
                    BodyMeta { name: format!("Body{n}"), visible: true, color: BODY_COLOR, component: None, material: None },
                );
            }
        }
        if matches!(kind, FeatureKind::Sketch { .. }) {
            self.sketch_visible.insert(uid, true);
        }
        let at = self.marker.min(self.features.len());
        self.features.insert(at, crate::Feature { uid, name, suppressed: false, kind, exprs: Default::default() });
        self.marker = at + 1;
        uid
    }

    /// Create a component holding `bodies`; returns its id.
    pub fn new_component(&mut self, bodies: &[u64]) -> u64 {
        let id = self.next_uid;
        self.next_uid += 1;
        let n = self.components.len() + 1;
        self.components.push(ComponentDef { id, name: format!("Component{n}"), visible: true });
        for b in bodies {
            let mut m = self.body_meta(*b);
            m.component = Some(id);
            self.bodies.insert(*b, m);
        }
        id
    }

    /// Bodies that belong to the same component as `body` (or just `body`).
    pub fn component_bodies(&self, body: u64) -> Vec<u64> {
        match self.body_meta(body).component {
            Some(c) => self.bodies.iter().filter(|(_, m)| m.component == Some(c)).map(|(id, _)| *id).collect(),
            None => vec![body],
        }
    }

    pub fn feature_index(&self, uid: u64) -> Option<usize> {
        self.features.iter().position(|f| f.uid == uid)
    }

    pub fn feature(&self, uid: u64) -> Option<&Feature> {
        self.features.iter().find(|f| f.uid == uid)
    }

    pub fn feature_mut(&mut self, uid: u64) -> Option<&mut Feature> {
        self.features.iter_mut().find(|f| f.uid == uid)
    }

    pub fn delete_feature(&mut self, uid: u64) {
        if let Some(i) = self.feature_index(uid) {
            self.features.remove(i);
            if self.marker > i {
                self.marker -= 1;
            }
        }
    }

    pub fn body_meta(&self, id: BodyId) -> BodyMeta {
        self.bodies.get(&id).cloned().unwrap_or(BodyMeta {
            name: format!("Body#{id}"),
            visible: true,
            color: BODY_COLOR,
            component: None,
            material: None,
        })
    }

    pub fn set_marker(&mut self, marker: usize) {
        self.marker = marker.min(self.features.len());
    }

    pub fn to_ron(&self) -> Result<String, String> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(|e| e.to_string())
    }

    pub fn from_ron(s: &str) -> Result<Self, String> {
        ron::from_str(s).map_err(|e| e.to_string())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        std::fs::write(path, self.to_ron()?).map_err(|e| e.to_string())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut doc = Self::from_ron(&s)?;
        if let Some(stem) = path.file_stem() {
            doc.name = stem.to_string_lossy().into_owned();
        }
        Ok(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BooleanOp, PlaneRef, Regenerator};

    fn boxf(x: f64) -> FeatureKind {
        FeatureKind::Box { plane: PlaneRef::XY, center: [x, 0.0], length: 10.0, width: 10.0, height: 10.0 }
    }

    #[test]
    fn names_and_marker() {
        let mut d = Document::default();
        let a = d.add_feature(boxf(0.0));
        let b = d.add_feature(boxf(20.0));
        assert_eq!(d.feature(a).unwrap().name, "Box1");
        assert_eq!(d.feature(b).unwrap().name, "Box2");
        assert_eq!(d.marker, 2);
        d.set_marker(1);
        let c = d.add_feature(boxf(40.0));
        assert_eq!(d.feature_index(c), Some(1));
        assert_eq!(d.marker, 2);
    }

    #[test]
    fn regenerate_and_rollback() {
        let mut d = Document::default();
        d.add_feature(boxf(0.0));
        d.add_feature(boxf(20.0));
        let mut r = Regenerator::default();
        assert_eq!(r.regenerate(&d).bodies.len(), 2);
        d.set_marker(1);
        assert_eq!(r.regenerate(&d).bodies.len(), 1);
    }

    #[test]
    fn combine_cut_and_missing_reference() {
        let mut d = Document::default();
        let a = d.add_feature(boxf(0.0));
        let b = d.add_feature(FeatureKind::Box {
            plane: PlaneRef::XY,
            center: [5.0, 0.0],
            length: 10.0,
            width: 20.0,
            height: 20.0,
        });
        // Offset the tool so faces are not coplanar.
        if let FeatureKind::Box { plane, .. } = &mut d.feature_mut(b).unwrap().kind {
            plane.offset = -5.0;
        }
        let c = d.add_feature(FeatureKind::Combine {
            target: a,
            tools: vec![b],
            op: BooleanOp::Cut,
            keep_tools: false,
        });
        let mut r = Regenerator::default();
        let m = r.regenerate(&d);
        assert!(m.errors.is_empty(), "{:?}", m.errors);
        assert_eq!(m.bodies.len(), 1);
        assert!((m.bodies[0].mesh.volume() - 500.0).abs() < 5.0);

        d.feature_mut(b).unwrap().suppressed = true;
        let m = r.regenerate(&d);
        assert!(m.errors.contains_key(&c));
    }

    #[test]
    fn sketch_extrude_then_cut() {
        use crate::{ExtrudeOp, SketchEntity};
        let mut d = Document::default();
        let sk = d.add_feature(FeatureKind::Sketch {
            constraints: vec![],
            plane: PlaneRef::XY,
            entities: {
                let mut e = crate::sketch::rectangle([-10.0, -10.0], [10.0, 10.0]);
                e.push(SketchEntity::Circle { c: [0.0, 0.0], r: 4.0 });
                e
            },
        });
        // Profile 0 is the circle, profile 1 the rectangle with a hole.
        let base = d.add_feature(FeatureKind::Extrude {
            taper: 0.0,
            faces: Vec::new(),
            sketch: sk,
            profiles: vec![0, 1],
            distance: 5.0,
            symmetric: false,
            op: ExtrudeOp::NewBody,
            participants: vec![],
        });
        d.add_feature(FeatureKind::Extrude {
            taper: 0.0,
            faces: Vec::new(),
            sketch: sk,
            profiles: vec![0],
            distance: 5.0,
            symmetric: false,
            op: ExtrudeOp::Cut,
            participants: vec![base],
        });
        let mut r = Regenerator::default();
        let m = r.regenerate(&d);
        assert!(m.errors.is_empty(), "{:?}", m.errors);
        assert_eq!(m.bodies.len(), 1);
        let expect = (400.0 - std::f64::consts::PI * 16.0) * 5.0;
        let v = m.bodies[0].mesh.volume();
        assert!((v - expect).abs() / expect < 0.02, "{v} vs {expect}");
    }

    #[test]
    fn revolve_and_slot_extrude() {
        use crate::{ExtrudeOp, RevolveAxis, SketchEntity};
        let mut d = Document::default();
        // Rectangle away from the Y axis on the XY plane, revolved into a tube.
        let sk = d.add_feature(FeatureKind::Sketch {
            constraints: vec![],
            plane: PlaneRef::XY,
            entities: crate::sketch::rectangle([5.0, 0.0], [10.0, 20.0]),
        });
        d.add_feature(FeatureKind::Revolve {
            sketch: sk,
            profiles: vec![0],
            axis: RevolveAxis::Y,
            angle_deg: 360.0,
            op: ExtrudeOp::NewBody,
            participants: vec![],
        });
        let slot = d.add_feature(FeatureKind::Sketch {
            constraints: vec![],
            plane: PlaneRef::XZ,
            entities: vec![
                SketchEntity::Line { a: [30.0, 0.0], b: [40.0, 0.0] },
                SketchEntity::Arc { a: [40.0, 0.0], m: [42.0, 2.0], b: [40.0, 4.0] },
                SketchEntity::Line { a: [40.0, 4.0], b: [30.0, 4.0] },
                SketchEntity::Arc { a: [30.0, 4.0], m: [28.0, 2.0], b: [30.0, 0.0] },
            ],
        });
        d.add_feature(FeatureKind::Extrude {
            taper: 0.0,
            faces: Vec::new(),
            sketch: slot,
            profiles: vec![0],
            distance: 5.0,
            symmetric: false,
            op: ExtrudeOp::NewBody,
            participants: vec![],
        });
        let m = Regenerator::default().regenerate(&d);
        assert!(m.errors.is_empty(), "{:?}", m.errors);
        assert_eq!(m.bodies.len(), 2);
        let tube = std::f64::consts::PI * 75.0 * 20.0;
        assert!((m.bodies[0].mesh.volume() - tube).abs() / tube < 0.01);
        let slot_v = (40.0 + std::f64::consts::PI * 4.0) * 5.0;
        assert!((m.bodies[1].mesh.volume() - slot_v).abs() / slot_v < 0.01, "{}", m.bodies[1].mesh.volume());
    }

    #[test]
    fn holes_in_box() {
        use crate::{BasePlane, HoleType};
        let mut d = Document::default();
        let b = d.add_feature(FeatureKind::Box { plane: PlaneRef::XZ, center: [0.0, 0.0], length: 40.0, width: 40.0, height: 10.0 });
        d.add_feature(FeatureKind::Hole {
            plane: PlaneRef { base: BasePlane::XZ, offset: 10.0, flip: false, custom: None },
            points: vec![[-10.0, -10.0], [10.0, 10.0]],
            diameter: 5.0,
            depth: 0.0,
            through_all: true,
            kind: HoleType::Counterbore { diameter: 9.0, depth: 3.0 },
            participants: vec![b],
        });
        let m = Regenerator::default().regenerate(&d);
        assert!(m.errors.is_empty(), "{:?}", m.errors);
        let pi = std::f64::consts::PI;
        let removed = 2.0 * (pi * 6.25 * 7.0 + pi * 20.25 * 3.0);
        let v = m.bodies[0].mesh.volume();
        assert!((v - (16000.0 - removed)).abs() < 15.0, "{v}");
    }

    #[test]
    fn pattern_and_mirror() {
        use crate::{Axis, BasePlane, PatternKind};
        let mut d = Document::default();
        let b = d.add_feature(FeatureKind::Box { plane: PlaneRef::XY, center: [20.0, 0.0], length: 10.0, width: 10.0, height: 10.0 });
        d.add_feature(FeatureKind::Pattern {
            bodies: vec![b],
            pattern: PatternKind::Rectangular { dir1: Axis::X, count1: 3, spacing1: 15.0, dir2: Axis::Y, count2: 2, spacing2: 15.0 },
        });
        d.add_feature(FeatureKind::Pattern { bodies: vec![b], pattern: PatternKind::Circular { axis: Axis::Z, count: 4, total_deg: 360.0 } });
        d.add_feature(FeatureKind::Mirror { bodies: vec![b], plane: PlaneRef { base: BasePlane::YZ, offset: 0.0, flip: false, custom: None } });
        let m = Regenerator::default().regenerate(&d);
        assert!(m.errors.is_empty(), "{:?}", m.errors);
        assert_eq!(m.bodies.len(), 1 + 5 + 3 + 1);
        assert_eq!(d.bodies.len(), 10);
        for body in &m.bodies {
            assert!((body.mesh.volume() - 1000.0).abs() < 1.0, "{}", body.mesh.volume());
        }
        let mirrored = m.bodies.last().unwrap();
        assert!((mirrored.mesh.bbox_max.x + 15.0).abs() < 1e-3);
    }

    #[test]
    fn sweep_pipe_loft_split_scale() {
        use crate::{BasePlane, ExtrudeOp, SketchEntity};
        let mut d = Document::default();
        // Path in the XY plane: line along X then an arc turning to +Y.
        let path = d.add_feature(FeatureKind::Sketch {
            constraints: vec![],
            plane: PlaneRef::XY,
            entities: vec![
                SketchEntity::Arc { a: [30.0, 0.0], m: [30.0 + 14.142135623730951, 5.857864376269049], b: [50.0, 20.0] },
                SketchEntity::Line { a: [0.0, 0.0], b: [30.0, 0.0] },
            ],
        });
        d.add_feature(FeatureKind::Pipe { path_sketch: path, path: vec![0, 1], diameter: 6.0, wall: 1.0, op: ExtrudeOp::NewBody, participants: vec![] });
        // Profile on the YZ plane at the path start (square 4x4).
        let prof = d.add_feature(FeatureKind::Sketch {
            constraints: vec![],
            plane: PlaneRef::YZ,
            entities: crate::sketch::rectangle([-2.0, -2.0], [2.0, 2.0]),
        });
        d.add_feature(FeatureKind::Sweep { sketch: prof, profiles: vec![0], path_sketch: path, path: vec![1, 0], op: ExtrudeOp::NewBody, participants: vec![] });
        // Loft between two squares on parallel planes.
        let s1 = d.add_feature(FeatureKind::Sketch { constraints: vec![], plane: PlaneRef { base: BasePlane::XZ, offset: 40.0, flip: false, custom: None }, entities: crate::sketch::rectangle([-10.0, -10.0], [10.0, 10.0]) });
        let s2 = d.add_feature(FeatureKind::Sketch { constraints: vec![], plane: PlaneRef { base: BasePlane::XZ, offset: 60.0, flip: false, custom: None }, entities: crate::sketch::rectangle([-4.0, -4.0], [4.0, 4.0]) });
        let loft = d.add_feature(FeatureKind::Loft { sections: vec![(s1, 0), (s2, 0)], op: ExtrudeOp::NewBody, participants: vec![] });
        d.add_feature(FeatureKind::SplitBody { body: loft, plane: PlaneRef { base: BasePlane::XZ, offset: 50.0, flip: false, custom: None } });
        d.add_feature(FeatureKind::Scale { bodies: vec![loft], point: [0.0, 0.0, 0.0], factor: 0.5 });
        let m = Regenerator::default().regenerate(&d);
        assert!(m.errors.is_empty(), "{:?}", m.errors);
        assert_eq!(m.bodies.len(), 4);
        let pi = std::f64::consts::PI;
        let len = 30.0 + pi * 20.0 / 2.0;
        let pipe = pi * (9.0 - 4.0) * len;
        assert!((m.bodies[0].mesh.volume() - pipe).abs() / pipe < 0.02, "{} vs {pipe}", m.bodies[0].mesh.volume());
        let sweep = 16.0 * len;
        assert!((m.bodies[1].mesh.volume() - sweep).abs() / sweep < 0.02, "{}", m.bodies[1].mesh.volume());
    }

    #[test]
    fn joint_mates_faces() {
        use crate::{JointGeo, JointMotion};
        let mut d = Document::default();
        let a = d.add_feature(FeatureKind::Box { plane: PlaneRef::XZ, center: [0.0, 0.0], length: 20.0, width: 20.0, height: 10.0 });
        let b = d.add_feature(FeatureKind::Box { plane: PlaneRef::XZ, center: [50.0, 0.0], length: 10.0, width: 10.0, height: 10.0 });
        d.new_component(&[b]);
        // Mate b's bottom face (y=0, normal -Y) onto a's top face (y=10, normal +Y).
        d.add_feature(FeatureKind::Joint {
            moving: d.component_bodies(b),
            a: JointGeo { point: [0.0, 10.0, 0.0], normal: [0.0, 1.0, 0.0] },
            b: JointGeo { point: [50.0, 0.0, 0.0], normal: [0.0, -1.0, 0.0] },
            motion: JointMotion::Slider { offset: 2.0 },
        });
        let m = Regenerator::default().regenerate(&d);
        assert!(m.errors.is_empty(), "{:?}", m.errors);
        let mb = &m.body(b).unwrap().mesh;
        assert!((mb.bbox_min.y - 12.0).abs() < 1e-3, "{:?}", mb.bbox_min);
        assert!(mb.bbox_min.x.abs() - 5.0 < 1e-3);
        let _ = a;
    }

    #[test]
    fn user_parameters_drive_features() {
        use crate::expr::UserParam;
        let mut d = Document::default();
        d.params.push(UserParam { name: "w".into(), expr: "20".into(), comment: String::new() });
        let b = d.add_feature(FeatureKind::Box { plane: PlaneRef::XY, center: [0.0, 0.0], length: 1.0, width: 1.0, height: 1.0 });
        let f = d.feature_mut(b).unwrap();
        f.exprs.insert("Length".into(), "w".into());
        f.exprs.insert("Width".into(), "w / 2".into());
        f.exprs.insert("Height".into(), "w / 4 + 5 mm".into());
        let mut r = Regenerator::default();
        let m = r.regenerate(&d);
        assert!(m.errors.is_empty(), "{:?}", m.errors);
        assert!((m.bodies[0].mesh.volume() - 20.0 * 10.0 * 10.0).abs() < 1e-3);
        d.params[0].expr = "40".into();
        let m = r.regenerate(&d);
        assert!((m.bodies[0].mesh.volume() - 40.0 * 20.0 * 15.0).abs() < 1e-3, "{}", m.bodies[0].mesh.volume());
        d.params[0].name = "q".into();
        let m = r.regenerate(&d);
        assert!(m.errors.contains_key(&b));
    }

    #[test]
    fn ron_roundtrip() {
        let mut d = Document::new("Part");
        d.add_feature(boxf(0.0));
        d.add_feature(FeatureKind::Move {
            bodies: vec![1],
            translate: [1.0, 2.0, 3.0],
            axis: crate::Axis::Z,
            angle_deg: 45.0,
            copy: true,
        });
        let s = d.to_ron().unwrap();
        assert_eq!(Document::from_ron(&s).unwrap(), d);
    }
}
