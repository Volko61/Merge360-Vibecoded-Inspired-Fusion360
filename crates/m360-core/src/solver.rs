//! 2D geometric constraint solver for sketches.
//!
//! Variables are the sketch's point coordinates (points that coincide are
//! merged into one variable pair, so connected geometry stays connected) and
//! circle radii. Constraints are residual functions; they are solved with a
//! damped Gauss-Newton (Levenberg-Marquardt) iteration. A weak pull towards
//! the current positions keeps under-constrained geometry where it is.

use serde::{Deserialize, Serialize};

use crate::feature::SketchEntity;

/// A point of a sketch entity. `index`: line 0=a 1=b; circle 0=center;
/// point 0; arc 0=a 1=b 2=midpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PointRef {
    pub entity: usize,
    pub index: u8,
}

impl PointRef {
    pub fn new(entity: usize, index: u8) -> Self {
        Self { entity, index }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SketchConstraint {
    Coincident(PointRef, PointRef),
    Horizontal(usize),
    Vertical(usize),
    Parallel(usize, usize),
    Perpendicular(usize, usize),
    /// Equal length (lines) or radius (circles/arcs).
    Equal(usize, usize),
    /// Line tangent to a circle/arc.
    Tangent(usize, usize),
    Concentric(usize, usize),
    Fix(PointRef),
    /// Point-to-point distance (a line's length when both are its ends).
    Distance { a: PointRef, b: PointRef, value: f64 },
    /// Circle/arc diameter.
    Diameter { entity: usize, value: f64 },
    /// Point at the midpoint of a line.
    Midpoint(PointRef, usize),
    /// Second line on the infinite line through the first.
    Collinear(usize, usize),
    /// Two points mirrored about a line.
    Symmetry(PointRef, PointRef, usize),
}

impl SketchConstraint {
    /// Entities referenced by the constraint.
    pub fn entities(&self) -> Vec<usize> {
        use SketchConstraint::*;
        match self {
            Coincident(a, b) | Distance { a, b, .. } => vec![a.entity, b.entity],
            Horizontal(e) | Vertical(e) | Diameter { entity: e, .. } => vec![*e],
            Parallel(a, b) | Perpendicular(a, b) | Equal(a, b) | Tangent(a, b) | Concentric(a, b) => vec![*a, *b],
            Fix(p) => vec![p.entity],
            Midpoint(p, l) => vec![p.entity, *l],
            Collinear(a, b) => vec![*a, *b],
            Symmetry(a, b, l) => vec![a.entity, b.entity, *l],
        }
    }

    /// Short glyph shown next to constrained geometry.
    pub fn glyph(&self) -> &'static str {
        use SketchConstraint::*;
        match self {
            Coincident(..) => "●",
            Horizontal(_) => "H",
            Vertical(_) => "V",
            Parallel(..) => "∥",
            Perpendicular(..) => "⊥",
            Equal(..) => "=",
            Tangent(..) => "T",
            Concentric(..) => "◎",
            Fix(_) => "F",
            Midpoint(..) => "M",
            Collinear(..) => "C",
            Symmetry(..) => "S",
            Distance { .. } | Diameter { .. } => "",
        }
    }

    /// Remap entity indices after deleting `removed` (sorted ascending).
    /// Returns `None` if the constraint references a removed entity.
    pub fn remapped(&self, removed: &[usize]) -> Option<SketchConstraint> {
        let map = |e: usize| -> Option<usize> {
            if removed.binary_search(&e).is_ok() {
                None
            } else {
                Some(e - removed.iter().filter(|&&r| r < e).count())
            }
        };
        let pr = |p: &PointRef| Some(PointRef { entity: map(p.entity)?, index: p.index });
        use SketchConstraint::*;
        Some(match self {
            Coincident(a, b) => Coincident(pr(a)?, pr(b)?),
            Horizontal(e) => Horizontal(map(*e)?),
            Vertical(e) => Vertical(map(*e)?),
            Parallel(a, b) => Parallel(map(*a)?, map(*b)?),
            Perpendicular(a, b) => Perpendicular(map(*a)?, map(*b)?),
            Equal(a, b) => Equal(map(*a)?, map(*b)?),
            Tangent(a, b) => Tangent(map(*a)?, map(*b)?),
            Concentric(a, b) => Concentric(map(*a)?, map(*b)?),
            Fix(p) => Fix(pr(p)?),
            Distance { a, b, value } => Distance { a: pr(a)?, b: pr(b)?, value: *value },
            Diameter { entity, value } => Diameter { entity: map(*entity)?, value: *value },
            Midpoint(p, l) => Midpoint(pr(p)?, map(*l)?),
            Collinear(a, b) => Collinear(map(*a)?, map(*b)?),
            Symmetry(a, b, l) => Symmetry(pr(a)?, pr(b)?, map(*l)?),
        })
    }
}

/// Coordinates of an entity point.
pub fn point_of(entities: &[SketchEntity], p: PointRef) -> Option<[f64; 2]> {
    Some(match (entities.get(p.entity)?, p.index) {
        (SketchEntity::Line { a, .. }, 0) => *a,
        (SketchEntity::Line { b, .. }, 1) => *b,
        (SketchEntity::Circle { c, .. }, 0) => *c,
        (SketchEntity::Point { p }, 0) => *p,
        (SketchEntity::Arc { a, .. }, 0) => *a,
        (SketchEntity::Arc { b, .. }, 1) => *b,
        (SketchEntity::Arc { m, .. }, 2) => *m,
        _ => return None,
    })
}

/// All point references of an entity.
pub fn points_of(entities: &[SketchEntity], e: usize) -> Vec<PointRef> {
    let n = match entities.get(e) {
        Some(SketchEntity::Line { .. }) => 2,
        Some(SketchEntity::Circle { .. }) | Some(SketchEntity::Point { .. }) => 1,
        Some(SketchEntity::Arc { .. }) => 3,
        None => 0,
    };
    (0..n).map(|i| PointRef::new(e, i)).collect()
}

/// Variable layout of a sketch.
struct Layout {
    /// For each (entity, point index): index of the x variable (y = x + 1).
    point_var: std::collections::HashMap<PointRef, usize>,
    /// Circle entity -> radius variable.
    radius_var: std::collections::HashMap<usize, usize>,
    x0: Vec<f64>,
}

const MERGE_TOL: f64 = 1e-7;

fn layout(entities: &[SketchEntity]) -> Layout {
    let mut point_var = std::collections::HashMap::new();
    let mut radius_var = std::collections::HashMap::new();
    let mut x0: Vec<f64> = Vec::new();
    let mut groups: Vec<([f64; 2], usize)> = Vec::new();
    for e in 0..entities.len() {
        for p in points_of(entities, e) {
            let q = point_of(entities, p).unwrap();
            // Arc midpoints are never merged with other points.
            let mergeable = !(matches!(entities[e], SketchEntity::Arc { .. }) && p.index == 2);
            let existing = if mergeable {
                groups.iter().find(|(g, _)| (g[0] - q[0]).abs() < MERGE_TOL && (g[1] - q[1]).abs() < MERGE_TOL).map(|g| g.1)
            } else {
                None
            };
            let var = existing.unwrap_or_else(|| {
                let v = x0.len();
                x0.extend_from_slice(&q);
                if mergeable {
                    groups.push((q, v));
                }
                v
            });
            point_var.insert(p, var);
        }
        if let SketchEntity::Circle { r, .. } = entities[e] {
            radius_var.insert(e, x0.len());
            x0.push(r);
        }
    }
    Layout { point_var, radius_var, x0 }
}

struct Eval<'a> {
    l: &'a Layout,
    entities: &'a [SketchEntity],
}

impl Eval<'_> {
    fn p(&self, x: &[f64], p: PointRef) -> Option<[f64; 2]> {
        let v = *self.l.point_var.get(&p)?;
        Some([x[v], x[v + 1]])
    }

    fn line(&self, x: &[f64], e: usize) -> Option<([f64; 2], [f64; 2])> {
        match self.entities.get(e)? {
            SketchEntity::Line { .. } => Some((self.p(x, PointRef::new(e, 0))?, self.p(x, PointRef::new(e, 1))?)),
            _ => None,
        }
    }

    /// Center and radius of a circle or arc.
    fn circle(&self, x: &[f64], e: usize) -> Option<([f64; 2], f64)> {
        match self.entities.get(e)? {
            SketchEntity::Circle { .. } => Some((self.p(x, PointRef::new(e, 0))?, x[*self.l.radius_var.get(&e)?])),
            SketchEntity::Arc { .. } => m360_kernel::profile::circumcircle(
                self.p(x, PointRef::new(e, 0))?,
                self.p(x, PointRef::new(e, 2))?,
                self.p(x, PointRef::new(e, 1))?,
            ),
            _ => None,
        }
    }

    fn residuals(&self, x: &[f64], cs: &[SketchConstraint], out: &mut Vec<f64>) -> Result<(), String> {
        use SketchConstraint::*;
        let bad = || "a constraint references geometry of the wrong kind".to_string();
        let len = |a: [f64; 2], b: [f64; 2]| (b[0] - a[0]).hypot(b[1] - a[1]);
        let dir = |(a, b): ([f64; 2], [f64; 2])| {
            let l = len(a, b).max(1e-12);
            [(b[0] - a[0]) / l, (b[1] - a[1]) / l]
        };
        for c in cs {
            match c {
                Coincident(a, b) => {
                    let (pa, pb) = (self.p(x, *a).ok_or_else(bad)?, self.p(x, *b).ok_or_else(bad)?);
                    out.extend([pa[0] - pb[0], pa[1] - pb[1]]);
                }
                Horizontal(e) => {
                    let (a, b) = self.line(x, *e).ok_or_else(bad)?;
                    out.push(b[1] - a[1]);
                }
                Vertical(e) => {
                    let (a, b) = self.line(x, *e).ok_or_else(bad)?;
                    out.push(b[0] - a[0]);
                }
                Parallel(e1, e2) | Perpendicular(e1, e2) => {
                    let d1 = dir(self.line(x, *e1).ok_or_else(bad)?);
                    let d2 = dir(self.line(x, *e2).ok_or_else(bad)?);
                    out.push(if matches!(c, Parallel(..)) { d1[0] * d2[1] - d1[1] * d2[0] } else { d1[0] * d2[0] + d1[1] * d2[1] });
                }
                Equal(e1, e2) => match (self.line(x, *e1), self.line(x, *e2)) {
                    (Some((a, b)), Some((c, d))) => out.push(len(a, b) - len(c, d)),
                    _ => {
                        let (_, r1) = self.circle(x, *e1).ok_or_else(bad)?;
                        let (_, r2) = self.circle(x, *e2).ok_or_else(bad)?;
                        out.push(r1 - r2);
                    }
                },
                Tangent(l, ci) => {
                    let (a, b) = self.line(x, *l).ok_or_else(bad)?;
                    let (c, r) = self.circle(x, *ci).ok_or_else(bad)?;
                    let d = dir((a, b));
                    let dist = ((c[0] - a[0]) * d[1] - (c[1] - a[1]) * d[0]).abs();
                    out.push(dist - r);
                }
                Concentric(e1, e2) => {
                    let (c1, _) = self.circle(x, *e1).ok_or_else(bad)?;
                    let (c2, _) = self.circle(x, *e2).ok_or_else(bad)?;
                    out.extend([c1[0] - c2[0], c1[1] - c2[1]]);
                }
                Fix(p) => {
                    let q = self.p(x, *p).ok_or_else(bad)?;
                    let q0 = self.p(&self.l.x0, *p).ok_or_else(bad)?;
                    out.extend([q[0] - q0[0], q[1] - q0[1]]);
                }
                Distance { a, b, value } => {
                    let (pa, pb) = (self.p(x, *a).ok_or_else(bad)?, self.p(x, *b).ok_or_else(bad)?);
                    out.push(len(pa, pb) - value);
                }
                Diameter { entity, value } => {
                    let (_, r) = self.circle(x, *entity).ok_or_else(bad)?;
                    out.push(2.0 * r - value);
                }
                Midpoint(p, l) => {
                    let q = self.p(x, *p).ok_or_else(bad)?;
                    let (a, b) = self.line(x, *l).ok_or_else(bad)?;
                    out.extend([q[0] - (a[0] + b[0]) / 2.0, q[1] - (a[1] + b[1]) / 2.0]);
                }
                Collinear(l1, l2) => {
                    let (a, b) = self.line(x, *l1).ok_or_else(bad)?;
                    let (c, e) = self.line(x, *l2).ok_or_else(bad)?;
                    let d = dir((a, b));
                    let off = |q: [f64; 2]| (q[0] - a[0]) * d[1] - (q[1] - a[1]) * d[0];
                    out.extend([off(c), off(e)]);
                }
                Symmetry(p, q, l) => {
                    let (pa, pb) = (self.p(x, *p).ok_or_else(bad)?, self.p(x, *q).ok_or_else(bad)?);
                    let (a, b) = self.line(x, *l).ok_or_else(bad)?;
                    let d = dir((a, b));
                    let m = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0];
                    // Midpoint on the line, and the segment p-q perpendicular to it.
                    out.push((m[0] - a[0]) * d[1] - (m[1] - a[1]) * d[0]);
                    out.push((pb[0] - pa[0]) * d[0] + (pb[1] - pa[1]) * d[1]);
                }
            }
        }
        Ok(())
    }
}

/// Solve the sketch in place. `drag` pulls one point towards a target
/// (interactive dragging). Returns an error if the constraints cannot be
/// satisfied (the entities are then left unchanged).
pub fn solve(entities: &mut [SketchEntity], constraints: &[SketchConstraint], drag: Option<(PointRef, [f64; 2])>) -> Result<(), String> {
    let l = layout(entities);
    let n = l.x0.len();
    if n == 0 {
        return Ok(());
    }
    let ev = Eval { l: &l, entities };
    let scale = l.x0.iter().fold(1.0f64, |m, v| m.max(v.abs()));
    // Weights of the soft terms decay over the iterations (continuation):
    // early steps stay close to the current geometry / follow the drag,
    // final steps satisfy the constraints exactly.
    let full = |x: &[f64], reg_w: f64, drag_w: f64, out: &mut Vec<f64>| -> Result<usize, String> {
        out.clear();
        ev.residuals(x, constraints, out)?;
        let nc = out.len();
        if let Some((p, t)) = drag {
            if let Some(v) = l.point_var.get(&p) {
                out.extend([drag_w * (x[*v] - t[0]), drag_w * (x[*v + 1] - t[1])]);
            }
        }
        out.extend(x.iter().zip(&l.x0).map(|(v, v0)| reg_w * (v - v0)));
        Ok(nc)
    };
    let cost = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();

    let mut x = l.x0.clone();
    let mut r = Vec::new();
    let mut nc = 0;
    let mut mu = 1e-3;
    let mut stall = 0;
    for it in 0..200 {
        let reg_w = (0.05 * 0.5f64.powi(it)).max(1e-9);
        let drag_w = (1.0 * 0.7f64.powi(it)).max(1e-4);
        nc = full(&x, reg_w, drag_w, &mut r)?;
        let c0 = cost(&r);
        let m = r.len();
        // Numeric Jacobian (central differences).
        let mut jac = vec![0.0; m * n];
        let mut rp = Vec::new();
        let mut rm = Vec::new();
        let h = 1e-7 * scale.max(1.0);
        for j in 0..n {
            let mut xp = x.clone();
            xp[j] += h;
            let mut xm = x.clone();
            xm[j] -= h;
            full(&xp, reg_w, drag_w, &mut rp)?;
            full(&xm, reg_w, drag_w, &mut rm)?;
            for i in 0..m {
                jac[i * n + j] = (rp[i] - rm[i]) / (2.0 * h);
            }
        }
        // Normal equations (JᵀJ + μ diag) Δ = -Jᵀr
        let mut a = vec![0.0; n * n];
        let mut g = vec![0.0; n];
        for i in 0..m {
            let row = &jac[i * n..(i + 1) * n];
            for p in 0..n {
                if row[p] == 0.0 {
                    continue;
                }
                g[p] += row[p] * r[i];
                for q in 0..n {
                    a[p * n + q] += row[p] * row[q];
                }
            }
        }
        let mut improved = false;
        for _ in 0..12 {
            let mut aa = a.clone();
            for p in 0..n {
                aa[p * n + p] += mu * (1.0 + a[p * n + p]);
            }
            let rhs: Vec<f64> = g.iter().map(|v| -v).collect();
            let Some(delta) = solve_dense(&mut aa, rhs, n) else {
                mu *= 10.0;
                continue;
            };
            let xn: Vec<f64> = x.iter().zip(&delta).map(|(a, b)| a + b).collect();
            let mut rn = Vec::new();
            full(&xn, reg_w, drag_w, &mut rn)?;
            if cost(&rn) <= c0 {
                x = xn;
                r = rn;
                mu = (mu * 0.3).max(1e-12);
                improved = true;
                break;
            }
            mu *= 10.0;
        }
        let cres: f64 = r[..nc].iter().map(|v| v * v).sum::<f64>().sqrt();
        if cres < 1e-11 * scale.max(1.0) && it > 20 {
            break;
        }
        if !improved {
            stall += 1;
            if stall > 5 {
                break;
            }
            mu = 1e-3;
        } else {
            stall = 0;
        }
    }
    let _ = nc;
    let cres: f64 = r[..nc].iter().map(|v| v * v).sum::<f64>().sqrt();
    if cres > 1e-6 * scale.max(1.0) {
        return Err("the sketch is over-constrained or the constraints conflict".into());
    }
    // Write back.
    let get = |p: PointRef| {
        let v = l.point_var[&p];
        [x[v], x[v + 1]]
    };
    for (e, ent) in entities.iter_mut().enumerate() {
        match ent {
            SketchEntity::Line { a, b } => {
                *a = get(PointRef::new(e, 0));
                *b = get(PointRef::new(e, 1));
            }
            SketchEntity::Circle { c, r } => {
                *c = get(PointRef::new(e, 0));
                *r = x[l.radius_var[&e]].abs();
            }
            SketchEntity::Point { p } => *p = get(PointRef::new(e, 0)),
            SketchEntity::Arc { a, m, b } => {
                *a = get(PointRef::new(e, 0));
                *b = get(PointRef::new(e, 1));
                *m = get(PointRef::new(e, 2));
            }
        }
    }
    Ok(())
}

/// Gaussian elimination with partial pivoting (in place).
fn solve_dense(a: &mut [f64], mut b: Vec<f64>, n: usize) -> Option<Vec<f64>> {
    for col in 0..n {
        let piv = (col..n).max_by(|&i, &j| a[i * n + col].abs().total_cmp(&a[j * n + col].abs()))?;
        if a[piv * n + col].abs() < 1e-300 {
            return None;
        }
        if piv != col {
            for k in 0..n {
                a.swap(col * n + k, piv * n + k);
            }
            b.swap(col, piv);
        }
        let d = a[col * n + col];
        for i in col + 1..n {
            let f = a[i * n + col] / d;
            if f == 0.0 {
                continue;
            }
            for k in col..n {
                a[i * n + k] -= f * a[col * n + k];
            }
            b[i] -= f * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let s: f64 = (i + 1..n).map(|k| a[i * n + k] * x[k]).sum();
        x[i] = (b[i] - s) / a[i * n + i];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::rectangle;

    #[test]
    fn midpoint_collinear_symmetry() {
        let mut e = vec![
            SketchEntity::Line { a: [0.0, 0.0], b: [10.0, 0.0] },
            SketchEntity::Point { p: [3.0, 2.0] },
            SketchEntity::Line { a: [12.0, 1.0], b: [20.0, 3.0] },
            SketchEntity::Line { a: [0.0, -5.0], b: [0.0, 5.0] },
            SketchEntity::Point { p: [-4.0, 1.0] },
            SketchEntity::Point { p: [3.0, 3.0] },
        ];
        let c = vec![
            SketchConstraint::Midpoint(PointRef::new(1, 0), 0),
            SketchConstraint::Collinear(0, 2),
            SketchConstraint::Symmetry(PointRef::new(4, 0), PointRef::new(5, 0), 3),
        ];
        solve(&mut e, &c, None).unwrap();
        let (SketchEntity::Line { a, b }, SketchEntity::Point { p }) = (e[0].clone(), e[1].clone()) else { panic!() };
        assert!((p[0] - (a[0] + b[0]) / 2.0).abs() < 1e-6 && (p[1] - (a[1] + b[1]) / 2.0).abs() < 1e-6);
        let SketchEntity::Line { a: c2, b: d2 } = e[2] else { panic!() };
        let d = [b[0] - a[0], b[1] - a[1]];
        for q in [c2, d2] {
            assert!(((q[0] - a[0]) * d[1] - (q[1] - a[1]) * d[0]).abs() < 1e-5);
        }
        let (SketchEntity::Point { p: s1 }, SketchEntity::Point { p: s2 }, SketchEntity::Line { a: la, b: lb }) = (e[4].clone(), e[5].clone(), e[3].clone()) else { panic!() };
        // The symmetry line stays vertical-ish here: check the mirror relation.
        let ld = [lb[0] - la[0], lb[1] - la[1]];
        assert!(((s2[0] - s1[0]) * ld[0] + (s2[1] - s1[1]) * ld[1]).abs() < 1e-5);
        let m = [(s1[0] + s2[0]) / 2.0 - la[0], (s1[1] + s2[1]) / 2.0 - la[1]];
        assert!((m[0] * ld[1] - m[1] * ld[0]).abs() < 1e-5);
    }

    fn lens(e: &[SketchEntity]) -> Vec<f64> {
        e.iter()
            .map(|x| match x {
                SketchEntity::Line { a, b } => (b[0] - a[0]).hypot(b[1] - a[1]),
                _ => 0.0,
            })
            .collect()
    }

    #[test]
    fn dimension_rectangle_stays_connected() {
        let mut e = rectangle([0.0, 0.0], [10.0, 5.0]);
        let cs = vec![
            SketchConstraint::Horizontal(0),
            SketchConstraint::Vertical(1),
            SketchConstraint::Horizontal(2),
            SketchConstraint::Vertical(3),
            SketchConstraint::Fix(PointRef::new(0, 0)),
            SketchConstraint::Distance { a: PointRef::new(0, 0), b: PointRef::new(0, 1), value: 30.0 },
            SketchConstraint::Distance { a: PointRef::new(1, 0), b: PointRef::new(1, 1), value: 12.0 },
        ];
        solve(&mut e, &cs, None).unwrap();
        let l = lens(&e);
        assert!((l[0] - 30.0).abs() < 1e-6 && (l[2] - 30.0).abs() < 1e-6, "{l:?}");
        assert!((l[1] - 12.0).abs() < 1e-6 && (l[3] - 12.0).abs() < 1e-6, "{l:?}");
        // Still a closed loop: profile still found.
        let p = crate::sketch::profiles(&e);
        assert_eq!(p.len(), 1);
        assert!((p[0].area() - 360.0).abs() < 1e-4);
        // Fixed corner did not move.
        assert!(matches!(e[0], SketchEntity::Line { a, .. } if a[0].abs() < 1e-9 && a[1].abs() < 1e-9));
    }

    #[test]
    fn perpendicular_tangent_diameter() {
        let mut e = vec![
            SketchEntity::Line { a: [0.0, 0.0], b: [10.0, 1.0] },
            SketchEntity::Line { a: [0.0, 0.0], b: [1.0, 8.0] },
            SketchEntity::Circle { c: [5.0, 5.0], r: 2.0 },
        ];
        let cs = vec![
            SketchConstraint::Horizontal(0),
            SketchConstraint::Perpendicular(0, 1),
            SketchConstraint::Diameter { entity: 2, value: 6.0 },
            SketchConstraint::Tangent(0, 2),
        ];
        solve(&mut e, &cs, None).unwrap();
        let SketchEntity::Line { a, b } = e[1] else { panic!() };
        assert!((b[0] - a[0]).abs() < 1e-6, "second line vertical");
        let SketchEntity::Circle { c, r } = e[2] else { panic!() };
        assert!((r - 3.0).abs() < 1e-6);
        let SketchEntity::Line { a, .. } = e[0] else { panic!() };
        assert!(((c[1] - a[1]).abs() - 3.0).abs() < 1e-6, "tangent to the horizontal line");
    }

    #[test]
    fn conflicting_constraints_fail() {
        let mut e = vec![SketchEntity::Line { a: [0.0, 0.0], b: [10.0, 0.0] }];
        let cs = vec![
            SketchConstraint::Horizontal(0),
            SketchConstraint::Vertical(0),
            SketchConstraint::Distance { a: PointRef::new(0, 0), b: PointRef::new(0, 1), value: 5.0 },
        ];
        assert!(solve(&mut e, &cs, None).is_err());
    }

    #[test]
    fn drag_point() {
        let mut e = rectangle([0.0, 0.0], [10.0, 5.0]);
        let cs = vec![SketchConstraint::Horizontal(0), SketchConstraint::Horizontal(2), SketchConstraint::Vertical(1), SketchConstraint::Vertical(3)];
        solve(&mut e, &cs, Some((PointRef::new(1, 1), [20.0, 9.0]))).unwrap();
        let SketchEntity::Line { b, .. } = e[1] else { panic!() };
        assert!((b[0] - 20.0).abs() < 0.5 && (b[1] - 9.0).abs() < 0.5, "{b:?}");
        assert_eq!(crate::sketch::profiles(&e).len(), 1);
    }

    #[test]
    fn remap_after_delete() {
        let c = SketchConstraint::Parallel(1, 3);
        assert_eq!(c.remapped(&[0]), Some(SketchConstraint::Parallel(0, 2)));
        assert_eq!(c.remapped(&[3]), None);
    }
}
