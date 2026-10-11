//! Top-level application: layout, actions, tool dispatch, files.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Key, Modifiers};
use m360_core::{BooleanOp, Document, ExtrudeOp, FeatureKind, PlaneRef};
use m360_render::Viewport3D;

use crate::files::{FileDialog, FileMode};
use crate::state::{CombineField, Command, DocTab, EditCtx, SelItem, SketchSession, SketchTool};
use crate::{theme, tools};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Warning,
    Error,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub time: f64,
}

/// Deferred actions requested by UI components.
#[derive(Debug, Clone)]
pub enum Action {
    Tool(&'static str),
    NewDesign,
    Open,
    OpenPath(PathBuf),
    Save,
    SaveAs,
    Export(FileMode),
    CloseTab(usize),
    Undo,
    Redo,
    EditFeature(u64),
    EditSketch(u64),
    FinishSketch,
    DeleteSelection,
    Fit,
}

pub struct UiFlags {
    pub browser_open: bool,
    pub timeline_open: bool,
    pub data_panel: bool,
    pub comments_open: bool,
    pub workspace: usize,
    pub toolbar_tab: usize,
    pub toolbox: Option<crate::marking::Toolbox>,
    pub marking: Option<crate::marking::MarkingMenu>,
    pub rename: Option<(SelItem, String)>,
    pub about: bool,
    pub confirm_close: Option<usize>,
    pub shortcuts: bool,
}

pub struct Merge360App {
    pub tabs: Vec<DocTab>,
    pub active: usize,
    pub viewport: Option<Viewport3D>,
    pub flags: UiFlags,
    pub toasts: Vec<Toast>,
    pub recent: Vec<PathBuf>,
    pub last_tool: Option<&'static str>,
    pub actions: Vec<Action>,
    pub file_dialog: Option<FileDialog>,
    pub canvas_rect: egui::Rect,
    pub canvas_time: f64,
    pub canvas: crate::viewport::CanvasState,
    pub pending_sketch_plane: Option<PlaneRef>,
    pub script: Option<crate::script::Script>,
    pub fps: Option<(std::time::Instant, u32)>,
    pub untitled_counter: usize,
}

impl Merge360App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        cc.egui_ctx.set_theme(egui::ThemePreference::Light);
        let viewport = cc.wgpu_render_state.as_ref().map(Viewport3D::new);
        let mut app = Self {
            tabs: vec![DocTab::new(Document::new("Untitled"), None)],
            active: 0,
            viewport,
            flags: UiFlags {
                browser_open: true,
                timeline_open: true,
                data_panel: false,
                comments_open: false,
                workspace: 0,
                toolbar_tab: 0,
                toolbox: None,
                marking: None,
                rename: None,
                about: false,
                confirm_close: None,
                shortcuts: false,
            },
            toasts: Vec::new(),
            recent: crate::files::load_recent(),
            last_tool: None,
            actions: Vec::new(),
            file_dialog: None,
            canvas_rect: egui::Rect::NOTHING,
            canvas_time: 0.0,
            canvas: Default::default(),
            pending_sketch_plane: None,
            fps: None,
            script: std::env::var("M360_SCRIPT").ok().and_then(|p| crate::script::Script::load(&p)),
            untitled_counter: 1,
        };
        if let Some(p) = open {
            app.open_path(&p);
        } else if std::env::var_os("M360_SAMPLE").is_some() {
            app.open_sample();
        }
        if std::env::var("M360_STYLE").as_deref() == Ok("wire") {
            app.tab().display.style = crate::state::VisualStyle::Wireframe;
        }
        // Dev hook: M360_CAMERA="yaw_deg,pitch_deg,distance,tx,ty,tz".
        if let Ok(c) = std::env::var("M360_CAMERA") {
            let v: Vec<f32> = c.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            if v.len() == 6 {
                let tab = app.tab();
                tab.fit_pending = false;
                tab.camera.yaw = v[0].to_radians();
                tab.camera.pitch = v[1].to_radians();
                tab.camera.distance = v[2];
                tab.camera.target = glam::Vec3::new(v[3], v[4], v[5]);
            }
        }
        app
    }

    /// Load the built-in sample design (Help ▸ Open Sample Design).
    pub fn open_sample(&mut self) {
        let doc = crate::sample::sample_document();
        let tab = DocTab::new(doc, None);
        if self.tabs.len() == 1 && self.tabs[0].doc.features.is_empty() && !self.tabs[0].dirty {
            self.tabs[0] = tab;
            self.active = 0;
        } else {
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
        }
    }

    /// Developer hooks: `M360_SCREENSHOT=out.png` saves the window after a
    /// few frames and exits; `M360_SCRIPT=file` replays scripted input.
    fn dev_hooks(&mut self, ctx: &egui::Context) {
        let n = ctx.cumulative_frame_nr();
        if std::env::var_os("M360_FPS").is_some() {
            let now = std::time::Instant::now();
            let (start, frames) = self.fps.get_or_insert((now, 0));
            *frames += 1;
            if now.duration_since(*start).as_secs_f32() >= 2.0 {
                eprintln!("fps: {:.1} ({} frames, {:.0}x{:.0} pts)", *frames as f32 / now.duration_since(*start).as_secs_f32(), frames, ctx.content_rect().width(), ctx.content_rect().height());
                self.fps = Some((now, 0));
            }
        }
        if let Some(path) = std::env::var_os("M360_SCREENSHOT") {
            let frames: u64 = std::env::var("M360_SCREENSHOT_FRAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(40);
            if n == frames {
                let p = std::path::PathBuf::from(path);
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new((p, true))));
            }
            if n <= frames + 5 {
                ctx.request_repaint();
            }
        }
        if let Some(s) = &mut self.script {
            if let Some(t) = s.tool.take() {
                match tools::find(&t) {
                    Some(tool) => self.actions.push(Action::Tool(tool.id)),
                    None => eprintln!("script: unknown tool {t}"),
                }
            }
            for (k, v) in std::mem::take(&mut s.params) {
                let num: Option<f64> = v.parse().ok();
                let tab = &mut self.tabs[self.active];
                match (&mut tab.command, k.as_str()) {
                    (Some(Command::Extrude { distance, .. }), "distance") => *distance = num.unwrap_or(*distance),
                    (Some(Command::Extrude { symmetric, .. }), "symmetric") => *symmetric = v == "true",
                    (Some(Command::Extrude { op, .. }), "op") => {
                        *op = match v.as_str() {
                            "cut" => ExtrudeOp::Cut,
                            "join" => ExtrudeOp::Join,
                            "intersect" => ExtrudeOp::Intersect,
                            _ => ExtrudeOp::NewBody,
                        }
                    }
                    (Some(Command::Primitive { kind, .. }), key) => match (kind, key) {
                        (FeatureKind::Box { height, .. } | FeatureKind::Cylinder { height, .. }, "height") => *height = num.unwrap_or(*height),
                        (FeatureKind::Box { length, .. }, "length") => *length = num.unwrap_or(*length),
                        (FeatureKind::Box { width, .. }, "width") => *width = num.unwrap_or(*width),
                        (FeatureKind::Cylinder { diameter, .. } | FeatureKind::Sphere { diameter, .. }, "diameter") => *diameter = num.unwrap_or(*diameter),
                        _ => eprintln!("script: unknown param {k}"),
                    },
                    (Some(Command::Combine { op, .. }), "op") => {
                        *op = match v.as_str() {
                            "cut" => BooleanOp::Cut,
                            "intersect" => BooleanOp::Intersect,
                            _ => BooleanOp::Join,
                        }
                    }
                    (Some(Command::Move { translate, .. }), "x") => translate[0] = num.unwrap_or(0.0),
                    (Some(Command::Move { translate, .. }), "y") => translate[1] = num.unwrap_or(0.0),
                    (Some(Command::Move { translate, .. }), "z") => translate[2] = num.unwrap_or(0.0),
                    (Some(Command::Move { copy, .. }), "copy") => *copy = v == "true",
                    (Some(Command::Fillet { size, .. }), "size") => *size = num.unwrap_or(*size),
                    (Some(Command::Revolve { angle_deg, .. }), "angle") => *angle_deg = num.unwrap_or(*angle_deg),
                    (Some(Command::Revolve { axis, .. }), "axis") => {
                        *axis = match v.as_str() {
                            "x" => Some(m360_core::RevolveAxis::X),
                            "y" => Some(m360_core::RevolveAxis::Y),
                            "z" => Some(m360_core::RevolveAxis::Z),
                            n => n.parse().ok().map(m360_core::RevolveAxis::SketchLine),
                        }
                    }
                    (Some(Command::Revolve { op, .. }), "op") => {
                        *op = match v.as_str() {
                            "cut" => ExtrudeOp::Cut,
                            "join" => ExtrudeOp::Join,
                            _ => ExtrudeOp::NewBody,
                        }
                    }
                    (Some(Command::Fillet { chamfer, .. }), "chamfer") => *chamfer = v == "true",
                    _ => eprintln!("script: cannot set {k} on the current command"),
                }
            }
            if let Some(p) = s.dump.take() {
                if let Ok(text) = self.tabs[self.active].doc.to_ron() {
                    let _ = std::fs::write(&p, text);
                }
            }
            if let Some(p) = s.shot.take() {
                let p = std::path::PathBuf::from(p);
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new((p, false))));
            }
            if s.quit {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint();
        }
        let shots: Vec<(egui::ColorImage, Option<(std::path::PathBuf, bool)>)> = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot { image, user_data, .. } => Some((
                        (**image).clone(),
                        user_data.data.as_ref().and_then(|d| d.downcast_ref::<(std::path::PathBuf, bool)>()).cloned(),
                    )),
                    _ => None,
                })
                .collect()
        });
        for (img, info) in shots {
            let Some((path, quit)) = info else { continue };
            let [w, h] = img.size;
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                let _ = buf.save(&path);
            }
            if quit {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    pub fn tab(&mut self) -> &mut DocTab {
        &mut self.tabs[self.active]
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), kind, time: f64::NAN });
    }

    // ------------------------------------------------------------------ tools

    /// Start a toolbar tool by id.
    pub fn run_tool(&mut self, id: &'static str) {
        if !tools::is_implemented(id) {
            let label = tools::find(id).map_or(id, |t| t.label);
            self.toast(ToastKind::Info, format!("{label} is not available yet in Merge360."));
            return;
        }
        if !matches!(id, "select" | "window_select" | "select_all" | "invert_selection" | "clear_selection") {
            self.last_tool = Some(id);
        }
        let in_sketch = self.tab().sketch.is_some();
        if let Some(tool) = sketch_tool(id) {
            if let Some(s) = &mut self.tab().sketch {
                s.tool = tool;
                s.pending.clear();
            } else {
                self.toast(ToastKind::Info, "Sketch tools are available while editing a sketch.");
            }
            return;
        }
        if in_sketch && !matches!(id, "finish_sketch" | "measure" | "select" | "clear_selection") {
            self.finish_sketch();
        }
        self.cancel_command();
        let tab = self.tab();
        let sel_bodies = tab.selected_bodies();
        match id {
            "box" | "cylinder" | "sphere" | "torus" => {
                let plane = PlaneRef::XZ;
                let c = [0.0, 0.0];
                let kind = match id {
                    "box" => FeatureKind::Box { plane, center: c, length: 50.0, width: 50.0, height: 50.0 },
                    "cylinder" => FeatureKind::Cylinder { plane, center: c, diameter: 50.0, height: 50.0 },
                    "sphere" => FeatureKind::Sphere { plane, center: c, diameter: 50.0 },
                    _ => FeatureKind::Torus { plane, center: c, inner_diameter: 50.0, torus_diameter: 10.0 },
                };
                tab.selection.clear();
                tab.command = Some(Command::Primitive { kind, plane_chosen: false, editing: None });
            }
            "create_sketch" => {
                let pick = tab.selection.iter().find_map(|s| match s {
                    SelItem::Plane(b) => Some(PlaneRef { base: *b, offset: 0.0, flip: false }),
                    SelItem::FacePlane(_, _, p) => Some(*p),
                    _ => None,
                });
                match pick {
                    Some(p) => self.start_sketch(p),
                    None => {
                        tab.selection.clear();
                        tab.command = Some(Command::SketchPlane);
                    }
                }
            }
            "extrude" => {
                let mut sketch = None;
                let mut profiles = Vec::new();
                for s in &tab.selection {
                    if let SelItem::Profile(sk, i) = s {
                        if sketch.is_none() || sketch == Some(*sk) {
                            sketch = Some(*sk);
                            profiles.push(*i);
                        }
                    }
                }
                tab.selection.clear();
                tab.command = Some(Command::Extrude {
                    sketch,
                    profiles,
                    distance: 10.0,
                    symmetric: false,
                    op: ExtrudeOp::NewBody,
                    editing: None,
                });
                self.auto_extrude_op();
            }
            "combine" => {
                let target = sel_bodies.first().copied();
                let tools = sel_bodies.iter().skip(1).copied().collect();
                tab.command = Some(Command::Combine {
                    target,
                    tools,
                    op: BooleanOp::Join,
                    keep_tools: false,
                    field: if target.is_some() { CombineField::Tools } else { CombineField::Target },
                    editing: None,
                });
                tab.selection.clear();
            }
            "move" => {
                tab.command = Some(Command::Move {
                    bodies: sel_bodies,
                    translate: [0.0; 3],
                    axis: m360_core::Axis::Y,
                    angle_deg: 0.0,
                    copy: false,
                    editing: None,
                });
            }
            "revolve" => {
                let mut sketch = None;
                let mut profiles = Vec::new();
                for sitem in &tab.selection {
                    if let SelItem::Profile(sk, i) = sitem {
                        if sketch.is_none() || sketch == Some(*sk) {
                            sketch = Some(*sk);
                            profiles.push(*i);
                        }
                    }
                }
                tab.selection.clear();
                let op = if tab.model.bodies.is_empty() { ExtrudeOp::NewBody } else { ExtrudeOp::Join };
                tab.command = Some(Command::Revolve {
                    sketch,
                    profiles,
                    axis: None,
                    angle_deg: 360.0,
                    op,
                    picking_axis: false,
                    editing: None,
                });
            }
            "fillet" | "chamfer" => {
                let mut body = None;
                let mut edges = Vec::new();
                for sitem in &tab.selection {
                    if let SelItem::Edge(b, e) = sitem {
                        if body.is_none() || body == Some(*b) {
                            body = Some(*b);
                            edges.push(*e);
                        }
                    }
                }
                tab.selection.clear();
                tab.command = Some(Command::Fillet { body, edges, size: if id == "fillet" { 2.0 } else { 1.0 }, chamfer: id == "chamfer", editing: None });
            }
            "press_pull" => {
                // Fusion's Press Pull: fillet on edges, extrude on profiles.
                let on_edges = tab.selection.iter().any(|s| matches!(s, SelItem::Edge(..)));
                self.run_tool(if on_edges { "fillet" } else { "extrude" });
                return;
            }
            "delete" => self.delete_selection(),
            "appearance" => tab.command = Some(Command::Appearance),
            "measure" => {
                let picks = tab.selection.iter().take(2).copied().collect();
                tab.command = Some(Command::Measure { picks });
            }
            "center_of_mass" => tab.command = Some(Command::CenterOfMass),
            "change_parameters" => tab.command = Some(Command::Parameters),
            "doc_settings" => tab.command = Some(Command::DocSettings),
            "properties" => tab.command = Some(Command::Properties),
            "compute_all" => {
                tab.regen.clear();
                tab.regenerate();
                self.toast(ToastKind::Info, "All features recomputed.");
            }
            "export_3dprint" => self.actions.push(Action::Export(FileMode::ExportStl)),
            "export_step" => self.actions.push(Action::Export(FileMode::ExportStep)),
            "select" | "window_select" => tab.nav = crate::state::NavMode::Select,
            "select_all" => {
                tab.selection = tab
                    .model
                    .bodies
                    .iter()
                    .filter(|b| tab.doc.body_meta(b.id).visible)
                    .map(|b| SelItem::Body(b.id))
                    .collect();
            }
            "invert_selection" => {
                let sel = tab.selected_bodies();
                tab.selection = tab
                    .model
                    .bodies
                    .iter()
                    .filter(|b| !sel.contains(&b.id))
                    .map(|b| SelItem::Body(b.id))
                    .collect();
            }
            "clear_selection" => tab.selection.clear(),
            "finish_sketch" => self.finish_sketch(),
            _ => {}
        }
    }

    /// Pick a sensible default operation for Extrude (Fusion: Join when the
    /// profile sits on a body, else New Body).
    pub fn auto_extrude_op(&mut self) {
        let tab = self.tab();
        let has_bodies = !tab.model.bodies.is_empty();
        if let Some(Command::Extrude { op, sketch: Some(_), editing: None, .. }) = &mut tab.command {
            *op = if has_bodies { ExtrudeOp::Join } else { ExtrudeOp::NewBody };
        }
    }

    pub fn cancel_command(&mut self) {
        let tab = self.tab();
        if let Some(cmd) = tab.command.take() {
            if let Some(EditCtx { before, .. }) = cmd.editing() {
                tab.doc = before.clone();
                tab.regenerate();
            }
        }
        tab.preview = None;
    }

    pub fn start_sketch(&mut self, plane: PlaneRef) {
        let now = self.now();
        let tab = self.tab();
        let before = tab.doc.clone();
        let mut uid = 0;
        tab.edit("Create Sketch", |d| {
            uid = d.add_feature(FeatureKind::Sketch { plane, entities: Vec::new() });
        });
        tab.command = None;
        tab.selection.clear();
        let saved = tab.camera;
        tab.sketch = Some(SketchSession {
            uid,
            tool: SketchTool::Line,
            pending: Vec::new(),
            before,
            saved_camera: saved,
            show_profile: true,
            look_at: true,
        });
        look_at_plane(tab, plane, now);
    }

    pub fn edit_sketch(&mut self, uid: u64) {
        let now = self.now();
        self.cancel_command();
        if self.tab().sketch.is_some() {
            self.finish_sketch();
        }
        let tab = self.tab();
        let Some(idx) = tab.doc.feature_index(uid) else { return };
        let FeatureKind::Sketch { plane, .. } = tab.doc.features[idx].kind else { return };
        let before = tab.doc.clone();
        // Roll the marker to just after the sketch while editing.
        tab.doc.marker = idx + 1;
        tab.regenerate();
        let saved = tab.camera;
        tab.sketch = Some(SketchSession {
            uid,
            tool: SketchTool::Select,
            pending: Vec::new(),
            before,
            saved_camera: saved,
            show_profile: true,
            look_at: true,
        });
        tab.selection.clear();
        look_at_plane(tab, plane, now);
    }

    pub fn finish_sketch(&mut self) {
        let now = self.now();
        let tab = self.tab();
        let Some(s) = tab.sketch.take() else { return };
        // Editing an older sketch rolled the marker back; restore it.
        if s.before.feature(s.uid).is_some() && s.before.marker > tab.doc.marker {
            tab.doc.marker = s.before.marker.min(tab.doc.features.len());
            tab.regenerate();
        }
        tab.selection.clear();
        tab.animate_to(s.saved_camera, now);
    }

    pub fn edit_feature(&mut self, uid: u64) {
        self.cancel_command();
        if self.tab().sketch.is_some() {
            self.finish_sketch();
        }
        let tab = self.tab();
        let Some(idx) = tab.doc.feature_index(uid) else { return };
        let kind = tab.doc.features[idx].kind.clone();
        if let FeatureKind::Sketch { .. } = kind {
            self.edit_sketch(uid);
            return;
        }
        let before = tab.doc.clone();
        tab.doc.marker = idx;
        tab.regenerate();
        let editing = Some(EditCtx { uid, before });
        tab.selection.clear();
        tab.command = Some(match kind {
            FeatureKind::Box { .. } | FeatureKind::Cylinder { .. } | FeatureKind::Sphere { .. } | FeatureKind::Torus { .. } => {
                Command::Primitive { kind, plane_chosen: true, editing }
            }
            FeatureKind::Extrude { sketch, profiles, distance, symmetric, op, .. } => {
                Command::Extrude { sketch: Some(sketch), profiles, distance, symmetric, op, editing }
            }
            FeatureKind::Combine { target, tools, op, keep_tools } => Command::Combine {
                target: Some(target),
                tools,
                op,
                keep_tools,
                field: CombineField::Tools,
                editing,
            },
            FeatureKind::Move { bodies, translate, axis, angle_deg, copy } => {
                Command::Move { bodies, translate, axis, angle_deg, copy, editing }
            }
            FeatureKind::Revolve { sketch, profiles, axis, angle_deg, op, .. } => Command::Revolve {
                sketch: Some(sketch),
                profiles,
                axis: Some(axis),
                angle_deg,
                op,
                picking_axis: false,
                editing,
            },
            FeatureKind::Fillet { body, edges, size, chamfer } => {
                let idx = tab.model.body(body).map(|b| crate::state::edge_indices(&b.mesh, &edges)).unwrap_or_default();
                Command::Fillet { body: Some(body), edges: idx, size, chamfer, editing }
            }
            _ => {
                // Remove features have nothing to edit.
                if let Some(EditCtx { before, .. }) = editing {
                    tab.doc = before;
                    tab.regenerate();
                }
                return;
            }
        });
    }

    /// Commit a feature from a command: add it, or replace the edited one.
    pub fn commit_feature(&mut self, kind: FeatureKind, editing: Option<EditCtx>) {
        let tab = self.tab();
        tab.preview = None;
        match editing {
            Some(EditCtx { uid, before }) => {
                let mut doc = tab.doc.clone();
                if let Some(f) = doc.feature_mut(uid) {
                    f.kind = kind;
                }
                doc.marker = before.marker;
                let label = format!("Edit {}", doc.feature(uid).map_or("feature", |f| f.name.as_str()));
                tab.doc = before;
                tab.edit(&label, |d| *d = doc);
            }
            None => {
                let label = kind.base_name().to_string();
                tab.edit(&label, |d| {
                    d.add_feature(kind);
                });
            }
        }
        let errs: Vec<String> = tab.model.errors.values().cloned().collect();
        if let Some(e) = errs.first() {
            self.toast(ToastKind::Warning, format!("Compute failed: {e}"));
        }
    }

    pub fn delete_selection(&mut self) {
        let tab = self.tab();
        if let Some(s) = &mut tab.sketch {
            let uid = s.uid;
            let mut idx: Vec<usize> = tab
                .selection
                .iter()
                .filter_map(|x| match x {
                    SelItem::SketchEntity(u, i) if *u == uid => Some(*i),
                    _ => None,
                })
                .collect();
            idx.sort_unstable();
            idx.dedup();
            if !idx.is_empty() {
                tab.edit("Delete Sketch Entity", |d| {
                    if let Some(FeatureKind::Sketch { entities, .. }) = d.feature_mut(uid).map(|f| &mut f.kind) {
                        for i in idx.iter().rev() {
                            if *i < entities.len() {
                                entities.remove(*i);
                            }
                        }
                    }
                });
                tab.selection.clear();
            }
            return;
        }
        let features: Vec<u64> = tab
            .selection
            .iter()
            .filter_map(|s| match s {
                SelItem::Feature(u) | SelItem::Sketch(u) => Some(*u),
                _ => None,
            })
            .collect();
        let bodies = tab.selected_bodies();
        if features.is_empty() && bodies.is_empty() {
            self.toast(ToastKind::Info, "Select bodies or features to delete.");
            return;
        }
        tab.edit("Delete", |d| {
            for u in &features {
                d.delete_feature(*u);
            }
            if !bodies.is_empty() {
                d.add_feature(FeatureKind::Remove { bodies: bodies.clone() });
            }
        });
        tab.selection.clear();
    }

    // ------------------------------------------------------------------ files

    pub fn now(&self) -> f64 {
        self.canvas_time
    }

    pub fn new_design(&mut self) {
        self.untitled_counter += 1;
        let name = format!("Untitled({})", self.untitled_counter);
        self.tabs.push(DocTab::new(Document::new(name), None));
        self.active = self.tabs.len() - 1;
    }

    pub fn open_path(&mut self, path: &Path) {
        match Document::load(path) {
            Ok(doc) => {
                if let Some(i) = self.tabs.iter().position(|t| t.path.as_deref() == Some(path)) {
                    self.active = i;
                    return;
                }
                let t = DocTab::new(doc, Some(path.to_path_buf()));
                let errs = t.model.errors.len();
                // Replace a pristine untitled tab.
                if self.tabs.len() == 1 && self.tabs[0].path.is_none() && !self.tabs[0].dirty && self.tabs[0].doc.features.is_empty() {
                    self.tabs[0] = t;
                    self.active = 0;
                } else {
                    self.tabs.push(t);
                    self.active = self.tabs.len() - 1;
                }
                self.add_recent(path);
                if errs > 0 {
                    self.toast(ToastKind::Warning, format!("{errs} feature(s) failed to compute."));
                }
            }
            Err(e) => self.toast(ToastKind::Error, format!("Could not open {}: {e}", path.display())),
        }
    }

    pub fn save_to(&mut self, path: &Path) {
        let path = if path.extension().is_none() { path.with_extension("m360") } else { path.to_path_buf() };
        let tab = self.tab();
        if let Some(stem) = path.file_stem() {
            tab.doc.name = stem.to_string_lossy().into_owned();
        }
        match tab.doc.save(&path) {
            Ok(()) => {
                if tab.path.as_deref() == Some(path.as_path()) {
                    tab.version += 1;
                }
                tab.path = Some(path.clone());
                tab.dirty = false;
                let msg = format!("Saved {}", path.display());
                self.add_recent(&path);
                self.toast(ToastKind::Info, msg);
            }
            Err(e) => self.toast(ToastKind::Error, format!("Save failed: {e}")),
        }
    }

    pub fn export_to(&mut self, mode: FileMode, path: &Path) {
        let tab = &self.tabs[self.active];
        let selected = tab.selected_bodies();
        let bodies: Vec<_> = tab
            .model
            .bodies
            .iter()
            .filter(|b| if selected.is_empty() { tab.body_visible(b.id) } else { selected.contains(&b.id) })
            .collect();
        if bodies.is_empty() {
            self.toast(ToastKind::Warning, "Nothing to export: no visible bodies.");
            return;
        }
        let res = match mode {
            FileMode::ExportStl => {
                let meshes: Vec<_> = bodies.iter().map(|b| b.mesh.as_ref()).collect();
                m360_kernel::io::write_stl(&path.with_extension("stl"), &meshes)
            }
            _ => {
                let solids: Vec<_> = bodies.iter().map(|b| &b.solid).collect();
                m360_kernel::io::write_step(&path.with_extension("step"), &solids)
            }
        };
        let n = bodies.len();
        match res {
            Ok(()) => self.toast(ToastKind::Info, format!("Exported {n} body(ies) to {}", path.display())),
            Err(e) => self.toast(ToastKind::Error, format!("Export failed: {e}")),
        }
    }

    fn add_recent(&mut self, path: &Path) {
        let p = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.recent.retain(|r| r != &p);
        self.recent.insert(0, p);
        self.recent.truncate(12);
        crate::files::save_recent(&self.recent);
    }

    fn process_actions(&mut self) {
        let actions = std::mem::take(&mut self.actions);
        for a in actions {
            match a {
                Action::Tool(id) => self.run_tool(id),
                Action::NewDesign => self.new_design(),
                Action::Open => self.file_dialog = Some(FileDialog::new(FileMode::Open, self.default_dir(), "")),
                Action::OpenPath(p) => self.open_path(&p),
                Action::Save => match self.tabs[self.active].path.clone() {
                    Some(p) => self.save_to(&p),
                    None => self.actions_save_as(),
                },
                Action::SaveAs => self.actions_save_as(),
                Action::Export(mode) => {
                    let name = self.tabs[self.active].doc.name.clone();
                    self.file_dialog = Some(FileDialog::new(mode, self.default_dir(), &name));
                }
                Action::CloseTab(i) => {
                    if self.tabs[i].dirty {
                        self.flags.confirm_close = Some(i);
                    } else {
                        self.close_tab(i);
                    }
                }
                Action::Undo => {
                    if self.tab().sketch.as_ref().is_some_and(|s| !s.pending.is_empty()) {
                        if let Some(s) = &mut self.tab().sketch {
                            s.pending.clear();
                        }
                    } else if let Some(l) = self.tab().undo() {
                        self.toast(ToastKind::Info, format!("Undo {l}"));
                    }
                }
                Action::Redo => {
                    if let Some(l) = self.tab().redo() {
                        self.toast(ToastKind::Info, format!("Redo {l}"));
                    }
                }
                Action::EditFeature(u) => self.edit_feature(u),
                Action::EditSketch(u) => self.edit_sketch(u),
                Action::FinishSketch => self.finish_sketch(),
                Action::DeleteSelection => self.delete_selection(),
                Action::Fit => self.tab().fit_pending = true,
            }
        }
    }

    fn actions_save_as(&mut self) {
        let name = self.tabs[self.active].doc.name.clone();
        let dir = self.tabs[self.active]
            .path
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| self.default_dir());
        self.file_dialog = Some(FileDialog::new(FileMode::Save, dir, &name));
    }

    pub fn close_tab(&mut self, i: usize) {
        self.tabs.remove(i);
        if self.tabs.is_empty() {
            self.tabs.push(DocTab::new(Document::new("Untitled"), None));
        }
        self.active = self.active.min(self.tabs.len() - 1);
    }

    pub fn default_dir(&self) -> PathBuf {
        self.recent
            .first()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    // ------------------------------------------------------------------ keyboard

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, k));
        let cmd_shift = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, k));
        if cmd_shift(Key::S) {
            self.actions.push(Action::SaveAs);
        }
        if cmd(Key::S) {
            self.actions.push(Action::Save);
        }
        if cmd(Key::N) {
            self.actions.push(Action::NewDesign);
        }
        if cmd(Key::O) {
            self.actions.push(Action::Open);
        }
        if cmd(Key::Z) {
            self.actions.push(Action::Undo);
        }
        if cmd(Key::Y) || cmd_shift(Key::Z) {
            self.actions.push(Action::Redo);
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let key = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
        if key(Key::Escape) {
            if self.flags.marking.is_some() || self.flags.toolbox.is_some() {
                self.flags.marking = None;
                self.flags.toolbox = None;
            } else if self.tab().sketch.as_ref().is_some_and(|s| !s.pending.is_empty()) {
                if let Some(s) = &mut self.tab().sketch {
                    s.pending.clear();
                }
            } else if self.tab().sketch.as_ref().is_some_and(|s| s.tool != SketchTool::Select) {
                if let Some(s) = &mut self.tab().sketch {
                    s.tool = SketchTool::Select;
                }
            } else if self.tab().command.is_some() {
                self.cancel_command();
            } else {
                let tab = self.tab();
                tab.selection.clear();
                tab.nav = crate::state::NavMode::Select;
            }
        }
        if key(Key::Delete) || key(Key::Backspace) {
            self.actions.push(Action::DeleteSelection);
        }
        if key(Key::F6) {
            self.actions.push(Action::Fit);
        }
        if key(Key::Enter) {
            self.flags.toolbox = None;
            if self.tab().command.is_none() {
                if let Some(t) = self.last_tool {
                    self.actions.push(Action::Tool(t));
                }
            }
        }
        if key(Key::S) {
            let pos = ctx.pointer_latest_pos().unwrap_or(self.canvas_rect.center());
            self.flags.toolbox = Some(crate::marking::Toolbox::new(pos));
        }
        let in_sketch = self.tab().sketch.is_some();
        let bindings: &[(Key, &'static str)] = if in_sketch {
            &[
                (Key::L, "sk_line"),
                (Key::R, "sk_rect"),
                (Key::C, "sk_circle"),
                (Key::P, "sk_point"),
                (Key::E, "extrude"),
                (Key::I, "measure"),
                (Key::D, "sk_dimension"),
                (Key::T, "sk_trim"),
                (Key::O, "sk_offset"),
            ]
        } else {
            &[
                (Key::E, "extrude"),
                (Key::Q, "press_pull"),
                (Key::F, "fillet"),
                (Key::M, "move"),
                (Key::J, "joint"),
                (Key::H, "hole"),
                (Key::I, "measure"),
                (Key::A, "appearance"),
                (Key::L, "sk_line"),
                (Key::R, "sk_rect"),
                (Key::C, "sk_circle"),
            ]
        };
        for (k, id) in bindings {
            if key(*k) {
                if !in_sketch && id.starts_with("sk_") {
                    // Sketch shortcuts outside a sketch start a new one first (like Fusion).
                    self.actions.push(Action::Tool("create_sketch"));
                } else {
                    self.actions.push(Action::Tool(id));
                }
            }
        }
    }
}

pub(crate) fn sketch_tool(id: &str) -> Option<SketchTool> {
    Some(match id {
        "sk_line" => SketchTool::Line,
        "sk_rect" => SketchTool::Rect,
        "sk_center_rect" => SketchTool::CenterRect,
        "sk_circle" => SketchTool::Circle,
        "sk_point" => SketchTool::Point,
        "sk_arc" => SketchTool::Arc,
        "sk_polygon" => SketchTool::Polygon,
        _ => return None,
    })
}

/// Animate the camera to look straight at a sketch plane.
pub fn look_at_plane(tab: &mut DocTab, plane: PlaneRef, now: f64) {
    let f = plane.frame([0.0, 0.0]);
    let mut cam = tab.camera;
    cam.look_from(f.n.as_vec3());
    if plane.base == m360_core::BasePlane::XZ && !plane.flip {
        // Looking straight down: keep X right (yaw 0).
        cam.yaw = 0.0;
    } else if plane.base == m360_core::BasePlane::XZ {
        cam.yaw = 0.0;
    }
    tab.animate_to(cam, now);
}

impl eframe::App for Merge360App {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(mut s) = self.script.take() {
            let cam = self.tabs[self.active].camera;
            let world = |x: f32, y: f32, z: f32| self.to_screen(&cam, glam::Vec3::new(x, y, z)).unwrap_or_default();
            s.feed(raw, self.canvas_rect, &world);
            self.script = Some(s);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.canvas_time = ctx.input(|i| i.time);
        self.shortcuts(&ctx);

        let title = format!("Merge360 - {}", self.tabs[self.active].title());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        self.app_bar(ui);
        self.toolbar(ui);
        if self.flags.timeline_open {
            self.timeline(ui);
        }
        if self.flags.data_panel {
            self.data_panel(ui);
        }
        egui::CentralPanel::no_frame().show(ui, |ui| {
            self.canvas(ui, frame);
        });
        self.overlays(&ctx);
        self.dialogs(&ctx);
        self.process_actions();
        self.dev_hooks(&ctx);
        // Keep animating while the camera transitions or toasts are visible.
        if self.tabs[self.active].anim.is_some() || !self.toasts.is_empty() {
            ctx.request_repaint();
        }
    }
}
