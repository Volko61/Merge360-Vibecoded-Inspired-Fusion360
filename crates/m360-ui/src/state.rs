//! Per-document UI state (a design tab).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

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
        }
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
        op: ExtrudeOp,
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
            Command::Fillet { chamfer: true, .. } => "CHAMFER",
            Command::Fillet { .. } => "FILLET",
            Command::SketchPlane => "CREATE SKETCH",
            Command::Measure { .. } => "MEASURE",
            Command::Appearance => "APPEARANCE",
            Command::Parameters => "PARAMETERS",
            Command::DocSettings => "DOCUMENT SETTINGS",
            Command::CenterOfMass => "CENTER OF MASS",
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
            | Command::Mirror { editing, .. } => editing.as_ref(),
            _ => None,
        }
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

    pub fn body_visible(&self, id: BodyId) -> bool {
        self.doc.bodies_folder_visible && self.doc.body_meta(id).visible
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
            Command::Extrude { sketch: Some(sk), profiles, distance, symmetric, .. } if !profiles.is_empty() => {
                let key = format!("{sk}{profiles:?}{distance}{symmetric}");
                if self.preview.as_ref().is_some_and(|p| p.key == key) {
                    return;
                }
                let Some((plane, ents)) = self.sketch_entities(*sk) else {
                    self.preview = None;
                    return;
                };
                (key, m360_core::model::extrude_tool(&plane, &ents, profiles, *distance, *symmetric))
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
            Command::Move { .. } | Command::Pattern { .. } | Command::Mirror { .. } => {
                let kind = match cmd {
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
