//! Timeline features.

use glam::{DMat4, DVec3};
use m360_kernel::{ops::BooleanOp, Frame};
use serde::{Deserialize, Serialize};

use crate::model::BodyId;

/// One of the three origin planes, optionally offset along its normal.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlaneRef {
    pub base: BasePlane,
    pub offset: f64,
    /// Sketch on the back side of the plane (normal reversed).
    #[serde(default)]
    pub flip: bool,
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

    /// Plane through `point` with world-axis-aligned `normal`, if any.
    pub fn from_point_normal(point: DVec3, normal: DVec3) -> Option<PlaneRef> {
        let n = normal.normalize_or_zero();
        let (base, axis) = if n.z.abs() > 0.999 {
            (BasePlane::XY, DVec3::Z)
        } else if n.y.abs() > 0.999 {
            (BasePlane::XZ, DVec3::Y)
        } else if n.x.abs() > 0.999 {
            (BasePlane::YZ, DVec3::X)
        } else {
            return None;
        };
        let flip = n.dot(axis) < 0.0;
        Some(PlaneRef { base, offset: point.dot(axis), flip })
    }

    pub const XY: PlaneRef = PlaneRef { base: BasePlane::XY, offset: 0.0, flip: false };
    pub const XZ: PlaneRef = PlaneRef { base: BasePlane::XZ, offset: 0.0, flip: false };
    pub const YZ: PlaneRef = PlaneRef { base: BasePlane::YZ, offset: 0.0, flip: false };

    /// Placement frame with the given in-plane center.
    pub fn frame(&self, center: [f64; 2]) -> Frame {
        let (u, v) = match self.base {
            BasePlane::XY => (DVec3::X, DVec3::Y),
            // Fusion's XZ ground plane: normal +Y.
            BasePlane::XZ => (DVec3::X, DVec3::NEG_Z),
            BasePlane::YZ => (DVec3::Y, DVec3::Z),
        };
        let n = u.cross(v);
        let v = if self.flip { -v } else { v };
        Frame::new(n * self.offset + u * center[0] + v * center[1], u, v)
    }

    pub fn label(&self) -> String {
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
    },
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
            FeatureKind::Sphere { .. } => "Sphere",
            FeatureKind::Torus { .. } => "Torus",
            FeatureKind::Combine { .. } => "Combine",
            FeatureKind::Move { copy: true, .. } => "Copy",
            FeatureKind::Move { .. } => "Move",
            FeatureKind::Remove { .. } => "Remove",
            FeatureKind::Sketch { .. } => "Sketch",
            FeatureKind::Extrude { .. } => "Extrude",
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
                | FeatureKind::Extrude { op: ExtrudeOp::NewBody, .. }
        ) || matches!(self, FeatureKind::Extrude { participants, op: ExtrudeOp::Join, .. } if participants.is_empty())
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
}
