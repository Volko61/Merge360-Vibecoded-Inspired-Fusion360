//! Sketch profile detection: closed loops of lines and circles, with holes.

use m360_kernel::profile::{Loop2, Region};

use crate::feature::SketchEntity;

const MERGE_TOL: f64 = 1e-4;

fn same(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < MERGE_TOL
}

/// Find closed loops: every circle, plus every cycle of lines whose
/// vertices all have exactly two incident lines.
pub fn loops(entities: &[SketchEntity]) -> Vec<Loop2> {
    let mut out = Vec::new();
    let mut verts: Vec<[f64; 2]> = Vec::new();
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let vid = |p: [f64; 2], verts: &mut Vec<[f64; 2]>| {
        if let Some(i) = verts.iter().position(|q| same(*q, p)) {
            i
        } else {
            verts.push(p);
            verts.len() - 1
        }
    };
    for e in entities {
        match e {
            SketchEntity::Circle { c, r } if *r > 1e-9 => out.push(Loop2::Circle { c: *c, r: *r }),
            SketchEntity::Line { a, b } if !same(*a, *b) => {
                let ia = vid(*a, &mut verts);
                let ib = vid(*b, &mut verts);
                if !edges.iter().any(|&(x, y)| (x, y) == (ia, ib) || (x, y) == (ib, ia)) {
                    edges.push((ia, ib));
                }
            }
            _ => {}
        }
    }
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); verts.len()];
    for (k, &(a, b)) in edges.iter().enumerate() {
        adj[a].push(k);
        adj[b].push(k);
    }
    let mut used = vec![false; edges.len()];
    for start in 0..edges.len() {
        if used[start] {
            continue;
        }
        let (a, b) = edges[start];
        if adj[a].len() != 2 || adj[b].len() != 2 {
            continue;
        }
        let mut chain = vec![a];
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
            let (x, y) = edges[next_edge];
            cur = if x == cur { y } else { x };
            cur_edge = next_edge;
        };
        for e in &seen {
            used[*e] = true;
        }
        if closed && chain.len() >= 3 {
            let poly = Loop2::Polygon(chain.iter().map(|&i| verts[i]).collect());
            if poly.area() > 1e-9 {
                out.push(poly);
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

#[cfg(test)]
mod tests {
    use super::*;

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
