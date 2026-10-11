//! Minimal DXF reader: 2D geometry of the ENTITIES section as sketch
//! entities (LINE, CIRCLE, ARC, LWPOLYLINE with bulges, POLYLINE/VERTEX,
//! POINT). Coordinates are taken as millimetres.

use crate::feature::SketchEntity;

/// One DXF entity as (group code, value) pairs.
struct Raw {
    kind: String,
    groups: Vec<(i32, String)>,
}

impl Raw {
    fn f(&self, code: i32) -> Option<f64> {
        self.groups.iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.trim().parse().ok())
    }
    fn i(&self, code: i32) -> i64 {
        self.groups.iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.trim().parse().ok()).unwrap_or(0)
    }
}

fn raw_entities(text: &str) -> Vec<Raw> {
    let lines: Vec<&str> = text.lines().collect();
    let mut pairs = Vec::with_capacity(lines.len() / 2);
    for c in lines.as_chunks::<2>().0 {
        if let Ok(code) = c[0].trim().parse::<i32>() {
            pairs.push((code, c[1].trim().to_string()));
        }
    }
    let mut out = Vec::new();
    let mut in_entities = false;
    let mut cur: Option<Raw> = None;
    for (i, (code, value)) in pairs.iter().enumerate() {
        if *code == 2 && i > 0 && pairs[i - 1] == (0, "SECTION".to_string()) {
            in_entities = value == "ENTITIES";
            continue;
        }
        if !in_entities {
            continue;
        }
        if *code == 0 {
            if let Some(r) = cur.take() {
                out.push(r);
            }
            if value == "ENDSEC" {
                in_entities = false;
            } else {
                cur = Some(Raw { kind: value.clone(), groups: Vec::new() });
            }
        } else if let Some(r) = &mut cur {
            r.groups.push((*code, value.clone()));
        }
    }
    out.extend(cur);
    out
}

/// Arc (or line) from `a` to `b` with DXF bulge (tan of a quarter of the
/// included angle; positive = counter-clockwise).
fn bulge_segment(a: [f64; 2], b: [f64; 2], bulge: f64) -> SketchEntity {
    if bulge.abs() < 1e-9 {
        return SketchEntity::Line { a, b };
    }
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let chord = dx.hypot(dy);
    // Sagitta: distance from the chord midpoint to the arc midpoint.
    let s = bulge * chord / 2.0;
    let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    // Right normal of a→b; a CCW arc bulges to the right of the chord.
    let n = [dy / chord, -dx / chord];
    SketchEntity::Arc { a, m: [mid[0] + n[0] * s, mid[1] + n[1] * s], b }
}

fn arc_from(c: [f64; 2], r: f64, a0: f64, a1: f64) -> SketchEntity {
    let mut a1 = a1;
    while a1 <= a0 {
        a1 += 360.0;
    }
    let p = |deg: f64| [c[0] + r * deg.to_radians().cos(), c[1] + r * deg.to_radians().sin()];
    SketchEntity::Arc { a: p(a0), m: p((a0 + a1) / 2.0), b: p(a1) }
}

fn polyline(pts: &[([f64; 2], f64)], closed: bool, out: &mut Vec<SketchEntity>) {
    let n = pts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for i in 0..segs {
        let (a, bulge) = pts[i];
        let b = pts[(i + 1) % n].0;
        if (a[0] - b[0]).hypot(a[1] - b[1]) > 1e-9 {
            out.push(bulge_segment(a, b, bulge));
        }
    }
}

/// Parse DXF text into sketch entities.
pub fn read_dxf(text: &str) -> Result<Vec<SketchEntity>, String> {
    let raws = raw_entities(text);
    if raws.is_empty() {
        return Err("no ENTITIES section found".into());
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < raws.len() {
        let r = &raws[i];
        let xy = |cx: i32, cy: i32| Some([r.f(cx)?, r.f(cy)?]);
        match r.kind.as_str() {
            "LINE" => {
                if let (Some(a), Some(b)) = (xy(10, 20), xy(11, 21)) {
                    out.push(SketchEntity::Line { a, b });
                }
            }
            "CIRCLE" => {
                if let (Some(c), Some(rad)) = (xy(10, 20), r.f(40)) {
                    out.push(SketchEntity::Circle { c, r: rad });
                }
            }
            "ARC" => {
                if let (Some(c), Some(rad), Some(a0), Some(a1)) = (xy(10, 20), r.f(40), r.f(50), r.f(51)) {
                    out.push(arc_from(c, rad, a0, a1));
                }
            }
            "POINT" => {
                if let Some(p) = xy(10, 20) {
                    out.push(SketchEntity::Point { p });
                }
            }
            "LWPOLYLINE" => {
                // Vertices: each 10 starts a vertex; a following 42 is its bulge.
                let mut pts: Vec<([f64; 2], f64)> = Vec::new();
                let mut x = None;
                for (code, v) in &r.groups {
                    let val: f64 = v.trim().parse().unwrap_or(0.0);
                    match code {
                        10 => x = Some(val),
                        20 => {
                            if let Some(x) = x.take() {
                                pts.push(([x, val], 0.0));
                            }
                        }
                        42 => {
                            if let Some(p) = pts.last_mut() {
                                p.1 = val;
                            }
                        }
                        _ => {}
                    }
                }
                polyline(&pts, r.i(70) & 1 == 1, &mut out);
            }
            "POLYLINE" => {
                let closed = r.i(70) & 1 == 1;
                let mut pts = Vec::new();
                i += 1;
                while i < raws.len() && raws[i].kind == "VERTEX" {
                    let v = &raws[i];
                    if let (Some(x), Some(y)) = (v.f(10), v.f(20)) {
                        pts.push(([x, y], v.f(42).unwrap_or(0.0)));
                    }
                    i += 1;
                }
                polyline(&pts, closed, &mut out);
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    if out.is_empty() {
        return Err("the file contains no supported 2D geometry (lines, arcs, circles, polylines)".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_entities() {
        let dxf = "0\nSECTION\n2\nHEADER\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
            0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n\
            0\nCIRCLE\n10\n5\n20\n5\n40\n2\n\
            0\nARC\n10\n0\n20\n0\n40\n10\n50\n0\n51\n90\n\
            0\nLWPOLYLINE\n90\n2\n70\n1\n10\n0\n20\n0\n42\n1\n10\n10\n20\n0\n\
            0\nENDSEC\n0\nEOF\n";
        let e = read_dxf(dxf).unwrap();
        assert_eq!(e.len(), 5);
        // Quarter arc midpoint at 45°.
        let SketchEntity::Arc { m, .. } = e[2] else { panic!() };
        assert!((m[0] - 10.0 * std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9);
        // Bulge 1 = CCW half circle from (0,0) to (10,0): midpoint below.
        let SketchEntity::Arc { m, .. } = e[3] else { panic!() };
        assert!((m[0] - 5.0).abs() < 1e-9 && (m[1] + 5.0).abs() < 1e-9, "{m:?}");
        // The closed polyline (half circle + line) is one profile.
        assert_eq!(crate::sketch::profiles(&e[3..]).len(), 1);
    }
}
