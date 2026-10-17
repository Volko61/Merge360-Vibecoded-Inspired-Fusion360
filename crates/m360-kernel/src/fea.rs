//! Linear static stress analysis on a voxel mesh.
//!
//! The body is voxelized into cubes of side `h` (8-node trilinear
//! hexahedra), loads and supports are applied to the nodes next to the
//! selected faces, and `K u = f` is solved with a matrix-free
//! Jacobi-preconditioned conjugate gradient.

use glam::{DVec3, Vec3};

use crate::mesh::z_crossings;
use crate::BodyMesh;

/// Material for the analysis (MPa = N/mm²).
#[derive(Debug, Clone, Copy)]
pub struct Elastic {
    pub youngs: f64,
    pub poisson: f64,
}

/// A force applied to a set of faces (N, total).
#[derive(Debug, Clone)]
pub struct FaceLoad {
    pub faces: Vec<u32>,
    pub force: DVec3,
}

#[derive(Debug, Clone)]
pub struct Study {
    pub fixed: Vec<u32>,
    pub loads: Vec<FaceLoad>,
    /// Voxels along the longest side of the body.
    pub resolution: usize,
}

/// Solution on the voxel grid.
#[derive(Debug, Clone)]
pub struct Solution {
    lo: DVec3,
    h: f64,
    n: [usize; 3],
    solid: Vec<bool>,
    /// Displacement of every grid node (zero for unused nodes), mm.
    disp: Vec<DVec3>,
    /// Von Mises stress per voxel (MPa).
    vm: Vec<f64>,
    pub max_disp: f64,
    pub max_vm: f64,
    pub elements: usize,
    pub iterations: usize,
}

const GP: f64 = 0.577_350_269_189_625_8; // 1/sqrt(3)

/// Derivatives of the 8 shape functions wrt x, y, z at natural point
/// (xi, eta, zeta) of a cube of side h. Node a has corner bits (a&1, a>>1&1, a>>2&1).
fn shape_grads(xi: f64, eta: f64, zeta: f64, h: f64) -> [[f64; 3]; 8] {
    let mut g = [[0.0; 3]; 8];
    for (a, ga) in g.iter_mut().enumerate() {
        let s = [((a & 1) as f64) * 2.0 - 1.0, (((a >> 1) & 1) as f64) * 2.0 - 1.0, (((a >> 2) & 1) as f64) * 2.0 - 1.0];
        let f = 2.0 / h / 8.0;
        ga[0] = f * s[0] * (1.0 + eta * s[1]) * (1.0 + zeta * s[2]);
        ga[1] = f * s[1] * (1.0 + xi * s[0]) * (1.0 + zeta * s[2]);
        ga[2] = f * s[2] * (1.0 + xi * s[0]) * (1.0 + eta * s[1]);
    }
    g
}

/// Strain-displacement matrix (6 x 24): exx, eyy, ezz, gxy, gyz, gzx.
fn b_matrix(g: &[[f64; 3]; 8]) -> [[f64; 24]; 6] {
    let mut b = [[0.0; 24]; 6];
    for (a, ga) in g.iter().enumerate() {
        let (x, y, z) = (ga[0], ga[1], ga[2]);
        let c = 3 * a;
        b[0][c] = x;
        b[1][c + 1] = y;
        b[2][c + 2] = z;
        b[3][c] = y;
        b[3][c + 1] = x;
        b[4][c + 1] = z;
        b[4][c + 2] = y;
        b[5][c] = z;
        b[5][c + 2] = x;
    }
    b
}

fn d_matrix(m: Elastic) -> [[f64; 6]; 6] {
    let (e, nu) = (m.youngs, m.poisson.clamp(0.0, 0.499));
    let lam = e * nu / ((1.0 + nu) * (1.0 - 2.0 * nu));
    let mu = e / (2.0 * (1.0 + nu));
    let mut d = [[0.0; 6]; 6];
    for i in 0..3 {
        d[i][..3].fill(lam);
        d[i][i] = lam + 2.0 * mu;
        d[i + 3][i + 3] = mu;
    }
    d
}

/// Element stiffness of a cube of side h (2x2x2 Gauss).
fn element_stiffness(m: Elastic, h: f64) -> Vec<f64> {
    let d = d_matrix(m);
    let det = (h / 2.0).powi(3);
    let mut k = vec![0.0; 24 * 24];
    for &xi in &[-GP, GP] {
        for &eta in &[-GP, GP] {
            for &zeta in &[-GP, GP] {
                let b = b_matrix(&shape_grads(xi, eta, zeta, h));
                // D B
                let mut db = [[0.0; 24]; 6];
                for i in 0..6 {
                    for c in 0..24 {
                        db[i][c] = (0..6).map(|j| d[i][j] * b[j][c]).sum();
                    }
                }
                for r in 0..24 {
                    for c in 0..24 {
                        let v: f64 = (0..6).map(|i| b[i][r] * db[i][c]).sum();
                        k[r * 24 + c] += v * det;
                    }
                }
            }
        }
    }
    k
}

/// Distance from p to triangle t.
fn tri_dist(t: &[Vec3; 3], p: Vec3) -> f32 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return ap.length();
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return bp.length();
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return (p - (a + ab * (d1 / (d1 - d3)))).length();
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return cp.length();
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return (p - (a + ac * (d2 / (d2 - d6)))).length();
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return (p - (b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6))))).length();
    }
    let n = ab.cross(ac).normalize_or_zero();
    (p - a).dot(n).abs()
}

pub fn solve(mesh: &BodyMesh, study: &Study, mat: Elastic) -> Result<Solution, String> {
    if study.fixed.is_empty() {
        return Err("add a fixed constraint".into());
    }
    if study.loads.is_empty() {
        return Err("add a load".into());
    }
    let lo = mesh.bbox_min.as_dvec3();
    let size = (mesh.bbox_max - mesh.bbox_min).as_dvec3();
    let res = study.resolution.clamp(4, 120);
    let h = size.max_element() / res as f64;
    if h <= 0.0 {
        return Err("the body is empty".into());
    }
    let n = [0, 1, 2].map(|k| ((size[k] / h).ceil() as usize).max(1));
    let vid = |i: usize, j: usize, k: usize| i + n[0] * (j + n[1] * k);
    let nid = |i: usize, j: usize, k: usize| i + (n[0] + 1) * (j + (n[1] + 1) * k);
    // Voxelize with vertical rays through the voxel centers.
    let tris: Vec<[Vec3; 3]> = mesh.triangles().map(|(t, _)| t).collect();
    // Rotate so the rays run along the grid's z axis (z_crossings casts along z).
    let mut solid = vec![false; n[0] * n[1] * n[2]];
    for i in 0..n[0] {
        for j in 0..n[1] {
            let x = lo.x + (i as f64 + 0.5013) * h;
            let y = lo.y + (j as f64 + 0.4987) * h;
            let zs = z_crossings(&tris, x as f32, y as f32);
            for k in 0..n[2] {
                let z = (lo.z + (k as f64 + 0.5) * h) as f32;
                let below = zs.iter().filter(|c| **c < z).count();
                if below % 2 == 1 {
                    solid[vid(i, j, k)] = true;
                }
            }
        }
    }
    let elements: Vec<[usize; 3]> =
        (0..n[2]).flat_map(|k| (0..n[1]).flat_map(move |j| (0..n[0]).map(move |i| [i, j, k]))).filter(|v| solid[vid(v[0], v[1], v[2])]).collect();
    if elements.is_empty() {
        return Err("the body is too thin for this mesh resolution".into());
    }
    // DOF numbering of the used nodes.
    let nn = (n[0] + 1) * (n[1] + 1) * (n[2] + 1);
    let mut dof = vec![usize::MAX; nn];
    let mut count = 0;
    let enodes = |v: &[usize; 3]| -> [usize; 8] {
        let mut out = [0; 8];
        for (a, o) in out.iter_mut().enumerate() {
            *o = nid(v[0] + (a & 1), v[1] + ((a >> 1) & 1), v[2] + ((a >> 2) & 1));
        }
        out
    };
    for v in &elements {
        for nd in enodes(v) {
            if dof[nd] == usize::MAX {
                dof[nd] = count;
                count += 1;
            }
        }
    }
    let node_pos = |nd: usize| {
        let i = nd % (n[0] + 1);
        let j = (nd / (n[0] + 1)) % (n[1] + 1);
        let k = nd / ((n[0] + 1) * (n[1] + 1));
        lo + DVec3::new(i as f64, j as f64, k as f64) * h
    };
    let near = |faces: &[u32]| -> Vec<usize> {
        let ft: Vec<[Vec3; 3]> = mesh.triangles().filter(|(_, f)| faces.contains(f)).map(|(t, _)| t).collect();
        (0..nn)
            .filter(|nd| dof[*nd] != usize::MAX)
            .filter(|nd| {
                let p = node_pos(*nd).as_vec3();
                ft.iter().any(|t| tri_dist(t, p) <= (0.75 * h) as f32)
            })
            .map(|nd| dof[nd])
            .collect()
    };
    let mut fixed = vec![false; 3 * count];
    let fixed_nodes = near(&study.fixed);
    if fixed_nodes.is_empty() {
        return Err("the fixed faces are not part of this body".into());
    }
    for d in fixed_nodes {
        fixed[3 * d] = true;
        fixed[3 * d + 1] = true;
        fixed[3 * d + 2] = true;
    }
    let mut f = vec![0.0; 3 * count];
    for load in &study.loads {
        let nodes = near(&load.faces);
        if nodes.is_empty() {
            return Err("a loaded face is not part of this body".into());
        }
        // Nodes shared with a support would absorb their share: spread the whole
        // force over the free nodes of the face.
        let nodes: Vec<usize> = nodes.into_iter().filter(|&d| !fixed[3 * d]).collect();
        if nodes.is_empty() {
            return Err("a loaded face is entirely fixed".into());
        }
        let per = load.force / nodes.len() as f64;
        for d in nodes {
            f[3 * d] += per.x;
            f[3 * d + 1] += per.y;
            f[3 * d + 2] += per.z;
        }
    }
    for (i, fx) in fixed.iter().enumerate() {
        if *fx {
            f[i] = 0.0;
        }
    }
    let ke = element_stiffness(mat, h);
    let edofs: Vec<[usize; 24]> = elements
        .iter()
        .map(|v| {
            let nodes = enodes(v);
            let mut d = [0; 24];
            for a in 0..8 {
                for c in 0..3 {
                    d[3 * a + c] = 3 * dof[nodes[a]] + c;
                }
            }
            d
        })
        .collect();
    let matvec = |x: &[f64], y: &mut [f64]| {
        y.iter_mut().for_each(|v| *v = 0.0);
        for ed in &edofs {
            let mut xe = [0.0; 24];
            for (k, d) in ed.iter().enumerate() {
                xe[k] = x[*d];
            }
            for r in 0..24 {
                let row = &ke[r * 24..r * 24 + 24];
                let v: f64 = row.iter().zip(&xe).map(|(a, b)| a * b).sum();
                y[ed[r]] += v;
            }
        }
        for (i, fx) in fixed.iter().enumerate() {
            if *fx {
                y[i] = 0.0;
            }
        }
    };
    // Jacobi preconditioner.
    let mut diag = vec![0.0; 3 * count];
    for ed in &edofs {
        for k in 0..24 {
            diag[ed[k]] += ke[k * 24 + k];
        }
    }
    let inv: Vec<f64> = diag.iter().zip(&fixed).map(|(d, fx)| if *fx || *d <= 0.0 { 0.0 } else { 1.0 / d }).collect();
    let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let mut u = vec![0.0; 3 * count];
    let mut r = f.clone();
    let mut z: Vec<f64> = r.iter().zip(&inv).map(|(a, b)| a * b).collect();
    let mut p = z.clone();
    let mut rz = dot(&r, &z);
    let fnorm = dot(&f, &f).sqrt().max(1e-30);
    let mut ap = vec![0.0; 3 * count];
    let mut iterations = 0;
    for it in 0..20000 {
        iterations = it;
        if dot(&r, &r).sqrt() / fnorm < 1e-7 {
            break;
        }
        matvec(&p, &mut ap);
        let pap = dot(&p, &ap);
        if pap.abs() < 1e-300 {
            break;
        }
        let alpha = rz / pap;
        for i in 0..u.len() {
            u[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
        }
        z = r.iter().zip(&inv).map(|(a, b)| a * b).collect();
        let rz2 = dot(&r, &z);
        let beta = rz2 / rz;
        rz = rz2;
        for i in 0..p.len() {
            p[i] = z[i] + beta * p[i];
        }
    }
    // Nodal displacements on the full grid.
    let mut disp = vec![DVec3::ZERO; nn];
    for nd in 0..nn {
        if dof[nd] != usize::MAX {
            let d = dof[nd];
            disp[nd] = DVec3::new(u[3 * d], u[3 * d + 1], u[3 * d + 2]);
        }
    }
    // Von Mises stress at the element centers.
    let d = d_matrix(mat);
    let b0 = b_matrix(&shape_grads(0.0, 0.0, 0.0, h));
    let mut vm = vec![0.0; n[0] * n[1] * n[2]];
    for (v, ed) in elements.iter().zip(&edofs) {
        let ue: Vec<f64> = ed.iter().map(|k| u[*k]).collect();
        let eps: Vec<f64> = (0..6).map(|i| (0..24).map(|c| b0[i][c] * ue[c]).sum()).collect();
        let s: Vec<f64> = (0..6).map(|i| (0..6).map(|j| d[i][j] * eps[j]).sum()).collect();
        let v_m = (0.5 * ((s[0] - s[1]).powi(2) + (s[1] - s[2]).powi(2) + (s[2] - s[0]).powi(2)) + 3.0 * (s[3] * s[3] + s[4] * s[4] + s[5] * s[5])).sqrt();
        vm[vid(v[0], v[1], v[2])] = v_m;
    }
    let max_disp = disp.iter().map(|d| d.length()).fold(0.0, f64::max);
    let max_vm = vm.iter().copied().fold(0.0, f64::max);
    Ok(Solution { lo, h, n, solid, disp, vm, max_disp, max_vm, elements: elements.len(), iterations })
}

impl Solution {
    fn voxel_of(&self, p: DVec3) -> [isize; 3] {
        let q = (p - self.lo) / self.h;
        [q.x.floor() as isize, q.y.floor() as isize, q.z.floor() as isize]
    }

    fn solid_at(&self, v: [isize; 3]) -> bool {
        (0..3).all(|k| v[k] >= 0 && (v[k] as usize) < self.n[k]) && self.solid[v[0] as usize + self.n[0] * (v[1] as usize + self.n[1] * v[2] as usize)]
    }

    /// The solid voxel containing (or nearest to) a surface point.
    fn nearest_solid(&self, p: DVec3) -> Option<[usize; 3]> {
        let v = self.voxel_of(p);
        let mut best: Option<([usize; 3], f64)> = None;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let w = [v[0] + dx, v[1] + dy, v[2] + dz];
                    if self.solid_at(w) {
                        let c = self.lo + (DVec3::new(w[0] as f64, w[1] as f64, w[2] as f64) + 0.5) * self.h;
                        let d = (c - p).length();
                        if best.is_none_or(|b| d < b.1) {
                            best = Some(([w[0] as usize, w[1] as usize, w[2] as usize], d));
                        }
                    }
                }
            }
        }
        best.map(|b| b.0)
    }

    /// Von Mises stress (MPa) at a surface point.
    pub fn stress_at(&self, p: DVec3) -> f64 {
        self.nearest_solid(p).map_or(0.0, |v| self.vm[v[0] + self.n[0] * (v[1] + self.n[1] * v[2])])
    }

    /// Displacement (mm) at a point (trilinear in its voxel).
    pub fn displacement_at(&self, p: DVec3) -> DVec3 {
        let Some(v) = self.nearest_solid(p) else { return DVec3::ZERO };
        let q = ((p - self.lo) / self.h - DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64)).clamp(DVec3::ZERO, DVec3::ONE);
        let nid = |i: usize, j: usize, k: usize| i + (self.n[0] + 1) * (j + (self.n[1] + 1) * k);
        let mut out = DVec3::ZERO;
        for a in 0..8 {
            let (bx, by, bz) = (a & 1, (a >> 1) & 1, (a >> 2) & 1);
            let w = (if bx == 1 { q.x } else { 1.0 - q.x }) * (if by == 1 { q.y } else { 1.0 - q.y }) * (if bz == 1 { q.z } else { 1.0 - q.z });
            out += self.disp[nid(v[0] + bx, v[1] + by, v[2] + bz)] * w;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::make_box;
    use crate::Frame;

    #[test]
    fn cantilever_beam() {
        // 100 x 10 x 10 steel beam along X, fixed at x = 0, 100 N down at x = 100.
        let b = make_box(&Frame::new(DVec3::new(50.0, 0.0, 0.0), DVec3::X, DVec3::Y), 100.0, 10.0, 10.0).unwrap();
        let m = crate::tessellate(&b);
        let face = |nx: f32| (0..m.face_count).find(|f| m.planar_normal(*f).is_some_and(|n| (n.x - nx).abs() < 1e-3)).unwrap();
        let study = Study { fixed: vec![face(-1.0)], loads: vec![FaceLoad { faces: vec![face(1.0)], force: DVec3::new(0.0, -100.0, 0.0) }], resolution: 60 };
        let s = solve(&m, &study, Elastic { youngs: 210_000.0, poisson: 0.3 }).unwrap();
        // Beam theory: delta = F L^3 / (3 E I), I = 10^4 / 12; sigma = M c / I.
        let i = 10f64.powi(4) / 12.0;
        let delta = 100.0 * 100f64.powi(3) / (3.0 * 210_000.0 * i);
        let tip = s.displacement_at(DVec3::new(100.0, 0.0, 5.0)).y;
        assert!(tip < 0.0 && (tip.abs() - delta).abs() / delta < 0.1, "tip {tip} vs {delta} ({} it)", s.iterations);
        let sigma = 100.0 * 100.0 * 5.0 / i;
        let near_root = s.stress_at(DVec3::new(8.0, 5.0, 0.0));
        // Stresses are evaluated at element centers (y = 5 - h / 2 for the top layer).
        let expect = sigma * 0.92 * (5.0 - 100.0 / 60.0 / 2.0) / 5.0;
        assert!((near_root - expect).abs() / expect < 0.15, "root stress {near_root} vs {expect}");
        assert!(s.max_disp > 0.0);
    }
}

#[cfg(test)]
mod axial_tests {
    use super::*;
    use crate::primitives::make_box;
    use crate::Frame;

    #[test]
    fn axial_bar() {
        // 100 x 20 x 20 steel bar along X, fixed at x = 0, 2000 N pushing at x = 100.
        let b = make_box(&Frame::new(DVec3::new(50.0, 0.0, 0.0), DVec3::X, DVec3::Y), 100.0, 20.0, 20.0).unwrap();
        let m = crate::tessellate(&b);
        let face = |nx: f32| (0..m.face_count).find(|f| m.planar_normal(*f).is_some_and(|n| (n.x - nx).abs() < 1e-3)).unwrap();
        let study = Study { fixed: vec![face(-1.0)], loads: vec![FaceLoad { faces: vec![face(1.0)], force: DVec3::new(-2000.0, 0.0, 0.0) }], resolution: 30 };
        let s = solve(&m, &study, Elastic { youngs: 210_000.0, poisson: 0.3 }).unwrap();
        // Uniform compression: sigma = F / A = 5 MPa, delta = F L / (E A).
        for x in [10.0, 30.0, 50.0, 70.0] {
            let st = s.stress_at(DVec3::new(x, 0.0, 10.0));
            assert!((st - 5.0).abs() < 0.25, "stress {st} at x = {x}");
        }
        let tip = s.displacement_at(DVec3::new(100.0, 0.0, 10.0)).x;
        let expect = -2000.0 * 100.0 / (210_000.0 * 400.0);
        assert!((tip - expect).abs() / expect.abs() < 0.1, "tip {tip} vs {expect}");
    }
}
