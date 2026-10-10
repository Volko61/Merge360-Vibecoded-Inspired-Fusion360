//! Per-frame scene description handed to the renderer.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use m360_kernel::BodyMesh;

use crate::Camera;

/// Convert an sRGB color (0..1) with alpha to linear.
pub fn srgb(r: f32, g: f32, b: f32, a: f32) -> [f32; 4] {
    let f = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    [f(r), f(g), f(b), a]
}

/// A screen-space-width line segment (colors are linear).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct LineInst {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub color: [f32; 4],
    /// Width in logical pixels.
    pub width: f32,
}

impl LineInst {
    pub fn new(a: Vec3, b: Vec3, color: [f32; 4], width: f32) -> Self {
        Self { a: a.to_array(), b: b.to_array(), color, width }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct FlatVertex {
    pub pos: [f32; 3],
    pub color: [f32; 4],
}

impl FlatVertex {
    pub fn new(p: Vec3, color: [f32; 4]) -> Self {
        Self { pos: p.to_array(), color }
    }
}

/// A body to draw with its display state.
#[derive(Clone)]
pub struct BodyDraw {
    pub mesh: Arc<BodyMesh>,
    /// Linear RGBA.
    pub color: [f32; 4],
    pub edge_color: [f32; 4],
    /// Whole-body highlight (rgb + strength).
    pub highlight: [f32; 4],
    pub hover_face: Option<u32>,
    pub selected_faces: Vec<u32>,
    pub show_faces: bool,
    pub show_edges: bool,
}

impl BodyDraw {
    pub fn new(mesh: Arc<BodyMesh>, color: [f32; 4]) -> Self {
        Self {
            mesh,
            color,
            edge_color: srgb(0.08, 0.08, 0.1, 1.0),
            highlight: [0.0; 4],
            hover_face: None,
            selected_faces: Vec::new(),
            show_faces: true,
            show_edges: true,
        }
    }
}

pub struct Scene {
    pub camera: Camera,
    pub bg_top: [f32; 4],
    pub bg_bottom: [f32; 4],
    pub bodies: Vec<BodyDraw>,
    /// Depth-tested lines (grid, sketch curves, axes).
    pub lines: Vec<LineInst>,
    /// Lines drawn on top of everything (manipulators).
    pub overlay_lines: Vec<LineInst>,
    /// Depth-tested translucent triangles (origin planes, previews).
    pub tris: Vec<FlatVertex>,
    /// Triangles drawn on top of everything (manipulator heads).
    pub overlay_tris: Vec<FlatVertex>,
    pub pixels_per_point: f32,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            camera: Camera::home(),
            bg_top: srgb(0.97, 0.97, 0.98, 1.0),
            bg_bottom: srgb(0.80, 0.82, 0.85, 1.0),
            bodies: Vec::new(),
            lines: Vec::new(),
            overlay_lines: Vec::new(),
            tris: Vec::new(),
            overlay_tris: Vec::new(),
            pixels_per_point: 1.0,
        }
    }
}
