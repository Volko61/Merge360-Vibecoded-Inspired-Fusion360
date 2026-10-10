//! Sketch-mode drawing: snapping, click handling, rubber-band previews.

use eframe::egui::Pos2;
use m360_core::{FeatureKind, SketchEntity, Units};

use crate::app::Merge360App;
use crate::state::{SketchSession, SketchTool};
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
        _ => Vec::new(),
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

    pub(crate) fn sketch_click(&mut self) {
        let Some(cur) = self.canvas.sketch_cursor else { return };
        let tab = self.tab();
        let Some(s) = &mut tab.sketch else { return };
        let uid = s.uid;
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
            SketchTool::Rect | SketchTool::CenterRect | SketchTool::Circle => {
                if s.pending.is_empty() {
                    s.pending.push(cur);
                } else {
                    new = rubber_band(s, cur);
                    let degenerate = match s.tool {
                        SketchTool::Circle => dist(s.pending[0], cur) < 1e-9,
                        _ => (cur[0] - s.pending[0][0]).abs() < 1e-9 || (cur[1] - s.pending[0][1]).abs() < 1e-9,
                    };
                    if degenerate {
                        new.clear();
                    }
                    label = if s.tool == SketchTool::Circle { "Circle" } else { "Rectangle" };
                    s.pending.clear();
                }
            }
            SketchTool::Select => {}
        }
        if new.is_empty() {
            return;
        }
        tab.edit(label, |d| {
            if let Some(FeatureKind::Sketch { entities, .. }) = d.feature_mut(uid).map(|f| &mut f.kind) {
                entities.extend(new);
            }
        });
    }
}
