//! Sketch profile detection: closed loops of lines and circles, with holes.

use m360_kernel::profile::{Loop2, Region, Seg2};

use crate::feature::SketchEntity;

const MERGE_TOL: f64 = 1e-4;

/// Samples per spline span.
const SPLINE_STEPS: usize = 16;

/// Centripetal Catmull-Rom spline through `pts` (closed if the ends meet).
pub fn spline_points(pts: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let closed = same(pts[0], pts[n - 1]);
    let p: Vec<[f64; 2]> = if closed { pts[..n - 1].to_vec() } else { pts.to_vec() };
    let m = p.len();
    let get = |i: isize| -> [f64; 2] {
        if closed {
            p[i.rem_euclid(m as isize) as usize]
        } else if i < 0 {
            // Reflected end tangents.
            add(p[0], sub(p[0], p[1]))
        } else if i as usize >= m {
            add(p[m - 1], sub(p[m - 1], p[m - 2]))
        } else {
            p[i as usize]
        }
    };
    let spans = if closed { m } else { m - 1 };
    let mut out = vec![p[0]];
    for s in 0..spans as isize {
        let (p0, p1, p2, p3) = (get(s - 1), get(s), get(s + 1), get(s + 2));
        // Centripetal parametrisation (alpha = 0.5) avoids cusps and loops.
        let knot = |a: [f64; 2], b: [f64; 2]| norm(sub(b, a)).sqrt().max(1e-9);
        let (t1, t2, t3) = (knot(p0, p1), knot(p1, p2), knot(p2, p3));
        let (t0, t1, t2, t3) = (0.0, t1, t1 + t2, t1 + t2 + t3);
        for k in 1..=SPLINE_STEPS {
            let t = t1 + (t2 - t1) * k as f64 / SPLINE_STEPS as f64;
            let lerp = |a: [f64; 2], b: [f64; 2], ta: f64, tb: f64| add(mul(a, (tb - t) / (tb - ta)), mul(b, (t - ta) / (tb - ta)));
            let a1 = lerp(p0, p1, t0, t1);
            let a2 = lerp(p1, p2, t1, t2);
            let a3 = lerp(p2, p3, t2, t3);
            let b1 = lerp(a1, a2, t0, t2);
            let b2 = lerp(a2, a3, t1, t3);
            out.push(lerp(b1, b2, t1, t2));
        }
    }
    out
}

/// Points of an ellipse (closed, last point != first).
pub fn ellipse_points(c: [f64; 2], a: [f64; 2], b: f64, n: usize) -> Vec<[f64; 2]> {
    let la = norm(a).max(1e-12);
    let v = [-a[1] / la * b, a[0] / la * b];
    (0..n)
        .map(|i| {
            let t = i as f64 / n as f64 * std::f64::consts::TAU;
            add(c, add(mul(a, t.cos()), mul(v, t.sin())))
        })
        .collect()
}

/// Polyline of a free-form curve (spline or ellipse), else `None`.
pub fn curve_polyline(e: &SketchEntity) -> Option<(Vec<[f64; 2]>, bool)> {
    match e {
        SketchEntity::Spline { pts } if pts.len() >= 2 => {
            let closed = pts.len() >= 4 && same(pts[0], pts[pts.len() - 1]);
            Some((spline_points(pts), closed))
        }
        SketchEntity::Ellipse { c, a, b } => Some((ellipse_points(*c, *a, *b, 72), true)),
        _ => None,
    }
}

fn same(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < MERGE_TOL
}

/// Find closed loops: every circle, plus every cycle of lines/arcs whose
/// vertices all have exactly two incident curves. Loops are ordered by their
/// first entity, so profile indices stay put when unrelated geometry is added.
pub fn loops(entities: &[SketchEntity]) -> Vec<Loop2> {
    let mut keyed = loops_keyed(entities);
    keyed.sort_by_key(|(k, _)| *k);
    keyed.into_iter().map(|(_, l)| l).collect()
}

/// For documents saved before profiles were ordered by entity: the current index
/// of each profile, by its old index (closed curves first, then line/arc cycles).
pub fn legacy_profile_map(entities: &[SketchEntity]) -> Vec<usize> {
    let keyed = loops_keyed(entities);
    let mut order: Vec<usize> = (0..keyed.len()).collect();
    order.sort_by_key(|&i| keyed[i].0);
    let mut map = vec![0; keyed.len()];
    for (new, old) in order.into_iter().enumerate() {
        map[old] = new;
    }
    map
}

/// Loops in discovery order (closed curves, then cycles), each with its lowest entity index.
fn loops_keyed(entities: &[SketchEntity]) -> Vec<(usize, Loop2)> {
    let mut out = Vec::new();
    let mut verts: Vec<[f64; 2]> = Vec::new();
    // (start vertex, end vertex, arc transit point, entity index)
    let mut edges: Vec<(usize, usize, Option<[f64; 2]>, usize)> = Vec::new();
    let vid = |p: [f64; 2], verts: &mut Vec<[f64; 2]>| {
        if let Some(i) = verts.iter().position(|q| same(*q, p)) {
            i
        } else {
            verts.push(p);
            verts.len() - 1
        }
    };
    for (k, e) in entities.iter().enumerate() {
        if let Some((pts, closed)) = curve_polyline(e) {
            if closed {
                let mut p = pts;
                if p.len() > 1 && same(p[0], p[p.len() - 1]) {
                    p.pop();
                }
                out.push((k, Loop2::Polygon(p)));
            } else {
                // Open splines take part in loops as chains of short lines.
                for w in pts.windows(2) {
                    let (ia, ib) = (vid(w[0], &mut verts), vid(w[1], &mut verts));
                    if ia != ib {
                        edges.push((ia, ib, None, k));
                    }
                }
            }
            continue;
        }
        let (a, b, m) = match e {
            SketchEntity::Circle { c, r } if *r > 1e-9 => {
                out.push((k, Loop2::Circle { c: *c, r: *r }));
                continue;
            }
            SketchEntity::Line { a, b } if !same(*a, *b) => (*a, *b, None),
            SketchEntity::Arc { a, m, b } if !same(*a, *b) => (*a, *b, Some(*m)),
            _ => continue,
        };
        let ia = vid(a, &mut verts);
        let ib = vid(b, &mut verts);
        let dup = edges.iter().any(|&(x, y, t, _)| t.is_none() && m.is_none() && ((x, y) == (ia, ib) || (x, y) == (ib, ia)));
        if !dup {
            edges.push((ia, ib, m, k));
        }
    }
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); verts.len()];
    for (k, &(a, b, _, _)) in edges.iter().enumerate() {
        adj[a].push(k);
        adj[b].push(k);
    }
    let mut used = vec![false; edges.len()];
    for start in 0..edges.len() {
        if used[start] {
            continue;
        }
        let (a, b, m0, _) = edges[start];
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
            let (x, y, m, _) = edges[next_edge];
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
                let key = seen.iter().map(|&e| edges[e].3).min().unwrap_or(0);
                out.push((key, lp));
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

/// Indices of the "solid" profiles: regions at an even nesting depth (outer
/// shapes and islands inside holes), e.g. the letters of a text.
pub fn solid_profiles(entities: &[SketchEntity]) -> Vec<usize> {
    let regions = profiles(entities);
    (0..regions.len())
        .filter(|&i| {
            let probe = regions[i].outer.sample();
            let depth = regions
                .iter()
                .enumerate()
                .filter(|(j, r)| *j != i && r.outer.area() > regions[i].outer.area() && r.outer.contains(probe))
                .count();
            depth % 2 == 0
        })
        .collect()
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

/// Center-to-center slot: two lines and two half-circle arcs around the
/// segment `c1`-`c2` with half width `w`.
pub fn slot(c1: [f64; 2], c2: [f64; 2], w: f64) -> Vec<SketchEntity> {
    let d = [c2[0] - c1[0], c2[1] - c1[1]];
    let l = d[0].hypot(d[1]);
    if l < 1e-9 || w < 1e-9 {
        return Vec::new();
    }
    let n = [-d[1] / l * w, d[0] / l * w];
    let t = [d[0] / l * w, d[1] / l * w];
    let p = |c: [f64; 2], k: f64, v: [f64; 2]| [c[0] + k * v[0], c[1] + k * v[1]];
    vec![
        SketchEntity::Line { a: p(c1, -1.0, n), b: p(c2, -1.0, n) },
        SketchEntity::Arc { a: p(c2, -1.0, n), m: p(c2, 1.0, t), b: p(c2, 1.0, n) },
        SketchEntity::Line { a: p(c2, 1.0, n), b: p(c1, 1.0, n) },
        SketchEntity::Arc { a: p(c1, 1.0, n), m: p(c1, -1.0, t), b: p(c1, -1.0, n) },
    ]
}

/// Apply a point map to an entity; circle radii are multiplied by `rs`.
pub fn map_entity(e: &SketchEntity, f: impl Fn([f64; 2]) -> [f64; 2], rs: f64) -> SketchEntity {
    match *e {
        SketchEntity::Line { a, b } => SketchEntity::Line { a: f(a), b: f(b) },
        SketchEntity::Circle { c, r } => SketchEntity::Circle { c: f(c), r: r * rs },
        SketchEntity::Point { p } => SketchEntity::Point { p: f(p) },
        SketchEntity::Arc { a, m, b } => SketchEntity::Arc { a: f(a), m: f(m), b: f(b) },
        SketchEntity::Spline { ref pts } => SketchEntity::Spline { pts: pts.iter().map(|p| f(*p)).collect() },
        SketchEntity::Ellipse { c, a, b } => {
            let nc = f(c);
            SketchEntity::Ellipse { c: nc, a: sub(f(add(c, a)), nc), b: b * rs }
        }
    }
}

/// Mirror of `p` about the line through `a` and `b`.
pub fn mirror_point(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    let d = sub(b, a);
    let l2 = dot(d, d).max(1e-24);
    let foot = add(a, mul(d, dot(sub(p, a), d) / l2));
    sub(mul(foot, 2.0), p)
}

/// Copies of entities rotated around `center` (`count` instances over
/// `total_deg`, the originals being the first instance).
pub fn circular_pattern(entities: &[SketchEntity], targets: &[usize], center: [f64; 2], count: usize, total_deg: f64) -> Vec<SketchEntity> {
    let full = (total_deg.abs() - 360.0).abs() < 1e-9;
    let steps = if full { count } else { count.saturating_sub(1).max(1) };
    let mut out = Vec::new();
    for k in 1..count {
        let ang = (total_deg / steps as f64 * k as f64).to_radians();
        let (s, c) = ang.sin_cos();
        let rot = |p: [f64; 2]| {
            let q = sub(p, center);
            add(center, [q[0] * c - q[1] * s, q[0] * s + q[1] * c])
        };
        out.extend(targets.iter().filter_map(|&i| entities.get(i)).map(|e| map_entity(e, rot, 1.0)));
    }
    out
}

/// Copies of entities on a grid: `n1` along `d1` and `n2` along `d2`.
pub fn rectangular_pattern(entities: &[SketchEntity], targets: &[usize], d1: [f64; 2], n1: usize, d2: [f64; 2], n2: usize) -> Vec<SketchEntity> {
    let mut out = Vec::new();
    for j in 0..n2.max(1) {
        for i in 0..n1.max(1) {
            if i == 0 && j == 0 {
                continue;
            }
            let off = add(mul(d1, i as f64), mul(d2, j as f64));
            out.extend(targets.iter().filter_map(|&k| entities.get(k)).map(|e| map_entity(e, |p| add(p, off), 1.0)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn profile_indices_survive_added_circles() {
        let mut e = super::rectangle([0.0, 0.0], [10.0, 10.0]);
        let before = super::loops(&e);
        e.push(super::SketchEntity::Circle { c: [50.0, 0.0], r: 3.0 });
        let after = super::loops(&e);
        assert_eq!(after.len(), 2);
        assert!((after[0].area() - before[0].area()).abs() < 1e-9);
        assert_eq!(super::legacy_profile_map(&e), vec![1, 0]);
    }

    #[test]
    fn empty_chains_do_not_panic() {
        let e = super::rectangle([0.0, 0.0], [10.0, 10.0]);
        assert!(super::order_chain(&e, &[]).is_none());
        assert!(super::chain_curves(&e, &[]).is_none());
        assert!(super::offset(&e, &[], 1.0).is_empty());
    }

    #[test]
    fn slot_is_one_profile() {
        let e = super::slot([0.0, 0.0], [20.0, 0.0], 5.0);
        let p = super::profiles(&e);
        assert_eq!(p.len(), 1);
        let expect = 20.0 * 10.0 + std::f64::consts::PI * 25.0;
        assert!((p[0].area() - expect).abs() < 1.5, "{}", p[0].area());
    }

    #[test]
    fn patterns_and_mirror() {
        let e = vec![crate::SketchEntity::Circle { c: [10.0, 0.0], r: 1.0 }];
        assert_eq!(super::circular_pattern(&e, &[0], [0.0, 0.0], 6, 360.0).len(), 5);
        assert_eq!(super::rectangular_pattern(&e, &[0], [5.0, 0.0], 3, [0.0, 5.0], 2).len(), 5);
        let m = super::mirror_point([3.0, 1.0], [0.0, 0.0], [0.0, 1.0]);
        assert!((m[0] + 3.0).abs() < 1e-12 && (m[1] - 1.0).abs() < 1e-12);
    }

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
    // Free-form curves intersect through their polylines.
    for (x, y) in [(e1, e2), (e2, e1)] {
        if let Some((pts, closed)) = curve_polyline(x) {
            let n = pts.len();
            let segs = if closed { n } else { n - 1 };
            return (0..segs).flat_map(|i| intersections(&Line { a: pts[i], b: pts[(i + 1) % n] }, y)).collect();
        }
    }
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
            Point { .. } | Spline { .. } | Ellipse { .. } => false,
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
            let lo = ts.iter().copied().rfind(|t| *t < tc).unwrap_or(0.0);
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
            let lo = ts.iter().copied().rfind(|t| *t < tc).unwrap_or(0.0);
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
        SketchEntity::Point { .. } | SketchEntity::Spline { .. } | SketchEntity::Ellipse { .. } => Some(vec![]),
    }
}

/// Extend line `idx` from the end nearest `click` to the closest
/// intersection with another entity.
pub fn extend(entities: &[SketchEntity], idx: usize, click: [f64; 2]) -> Option<SketchEntity> {
    let SketchEntity::Line { a, b } = *entities.get(idx)? else { return None };
    let from_b = norm(sub(click, b)) <= norm(sub(click, a));
    let (end, other) = if from_b { (b, a) } else { (a, b) };
    let d = sub(end, other);
    let l = norm(d);
    if l < 1e-12 {
        return None;
    }
    let ray = SketchEntity::Line { a: end, b: add(end, mul(d, 1e5 / l)) };
    let hit = entities
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != idx)
        .flat_map(|(_, o)| intersections(&ray, o))
        .map(|p| (norm(sub(p, end)), p))
        .filter(|(t, _)| *t > 1e-7)
        .min_by(|x, y| x.0.total_cmp(&y.0))?
        .1;
    Some(if from_b { SketchEntity::Line { a, b: hit } } else { SketchEntity::Line { a: hit, b } })
}

/// Split entity `idx` at its intersections with the other entities.
pub fn break_at(entities: &[SketchEntity], idx: usize) -> Option<Vec<SketchEntity>> {
    let e = entities.get(idx)?;
    let cuts: Vec<[f64; 2]> =
        entities.iter().enumerate().filter(|(i, _)| *i != idx).flat_map(|(_, o)| intersections(e, o)).collect();
    let tau = std::f64::consts::TAU;
    let pieces = match *e {
        SketchEntity::Line { a, b } => {
            let d = sub(b, a);
            let len2 = dot(d, d).max(1e-18);
            let mut ts: Vec<f64> = cuts.iter().map(|p| dot(sub(*p, a), d) / len2).filter(|t| *t > 1e-9 && *t < 1.0 - 1e-9).collect();
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
            let mut ks = vec![0.0];
            ks.extend(ts);
            ks.push(1.0);
            ks.windows(2).map(|w| SketchEntity::Line { a: add(a, mul(d, w[0])), b: add(a, mul(d, w[1])) }).collect()
        }
        SketchEntity::Arc { a, m, b } => {
            let (c, r, t0, sw) = arc_params(a, m, b)?;
            let mut ts: Vec<f64> =
                cuts.iter().map(|p| (angle_of(c, *p) - t0).rem_euclid(tau)).filter(|t| *t > 1e-9 && *t < sw - 1e-9).collect();
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
            let mut ks = vec![0.0];
            ks.extend(ts);
            ks.push(sw);
            ks.windows(2).map(|w| arc_entity(c, r, t0 + w[0], w[1] - w[0])).collect()
        }
        SketchEntity::Circle { c, r } => {
            let mut ts: Vec<f64> = cuts.iter().map(|p| angle_of(c, *p)).collect();
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
            if ts.len() < 2 {
                return None;
            }
            let n = ts.len();
            (0..n).map(|k| arc_entity(c, r, ts[k], (ts[(k + 1) % n] - ts[k]).rem_euclid(tau))).collect()
        }
        _ => return None,
    };
    let pieces: Vec<SketchEntity> = pieces;
    (pieces.len() > 1).then_some(pieces)
}

/// Sketch entity approximating a projected polyline: a line, circle or arc
/// when the points fit one, else a fit-point spline.
pub fn entity_from_polyline(pts: &[[f64; 2]], tol: f64) -> Option<SketchEntity> {
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let (first, last) = (pts[0], pts[n - 1]);
    let closed = n > 3 && norm(sub(first, last)) < tol;
    if !closed {
        let d = sub(last, first);
        let l = norm(d);
        if l < tol {
            return None;
        }
        if pts.iter().all(|p| cross(d, sub(*p, first)).abs() / l < tol) {
            return Some(SketchEntity::Line { a: first, b: last });
        }
    }
    let mid = pts[n / 2];
    let probe = if closed { pts[n / 3] } else { mid };
    let third = if closed { pts[2 * n / 3] } else { last };
    if let Some((c, r)) = m360_kernel::profile::circumcircle(first, probe, third) {
        if pts.iter().all(|p| (norm(sub(*p, c)) - r).abs() < tol) {
            return Some(if closed { SketchEntity::Circle { c, r } } else { SketchEntity::Arc { a: first, m: mid, b: last } });
        }
    }
    // Spline through a reduced set of points.
    let step = (n / 24).max(1);
    let mut fit: Vec<[f64; 2]> = pts.iter().step_by(step).copied().collect();
    if norm(sub(*fit.last().unwrap(), last)) > 1e-9 {
        fit.push(last);
    }
    Some(SketchEntity::Spline { pts: fit })
}

/// Ordered curves of a connected chain of lines, arcs and splines (for
/// surfaces), or `None` if the entities do not form one chain.
pub fn chain_curves(entities: &[SketchEntity], chain: &[usize]) -> Option<Vec<m360_kernel::surface::Curve2>> {
    use m360_kernel::surface::Curve2;
    if chain.is_empty() {
        return None;
    }
    // (start, end, curve from start to end)
    let mut items: Vec<([f64; 2], [f64; 2], Curve2)> = Vec::new();
    for &i in chain {
        match entities.get(i)? {
            SketchEntity::Line { a, b } => items.push((*a, *b, Curve2::Line(*a, *b))),
            SketchEntity::Arc { a, m, b } => items.push((*a, *b, Curve2::Arc(*a, *m, *b))),
            e @ SketchEntity::Spline { .. } => {
                let (p, _) = curve_polyline(e)?;
                items.push((p[0], p[p.len() - 1], Curve2::Poly(p)))
            }
            _ => return None,
        }
    }
    let rev = |c: Curve2| match c {
        Curve2::Line(a, b) => Curve2::Line(b, a),
        Curve2::Arc(a, m, b) => Curve2::Arc(b, m, a),
        Curve2::Poly(mut p) => {
            p.reverse();
            Curve2::Poly(p)
        }
    };
    let same = |p: [f64; 2], q: [f64; 2]| norm(sub(p, q)) < 1e-6;
    let degree = |p: [f64; 2], s: &[([f64; 2], [f64; 2], Curve2)]| s.iter().filter(|x| same(x.0, p) || same(x.1, p)).count();
    let start = items.iter().position(|x| degree(x.0, &items) == 1 || degree(x.1, &items) == 1).unwrap_or(0);
    let mut first = items.remove(start);
    if degree(first.0, &items) > 0 && degree(first.1, &items) == 0 {
        first = (first.1, first.0, rev(first.2));
    }
    let mut tail = first.1;
    let mut out = vec![first.2];
    while !items.is_empty() {
        let k = items.iter().position(|x| same(x.0, tail) || same(x.1, tail))?;
        let x = items.remove(k);
        if same(x.0, tail) {
            tail = x.1;
            out.push(x.2);
        } else {
            tail = x.0;
            out.push(rev(x.2));
        }
    }
    Some(out)
}

/// A chain segment: (start, end, arc midpoint).
pub type ChainSeg = ([f64; 2], [f64; 2], Option<[f64; 2]>);

/// Ordered chain of lines/arcs, and whether it is closed.
pub fn order_chain(entities: &[SketchEntity], chain: &[usize]) -> Option<(Vec<ChainSeg>, bool)> {
    if chain.is_empty() {
        return None;
    }
    let mut segs: Vec<ChainSeg> = Vec::new();
    for &i in chain {
        match entities.get(i)? {
            SketchEntity::Line { a, b } => segs.push((*a, *b, None)),
            SketchEntity::Arc { a, m, b } => segs.push((*a, *b, Some(*m))),
            _ => return None,
        }
    }
    let same = |p: [f64; 2], q: [f64; 2]| norm(sub(p, q)) < 1e-6;
    let degree = |p: [f64; 2], s: &[ChainSeg]| s.iter().filter(|x| same(x.0, p) || same(x.1, p)).count();
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
fn offset_chain(chain: &[ChainSeg], closed: bool, d: f64) -> Option<Vec<SketchEntity>> {
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
    fn extend_break_and_fit() {
        let e = vec![
            SketchEntity::Line { a: [0.0, 0.0], b: [4.0, 0.0] },
            SketchEntity::Line { a: [10.0, -5.0], b: [10.0, 5.0] },
            SketchEntity::Circle { c: [10.0, 0.0], r: 2.0 },
        ];
        // Extends to the circle (nearest), not the line.
        assert_eq!(extend(&e, 0, [3.9, 0.0]), Some(SketchEntity::Line { a: [0.0, 0.0], b: [8.0, 0.0] }));
        assert_eq!(break_at(&e, 1).map(|v| v.len()), Some(3));
        let circle: Vec<[f64; 2]> = (0..=40).map(|i| {
            let t = i as f64 / 40.0 * std::f64::consts::TAU;
            [3.0 + 2.0 * t.cos(), 2.0 * t.sin()]
        }).collect();
        assert!(matches!(entity_from_polyline(&circle, 1e-3), Some(SketchEntity::Circle { .. })));
        let wave: Vec<[f64; 2]> = (0..30).map(|i| [i as f64, (i as f64 * 0.3).sin()]).collect();
        assert!(matches!(entity_from_polyline(&wave, 1e-3), Some(SketchEntity::Spline { .. })));
        // Splines and ellipses make profiles.
        let sp = vec![SketchEntity::Spline { pts: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]] }];
        assert_eq!(profiles(&sp).len(), 1);
        let el = vec![SketchEntity::Ellipse { c: [0.0, 0.0], a: [5.0, 0.0], b: 2.0 }];
        assert!((profiles(&el)[0].area() - std::f64::consts::PI * 10.0).abs() < 0.2);
    }

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
