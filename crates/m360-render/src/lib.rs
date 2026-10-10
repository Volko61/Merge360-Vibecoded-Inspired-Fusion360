//! Offscreen wgpu viewport renderer for Merge360.
//!
//! The scene is rendered into an MSAA texture that egui then displays as an
//! image, which keeps 3D depth handling completely separate from egui's pass.

pub mod camera;
mod renderer;
pub mod scene;

pub use camera::{Camera, Ray};
pub use renderer::Viewport3D;
pub use scene::{srgb, BodyDraw, FlatVertex, LineInst, Scene};
