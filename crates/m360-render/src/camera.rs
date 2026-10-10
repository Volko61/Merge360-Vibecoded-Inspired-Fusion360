//! Orbit camera (Y-up, like Fusion's default).

use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec2, Vec3};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub target: Vec3,
    /// Rotation around +Y (radians). 0 looks from +Z (Front).
    pub yaw: f32,
    /// Elevation (radians), +π/2 looks straight down (Top).
    pub pitch: f32,
    pub distance: f32,
    pub ortho: bool,
    pub fov_y: f32,
}

pub const MAX_PITCH: f32 = std::f32::consts::FRAC_PI_2 - 1e-4;

impl Default for Camera {
    fn default() -> Self {
        Self::home()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

impl Camera {
    /// Fusion's "Home" view: front-right-top isometric.
    pub fn home() -> Self {
        Self {
            target: Vec3::ZERO,
            yaw: 45f32.to_radians(),
            pitch: 30f32.to_radians(),
            distance: 250.0,
            ortho: false,
            fov_y: 35f32.to_radians(),
        }
    }

    pub fn forward(&self) -> Vec3 {
        -self.offset_dir()
    }

    fn offset_dir(&self) -> Vec3 {
        Vec3::new(
            self.pitch.cos() * self.yaw.sin(),
            self.pitch.sin(),
            self.pitch.cos() * self.yaw.cos(),
        )
    }

    pub fn eye(&self) -> Vec3 {
        self.target + self.offset_dir() * self.distance
    }

    pub fn right(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin())
    }

    pub fn up(&self) -> Vec3 {
        self.right().cross(self.forward()).normalize()
    }

    pub fn view(&self) -> Mat4 {
        look_at_mat4(self.eye(), self.target, self.up())
    }

    fn clip_planes(&self) -> (f32, f32) {
        let near = (self.distance * 0.005).max(1e-3);
        let far = self.distance * 200.0 + 1000.0;
        (near, far)
    }

    /// Half height of the visible region at the target depth.
    pub fn half_height(&self) -> f32 {
        self.distance * (self.fov_y * 0.5).tan()
    }

    pub fn proj(&self, aspect: f32) -> Mat4 {
        let (near, far) = self.clip_planes();
        if self.ortho {
            let h = self.half_height();
            let w = h * aspect;
            directx::orthographic(-w, w, -h, h, -far, far)
        } else {
            directx::perspective(self.fov_y, aspect, near, far)
        }
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.proj(aspect) * self.view()
    }

    /// Ray through a point given in normalized device coords (-1..1, y up).
    pub fn ray(&self, ndc: Vec2, aspect: f32) -> Ray {
        let inv = self.view_proj(aspect).inverse();
        let a = inv.project_point3(ndc.extend(0.0));
        let b = inv.project_point3(ndc.extend(0.5));
        let dir = (b - a).normalize();
        let origin = if self.ortho { a - dir * self.distance * 100.0 } else { self.eye() };
        Ray { origin, dir }
    }

    /// Project a world point to NDC (x, y, depth). `None` if behind the camera.
    pub fn project(&self, p: Vec3, aspect: f32) -> Option<Vec3> {
        let c = self.view_proj(aspect) * p.extend(1.0);
        (c.w > 1e-6).then(|| c.truncate() / c.w)
    }

    /// World-space size of one screen pixel at the target depth.
    pub fn world_per_pixel(&self, viewport_h: f32) -> f32 {
        2.0 * self.half_height() / viewport_h.max(1.0)
    }

    pub fn orbit(&mut self, delta: Vec2) {
        self.yaw -= delta.x * 0.008;
        self.pitch = (self.pitch + delta.y * 0.008).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Pan by a screen delta in pixels.
    pub fn pan(&mut self, delta: Vec2, viewport_h: f32) {
        let s = self.world_per_pixel(viewport_h);
        self.target += (-self.right() * delta.x + self.up() * delta.y) * s;
    }

    /// Zoom by `factor` (<1 zooms in) keeping the point under `ndc` fixed.
    pub fn zoom_at(&mut self, factor: f32, ndc: Vec2, aspect: f32) {
        let factor = factor.clamp(0.2, 5.0);
        let h = self.half_height();
        let anchor = self.target + self.right() * ndc.x * h * aspect + self.up() * ndc.y * h;
        self.target = anchor + (self.target - anchor) * factor;
        self.distance = (self.distance * factor).clamp(0.05, 1.0e6);
    }

    /// Frame a bounding box.
    pub fn fit(&mut self, min: Vec3, max: Vec3, aspect: f32) {
        if !(min.x <= max.x) {
            self.target = Vec3::ZERO;
            self.distance = 250.0;
            return;
        }
        let center = (min + max) * 0.5;
        let radius = ((max - min).length() * 0.5).max(1.0);
        let half_fov = (self.fov_y * 0.5).min((self.fov_y * 0.5).tan().atan2(1.0 / aspect.min(1.0)));
        self.target = center;
        self.distance = radius / half_fov.sin() * 1.1;
    }

    /// Standard views by ViewCube direction (from target to eye).
    pub fn look_from(&mut self, dir: Vec3) {
        let d = dir.normalize();
        self.pitch = d.y.asin().clamp(-MAX_PITCH, MAX_PITCH);
        if d.x.abs() > 1e-5 || d.z.abs() > 1e-5 {
            self.yaw = d.x.atan2(d.z);
        }
    }

    /// Interpolate between two cameras (for animated view transitions).
    pub fn lerp(a: &Camera, b: &Camera, t: f32) -> Camera {
        let t = t * t * (3.0 - 2.0 * t);
        let mut dyaw = (b.yaw - a.yaw) % std::f32::consts::TAU;
        if dyaw > std::f32::consts::PI {
            dyaw -= std::f32::consts::TAU;
        } else if dyaw < -std::f32::consts::PI {
            dyaw += std::f32::consts::TAU;
        }
        Camera {
            target: a.target.lerp(b.target, t),
            yaw: a.yaw + dyaw * t,
            pitch: a.pitch + (b.pitch - a.pitch) * t,
            distance: a.distance * (b.distance / a.distance).powf(t),
            ortho: if t < 0.5 { a.ortho } else { b.ortho },
            fov_y: a.fov_y + (b.fov_y - a.fov_y) * t,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_view_looks_down_neg_z() {
        let mut c = Camera::home();
        c.look_from(Vec3::Z);
        assert!((c.forward() - Vec3::NEG_Z).length() < 1e-4);
        assert!((c.right() - Vec3::X).length() < 1e-4);
    }

    #[test]
    fn ray_hits_target_at_center() {
        let c = Camera::home();
        let r = c.ray(Vec2::ZERO, 1.5);
        let to_target = (c.target - r.origin).normalize();
        assert!(to_target.dot(r.dir) > 0.9999);
    }

    #[test]
    fn project_roundtrip() {
        let c = Camera::home();
        let p = Vec3::new(10.0, 5.0, -3.0);
        let ndc = c.project(p, 1.3).unwrap();
        let r = c.ray(ndc.truncate(), 1.3);
        let t = (p - r.origin).dot(r.dir);
        assert!((r.origin + r.dir * t - p).length() < 1e-2);
    }
}
