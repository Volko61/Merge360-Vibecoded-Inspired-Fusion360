//! Sketch-mode drawing: snapping, click handling, rubber-band previews.

use eframe::egui::Pos2;
use m360_core::solver::{point_of, solve, PointRef, SketchConstraint};
use m360_core::{FeatureKind, SketchEntity, Units};

use crate::app::Merge360App;
use crate::app::ToastKind;
use crate::state::{ConKind, DimEdit, EditPurpose, SelItem, SketchSession, SketchTool};
use crate::viewport::{grid_step, ray_plane, to_world};

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Entities the current tool would create with `cursor` as the next point.
pub fn rubber_band(s: &SketchSession, cursor: [f64; 2]) -> Vec<SketchEntity> {
    let Some(&first) = s.pending.first() else { return Vec::new() };
    match s.tool {
        SketchTool::Line => vec![SketchEntity::Line { a: *s.pending.last().unwrap(), b: cursor }],
        SketchTool::Rect => m360_core::sketch::rectangle(first, cursor),
        SketchTool::CenterRect => {
            let d = [cursor[0] - first[0], cursor[1] - first[1]];
            m360_core::sketch::rectangle([first[0] - d[0], first[1] - d[1]], cursor)
        }
        SketchTool::Circle => vec![SketchEntity::Circle { c: first, r: dist(first, cursor) }],
        SketchTool::Arc => match s.pending.len() {
            1 => vec![SketchEntity::Line { a: first, b: cursor }],
            _ => arc_or_line(first, s.pending[1], cursor),
        },
        SketchTool::Polygon => m360_core::sketch::polygon(first, cursor, POLYGON_SIDES),
        _ => Vec::new(),
    }
}

pub const POLYGON_SIDES: usize = 6;

/// Arc from a to b through m, or a line when the points are collinear.
fn arc_or_line(a: [f64; 2], b: [f64; 2], m: [f64; 2]) -> Vec<SketchEntity> {
    let cross = (b[0] - a[0]) * (m[1] - a[1]) - (b[1] - a[1]) * (m[0] - a[0]);
    if cross.abs() < 1e-9 || dist(m, a) < 1e-9 || dist(m, b) < 1e-9 {
        vec![SketchEntity::Line { a, b }]
    } else {
        vec![SketchEntity::Arc { a, m, b }]
    }
}

pub fn rubber_band_label(s: &SketchSession, cursor: [f64; 2], units: Units) -> Option<String> {
    let first = *s.pending.first()?;
    Some(match s.tool {
        SketchTool::Line => {
            let a = *s.pending.last().unwrap();
            let ang = (cursor[1] - a[1]).atan2(cursor[0] - a[0]).to_degrees();
            format!("{}   {:.1}°", units.format(dist(a, cursor)), ang)
        }
        SketchTool::Rect => format!(
            "{} × {}",
            units.format((cursor[0] - first[0]).abs()),
            units.format((cursor[1] - first[1]).abs())
        ),
        SketchTool::CenterRect => format!(
            "{} × {}",
            units.format(2.0 * (cursor[0] - first[0]).abs()),
            units.format(2.0 * (cursor[1] - first[1]).abs())
        ),
        SketchTool::Circle => format!("⌀ {}", units.format(2.0 * dist(first, cursor))),
        SketchTool::Arc if s.pending.len() == 1 => units.format(dist(first, cursor)),
        SketchTool::Arc => match m360_kernel::profile::circumcircle(first, cursor, s.pending[1]) {
            Some((_, r)) => format!("R {}", units.format(r)),
            None => return None,
        },
        SketchTool::Polygon => format!("{} sides, R {}", POLYGON_SIDES, units.format(dist(first, cursor))),
        _ => return None,
    })
}

impl Merge360App {
    /// Update the snapped sketch cursor from the pointer position.
    pub(crate) fn update_sketch_cursor(&mut self, pos: Pos2) {
        let tab = &self.tabs[self.active];
        let Some(s) = &tab.sketch else { return };
        let Some((plane, ents)) = tab.sketch_entities(s.uid) else { return };
        let cam = tab.camera;
        let r = self.canvas_rect;
        let ndc = glam::Vec2::new((pos.x - r.left()) / r.width() * 2.0 - 1.0, 1.0 - (pos.y - r.top()) / r.height() * 2.0);
        let ray = cam.ray(ndc, r.aspect_ratio());
        let f = plane.frame([0.0, 0.0]);
        let Some(t) = ray_plane(&ray, f.origin.as_vec3(), f.n.as_vec3()) else {
            self.canvas.sketch_cursor = None;
            return;
        };
        let w = (ray.origin + ray.dir * t).as_dvec3() - f.origin;
        let mut local = [w.dot(f.u), w.dot(f.v)];
        self.canvas.sketch_raw = Some(local);
        // Snap to existing points within 10 px.
        let mut snapped = false;
        let mut best = 10.0f32;
        let mut candidates: Vec<[f64; 2]> = ents.iter().flat_map(crate::viewport::entity_points).collect();
        candidates.extend(s.pending.iter().copied());
        candidates.push([0.0, 0.0]);
        for c in candidates {
            if let Some(sp) = self.to_screen(&cam, to_world(&plane, c)) {
                let d = (sp - pos).length();
                if d < best {
                    best = d;
                    local = c;
                    snapped = true;
                }
            }
        }
        if !snapped && tab.display.snap {
            let step = grid_step(&cam) as f64 / 10.0;
            local = [(local[0] / step).round() * step, (local[1] / step).round() * step];
        }
        self.canvas.sketch_cursor = Some(local);
        self.canvas.snapped = snapped;
    }

    pub(crate) fn sketch_click(&mut self, hover: Option<SelItem>) {
        if self.tabs[self.active].sketch.as_ref().is_some_and(|s| matches!(s.tool, SketchTool::Dimension | SketchTool::Constrain(_))) {
            self.constraint_pick(hover);
            return;
        }
        if self.tabs[self.active].sketch.as_ref().is_some_and(|s| matches!(s.tool, SketchTool::Trim | SketchTool::Offset | SketchTool::Fillet)) {
            self.edit_tool_pick(hover);
            return;
        }
        let Some(cur) = self.canvas.sketch_cursor else { return };
        let tab = self.tab();
        let Some(s) = &mut tab.sketch else { return };
        let mut new: Vec<SketchEntity> = Vec::new();
        let mut label = "Sketch";
        match s.tool {
            SketchTool::Point => {
                new.push(SketchEntity::Point { p: cur });
                label = "Point";
            }
            SketchTool::Line => {
                if let Some(&last) = s.pending.last() {
                    if dist(last, cur) > 1e-9 {
                        new.push(SketchEntity::Line { a: last, b: cur });
                        label = "Line";
                        // Close the chain when returning to its start.
                        if s.pending.len() > 1 && dist(s.pending[0], cur) < 1e-9 {
                            s.pending.clear();
                        } else {
                            s.pending.push(cur);
                        }
                    }
                } else {
                    s.pending.push(cur);
                }
            }
            SketchTool::Arc => {
                if s.pending.len() < 2 {
                    if s.pending.last().is_none_or(|p| dist(*p, cur) > 1e-9) {
                        s.pending.push(cur);
                    }
                } else {
                    new = rubber_band(s, cur);
                    label = "Arc";
                    s.pending.clear();
                }
            }
            SketchTool::Rect | SketchTool::CenterRect | SketchTool::Circle | SketchTool::Polygon => {
                if s.pending.is_empty() {
                    s.pending.push(cur);
                } else {
                    new = rubber_band(s, cur);
                    let degenerate = match s.tool {
                        SketchTool::Circle | SketchTool::Polygon => dist(s.pending[0], cur) < 1e-9,
                        _ => (cur[0] - s.pending[0][0]).abs() < 1e-9 || (cur[1] - s.pending[0][1]).abs() < 1e-9,
                    };
                    if degenerate {
                        new.clear();
                    }
                    label = match s.tool {
                        SketchTool::Circle => "Circle",
                        SketchTool::Polygon => "Polygon",
                        _ => "Rectangle",
                    };
                    s.pending.clear();
                }
            }
            SketchTool::Select | SketchTool::Dimension | SketchTool::Constrain(_) | SketchTool::Trim | SketchTool::Offset | SketchTool::Fillet => {}
        }
        if new.is_empty() {
            return;
        }
        self.sketch_edit(label, |entities, constraints| {
            // Automatic constraints: exact horizontal / vertical lines.
            let base = entities.len();
            for (k, e) in new.iter().enumerate() {
                if let SketchEntity::Line { a, b } = e {
                    if (a[1] - b[1]).abs() < 1e-9 {
                        constraints.push(SketchConstraint::Horizontal(base + k));
                    } else if (a[0] - b[0]).abs() < 1e-9 {
                        constraints.push(SketchConstraint::Vertical(base + k));
                    }
                }
            }
            entities.extend(new);
        });
    }

    /// Edit the active sketch's geometry/constraints and re-solve it. The
    /// edit is rejected (with a message) if the constraints cannot be met.
    pub(crate) fn sketch_edit(&mut self, label: &str, f: impl FnOnce(&mut Vec<SketchEntity>, &mut Vec<SketchConstraint>)) -> bool {
        let tab = self.tab();
        let Some(uid) = tab.sketch.as_ref().map(|s| s.uid) else { return false };
        let Some(FeatureKind::Sketch { entities, constraints, .. }) = tab.doc.feature(uid).map(|f| f.kind.clone()) else {
            return false;
        };
        let (mut e, mut c) = (entities, constraints);
        f(&mut e, &mut c);
        if let Err(err) = solve(&mut e, &c, None) {
            self.toast(ToastKind::Warning, format!("{label}: {err}"));
            return false;
        }
        let tab = self.tab();
        tab.edit(label, |d| {
            if let Some(FeatureKind::Sketch { entities, constraints, .. }) = d.feature_mut(uid).map(|f| &mut f.kind) {
                *entities = e;
                *constraints = c;
            }
        });
        true
    }

    /// Trim / Offset / Fillet tools.
    fn edit_tool_pick(&mut self, hover: Option<SelItem>) {
        let pointer = self.canvas.last_pointer.unwrap_or_default();
        let units = self.tabs[self.active].doc.units;
        let tab = &self.tabs[self.active];
        let Some(s) = &tab.sketch else { return };
        let (uid, tool) = (s.uid, s.tool);
        let Some((_, ents)) = tab.sketch_entities(uid) else { return };
        let open = |app: &mut Self, purpose: EditPurpose, default: f64| {
            if let Some(s) = &mut app.tab().sketch {
                let text = format!("{}", units.from_mm(default));
                s.dim_edit = Some(DimEdit {
                    purpose,
                    index: None,
                    draft: SketchConstraint::Fix(PointRef::new(0, 0)),
                    text,
                    pos: pointer,
                    focus: 3,
                });
            }
        };
        match (tool, hover) {
            (SketchTool::Trim, Some(SelItem::SketchEntity(_, i))) => {
                let Some(click) = self.canvas.sketch_raw else { return };
                let Some(rep) = m360_core::sketch::trim(&ents, i, click) else { return };
                self.sketch_edit("Trim", move |e, c| {
                    // Constraints on the trimmed entity no longer apply.
                    c.retain(|k| !k.entities().contains(&i));
                    let mut rep = rep.into_iter();
                    match rep.next() {
                        Some(first) => {
                            e[i] = first;
                            e.extend(rep);
                        }
                        None => {
                            e.remove(i);
                            *c = c.iter().filter_map(|k| k.remapped(&[i])).collect();
                        }
                    }
                });
            }
            (SketchTool::Offset, Some(SelItem::SketchEntity(_, i))) => {
                let chain = match ents.get(i) {
                    Some(SketchEntity::Line { .. }) | Some(SketchEntity::Arc { .. }) => crate::viewport::chain_of(&tab.model, uid, i),
                    _ => vec![i],
                };
                open(self, EditPurpose::Offset(chain), 2.0);
            }
            (SketchTool::Fillet, Some(SelItem::SketchPoint(_, p))) => {
                let Some(q) = point_of(&ents, p) else { return };
                let lines: Vec<usize> = ents
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| matches!(e, SketchEntity::Line { a, b } if dist(*a, q) < 1e-6 || dist(*b, q) < 1e-6))
                    .map(|(i, _)| i)
                    .collect();
                if lines.len() == 2 {
                    open(self, EditPurpose::Fillet(lines[0], lines[1]), 2.0);
                } else {
                    self.toast(ToastKind::Info, "Click a corner where exactly two lines meet.");
                }
            }
            _ => {}
        }
    }

    /// Picks of the Sketch Dimension and constraint tools.
    fn constraint_pick(&mut self, hover: Option<SelItem>) {
        let pointer = self.canvas.last_pointer.unwrap_or_default();
        let units = self.tabs[self.active].doc.units;
        let tab = &self.tabs[self.active];
        let Some(s) = &tab.sketch else { return };
        let uid = s.uid;
        let tool = s.tool;
        let Some((_, ents)) = tab.sketch_entities(uid) else { return };
        let constraints = tab
            .model
            .sketches
            .iter()
            .find(|k| k.uid == uid)
            .map(|k| k.constraints.clone())
            .unwrap_or_default();
        let Some(hover) = hover else {
            if let Some(s) = &mut self.tab().sketch {
                s.picked.clear();
            }
            return;
        };
        let line = |i: usize| matches!(ents.get(i), Some(SketchEntity::Line { .. }));
        let round = |i: usize| matches!(ents.get(i), Some(SketchEntity::Circle { .. }) | Some(SketchEntity::Arc { .. }));
        match tool {
            SketchTool::Dimension => {
                let draft = match hover {
                    SelItem::SketchConstraint(_, ci) => {
                        if let Some(c) = constraints.get(ci) {
                            if matches!(c, SketchConstraint::Distance { .. } | SketchConstraint::Diameter { .. }) {
                                self.open_dim_edit(Some(ci), c.clone(), pointer, units);
                            }
                        }
                        return;
                    }
                    SelItem::SketchEntity(_, i) if line(i) => {
                        let (a, b) = (PointRef::new(i, 0), PointRef::new(i, 1));
                        let (pa, pb) = (point_of(&ents, a).unwrap(), point_of(&ents, b).unwrap());
                        Some(SketchConstraint::Distance { a, b, value: dist(pa, pb) })
                    }
                    SelItem::SketchEntity(_, i) if round(i) => {
                        let r = match ents[i] {
                            SketchEntity::Circle { r, .. } => r,
                            SketchEntity::Arc { a, m, b } => m360_kernel::profile::circumcircle(a, m, b).map_or(0.0, |c| c.1),
                            _ => 0.0,
                        };
                        Some(SketchConstraint::Diameter { entity: i, value: 2.0 * r })
                    }
                    SelItem::SketchPoint(_, p) => {
                        let first = self.tabs[self.active].sketch.as_ref().and_then(|s| s.picked.first().copied());
                        match first {
                            Some(SelItem::SketchPoint(_, q)) if q != p => {
                                let (pa, pb) = (point_of(&ents, q).unwrap(), point_of(&ents, p).unwrap());
                                Some(SketchConstraint::Distance { a: q, b: p, value: dist(pa, pb) })
                            }
                            _ => {
                                if let Some(s) = &mut self.tab().sketch {
                                    s.picked = vec![hover];
                                }
                                None
                            }
                        }
                    }
                    _ => None,
                };
                if let Some(d) = draft {
                    if let Some(s) = &mut self.tab().sketch {
                        s.picked.clear();
                    }
                    self.open_dim_edit(None, d, pointer, units);
                }
            }
            SketchTool::Constrain(kind) => {
                // Normalise the pick: points for point tools, entities otherwise.
                let pick = match (kind.wants_points(), hover) {
                    (true, SelItem::SketchPoint(..)) | (false, SelItem::SketchEntity(..)) => hover,
                    (true, SelItem::SketchEntity(u, i)) if kind == ConKind::Fix => SelItem::SketchEntity(u, i),
                    (false, SelItem::SketchPoint(u, p)) => SelItem::SketchEntity(u, p.entity),
                    _ => return,
                };
                let mut picks = self.tabs[self.active].sketch.as_ref().map(|s| s.picked.clone()).unwrap_or_default();
                picks.push(pick);
                if picks.len() < kind.picks() {
                    if let Some(s) = &mut self.tab().sketch {
                        s.picked = picks;
                    }
                    return;
                }
                if let Some(s) = &mut self.tab().sketch {
                    s.picked.clear();
                }
                let ent = |x: &SelItem| match x {
                    SelItem::SketchEntity(_, i) => Some(*i),
                    _ => None,
                };
                let pt = |x: &SelItem| match x {
                    SelItem::SketchPoint(_, p) => Some(*p),
                    _ => None,
                };
                let mut new: Vec<SketchConstraint> = Vec::new();
                let bad = |app: &mut Self, msg: &str| app.toast(ToastKind::Info, msg.to_string());
                match kind {
                    ConKind::HorizontalVertical => match ent(&picks[0]) {
                        Some(i) if line(i) => {
                            let (a, b) = (point_of(&ents, PointRef::new(i, 0)).unwrap(), point_of(&ents, PointRef::new(i, 1)).unwrap());
                            new.push(if (b[0] - a[0]).abs() >= (b[1] - a[1]).abs() {
                                SketchConstraint::Horizontal(i)
                            } else {
                                SketchConstraint::Vertical(i)
                            });
                        }
                        _ => return bad(self, "Select a line."),
                    },
                    ConKind::Fix => match (&picks[0], ent(&picks[0])) {
                        (SelItem::SketchPoint(_, p), _) => new.push(SketchConstraint::Fix(*p)),
                        (_, Some(i)) => new.extend(m360_core::solver::points_of(&ents, i).into_iter().map(SketchConstraint::Fix)),
                        _ => return,
                    },
                    ConKind::Coincident => match (pt(&picks[0]), pt(&picks[1])) {
                        (Some(a), Some(b)) if a.entity != b.entity => new.push(SketchConstraint::Coincident(a, b)),
                        _ => return bad(self, "Select two points of different entities."),
                    },
                    ConKind::Parallel | ConKind::Perpendicular => match (ent(&picks[0]), ent(&picks[1])) {
                        (Some(a), Some(b)) if a != b && line(a) && line(b) => new.push(if kind == ConKind::Parallel {
                            SketchConstraint::Parallel(a, b)
                        } else {
                            SketchConstraint::Perpendicular(a, b)
                        }),
                        _ => return bad(self, "Select two lines."),
                    },
                    ConKind::Equal => match (ent(&picks[0]), ent(&picks[1])) {
                        (Some(a), Some(b)) if a != b && ((line(a) && line(b)) || (round(a) && round(b))) => new.push(SketchConstraint::Equal(a, b)),
                        _ => return bad(self, "Select two lines or two circles/arcs."),
                    },
                    ConKind::Tangent => match (ent(&picks[0]), ent(&picks[1])) {
                        (Some(a), Some(b)) if line(a) && round(b) => new.push(SketchConstraint::Tangent(a, b)),
                        (Some(a), Some(b)) if round(a) && line(b) => new.push(SketchConstraint::Tangent(b, a)),
                        _ => return bad(self, "Select a line and a circle or arc."),
                    },
                    ConKind::Concentric => match (ent(&picks[0]), ent(&picks[1])) {
                        (Some(a), Some(b)) if a != b && round(a) && round(b) => new.push(SketchConstraint::Concentric(a, b)),
                        _ => return bad(self, "Select two circles or arcs."),
                    },
                }
                self.sketch_edit(kind.label(), |_, c| c.extend(new));
            }
            _ => {}
        }
    }

    pub(crate) fn open_dim_edit(&mut self, index: Option<usize>, draft: SketchConstraint, pos: eframe::egui::Pos2, units: Units) {
        let value = match draft {
            SketchConstraint::Distance { value, .. } | SketchConstraint::Diameter { value, .. } => value,
            _ => return,
        };
        let text = format!("{:.3}", units.from_mm(value)).trim_end_matches('0').trim_end_matches('.').to_string();
        if let Some(s) = &mut self.tab().sketch {
            s.dim_edit = Some(DimEdit { purpose: EditPurpose::Dimension, index, draft, text, pos, focus: 3 });
        }
    }

    /// The inline dimension value editor.
    pub(crate) fn dim_edit_ui(&mut self, ctx: &eframe::egui::Context) {
        use eframe::egui;
        let units = self.tabs[self.active].doc.units;
        let Some(s) = &mut self.tab().sketch else { return };
        let Some(ed) = &mut s.dim_edit else { return };
        let mut commit = None;
        let mut cancel = false;
        egui::Area::new(egui::Id::new("dim_edit")).fixed_pos(ed.pos + egui::vec2(8.0, -12.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut ed.text).desired_width(90.0));
                if ed.focus > 0 {
                    ed.focus -= 1;
                    r.request_focus();
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    commit = units.parse(&ed.text);
                    if commit.is_none() {
                        cancel = true;
                    }
                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) || (r.lost_focus() && ed.focus == 0) {
                    cancel = true;
                }
            });
        });
        if cancel {
            s.dim_edit = None;
            return;
        }
        let Some(v) = commit else { return };
        let ed = s.dim_edit.take().unwrap();
        match ed.purpose.clone() {
            EditPurpose::Offset(chain) => {
                self.sketch_edit("Offset", |e, _| {
                    let new = m360_core::sketch::offset(e, &chain, v);
                    e.extend(new);
                });
                return;
            }
            EditPurpose::Fillet(l1, l2) => {
                if v <= 0.0 {
                    return;
                }
                let tab = self.tab();
                let ents = tab.sketch.as_ref().and_then(|s| tab.sketch_entities(s.uid)).map(|x| x.1).unwrap_or_default();
                match m360_core::sketch::fillet_corner(&ents, l1, l2, v) {
                    Some((n1, n2, arc)) => {
                        self.sketch_edit("Sketch Fillet", |e, _| {
                            e[l1] = n1;
                            e[l2] = n2;
                            e.push(arc);
                        });
                    }
                    None => self.toast(ToastKind::Warning, "The fillet radius is too large for these lines."),
                }
                return;
            }
            EditPurpose::Dimension => {}
        }
        if v <= 0.0 {
            self.toast(ToastKind::Warning, "Dimensions must be positive.");
            return;
        }
        let mut c = ed.draft.clone();
        match &mut c {
            SketchConstraint::Distance { value, .. } | SketchConstraint::Diameter { value, .. } => *value = v,
            _ => {}
        }
        self.sketch_edit("Sketch Dimension", move |_, cs| match ed.index {
            Some(i) if i < cs.len() => cs[i] = c,
            _ => cs.push(c),
        });
    }

    /// Drag a sketch point with the Select tool (live solve).
    pub(crate) fn sketch_drag(&mut self, start: bool, release: bool) {
        let tab = self.tab();
        let Some(s) = &tab.sketch else { return };
        let uid = s.uid;
        let Some(p) = s.drag else { return };
        if start {
            self.canvas.drag_before = Some(self.tabs[self.active].doc.clone());
        }
        let Some(before) = self.canvas.drag_before.clone() else { return };
        let Some(FeatureKind::Sketch { entities, constraints, .. }) = before.feature(uid).map(|f| f.kind.clone()) else { return };
        if release {
            let tab = self.tab();
            if let Some(s) = &mut tab.sketch {
                s.drag = None;
            }
            if tab.doc != before {
                tab.history.push("Drag", before);
                tab.dirty = true;
                tab.regenerate();
            }
            self.canvas.drag_before = None;
            return;
        }
        let Some(target) = self.canvas.sketch_raw else { return };
        let mut e = entities;
        if solve(&mut e, &constraints, Some((p, target))).is_ok() {
            let tab = self.tab();
            if let Some(FeatureKind::Sketch { entities, .. }) = tab.doc.feature_mut(uid).map(|f| &mut f.kind) {
                *entities = e.clone();
            }
            // Update the displayed geometry without a full regeneration.
            if let Some(sk) = tab.model.sketches.iter_mut().find(|k| k.uid == uid) {
                sk.entities = e;
            }
        }
    }
}
