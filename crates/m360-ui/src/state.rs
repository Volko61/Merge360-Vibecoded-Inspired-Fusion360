//! Per-document UI state (a design tab).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use glam::DVec3;

use m360_core::{
    model::build_primitive, BasePlane, BodyId, BooleanOp, Document, ExtrudeOp, FeatureKind, History, Model,
    PlaneRef, Regenerator, SketchEntity,
};
use m360_kernel::{profile, profile::Region, BodyMesh};
use m360_render::Camera;

/// Something that can be hovered or selected.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SelItem {
    Body(BodyId),
    Face(BodyId, u32),
    Plane(BasePlane),
    Sketch(u64),
    Profile(u64, usize),
    SketchEntity(u64, usize),
    Feature(u64),
    /// A planar face usable as a sketch plane.
    FacePlane(BodyId, u32, PlaneRef),
    /// A B-rep edge (index into the body mesh's edges).
    Edge(BodyId, u32),
    /// A construction plane feature.
    ConstructionPlane(u64),
    /// A point of a sketch entity.
    SketchPoint(u64, m360_core::solver::PointRef),
    /// A sketch constraint or dimension (by index).
    SketchConstraint(u64, usize),
}

impl SelItem {
    pub fn body(&self) -> Option<BodyId> {
        match self {
            SelItem::Body(b) | SelItem::Face(b, _) | SelItem::FacePlane(b, _, _) | SelItem::Edge(b, _) => Some(*b),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualStyle {
    ShadedVisibleEdges,
    Shaded,
    ShadedHiddenEdges,
    Wireframe,
}

impl VisualStyle {
    pub const ALL: [VisualStyle; 4] = [
        VisualStyle::ShadedVisibleEdges,
        VisualStyle::Shaded,
        VisualStyle::ShadedHiddenEdges,
        VisualStyle::Wireframe,
    ];
    pub fn label(self) -> &'static str {
        match self {
            VisualStyle::ShadedVisibleEdges => "Shaded with Visible Edges Only",
            VisualStyle::Shaded => "Shaded",
            VisualStyle::ShadedHiddenEdges => "Shaded with Hidden Edges",
            VisualStyle::Wireframe => "Wireframe",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    PhotoBooth,
    GreyRoom,
    DarkSky,
    White,
}

impl Environment {
    pub const ALL: [Environment; 4] =
        [Environment::PhotoBooth, Environment::GreyRoom, Environment::DarkSky, Environment::White];
    pub fn label(self) -> &'static str {
        match self {
            Environment::PhotoBooth => "Photo Booth",
            Environment::GreyRoom => "Grey Room",
            Environment::DarkSky => "Dark Sky",
            Environment::White => "Plain White",
        }
    }
    /// (top, bottom) sRGB gradient.
    pub fn gradient(self) -> ([f32; 3], [f32; 3]) {
        match self {
            Environment::PhotoBooth => ([0.98, 0.98, 0.99], [0.78, 0.80, 0.84]),
            Environment::GreyRoom => ([0.75, 0.76, 0.78], [0.52, 0.53, 0.56]),
            Environment::DarkSky => ([0.20, 0.23, 0.30], [0.05, 0.06, 0.09]),
            Environment::White => ([1.0, 1.0, 1.0], [1.0, 1.0, 1.0]),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Display {
    pub style: VisualStyle,
    pub environment: Environment,
    pub grid: bool,
    pub snap: bool,
    pub ground_shadow: bool,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            style: VisualStyle::ShadedVisibleEdges,
            environment: Environment::PhotoBooth,
            grid: true,
            snap: true,
            ground_shadow: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavMode {
    Select,
    Orbit,
    Pan,
    Zoom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchTool {
    Select,
    Line,
    Rect,
    CenterRect,
    Circle,
    Point,
    Arc,
    Polygon,
    Dimension,
    Constrain(ConKind),
    Trim,
    Offset,
    Fillet,
    Text,
    Slot,
    Spline,
    Ellipse,
    Project,
    Extend,
    Break,
    Mirror,
    CircPattern,
    RectPattern,
    Move,
    Scale,
}

/// Geometric constraint tools of the SKETCH tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConKind {
    HorizontalVertical,
    Coincident,
    Tangent,
    Equal,
    Parallel,
    Perpendicular,
    Fix,
    Concentric,
    Midpoint,
    Collinear,
    Symmetry,
}

impl ConKind {
    pub fn label(self) -> &'static str {
        match self {
            ConKind::HorizontalVertical => "Horizontal/Vertical",
            ConKind::Coincident => "Coincident",
            ConKind::Tangent => "Tangent",
            ConKind::Equal => "Equal",
            ConKind::Parallel => "Parallel",
            ConKind::Perpendicular => "Perpendicular",
            ConKind::Fix => "Fix",
            ConKind::Concentric => "Concentric",
            ConKind::Midpoint => "Midpoint",
            ConKind::Collinear => "Collinear",
            ConKind::Symmetry => "Symmetry",
        }
    }
    /// Number of picks needed.
    pub fn picks(self) -> usize {
        match self {
            ConKind::HorizontalVertical | ConKind::Fix => 1,
            ConKind::Symmetry => 3,
            _ => 2,
        }
    }
    /// Whether the tool picks points (else entities).
    pub fn wants_points(self) -> bool {
        matches!(self, ConKind::Coincident | ConKind::Fix)
    }
    /// Number of point picks of tools that pick points and then a line.
    pub fn mixed_points(self) -> Option<usize> {
        match self {
            ConKind::Midpoint => Some(1),
            ConKind::Symmetry => Some(2),
            _ => None,
        }
    }
}

/// What the inline value editor applies to.
#[derive(Debug, Clone, PartialEq)]
pub enum EditPurpose {
    Dimension,
    /// Offset these entities.
    Offset(Vec<usize>),
    /// Round the corner between two lines.
    Fillet(usize, usize),
    /// Place text at a point (the editor holds the text; height in mm).
    Text([f64; 2], f64),
    /// Circular pattern of entities around a center (text: count).
    CircPattern(Vec<usize>, [f64; 2]),
    /// Rectangular pattern along a spacing vector (text: "N" or "N x M").
    RectPattern(Vec<usize>, [f64; 2]),
    /// Scale entities about a base point (text: factor).
    Scale(Vec<usize>, [f64; 2]),
}

impl EditPurpose {
    /// The editor holds free text rather than a length.
    pub fn raw_text(&self) -> bool {
        matches!(self, EditPurpose::Text(..) | EditPurpose::CircPattern(..) | EditPurpose::RectPattern(..) | EditPurpose::Scale(..))
    }
}

/// An open inline dimension editor in sketch mode.
#[derive(Debug, Clone)]
pub struct DimEdit {
    pub purpose: EditPurpose,
    /// Existing constraint being edited, or the draft of a new one.
    pub index: Option<usize>,
    pub draft: m360_core::solver::SketchConstraint,
    pub text: String,
    pub pos: eframe::egui::Pos2,
    pub focus: u8,
}

impl SketchTool {
    pub fn label(self) -> &'static str {
        match self {
            SketchTool::Select => "Select",
            SketchTool::Line => "Line",
            SketchTool::Rect => "2-Point Rectangle",
            SketchTool::CenterRect => "Center Rectangle",
            SketchTool::Circle => "Center Diameter Circle",
            SketchTool::Point => "Point",
            SketchTool::Arc => "3-Point Arc",
            SketchTool::Polygon => "Inscribed Polygon",
            SketchTool::Dimension => "Sketch Dimension",
            SketchTool::Trim => "Trim",
            SketchTool::Offset => "Offset",
            SketchTool::Fillet => "Fillet",
            SketchTool::Text => "Text",
            SketchTool::Slot => "Center to Center Slot",
            SketchTool::Spline => "Fit Point Spline",
            SketchTool::Ellipse => "Ellipse",
            SketchTool::Project => "Project",
            SketchTool::Extend => "Extend",
            SketchTool::Break => "Break",
            SketchTool::Mirror => "Mirror",
            SketchTool::CircPattern => "Circular Pattern",
            SketchTool::RectPattern => "Rectangular Pattern",
            SketchTool::Move => "Move/Copy",
            SketchTool::Scale => "Sketch Scale",
            SketchTool::Constrain(k) => k.label(),
        }
    }

    /// Tools that transform the sketch geometry selected beforehand.
    pub fn acts_on_selection(self) -> bool {
        matches!(self, SketchTool::Mirror | SketchTool::CircPattern | SketchTool::RectPattern | SketchTool::Move | SketchTool::Scale)
    }

    /// Tools that edit existing geometry (clicks pick, they do not draw).
    pub fn is_edit(self) -> bool {
        matches!(
            self,
            SketchTool::Trim | SketchTool::Offset | SketchTool::Fillet | SketchTool::Text | SketchTool::Project | SketchTool::Extend | SketchTool::Break
        ) || self.acts_on_selection()
    }

    /// Prompt shown next to the cursor.
    pub fn prompt(self, clicks: usize) -> Option<&'static str> {
        Some(match (self, clicks) {
            (SketchTool::Mirror, _) => "Select the mirror line",
            (SketchTool::Project, _) => "Select body edges or faces to project",
            (SketchTool::Extend, _) => "Click a line near the end to extend",
            (SketchTool::Break, _) => "Click a curve to split at its intersections",
            (SketchTool::CircPattern, _) => "Click the center point",
            (SketchTool::RectPattern, 0) => "Click the start of the spacing vector",
            (SketchTool::RectPattern, _) => "Click the end of the spacing vector",
            (SketchTool::Move, 0) => "Click the base point",
            (SketchTool::Move, _) => "Click the target point",
            (SketchTool::Scale, _) => "Click the base point",
            _ => return None,
        })
    }
}

/// An open sketch being edited (Fusion's sketch mode).
pub struct SketchSession {
    pub uid: u64,
    pub tool: SketchTool,
    /// Clicked points of the entity being drawn.
    pub pending: Vec<[f64; 2]>,
    pub before: Document,
    pub saved_camera: Camera,
    pub show_profile: bool,
    pub look_at: bool,
    /// Picks of the current constraint / dimension tool.
    pub picked: Vec<SelItem>,
    pub dim_edit: Option<DimEdit>,
    /// Point being dragged with the Select tool.
    pub drag: Option<m360_core::solver::PointRef>,
    /// Entities the Mirror / Pattern / Move / Scale tools act on.
    pub targets: Vec<usize>,
}

/// Context for editing an existing feature from the timeline.
#[derive(Clone)]
pub struct EditCtx {
    pub uid: u64,
    pub before: Document,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombineField {
    Target,
    Tools,
}

pub enum Command {
    Primitive { kind: FeatureKind, plane_chosen: bool, editing: Option<EditCtx> },
    Extrude {
        sketch: Option<u64>,
        profiles: Vec<usize>,
        distance: f64,
        symmetric: bool,
        taper: f64,
        op: ExtrudeOp,
        /// Planar body faces to extrude (with their mesh face index).
        faces: Vec<(m360_core::FaceRef, u32)>,
        editing: Option<EditCtx>,
    },
    Combine {
        target: Option<BodyId>,
        tools: Vec<BodyId>,
        op: BooleanOp,
        keep_tools: bool,
        field: CombineField,
        editing: Option<EditCtx>,
    },
    Move {
        bodies: Vec<BodyId>,
        translate: [f64; 3],
        axis: m360_core::Axis,
        angle_deg: f64,
        copy: bool,
        editing: Option<EditCtx>,
    },
    Revolve {
        sketch: Option<u64>,
        profiles: Vec<usize>,
        axis: Option<m360_core::RevolveAxis>,
        angle_deg: f64,
        op: ExtrudeOp,
        /// True while the Axis selection field is active.
        picking_axis: bool,
        editing: Option<EditCtx>,
    },
    Sweep {
        sketch: Option<u64>,
        profiles: Vec<usize>,
        path_sketch: Option<u64>,
        path: Vec<usize>,
        /// True while the Path field is active.
        picking_path: bool,
        op: ExtrudeOp,
        editing: Option<EditCtx>,
    },
    Pipe {
        path_sketch: Option<u64>,
        path: Vec<usize>,
        diameter: f64,
        wall: f64,
        hollow: bool,
        op: ExtrudeOp,
        editing: Option<EditCtx>,
    },
    Loft {
        sections: Vec<(u64, usize)>,
        op: ExtrudeOp,
        editing: Option<EditCtx>,
    },
    Shell {
        body: Option<BodyId>,
        /// Faces to remove (left open).
        faces: Vec<u32>,
        thickness: f64,
        editing: Option<EditCtx>,
    },
    SplitBody {
        body: Option<BodyId>,
        plane: Option<PlaneRef>,
        picking_plane: bool,
        editing: Option<EditCtx>,
    },
    Scale {
        bodies: Vec<BodyId>,
        point: [f64; 3],
        factor: f64,
        editing: Option<EditCtx>,
    },
    Joint {
        /// Moving side (picked first) and its bodies.
        b: Option<(Vec<BodyId>, m360_core::JointGeo)>,
        a: Option<m360_core::JointGeo>,
        motion: m360_core::JointMotion,
        editing: Option<EditCtx>,
    },
    OffsetPlane {
        base: Option<PlaneRef>,
        distance: f64,
        editing: Option<EditCtx>,
    },
    Thread {
        body: Option<BodyId>,
        cyl: Option<m360_kernel::Cylinder>,
        /// Index into [`ISO_THREADS`].
        size: usize,
        pitch: f64,
        full_length: bool,
        length: f64,
        editing: Option<EditCtx>,
    },
    Interference {
        bodies: Vec<BodyId>,
        /// Computed overlaps (None until Compute).
        results: Option<Vec<Interference>>,
    },
    /// Plane at Angle / Midplane / Plane Through Three Points.
    RefPlane {
        method: RefPlaneMethod,
        picks: Vec<RefPick>,
        angle: f64,
    },
    Section {
        plane: Option<PlaneRef>,
        offset: f64,
        flip: bool,
    },
    Pattern {
        bodies: Vec<BodyId>,
        pattern: m360_core::PatternKind,
        editing: Option<EditCtx>,
    },
    Mirror {
        bodies: Vec<BodyId>,
        plane: Option<PlaneRef>,
        /// True while the Mirror Plane field is active.
        picking_plane: bool,
        editing: Option<EditCtx>,
    },
    Hole {
        plane: Option<PlaneRef>,
        points: Vec<[f64; 2]>,
        diameter: f64,
        depth: f64,
        through_all: bool,
        kind: m360_core::HoleType,
        editing: Option<EditCtx>,
    },
    Fillet {
        body: Option<BodyId>,
        edges: Vec<u32>,
        size: f64,
        chamfer: bool,
        editing: Option<EditCtx>,
    },
    SketchPlane,
    Measure { picks: Vec<SelItem> },
    Appearance,
    Parameters,
    DocSettings,
    CenterOfMass,
    Properties,
}

impl Command {
    pub fn title(&self) -> &'static str {
        match self {
            Command::Primitive { kind, .. } => match kind {
                FeatureKind::Box { .. } => "BOX",
                FeatureKind::Cylinder { .. } => "CYLINDER",
                FeatureKind::Sphere { .. } => "SPHERE",
                FeatureKind::Coil { .. } => "COIL",
                _ => "TORUS",
            },
            Command::Extrude { .. } => "EXTRUDE",
            Command::Combine { .. } => "COMBINE",
            Command::Move { .. } => "MOVE/COPY",
            Command::Revolve { .. } => "REVOLVE",
            Command::Hole { .. } => "HOLE",
            Command::Pattern { pattern: m360_core::PatternKind::Circular { .. }, .. } => "CIRCULAR PATTERN",
            Command::Pattern { .. } => "RECTANGULAR PATTERN",
            Command::Mirror { .. } => "MIRROR",
            Command::OffsetPlane { .. } => "OFFSET PLANE",
            Command::RefPlane { method: RefPlaneMethod::Angle, .. } => "PLANE AT ANGLE",
            Command::RefPlane { method: RefPlaneMethod::Mid, .. } => "MIDPLANE",
            Command::RefPlane { method: RefPlaneMethod::ThreePoints, .. } => "PLANE THROUGH THREE POINTS",
            Command::Joint { .. } => "JOINT",
            Command::Sweep { .. } => "SWEEP",
            Command::Pipe { .. } => "PIPE",
            Command::Loft { .. } => "LOFT",
            Command::SplitBody { .. } => "SPLIT BODY",
            Command::Shell { .. } => "SHELL",
            Command::Thread { .. } => "THREAD",
            Command::Scale { .. } => "SCALE",
            Command::Section { .. } => "SECTION ANALYSIS",
            Command::Fillet { chamfer: true, .. } => "CHAMFER",
            Command::Fillet { .. } => "FILLET",
            Command::SketchPlane => "CREATE SKETCH",
            Command::Measure { .. } => "MEASURE",
            Command::Appearance => "APPEARANCE",
            Command::Parameters => "PARAMETERS",
            Command::DocSettings => "DOCUMENT SETTINGS",
            Command::CenterOfMass => "CENTER OF MASS",
            Command::Interference { .. } => "INTERFERENCE",
            Command::Properties => "PROPERTIES",
        }
    }

    pub fn editing(&self) -> Option<&EditCtx> {
        match self {
            Command::Primitive { editing, .. }
            | Command::Extrude { editing, .. }
            | Command::Combine { editing, .. }
            | Command::Move { editing, .. }
            | Command::Fillet { editing, .. }
            | Command::Revolve { editing, .. }
            | Command::Hole { editing, .. }
            | Command::Pattern { editing, .. }
            | Command::Mirror { editing, .. }
            | Command::OffsetPlane { editing, .. }
            | Command::Sweep { editing, .. }
            | Command::Pipe { editing, .. }
            | Command::Loft { editing, .. }
            | Command::SplitBody { editing, .. }
            | Command::Scale { editing, .. }
            | Command::Shell { editing, .. }
            | Command::Thread { editing, .. }
            | Command::Joint { editing, .. } => editing.as_ref(),
            _ => None,
        }
    }
}

/// Overlap between two bodies.
#[derive(Clone)]
pub struct Interference {
    pub a: BodyId,
    pub b: BodyId,
    pub volume: f64,
    pub mesh: Arc<BodyMesh>,
}

/// Pairwise overlaps of bodies (bounding boxes first, then a boolean
/// intersection).
pub fn interferences(model: &Model, bodies: &[BodyId]) -> Vec<Interference> {
    let bs: Vec<_> = bodies.iter().filter_map(|id| model.body(*id)).collect();
    let mut out = Vec::new();
    for i in 0..bs.len() {
        for j in i + 1..bs.len() {
            let (a, b) = (bs[i], bs[j]);
            let (am, bm) = (&a.mesh, &b.mesh);
            if (0..3).any(|k| am.bbox_max[k] < bm.bbox_min[k] || bm.bbox_max[k] < am.bbox_min[k]) {
                continue;
            }
            // Exact boolean when truck manages it, else a mesh estimate.
            let exact = m360_kernel::quiet_catch(|| m360_kernel::ops::boolean(&a.solid, std::slice::from_ref(&b.solid), BooleanOp::Intersect))
                .and_then(|r| r.ok())
                .map(|s| {
                    let mesh = m360_kernel::tessellate(&s);
                    (mesh.volume(), mesh)
                });
            let found = match exact {
                Some(e) => Some(e),
                None => m360_kernel::mesh_overlap(am, bm),
            };
            // Bodies that merely touch give (numerically) tiny overlaps.
            let min_vol = am.volume().abs().min(bm.volume().abs());
            if let Some((volume, mesh)) = found.filter(|(v, _)| *v > 1e-4 * min_vol.max(1e-9)) {
                out.push(Interference { a: a.id, b: b.id, volume, mesh: Arc::new(mesh) });
            }
        }
    }
    out
}

/// ISO metric coarse threads: (designation, nominal diameter, pitch).
pub const ISO_THREADS: &[(&str, f64, f64)] = &[
    ("M1.6x0.35", 1.6, 0.35),
    ("M2x0.4", 2.0, 0.4),
    ("M2.5x0.45", 2.5, 0.45),
    ("M3x0.5", 3.0, 0.5),
    ("M4x0.7", 4.0, 0.7),
    ("M5x0.8", 5.0, 0.8),
    ("M6x1", 6.0, 1.0),
    ("M8x1.25", 8.0, 1.25),
    ("M10x1.5", 10.0, 1.5),
    ("M12x1.75", 12.0, 1.75),
    ("M14x2", 14.0, 2.0),
    ("M16x2", 16.0, 2.0),
    ("M20x2.5", 20.0, 2.5),
    ("M24x3", 24.0, 3.0),
    ("M30x3.5", 30.0, 3.5),
    ("M36x4", 36.0, 4.0),
    ("M42x4.5", 42.0, 4.5),
    ("M48x5", 48.0, 5.0),
    ("M56x5.5", 56.0, 5.5),
    ("M64x6", 64.0, 6.0),
];

/// The ISO size that best fits a cylindrical face (a hole is matched by
/// its minor diameter).
pub fn iso_for(radius: f64, internal: bool) -> usize {
    let d = 2.0 * radius;
    let fit = |&(_, nom, p): &(&str, f64, f64)| if internal { nom - 1.0825 * p } else { nom };
    (0..ISO_THREADS.len()).min_by(|a, b| (fit(&ISO_THREADS[*a]) - d).abs().total_cmp(&(fit(&ISO_THREADS[*b]) - d).abs())).unwrap_or(0)
}

/// How a reference plane is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefPlaneMethod {
    /// Through a line, rotated by an angle from an adjacent face.
    Angle,
    /// Midway between two planes (or bisecting them).
    Mid,
    ThreePoints,
}

impl RefPlaneMethod {
    pub fn picks(self) -> usize {
        match self {
            RefPlaneMethod::Angle => 1,
            RefPlaneMethod::Mid => 2,
            RefPlaneMethod::ThreePoints => 3,
        }
    }
}

/// A geometric reference picked for a reference plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RefPick {
    Plane(PlaneRef),
    /// Line from `a` to `b`; `n0` is the normal of the plane at 0°.
    Line { a: DVec3, b: DVec3, n0: DVec3 },
    Point(DVec3),
}

/// The plane defined by reference picks.
pub fn ref_plane(method: RefPlaneMethod, picks: &[RefPick], angle_deg: f64) -> Option<PlaneRef> {
    match (method, picks) {
        (RefPlaneMethod::Angle, [RefPick::Line { a, b, n0 }]) => {
            let d = (*b - *a).normalize_or_zero();
            if d == DVec3::ZERO {
                return None;
            }
            let n0 = (*n0 - d * n0.dot(d)).normalize_or_zero();
            if n0 == DVec3::ZERO {
                return None;
            }
            let (s, c) = angle_deg.to_radians().sin_cos();
            let n = n0 * c + d.cross(n0) * s;
            Some(PlaneRef::from_frame((*a + *b) / 2.0, d, n.cross(d)))
        }
        (RefPlaneMethod::Mid, [RefPick::Plane(p), RefPick::Plane(q)]) => {
            let (f, g) = (p.frame([0.0, 0.0]), q.frame([0.0, 0.0]));
            let (n1, n2) = (f.n, g.n);
            let (d1, d2) = (f.origin.dot(n1), g.origin.dot(n2));
            if n1.dot(n2).abs() > 1.0 - 1e-9 {
                // Parallel: halfway between them.
                let off = (g.origin - f.origin).dot(n1) / 2.0;
                return Some(PlaneRef::from_frame(f.origin + n1 * off, f.u, f.v));
            }
            // Bisector through the intersection line.
            let k = n1.dot(n2);
            let det = 1.0 - k * k;
            let p0 = n1 * ((d1 - d2 * k) / det) + n2 * ((d2 - d1 * k) / det);
            let line = n1.cross(n2).normalize();
            let n = (n1 - n2).normalize();
            Some(PlaneRef::from_frame(p0, line, n.cross(line)))
        }
        (RefPlaneMethod::ThreePoints, [RefPick::Point(a), RefPick::Point(b), RefPick::Point(c)]) => {
            let n = (*b - *a).cross(*c - *a);
            if n.length() < 1e-9 {
                return None;
            }
            Some(PlaneRef::from_frame((*a + *b + *c) / 3.0, *b - *a, n.normalize().cross((*b - *a).normalize())))
        }
        _ => None,
    }
}

/// Cached sketch profiles (regions + shading triangles).
pub struct ProfileCache {
    pub key: String,
    pub regions: Vec<Region>,
    pub tris: Vec<Vec<[f32; 3]>>,
}

pub struct Preview {
    pub key: String,
    pub mesh: Option<Arc<BodyMesh>>,
    pub error: Option<String>,
}

/// Reference to a planar face (a point inside one of its triangles and its
/// outward normal), or `None` for curved faces.
pub fn face_ref(mesh: &BodyMesh, body: BodyId, face: u32) -> Option<m360_core::FaceRef> {
    let n = mesh.planar_normal(face)?;
    let (t, _) = mesh.triangles().find(|(_, f)| *f == face)?;
    let c = (t[0] + t[1] + t[2]) / 3.0;
    Some(m360_core::FaceRef { body, point: c.as_dvec3().to_array(), normal: n.as_dvec3().to_array() })
}

/// Joint origin of a mesh face: centroid and outward normal.
pub fn face_joint_geo(mesh: &BodyMesh, face: u32) -> Option<m360_core::JointGeo> {
    let c = mesh.face_centroid(face)?;
    let n = mesh.planar_normal(face).or_else(|| {
        let s: glam::Vec3 = mesh.face_ids.iter().zip(&mesh.normals).filter(|(f, _)| **f == face).map(|(_, n)| glam::Vec3::from(*n)).sum();
        (s.length_squared() > 1e-12).then(|| s.normalize())
    })?;
    Some(m360_core::JointGeo { point: c.as_dvec3().to_array(), normal: n.as_dvec3().to_array() })
}

/// Representative points (centroids) of mesh faces.
pub fn face_points(mesh: &BodyMesh, faces: &[u32]) -> Vec<[f64; 3]> {
    faces.iter().filter_map(|f| mesh.face_centroid(*f)).map(|c| c.as_dvec3().to_array()).collect()
}

/// End points of mesh edges (as stored by Fillet features).
pub fn edge_endpoints(mesh: &BodyMesh, edges: &[u32]) -> Vec<[[f64; 3]; 2]> {
    edges
        .iter()
        .filter_map(|&i| mesh.edges.get(i as usize))
        .map(|pts| {
            let a = pts[0].map(f64::from);
            let b = pts[pts.len() - 1].map(f64::from);
            [a, b]
        })
        .collect()
}

/// Indices of the mesh edges matching stored end points.
pub fn edge_indices(mesh: &BodyMesh, ends: &[[[f64; 3]; 2]]) -> Vec<u32> {
    ends.iter()
        .filter_map(|[a, b]| {
            let (a, b) = (glam::DVec3::from(*a), glam::DVec3::from(*b));
            mesh.edges
                .iter()
                .enumerate()
                .map(|(i, pts)| {
                    let p = glam::Vec3::from(pts[0]).as_dvec3();
                    let q = glam::Vec3::from(pts[pts.len() - 1]).as_dvec3();
                    (i, ((p - a).length() + (q - b).length()).min((p - b).length() + (q - a).length()))
                })
                .filter(|(_, d)| *d < 0.5)
                .min_by(|x, y| x.1.total_cmp(&y.1))
                .map(|(i, _)| i as u32)
        })
        .collect()
}

/// An active section analysis.
#[derive(Debug, Clone, Copy)]
pub struct SectionState {
    pub plane: PlaneRef,
    pub flip: bool,
    pub visible: bool,
}

impl SectionState {
    /// Clip plane as (normal, offset): geometry with n·p > d is hidden.
    pub fn clip(&self) -> [f32; 4] {
        let f = self.plane.frame([0.0, 0.0]);
        let n = if self.flip { -f.n } else { f.n };
        let d = n.dot(f.origin);
        [n.x as f32, n.y as f32, n.z as f32, d as f32]
    }
}

pub struct DocTab {
    pub doc: Document,
    pub history: History,
    pub regen: Regenerator,
    pub model: Model,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub version: u32,
    pub camera: Camera,
    pub home: Camera,
    pub anim: Option<(Camera, Camera, f64)>,
    pub selection: Vec<SelItem>,
    pub hover: Option<SelItem>,
    pub command: Option<Command>,
    pub sketch: Option<SketchSession>,
    pub display: Display,
    pub nav: NavMode,
    pub timeline_hover: Option<u64>,
    pub profiles: HashMap<u64, ProfileCache>,
    pub preview: Option<Preview>,
    pub comments: Vec<String>,
    pub fit_pending: bool,
    pub section: Option<SectionState>,
}

impl DocTab {
    pub fn new(doc: Document, path: Option<PathBuf>) -> Self {
        let mut t = Self {
            doc,
            history: History::default(),
            regen: Regenerator::default(),
            model: Model::default(),
            path,
            dirty: false,
            version: 1,
            camera: Camera::home(),
            home: Camera::home(),
            anim: None,
            selection: Vec::new(),
            hover: None,
            command: None,
            sketch: None,
            display: Display::default(),
            nav: NavMode::Select,
            timeline_hover: None,
            profiles: HashMap::new(),
            preview: None,
            comments: Vec::new(),
            fit_pending: false,
            section: None,
        };
        t.regenerate();
        if !t.model.bodies.is_empty() {
            t.fit_pending = true;
        }
        t
    }

    pub fn title(&self) -> String {
        format!("{} v{}{}", self.doc.name, self.version, if self.dirty { "*" } else { "" })
    }

    pub fn regenerate(&mut self) {
        self.model = self.regen.regenerate(&self.doc);
        let model = &self.model;
        self.selection.retain(|s| match s.body() {
            Some(b) => model.body(b).is_some(),
            None => true,
        });
    }

    /// Apply an undoable edit.
    pub fn edit(&mut self, label: &str, f: impl FnOnce(&mut Document)) {
        let before = self.doc.clone();
        f(&mut self.doc);
        if self.doc != before {
            self.history.push(label, before);
            self.dirty = true;
            self.regenerate();
        }
    }

    pub fn undo(&mut self) -> Option<String> {
        let l = self.history.undo(&mut self.doc)?;
        self.dirty = true;
        self.regenerate();
        Some(l)
    }

    pub fn redo(&mut self) -> Option<String> {
        let l = self.history.redo(&mut self.doc)?;
        self.dirty = true;
        self.regenerate();
        Some(l)
    }

    /// The plane referenced by a selectable item, if any.
    pub fn plane_of(&self, item: &SelItem) -> Option<PlaneRef> {
        match item {
            SelItem::Plane(b) => Some(PlaneRef { base: *b, offset: 0.0, flip: false, custom: None }),
            SelItem::FacePlane(_, _, p) => Some(*p),
            SelItem::ConstructionPlane(uid) => self.model.planes.iter().find(|p| p.uid == *uid).map(|p| p.plane),
            _ => None,
        }
    }

    pub fn body_visible(&self, id: BodyId) -> bool {
        let meta = self.doc.body_meta(id);
        let comp_visible = meta.component.is_none_or(|c| self.doc.components.iter().find(|k| k.id == c).is_none_or(|k| k.visible));
        self.doc.bodies_folder_visible && meta.visible && comp_visible
    }

    pub fn selected_bodies(&self) -> Vec<BodyId> {
        let mut out = Vec::new();
        for s in &self.selection {
            if let Some(b) = s.body() {
                if !out.contains(&b) {
                    out.push(b);
                }
            }
        }
        out
    }

    pub fn animate_to(&mut self, target: Camera, now: f64) {
        self.anim = Some((self.camera, target, now));
    }

    /// Sketch profile regions of a sketch (cached).
    pub fn sketch_profiles(&mut self, uid: u64) -> Option<&ProfileCache> {
        let sk = self.model.sketches.iter().find(|s| s.uid == uid)?;
        let key = format!("{:?}{:?}", sk.plane, sk.entities);
        let stale = self.profiles.get(&uid).is_none_or(|c| c.key != key);
        if stale {
            let regions = m360_core::sketch::profiles(&sk.entities);
            let frame = sk.plane.frame([0.0, 0.0]);
            let tris = regions.iter().map(|r| profile::region_triangles(&frame, r)).collect();
            self.profiles.insert(uid, ProfileCache { key, regions, tris });
        }
        self.profiles.get(&uid)
    }

    /// Entities of a sketch in the (possibly rolled back) model.
    pub fn sketch_entities(&self, uid: u64) -> Option<(PlaneRef, Vec<SketchEntity>)> {
        self.model.sketches.iter().find(|s| s.uid == uid).map(|s| (s.plane, s.entities.clone()))
    }

    /// Build (and cache) the preview body for the active creation command.
    pub fn update_preview(&mut self) {
        let Some(cmd) = &self.command else {
            self.preview = None;
            return;
        };
        let (key, solid) = match cmd {
            Command::Primitive { kind, plane_chosen: true, .. } => {
                let key = format!("{kind:?}");
                if self.preview.as_ref().is_some_and(|p| p.key == key) {
                    return;
                }
                (key, build_primitive(kind).map(|o| o.unwrap()))
            }
            Command::Extrude { faces, distance, symmetric, .. } if !faces.is_empty() => {
                let key = format!("faces{faces:?}{distance}{symmetric}");
                if self.preview.as_ref().is_some_and(|p| p.key == key) {
                    return;
                }
                let (z0, z1) = profile::extent(*distance, *symmetric);
                let mut shells = Vec::new();
                let mut err = None;
                for (fr, f) in faces {
                    match self.model.body(fr.body) {
                        Some(b) => match m360_kernel::ops::extrude_face(&b.solid, *f as usize, DVec3::from(fr.normal), z0, z1) {
                            Ok(s) => shells.extend(s.into_boundaries()),
                            Err(e) => err = Some(e.to_string()),
                        },
                        None => err = Some("the body no longer exists".into()),
                    }
                }
                (key, match err {
                    Some(e) => Err(e),
                    None => Ok(m360_kernel::Solid::new(shells)),
                })
            }
            Command::Extrude { sketch: Some(sk), profiles, distance, symmetric, taper, .. } if !profiles.is_empty() => {
                let key = format!("{sk}{profiles:?}{distance}{symmetric}{taper}");
                if self.preview.as_ref().is_some_and(|p| p.key == key) {
                    return;
                }
                let Some((plane, ents)) = self.sketch_entities(*sk) else {
                    self.preview = None;
                    return;
                };
                (key, m360_core::model::extrude_tool(&plane, &ents, profiles, *distance, *symmetric, *taper))
            }
            Command::Revolve { sketch: Some(sk), profiles, axis: Some(axis), angle_deg, .. } if !profiles.is_empty() => {
                let key = format!("rev{sk}{profiles:?}{axis:?}{angle_deg}");
                if self.preview.as_ref().is_some_and(|p| p.key == key) {
                    return;
                }
                let Some((plane, ents)) = self.sketch_entities(*sk) else {
                    self.preview = None;
                    return;
                };
                (key, m360_core::model::revolve_tool(&plane, &ents, profiles, axis, *angle_deg))
            }
            Command::Sweep { sketch: Some(sk), profiles, path_sketch: Some(ps), path, .. } if !profiles.is_empty() && !path.is_empty() => {
                let key = format!("sweep{sk}{profiles:?}{ps}{path:?}");
                if self.preview.as_ref().is_some_and(|p| p.key == key) {
                    return;
                }
                let r = (|| {
                    let (plane, ents) = self.sketch_entities(*sk).ok_or("sketch missing")?;
                    let segs = m360_core::model::path_segments(&self.model, *ps, path)?;
                    let frame = plane.frame([0.0, 0.0]);
                    let regions = m360_core::sketch::profiles(&ents);
                    let mut shells = Vec::new();
                    for &i in profiles {
                        let r = regions.get(i).ok_or("profile missing")?;
                        let s = m360_kernel::sweep::sweep_region(&frame, r, &segs).map_err(|e| e.to_string())?;
                        shells.extend(s.into_boundaries());
                    }
                    Ok::<_, String>(m360_kernel::Solid::new(shells))
                })();
                (key, r)
            }
            Command::Pipe { path_sketch: Some(ps), path, diameter, wall, hollow, .. } if !path.is_empty() => {
                let key = format!("pipe{ps}{path:?}{diameter}{wall}{hollow}");
                if self.preview.as_ref().is_some_and(|p| p.key == key) {
                    return;
                }
                let r = m360_core::model::path_segments(&self.model, *ps, path)
                    .and_then(|segs| m360_core::model::pipe_tool(&segs, *diameter, if *hollow { *wall } else { 0.0 }));
                (key, r)
            }
            Command::Shell { body: Some(b), faces, thickness, .. } => {
                let key = format!("shell{b}{faces:?}{thickness}");
                if self.preview.as_ref().is_some_and(|p| p.key == key) {
                    return;
                }
                let Some(body) = self.model.body(*b) else {
                    self.preview = None;
                    return;
                };
                let pts = face_points(&body.mesh, faces);
                let open = m360_core::model::shell_open_caps(&body.mesh, &pts);
                let r = m360_kernel::shell::shell(&body.solid, &body.mesh, *thickness, open).map_err(|e| e.to_string());
                self.preview = Some(match r {
                    Ok(s) => Preview { key, mesh: Some(Arc::new(m360_kernel::tessellate_smooth(&s, 0.985))), error: None },
                    Err(e) => Preview { key, mesh: None, error: Some(e) },
                });
                return;
            }
            Command::Loft { sections, .. } if sections.len() >= 2 => {
                let key = format!("loft{sections:?}");
                if self.preview.as_ref().is_some_and(|p| p.key == key) {
                    return;
                }
                let r = (|| {
                    let mut secs = Vec::new();
                    for (sk, i) in sections {
                        let (plane, ents) = self.sketch_entities(*sk).ok_or("sketch missing")?;
                        let r = m360_core::sketch::profiles(&ents).get(*i).cloned().ok_or("profile missing")?;
                        secs.push((plane.frame([0.0, 0.0]), r));
                    }
                    m360_kernel::sweep::loft(&secs).map_err(|e| e.to_string())
                })();
                (key, r)
            }
            Command::Move { .. } | Command::Pattern { .. } | Command::Mirror { .. } | Command::Joint { .. } => {
                let kind = match cmd {
                    Command::Joint { b: Some((bodies, gb)), a: Some(ga), motion, .. } => {
                        // Preview the mated position as a transformed copy.
                        let m = m360_core::JointGeo::mate(ga, gb, motion);
                        let key = format!("joint{bodies:?}{ga:?}{gb:?}{motion:?}");
                        if self.preview.as_ref().is_some_and(|p| p.key == key) {
                            return;
                        }
                        let meshes: Vec<BodyMesh> = bodies.iter().filter_map(|id| self.model.body(*id)).map(|b| b.mesh.transformed(m)).collect();
                        self.preview = (!meshes.is_empty()).then(|| Preview { key, mesh: Some(Arc::new(BodyMesh::merge(&meshes))), error: None });
                        return;
                    }
                    Command::Move { bodies, translate, axis, angle_deg, .. } => {
                        // Moves preview as a "copy" of the moved bodies.
                        FeatureKind::Move { bodies: bodies.clone(), translate: *translate, axis: *axis, angle_deg: *angle_deg, copy: true }
                    }
                    Command::Pattern { bodies, pattern, .. } => FeatureKind::Pattern { bodies: bodies.clone(), pattern: *pattern },
                    Command::Mirror { bodies, plane: Some(plane), .. } => FeatureKind::Mirror { bodies: bodies.clone(), plane: *plane },
                    _ => {
                        self.preview = None;
                        return;
                    }
                };
                let key = format!("{kind:?}");
                if self.preview.as_ref().is_some_and(|p| p.key == key) {
                    return;
                }
                let (bodies, mats) = kind.copy_transforms().unwrap();
                let mut meshes = Vec::new();
                for id in bodies {
                    if let Some(b) = self.model.body(*id) {
                        for m in &mats {
                            meshes.push(b.mesh.transformed(*m));
                        }
                    }
                }
                self.preview = if meshes.is_empty() {
                    None
                } else {
                    Some(Preview { key, mesh: Some(Arc::new(BodyMesh::merge(&meshes))), error: None })
                };
                return;
            }
            Command::Hole { plane: Some(plane), points, diameter, depth, through_all, kind, .. } if !points.is_empty() => {
                let key = format!("hole{plane:?}{points:?}{diameter}{depth}{through_all}{kind:?}");
                if self.preview.as_ref().is_some_and(|p| p.key == key) {
                    return;
                }
                // Preview the tools; through-all holes are drawn 2x the model size deep.
                let deep = {
                    let (min, max) = crate::viewport::scene_bounds(self);
                    (max - min).length() as f64 * 1.2
                };
                let d = if *through_all { deep } else { *depth };
                let r = m360_core::model::hole_tools(plane, points, *diameter, d, false, kind);
                self.preview = Some(match r {
                    Ok(tools) => {
                        let meshes: Vec<_> = tools.iter().map(m360_kernel::tessellate).collect();
                        Preview { key, mesh: Some(Arc::new(BodyMesh::merge(&meshes))), error: None }
                    }
                    Err(e) => Preview { key, mesh: None, error: Some(e) },
                });
                return;
            }
            Command::Fillet { body: Some(b), edges, size, chamfer, .. } if !edges.is_empty() => {
                let key = format!("fillet{b}{edges:?}{size}{chamfer}");
                if self.preview.as_ref().is_some_and(|p| p.key == key) {
                    return;
                }
                let Some(body) = self.model.body(*b) else {
                    self.preview = None;
                    return;
                };
                let ends = edge_endpoints(&body.mesh, edges);
                let specs = m360_core::model::resolve_edges(&body.mesh, &ends);
                let t = if *chamfer {
                    m360_kernel::fillet::Treatment::Chamfer { distance: *size }
                } else {
                    m360_kernel::fillet::Treatment::Fillet { radius: *size }
                };
                let r = specs.and_then(|sp| {
                    m360_kernel::fillet::treat_edges_report(&body.solid, &sp, t).map_err(|e| e.to_string())
                });
                self.preview = Some(match r {
                    Ok((s, faceted)) => {
                        let mesh = if faceted { m360_kernel::tessellate_smooth(&s, 0.97) } else { m360_kernel::tessellate(&s) };
                        Preview { key, mesh: Some(Arc::new(mesh)), error: None }
                    }
                    Err(e) => Preview { key, mesh: None, error: Some(e) },
                });
                return;
            }
            _ => {
                self.preview = None;
                return;
            }
        };
        self.preview = Some(match solid {
            Ok(s) => Preview { key, mesh: Some(Arc::new(m360_kernel::tessellate(&s))), error: None },
            Err(e) => Preview { key, mesh: None, error: Some(e) },
        });
    }
}
