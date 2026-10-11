//! Export to exchange formats (STL, STEP).

use std::io::Write;
use std::path::Path;

use truck_stepio::out;

use crate::{BodyMesh, KernelError, Result, Solid};

/// Parse an STL file (binary or ASCII) into triangles.
pub fn read_stl(bytes: &[u8]) -> Result<Vec<[[f32; 3]; 3]>> {
    let ascii = bytes.starts_with(b"solid") && std::str::from_utf8(bytes).is_ok_and(|s| s.contains("facet"));
    if ascii {
        let text = std::str::from_utf8(bytes).map_err(|e| KernelError::Io(e.to_string()))?;
        let mut tris = Vec::new();
        let mut cur: Vec<[f32; 3]> = Vec::new();
        for line in text.lines() {
            let mut it = line.split_whitespace();
            if it.next() == Some("vertex") {
                let v: Vec<f32> = it.filter_map(|x| x.parse().ok()).collect();
                if v.len() == 3 {
                    cur.push([v[0], v[1], v[2]]);
                }
                if cur.len() == 3 {
                    tris.push([cur[0], cur[1], cur[2]]);
                    cur.clear();
                }
            }
        }
        return Ok(tris);
    }
    if bytes.len() < 84 {
        return Err(KernelError::Io("not an STL file".into()));
    }
    let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    if bytes.len() < 84 + n * 50 {
        return Err(KernelError::Io("truncated STL file".into()));
    }
    let f = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    Ok((0..n)
        .map(|i| {
            let o = 84 + i * 50 + 12;
            let v = |k: usize| [f(o + k * 12), f(o + k * 12 + 4), f(o + k * 12 + 8)];
            [v(0), v(1), v(2)]
        })
        .collect())
}

/// Build a polyhedral solid (one planar face per triangle) from a closed,
/// consistently oriented triangle mesh.
pub fn mesh_to_solid(tris: &[[[f32; 3]; 3]]) -> Result<Solid> {
    use std::collections::HashMap;
    use truck_modeling::{builder, Point3, Shell, Wire};
    if tris.is_empty() {
        return Err(KernelError::InvalidParameter("the mesh has no triangles".into()));
    }
    let key = |p: [f32; 3]| [(p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64, (p[2] * 1e4).round() as i64];
    let mut verts: HashMap<[i64; 3], truck_modeling::Vertex> = HashMap::new();
    let mut edges: HashMap<([i64; 3], [i64; 3]), truck_modeling::Edge> = HashMap::new();
    let mut faces = Vec::new();
    for t in tris {
        let ks = [key(t[0]), key(t[1]), key(t[2])];
        if ks[0] == ks[1] || ks[1] == ks[2] || ks[0] == ks[2] {
            continue; // degenerate
        }
        for (k, p) in ks.iter().zip(t) {
            verts.entry(*k).or_insert_with(|| builder::vertex(Point3::new(p[0] as f64, p[1] as f64, p[2] as f64)));
        }
        let mut wire = Wire::new();
        for i in 0..3 {
            let (a, b) = (ks[i], ks[(i + 1) % 3]);
            if let Some(e) = edges.get(&(b, a)) {
                wire.push_back(e.inverse());
            } else {
                let e = edges.entry((a, b)).or_insert_with(|| builder::line(&verts[&a], &verts[&b]));
                wire.push_back(e.clone());
            }
        }
        faces.push(builder::try_attach_plane(&[wire]).map_err(|e| KernelError::OperationFailed(format!("mesh face: {e:?}")))?);
    }
    let shell: Shell = faces.into();
    // One shell per connected part; parts that are not closed are kept for
    // display (they cannot take part in booleans).
    let parts = shell.connected_components();
    let all_closed = parts.iter().all(|p| Solid::try_new(vec![p.clone()]).is_ok());
    let mut solid = if all_closed { Solid::new(parts) } else { Solid::new_unchecked(parts) };
    if all_closed && crate::tessellate(&solid).volume() < 0.0 {
        solid.not();
    }
    Ok(solid)
}

/// Binary STL containing every mesh.
pub fn stl_bytes(meshes: &[&BodyMesh]) -> Vec<u8> {
    let count: usize = meshes.iter().map(|m| m.triangle_count()).sum();
    let mut buf = Vec::with_capacity(84 + count * 50);
    let mut header = [0u8; 80];
    let tag = b"Merge360 binary STL";
    header[..tag.len()].copy_from_slice(tag);
    buf.extend_from_slice(&header);
    buf.extend_from_slice(&(count as u32).to_le_bytes());
    for m in meshes {
        for (t, _) in m.triangles() {
            let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
            for v in [n, t[0], t[1], t[2]] {
                for c in v.to_array() {
                    buf.extend_from_slice(&c.to_le_bytes());
                }
            }
            buf.extend_from_slice(&0u16.to_le_bytes());
        }
    }
    buf
}

pub fn write_stl(path: &Path, meshes: &[&BodyMesh]) -> Result<()> {
    std::fs::write(path, stl_bytes(meshes)).map_err(|e| KernelError::Io(e.to_string()))
}

/// STEP AP203 text for a set of solids.
pub fn step_string(solids: &[&Solid]) -> String {
    let compressed: Vec<_> = solids.iter().map(|s| s.compress()).collect();
    let models: out::StepModels<_, _, _> = compressed.iter().collect();
    out::CompleteStepDisplay::new(
        models,
        out::StepHeaderDescriptor {
            file_name: "merge360".to_owned(),
            organization_system: "Merge360".to_owned(),
            ..Default::default()
        },
    )
    .to_string()
}

pub fn write_step(path: &Path, solids: &[&Solid]) -> Result<()> {
    let s = step_string(solids);
    let mut f = std::fs::File::create(path).map_err(|e| KernelError::Io(e.to_string()))?;
    f.write_all(s.as_bytes()).map_err(|e| KernelError::Io(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::*;
    use glam::DVec3;

    #[test]
    fn stl_has_correct_size() {
        let s = make_box(&Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y), 1.0, 1.0, 1.0).unwrap();
        let m = crate::tessellate(&s);
        let bytes = stl_bytes(&[&m]);
        let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        assert_eq!(n, m.triangle_count());
        assert_eq!(bytes.len(), 84 + n * 50);
        assert!(n >= 12);
    }

    #[test]
    fn stl_roundtrip_to_solid() {
        let s = crate::primitives::make_sphere(&Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y), 20.0).unwrap();
        let m = crate::tessellate(&s);
        let tris = read_stl(&stl_bytes(&[&m])).unwrap();
        assert_eq!(tris.len(), m.triangle_count());
        let t0 = std::time::Instant::now();
        let solid = mesh_to_solid(&tris).unwrap();
        let back = crate::tessellate(&solid);
        assert!((back.volume() - m.volume()).abs() / m.volume() < 1e-3, "{} vs {}", back.volume(), m.volume());
        eprintln!("{} triangles -> solid in {:?}", tris.len(), t0.elapsed());
    }

    #[test]
    fn step_contains_solid() {
        let s = make_box(&Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y), 1.0, 1.0, 1.0).unwrap();
        let text = step_string(&[&s]);
        assert!(text.contains("ISO-10303-21"));
        assert!(text.contains("MANIFOLD_SOLID_BREP"));
    }
}
