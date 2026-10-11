//! Thin geometry layer over the `truck` B-rep kernel.
//!
//! Everything in Merge360 is modeled in millimetres in a right-handed,
//! Y-up world (Fusion's default orientation).

pub mod coil;
pub mod io;
pub mod mesh;
pub mod ops;
pub mod primitives;
pub mod profile;
pub mod shell;
pub mod surface;
pub mod sweep;
pub mod fillet;
pub mod hole;

pub use mesh::{mesh_overlap, tessellate, tessellate_smooth, BodyMesh, Cylinder, EdgeInfo, SMOOTH_COS};
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

/// Run kernel code that may panic inside `truck`, converting panics into
/// `None` without printing the panic message (expected failures are
/// reported to the user as feature errors instead).
pub fn quiet_catch<T>(f: impl FnOnce() -> T) -> Option<T> {
    use std::cell::Cell;
    use std::sync::Once;
    thread_local!(static QUIET: Cell<u32> = const { Cell::new(0) });
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if QUIET.with(|q| q.get()) == 0 {
                default(info);
            }
        }));
    });
    QUIET.with(|q| q.set(q.get() + 1));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok();
    QUIET.with(|q| q.set(q.get() - 1));
    r
}
