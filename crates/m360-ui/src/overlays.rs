//! Canvas overlays: ViewCube, navigation bar, comments, toasts, sketch palette.

use eframe::egui::{self, pos2, vec2, Color32, Pos2, RichText, Sense, Stroke};
use glam::{Mat3, Vec3};

use crate::app::{Action, Merge360App, ToastKind};
use crate::icons::{self, Icon};
use crate::state::{Environment, NavMode, VisualStyle};
use crate::theme;
use crate::i18n::tr;

struct CubeFace {
    name: &'static str,
    n: Vec3,
    a: Vec3,
    b: Vec3,
}

const FACES: [CubeFace; 6] = [
    CubeFace { name: "FRONT", n: Vec3::Z, a: Vec3::X, b: Vec3::Y },
    CubeFace { name: "BACK", n: Vec3::NEG_Z, a: Vec3::NEG_X, b: Vec3::Y },
    CubeFace { name: "RIGHT", n: Vec3::X, a: Vec3::NEG_Z, b: Vec3::Y },
    CubeFace { name: "LEFT", n: Vec3::NEG_X, a: Vec3::Z, b: Vec3::Y },
    CubeFace { name: "TOP", n: Vec3::Y, a: Vec3::X, b: Vec3::NEG_Z },
    CubeFace { name: "BOTTOM", n: Vec3::NEG_Y, a: Vec3::X, b: Vec3::Z },
];

const EDGE: f32 = 0.64;

fn point_in_convex(p: Pos2, poly: &[Pos2]) -> bool {
    let mut sign = 0.0f32;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let c = (b - a).x * (p - a).y - (b - a).y * (p - a).x;
        if c.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = c.signum();
        } else if c.signum() != sign {
            return false;
        }
    }
    true
}

impl Merge360App {
    pub(crate) fn overlays(&mut self, ctx: &egui::Context) {
        if self.flags.workspace == crate::tools::WS_MANUFACTURE {
            self.cam_browser(ctx);
        } else {
            self.browser(ctx);
        }
        if self.flags.workspace == crate::tools::WS_ANIMATION {
            self.storyboard_ui(ctx);
        }
        if self.prefs.show_view_cube {
            self.view_cube(ctx);
        }
        self.nav_bar(ctx);
        self.comments(ctx);
        self.sketch_palette(ctx);
        self.toasts_ui(ctx);
        self.marking_menu(ctx);
        self.toolbox(ctx);
        self.dim_edit_ui(ctx);
    }

    fn view_cube(&mut self, ctx: &egui::Context) {
        let size = 110.0;
        let rect = egui::Rect::from_min_size(pos2(self.canvas_rect.right() - size - 18.0, self.canvas_rect.top() + 14.0), vec2(size, size));
        let now = self.now();
        egui::Area::new(egui::Id::new("viewcube")).fixed_pos(rect.min).order(egui::Order::Middle).show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
            let painter = ui.painter();
            let cam = self.tabs[self.active].camera;
            let rot = Mat3::from_mat4(cam.view());
            let center = rect.center() + vec2(4.0, 6.0);
            let scale = size * 0.27;
            let proj = |p: Vec3| {
                let v = rot * p;
                (center + vec2(v.x, -v.y) * scale, v.z)
            };

            // Ground compass ring.
            let ring: Vec<Pos2> = (0..=48)
                .map(|i| {
                    let a = i as f32 / 48.0 * std::f32::consts::TAU;
                    proj(Vec3::new(a.cos() * 1.75, -1.0, a.sin() * 1.75)).0
                })
                .collect();
            painter.add(egui::Shape::line(ring, Stroke::new(1.5, Color32::from_rgba_unmultiplied(120, 124, 130, 120))));

            let hover = resp.hover_pos();
            let mut faces: Vec<(f32, &CubeFace)> = FACES.iter().filter(|f| (rot * f.n).z > 0.02).map(|f| ((rot * f.n).z, f)).collect();
            faces.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let mut hovered_dir: Option<Vec3> = None;
            // Find hovered cell (front-most face first).
            for (_, f) in faces.iter().rev() {
                if hovered_dir.is_some() {
                    break;
                }
                for i in 0..3 {
                    for j in 0..3 {
                        let r = [(-1.0, -EDGE), (-EDGE, EDGE), (EDGE, 1.0)];
                        let (u0, u1) = r[i];
                        let (v0, v1) = r[j];
                        let corners = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)].map(|(u, v)| proj(f.n + f.a * u + f.b * v).0);
                        if hover.is_some_and(|h| point_in_convex(h, &corners)) {
                            let du = [-1.0, 0.0, 1.0][i];
                            let dv = [-1.0, 0.0, 1.0][j];
                            hovered_dir = Some(f.n + f.a * du + f.b * dv);
                        }
                    }
                }
            }
            for (facing, f) in &faces {
                let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(u, v)| proj(f.n + f.a * u + f.b * v).0);
                let shade = (200.0 + 45.0 * facing) as u8;
                painter.add(egui::Shape::convex_polygon(corners.to_vec(), Color32::from_rgb(shade, shade, shade.saturating_add(3)), Stroke::new(1.0, Color32::from_rgb(0x8a, 0x8d, 0x92))));
                // Highlight hovered cells belonging to this face.
                if let Some(d) = hovered_dir {
                    for i in 0..3 {
                        for j in 0..3 {
                            let du = [-1.0, 0.0, 1.0][i];
                            let dv = [-1.0, 0.0, 1.0][j];
                            let dir = f.n + f.a * du + f.b * dv;
                            // Cells of other faces that map to the same edge/corner share the direction.
                            let matches = (dir.normalize() - d.normalize()).length() < 1e-3;
                            if matches {
                                let r = [(-1.0, -EDGE), (-EDGE, EDGE), (EDGE, 1.0)];
                                let (u0, u1) = r[i];
                                let (v0, v1) = r[j];
                                let c = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)].map(|(u, v)| proj(f.n + f.a * u + f.b * v).0);
                                painter.add(egui::Shape::convex_polygon(c.to_vec(), Color32::from_rgba_unmultiplied(6, 150, 215, 140), Stroke::NONE));
                            }
                        }
                    }
                }
                if *facing > 0.3 {
                    let (c, _) = proj(f.n);
                    let (ax, _) = proj(f.n + f.a);
                    let d = ax - c;
                    let angle = d.y.atan2(d.x);
                    let font = egui::FontId::proportional(10.0 * facing.sqrt());
                    let galley = painter.layout_no_wrap(tr(f.name).to_string(), font, Color32::from_rgb(0x3a, 0x3c, 0x40));
                    let half = galley.size() / 2.0;
                    let rot2 = egui::emath::Rot2::from_angle(angle);
                    let pos = c - rot2 * half;
                    painter.add(egui::epaint::TextShape::new(pos, galley, Color32::from_rgb(0x3a, 0x3c, 0x40)).with_angle(angle));
                }
            }
            // Axis triad at the cube corner.
            let origin = Vec3::new(-1.0, -1.0, 1.0);
            for (d, c) in [(Vec3::X, Color32::from_rgb(214, 59, 48)), (Vec3::Y, Color32::from_rgb(47, 168, 79)), (Vec3::Z, Color32::from_rgb(47, 111, 214))] {
                painter.line_segment([proj(origin).0, proj(origin + d * 0.9).0], Stroke::new(2.0, c));
            }

            if resp.dragged() {
                let d = resp.drag_delta();
                let tab = self.tab();
                tab.anim = None;
                tab.camera.orbit(glam::Vec2::new(d.x, d.y));
            } else if resp.clicked() {
                if let Some(dir) = hovered_dir {
                    let tab = self.tab();
                    let mut target = tab.camera;
                    target.look_from(dir);
                    if dir.x.abs() < 1e-4 && dir.z.abs() < 1e-4 {
                        let q = std::f32::consts::FRAC_PI_2;
                        target.yaw = (tab.camera.yaw / q).round() * q;
                    }
                    tab.animate_to(target, now);
                }
            }
            if hovered_dir.is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            resp.context_menu(|ui| {
                if ui.button(tr("Go Home")).clicked() {
                    let tab = self.tab();
                    let h = tab.home;
                    tab.animate_to(h, now);
                }
                let ortho = self.tabs[self.active].camera.ortho;
                if ui.radio(!ortho, tr("Perspective")).clicked() {
                    self.tab().camera.ortho = false;
                }
                if ui.radio(ortho, tr("Orthographic")).clicked() {
                    self.tab().camera.ortho = true;
                }
                ui.separator();
                if ui.button(tr("Set current view as Home")).clicked() {
                    let tab = self.tab();
                    tab.home = tab.camera;
                }
                if ui.button(tr("Reset Home")).clicked() {
                    self.tab().home = m360_render::Camera::home();
                }
                if ui.button(tr("Fit to View")).clicked() {
                    self.actions.push(Action::Fit);
                }
            });

            // Home button.
            let home = egui::Rect::from_min_size(rect.min, vec2(18.0, 18.0));
            let hr = ui.interact(home, ui.id().with("home"), Sense::click());
            if hr.hovered() {
                ui.painter().rect_filled(home, 2.0, theme::HOVER_BG);
            }
            icons::paint(ui.painter(), home.shrink(2.0), Icon::Home, None);
            if hr.on_hover_text(tr("Home")).clicked() {
                let aspect = self.canvas_rect.aspect_ratio();
                let tab = self.tab();
                let mut h = tab.home;
                if tab.model.bodies.is_empty() {
                    h.target = Vec3::ZERO;
                } else {
                    let (min, max) = crate::viewport::scene_bounds(tab);
                    h.fit(min, max, aspect);
                    h.yaw = tab.home.yaw;
                    h.pitch = tab.home.pitch;
                }
                self.tab().animate_to(h, now);
            }
        });
    }

    fn nav_bar(&mut self, ctx: &egui::Context) {
        let pos = pos2(self.canvas_rect.center().x, self.canvas_rect.bottom() - 10.0);
        egui::Area::new(egui::Id::new("navbar"))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(pos)
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(0xfb, 0xfb, 0xfb, 235))
                    .stroke(Stroke::new(1.0, theme::PANEL_BORDER))
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(4, 2))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 1.0;
                            let nav = self.tabs[self.active].nav;
                            let toggle = |ui: &mut egui::Ui, icon: Icon, mode: NavMode, tip: &str, app: &mut Self| {
                                let r = nav_icon(ui, icon, nav == mode, tip);
                                if r.clicked() {
                                    app.tab().nav = if nav == mode { NavMode::Select } else { mode };
                                }
                            };
                            toggle(ui, Icon::Orbit, NavMode::Orbit, "Orbit (Shift + Middle mouse)", self);
                            if nav_icon(ui, Icon::LookAt, false, "Look At").clicked() {
                                self.look_at_selection();
                            }
                            toggle(ui, Icon::Pan, NavMode::Pan, "Pan (Middle mouse)", self);
                            toggle(ui, Icon::Zoom, NavMode::Zoom, "Zoom (Mouse wheel)", self);
                            if nav_icon(ui, Icon::Fit, false, "Fit (F6)").clicked() {
                                self.actions.push(Action::Fit);
                            }
                            nav_sep(ui);
                            let disp = nav_icon(ui, Icon::Display, false, "Display Settings");
                            egui::Popup::menu(&disp).show(|ui| self.display_menu(ui));
                            let grid = nav_icon(ui, Icon::Grid, false, "Grid and Snaps");
                            egui::Popup::menu(&grid).show(|ui| {
                                let tab = self.tab();
                                ui.checkbox(&mut tab.display.grid, tr("Layout Grid"));
                                ui.checkbox(&mut tab.display.snap, tr("Snap to Grid"));
                                ui.label(RichText::new(format!("Grid spacing: {}", tab.doc.units.format(crate::viewport::grid_step(&tab.camera) as f64))).color(theme::TEXT_DIM));
                            });
                            let vp = nav_icon(ui, Icon::Viewports, false, "Viewports");
                            egui::Popup::menu(&vp).show(|ui| {
                                ui.add_enabled(false, egui::Button::new(tr("Single Viewport ✓")));
                                if ui.button(tr("Multiple Views")).clicked() {
                                    self.toast(ToastKind::Info, tr("Multiple viewports are not available yet."));
                                }
                            });
                        });
                    });
            });
    }

    fn display_menu(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(240.0);
        let tab = self.tab();
        ui.menu_button("Visual Style", |ui| {
            for s in VisualStyle::ALL {
                if ui.radio(tab.display.style == s, tr(s.label())).clicked() {
                    tab.display.style = s;
                }
            }
        });
        ui.menu_button("Environment", |ui| {
            for e in Environment::ALL {
                if ui.radio(tab.display.environment == e, tr(e.label())).clicked() {
                    tab.display.environment = e;
                }
            }
        });
        ui.menu_button("Camera", |ui| {
            if ui.radio(!tab.camera.ortho, tr("Perspective")).clicked() {
                tab.camera.ortho = false;
            }
            if ui.radio(tab.camera.ortho, tr("Orthographic")).clicked() {
                tab.camera.ortho = true;
            }
        });
        ui.separator();
        ui.checkbox(&mut tab.display.grid, tr("Show Layout Grid"));
        let mut origin = tab.doc.origin_visible;
        if ui.checkbox(&mut origin, tr("Show Origin")).changed() {
            tab.doc.origin_visible = origin;
        }
        ui.separator();
        ui.checkbox(&mut self.flags.browser_open, tr("Show Browser"));
        ui.checkbox(&mut self.flags.timeline_open, tr("Show Timeline"));
        ui.checkbox(&mut self.flags.comments_open, tr("Show Comments"));
    }

    /// Look straight at the selected planar face / plane / sketch.
    fn look_at_selection(&mut self) {
        let now = self.now();
        let tab = self.tab();
        let plane = tab.selection.iter().find_map(|s| match s {
            crate::state::SelItem::Plane(b) => Some(m360_core::PlaneRef { base: *b, offset: 0.0, flip: false, custom: None }),
            crate::state::SelItem::FacePlane(_, _, p) => Some(*p),
            crate::state::SelItem::Sketch(u) => tab.sketch_entities(*u).map(|x| x.0),
            _ => None,
        });
        let face_normal = tab.selection.iter().find_map(|s| match s {
            crate::state::SelItem::Face(b, f) => {
                let body = tab.model.body(*b)?;
                body.mesh.triangles().find(|(_, id)| id == f).map(|(t, _)| (t[1] - t[0]).cross(t[2] - t[0]).normalize())
            }
            _ => None,
        });
        if let Some(p) = plane {
            crate::app::look_at_plane(tab, p, now);
        } else if let Some(n) = face_normal {
            let mut c = tab.camera;
            c.look_from(n);
            tab.animate_to(c, now);
        } else if let Some(s) = &tab.sketch {
            if let Some((p, _)) = tab.sketch_entities(s.uid) {
                crate::app::look_at_plane(tab, p, now);
            }
        } else {
            self.toast(ToastKind::Info, tr("Select a face or plane to look at."));
        }
    }

    fn comments(&mut self, ctx: &egui::Context) {
        let pos = pos2(self.canvas_rect.left() + 8.0, self.canvas_rect.bottom() - 10.0);
        egui::Area::new(egui::Id::new("comments")).pivot(egui::Align2::LEFT_BOTTOM).fixed_pos(pos).order(egui::Order::Middle).show(ctx, |ui| {
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(0xfb, 0xfb, 0xfb, 235))
                .stroke(Stroke::new(1.0, theme::PANEL_BORDER))
                .corner_radius(3)
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.set_width(200.0);
                    ui.horizontal(|ui| {
                        let arrow = if self.flags.comments_open { "▼" } else { "▶" };
                        if ui.add(egui::Button::new(RichText::new(arrow).size(9.0)).frame(false)).clicked() {
                            self.flags.comments_open = !self.flags.comments_open;
                        }
                        ui.label(RichText::new(tr("COMMENTS")).size(11.5).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(egui::Button::new(tr("⊕")).frame(false)).on_hover_text(tr("Add comment")).clicked() {
                                self.flags.comments_open = true;
                            }
                        });
                    });
                    if self.flags.comments_open {
                        let tab = self.tab();
                        for c in &tab.comments {
                            ui.label(RichText::new(c).small());
                        }
                        let id = egui::Id::new("comment_draft");
                        let mut draft: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
                        let r = ui.add(egui::TextEdit::singleline(&mut draft).hint_text("Add a comment…").desired_width(184.0));
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !draft.trim().is_empty() {
                            tab.comments.push(draft.trim().to_string());
                            draft.clear();
                        }
                        ui.data_mut(|d| d.insert_temp(id, draft));
                    }
                });
        });
    }

    fn sketch_palette(&mut self, ctx: &egui::Context) {
        if self.tabs[self.active].sketch.is_none() || self.tabs[self.active].command.is_some() {
            return;
        }
        let pos = pos2(self.canvas_rect.right() - 248.0, self.canvas_rect.top() + 150.0);
        egui::Area::new(egui::Id::new("sketch_palette")).fixed_pos(pos).order(egui::Order::Middle).show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme::PANEL_BG)
                .stroke(Stroke::new(1.0, theme::PANEL_BORDER))
                .corner_radius(3)
                .shadow(ui.visuals().window_shadow)
                .show(ui, |ui| {
                    ui.set_width(230.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(230.0, 26.0), Sense::hover());
                    ui.painter().rect_filled(rect, egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, theme::DIALOG_HEADER);
                    ui.painter().text(rect.left_center() + vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, "SKETCH PALETTE", egui::FontId::proportional(12.5), Color32::WHITE);
                    egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                        let tab = self.tab();
                        let tool = tab.sketch.as_ref().map(|s| s.tool);
                        ui.label(RichText::new(tr("Tools")).color(theme::TEXT_DIM));
                        ui.horizontal_wrapped(|ui| {
                            for (icon, t, id) in [
                                (Icon::Select, crate::state::SketchTool::Select, "select"),
                                (Icon::Line, crate::state::SketchTool::Line, "sk_line"),
                                (Icon::Rectangle, crate::state::SketchTool::Rect, "sk_rect"),
                                (Icon::Rectangle, crate::state::SketchTool::CenterRect, "sk_center_rect"),
                                (Icon::Circle, crate::state::SketchTool::Circle, "sk_circle"),
                                (Icon::Arc, crate::state::SketchTool::Arc, "sk_arc"),
                                (Icon::Polygon, crate::state::SketchTool::Polygon, "sk_polygon"),
                                (Icon::Dimension, crate::state::SketchTool::Dimension, "sk_dimension"),
                                (Icon::Point, crate::state::SketchTool::Point, "sk_point"),
                            ] {
                                if nav_icon(ui, icon, tool == Some(t), tr(t.label())).clicked() {
                                    if t == crate::state::SketchTool::Select {
                                        if let Some(s) = &mut self.tab().sketch {
                                            s.tool = t;
                                            s.pending.clear();
                                        }
                                    } else {
                                        self.actions.push(Action::Tool(id));
                                    }
                                }
                            }
                        });
                        ui.separator();
                        ui.label(RichText::new(tr("Options")).color(theme::TEXT_DIM));
                        let tab = self.tab();
                        ui.checkbox(&mut tab.display.grid, tr("Sketch Grid"));
                        ui.checkbox(&mut tab.display.snap, tr("Snap"));
                        if ui.button(tr("Look At")).clicked() {
                            let now = self.now();
                            let tab = self.tab();
                            if let Some(s) = &tab.sketch {
                                if let Some((p, _)) = tab.sketch_entities(s.uid) {
                                    crate::app::look_at_plane(tab, p, now);
                                }
                            }
                        }
                        ui.add_space(6.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let b = egui::Button::new(RichText::new(tr("Finish Sketch")).color(Color32::WHITE)).fill(theme::SKETCH_GREEN).min_size(vec2(100.0, 24.0));
                            if ui.add(b).clicked() {
                                self.actions.push(Action::FinishSketch);
                            }
                        });
                    });
                });
        });
    }

    pub(crate) fn toasts_ui_pub(&mut self, ctx: &egui::Context) {
        self.toasts_ui(ctx);
    }

    fn toasts_ui(&mut self, ctx: &egui::Context) {
        let now = self.now();
        for t in &mut self.toasts {
            if t.time.is_nan() {
                t.time = now;
            }
        }
        self.toasts.retain(|t| now - t.time < 4.0);
        if self.toasts.is_empty() {
            return;
        }
        let pos = pos2(self.canvas_rect.right() - 12.0, self.canvas_rect.bottom() - 12.0);
        egui::Area::new(egui::Id::new("toasts")).pivot(egui::Align2::RIGHT_BOTTOM).fixed_pos(pos).order(egui::Order::Tooltip).interactable(false).show(ctx, |ui| {
            for t in self.toasts.iter().rev().take(4) {
                let fade = ((4.0 - (now - t.time)) / 0.5).clamp(0.0, 1.0) as f32;
                let accent = match t.kind {
                    ToastKind::Info => theme::ACCENT,
                    ToastKind::Warning => theme::WARNING,
                    ToastKind::Error => theme::ERROR,
                };
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(0x2e, 0x2f, 0x33, (235.0 * fade) as u8))
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(4.0, 16.0), Sense::hover());
                            ui.painter().rect_filled(r, 1.0, accent.gamma_multiply(fade));
                            ui.label(RichText::new(&t.text).color(Color32::WHITE.gamma_multiply(fade)));
                        });
                    });
                ui.add_space(4.0);
            }
        });
    }
}

fn nav_icon(ui: &mut egui::Ui, icon: Icon, active: bool, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    if active {
        ui.painter().rect_filled(rect, 3.0, theme::SELECT_BG);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, theme::HOVER_BG);
    }
    icons::paint(ui.painter(), rect.shrink(5.0), icon, None);
    resp.on_hover_text(tip)
}

fn nav_sep(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 24.0), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range().shrink(3.0), Stroke::new(1.0, theme::PANEL_BORDER));
}
