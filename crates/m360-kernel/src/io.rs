//! Export to exchange formats (STL, STEP).

use std::io::Write;
use std::path::Path;

use truck_stepio::out;

use crate::{BodyMesh, KernelError, Result, Solid};

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
    fn step_contains_solid() {
        let s = make_box(&Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y), 1.0, 1.0, 1.0).unwrap();
        let text = step_string(&[&s]);
        assert!(text.contains("ISO-10303-21"));
        assert!(text.contains("MANIFOLD_SOLID_BREP"));
    }
}
