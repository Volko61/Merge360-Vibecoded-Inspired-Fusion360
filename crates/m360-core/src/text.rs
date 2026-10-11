//! Sketch text: glyph outlines (from the bundled Ubuntu font) as closed
//! polylines of sketch lines, ready to be extruded.

use crate::feature::SketchEntity;

struct Outline {
    scale: f64,
    origin: [f64; 2],
    cur: [f64; 2],
    start: [f64; 2],
    loops: Vec<Vec<[f64; 2]>>,
    pts: Vec<[f64; 2]>,
}

impl Outline {
    fn map(&self, x: f32, y: f32) -> [f64; 2] {
        [self.origin[0] + x as f64 * self.scale, self.origin[1] + y as f64 * self.scale]
    }
    fn push(&mut self, p: [f64; 2]) {
        if self.pts.last().is_none_or(|q| (q[0] - p[0]).hypot(q[1] - p[1]) > 1e-6) {
            self.pts.push(p);
        }
        self.cur = p;
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.map(x, y);
        self.start = p;
        self.pts.clear();
        self.push(p);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.map(x, y);
        self.push(p);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (p0, c, p) = (self.cur, self.map(x1, y1), self.map(x, y));
        for k in 1..=6 {
            let t = k as f64 / 6.0;
            let u = 1.0 - t;
            self.push([u * u * p0[0] + 2.0 * u * t * c[0] + t * t * p[0], u * u * p0[1] + 2.0 * u * t * c[1] + t * t * p[1]]);
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (p0, c1, c2, p) = (self.cur, self.map(x1, y1), self.map(x2, y2), self.map(x, y));
        for k in 1..=8 {
            let t = k as f64 / 8.0;
            let u = 1.0 - t;
            let b = |a: f64, b: f64, c: f64, d: f64| u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
            self.push([b(p0[0], c1[0], c2[0], p[0]), b(p0[1], c1[1], c2[1], p[1])]);
        }
    }
    fn close(&mut self) {
        let mut pts = std::mem::take(&mut self.pts);
        if pts.last().is_some_and(|q| (q[0] - self.start[0]).hypot(q[1] - self.start[1]) < 1e-6) {
            pts.pop();
        }
        if pts.len() >= 3 {
            self.loops.push(pts);
        }
    }
}

/// Sketch lines outlining `text` with its baseline starting at `origin`;
/// `height` is the cap height (mm).
pub fn text_entities(text: &str, origin: [f64; 2], height: f64) -> Vec<SketchEntity> {
    let Ok(face) = ttf_parser::Face::parse(epaint_default_fonts::UBUNTU_LIGHT, 0) else { return Vec::new() };
    let cap = face.capital_height().unwrap_or(face.ascender()) as f64;
    let scale = height / cap.max(1.0);
    let mut out = Vec::new();
    let mut x = origin[0];
    for ch in text.chars() {
        let Some(gid) = face.glyph_index(ch) else { continue };
        let mut ob = Outline { scale, origin: [x, origin[1]], cur: [0.0; 2], start: [0.0; 2], loops: Vec::new(), pts: Vec::new() };
        face.outline_glyph(gid, &mut ob);
        for lp in ob.loops {
            let n = lp.len();
            out.extend((0..n).map(|i| SketchEntity::Line { a: lp[i], b: lp[(i + 1) % n] }));
        }
        x += face.glyph_hor_advance(gid).unwrap_or(0) as f64 * scale;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_makes_profiles_with_holes() {
        let e = text_entities("OB", [0.0, 0.0], 10.0);
        assert!(!e.is_empty());
        let p = crate::sketch::profiles(&e);
        // O: outer + inner (2 regions), B: outer + 2 counters (3 regions).
        assert!(p.len() >= 5, "{}", p.len());
        let with_holes = p.iter().filter(|r| !r.holes.is_empty()).count();
        assert!(with_holes >= 2);
        // Only the letters themselves are solid (counters excluded).
        assert_eq!(crate::sketch::solid_profiles(&e).len(), 2);
    }
}
