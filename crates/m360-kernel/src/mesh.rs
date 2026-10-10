//! Tessellation of B-rep solids into render/pick friendly meshes.

use glam::{Vec3, DVec3};
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
    pub face_count: u32,
    pub bbox_min: Vec3,
    pub bbox_max: Vec3,
}

impl BodyMesh {
    /// Signed volume via the divergence theorem (mm³).
    pub fn volume(&self) -> f64 {
        self.indices
            .chunks_exact(3)
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
            .chunks_exact(3)
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
        self.indices.chunks_exact(3).map(|t| {
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

pub fn tessellate_with(solid: &Solid, tol: f64) -> BodyMesh {
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

    for edge in meshed.edge_iter() {
        let curve = edge.curve();
        let pts: Vec<[f32; 3]> = curve
            .iter()
            .map(|p| [p.x as f32, p.y as f32, p.z as f32])
            .collect();
        if pts.len() >= 2 {
            out.edges.push(pts);
        }
    }

    if out.positions.is_empty() {
        out.bbox_min = Vec3::ZERO;
        out.bbox_max = Vec3::ZERO;
    }
    out
}
