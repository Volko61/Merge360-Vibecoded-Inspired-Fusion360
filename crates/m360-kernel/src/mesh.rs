//! Tessellation of B-rep solids into render/pick friendly meshes.

use std::collections::HashMap;

use glam::{DVec3, Vec3};
use truck_meshalgo::prelude::*;

use crate::Solid;

/// Triangulated body: flat vertex arrays with a per-vertex B-rep face index,
/// plus polylines for the B-rep edges.
#[derive(Debug, Clone, Default)]
pub struct BodyMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub face_ids: Vec<u32>,
    pub indices: Vec<u32>,
    pub edges: Vec<Vec<[f32; 3]>>,
    /// Topological info for each entry of `edges`.
    pub edge_info: Vec<EdgeInfo>,
    pub face_count: u32,
    pub bbox_min: Vec3,
    pub bbox_max: Vec3,
}

/// Adjacency information of a visible B-rep edge, used by Fillet/Chamfer.
#[derive(Debug, Clone, Default)]
pub struct EdgeInfo {
    /// Adjacent face ids (two for manifold edges).
    pub faces: Vec<u32>,
    /// The edge is a straight segment.
    pub straight: bool,
    /// Outward normals of the adjacent faces when both are planar.
    pub normals: Option<[Vec3; 2]>,
    /// Directions (perpendicular to the edge) from the edge into each face.
    pub dirs: Option<[Vec3; 2]>,
}

impl EdgeInfo {
    /// Convex edges (material angle < 180°) are filleted by removing material.
    pub fn convex(&self) -> Option<bool> {
        let (n, d) = (self.normals?, self.dirs?);
        Some(d[1].dot(n[0]) < 0.0)
    }
}

impl BodyMesh {
    /// Concatenate several meshes into one (face ids are offset).
    pub fn merge(meshes: &[BodyMesh]) -> BodyMesh {
        let mut out = BodyMesh { bbox_min: Vec3::splat(f32::MAX), bbox_max: Vec3::splat(f32::MIN), ..Default::default() };
        for m in meshes {
            let base = out.positions.len() as u32;
            let fbase = out.face_count;
            out.positions.extend_from_slice(&m.positions);
            out.normals.extend_from_slice(&m.normals);
            out.face_ids.extend(m.face_ids.iter().map(|f| f + fbase));
            out.indices.extend(m.indices.iter().map(|i| i + base));
            out.edges.extend(m.edges.iter().cloned());
            out.edge_info.extend(m.edge_info.iter().cloned());
            out.face_count += m.face_count;
            out.bbox_min = out.bbox_min.min(m.bbox_min);
            out.bbox_max = out.bbox_max.max(m.bbox_max);
        }
        out
    }

    /// Smooth shading: average the normals of coincident vertices whose
    /// normals differ by less than `acos(cos)` (for polyhedral imported meshes).
    pub fn smooth_shading(&mut self, cos: f32) {
        let key = |p: [f32; 3]| [(p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64, (p[2] * 1e4).round() as i64];
        let mut groups: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
        for (i, p) in self.positions.iter().enumerate() {
            groups.entry(key(*p)).or_default().push(i);
        }
        let old = self.normals.clone();
        for idx in groups.values() {
            for &i in idx {
                let ni = Vec3::from(old[i]);
                let sum: Vec3 = idx.iter().map(|&j| Vec3::from(old[j])).filter(|nj| nj.dot(ni) >= cos).sum();
                self.normals[i] = sum.normalize_or_zero().to_array();
            }
        }
    }

    /// A copy transformed by `m` (normals and winding fixed for reflections).
    pub fn transformed(&self, m: glam::DMat4) -> BodyMesh {
        let mf = m.as_mat4();
        let nm = glam::Mat3::from_mat4(mf).inverse().transpose();
        let flip = m.determinant() < 0.0;
        let mut out = self.clone();
        out.positions = self.positions.iter().map(|p| mf.transform_point3(Vec3::from(*p)).to_array()).collect();
        out.normals = self.normals.iter().map(|n| (nm * Vec3::from(*n)).normalize_or_zero().to_array()).collect();
        if flip {
            for t in out.indices.as_chunks_mut::<3>().0 {
                t.swap(1, 2);
            }
        }
        out.edges = self.edges.iter().map(|e| e.iter().map(|p| mf.transform_point3(Vec3::from(*p)).to_array()).collect()).collect();
        out.edge_info = Vec::new();
        out.bbox_min = Vec3::splat(f32::MAX);
        out.bbox_max = Vec3::splat(f32::MIN);
        for p in &out.positions {
            out.bbox_min = out.bbox_min.min(Vec3::from(*p));
            out.bbox_max = out.bbox_max.max(Vec3::from(*p));
        }
        out
    }

    /// Unit normal of a face if it is planar.
    pub fn planar_normal(&self, face: u32) -> Option<Vec3> {
        let mut n0: Option<Vec3> = None;
        for (t, f) in self.triangles() {
            if f != face {
                continue;
            }
            let n = (t[1] - t[0]).cross(t[2] - t[0]);
            if n.length_squared() < 1e-12 {
                continue;
            }
            let n = n.normalize();
            match n0 {
                None => n0 = Some(n),
                Some(a) if a.dot(n) < 0.9999 => return None,
                _ => {}
            }
        }
        n0
    }

    /// Area-weighted centroid of a face.
    /// Fit a cylinder to a face: `None` unless the face is (part of) a
    /// cylinder. Uses the vertex normals, which are radial on a cylinder.
    pub fn face_cylinder(&self, face: u32) -> Option<Cylinder> {
        use glam::DVec3;
        let mut pts: Vec<(DVec3, DVec3)> = Vec::new();
        for (i, f) in self.face_ids.iter().enumerate() {
            if *f == face {
                pts.push((Vec3::from(self.positions[i]).as_dvec3(), Vec3::from(self.normals[i]).as_dvec3().normalize_or_zero()));
            }
        }
        if pts.len() < 6 {
            return None;
        }
        // Axis: the direction perpendicular to all normals (largest cross product).
        let n0 = pts[0].1;
        let other = pts.iter().map(|p| p.1).max_by(|a, b| n0.cross(*a).length().total_cmp(&n0.cross(*b).length()))?;
        let axis = n0.cross(other);
        if axis.length() < 0.05 {
            return None; // planar face
        }
        let axis = axis.normalize();
        if pts.iter().any(|(_, n)| n.dot(axis).abs() > 0.02) {
            return None; // not a cylinder (cone, sphere, ...)
        }
        // Circle through the projections: least squares (Kasa) in the plane ⟂ axis.
        let u = axis.any_orthonormal_vector();
        let v = axis.cross(u);
        let (mut a, mut b_) = ([[0.0f64; 3]; 3], [0.0f64; 3]);
        for (p, _) in &pts {
            let (x, y) = (p.dot(u), p.dot(v));
            let row = [x, y, 1.0];
            let rhs = x * x + y * y;
            for r in 0..3 {
                for c in 0..3 {
                    a[r][c] += row[r] * row[c];
                }
                b_[r] += row[r] * rhs;
            }
        }
        let sol = solve3(a, b_)?;
        let (cx, cy) = (sol[0] / 2.0, sol[1] / 2.0);
        let radius = (sol[2] + cx * cx + cy * cy).sqrt();
        if !radius.is_finite() || radius < 1e-6 {
            return None;
        }
        let center = u * cx + v * cy;
        let tol = radius * 0.01 + 1e-3;
        let mut internal_votes = 0;
        let (mut z0, mut z1) = (f64::MAX, f64::MIN);
        for (p, n) in &pts {
            let z = p.dot(axis);
            let radial = *p - axis * z - center;
            if (radial.length() - radius).abs() > tol {
                return None;
            }
            if radial.dot(*n) < 0.0 {
                internal_votes += 1;
            }
            z0 = z0.min(z);
            z1 = z1.max(z);
        }
        Some(Cylinder { origin: center + axis * z0, axis, radius, length: z1 - z0, internal: internal_votes * 2 > pts.len() })
    }

    pub fn face_centroid(&self, face: u32) -> Option<Vec3> {
        let mut c = Vec3::ZERO;
        let mut w = 0.0;
        for (t, f) in self.triangles() {
            if f == face {
                let a = (t[1] - t[0]).cross(t[2] - t[0]).length();
                c += (t[0] + t[1] + t[2]) / 3.0 * a;
                w += a;
            }
        }
        (w > 0.0).then(|| c / w)
    }

    /// Signed volume via the divergence theorem (mm³).
    pub fn volume(&self) -> f64 {
        self.indices
            .as_chunks::<3>().0.iter()
            .map(|t| {
                let a = DVec3::from(self.positions[t[0] as usize].map(f64::from));
                let b = DVec3::from(self.positions[t[1] as usize].map(f64::from));
                let c = DVec3::from(self.positions[t[2] as usize].map(f64::from));
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    pub fn area(&self) -> f64 {
        self.indices
            .as_chunks::<3>().0.iter()
            .map(|t| {
                let a = DVec3::from(self.positions[t[0] as usize].map(f64::from));
                let b = DVec3::from(self.positions[t[1] as usize].map(f64::from));
                let c = DVec3::from(self.positions[t[2] as usize].map(f64::from));
                (b - a).cross(c - a).length() / 2.0
            })
            .sum()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Iterate triangles as world-space vertex triples together with their face id.
    pub fn triangles(&self) -> impl Iterator<Item = ([Vec3; 3], u32)> + '_ {
        self.indices.as_chunks::<3>().0.iter().map(|t| {
            (
                [
                    Vec3::from(self.positions[t[0] as usize]),
                    Vec3::from(self.positions[t[1] as usize]),
                    Vec3::from(self.positions[t[2] as usize]),
                ],
                self.face_ids[t[0] as usize],
            )
        })
    }
}

fn bounding_size(solid: &Solid) -> f64 {
    let mut min = DVec3::splat(f64::MAX);
    let mut max = DVec3::splat(f64::MIN);
    for v in solid.vertex_iter() {
        let p = v.point();
        let p = DVec3::new(p.x, p.y, p.z);
        min = min.min(p);
        max = max.max(p);
    }
    if min.x > max.x {
        1.0
    } else {
        (max - min).length().max(1e-3)
    }
}

/// Largest body extent (mm) that is tessellated: 100 km.
pub const MAX_SIZE: f64 = 1e8;

/// Tessellate a solid with a chordal tolerance proportional to its size.
pub fn tessellate(solid: &Solid) -> BodyMesh {
    let tol = (bounding_size(solid) * 0.001).clamp(1e-3, 0.5);
    tessellate_with(solid, tol)
}

/// Default cosine threshold above which an edge between two faces is
/// considered tangent-continuous (hidden).
pub const SMOOTH_COS: f32 = 0.999;

/// Tessellate, hiding edges whose adjacent faces meet at less than
/// `acos(smooth_cos)` (used to hide facet edges of approximated fillets).
pub fn tessellate_smooth(solid: &Solid, smooth_cos: f32) -> BodyMesh {
    let tol = (bounding_size(solid) * 0.001).clamp(1e-3, 0.5);
    tessellate_impl(solid, tol, smooth_cos)
}

pub fn tessellate_with(solid: &Solid, tol: f64) -> BodyMesh {
    tessellate_impl(solid, tol, SMOOTH_COS)
}

fn tessellate_impl(solid: &Solid, tol: f64, smooth_cos: f32) -> BodyMesh {
    // Non-finite or absurdly large geometry would make the triangulation loop forever.
    let finite = solid.vertex_iter().all(|v| {
        let p = v.point();
        p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
    });
    if !finite || !tol.is_finite() || tol <= 0.0 || bounding_size(solid) > MAX_SIZE {
        return BodyMesh::default();
    }
    // Shell by shell: open shells (surface bodies) cannot be meshed as a solid.
    let meshed: Vec<_> = solid.boundaries().iter().map(|sh| sh.robust_triangulation(tol)).collect();
    let mut out = BodyMesh {
        bbox_min: Vec3::splat(f32::MAX),
        bbox_max: Vec3::splat(f32::MIN),
        ..Default::default()
    };

    for (face_id, face) in meshed.iter().flat_map(|sh| sh.face_iter()).enumerate() {
        let Some(poly) = face.surface() else { continue };
        let flip = !face.orientation();
        let positions = poly.positions();
        let normals = poly.normals();
        for tri in poly.faces().triangle_iter() {
            let base = out.positions.len() as u32;
            let p: Vec<DVec3> = tri
                .iter()
                .map(|v| {
                    let q = positions[v.pos];
                    DVec3::new(q.x, q.y, q.z)
                })
                .collect();
            let mut flat = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            if flip {
                flat = -flat;
            }
            for (k, v) in tri.iter().enumerate() {
                let n = v
                    .nor
                    .and_then(|i| normals.get(i))
                    .map(|n| DVec3::new(n.x, n.y, n.z))
                    .filter(|n| n.length_squared() > 0.5)
                    .map(|n| if flip { -n } else { n })
                    .unwrap_or(flat);
                let pos = p[k].as_vec3();
                out.bbox_min = out.bbox_min.min(pos);
                out.bbox_max = out.bbox_max.max(pos);
                out.positions.push(pos.to_array());
                out.normals.push(n.as_vec3().to_array());
                out.face_ids.push(face_id as u32);
            }
            if flip {
                out.indices.extend_from_slice(&[base, base + 2, base + 1]);
            } else {
                out.indices.extend_from_slice(&[base, base + 1, base + 2]);
            }
        }
        out.face_count = face_id as u32 + 1;
    }

    // Hide seam edges (same face on both sides) and smooth edges between
    // tangent-continuous faces, like Fusion does.
    let scale = 1.0e3 / (out.bbox_max - out.bbox_min).max_element().max(1e-3);
    let q = |p: [f32; 3]| [(p[0] * scale).round() as i64, (p[1] * scale).round() as i64, (p[2] * scale).round() as i64];
    let mut normals_at: HashMap<[i64; 3], Vec<(u32, Vec3)>> = HashMap::new();
    for (i, p) in out.positions.iter().enumerate() {
        normals_at.entry(q(*p)).or_default().push((out.face_ids[i], Vec3::from(out.normals[i])));
    }
    let mut edge_faces: HashMap<_, Vec<u32>> = HashMap::new();
    for (fi, face) in meshed.iter().flat_map(|sh| sh.face_iter()).enumerate() {
        for wire in face.boundaries() {
            for e in wire.edge_iter() {
                edge_faces.entry(e.id()).or_default().push(fi as u32);
            }
        }
    }
    let normal_of = |p: [f32; 3], f: u32| -> Option<Vec3> {
        let ns = normals_at.get(&q(p))?;
        let v: Vec3 = ns.iter().filter(|(id, _)| *id == f).map(|(_, n)| *n).sum();
        (v.length_squared() > 1e-12).then(|| v.normalize())
    };

    let mut seen = std::collections::HashSet::new();
    for edge in meshed.iter().flat_map(|sh| sh.edge_iter()) {
        if !seen.insert(edge.id()) {
            continue;
        }
        let curve = edge.curve();
        let pts: Vec<[f32; 3]> = curve
            .iter()
            .map(|p| [p.x as f32, p.y as f32, p.z as f32])
            .collect();
        if pts.len() < 2 {
            continue;
        }
        let faces_of = edge_faces.get(&edge.id()).cloned().unwrap_or_default();
        if let Some(faces) = edge_faces.get(&edge.id()) {
            if faces.len() == 2 {
                if faces[0] == faces[1] {
                    continue; // seam
                }
                let samples = [pts[0], pts[pts.len() / 2], pts[pts.len() - 1]];
                let smooth = samples.iter().all(|p| match (normal_of(*p, faces[0]), normal_of(*p, faces[1])) {
                    (Some(a), Some(b)) => a.dot(b) > smooth_cos,
                    _ => false,
                });
                if smooth {
                    continue;
                }
            }
        }
        let p0 = Vec3::from(pts[0]);
        let p1 = Vec3::from(pts[pts.len() - 1]);
        let dir = (p1 - p0).normalize_or_zero();
        let straight = dir != Vec3::ZERO
            && pts.iter().all(|p| {
                let v = Vec3::from(*p) - p0;
                (v - dir * v.dot(dir)).length() < 1e-3 * (p1 - p0).length().max(1.0)
            });
        out.edges.push(pts);
        out.edge_info.push(EdgeInfo { faces: faces_of, straight, normals: None, dirs: None });
    }
    // Planar adjacency for straight edges (normals + in-face directions).
    for i in 0..out.edges.len() {
        let info = &out.edge_info[i];
        if !info.straight || info.faces.len() != 2 {
            continue;
        }
        let (f1, f2) = (info.faces[0], info.faces[1]);
        let pts = &out.edges[i];
        let p0 = Vec3::from(pts[0]);
        let dir = (Vec3::from(pts[pts.len() - 1]) - p0).normalize();
        let (Some(n1), Some(n2)) = (out.planar_normal(f1), out.planar_normal(f2)) else { continue };
        let into = |n: Vec3, f: u32| -> Option<Vec3> {
            // In-plane direction perpendicular to the edge, pointing into the face.
            let d = n.cross(dir).normalize_or_zero();
            let c = out.face_centroid(f)?;
            Some(if (c - p0).dot(d) >= 0.0 { d } else { -d })
        };
        if let (Some(d1), Some(d2)) = (into(n1, f1), into(n2, f2)) {
            out.edge_info[i].normals = Some([n1, n2]);
            out.edge_info[i].dirs = Some([d1, d2]);
        }
    }

    if out.positions.is_empty() {
        out.bbox_min = Vec3::ZERO;
        out.bbox_max = Vec3::ZERO;
    }
    out
}

/// A cylindrical face: axis from `origin` along `axis` for `length`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cylinder {
    pub origin: glam::DVec3,
    pub axis: glam::DVec3,
    pub radius: f64,
    pub length: f64,
    /// The face looks towards the axis (a hole).
    pub internal: bool,
}

/// Solve a 3x3 linear system (Cramer's rule).
fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(a);
    if d.abs() < 1e-12 {
        return None;
    }
    let mut out = [0.0; 3];
    for (k, o) in out.iter_mut().enumerate() {
        let mut m = a;
        for r in 0..3 {
            m[r][k] = b[r];
        }
        *o = det(m) / d;
    }
    Some(out)
}

/// Heights where a vertical line (x, y) crosses the mesh, sorted.
pub(crate) fn z_crossings(tris: &[[Vec3; 3]], x: f32, y: f32) -> Vec<f32> {
    let mut zs = Vec::new();
    for t in tris {
        let (a, b, c) = (t[0], t[1], t[2]);
        let d = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
        if d.abs() < 1e-12 {
            continue;
        }
        let l1 = ((x - a.x) * (c.y - a.y) - (c.x - a.x) * (y - a.y)) / d;
        let l2 = ((b.x - a.x) * (y - a.y) - (x - a.x) * (b.y - a.y)) / d;
        if l1 < 0.0 || l2 < 0.0 || l1 + l2 > 1.0 {
            continue;
        }
        zs.push(a.z + l1 * (b.z - a.z) + l2 * (c.z - a.z));
    }
    zs.sort_by(f32::total_cmp);
    zs
}

/// Whether a point is inside a closed mesh (crossing parity upwards).
fn inside(tris: &[[Vec3; 3]], p: Vec3) -> bool {
    z_crossings(tris, p.x, p.y).iter().filter(|z| **z > p.z).count() % 2 == 1
}

/// Approximate overlap of two closed meshes: its volume (exact along a grid
/// of vertical rays) and a display mesh made of the parts of each surface
/// that lie inside the other body. `None` when they do not overlap.
pub fn mesh_overlap(a: &BodyMesh, b: &BodyMesh) -> Option<(f64, BodyMesh)> {
    let lo = a.bbox_min.max(b.bbox_min);
    let hi = a.bbox_max.min(b.bbox_max);
    if (0..3).any(|k| lo[k] >= hi[k]) {
        return None;
    }
    let ta: Vec<[Vec3; 3]> = a.triangles().map(|(t, _)| t).collect();
    let tb: Vec<[Vec3; 3]> = b.triangles().map(|(t, _)| t).collect();
    let n = 96;
    let (dx, dy) = ((hi.x - lo.x) / n as f32, (hi.y - lo.y) / n as f32);
    let mut volume = 0.0f64;
    for i in 0..n {
        for j in 0..n {
            // Off-center samples avoid hitting mesh edges exactly.
            let x = lo.x + (i as f32 + 0.5 + 0.0137) * dx;
            let y = lo.y + (j as f32 + 0.5 - 0.0071) * dy;
            let intervals = |zs: Vec<f32>| -> Vec<(f32, f32)> { zs.as_chunks::<2>().0.iter().map(|c| (c[0], c[1])).collect() };
            let (ia, ib) = (intervals(z_crossings(&ta, x, y)), intervals(z_crossings(&tb, x, y)));
            for (a0, a1) in &ia {
                for (b0, b1) in &ib {
                    let len = a1.min(*b1) - a0.max(*b0);
                    if len > 0.0 {
                        volume += len as f64;
                    }
                }
            }
        }
    }
    volume *= (dx * dy) as f64;
    if volume <= 1e-6 {
        return None;
    }
    let mut out = BodyMesh { bbox_min: lo, bbox_max: hi, face_count: 1, ..Default::default() };
    for (src, other) in [(&ta, &tb), (&tb, &ta)] {
        for t in src.iter() {
            let c = (t[0] + t[1] + t[2]) / 3.0;
            if (0..3).any(|k| c[k] < lo[k] || c[k] > hi[k]) || !inside(other, c) {
                continue;
            }
            let nrm = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
            for p in t {
                out.indices.push(out.positions.len() as u32);
                out.positions.push(p.to_array());
                out.normals.push(nrm.to_array());
                out.face_ids.push(0);
            }
        }
    }
    Some((volume, out))
}

#[cfg(test)]
mod tests {
    use crate::primitives::{make_box, make_cylinder};
    use crate::Frame;
    use glam::DVec3;

    #[test]
    fn cylinder_fit() {
        let f = Frame::new(DVec3::new(5.0, 0.0, 0.0), DVec3::Y, DVec3::Z);
        let m = crate::tessellate(&make_cylinder(&f, 10.0, 20.0).unwrap());
        let cyls: Vec<_> = (0..m.face_count).filter_map(|i| m.face_cylinder(i)).collect();
        assert!(!cyls.is_empty());
        let c = cyls[0];
        assert!((c.radius - 5.0).abs() < 0.05, "{c:?}");
        assert!((c.length - 20.0).abs() < 0.05, "{c:?}");
        assert!(c.axis.dot(DVec3::X).abs() > 0.999 && !c.internal);
        let b = crate::tessellate(&make_box(&f, 5.0, 5.0, 5.0).unwrap());
        assert!((0..b.face_count).all(|i| b.face_cylinder(i).is_none()));
    }

    #[test]
    fn overlap_box_sphere() {
        use crate::primitives::make_sphere;
        let f = Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y);
        let b = crate::tessellate(&make_box(&f, 50.0, 50.0, 50.0).unwrap());
        // Sphere centred on the box face y = 25: half of it overlaps.
        let s = crate::tessellate(&make_sphere(&Frame::new(DVec3::new(0.0, 25.0, 25.0), DVec3::X, DVec3::Y), 40.0).unwrap());
        let (v, m) = super::mesh_overlap(&b, &s).unwrap();
        let expect = 2.0 / 3.0 * std::f64::consts::PI * 8000.0;
        assert!((v - expect).abs() / expect < 0.03, "{v} vs {expect}");
        assert!(!m.indices.is_empty());
        let far = crate::tessellate(&make_box(&Frame::new(DVec3::new(100.0, 0.0, 0.0), DVec3::X, DVec3::Y), 5.0, 5.0, 5.0).unwrap());
        assert!(super::mesh_overlap(&b, &far).is_none());
    }
}
