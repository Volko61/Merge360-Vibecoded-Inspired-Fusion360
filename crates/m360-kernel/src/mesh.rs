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
    let meshed = solid.robust_triangulation(tol);
    let mut out = BodyMesh {
        bbox_min: Vec3::splat(f32::MAX),
        bbox_max: Vec3::splat(f32::MIN),
        ..Default::default()
    };

    for (face_id, face) in meshed.face_iter().enumerate() {
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
    for (fi, face) in meshed.face_iter().enumerate() {
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
    for edge in meshed.edge_iter() {
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
