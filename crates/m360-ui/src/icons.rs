//! Vector tool icons drawn with the egui painter (no bitmap assets).

use eframe::egui::{self, pos2, Color32, Pos2, Rect, Shape, Stroke};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    NewComponent,
    Sketch,
    Extrude,
    Revolve,
    Hole,
    Pattern,
    Sweep,
    Loft,
    Box,
    Cylinder,
    Sphere,
    Torus,
    Coil,
    Pipe,
    Mirror,
    PressPull,
    Fillet,
    Chamfer,
    Shell,
    Combine,
    Move,
    Delete,
    Appearance,
    Parameters,
    Joint,
    Plane,
    Axis,
    Point,
    Measure,
    Interference,
    Section,
    CenterOfMass,
    Insert,
    Select,
    Print3D,
    Line,
    Rectangle,
    Circle,
    Arc,
    Polygon,
    Spline,
    Dimension,
    Trim,
    ConHorizontal,
    ConCoincident,
    ConTangent,
    ConEqual,
    ConParallel,
    ConPerpendicular,
    FinishSketch,
    Generic,
    // UI glyphs
    Home,
    Orbit,
    Pan,
    Zoom,
    Fit,
    LookAt,
    Display,
    Grid,
    Viewports,
    Eye,
    EyeOff,
    Folder,
    Component,
    Body,
    SketchSmall,
    Gear,
    Camera,
    Save,
    Undo,
    Redo,
    DataPanel,
    FileMenu,
    Bell,
    Help,
    User,
    Plus,
    Close,
    PlayStart,
    PlayBack,
    Play,
    PlayEnd,
}

const BODY_FILL: Color32 = Color32::from_rgb(0xa9, 0xc9, 0xe6);
const BODY_TOP: Color32 = Color32::from_rgb(0xcf, 0xe2, 0xf3);
const BODY_SIDE: Color32 = Color32::from_rgb(0x7f, 0xa9, 0xd1);
const OUTLINE: Color32 = Color32::from_rgb(0x2d, 0x5f, 0x8f);
const ORANGE: Color32 = Color32::from_rgb(0xe8, 0x89, 0x2f);
const GREEN: Color32 = Color32::from_rgb(0x2f, 0xa8, 0x4f);
const RED: Color32 = Color32::from_rgb(0xd6, 0x3b, 0x30);
const GREY: Color32 = Color32::from_rgb(0x5f, 0x63, 0x68);
const DARK: Color32 = Color32::from_rgb(0x3a, 0x3c, 0x40);

struct Pen<'a> {
    painter: &'a egui::Painter,
    rect: Rect,
    tint: Option<Color32>,
}

impl Pen<'_> {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.rect.min.x + x * self.rect.width(), self.rect.min.y + y * self.rect.height())
    }
    fn s(&self) -> f32 {
        self.rect.width() / 32.0
    }
    fn c(&self, c: Color32) -> Color32 {
        self.tint.unwrap_or(c)
    }
    fn stroke(&self, w: f32, c: Color32) -> Stroke {
        Stroke::new(w * self.s().max(0.6), self.c(c))
    }
    fn poly(&self, pts: &[(f32, f32)], fill: Color32, outline: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(Shape::convex_polygon(pts, self.c(fill), self.stroke(1.0, outline)));
    }
    fn line(&self, pts: &[(f32, f32)], w: f32, c: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(Shape::line(pts, self.stroke(w, c)));
    }
    fn circle(&self, x: f32, y: f32, r: f32, fill: Color32, outline: Color32) {
        self.painter.circle(self.p(x, y), r * self.rect.width(), self.c(fill), self.stroke(1.0, outline));
    }
    fn ring(&self, x: f32, y: f32, r: f32, w: f32, c: Color32) {
        self.painter.circle_stroke(self.p(x, y), r * self.rect.width(), self.stroke(w, c));
    }
    fn arc(&self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, w: f32, c: Color32) {
        let n = 24;
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                (cx + r * a.cos(), cy - r * a.sin())
            })
            .collect();
        self.line(&pts, w, c);
    }
    fn arrow(&self, from: (f32, f32), to: (f32, f32), c: Color32) {
        self.line(&[from, to], 1.6, c);
        let d = (to.0 - from.0, to.1 - from.1);
        let len = (d.0 * d.0 + d.1 * d.1).sqrt().max(1e-4);
        let (ux, uy) = (d.0 / len, d.1 / len);
        let h = 0.13;
        let base = (to.0 - ux * h, to.1 - uy * h);
        self.poly(
            &[to, (base.0 - uy * h * 0.6, base.1 + ux * h * 0.6), (base.0 + uy * h * 0.6, base.1 - ux * h * 0.6)],
            c,
            c,
        );
    }
    /// Isometric box occupying (x0..x1, y0..y1).
    fn iso_box(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let w = x1 - x0;
        let dy = w * 0.25;
        let mx = (x0 + x1) / 2.0;
        self.poly(&[(x0, y0 + dy), (mx, y0), (x1, y0 + dy), (mx, y0 + 2.0 * dy)], BODY_TOP, OUTLINE);
        self.poly(&[(x0, y0 + dy), (mx, y0 + 2.0 * dy), (mx, y1), (x0, y1 - dy)], BODY_FILL, OUTLINE);
        self.poly(&[(mx, y0 + 2.0 * dy), (x1, y0 + dy), (x1, y1 - dy), (mx, y1)], BODY_SIDE, OUTLINE);
    }
    fn cylinder(&self, cx: f32, top: f32, bottom: f32, rx: f32) {
        let ry = rx * 0.35;
        let r = Rect::from_min_max(self.p(cx - rx, top), self.p(cx + rx, bottom));
        self.painter.rect_filled(Rect::from_min_max(pos2(r.min.x, self.p(0.0, top).y), pos2(r.max.x, self.p(0.0, bottom).y)), 0.0, self.c(BODY_FILL));
        self.line(&[(cx - rx, top), (cx - rx, bottom)], 1.0, OUTLINE);
        self.line(&[(cx + rx, top), (cx + rx, bottom)], 1.0, OUTLINE);
        self.ellipse(cx, bottom, rx, ry, BODY_FILL, OUTLINE, true);
        self.ellipse(cx, top, rx, ry, BODY_TOP, OUTLINE, false);
    }
    fn ellipse(&self, cx: f32, cy: f32, rx: f32, ry: f32, fill: Color32, outline: Color32, lower_only: bool) {
        let n = 28;
        let (a0, a1) = if lower_only { (std::f32::consts::PI, std::f32::consts::TAU) } else { (0.0, std::f32::consts::TAU) };
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / n as f32;
                (cx + rx * a.cos(), cy - ry * a.sin())
            })
            .collect();
        if !lower_only {
            self.poly(&pts[..n], fill, outline);
        } else {
            self.line(&pts, 1.0, outline);
        }
    }
}

/// Paint `icon` into `rect`. `tint` forces a single color (used for
/// monochrome UI glyphs and disabled state).
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, tint: Option<Color32>) {
    let pen = Pen { painter, rect, tint };
    let q = &pen;
    use Icon::*;
    match icon {
        Box => q.iso_box(0.15, 0.15, 0.85, 0.85),
        Extrude => {
            q.poly(&[(0.1, 0.72), (0.5, 0.58), (0.9, 0.72), (0.5, 0.86)], BODY_SIDE, OUTLINE);
            q.iso_box(0.2, 0.32, 0.8, 0.78);
            q.arrow((0.5, 0.5), (0.5, 0.06), GREEN);
        }
        PressPull => {
            q.iso_box(0.15, 0.3, 0.75, 0.85);
            q.arrow((0.68, 0.35), (0.9, 0.08), GREEN);
            q.arrow((0.68, 0.35), (0.45, 0.08), GREEN);
        }
        Cylinder => q.cylinder(0.5, 0.25, 0.8, 0.3),
        Sphere => {
            q.circle(0.5, 0.5, 0.34, BODY_FILL, OUTLINE);
            q.circle(0.4, 0.4, 0.1, BODY_TOP, BODY_TOP);
            q.ellipse(0.5, 0.5, 0.34, 0.1, Color32::TRANSPARENT, OUTLINE, true);
        }
        Torus => {
            q.ellipse(0.5, 0.52, 0.38, 0.22, BODY_FILL, OUTLINE, false);
            q.ellipse(0.5, 0.48, 0.14, 0.06, Color32::WHITE, OUTLINE, false);
        }
        Revolve => {
            q.cylinder(0.55, 0.3, 0.8, 0.25);
            q.line(&[(0.15, 0.1), (0.15, 0.95)], 1.2, ORANGE);
            q.arc(0.55, 0.18, 0.28, 200.0, 340.0, 1.5, GREEN);
        }
        Hole => {
            q.iso_box(0.12, 0.2, 0.88, 0.85);
            q.ellipse(0.5, 0.38, 0.14, 0.06, DARK, OUTLINE, false);
        }
        Pattern => {
            for (x, y) in [(0.08, 0.08), (0.52, 0.08), (0.08, 0.52), (0.52, 0.52)] {
                q.iso_box(x, y, x + 0.4, y + 0.4);
            }
        }
        Sweep => {
            q.line(&[(0.15, 0.85), (0.35, 0.55), (0.6, 0.45), (0.85, 0.2)], 3.5, BODY_SIDE);
            q.line(&[(0.15, 0.85), (0.35, 0.55), (0.6, 0.45), (0.85, 0.2)], 1.0, ORANGE);
            q.circle(0.15, 0.85, 0.08, BODY_TOP, OUTLINE);
        }
        Loft => {
            q.poly(&[(0.2, 0.75), (0.8, 0.75), (0.65, 0.25), (0.35, 0.25)], BODY_FILL, OUTLINE);
            q.ellipse(0.5, 0.25, 0.15, 0.05, BODY_TOP, OUTLINE, false);
            q.line(&[(0.2, 0.75), (0.8, 0.75)], 1.5, ORANGE);
        }
        Coil => {
            for i in 0..4 {
                let y = 0.2 + i as f32 * 0.17;
                q.ellipse(0.5, y, 0.3, 0.08, Color32::TRANSPARENT, OUTLINE, true);
                q.arc(0.5, y, 0.3, 0.0, 180.0, 1.0, BODY_SIDE);
            }
        }
        Pipe => {
            q.line(&[(0.15, 0.8), (0.4, 0.8), (0.6, 0.6), (0.6, 0.2)], 5.0, BODY_FILL);
            q.line(&[(0.15, 0.8), (0.4, 0.8), (0.6, 0.6), (0.6, 0.2)], 1.0, OUTLINE);
        }
        Mirror => {
            q.iso_box(0.05, 0.3, 0.42, 0.75);
            q.iso_box(0.58, 0.3, 0.95, 0.75);
            q.line(&[(0.5, 0.1), (0.5, 0.92)], 1.2, ORANGE);
        }
        NewComponent | Component => {
            q.iso_box(0.15, 0.2, 0.75, 0.8);
            if icon == NewComponent {
                q.circle(0.78, 0.25, 0.14, GREEN, GREEN);
                q.line(&[(0.7, 0.25), (0.86, 0.25)], 1.6, Color32::WHITE);
                q.line(&[(0.78, 0.17), (0.78, 0.33)], 1.6, Color32::WHITE);
            }
        }
        Sketch | SketchSmall => {
            q.poly(&[(0.05, 0.7), (0.5, 0.52), (0.95, 0.7), (0.5, 0.88)], Color32::from_rgb(0xf6, 0xe7, 0xd6), ORANGE);
            q.line(&[(0.25, 0.7), (0.5, 0.62), (0.72, 0.72)], 1.4, ORANGE);
            if icon == Sketch {
                q.line(&[(0.45, 0.6), (0.85, 0.12)], 3.0, GREY);
                q.line(&[(0.45, 0.6), (0.5, 0.52)], 1.5, DARK);
            }
        }
        Fillet => {
            q.poly(&[(0.15, 0.85), (0.15, 0.2), (0.45, 0.2), (0.85, 0.55), (0.85, 0.85)], BODY_FILL, OUTLINE);
            q.arc(0.45, 0.6, 0.4, 0.0, 90.0, 2.0, ORANGE);
        }
        Chamfer => {
            q.poly(&[(0.15, 0.85), (0.15, 0.2), (0.55, 0.2), (0.85, 0.5), (0.85, 0.85)], BODY_FILL, OUTLINE);
            q.line(&[(0.55, 0.2), (0.85, 0.5)], 2.0, ORANGE);
        }
        Shell => {
            q.iso_box(0.12, 0.15, 0.88, 0.85);
            q.poly(&[(0.3, 0.33), (0.5, 0.26), (0.7, 0.33), (0.5, 0.4)], Color32::WHITE, OUTLINE);
        }
        Combine => {
            q.iso_box(0.08, 0.3, 0.6, 0.85);
            q.circle(0.65, 0.38, 0.25, BODY_SIDE.gamma_multiply(0.8), OUTLINE);
        }
        Move => {
            q.iso_box(0.3, 0.3, 0.7, 0.7);
            q.arrow((0.5, 0.5), (0.95, 0.5), RED);
            q.arrow((0.5, 0.5), (0.5, 0.05), GREEN);
            q.arrow((0.5, 0.5), (0.15, 0.85), Color32::from_rgb(0x2f, 0x6f, 0xd6));
        }
        Delete | Close => {
            let c = if icon == Delete { RED } else { GREY };
            q.line(&[(0.25, 0.25), (0.75, 0.75)], 2.4, c);
            q.line(&[(0.75, 0.25), (0.25, 0.75)], 2.4, c);
        }
        Appearance => {
            q.circle(0.5, 0.5, 0.34, Color32::from_rgb(0xe0, 0x5d, 0x4f), OUTLINE);
            q.poly(&[(0.5, 0.5), (0.84, 0.5), (0.74, 0.74), (0.5, 0.84)], Color32::from_rgb(0x4f, 0x9d, 0xe0), Color32::TRANSPARENT);
            q.circle(0.38, 0.38, 0.08, Color32::WHITE, Color32::WHITE);
        }
        Parameters => {
            q.line(&[(0.2, 0.25), (0.8, 0.25)], 1.4, GREY);
            q.line(&[(0.2, 0.5), (0.8, 0.5)], 1.4, GREY);
            q.line(&[(0.2, 0.75), (0.8, 0.75)], 1.4, GREY);
            q.circle(0.35, 0.25, 0.07, Color32::WHITE, OUTLINE);
            q.circle(0.65, 0.5, 0.07, Color32::WHITE, OUTLINE);
            q.circle(0.45, 0.75, 0.07, Color32::WHITE, OUTLINE);
        }
        Joint => {
            q.iso_box(0.05, 0.45, 0.45, 0.9);
            q.iso_box(0.55, 0.1, 0.95, 0.55);
            q.line(&[(0.25, 0.55), (0.75, 0.25)], 1.4, ORANGE);
            q.circle(0.5, 0.4, 0.07, ORANGE, ORANGE);
        }
        Plane => q.poly(&[(0.05, 0.6), (0.45, 0.3), (0.95, 0.4), (0.55, 0.72)], Color32::from_rgb(0xf3, 0xc9, 0x9a), ORANGE),
        Axis => {
            q.line(&[(0.15, 0.85), (0.85, 0.15)], 1.8, ORANGE);
            q.circle(0.5, 0.5, 0.05, ORANGE, ORANGE);
        }
        Point => {
            q.circle(0.5, 0.5, 0.1, ORANGE, DARK);
        }
        Measure => {
            q.poly(&[(0.08, 0.62), (0.62, 0.08), (0.92, 0.38), (0.38, 0.92)], Color32::from_rgb(0xf2, 0xd0, 0x6b), DARK);
            for i in 0..5 {
                let f = 0.2 + i as f32 * 0.1;
                q.line(&[(f, 0.95 - f - 0.1 + 0.02), (f + 0.06, 0.95 - f - 0.04)], 1.0, DARK);
            }
        }
        Interference => {
            q.circle(0.38, 0.5, 0.26, BODY_FILL, OUTLINE);
            q.circle(0.62, 0.5, 0.26, BODY_SIDE.gamma_multiply(0.7), OUTLINE);
            q.circle(0.5, 0.5, 0.08, RED, RED);
        }
        Section => {
            q.iso_box(0.15, 0.2, 0.85, 0.85);
            q.poly(&[(0.05, 0.55), (0.5, 0.35), (0.95, 0.55), (0.5, 0.75)], Color32::from_rgba_unmultiplied(232, 137, 47, 140), ORANGE);
        }
        CenterOfMass => {
            q.circle(0.5, 0.5, 0.3, Color32::WHITE, DARK);
            q.poly(&[(0.5, 0.5), (0.8, 0.5), (0.5, 0.2)], DARK, DARK);
            q.poly(&[(0.5, 0.5), (0.2, 0.5), (0.5, 0.8)], DARK, DARK);
        }
        Insert => {
            q.poly(&[(0.2, 0.1), (0.65, 0.1), (0.8, 0.25), (0.8, 0.9), (0.2, 0.9)], Color32::WHITE, GREY);
            q.arrow((0.5, 0.3), (0.5, 0.75), Color32::from_rgb(0x2f, 0x6f, 0xd6));
        }
        Select => {
            q.poly(&[(0.3, 0.12), (0.3, 0.8), (0.45, 0.65), (0.75, 0.62)], Color32::WHITE, DARK);
            q.line(&[(0.45, 0.65), (0.6, 0.9)], 1.5, DARK);
        }
        Print3D => {
            q.poly(&[(0.15, 0.85), (0.85, 0.85), (0.85, 0.75), (0.15, 0.75)], GREY, DARK);
            q.iso_box(0.3, 0.35, 0.7, 0.75);
            q.line(&[(0.5, 0.05), (0.5, 0.3)], 2.0, DARK);
        }
        Line => {
            q.line(&[(0.15, 0.8), (0.85, 0.2)], 1.6, ORANGE);
            q.circle(0.15, 0.8, 0.06, Color32::WHITE, DARK);
            q.circle(0.85, 0.2, 0.06, Color32::WHITE, DARK);
        }
        Rectangle => {
            q.line(&[(0.15, 0.25), (0.85, 0.25), (0.85, 0.75), (0.15, 0.75), (0.15, 0.25)], 1.6, ORANGE);
            q.circle(0.15, 0.75, 0.06, Color32::WHITE, DARK);
            q.circle(0.85, 0.25, 0.06, Color32::WHITE, DARK);
        }
        Circle => {
            q.ring(0.5, 0.5, 0.34, 1.6, ORANGE);
            q.circle(0.5, 0.5, 0.05, Color32::WHITE, DARK);
            q.line(&[(0.5, 0.5), (0.74, 0.26)], 1.0, DARK);
        }
        Arc => {
            q.arc(0.5, 0.65, 0.35, 10.0, 170.0, 1.6, ORANGE);
            q.circle(0.155, 0.59, 0.05, Color32::WHITE, DARK);
            q.circle(0.845, 0.59, 0.05, Color32::WHITE, DARK);
        }
        Polygon => {
            let pts: Vec<(f32, f32)> = (0..=6)
                .map(|i| {
                    let a = (i as f32 * 60.0 + 30.0).to_radians();
                    (0.5 + 0.36 * a.cos(), 0.5 + 0.36 * a.sin())
                })
                .collect();
            q.line(&pts, 1.6, ORANGE);
        }
        Spline => {
            q.line(&[(0.1, 0.7), (0.3, 0.35), (0.5, 0.5), (0.7, 0.65), (0.9, 0.3)], 1.6, ORANGE);
        }
        Dimension => {
            q.line(&[(0.15, 0.3), (0.15, 0.8)], 1.0, DARK);
            q.line(&[(0.85, 0.3), (0.85, 0.8)], 1.0, DARK);
            q.arrow((0.5, 0.5), (0.17, 0.5), DARK);
            q.arrow((0.5, 0.5), (0.83, 0.5), DARK);
        }
        Trim => {
            q.line(&[(0.1, 0.5), (0.9, 0.5)], 1.6, ORANGE);
            q.line(&[(0.5, 0.15), (0.5, 0.85)], 1.6, ORANGE);
            q.line(&[(0.6, 0.35), (0.85, 0.65)], 1.2, RED);
            q.line(&[(0.85, 0.35), (0.6, 0.65)], 1.2, RED);
        }
        ConHorizontal => {
            q.line(&[(0.15, 0.6), (0.85, 0.6)], 1.8, ORANGE);
            q.line(&[(0.35, 0.3), (0.65, 0.3)], 1.4, GREEN);
        }
        ConCoincident => {
            q.line(&[(0.1, 0.8), (0.5, 0.5), (0.9, 0.7)], 1.6, ORANGE);
            q.circle(0.5, 0.5, 0.08, GREEN, DARK);
        }
        ConTangent => {
            q.ring(0.45, 0.6, 0.25, 1.6, ORANGE);
            q.line(&[(0.1, 0.35), (0.9, 0.35)], 1.6, GREEN);
        }
        ConEqual => {
            q.line(&[(0.25, 0.4), (0.75, 0.4)], 2.0, GREEN);
            q.line(&[(0.25, 0.6), (0.75, 0.6)], 2.0, GREEN);
        }
        ConParallel => {
            q.line(&[(0.25, 0.8), (0.55, 0.2)], 1.8, GREEN);
            q.line(&[(0.45, 0.8), (0.75, 0.2)], 1.8, GREEN);
        }
        ConPerpendicular => {
            q.line(&[(0.2, 0.8), (0.8, 0.8)], 1.8, GREEN);
            q.line(&[(0.5, 0.8), (0.5, 0.2)], 1.8, GREEN);
        }
        FinishSketch => {
            q.circle(0.5, 0.5, 0.4, GREEN, GREEN);
            q.line(&[(0.3, 0.5), (0.45, 0.66), (0.72, 0.35)], 2.6, Color32::WHITE);
        }
        Generic => {
            q.iso_box(0.2, 0.2, 0.8, 0.8);
            q.circle(0.75, 0.75, 0.12, Color32::WHITE, GREY);
        }
        Body => {
            q.iso_box(0.15, 0.15, 0.85, 0.85);
        }
        Home => {
            q.poly(&[(0.5, 0.12), (0.9, 0.5), (0.1, 0.5)], DARK, DARK);
            q.poly(&[(0.22, 0.48), (0.78, 0.48), (0.78, 0.88), (0.22, 0.88)], DARK, DARK);
            q.poly(&[(0.42, 0.62), (0.58, 0.62), (0.58, 0.88), (0.42, 0.88)], Color32::WHITE, Color32::WHITE);
        }
        Orbit => {
            q.ellipse(0.5, 0.5, 0.38, 0.16, Color32::TRANSPARENT, DARK, true);
            q.arc(0.5, 0.5, 0.38, 0.0, 160.0, 1.4, DARK);
            q.circle(0.5, 0.5, 0.12, DARK, DARK);
        }
        Pan => {
            q.poly(&[(0.3, 0.45), (0.3, 0.85), (0.72, 0.85), (0.8, 0.5), (0.7, 0.45)], Color32::WHITE, DARK);
            for x in [0.34, 0.46, 0.58] {
                q.poly(&[(x, 0.2), (x + 0.1, 0.2), (x + 0.1, 0.55), (x, 0.55)], Color32::WHITE, DARK);
            }
        }
        Zoom => {
            q.ring(0.42, 0.42, 0.24, 1.8, DARK);
            q.line(&[(0.6, 0.6), (0.88, 0.88)], 2.6, DARK);
        }
        Fit => {
            q.ring(0.42, 0.42, 0.24, 1.8, DARK);
            q.line(&[(0.6, 0.6), (0.88, 0.88)], 2.6, DARK);
            q.poly(&[(0.32, 0.32), (0.52, 0.32), (0.52, 0.52), (0.32, 0.52)], DARK, DARK);
        }
        LookAt => {
            q.poly(&[(0.1, 0.65), (0.5, 0.45), (0.9, 0.65), (0.5, 0.85)], Color32::WHITE, DARK);
            q.arrow((0.5, 0.08), (0.5, 0.6), DARK);
        }
        Display => {
            q.poly(&[(0.1, 0.18), (0.9, 0.18), (0.9, 0.7), (0.1, 0.7)], Color32::WHITE, DARK);
            q.line(&[(0.5, 0.7), (0.5, 0.85)], 1.6, DARK);
            q.line(&[(0.3, 0.86), (0.7, 0.86)], 1.6, DARK);
        }
        Grid => {
            for f in [0.2, 0.4, 0.6, 0.8] {
                q.line(&[(f, 0.15), (f, 0.85)], 1.0, DARK);
                q.line(&[(0.15, f), (0.85, f)], 1.0, DARK);
            }
        }
        Viewports => {
            q.poly(&[(0.12, 0.15), (0.88, 0.15), (0.88, 0.85), (0.12, 0.85)], Color32::WHITE, DARK);
            q.line(&[(0.5, 0.15), (0.5, 0.85)], 1.2, DARK);
            q.line(&[(0.12, 0.5), (0.88, 0.5)], 1.2, DARK);
        }
        Eye | EyeOff => {
            let c = if icon == Eye { DARK } else { Color32::from_gray(170) };
            let top: Vec<(f32, f32)> = (0..=12).map(|i| {
                let t = i as f32 / 12.0;
                (0.1 + 0.8 * t, 0.5 - 0.28 * (t * std::f32::consts::PI).sin())
            }).collect();
            let bot: Vec<(f32, f32)> = top.iter().map(|&(x, y)| (x, 1.0 - y)).collect();
            q.line(&top, 1.2, c);
            q.line(&bot, 1.2, c);
            q.circle(0.5, 0.5, 0.12, c, c);
            if icon == EyeOff {
                q.line(&[(0.15, 0.85), (0.85, 0.15)], 1.2, c);
            }
        }
        Folder => {
            q.poly(&[(0.08, 0.25), (0.4, 0.25), (0.48, 0.33), (0.92, 0.33), (0.92, 0.82), (0.08, 0.82)], Color32::from_rgb(0xf3, 0xc8, 0x6b), Color32::from_rgb(0xb8, 0x8a, 0x2c));
        }
        Gear => {
            q.ring(0.5, 0.5, 0.22, 3.0, GREY);
            for i in 0..8 {
                let a = (i as f32 * 45.0).to_radians();
                q.line(&[(0.5 + 0.26 * a.cos(), 0.5 + 0.26 * a.sin()), (0.5 + 0.38 * a.cos(), 0.5 + 0.38 * a.sin())], 2.6, GREY);
            }
        }
        Camera => {
            q.poly(&[(0.1, 0.3), (0.9, 0.3), (0.9, 0.8), (0.1, 0.8)], GREY, GREY);
            q.circle(0.5, 0.55, 0.16, Color32::WHITE, GREY);
            q.poly(&[(0.35, 0.3), (0.42, 0.18), (0.58, 0.18), (0.65, 0.3)], GREY, GREY);
        }
        Save => {
            q.poly(&[(0.15, 0.15), (0.75, 0.15), (0.85, 0.25), (0.85, 0.85), (0.15, 0.85)], Color32::TRANSPARENT, APP_FG);
            q.poly(&[(0.3, 0.15), (0.65, 0.15), (0.65, 0.38), (0.3, 0.38)], Color32::TRANSPARENT, APP_FG);
            q.poly(&[(0.28, 0.55), (0.72, 0.55), (0.72, 0.85), (0.28, 0.85)], Color32::TRANSPARENT, APP_FG);
        }
        Undo | Redo => {
            let flip = |x: f32| if icon == Undo { x } else { 1.0 - x };
            q.arc(flip(0.5) , 0.55, 0.28, if icon == Undo { -90.0 } else { 90.0 }, if icon == Undo { 120.0 } else { 300.0 }, 1.8, APP_FG);
            q.poly(&[(flip(0.12), 0.38), (flip(0.38), 0.3), (flip(0.3), 0.55)], APP_FG, APP_FG);
        }
        DataPanel => {
            for (x, y) in [(0.15, 0.15), (0.55, 0.15), (0.15, 0.55), (0.55, 0.55)] {
                q.poly(&[(x, y), (x + 0.3, y), (x + 0.3, y + 0.3), (x, y + 0.3)], APP_FG, APP_FG);
            }
        }
        FileMenu => {
            q.poly(&[(0.2, 0.1), (0.62, 0.1), (0.8, 0.28), (0.8, 0.9), (0.2, 0.9)], Color32::TRANSPARENT, APP_FG);
            q.line(&[(0.62, 0.1), (0.62, 0.28), (0.8, 0.28)], 1.0, APP_FG);
        }
        Bell => {
            q.poly(&[(0.25, 0.7), (0.3, 0.35), (0.5, 0.2), (0.7, 0.35), (0.75, 0.7)], Color32::TRANSPARENT, APP_FG);
            q.line(&[(0.15, 0.72), (0.85, 0.72)], 1.4, APP_FG);
            q.circle(0.5, 0.82, 0.06, APP_FG, APP_FG);
        }
        Help => {
            q.ring(0.5, 0.5, 0.38, 1.4, APP_FG);
            q.arc(0.5, 0.4, 0.13, -60.0, 180.0, 1.6, APP_FG);
            q.line(&[(0.5, 0.52), (0.5, 0.62)], 1.6, APP_FG);
            q.circle(0.5, 0.74, 0.04, APP_FG, APP_FG);
        }
        User => {
            q.circle(0.5, 0.5, 0.42, ACCENT_ICON, ACCENT_ICON);
            q.circle(0.5, 0.4, 0.13, Color32::WHITE, Color32::WHITE);
            q.arc(0.5, 0.82, 0.24, 20.0, 160.0, 2.4, Color32::WHITE);
        }
        Plus => {
            q.line(&[(0.5, 0.2), (0.5, 0.8)], 1.8, APP_FG);
            q.line(&[(0.2, 0.5), (0.8, 0.5)], 1.8, APP_FG);
        }
        PlayStart => {
            q.line(&[(0.25, 0.25), (0.25, 0.75)], 1.8, DARK);
            q.poly(&[(0.75, 0.25), (0.75, 0.75), (0.32, 0.5)], DARK, DARK);
        }
        PlayBack => q.poly(&[(0.7, 0.25), (0.7, 0.75), (0.3, 0.5)], DARK, DARK),
        Play => q.poly(&[(0.3, 0.25), (0.3, 0.75), (0.7, 0.5)], DARK, DARK),
        PlayEnd => {
            q.poly(&[(0.25, 0.25), (0.25, 0.75), (0.68, 0.5)], DARK, DARK);
            q.line(&[(0.75, 0.25), (0.75, 0.75)], 1.8, DARK);
        }
    }
}

const APP_FG: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);
const ACCENT_ICON: Color32 = Color32::from_rgb(0x06, 0x96, 0xd7);

/// Allocate a square icon button; returns its response.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, size: f32, tooltip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    paint(ui.painter(), rect.shrink(size * 0.12), icon, None);
    if tooltip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tooltip)
    }
}
