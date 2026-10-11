//! DRAWING workspace: 2D orthographic views with hidden-line removal on an
//! A3 sheet with a title block, plus SVG export.

use std::fmt::Write as _;

use eframe::egui::{self, pos2, vec2, Color32, Rect, Sense, Stroke};
use glam::{Vec2, Vec3};
use m360_kernel::BodyMesh;

use crate::app::{Merge360App, ToastKind};
use crate::theme;

/// Sheet size (mm) — ISO A3 landscape.
const SHEET: Vec2 = Vec2::new(420.0, 297.0);
const MARGIN: f32 = 10.0;
const TITLE_W: f32 = 180.0;
const TITLE_H: f32 = 32.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    Front,
    Top,
    Right,
    Iso,
}

impl ViewKind {
    fn label(self) -> &'static str {
        match self {
            ViewKind::Front => "FRONT",
            ViewKind::Top => "TOP",
            ViewKind::Right => "RIGHT",
            ViewKind::Iso => "ISOMETRIC",
        }
    }

    /// (right, up, toward-viewer) axes of the view (Y-up world).
    fn axes(self) -> (Vec3, Vec3, Vec3) {
        match self {
            ViewKind::Front => (Vec3::X, Vec3::Y, Vec3::Z),
            ViewKind::Top => (Vec3::X, Vec3::NEG_Z, Vec3::Y),
            ViewKind::Right => (Vec3::NEG_Z, Vec3::Y, Vec3::X),
            ViewKind::Iso => {
                let toward = Vec3::new(1.0, 1.0, 1.0).normalize();
                let right = Vec3::Y.cross(toward).normalize();
                let up = toward.cross(right);
                (right, up, toward)
            }
        }
    }
}

/// A projected view: segments in view units (mm, model scale 1).
pub struct View2D {
    pub kind: ViewKind,
    pub visible: Vec<[Vec2; 2]>,
    pub hidden: Vec<[Vec2; 2]>,
    pub min: Vec2,
    pub max: Vec2,
}

pub struct DrawingCache {
    key: Vec<usize>,
    pub views: Vec<View2D>,
    pub scale: f32,
}

/// Standard drawing scales (paper / model).
const SCALES: [f32; 13] = [10.0, 5.0, 4.0, 2.0, 1.0, 0.5, 0.4, 0.25, 0.2, 0.1, 0.05, 0.02, 0.01];

fn scale_label(s: f32) -> String {
    if s >= 1.0 {
        format!("{}:1", s)
    } else {
        format!("1:{}", (1.0 / s).round())
    }
}

/// Simple CPU depth buffer for visibility queries.
struct ZBuf {
    w: usize,
    h: usize,
    min: Vec2,
    px: f32,
    z: Vec<f32>,
}

impl ZBuf {
    fn new(min: Vec2, max: Vec2, res: usize) -> Self {
        let size = (max - min).max(Vec2::splat(1e-3));
        let px = size.x.max(size.y) / res as f32;
        let w = (size.x / px).ceil() as usize + 3;
        let h = (size.y / px).ceil() as usize + 3;
        Self { w, h, min: min - Vec2::splat(px), px, z: vec![f32::MIN; w * h] }
    }

    fn cell(&self, p: Vec2) -> (f32, f32) {
        ((p.x - self.min.x) / self.px, (p.y - self.min.y) / self.px)
    }

    fn raster(&mut self, t: [Vec3; 3]) {
        let (a, b, c) = (t[0], t[1], t[2]);
        let (ax, ay) = self.cell(a.truncate());
        let (bx, by) = self.cell(b.truncate());
        let (cx, cy) = self.cell(c.truncate());
        let area = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
        if area.abs() < 1e-9 {
            return;
        }
        let x0 = ax.min(bx).min(cx).floor().max(0.0) as usize;
        let x1 = (ax.max(bx).max(cx).ceil() as usize).min(self.w - 1);
        let y0 = ay.min(by).min(cy).floor().max(0.0) as usize;
        let y1 = (ay.max(by).max(cy).ceil() as usize).min(self.h - 1);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((bx - px) * (cy - py) - (by - py) * (cx - px)) / area;
                let w1 = ((cx - px) * (ay - py) - (cy - py) * (ax - px)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                    continue;
                }
                let z = w0 * a.z + w1 * b.z + w2 * c.z;
                let k = y * self.w + x;
                if z > self.z[k] {
                    self.z[k] = z;
                }
            }
        }
    }

    /// Visible if not behind the nearest surface (checking a 5x5 neighbourhood
    /// so that edges lying on silhouettes are not hidden by their own faces).
    fn visible(&self, p: Vec3, eps: f32) -> bool {
        let (x, y) = self.cell(p.truncate());
        let (x, y) = (x as isize, y as isize);
        let mut nearest = f32::MAX;
        for dy in -2..=2 {
            for dx in -2..=2 {
                let (xx, yy) = (x + dx, y + dy);
                if xx < 0 || yy < 0 || xx >= self.w as isize || yy >= self.h as isize {
                    continue;
                }
                nearest = nearest.min(self.z[yy as usize * self.w + xx as usize]);
            }
        }
        nearest == f32::MIN || p.z >= nearest - eps
    }
}

fn project(meshes: &[&BodyMesh], kind: ViewKind) -> View2D {
    let (r, u, t) = kind.axes();
    let pr = |p: Vec3| Vec3::new(p.dot(r), p.dot(u), p.dot(t));
    let mut min = Vec2::splat(f32::MAX);
    let mut max = Vec2::splat(f32::MIN);
    let mut zmin = f32::MAX;
    let mut zmax = f32::MIN;
    for m in meshes {
        for p in &m.positions {
            let q = pr(Vec3::from(*p));
            min = min.min(q.truncate());
            max = max.max(q.truncate());
            zmin = zmin.min(q.z);
            zmax = zmax.max(q.z);
        }
    }
    let mut zb = ZBuf::new(min, max, 900);
    for m in meshes {
        for (tri, _) in m.triangles() {
            zb.raster(tri.map(pr));
        }
    }
    let eps = ((zmax - zmin) * 0.004).max(zb.px * 1.5);

    // Segments: B-rep edges + silhouettes of curved surfaces.
    let mut segs: Vec<[Vec3; 2]> = Vec::new();
    for m in meshes {
        for e in &m.edges {
            for w in e.windows(2) {
                segs.push([pr(Vec3::from(w[0])), pr(Vec3::from(w[1]))]);
            }
        }
        segs.extend(silhouettes(m, t).into_iter().map(|s| s.map(pr)));
    }

    let mut visible = Vec::new();
    let mut hidden = Vec::new();
    for [a, b] in segs {
        // Split each segment into pieces of uniform visibility.
        let len = (b - a).truncate().length();
        let n = ((len / (zb.px * 2.0)).ceil() as usize).clamp(1, 400);
        let mut start = a;
        let mut start_vis = zb.visible(a, eps);
        for k in 1..=n {
            let p = a.lerp(b, k as f32 / n as f32);
            let vis = zb.visible(p, eps);
            if vis != start_vis || k == n {
                let seg = [start.truncate(), p.truncate()];
                if start_vis { visible.push(seg) } else { hidden.push(seg) }
                start = p;
                start_vis = vis;
            }
        }
    }
    View2D { kind, visible, hidden, min, max }
}

/// Silhouette curves seen along `toward`: the zero contour of n·v traced
/// through every triangle using the (exact) smooth vertex normals. Planar
/// faces have constant normals and produce no contour.
fn silhouettes(m: &BodyMesh, toward: Vec3) -> Vec<[Vec3; 2]> {
    let mut out = Vec::new();
    for t in m.indices.as_chunks::<3>().0 {
        let p: Vec<Vec3> = t.iter().map(|&i| Vec3::from(m.positions[i as usize])).collect();
        // Snap tiny values to zero: seams often lie exactly on the outline.
        let s: Vec<f32> = t
            .iter()
            .map(|&i| Vec3::from(m.normals[i as usize]).dot(toward))
            .map(|v| if v.abs() < 1e-4 { 0.0 } else { v })
            .collect();
        let zeros: Vec<usize> = (0..3).filter(|&k| s[k] == 0.0).collect();
        match zeros.len() {
            3 => continue, // planar face seen edge-on (its edges are B-rep edges)
            2 => out.push([p[zeros[0]], p[zeros[1]]]),
            _ => {
                let mut pts = Vec::new();
                for k in 0..3 {
                    let (a, b) = (k, (k + 1) % 3);
                    if s[a] == 0.0 {
                        pts.push(p[a]);
                    } else if s[b] != 0.0 && (s[a] < 0.0) != (s[b] < 0.0) {
                        let f = s[a] / (s[a] - s[b]);
                        pts.push(p[a].lerp(p[b], f));
                    }
                }
                if pts.len() == 2 && (pts[0] - pts[1]).length_squared() > 1e-12 {
                    out.push([pts[0], pts[1]]);
                }
            }
        }
    }
    out
}

impl Merge360App {
    fn drawing_cache(&mut self) -> Option<&DrawingCache> {
        let tab = &self.tabs[self.active];
        let meshes: Vec<std::sync::Arc<BodyMesh>> = tab.model.bodies.iter().filter(|b| tab.body_visible(b.id)).map(|b| b.mesh.clone()).collect();
        let key: Vec<usize> = meshes.iter().map(|m| std::sync::Arc::as_ptr(m) as usize).collect();
        if meshes.is_empty() {
            self.drawing = None;
            return None;
        }
        if self.drawing.as_ref().is_none_or(|d| d.key != key) {
            let refs: Vec<&BodyMesh> = meshes.iter().map(|m| m.as_ref()).collect();
            let views: Vec<View2D> = [ViewKind::Front, ViewKind::Top, ViewKind::Right, ViewKind::Iso].into_iter().map(|k| project(&refs, k)).collect();
            // Fit the 2x2 layout into the drawing area.
            let area = Vec2::new(SHEET.x - 2.0 * MARGIN, SHEET.y - 2.0 * MARGIN - TITLE_H);
            let w = views[0].max.x - views[0].min.x + views[2].max.x - views[2].min.x;
            let h = views[0].max.y - views[0].min.y + views[1].max.y - views[1].min.y;
            let need = (w / (area.x * 0.7)).max(h / (area.y * 0.7));
            let scale = SCALES.iter().copied().find(|s| 1.0 / s >= need).unwrap_or(0.01);
            self.drawing = Some(DrawingCache { key, views, scale });
        }
        self.drawing.as_ref()
    }

    /// Placement (sheet mm) of each view's min corner, third-angle layout.
    fn layout(d: &DrawingCache) -> Vec<Vec2> {
        let s = d.scale;
        let size = |v: &View2D| (v.max - v.min) * s;
        let (front, top, right, iso) = (size(&d.views[0]), size(&d.views[1]), size(&d.views[2]), size(&d.views[3]));
        let area_min = Vec2::new(MARGIN, MARGIN + TITLE_H);
        let area = Vec2::new(SHEET.x - 2.0 * MARGIN, SHEET.y - 2.0 * MARGIN - TITLE_H);
        let gap_x = ((area.x - front.x - right.x.max(iso.x)) / 3.0).max(5.0);
        let gap_y = ((area.y - front.y - top.y.max(iso.y)) / 3.0).max(5.0);
        // Sheet coordinates: origin bottom-left, y up.
        let fx = area_min.x + gap_x;
        let fy = area_min.y + gap_y;
        vec![
            Vec2::new(fx, fy),
            Vec2::new(fx, fy + front.y + gap_y),
            Vec2::new(fx + front.x + gap_x, fy),
            Vec2::new(fx + front.x + gap_x, fy + front.y + gap_y),
        ]
    }

    pub(crate) fn drawing_ui(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(0x8f, 0x93, 0x99));
        let resp = ui.interact(rect, ui.id().with("drawing"), Sense::click_and_drag());
        // Navigation: wheel zoom, middle/primary drag pan.
        let fit = (rect.width() / (SHEET.x + 40.0)).min(rect.height() / (SHEET.y + 40.0));
        let (mut zoom, mut pan) = self.drawing_view;
        if zoom <= 0.0 {
            zoom = 1.0;
            pan = Vec2::ZERO;
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                zoom = (zoom * (scroll * 0.0025).exp()).clamp(0.2, 20.0);
            }
        }
        if resp.dragged() {
            let d = resp.drag_delta();
            pan += Vec2::new(d.x, d.y);
        }
        self.drawing_view = (zoom, pan);
        let k = fit * zoom;
        let origin = rect.center() + vec2(pan.x, pan.y) - vec2(SHEET.x, -SHEET.y) * k * 0.5;
        // Sheet mm (y up) -> screen.
        let to = move |p: Vec2| pos2(origin.x + p.x * k, origin.y - p.y * k);
        let painter = ui.painter_at(rect);
        let sheet = Rect::from_two_pos(to(Vec2::ZERO), to(SHEET));
        painter.rect_filled(sheet.translate(vec2(4.0, 4.0)), 0.0, Color32::from_black_alpha(60));
        painter.rect_filled(sheet, 0.0, Color32::WHITE);
        let ink = Color32::from_rgb(20, 20, 24);
        let frame = Rect::from_two_pos(to(Vec2::splat(MARGIN)), to(SHEET - Vec2::splat(MARGIN)));
        painter.rect_stroke(frame, 0.0, Stroke::new(1.2, ink), egui::StrokeKind::Inside);

        let name = self.tabs[self.active].doc.name.clone();
        let units = self.tabs[self.active].doc.units;
        let Some(d) = self.drawing_cache() else {
            painter.text(sheet.center(), egui::Align2::CENTER_CENTER, "The design has no visible bodies to draw.", egui::FontId::proportional(14.0), theme::TEXT_DIM);
            return;
        };
        let scale = d.scale;
        let place = Self::layout(d);
        let thin = Stroke::new(1.0, ink);
        for (v, at) in d.views.iter().zip(&place) {
            let map = |p: Vec2| to(*at + (p - v.min) * scale);
            for [a, b] in &v.hidden {
                painter.add(egui::Shape::dashed_line(&[map(*a), map(*b)], Stroke::new(0.7, Color32::from_gray(150)), 4.0, 3.0));
            }
            for [a, b] in &v.visible {
                painter.line_segment([map(*a), map(*b)], thin);
            }
            let label_at = to(*at + Vec2::new((v.max.x - v.min.x) * scale * 0.5, if v.kind == ViewKind::Iso { -6.0 } else { -14.0 }));
            painter.text(label_at, egui::Align2::CENTER_CENTER, v.kind.label(), egui::FontId::proportional(3.5 * k), ink);
            // Overall dimensions for the orthographic views.
            if v.kind != ViewKind::Iso {
                let (w, h) = (v.max.x - v.min.x, v.max.y - v.min.y);
                let ds = Stroke::new(0.8, Color32::from_rgb(30, 60, 140));
                // Horizontal overall dimension below the view.
                let (p0, p1) = (map(v.min), map(Vec2::new(v.max.x, v.min.y)));
                let off = vec2(0.0, 6.0 * k);
                painter.line_segment([p0, p0 + off * 1.3], Stroke::new(0.5, ds.color));
                painter.line_segment([p1, p1 + off * 1.3], Stroke::new(0.5, ds.color));
                dim_line(&painter, p0 + off, p1 + off, 2.2 * k, ds);
                painter.text(p0 + off + (p1 - p0) * 0.5 - vec2(0.0, 0.8 * k), egui::Align2::CENTER_BOTTOM, units.format(w as f64), egui::FontId::proportional(3.0 * k), ds.color);
                // Vertical overall dimension right of the view.
                let (q0, q1) = (map(Vec2::new(v.max.x, v.min.y)), map(v.max));
                let off = vec2(6.0 * k, 0.0);
                painter.line_segment([q0, q0 + off * 1.3], Stroke::new(0.5, ds.color));
                painter.line_segment([q1, q1 + off * 1.3], Stroke::new(0.5, ds.color));
                dim_line(&painter, q0 + off, q1 + off, 2.2 * k, ds);
                painter.text(q0 + off + (q1 - q0) * 0.5 + vec2(1.0 * k, 0.0), egui::Align2::LEFT_CENTER, units.format(h as f64), egui::FontId::proportional(3.0 * k), ds.color);
            }
        }
        // Title block.
        let tb_min = Vec2::new(SHEET.x - MARGIN - TITLE_W, MARGIN);
        let tb = Rect::from_two_pos(to(tb_min), to(tb_min + Vec2::new(TITLE_W, TITLE_H)));
        painter.rect_stroke(tb, 0.0, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
        let row = |i: f32| to(tb_min + Vec2::new(3.0, TITLE_H - 6.0 - i * 9.0));
        let f = egui::FontId::proportional(3.4 * k);
        painter.text(row(0.0), egui::Align2::LEFT_CENTER, format!("TITLE: {name}"), egui::FontId::proportional(4.5 * k), ink);
        painter.text(row(1.0), egui::Align2::LEFT_CENTER, format!("SCALE: {}    UNITS: {}    SHEET: A3", scale_label(scale), units.label()), f.clone(), ink);
        painter.text(row(2.0), egui::Align2::LEFT_CENTER, format!("DATE: {}    DRAWN WITH MERGE360    PROJECTION: THIRD ANGLE", today()), f, ink);
    }

    /// DRAWING toolbar content.
    pub(crate) fn drawing_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            if theme::secondary_button(ui, "Update Views").clicked() {
                self.drawing = None;
            }
            if theme::secondary_button(ui, "Fit Sheet").clicked() {
                self.drawing_view = (1.0, Vec2::ZERO);
            }
            if theme::primary_button(ui, "Export SVG…").clicked() {
                self.export_drawing_svg();
            }
            ui.label(egui::RichText::new("Views are generated from all visible bodies (Front, Top, Right, Isometric).").color(theme::TEXT_DIM));
        });
    }

    fn export_drawing_svg(&mut self) {
        let Some(d) = self.drawing_cache() else { return };
        let scale = d.scale;
        let place = Self::layout(d);
        let mut svg = String::new();
        let _ = writeln!(svg, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}mm" height="{}mm" viewBox="0 0 {} {}">"#, SHEET.x, SHEET.y, SHEET.x, SHEET.y);
        let _ = writeln!(svg, r#"<rect x="0" y="0" width="{}" height="{}" fill="white"/>"#, SHEET.x, SHEET.y);
        let fy = |y: f32| SHEET.y - y;
        let _ = writeln!(svg, r#"<rect x="{m}" y="{m}" width="{}" height="{}" fill="none" stroke="black" stroke-width="0.5"/>"#, SHEET.x - 2.0 * MARGIN, SHEET.y - 2.0 * MARGIN, m = MARGIN);
        for (v, at) in d.views.iter().zip(&place) {
            let map = |p: Vec2| *at + (p - v.min) * scale;
            for [a, b] in &v.hidden {
                let (a, b) = (map(*a), map(*b));
                let _ = writeln!(svg, r##"<line x1="{:.3}" y1="{:.3}" x2="{:.3}" y2="{:.3}" stroke="#999" stroke-width="0.18" stroke-dasharray="1.5 1"/>"##, a.x, fy(a.y), b.x, fy(b.y));
            }
            for [a, b] in &v.visible {
                let (a, b) = (map(*a), map(*b));
                let _ = writeln!(svg, r#"<line x1="{:.3}" y1="{:.3}" x2="{:.3}" y2="{:.3}" stroke="black" stroke-width="0.35"/>"#, a.x, fy(a.y), b.x, fy(b.y));
            }
            let c = *at + Vec2::new((v.max.x - v.min.x) * scale * 0.5, if v.kind == ViewKind::Iso { -6.0 } else { -14.0 });
            let _ = writeln!(svg, r#"<text x="{:.2}" y="{:.2}" font-size="3.5" text-anchor="middle" font-family="sans-serif">{}</text>"#, c.x, fy(c.y), v.kind.label());
        }
        let name = self.tabs[self.active].doc.name.clone();
        let units = self.tabs[self.active].doc.units;
        let tx = SHEET.x - MARGIN - TITLE_W;
        let _ = writeln!(svg, r#"<rect x="{tx}" y="{}" width="{TITLE_W}" height="{TITLE_H}" fill="none" stroke="black" stroke-width="0.35"/>"#, fy(MARGIN + TITLE_H));
        let lines = [
            format!("TITLE: {name}"),
            format!("SCALE: {}    UNITS: {}    SHEET: A3", scale_label(scale), units.label()),
            format!("DATE: {}    DRAWN WITH MERGE360    PROJECTION: THIRD ANGLE", today()),
        ];
        for (i, l) in lines.iter().enumerate() {
            let y = MARGIN + TITLE_H - 6.0 - i as f32 * 9.0;
            let _ = writeln!(svg, r#"<text x="{:.2}" y="{:.2}" font-size="3.4" font-family="sans-serif">{}</text>"#, tx + 3.0, fy(y) + 1.2, xml_escape(l));
        }
        svg.push_str("</svg>\n");
        let dir = self.default_dir();
        let path = dir.join(format!("{}-drawing.svg", name.replace(['/', '\\'], "_")));
        match std::fs::write(&path, svg) {
            Ok(()) => self.toast(ToastKind::Info, format!("Drawing exported to {}", path.display())),
            Err(e) => self.toast(ToastKind::Error, format!("Export failed: {e}")),
        }
    }
}

/// Dimension line with small arrowheads at both ends.
fn dim_line(p: &egui::Painter, a: egui::Pos2, b: egui::Pos2, head: f32, stroke: Stroke) {
    p.line_segment([a, b], stroke);
    let d = (b - a).normalized();
    let n = vec2(-d.y, d.x);
    for (tip, dir) in [(a, d), (b, -d)] {
        let base = tip + dir * head;
        p.add(egui::Shape::convex_polygon(vec![tip, base + n * head * 0.35, base - n * head * 0.35], stroke.color, Stroke::NONE));
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Today's date (UTC) as YYYY-MM-DD without external crates.
fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let days = secs.div_euclid(86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;
    use m360_kernel::{primitives::make_cylinder, tessellate, Frame};

    #[test]
    fn cylinder_views_have_silhouettes_and_hidden_lines() {
        let c = make_cylinder(&Frame::new(DVec3::ZERO, DVec3::X, DVec3::Z), 20.0, 30.0).unwrap();
        let m = tessellate(&c);
        let front = project(&[&m], ViewKind::Front);
        // Front view of a vertical cylinder: rectangle (2 rims + 2 silhouettes).
        assert!(front.visible.len() > 4, "{}", front.visible.len());
        assert!(((front.max.x - front.min.x) - 20.0).abs() < 0.2);
        assert!(((front.max.y - front.min.y) - 30.0).abs() < 0.2);
    }

    #[test]
    fn box_behind_box_is_hidden() {
        use m360_kernel::primitives::make_box;
        let front_box = make_box(&Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y), 40.0, 40.0, 10.0).unwrap();
        let back_box = make_box(&Frame::new(DVec3::new(0.0, 0.0, -30.0), DVec3::X, DVec3::Y), 10.0, 10.0, 10.0).unwrap();
        let (a, b) = (tessellate(&front_box), tessellate(&back_box));
        let v = project(&[&a, &b], ViewKind::Front);
        let hidden_len: f32 = v.hidden.iter().map(|s| (s[1] - s[0]).length()).sum();
        assert!(hidden_len > 35.0, "the small box outline (40 mm) is hidden: {hidden_len}");
    }

    #[test]
    fn date_format() {
        let d = today();
        assert_eq!(d.len(), 10);
        assert!(d.starts_with("20"));
    }
}
