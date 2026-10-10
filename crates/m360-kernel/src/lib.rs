//! Thin geometry layer over the `truck` B-rep kernel.
//!
//! Everything in Merge360 is modeled in millimetres in a right-handed,
//! Y-up world (Fusion's default orientation).

pub mod io;
pub mod mesh;
pub mod ops;
pub mod primitives;
pub mod profile;

pub use mesh::{tessellate, BodyMesh};
pub use primitives::Frame;

/// A B-rep solid produced by the kernel.
pub type Solid = truck_modeling::Solid;

/// Error type for kernel operations.
#[derive(Debug, Clone, PartialEq)]
pub enum KernelError {
    InvalidParameter(String),
    OperationFailed(String),
    Io(String),
}

impl std::fmt::Display for KernelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KernelError::InvalidParameter(s) => write!(f, "invalid parameter: {s}"),
            KernelError::OperationFailed(s) => write!(f, "operation failed: {s}"),
            KernelError::Io(s) => write!(f, "I/O error: {s}"),
        }
    }
}

impl std::error::Error for KernelError {}

pub type Result<T> = std::result::Result<T, KernelError>;
