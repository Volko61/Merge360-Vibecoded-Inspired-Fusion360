//! Timeline features.

use glam::{DMat4, DVec3};
use m360_kernel::{ops::BooleanOp, Frame};
use serde::{Deserialize, Serialize};

use crate::model::BodyId;

/// One of the three origin planes (or an arbitrary plane), optionally
/// offset along its normal.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlaneRef {
    pub base: BasePlane,
    pub offset: f64,
    /// Sketch on the back side of the plane (normal reversed).
    #[serde(default)]
    pub flip: bool,
    /// Arbitrary plane (replaces `base` when set).
    #[serde(default)]
    pub custom: Option<CustomFrame>,
}

/// An arbitrary plane: origin and orthonormal in-plane axes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CustomFrame {
    pub origin: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BasePlane {
    XY,
    XZ,
    YZ,
}

impl PlaneRef {
    pub fn normal(&self) -> DVec3 {
        self.frame([0.0, 0.0]).n
    }

    /// Plane through `point` with the given `normal`. Axis-aligned normals
    /// give origin-plane references, others an arbitrary plane.
    pub fn from_point_normal(point: DVec3, normal: DVec3) -> Option<PlaneRef> {
        let n = normal.normalize_or_zero();
        if n == DVec3::ZERO {
            return None;
        }
        let (base, axis) = if n.z.abs() > 0.999 {
            (BasePlane::XY, DVec3::Z)
        } else if n.y.abs() > 0.999 {
            (BasePlane::XZ, DVec3::Y)
        } else if n.x.abs() > 0.999 {
            (BasePlane::YZ, DVec3::X)
        } else {
            return Some(Self::arbitrary(point, n));
        };
        let flip = n.dot(axis) < 0.0;
        // Mesh hits are f32: snap the offset to 1e-4 mm to remove noise.
        let offset = (point.dot(axis) * 1e4).round() / 1e4;
        Some(PlaneRef { base, offset, flip, custom: None })
    }

    /// Arbitrary plane through `point` with unit normal `n`. The in-plane U
    /// axis is horizontal (perpendicular to the world up axis +Y) when
    /// possible; the origin is the plane point closest to the world origin.
    pub fn arbitrary(point: DVec3, n: DVec3) -> PlaneRef {
        let n = n.normalize();
        let u = if n.y.abs() < 0.999 { DVec3::Y.cross(n).normalize() } else { DVec3::X };
        let v = n.cross(u);
        let d = (point.dot(n) * 1e4).round() / 1e4;
        let origin = n * d;
        PlaneRef { base: BasePlane::XY, offset: 0.0, flip: false, custom: Some(CustomFrame { origin: origin.into(), u: u.into(), v: v.into() }) }
    }

    /// Arbitrary plane with the given origin and in-plane axes.
    pub fn from_frame(origin: DVec3, u: DVec3, v: DVec3) -> PlaneRef {
        let n = u.cross(v).normalize();
        let u = u.normalize();
        let v = n.cross(u);
        PlaneRef { base: BasePlane::XY, offset: 0.0, flip: false, custom: Some(CustomFrame { origin: origin.into(), u: u.into(), v: v.into() }) }
    }

    /// Whether two references describe the same oriented plane.
    pub fn same_plane(&self, other: &PlaneRef) -> bool {
        let (a, b) = (self.frame([0.0, 0.0]), other.frame([0.0, 0.0]));
        a.n.dot(b.n) > 1.0 - 1e-6 && (b.origin - a.origin).dot(a.n).abs() < 1e-3
    }

    pub const XY: PlaneRef = PlaneRef { base: BasePlane::XY, offset: 0.0, flip: false, custom: None };
    pub const XZ: PlaneRef = PlaneRef { base: BasePlane::XZ, offset: 0.0, flip: false, custom: None };
    pub const YZ: PlaneRef = PlaneRef { base: BasePlane::YZ, offset: 0.0, flip: false, custom: None };

    /// Origin plane reference.
    pub fn base(base: BasePlane) -> PlaneRef {
        PlaneRef { base, offset: 0.0, flip: false, custom: None }
    }

    /// Placement frame with the given in-plane center.
    pub fn frame(&self, center: [f64; 2]) -> Frame {
        let (o, u, v) = match &self.custom {
            Some(c) => (DVec3::from(c.origin), DVec3::from(c.u), DVec3::from(c.v)),
            None => match self.base {
                BasePlane::XY => (DVec3::ZERO, DVec3::X, DVec3::Y),
                // Fusion's XZ ground plane: normal +Y.
                BasePlane::XZ => (DVec3::ZERO, DVec3::X, DVec3::NEG_Z),
                BasePlane::YZ => (DVec3::ZERO, DVec3::Y, DVec3::Z),
            },
        };
        let n = u.cross(v);
        let v = if self.flip { -v } else { v };
        Frame::new(o + n * self.offset + u * center[0] + v * center[1], u, v)
    }

    pub fn label(&self) -> String {
        if self.custom.is_some() {
            return if self.offset == 0.0 { "Plane".to_string() } else { format!("Plane ({:+.2})", self.offset) };
        }
        let name = match self.base {
            BasePlane::XY => "XY Plane",
            BasePlane::XZ => "XZ Plane",
            BasePlane::YZ => "YZ Plane",
        };
        if self.offset == 0.0 && !self.flip {
            name.to_string()
        } else {
            format!("{name} ({:+.2})", self.offset)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn vec(self) -> DVec3 {
        match self {
            Axis::X => DVec3::X,
            Axis::Y => DVec3::Y,
            Axis::Z => DVec3::Z,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "BooleanOp")]
pub enum BooleanOpDef {
    Join,
    Cut,
    Intersect,
}

/// A 2D sketch entity in plane coordinates (mm).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SketchEntity {
    Line { a: [f64; 2], b: [f64; 2] },
    Circle { c: [f64; 2], r: f64 },
    Point { p: [f64; 2] },
    /// Three-point arc from `a` through `m` to `b`.
    Arc { a: [f64; 2], m: [f64; 2], b: [f64; 2] },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FeatureKind {
    Box {
        plane: PlaneRef,
        center: [f64; 2],
        length: f64,
        width: f64,
        height: f64,
    },
    Cylinder {
        plane: PlaneRef,
        center: [f64; 2],
        diameter: f64,
        height: f64,
    },
    Sphere {
        plane: PlaneRef,
        center: [f64; 2],
        diameter: f64,
    },
    Torus {
        plane: PlaneRef,
        center: [f64; 2],
        inner_diameter: f64,
        torus_diameter: f64,
    },
    /// Cosmetic thread on a cylindrical face (axis from `origin` along
    /// `axis`; `length` 0 = full face length).
    Thread {
        body: BodyId,
        origin: [f64; 3],
        axis: [f64; 3],
        radius: f64,
        face_length: f64,
        length: f64,
        pitch: f64,
        internal: bool,
        designation: String,
    },
    /// Helical coil around the plane normal through `center`.
    Coil {
        plane: PlaneRef,
        center: [f64; 2],
        diameter: f64,
        turns: f64,
        height: f64,
        taper: f64,
        section: CoilSectionType,
        size: f64,
        position: CoilPosition,
        clockwise: bool,
    },
    Combine {
        target: BodyId,
        tools: Vec<BodyId>,
        #[serde(with = "BooleanOpDef")]
        op: BooleanOp,
        keep_tools: bool,
    },
    Move {
        bodies: Vec<BodyId>,
        translate: [f64; 3],
        axis: Axis,
        angle_deg: f64,
        copy: bool,
    },
    Remove {
        bodies: Vec<BodyId>,
    },
    Sketch {
        plane: PlaneRef,
        entities: Vec<SketchEntity>,
        #[serde(default)]
        constraints: Vec<crate::solver::SketchConstraint>,
    },
    Revolve {
        sketch: u64,
        profiles: Vec<usize>,
        axis: RevolveAxis,
        angle_deg: f64,
        op: ExtrudeOp,
        participants: Vec<BodyId>,
    },
    /// Holes drilled into a face plane at one or more points.
    Hole {
        /// Plane of the placement face; its normal points out of the material.
        plane: PlaneRef,
        points: Vec<[f64; 2]>,
        diameter: f64,
        depth: f64,
        through_all: bool,
        kind: HoleType,
        participants: Vec<BodyId>,
    },
    /// Sweep sketch profiles along a path of sketch lines/arcs.
    Sweep {
        sketch: u64,
        profiles: Vec<usize>,
        path_sketch: u64,
        path: Vec<usize>,
        op: ExtrudeOp,
        participants: Vec<BodyId>,
    },
    /// Circular pipe along a path (hollow when `wall` > 0).
    Pipe {
        path_sketch: u64,
        path: Vec<usize>,
        diameter: f64,
        wall: f64,
        op: ExtrudeOp,
        participants: Vec<BodyId>,
    },
    /// Loft through profiles (sketch uid, profile index), in order.
    Loft {
        sections: Vec<(u64, usize)>,
        op: ExtrudeOp,
        participants: Vec<BodyId>,
    },
    /// Hollow out a (prismatic) body; faces containing `open_points` are removed.
    Shell {
        body: BodyId,
        open_points: Vec<[f64; 3]>,
        thickness: f64,
    },
    /// Split a body with a plane (the far side becomes a new body).
    SplitBody {
        body: BodyId,
        plane: PlaneRef,
    },
    /// Uniform scale of bodies about a point.
    Scale {
        bodies: Vec<BodyId>,
        point: [f64; 3],
        factor: f64,
    },
    /// Joint: moves `moving` bodies so that face `b` mates with face `a`.
    Joint {
        moving: Vec<BodyId>,
        a: JointGeo,
        b: JointGeo,
        motion: JointMotion,
    },
    /// An imported triangle mesh (Insert ▸ Insert Mesh), as a polyhedral body.
    ImportMesh {
        name: String,
        triangles: Vec<[[f32; 3]; 3]>,
    },
    /// A construction plane (Construct ▸ Offset Plane).
    ConstructionPlane {
        plane: PlaneRef,
    },
    /// Copies of bodies along a rectangular grid or around an axis.
    Pattern {
        bodies: Vec<BodyId>,
        pattern: PatternKind,
    },
    /// Mirrored copies of bodies across a plane.
    Mirror {
        bodies: Vec<BodyId>,
        plane: PlaneRef,
    },
    /// Fillet (or chamfer) straight edges of a body. Edges are stored by
    /// their end points and re-resolved on every regeneration.
    Fillet {
        body: BodyId,
        edges: Vec<[[f64; 3]; 2]>,
        size: f64,
        chamfer: bool,
    },
    Extrude {
        sketch: u64,
        profiles: Vec<usize>,
        distance: f64,
        symmetric: bool,
        op: ExtrudeOp,
        /// Bodies a Join/Cut/Intersect acts on.
        participants: Vec<BodyId>,
        /// Side wall angle in degrees (positive = outwards).
        #[serde(default)]
        taper: f64,
    },
}

/// Joint origin: a point on a face and the face's outward normal.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct JointGeo {
    pub point: [f64; 3],
    pub normal: [f64; 3],
}

/// Joint type with its driven value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum JointMotion {
    Rigid,
    /// Rotation about the joint axis (degrees).
    Revolute { angle_deg: f64 },
    /// Translation along the joint axis.
    Slider { offset: f64 },
}

impl JointMotion {
    pub fn label(&self) -> &'static str {
        match self {
            JointMotion::Rigid => "Rigid",
            JointMotion::Revolute { .. } => "Revolute",
            JointMotion::Slider { .. } => "Slider",
        }
    }
}

impl JointGeo {
    /// Transform that brings `b` onto `a` (normals opposed), then applies the motion.
    pub fn mate(a: &JointGeo, b: &JointGeo, motion: &JointMotion) -> DMat4 {
        let (pa, na) = (DVec3::from(a.point), DVec3::from(a.normal).normalize());
        let (pb, nb) = (DVec3::from(b.point), DVec3::from(b.normal).normalize());
        let rot = glam::DQuat::from_rotation_arc(nb, -na);
        let mut m = DMat4::from_translation(pa) * DMat4::from_quat(rot) * DMat4::from_translation(-pb);
        match *motion {
            JointMotion::Rigid => {}
            JointMotion::Revolute { angle_deg } => {
                m = DMat4::from_translation(pa) * DMat4::from_axis_angle(na, angle_deg.to_radians()) * DMat4::from_translation(-pa) * m;
            }
            JointMotion::Slider { offset } => m = DMat4::from_translation(na * offset) * m,
        }
        m
    }
}

/// Layout of a Pattern feature.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PatternKind {
    Rectangular { dir1: Axis, count1: u32, spacing1: f64, dir2: Axis, count2: u32, spacing2: f64 },
    /// Around an origin axis; `total_deg` = 360 spreads instances evenly.
    Circular { axis: Axis, count: u32, total_deg: f64 },
}

impl PatternKind {
    /// Transforms of the copies (identity excluded).
    pub fn transforms(&self) -> Vec<DMat4> {
        match *self {
            PatternKind::Rectangular { dir1, count1, spacing1, dir2, count2, spacing2 } => {
                let mut out = Vec::new();
                for i in 0..count1.max(1) {
                    for j in 0..count2.max(1) {
                        if i == 0 && j == 0 {
                            continue;
                        }
                        let t = dir1.vec() * spacing1 * i as f64 + dir2.vec() * spacing2 * j as f64;
                        out.push(DMat4::from_translation(t));
                    }
                }
                out
            }
            PatternKind::Circular { axis, count, total_deg } => {
                let n = count.max(1);
                let full = (total_deg.abs() - 360.0).abs() < 1e-6;
                let step = if full || n == 1 { total_deg / n as f64 } else { total_deg / (n - 1) as f64 };
                (1..n).map(|k| DMat4::from_axis_angle(axis.vec(), (step * k as f64).to_radians())).collect()
            }
        }
    }
}

/// Hole type (Fusion's Simple / Counterbore / Countersink).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum HoleType {
    Simple,
    Counterbore { diameter: f64, depth: f64 },
    Countersink { diameter: f64, angle_deg: f64 },
}

impl HoleType {
    pub fn label(&self) -> &'static str {
        match self {
            HoleType::Simple => "Simple",
            HoleType::Counterbore { .. } => "Counterbore",
            HoleType::Countersink { .. } => "Countersink",
        }
    }
}

/// Coil section shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoilSectionType {
    Circular,
    Square,
    TriangularExternal,
    TriangularInternal,
}

impl CoilSectionType {
    pub const ALL: [CoilSectionType; 4] = [Self::Circular, Self::Square, Self::TriangularExternal, Self::TriangularInternal];
    pub fn label(&self) -> &'static str {
        match self {
            Self::Circular => "Circular",
            Self::Square => "Square",
            Self::TriangularExternal => "Triangular (External)",
            Self::TriangularInternal => "Triangular (Internal)",
        }
    }
}

/// Position of the coil section relative to the coil diameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoilPosition {
    Inside,
    Center,
    Outside,
}

impl CoilPosition {
    pub const ALL: [CoilPosition; 3] = [Self::Inside, Self::Center, Self::Outside];
    pub fn label(&self) -> &'static str {
        match self {
            Self::Inside => "Inside",
            Self::Center => "On Center",
            Self::Outside => "Outside",
        }
    }
}

/// Axis of a Revolve feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevolveAxis {
    X,
    Y,
    Z,
    /// A line entity of the profile's sketch.
    SketchLine(usize),
}

impl RevolveAxis {
    pub fn label(&self) -> String {
        match self {
            RevolveAxis::X => "X Axis".into(),
            RevolveAxis::Y => "Y Axis".into(),
            RevolveAxis::Z => "Z Axis".into(),
            RevolveAxis::SketchLine(i) => format!("Sketch line {}", i + 1),
        }
    }
}

/// Operation of a sketch-based feature (Fusion's "Operation" dropdown).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtrudeOp {
    NewBody,
    Join,
    Cut,
    Intersect,
}

impl ExtrudeOp {
    pub const ALL: [ExtrudeOp; 4] = [ExtrudeOp::Join, ExtrudeOp::Cut, ExtrudeOp::Intersect, ExtrudeOp::NewBody];

    pub fn label(self) -> &'static str {
        match self {
            ExtrudeOp::NewBody => "New Body",
            ExtrudeOp::Join => "Join",
            ExtrudeOp::Cut => "Cut",
            ExtrudeOp::Intersect => "Intersect",
        }
    }
}

impl FeatureKind {
    /// Default display name prefix (Fusion names features "Box1", "Combine2", …).
    pub fn base_name(&self) -> &'static str {
        match self {
            FeatureKind::Box { .. } => "Box",
            FeatureKind::Cylinder { .. } => "Cylinder",
            FeatureKind::Coil { .. } => "Coil",
            FeatureKind::Sphere { .. } => "Sphere",
            FeatureKind::Torus { .. } => "Torus",
            FeatureKind::Combine { .. } => "Combine",
            FeatureKind::Move { copy: true, .. } => "Copy",
            FeatureKind::Move { .. } => "Move",
            FeatureKind::Remove { .. } => "Remove",
            FeatureKind::Sketch { .. } => "Sketch",
            FeatureKind::Extrude { .. } => "Extrude",
            FeatureKind::Revolve { .. } => "Revolve",
            FeatureKind::Hole { .. } => "Hole",
            FeatureKind::Pattern { pattern: PatternKind::Circular { .. }, .. } => "CircularPattern",
            FeatureKind::Pattern { .. } => "RectangularPattern",
            FeatureKind::Mirror { .. } => "Mirror",
            FeatureKind::ConstructionPlane { .. } => "Plane",
            FeatureKind::Joint { .. } => "Joint",
            FeatureKind::ImportMesh { .. } => "Mesh",
            FeatureKind::Sweep { .. } => "Sweep",
            FeatureKind::Pipe { .. } => "Pipe",
            FeatureKind::Loft { .. } => "Loft",
            FeatureKind::SplitBody { .. } => "SplitBody",
            FeatureKind::Scale { .. } => "Scale",
            FeatureKind::Shell { .. } => "Shell",
            FeatureKind::Thread { .. } => "Thread",
            FeatureKind::Fillet { chamfer: true, .. } => "Chamfer",
            FeatureKind::Fillet { .. } => "Fillet",
        }
    }

    pub fn creates_body(&self) -> bool {
        matches!(
            self,
            FeatureKind::Box { .. }
                | FeatureKind::Cylinder { .. }
                | FeatureKind::Sphere { .. }
                | FeatureKind::Torus { .. }
                | FeatureKind::Coil { .. }
                | FeatureKind::Extrude { op: ExtrudeOp::NewBody, .. }
                | FeatureKind::Revolve { op: ExtrudeOp::NewBody, .. }
                | FeatureKind::Sweep { op: ExtrudeOp::NewBody, .. }
                | FeatureKind::Pipe { op: ExtrudeOp::NewBody, .. }
                | FeatureKind::Loft { op: ExtrudeOp::NewBody, .. }
                | FeatureKind::SplitBody { .. }
                | FeatureKind::ImportMesh { .. }
        ) || matches!(self,
            FeatureKind::Extrude { participants, op: ExtrudeOp::Join, .. }
            | FeatureKind::Revolve { participants, op: ExtrudeOp::Join, .. }
            | FeatureKind::Sweep { participants, op: ExtrudeOp::Join, .. }
            | FeatureKind::Pipe { participants, op: ExtrudeOp::Join, .. }
            | FeatureKind::Loft { participants, op: ExtrudeOp::Join, .. }
            if participants.is_empty())
    }

    /// Named numeric parameters of the feature: (name, is_angle, value).
    pub fn numeric_fields_mut(&mut self) -> Vec<(&'static str, bool, &mut f64)> {
        use FeatureKind::*;
        match self {
            Box { length, width, height, .. } => vec![("Length", false, length), ("Width", false, width), ("Height", false, height)],
            Cylinder { diameter, height, .. } => vec![("Diameter", false, diameter), ("Height", false, height)],
            Sphere { diameter, .. } => vec![("Diameter", false, diameter)],
            Torus { inner_diameter, torus_diameter, .. } => vec![("Inner Diameter", false, inner_diameter), ("Torus Diameter", false, torus_diameter)],
            Coil { diameter, turns, height, taper, size, .. } => vec![
                ("Diameter", false, diameter),
                ("Revolutions", true, turns),
                ("Height", false, height),
                ("Angle", true, taper),
                ("Section Size", false, size),
            ],
            Extrude { distance, taper, .. } => vec![("Distance", false, distance), ("Taper Angle", true, taper)],
            Revolve { angle_deg, .. } => vec![("Angle", true, angle_deg)],
            Hole { diameter, depth, .. } => vec![("Diameter", false, diameter), ("Depth", false, depth)],
            Pipe { diameter, wall, .. } => vec![("Section Size", false, diameter), ("Thickness", false, wall)],
            Fillet { size, chamfer, .. } => vec![(if *chamfer { "Distance" } else { "Radius" }, false, size)],
            Shell { thickness, .. } => vec![("Thickness", false, thickness)],
            Thread { pitch, length, .. } => vec![("Pitch", false, pitch), ("Length", false, length)],
            Move { translate, angle_deg, .. } => {
                let [x, y, z] = translate;
                vec![("X Distance", false, x), ("Y Distance", false, y), ("Z Distance", false, z), ("Angle", true, angle_deg)]
            }
            Scale { factor, .. } => vec![("Scale Factor", true, factor)],
            ConstructionPlane { plane } => vec![("Offset", false, &mut plane.offset)],
            Pattern { pattern: PatternKind::Rectangular { spacing1, spacing2, .. }, .. } => vec![("Spacing 1", false, spacing1), ("Spacing 2", false, spacing2)],
            Pattern { pattern: PatternKind::Circular { total_deg, .. }, .. } => vec![("Total Angle", true, total_deg)],
            Joint { motion: JointMotion::Revolute { angle_deg }, .. } => vec![("Angle", true, angle_deg)],
            Joint { motion: JointMotion::Slider { offset }, .. } => vec![("Distance", false, offset)],
            _ => vec![],
        }
    }

    /// For features that create copies of bodies: the source bodies and the
    /// transform of each copy. Copy `k` of body `i` gets id
    /// `uid << 16 | (i * transforms + k)`.
    pub fn copy_transforms(&self) -> Option<(&[BodyId], Vec<DMat4>)> {
        match self {
            FeatureKind::Move { bodies, translate, axis, angle_deg, copy: true } => {
                Some((bodies, vec![Self::move_matrix(*translate, *axis, *angle_deg)]))
            }
            FeatureKind::Pattern { bodies, pattern } => Some((bodies, pattern.transforms())),
            FeatureKind::Mirror { bodies, plane } => {
                let n = plane.normal();
                let d = plane.frame([0.0, 0.0]).origin.dot(n);
                // Reflection x' = x - 2 (n.x - d) n
                let r = glam::DMat3::IDENTITY - 2.0 * glam::DMat3::from_cols(n * n.x, n * n.y, n * n.z);
                let mut m = DMat4::from_mat3(r);
                m.w_axis = (n * 2.0 * d).extend(1.0);
                Some((bodies, vec![m]))
            }
            _ => None,
        }
    }

    pub fn move_matrix(translate: [f64; 3], axis: Axis, angle_deg: f64) -> DMat4 {
        DMat4::from_translation(DVec3::from(translate))
            * DMat4::from_axis_angle(axis.vec(), angle_deg.to_radians())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub uid: u64,
    pub name: String,
    pub suppressed: bool,
    pub kind: FeatureKind,
    /// Parameter expressions driving numeric fields (field name -> expression).
    #[serde(default)]
    pub exprs: std::collections::BTreeMap<String, String>,
}

impl Feature {
    /// Copy with expression-driven fields evaluated.
    pub fn evaluated(&self, units: crate::Units, vars: &std::collections::HashMap<String, f64>) -> Result<Feature, String> {
        if self.exprs.is_empty() {
            return Ok(self.clone());
        }
        let mut f = self.clone();
        for (name, angle, value) in f.kind.numeric_fields_mut() {
            if let Some(e) = self.exprs.get(name) {
                let u = if angle { crate::Units::Millimeter } else { units };
                *value = crate::expr::eval(e, u, vars).map_err(|err| format!("{name} = {e}: {err}"))?;
            }
        }
        Ok(f)
    }
}
