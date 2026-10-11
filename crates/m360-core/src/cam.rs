//! Manufacture (CAM): milling setups, 2D toolpaths and G-code.
//!
//! The work coordinate system (WCS) looks down the world Y axis like a
//! 3-axis mill: machine X = world X, machine Y = -world Z, machine Z = world
//! Y. Its origin is the center of the top of the stock.

use cavalier_contours::polyline::{PlineSource, PlineSourceMut, PlineVertex, Polyline};
use cavalier_contours::shape_algorithms::{Shape, ShapeOffsetOptions};
use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::feature::SketchEntity;
use crate::model::Model;
use m360_kernel::profile::{Loop2, Region};

/// A milling tool.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CamTool {
    pub number: u32,
    pub diameter: f64,
    /// Cutting feed (mm/min).
    pub feed: f64,
    /// Plunge feed (mm/min).
    pub plunge: f64,
    pub rpm: f64,
}

impl Default for CamTool {
    fn default() -> Self {
        Self { number: 1, diameter: 6.0, feed: 800.0, plunge: 200.0, rpm: 12000.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CamOpKind {
    /// Face the stock top down to the model top.
    Face { stepover: f64 },
    /// Follow sketch profiles (outside, or inside for holes and slots).
    Contour { sketch: u64, profiles: Vec<usize>, depth: f64, inside: bool },
    /// Clear sketch profiles (islands are left standing).
    Pocket { sketch: u64, profiles: Vec<usize>, depth: f64, stepover: f64 },
    /// Drill at the centers of sketch circles and at sketch points.
    Drill { sketch: u64, depth: f64, peck: f64 },
}

impl CamOpKind {
    pub fn label(&self) -> &'static str {
        match self {
            CamOpKind::Face { .. } => "Face",
            CamOpKind::Contour { .. } => "2D Contour",
            CamOpKind::Pocket { .. } => "2D Pocket",
            CamOpKind::Drill { .. } => "Drill",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamOp {
    pub name: String,
    pub kind: CamOpKind,
    pub tool: CamTool,
    /// Maximum depth of cut per pass.
    pub stepdown: f64,
    /// Retract height above the stock top.
    pub clearance: f64,
}

/// A milling setup: stock around the visible bodies plus operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamSetup {
    pub name: String,
    /// Extra stock around the model on the sides.
    pub stock_side: f64,
    /// Extra stock above the model top.
    pub stock_top: f64,
    pub ops: Vec<CamOp>,
}

impl Default for CamSetup {
    fn default() -> Self {
        Self { name: "Setup1".into(), stock_side: 2.0, stock_top: 1.0, ops: Vec::new() }
    }
}

/// One toolpath move (WCS, mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Move {
    pub rapid: bool,
    pub p: [f64; 3],
}

/// Stock box and WCS of a setup.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stock {
    /// World point of the WCS origin (top center of the stock).
    pub origin: DVec3,
    /// Stock size in WCS (x, y, z).
    pub size: [f64; 3],
    /// WCS height of the model top (stock top is 0).
    pub model_top: f64,
}

impl Stock {
    pub fn of(setup: &CamSetup, model: &Model) -> Option<Stock> {
        let (mut lo, mut hi) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
        for b in &model.bodies {
            lo = lo.min(b.mesh.bbox_min.as_dvec3());
            hi = hi.max(b.mesh.bbox_max.as_dvec3());
        }
        if lo.x > hi.x {
            return None;
        }
        let s = setup.stock_side.max(0.0);
        let t = setup.stock_top.max(0.0);
        let origin = DVec3::new((lo.x + hi.x) / 2.0, hi.y + t, (lo.z + hi.z) / 2.0);
        Some(Stock { origin, size: [hi.x - lo.x + 2.0 * s, hi.z - lo.z + 2.0 * s, hi.y - lo.y + t], model_top: -t })
    }

    /// World point to WCS.
    pub fn to_wcs(&self, w: DVec3) -> [f64; 3] {
        let d = w - self.origin;
        [d.x, -d.z, d.y]
    }

    /// WCS point to world.
    pub fn to_world(&self, p: [f64; 3]) -> DVec3 {
        self.origin + DVec3::new(p[0], p[2], -p[1])
    }
}

/// Signed sweep (radians, CCW positive) of the arc a -> m -> b.
fn arc_sweep(a: [f64; 2], m: [f64; 2], b: [f64; 2]) -> Option<f64> {
    let (c, _) = m360_kernel::profile::circumcircle(a, m, b)?;
    let ang = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]);
    let tau = std::f64::consts::TAU;
    let ccw = (m[0] - a[0]) * (b[1] - m[1]) - (m[1] - a[1]) * (b[0] - m[0]) > 0.0;
    let s = (ang(b) - ang(a)).rem_euclid(tau);
    Some(if ccw { s } else { s - tau })
}

/// Closed polyline (with arc bulges) of a sketch loop mapped by `f`.
fn loop_pline(l: &Loop2, f: &impl Fn([f64; 2]) -> [f64; 2]) -> Polyline<f64> {
    let mut p = Polyline::new_closed();
    match l {
        Loop2::Polygon(pts) => {
            for q in pts {
                let w = f(*q);
                p.add(w[0], w[1], 0.0);
            }
        }
        Loop2::Circle { c, r } => {
            // Two half circles (direction fixed below by the area sign).
            let (a, b, m1, m2) = (f([c[0] + r, c[1]]), f([c[0] - r, c[1]]), f([c[0], c[1] + r]), f([c[0], c[1] - r]));
            let s1 = arc_sweep(a, m1, b).unwrap_or(std::f64::consts::PI);
            let s2 = arc_sweep(b, m2, a).unwrap_or(std::f64::consts::PI);
            p.add(a[0], a[1], (s1 / 4.0).tan());
            p.add(b[0], b[1], (s2 / 4.0).tan());
        }
        Loop2::Path(segs) => {
            let n = segs.len();
            for i in 0..n {
                let start = f(segs[(i + n - 1) % n].end);
                let bulge = match segs[i].transit {
                    Some(m) => arc_sweep(start, f(m), f(segs[i].end)).map_or(0.0, |s| (s / 4.0).tan()),
                    None => 0.0,
                };
                p.add(start[0], start[1], bulge);
            }
        }
    }
    p
}

fn oriented(mut p: Polyline<f64>, ccw: bool) -> Polyline<f64> {
    if (p.area() > 0.0) != ccw {
        p.invert_direction_mut();
    }
    p
}

/// Points along a closed polyline (arcs sampled every ~5°), closed back to
/// the start.
fn pline_points(p: &Polyline<f64>) -> Vec<[f64; 2]> {
    let n = p.vertex_count();
    let mut out = Vec::new();
    for i in 0..n {
        let (v, w): (PlineVertex<f64>, PlineVertex<f64>) = (p.at(i), p.at((i + 1) % n));
        out.push([v.x, v.y]);
        if v.bulge.abs() > 1e-9 {
            let sweep = 4.0 * v.bulge.atan();
            let chord = [w.x - v.x, w.y - v.y];
            let l = chord[0].hypot(chord[1]);
            if l < 1e-12 {
                continue;
            }
            let r = l / (2.0 * (sweep / 2.0).sin().abs());
            // Center: from the chord midpoint along its normal.
            let h = r * (sweep / 2.0).cos().abs();
            let mid = [(v.x + w.x) / 2.0, (v.y + w.y) / 2.0];
            let nrm = [-chord[1] / l, chord[0] / l];
            let side = if (sweep.abs() < std::f64::consts::PI) == (sweep > 0.0) { 1.0 } else { -1.0 };
            let c = [mid[0] + nrm[0] * h * side, mid[1] + nrm[1] * h * side];
            let a0 = (v.y - c[1]).atan2(v.x - c[0]);
            let steps = ((sweep.abs().to_degrees() / 5.0).ceil() as usize).max(2);
            for k in 1..steps {
                let a = a0 + sweep * k as f64 / steps as f64;
                out.push([c[0] + r * a.cos(), c[1] + r * a.sin()]);
            }
        }
    }
    if let Some(&first) = out.first() {
        out.push(first);
    }
    out
}

/// A region as polylines: CCW outer loop and CW holes.
type PlineRegion = (Polyline<f64>, Vec<Polyline<f64>>);

/// Sketch regions in WCS (x, y) plus the WCS height of the sketch plane.
fn sketch_regions(model: &Model, sketch: u64, profiles: &[usize], stock: &Stock) -> Result<(Vec<PlineRegion>, f64), String> {
    let sk = model.sketches.iter().find(|s| s.uid == sketch).ok_or("the sketch of this operation no longer exists")?;
    let frame = sk.plane.frame([0.0, 0.0]);
    if frame.n.y.abs() < 0.999 {
        return Err("the sketch must be on a horizontal plane (seen from the top)".into());
    }
    let top = stock.to_wcs(frame.origin)[2];
    let map = |q: [f64; 2]| {
        let w = stock.to_wcs(frame.origin + frame.u * q[0] + frame.v * q[1]);
        [w[0], w[1]]
    };
    let regions: Vec<Region> = crate::sketch::profiles(&sk.entities);
    let mut out = Vec::new();
    for &i in profiles {
        let r = regions.get(i).ok_or("a profile of this operation no longer exists")?;
        let outer = oriented(loop_pline(&r.outer, &map), true);
        let holes = r.holes.iter().map(|h| oriented(loop_pline(h, &map), false)).collect();
        out.push((outer, holes));
    }
    if out.is_empty() {
        return Err("select profiles".into());
    }
    Ok((out, top))
}

/// Depths of the passes from `top` down to `bottom`.
fn levels(top: f64, bottom: f64, stepdown: f64) -> Vec<f64> {
    let total = (top - bottom).max(0.0);
    let n = (total / stepdown.max(0.01)).ceil().max(1.0) as usize;
    (1..=n).map(|k| top - total * k as f64 / n as f64).collect()
}

/// Cut a closed loop at depth `z`: rapid above its start, plunge, follow, retract.
fn cut_loop(out: &mut Vec<Move>, pts: &[[f64; 2]], z: f64, safe: f64) {
    let Some(&s) = pts.first() else { return };
    out.push(Move { rapid: true, p: [s[0], s[1], safe] });
    out.push(Move { rapid: false, p: [s[0], s[1], z] });
    for q in &pts[1..] {
        out.push(Move { rapid: false, p: [q[0], q[1], z] });
    }
    out.push(Move { rapid: true, p: [pts[pts.len() - 1][0], pts[pts.len() - 1][1], safe] });
}

/// Toolpath of one operation.
pub fn toolpath(setup: &CamSetup, op: &CamOp, model: &Model) -> Result<Vec<Move>, String> {
    let stock = Stock::of(setup, model).ok_or("there is no body to machine")?;
    let r = op.tool.diameter / 2.0;
    if r <= 0.0 {
        return Err("the tool diameter must be positive".into());
    }
    let safe = op.clearance.max(1.0);
    let mut out = Vec::new();
    let opts = ShapeOffsetOptions::default();
    match &op.kind {
        CamOpKind::Face { stepover } => {
            let step = (op.tool.diameter * stepover.clamp(0.05, 0.95)).max(0.01);
            let (hx, hy) = (stock.size[0] / 2.0 + r, stock.size[1] / 2.0 + r);
            for z in levels(0.0, stock.model_top, op.stepdown) {
                let mut y = -hy + r * 0.5;
                let mut forward = true;
                out.push(Move { rapid: true, p: [-hx - r, y, safe] });
                out.push(Move { rapid: false, p: [-hx - r, y, z] });
                while y <= hy {
                    let (x0, x1) = if forward { (-hx - r, hx + r) } else { (hx + r, -hx - r) };
                    out.push(Move { rapid: false, p: [x0, y, z] });
                    out.push(Move { rapid: false, p: [x1, y, z] });
                    y += step;
                    forward = !forward;
                }
                out.push(Move { rapid: true, p: [out.last().unwrap().p[0], out.last().unwrap().p[1], safe] });
            }
        }
        CamOpKind::Contour { sketch, profiles, depth, inside } => {
            let (regions, top) = sketch_regions(model, *sketch, profiles, &stock)?;
            for z in levels(top, top - depth.abs(), op.stepdown) {
                for (outer, holes) in &regions {
                    // Outside: offset the outer loop out; inside: offset holes in
                    // (and the outer loop in when the profile has no holes).
                    let mut loops: Vec<Polyline<f64>> = Vec::new();
                    if *inside {
                        if holes.is_empty() {
                            loops.extend(outer.parallel_offset(r));
                        }
                        for h in holes {
                            loops.extend(oriented(h.clone(), true).parallel_offset(r));
                        }
                    } else {
                        loops.extend(outer.parallel_offset(-r));
                    }
                    if loops.is_empty() {
                        return Err("the tool is too large for this profile".into());
                    }
                    for l in &loops {
                        cut_loop(&mut out, &pline_points(l), z, safe);
                    }
                }
            }
        }
        CamOpKind::Pocket { sketch, profiles, depth, stepover } => {
            let (regions, top) = sketch_regions(model, *sketch, profiles, &stock)?;
            let step = (op.tool.diameter * stepover.clamp(0.05, 0.95)).max(0.01);
            for z in levels(top, top - depth.abs(), op.stepdown) {
                for (outer, holes) in &regions {
                    let mut plines = vec![outer.clone()];
                    plines.extend(holes.iter().cloned());
                    let shape = Shape::from_plines(plines);
                    // Offset rings from the wall inwards.
                    let mut rings: Vec<Vec<[f64; 2]>> = Vec::new();
                    let mut cur = shape.parallel_offset(r, &opts);
                    let mut guard = 0;
                    while !(cur.ccw_plines.is_empty() && cur.cw_plines.is_empty()) && guard < 2000 {
                        for p in cur.ccw_plines.iter().chain(cur.cw_plines.iter()) {
                            rings.push(pline_points(&p.polyline));
                        }
                        cur = cur.parallel_offset(step, &opts);
                        guard += 1;
                    }
                    if rings.is_empty() {
                        return Err("the tool is too large for this pocket".into());
                    }
                    // Center first, then outwards to the wall.
                    for ring in rings.iter().rev() {
                        cut_loop(&mut out, ring, z, safe);
                    }
                }
            }
        }
        CamOpKind::Drill { sketch, depth, peck } => {
            let sk = model.sketches.iter().find(|s| s.uid == *sketch).ok_or("the sketch of this operation no longer exists")?;
            let frame = sk.plane.frame([0.0, 0.0]);
            if frame.n.y.abs() < 0.999 {
                return Err("the sketch must be on a horizontal plane (seen from the top)".into());
            }
            let pts: Vec<[f64; 2]> = sk
                .entities
                .iter()
                .filter_map(|e| match e {
                    SketchEntity::Circle { c, .. } | SketchEntity::Point { p: c } => Some(*c),
                    _ => None,
                })
                .collect();
            if pts.is_empty() {
                return Err("the sketch has no circles or points to drill".into());
            }
            for q in pts {
                let w = stock.to_wcs(frame.origin + frame.u * q[0] + frame.v * q[1]);
                let (top, bottom) = (w[2], w[2] - depth.abs());
                out.push(Move { rapid: true, p: [w[0], w[1], safe] });
                // Peck: cut, retract to clear chips, rapid back down, continue.
                let mut z = top;
                let peck = if *peck > 0.01 { *peck } else { depth.abs() };
                while z > bottom + 1e-9 {
                    let next = (z - peck).max(bottom);
                    out.push(Move { rapid: true, p: [w[0], w[1], z + 0.5_f64.min(safe)] });
                    out.push(Move { rapid: false, p: [w[0], w[1], next] });
                    out.push(Move { rapid: true, p: [w[0], w[1], safe] });
                    z = next;
                }
            }
        }
    }
    Ok(out)
}

/// Estimated machining time (minutes): feeds at the cutting feed, rapids at 5 m/min.
pub fn machining_time(moves: &[Move], tool: &CamTool) -> f64 {
    let mut t = 0.0;
    for w in moves.windows(2) {
        let d = DVec3::from(w[1].p) - DVec3::from(w[0].p);
        let feed = if w[1].rapid { 5000.0 } else if d.z < -1e-9 && d.x.abs() + d.y.abs() < 1e-9 { tool.plunge } else { tool.feed };
        t += d.length() / feed.max(1.0);
    }
    t
}

/// Generic (Fanuc / GRBL style) G-code of a setup.
pub fn gcode(setup: &CamSetup, model: &Model) -> Result<String, String> {
    use std::fmt::Write;
    let mut g = String::new();
    let f = |v: f64| format!("{:.3}", if v.abs() < 5e-4 { 0.0 } else { v });
    writeln!(g, "%").ok();
    writeln!(g, "(Merge360 - {})", setup.name).ok();
    writeln!(g, "(WCS: X/Y on the stock top center, Z up, millimeters)").ok();
    writeln!(g, "G21 G90 G17 G94").ok();
    for op in &setup.ops {
        let moves = toolpath(setup, op, model).map_err(|e| format!("{}: {e}", op.name))?;
        writeln!(g, "({} - D{} mm)", op.name, op.tool.diameter).ok();
        writeln!(g, "T{} M6", op.tool.number).ok();
        writeln!(g, "S{:.0} M3", op.tool.rpm).ok();
        writeln!(g, "G0 Z{}", f(op.clearance.max(1.0))).ok();
        let mut last = [f64::NAN; 3];
        for m in &moves {
            let plunge = !m.rapid && (m.p[0] - last[0]).abs() < 1e-9 && (m.p[1] - last[1]).abs() < 1e-9 && m.p[2] < last[2];
            if m.rapid {
                writeln!(g, "G0 X{} Y{} Z{}", f(m.p[0]), f(m.p[1]), f(m.p[2])).ok();
            } else {
                let feed = if plunge { op.tool.plunge } else { op.tool.feed };
                writeln!(g, "G1 X{} Y{} Z{} F{:.0}", f(m.p[0]), f(m.p[1]), f(m.p[2]), feed).ok();
            }
            last = m.p;
        }
        writeln!(g, "M5").ok();
    }
    writeln!(g, "G0 Z{}", f(setup.ops.first().map_or(10.0, |o| o.clearance.max(1.0)))).ok();
    writeln!(g, "M30").ok();
    writeln!(g, "%").ok();
    Ok(g)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::feature::{ExtrudeOp, FeatureKind};
    use crate::PlaneRef;

    /// A 40 x 30 x 10 plate with a 20 x 10 pocket sketch on its top.
    fn plate() -> (Document, u64, u64) {
        let mut d = Document::new("t");
        let base = d.add_feature(FeatureKind::Sketch {
            plane: PlaneRef::XZ,
            entities: crate::sketch::rectangle([-20.0, -15.0], [20.0, 15.0]),
            constraints: vec![],
        });
        d.add_feature(FeatureKind::Extrude {
            sketch: base,
            profiles: vec![0],
            distance: 10.0,
            symmetric: false,
            op: ExtrudeOp::NewBody,
            participants: vec![],
            taper: 0.0,
            faces: vec![],
        });
        let top = PlaneRef { offset: 10.0, ..PlaneRef::XZ };
        let mut ents = crate::sketch::rectangle([-10.0, -5.0], [10.0, 5.0]);
        ents.push(SketchEntity::Circle { c: [15.0, 10.0], r: 2.0 });
        let pocket = d.add_feature(FeatureKind::Sketch { plane: top, entities: ents, constraints: vec![] });
        (d, base, pocket)
    }

    #[test]
    fn toolpaths_and_gcode() {
        let (d, base, pocket) = plate();
        let model = crate::Regenerator::default().regenerate(&d);
        let mut setup = CamSetup::default();
        let tool = CamTool { diameter: 4.0, ..Default::default() };
        let prof = crate::sketch::profiles(&model.sketches.iter().find(|s| s.uid == pocket).unwrap().entities);
        let rect = prof.iter().position(|r| (r.area() - 200.0).abs() < 1e-6).unwrap();
        setup.ops.push(CamOp { name: "Pocket".into(), kind: CamOpKind::Pocket { sketch: pocket, profiles: vec![rect], depth: 3.0, stepover: 0.4 }, tool, stepdown: 1.5, clearance: 5.0 });
        setup.ops.push(CamOp { name: "Contour".into(), kind: CamOpKind::Contour { sketch: base, profiles: vec![0], depth: 10.0, inside: false }, tool, stepdown: 5.0, clearance: 5.0 });
        setup.ops.push(CamOp { name: "Drill".into(), kind: CamOpKind::Drill { sketch: pocket, depth: 5.0, peck: 2.0 }, tool, stepdown: 1.0, clearance: 5.0 });
        setup.ops.push(CamOp { name: "Face".into(), kind: CamOpKind::Face { stepover: 0.5 }, tool, stepdown: 1.0, clearance: 5.0 });
        // Pocket: never leaves the pocket (inset by the tool radius) and reaches the depth.
        let p = toolpath(&setup, &setup.ops[0], &model).unwrap();
        let feeds: Vec<&Move> = p.iter().filter(|m| !m.rapid).collect();
        assert!(feeds.iter().all(|m| m.p[0].abs() <= 10.0 - 2.0 + 1e-6 && m.p[1].abs() <= 5.0 - 2.0 + 1e-6), "pocket leaves its walls");
        let stock = Stock::of(&setup, &model).unwrap();
        let top = -setup.stock_top;
        assert!((stock.model_top - top).abs() < 1e-9);
        let lowest = feeds.iter().map(|m| m.p[2]).fold(f64::MAX, f64::min);
        assert!((lowest - (top - 3.0)).abs() < 1e-6, "{lowest}");
        // Contour: the outside offset runs at the tool radius from the 40 x 30 plate.
        let c = toolpath(&setup, &setup.ops[1], &model).unwrap();
        let maxx = c.iter().filter(|m| !m.rapid).map(|m| m.p[0]).fold(f64::MIN, f64::max);
        assert!((maxx - 22.0).abs() < 1e-6, "{maxx}");
        // Drill: one hole at (15, 10) in sketch coordinates = (15, 10) in the WCS.
        let dr = toolpath(&setup, &setup.ops[2], &model).unwrap();
        assert!(dr.iter().any(|m| (m.p[0] - 15.0).abs() < 1e-6 && (m.p[1] - 10.0).abs() < 1e-6 && !m.rapid));
        let g = gcode(&setup, &model).unwrap();
        assert!(g.contains("G21") && g.contains("M30") && g.contains("T1 M6"));
        assert!(machining_time(&p, &tool) > 0.0);
    }
}
