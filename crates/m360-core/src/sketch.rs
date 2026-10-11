//! Sketch profile detection: closed loops of lines and circles, with holes.

use m360_kernel::profile::{Loop2, Region, Seg2};

use crate::feature::SketchEntity;

const MERGE_TOL: f64 = 1e-4;

fn same(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < MERGE_TOL
}

/// Find closed loops: every circle, plus every cycle of lines/arcs whose
/// vertices all have exactly two incident curves.
pub fn loops(entities: &[SketchEntity]) -> Vec<Loop2> {
    let mut out = Vec::new();
    let mut verts: Vec<[f64; 2]> = Vec::new();
    // (start vertex, end vertex, arc transit point)
    let mut edges: Vec<(usize, usize, Option<[f64; 2]>)> = Vec::new();
    let vid = |p: [f64; 2], verts: &mut Vec<[f64; 2]>| {
        if let Some(i) = verts.iter().position(|q| same(*q, p)) {
            i
        } else {
            verts.push(p);
            verts.len() - 1
        }
    };
    for e in entities {
        let (a, b, m) = match e {
            SketchEntity::Circle { c, r } if *r > 1e-9 => {
                out.push(Loop2::Circle { c: *c, r: *r });
                continue;
            }
            SketchEntity::Line { a, b } if !same(*a, *b) => (*a, *b, None),
            SketchEntity::Arc { a, m, b } if !same(*a, *b) => (*a, *b, Some(*m)),
            _ => continue,
        };
        let ia = vid(a, &mut verts);
        let ib = vid(b, &mut verts);
        let dup = edges.iter().any(|&(x, y, t)| t.is_none() && m.is_none() && ((x, y) == (ia, ib) || (x, y) == (ib, ia)));
        if !dup {
            edges.push((ia, ib, m));
        }
    }
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); verts.len()];
    for (k, &(a, b, _)) in edges.iter().enumerate() {
        adj[a].push(k);
        adj[b].push(k);
    }
    let mut used = vec![false; edges.len()];
    for start in 0..edges.len() {
        if used[start] {
            continue;
        }
        let (a, b, m0) = edges[start];
        if adj[a].len() != 2 || adj[b].len() != 2 {
            continue;
        }
        let mut chain = vec![a];
        let mut segs = vec![Seg2 { end: verts[b], transit: m0 }];
        let mut cur_edge = start;
        let mut cur = b;
        let mut seen = vec![start];
        let closed = loop {
            if cur == a {
                break true;
            }
            if adj[cur].len() != 2 {
                break false;
            }
            chain.push(cur);
            let next_edge = if adj[cur][0] == cur_edge { adj[cur][1] } else { adj[cur][0] };
            if seen.contains(&next_edge) {
                break false;
            }
            seen.push(next_edge);
            let (x, y, m) = edges[next_edge];
            cur = if x == cur { y } else { x };
            segs.push(Seg2 { end: verts[cur], transit: m });
            cur_edge = next_edge;
        };
        for e in &seen {
            used[*e] = true;
        }
        let has_arc = segs.iter().any(|s| s.transit.is_some());
        if closed && (chain.len() >= 3 || (has_arc && segs.len() >= 2)) {
            let lp = if has_arc { Loop2::Path(segs) } else { Loop2::Polygon(chain.iter().map(|&i| verts[i]).collect()) };
            if lp.area() > 1e-9 {
                out.push(lp);
            }
        }
    }
    out
}

/// Turn loops into regions: each loop minus the loops directly inside it.
pub fn profiles(entities: &[SketchEntity]) -> Vec<Region> {
    let ls = loops(entities);
    let inside = |inner: &Loop2, outer: &Loop2| {
        inner.area() < outer.area() && outer.contains(inner.sample()) && {
            // Also test the loop's centroid-ish point for circles touching edges.
            let pts = inner.points(16);
            pts.iter().all(|p| outer.contains(*p) || on_boundary(outer, *p))
        }
    };
    (0..ls.len())
        .map(|i| {
            let children: Vec<usize> = (0..ls.len()).filter(|&j| j != i && inside(&ls[j], &ls[i])).collect();
            // Direct children only: not contained in another child.
            let holes = children
                .iter()
                .filter(|&&j| !children.iter().any(|&k| k != j && inside(&ls[j], &ls[k])))
                .map(|&j| ls[j].clone())
                .collect();
            Region { outer: ls[i].clone(), holes }
        })
        .collect()
}

fn polyline_dist(pts: &[[f64; 2]], p: [f64; 2]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let a = pts[i];
            let b = pts[(i + 1) % n];
            let ab = [b[0] - a[0], b[1] - a[1]];
            let ap = [p[0] - a[0], p[1] - a[1]];
            let len2 = (ab[0] * ab[0] + ab[1] * ab[1]).max(1e-18);
            let t = ((ap[0] * ab[0] + ap[1] * ab[1]) / len2).clamp(0.0, 1.0);
            (a[0] + ab[0] * t - p[0]).hypot(a[1] + ab[1] * t - p[1])
        })
        .fold(f64::MAX, f64::min)
}

fn on_boundary(l: &Loop2, p: [f64; 2]) -> bool {
    match l {
        Loop2::Circle { c, r } => ((p[0] - c[0]).hypot(p[1] - c[1]) - r).abs() < 1e-6,
        Loop2::Polygon(pts) => polyline_dist(pts, p) < 1e-6,
        Loop2::Path(_) => polyline_dist(&l.points(256), p) < 1e-3,
    }
}

/// The 4 lines of an axis-aligned rectangle.
pub fn rectangle(a: [f64; 2], b: [f64; 2]) -> Vec<SketchEntity> {
    let c = [b[0], a[1]];
    let d = [a[0], b[1]];
    vec![
        SketchEntity::Line { a, b: c },
        SketchEntity::Line { a: c, b },
        SketchEntity::Line { a: b, b: d },
        SketchEntity::Line { a: d, b: a },
    ]
}

/// Vertices of a regular polygon inscribed in the circle (c, |v - c|),
/// starting at v.
pub fn polygon(c: [f64; 2], v: [f64; 2], sides: usize) -> Vec<SketchEntity> {
    let r = (v[0] - c[0]).hypot(v[1] - c[1]);
    let a0 = (v[1] - c[1]).atan2(v[0] - c[0]);
    let pts: Vec<[f64; 2]> = (0..sides)
        .map(|i| {
            let a = a0 + i as f64 / sides as f64 * std::f64::consts::TAU;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect();
    (0..sides).map(|i| SketchEntity::Line { a: pts[i], b: pts[(i + 1) % sides] }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_of_lines_and_arcs() {
        // A slot: two lines joined by two half circles.
        let e = vec![
            SketchEntity::Line { a: [0.0, 0.0], b: [10.0, 0.0] },
            SketchEntity::Arc { a: [10.0, 0.0], m: [12.0, 2.0], b: [10.0, 4.0] },
            SketchEntity::Line { a: [10.0, 4.0], b: [0.0, 4.0] },
            SketchEntity::Arc { a: [0.0, 4.0], m: [-2.0, 2.0], b: [0.0, 0.0] },
        ];
        let p = profiles(&e);
        assert_eq!(p.len(), 1);
        let expect = 40.0 + std::f64::consts::PI * 4.0;
        assert!((p[0].area() - expect).abs() < 0.2, "{}", p[0].area());
    }

    #[test]
    fn hexagon() {
        let p = profiles(&polygon([0.0, 0.0], [10.0, 0.0], 6));
        assert_eq!(p.len(), 1);
        assert!((p[0].area() - 1.5 * 3f64.sqrt() * 100.0).abs() < 1e-6);
    }

    #[test]
    fn rectangle_with_circle_hole() {
        let mut e = rectangle([0.0, 0.0], [10.0, 10.0]);
        e.push(SketchEntity::Circle { c: [5.0, 5.0], r: 2.0 });
        e.push(SketchEntity::Line { a: [20.0, 0.0], b: [30.0, 0.0] }); // open
        let p = profiles(&e);
        assert_eq!(p.len(), 2);
        let rect = p.iter().find(|r| matches!(r.outer, Loop2::Polygon(_))).unwrap();
        assert_eq!(rect.holes.len(), 1);
        assert!((rect.area() - (100.0 - std::f64::consts::PI * 4.0)).abs() < 1e-6);
    }

    #[test]
    fn triangle_loop() {
        let e = vec![
            SketchEntity::Line { a: [0.0, 0.0], b: [4.0, 0.0] },
            SketchEntity::Line { a: [4.0, 0.0], b: [0.0, 3.0] },
            SketchEntity::Line { a: [0.0, 3.0], b: [0.0, 0.0] },
        ];
        let l = loops(&e);
        assert_eq!(l.len(), 1);
        assert!((l[0].area() - 6.0).abs() < 1e-9);
    }
}

// ------------------------------------------------------------------ sketch editing (trim / offset / fillet)

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn add(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] + b[0], a[1] + b[1]]
}
fn mul(a: [f64; 2], k: f64) -> [f64; 2] {
    [a[0] * k, a[1] * k]
}
fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn norm(a: [f64; 2]) -> f64 {
    a[0].hypot(a[1])
}

/// Angle (radians, [0, 2π)) of `p` around `c`.
fn angle_of(c: [f64; 2], p: [f64; 2]) -> f64 {
    (p[1] - c[1]).atan2(p[0] - c[0]).rem_euclid(std::f64::consts::TAU)
}

/// Arc as (center, radius, start angle, ccw sweep).
fn arc_params(a: [f64; 2], m: [f64; 2], b: [f64; 2]) -> Option<([f64; 2], f64, f64, f64)> {
    let (c, r) = m360_kernel::profile::circumcircle(a, m, b)?;
    let (ta, tm, tb) = (angle_of(c, a), angle_of(c, m), angle_of(c, b));
    let tau = std::f64::consts::TAU;
    let ccw = |x: f64| (x - ta).rem_euclid(tau);
    // Express as a CCW arc from start to end.
    if ccw(tm) < ccw(tb) {
        Some((c, r, ta, ccw(tb)))
    } else {
        Some((c, r, tb, (ta - tb).rem_euclid(tau)))
    }
}

fn arc_point(c: [f64; 2], r: f64, t: f64) -> [f64; 2] {
    [c[0] + r * t.cos(), c[1] + r * t.sin()]
}

/// Arc entity from center/radius/start/sweep (CCW).
fn arc_entity(c: [f64; 2], r: f64, t0: f64, sweep: f64) -> SketchEntity {
    SketchEntity::Arc { a: arc_point(c, r, t0), m: arc_point(c, r, t0 + sweep / 2.0), b: arc_point(c, r, t0 + sweep) }
}

/// Intersection points of two entities.
pub fn intersections(e1: &SketchEntity, e2: &SketchEntity) -> Vec<[f64; 2]> {
    use SketchEntity::*;
    let circ = |e: &SketchEntity| -> Option<([f64; 2], f64)> {
        match e {
            Circle { c, r } => Some((*c, *r)),
            Arc { a, m, b } => m360_kernel::profile::circumcircle(*a, *m, *b),
            _ => None,
        }
    };
    let on = |e: &SketchEntity, p: [f64; 2]| -> bool {
        match e {
            Line { a, b } => {
                let d = sub(*b, *a);
                let t = dot(sub(p, *a), d) / dot(d, d).max(1e-18);
                (-1e-9..=1.0 + 1e-9).contains(&t)
            }
            Arc { a, m, b } => match arc_params(*a, *m, *b) {
                Some((c, _, t0, sw)) => (angle_of(c, p) - t0).rem_euclid(std::f64::consts::TAU) <= sw + 1e-9,
                None => false,
            },
            Circle { .. } => true,
            Point { .. } => false,
        }
    };
    let raw: Vec<[f64; 2]> = match (e1, e2) {
        (Line { a, b }, Line { a: c, b: d }) => {
            let (r, s) = (sub(*b, *a), sub(*d, *c));
            let den = cross(r, s);
            if den.abs() < 1e-12 {
                vec![]
            } else {
                let t = cross(sub(*c, *a), s) / den;
                vec![add(*a, mul(r, t))]
            }
        }
        (Line { a, b }, other) | (other, Line { a, b }) if circ(other).is_some() => {
            let (c, r) = circ(other).unwrap();
            let d = sub(*b, *a);
            let f = sub(*a, c);
            let (qa, qb, qc) = (dot(d, d), 2.0 * dot(f, d), dot(f, f) - r * r);
            let disc = qb * qb - 4.0 * qa * qc;
            if disc < 0.0 || qa < 1e-18 {
                vec![]
            } else {
                let sq = disc.sqrt();
                vec![add(*a, mul(d, (-qb - sq) / (2.0 * qa))), add(*a, mul(d, (-qb + sq) / (2.0 * qa)))]
            }
        }
        (x, y) if circ(x).is_some() && circ(y).is_some() => {
            let ((c1, r1), (c2, r2)) = (circ(x).unwrap(), circ(y).unwrap());
            let dvec = sub(c2, c1);
            let dist = norm(dvec);
            if dist < 1e-12 || dist > r1 + r2 || dist < (r1 - r2).abs() {
                vec![]
            } else {
                let a = (r1 * r1 - r2 * r2 + dist * dist) / (2.0 * dist);
                let h = (r1 * r1 - a * a).max(0.0).sqrt();
                let p = add(c1, mul(dvec, a / dist));
                let perp = [-dvec[1] / dist, dvec[0] / dist];
                vec![add(p, mul(perp, h)), sub(p, mul(perp, h))]
            }
        }
        _ => vec![],
    };
    raw.into_iter().filter(|p| on(e1, *p) && on(e2, *p)).collect()
}

/// Trim the piece of entity `idx` around `click` that lies between its
/// intersections with other entities. Returns the replacement entities
/// (empty: the whole entity is removed).
pub fn trim(entities: &[SketchEntity], idx: usize, click: [f64; 2]) -> Option<Vec<SketchEntity>> {
    let e = entities.get(idx)?;
    let cuts: Vec<[f64; 2]> = entities
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != idx)
        .flat_map(|(_, o)| intersections(e, o))
        .collect();
    match *e {
        SketchEntity::Line { a, b } => {
            let d = sub(b, a);
            let len2 = dot(d, d).max(1e-18);
            let tc = (dot(sub(click, a), d) / len2).clamp(0.0, 1.0);
            let mut ts: Vec<f64> = cuts.iter().map(|p| dot(sub(*p, a), d) / len2).filter(|t| *t > 1e-9 && *t < 1.0 - 1e-9).collect();
            ts.sort_by(f64::total_cmp);
            let lo = ts.iter().copied().filter(|t| *t < tc).last().unwrap_or(0.0);
            let hi = ts.iter().copied().find(|t| *t > tc).unwrap_or(1.0);
            let mut out = Vec::new();
            if lo > 0.0 {
                out.push(SketchEntity::Line { a, b: add(a, mul(d, lo)) });
            }
            if hi < 1.0 {
                out.push(SketchEntity::Line { a: add(a, mul(d, hi)), b });
            }
            Some(out)
        }
        SketchEntity::Circle { c, r } => {
            let mut ts: Vec<f64> = cuts.iter().map(|p| angle_of(c, *p)).collect();
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
            if ts.len() < 2 {
                return Some(vec![]);
            }
            let tc = angle_of(c, click);
            let tau = std::f64::consts::TAU;
            // Interval (CCW) that contains the click; keep the rest of the circle.
            let n = ts.len();
            for k in 0..n {
                let (t0, t1) = (ts[k], ts[(k + 1) % n]);
                let sweep = (t1 - t0).rem_euclid(tau);
                if (tc - t0).rem_euclid(tau) < sweep {
                    return Some(vec![arc_entity(c, r, t1, tau - sweep)]);
                }
            }
            Some(vec![])
        }
        SketchEntity::Arc { a, m, b } => {
            let (c, r, t0, sw) = arc_params(a, m, b)?;
            let tau = std::f64::consts::TAU;
            let rel = |p: [f64; 2]| (angle_of(c, p) - t0).rem_euclid(tau);
            let tc = rel(click).min(sw);
            let mut ts: Vec<f64> = cuts.iter().map(|p| rel(*p)).filter(|t| *t > 1e-9 && *t < sw - 1e-9).collect();
            ts.sort_by(f64::total_cmp);
            let lo = ts.iter().copied().filter(|t| *t < tc).last().unwrap_or(0.0);
            let hi = ts.iter().copied().find(|t| *t > tc).unwrap_or(sw);
            let mut out = Vec::new();
            if lo > 1e-9 {
                out.push(arc_entity(c, r, t0, lo));
            }
            if hi < sw - 1e-9 {
                out.push(arc_entity(c, r, t0 + hi, sw - hi));
            }
            Some(out)
        }
        SketchEntity::Point { .. } => Some(vec![]),
    }
}

/// Ordered chain of lines/arcs: (start, end, arc midpoint), and whether it is closed.
pub fn order_chain(entities: &[SketchEntity], chain: &[usize]) -> Option<(Vec<([f64; 2], [f64; 2], Option<[f64; 2]>)>, bool)> {
    let mut segs: Vec<([f64; 2], [f64; 2], Option<[f64; 2]>)> = Vec::new();
    for &i in chain {
        match entities.get(i)? {
            SketchEntity::Line { a, b } => segs.push((*a, *b, None)),
            SketchEntity::Arc { a, m, b } => segs.push((*a, *b, Some(*m))),
            _ => return None,
        }
    }
    let same = |p: [f64; 2], q: [f64; 2]| norm(sub(p, q)) < 1e-6;
    let degree = |p: [f64; 2], s: &[([f64; 2], [f64; 2], Option<[f64; 2]>)]| s.iter().filter(|x| same(x.0, p) || same(x.1, p)).count();
    let start = segs.iter().position(|x| degree(x.0, &segs) == 1 || degree(x.1, &segs) == 1).unwrap_or(0);
    let mut first = segs.remove(start);
    if degree(first.0, &segs) > 0 && degree(first.1, &segs) == 0 {
        first = (first.1, first.0, first.2);
    }
    let mut out = vec![first];
    while !segs.is_empty() {
        let tail = out.last().unwrap().1;
        let k = segs.iter().position(|x| same(x.0, tail) || same(x.1, tail))?;
        let x = segs.remove(k);
        out.push(if same(x.0, tail) { x } else { (x.1, x.0, x.2) });
    }
    let closed = out.len() > 1 && same(out[0].0, out.last().unwrap().1);
    Some((out, closed))
}

/// Offset an ordered chain to its left by `d` (negative: right), joining
/// consecutive offset curves at their intersections.
fn offset_chain(chain: &[([f64; 2], [f64; 2], Option<[f64; 2]>)], closed: bool, d: f64) -> Option<Vec<SketchEntity>> {
    // Offset curve representation.
    #[derive(Clone, Copy)]
    enum C {
        L([f64; 2], [f64; 2]),
        /// center, radius, start angle, signed sweep (+ = CCW)
        A([f64; 2], f64, f64, f64),
    }
    let left = |a: [f64; 2], b: [f64; 2]| {
        let v = sub(b, a);
        let l = norm(v).max(1e-12);
        [-v[1] / l, v[0] / l]
    };
    let mut cs: Vec<C> = Vec::new();
    for &(a, b, m) in chain {
        match m {
            None => {
                let n = mul(left(a, b), d);
                cs.push(C::L(add(a, n), add(b, n)));
            }
            Some(m) => {
                let (c, r) = m360_kernel::profile::circumcircle(a, m, b)?;
                // Traversal direction a -> m -> b.
                let ccw = cross(sub(m, a), sub(b, m)) > 0.0;
                // The left side of a CCW arc is towards the center.
                let nr = if ccw { r - d } else { r + d };
                if nr <= 1e-9 {
                    return None;
                }
                let tau = std::f64::consts::TAU;
                let (ta, tb, tm) = (angle_of(c, a), angle_of(c, b), angle_of(c, m));
                let sweep = if ccw {
                    (tm - ta).rem_euclid(tau) + (tb - tm).rem_euclid(tau)
                } else {
                    -((ta - tm).rem_euclid(tau) + (tm - tb).rem_euclid(tau))
                };
                cs.push(C::A(c, nr, ta, sweep));
            }
        }
    }
    let ends = |c: &C| match *c {
        C::L(a, b) => (a, b),
        C::A(c, r, t0, sw) => (arc_point(c, r, t0), arc_point(c, r, t0 + sw)),
    };
    // Join consecutive curves where they no longer meet.
    let n = cs.len();
    let joints = if closed { n } else { n - 1 };
    for k in 0..joints {
        let (i, j) = (k, (k + 1) % n);
        let (pi, pj) = (ends(&cs[i]).1, ends(&cs[j]).0);
        if norm(sub(pi, pj)) < 1e-6 {
            continue;
        }
        // Intersection of the (extended) curves nearest to the joint.
        let as_entity = |c: &C, extend: bool| match *c {
            C::L(a, b) => {
                let v = sub(b, a);
                if extend {
                    SketchEntity::Line { a: sub(a, mul(v, 100.0)), b: add(b, mul(v, 100.0)) }
                } else {
                    SketchEntity::Line { a, b }
                }
            }
            C::A(c, r, _, _) => SketchEntity::Circle { c, r },
        };
        let x = intersections(&as_entity(&cs[i], true), &as_entity(&cs[j], true));
        let target = mul(add(pi, pj), 0.5);
        let p = x.into_iter().min_by(|a, b| norm(sub(*a, target)).total_cmp(&norm(sub(*b, target))))?;
        let set_end = |c: &mut C, p: [f64; 2], at_start: bool| match c {
            C::L(a, b) => {
                if at_start {
                    *a = p
                } else {
                    *b = p
                }
            }
            C::A(cc, _, t0, sw) => {
                let t = angle_of(*cc, p);
                let tau = std::f64::consts::TAU;
                if at_start {
                    let end = *t0 + *sw;
                    *t0 = t;
                    let mut nsw = end - t;
                    nsw = if *sw >= 0.0 { nsw.rem_euclid(tau) } else { -(-nsw).rem_euclid(tau) };
                    *sw = nsw;
                } else {
                    let mut nsw = t - *t0;
                    nsw = if *sw >= 0.0 { nsw.rem_euclid(tau) } else { -(-nsw).rem_euclid(tau) };
                    *sw = nsw;
                }
            }
        };
        set_end(&mut cs[i], p, false);
        set_end(&mut cs[j], p, true);
    }
    Some(
        cs.into_iter()
            .map(|c| match c {
                C::L(a, b) => SketchEntity::Line { a, b },
                C::A(c, r, t0, sw) => SketchEntity::Arc { a: arc_point(c, r, t0), m: arc_point(c, r, t0 + sw / 2.0), b: arc_point(c, r, t0 + sw) },
            })
            .collect(),
    )
}

/// Offset curves by `d`. Closed chains grow outward for positive `d`;
/// open chains move to the left of their traversal direction; circles
/// become concentric.
pub fn offset(entities: &[SketchEntity], chain: &[usize], d: f64) -> Vec<SketchEntity> {
    if let [i] = chain {
        if let Some(SketchEntity::Circle { c, r }) = entities.get(*i) {
            return if r + d > 1e-9 { vec![SketchEntity::Circle { c: *c, r: r + d }] } else { vec![] };
        }
    }
    let Some((ordered, closed)) = order_chain(entities, chain) else { return vec![] };
    let side = if closed {
        // Outward is to the right of a CCW loop.
        let mut pts: Vec<[f64; 2]> = Vec::new();
        for &(a, b, m) in &ordered {
            pts.push(a);
            if let Some(m) = m {
                pts.extend(m360_kernel::profile::arc_points(a, m, b, 8));
            }
        }
        if Loop2::Polygon(pts).signed_area() > 0.0 { -1.0 } else { 1.0 }
    } else {
        1.0
    };
    offset_chain(&ordered, closed, side * d).unwrap_or_default()
}

/// Round the corner shared by lines `l1` and `l2` with radius `r`.
/// Returns (new l1, new l2, fillet arc).
pub fn fillet_corner(entities: &[SketchEntity], l1: usize, l2: usize, r: f64) -> Option<(SketchEntity, SketchEntity, SketchEntity)> {
    let (SketchEntity::Line { a: a1, b: b1 }, SketchEntity::Line { a: a2, b: b2 }) = (entities.get(l1)?, entities.get(l2)?) else {
        return None;
    };
    let close = |p: [f64; 2], q: [f64; 2]| norm(sub(p, q)) < 1e-6;
    // Corner and the far ends.
    let (corner, f1, c1_is_a) = if close(*a1, *a2) || close(*a1, *b2) { (*a1, *b1, true) } else if close(*b1, *a2) || close(*b1, *b2) { (*b1, *a1, false) } else { return None };
    let (f2, c2_is_a) = if close(*a2, corner) { (*b2, true) } else { (*a2, false) };
    let (d1, d2) = (sub(f1, corner), sub(f2, corner));
    let (l1n, l2n) = (norm(d1), norm(d2));
    let (u1, u2) = (mul(d1, 1.0 / l1n), mul(d2, 1.0 / l2n));
    let theta = dot(u1, u2).clamp(-1.0, 1.0).acos();
    if !(1e-3..std::f64::consts::PI - 1e-3).contains(&theta) || r <= 0.0 {
        return None;
    }
    let t = r / (theta / 2.0).tan();
    if t >= l1n || t >= l2n {
        return None;
    }
    let (p1, p2) = (add(corner, mul(u1, t)), add(corner, mul(u2, t)));
    let bis = {
        let s = add(u1, u2);
        mul(s, 1.0 / norm(s))
    };
    let center = add(corner, mul(bis, r / (theta / 2.0).sin()));
    let mid = sub(center, mul(bis, r));
    let n1 = if c1_is_a { SketchEntity::Line { a: p1, b: f1 } } else { SketchEntity::Line { a: f1, b: p1 } };
    let n2 = if c2_is_a { SketchEntity::Line { a: p2, b: f2 } } else { SketchEntity::Line { a: f2, b: p2 } };
    Some((n1, n2, SketchEntity::Arc { a: p1, m: mid, b: p2 }))
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    #[test]
    fn trim_crossing_lines() {
        let e = vec![
            SketchEntity::Line { a: [0.0, 0.0], b: [10.0, 0.0] },
            SketchEntity::Line { a: [5.0, -5.0], b: [5.0, 5.0] },
        ];
        let r = trim(&e, 0, [8.0, 0.0]).unwrap();
        assert_eq!(r, vec![SketchEntity::Line { a: [0.0, 0.0], b: [5.0, 0.0] }]);
        let r = trim(&e, 1, [5.0, 0.5]).unwrap();
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn trim_circle_by_line() {
        let e = vec![SketchEntity::Circle { c: [0.0, 0.0], r: 5.0 }, SketchEntity::Line { a: [-10.0, 0.0], b: [10.0, 0.0] }];
        let r = trim(&e, 0, [0.0, 5.0]).unwrap();
        assert_eq!(r.len(), 1);
        let SketchEntity::Arc { m, .. } = r[0] else { panic!() };
        assert!(m[1] < -4.9, "kept the lower half: {m:?}");
    }

    #[test]
    fn offset_rectangle_and_circle() {
        let mut e = rectangle([0.0, 0.0], [10.0, 10.0]);
        e.push(SketchEntity::Circle { c: [20.0, 0.0], r: 3.0 });
        let o = offset(&e, &[0, 1, 2, 3], -1.0);
        assert_eq!(o.len(), 4);
        assert!((profiles(&o)[0].area() - 64.0).abs() < 1e-6, "{}", profiles(&o)[0].area());
        let c = offset(&e, &[4], 2.0);
        assert_eq!(c, vec![SketchEntity::Circle { c: [20.0, 0.0], r: 5.0 }]);
    }

    #[test]
    fn offset_rounded_rectangle() {
        let e = rectangle([0.0, 0.0], [10.0, 10.0]);
        let (n1, n2, arc) = fillet_corner(&e, 0, 1, 2.0).unwrap();
        let mut e2 = e.clone();
        e2[0] = n1;
        e2[1] = n2;
        e2.push(arc);
        for d in [-1.0, 1.0] {
            let o = offset(&e2, &[0, 1, 2, 3, 4], d);
            let p = profiles(&o);
            assert_eq!(p.len(), 1, "d={d}");
            let side = 10.0 + 2.0 * d;
            let r = 2.0 + d;
            let expect = side * side - (1.0 - std::f64::consts::PI / 4.0) * r * r;
            assert!((p[0].area() - expect).abs() < 0.2, "d={d}: {} vs {expect}", p[0].area());
        }
    }

    #[test]
    fn fillet_rectangle_corner() {
        let e = rectangle([0.0, 0.0], [10.0, 10.0]);
        let (n1, n2, arc) = fillet_corner(&e, 0, 1, 2.0).unwrap();
        let mut e2 = e.clone();
        e2[0] = n1;
        e2[1] = n2;
        e2.push(arc);
        let p = profiles(&e2);
        assert_eq!(p.len(), 1);
        let expect = 100.0 - (4.0 - std::f64::consts::PI);
        assert!((p[0].area() - expect).abs() < 0.05, "{}", p[0].area());
    }
}
