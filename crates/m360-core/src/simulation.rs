//! SIMULATION workspace: a linear static stress study of one body.

use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::model::{face_at_point, BodyId, Model};
use m360_kernel::fea::{self, Elastic, FaceLoad, Solution, Study};

/// Direction of a force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadDir {
    /// Pushing into the faces (against their outward normal).
    Normal,
    X,
    Y,
    Z,
}

impl LoadDir {
    pub const ALL: [LoadDir; 4] = [LoadDir::Normal, LoadDir::X, LoadDir::Y, LoadDir::Z];
    pub fn label(self) -> &'static str {
        match self {
            LoadDir::Normal => "Normal to face",
            LoadDir::X => "X Axis",
            LoadDir::Y => "Y Axis",
            LoadDir::Z => "Z Axis",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimLoad {
    /// A point on each loaded face.
    pub faces: Vec<[f64; 3]>,
    /// Total force (N); negative flips the direction.
    pub force: f64,
    pub dir: LoadDir,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimStudy {
    pub body: BodyId,
    pub fixed: Vec<[f64; 3]>,
    pub loads: Vec<SimLoad>,
    /// Mesh elements along the longest side.
    pub resolution: usize,
}

/// Result of a solved study.
#[derive(Debug, Clone)]
pub struct SimResult {
    pub solution: Solution,
    pub material: &'static crate::materials::Material,
}

impl SimResult {
    /// Lowest safety factor (yield strength / peak von Mises stress).
    pub fn min_safety(&self) -> f64 {
        if self.solution.max_vm <= 1e-12 {
            f64::INFINITY
        } else {
            self.material.yield_strength / self.solution.max_vm
        }
    }
}

pub fn solve(study: &SimStudy, model: &Model, doc: &Document) -> Result<SimResult, String> {
    let body = model.body(study.body).ok_or("the studied body no longer exists")?;
    let mesh = &body.mesh;
    let resolve = |pts: &[[f64; 3]]| -> Result<Vec<u32>, String> {
        pts.iter().map(|p| face_at_point(mesh, DVec3::from(*p)).ok_or_else(|| "a face of the study no longer exists".to_string())).collect()
    };
    let fixed = resolve(&study.fixed)?;
    let mut loads = Vec::new();
    for l in &study.loads {
        let faces = resolve(&l.faces)?;
        let dir = match l.dir {
            LoadDir::Normal => {
                // Average outward normal of the faces, pushed inwards.
                let n: DVec3 = faces
                    .iter()
                    .filter_map(|f| {
                        mesh.planar_normal(*f).map(|n| n.as_dvec3()).or_else(|| {
                            let s: glam::Vec3 = mesh.face_ids.iter().zip(&mesh.normals).filter(|(x, _)| *x == f).map(|(_, n)| glam::Vec3::from(*n)).sum();
                            Some(s.normalize_or_zero().as_dvec3())
                        })
                    })
                    .sum();
                -n.normalize_or_zero()
            }
            LoadDir::X => DVec3::X,
            LoadDir::Y => DVec3::Y,
            LoadDir::Z => DVec3::Z,
        };
        loads.push(FaceLoad { faces, force: dir * l.force });
    }
    let material = crate::materials::material(doc.body_meta(study.body).material.as_deref());
    let fea_study = Study { fixed, loads, resolution: study.resolution };
    let solution = fea::solve(mesh, &fea_study, Elastic { youngs: material.youngs, poisson: material.poisson })?;
    Ok(SimResult { solution, material })
}
