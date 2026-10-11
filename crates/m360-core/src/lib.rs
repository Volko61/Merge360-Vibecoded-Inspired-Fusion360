//! Merge360 document model: parametric feature history, regeneration and undo.

pub mod document;
pub mod expr;
pub mod feature;
pub mod history;
pub mod model;
pub mod sketch;
pub mod solver;
pub mod units;

pub use document::{BodyMeta, ComponentDef, Document, NamedView};
pub use feature::{Axis, BasePlane, ExtrudeOp, Feature, FeatureKind, HoleType, JointGeo, JointMotion, PatternKind, PlaneRef, RevolveAxis, SketchEntity};
pub use history::History;
pub use m360_kernel::ops::BooleanOp;
pub use model::{Body, BodyId, Model, Regenerator};
pub use units::Units;
