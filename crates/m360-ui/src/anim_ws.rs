//! ANIMATION workspace: storyboard of body moves (exploded views), playback
//! and video frame export.

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use glam::DMat4;
use m360_core::animation::{AnimStep, Storyboard};
use m360_core::Axis;

use crate::app::{Merge360App, ToastKind};
use crate::i18n::tr;
use crate::theme;

/// Frames per second of published videos.
const FPS: f64 = 24.0;

#[derive(Default)]
pub struct AnimUi {
    pub time: f64,
    pub playing: bool,
    pub step: Option<usize>,
    /// Video export in progress: (next frame, frame count, folder).
    pub publishing: Option<(usize, usize, PathBuf)>,
}

impl Merge360App {
    /// Transform of a body in the ANIMATION workspace at the current time.
    pub(crate) fn anim_transform(&self, body: m360_core::BodyId) -> DMat4 {
        let tab = &self.tabs[self.active];
        if tab.doc.animation.steps.is_empty() {
            return DMat4::IDENTITY;
        }
        let Some(b) = tab.model.body(body) else { return DMat4::IDENTITY };
        let c = ((b.mesh.bbox_min + b.mesh.bbox_max) * 0.5).as_dvec3();
        tab.doc.animation.transform(body, c, self.anim.time)
    }

    pub(crate) fn anim_action(&mut self, id: &str) {
        self.flags.workspace = crate::tools::WS_ANIMATION;
        let tab = &self.tabs[self.active];
        let mut bodies = tab.selected_bodies();
        let end = tab.doc.animation.length();
        match id {
            "anim_transform" => {
                if bodies.is_empty() {
                    return self.toast(ToastKind::Info, tr("Select bodies first."));
                }
                let tab = self.tab();
                tab.doc.animation.steps.push(AnimStep { bodies, start: end, duration: 1.0, translate: [0.0, 50.0, 0.0], axis: Axis::Y, angle_deg: 0.0 });
                tab.dirty = true;
                self.anim.step = Some(self.tabs[self.active].doc.animation.steps.len() - 1);
            }
            "anim_explode" => {
                if bodies.len() < 2 {
                    bodies = tab.model.bodies.iter().map(|b| b.id).filter(|id| tab.body_visible(*id)).collect();
                }
                let steps = Storyboard::auto_explode(&tab.model, &bodies, end, 0.3);
                if steps.is_empty() {
                    return self.toast(ToastKind::Info, tr("Auto Explode needs at least two bodies."));
                }
                let tab = self.tab();
                tab.doc.animation.steps.extend(steps);
                tab.dirty = true;
            }
            "anim_play" => {
                self.anim.playing = !self.anim.playing;
                if self.anim.time >= self.tabs[self.active].doc.animation.length() {
                    self.anim.time = 0.0;
                }
            }
            "anim_publish" => {
                let len = self.tabs[self.active].doc.animation.length();
                if len <= 0.0 {
                    return self.toast(ToastKind::Info, tr("The storyboard is empty."));
                }
                let name = self.tabs[self.active].doc.name.replace(['/', '\\', ' '], "_");
                let dir = self.default_dir().join(format!("{name}-animation"));
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    return self.toast(ToastKind::Error, format!("{e}"));
                }
                let frames = (len * FPS).ceil() as usize + 1;
                self.anim.playing = false;
                self.anim.publishing = Some((0, frames, dir));
            }
            _ => {}
        }
    }

    /// Advance playback / video export (called once per frame, before the
    /// canvas is drawn).
    pub(crate) fn anim_tick(&mut self, ctx: &egui::Context) {
        if self.flags.workspace != crate::tools::WS_ANIMATION {
            return;
        }
        let len = self.tabs[self.active].doc.animation.length();
        if let Some((frame, total, dir)) = self.anim.publishing.clone() {
            if self.render_settings.capture.is_none() {
                if frame >= total {
                    self.anim.publishing = None;
                    self.toast(ToastKind::Info, format!("{} {} ({} {})", tr("Video frames saved in"), dir.display(), total, tr("frames, 24 fps")));
                } else {
                    self.anim.time = frame as f64 / FPS;
                    let (w, h, _) = crate::render_ws::RESOLUTIONS[0];
                    self.render_settings.capture = Some((w, h));
                    self.render_settings.capture_path = Some(dir.join(format!("frame_{:04}.png", frame + 1)));
                    self.anim.publishing = Some((frame + 1, total, dir));
                }
            }
            ctx.request_repaint();
            return;
        }
        if self.anim.playing {
            let dt = ctx.input(|i| i.stable_dt).min(0.1) as f64;
            self.anim.time += dt;
            if self.anim.time >= len {
                self.anim.time = len;
                self.anim.playing = false;
            }
            ctx.request_repaint();
        }
    }

    pub(crate) fn animation_toolbar(&mut self, ui: &mut egui::Ui) {
        let len = self.tabs[self.active].doc.animation.length();
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(tr("TRANSFORM")).small().strong().color(theme::TEXT_DIM));
            if ui.button(tr("Transform Components")).clicked() {
                self.anim_action("anim_transform");
            }
            if ui.button(tr("Auto Explode")).clicked() {
                self.anim_action("anim_explode");
            }
            ui.separator();
            ui.label(RichText::new(tr("PLAYBACK")).small().strong().color(theme::TEXT_DIM));
            if ui.button(if self.anim.playing { "⏸" } else { "▶" }).on_hover_text(tr("Play")).clicked() {
                self.anim_action("anim_play");
            }
            ui.add(egui::Slider::new(&mut self.anim.time, 0.0..=len.max(0.01)).show_value(false));
            ui.label(format!("{:.1} / {:.1} s", self.anim.time, len));
            ui.separator();
            ui.label(RichText::new(tr("PUBLISH")).small().strong().color(theme::TEXT_DIM));
            let busy = self.anim.publishing.is_some();
            if ui.add_enabled(!busy, egui::Button::new(tr("Publish Video"))).clicked() {
                self.anim_action("anim_publish");
            }
            if let Some((f, n, _)) = &self.anim.publishing {
                ui.label(format!("{f}/{n}"));
            }
        });
    }

    /// Storyboard editor (bottom of the canvas).
    pub(crate) fn storyboard_ui(&mut self, ctx: &egui::Context) {
        let units = self.tabs[self.active].doc.units;
        let names: Vec<String> = {
            let tab = &self.tabs[self.active];
            tab.doc.animation.steps.iter().map(|s| s.bodies.iter().map(|b| tab.doc.body_meta(*b).name).collect::<Vec<_>>().join(", ")).collect()
        };
        let mut steps = self.tabs[self.active].doc.animation.steps.clone();
        let mut delete = None;
        let rect = self.canvas_rect;
        egui::Window::new(tr("Storyboard"))
            .id(egui::Id::new("storyboard"))
            .default_pos(rect.left_bottom() + egui::vec2(240.0, -230.0))
            .default_size([760.0, 180.0])
            .collapsible(true)
            .show(ctx, |ui| {
                if steps.is_empty() {
                    ui.label(RichText::new(tr("Select bodies and click Transform Components, or use Auto Explode.")).color(theme::TEXT_DIM));
                    return;
                }
                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    egui::Grid::new("steps").striped(true).num_columns(7).show(ui, |ui| {
                        for h in ["Bodies", "Start", "Duration", "Move X / Y / Z", "Axis", "Angle", ""] {
                            ui.label(RichText::new(tr(h)).strong());
                        }
                        ui.end_row();
                        for (i, s) in steps.iter_mut().enumerate() {
                            if ui.selectable_label(self.anim.step == Some(i), &names[i]).clicked() {
                                self.anim.step = Some(i);
                            }
                            ui.add(egui::DragValue::new(&mut s.start).range(0.0..=600.0).speed(0.05).suffix(" s"));
                            ui.add(egui::DragValue::new(&mut s.duration).range(0.0..=600.0).speed(0.05).suffix(" s"));
                            ui.horizontal(|ui| {
                                for (k, v) in s.translate.iter_mut().enumerate() {
                                    crate::widgets::length_field(ui, ("anim_t", i, k), v, units);
                                }
                            });
                            egui::ComboBox::from_id_salt(("anim_axis", i)).selected_text(format!("{:?}", s.axis)).width(50.0).show_ui(ui, |ui| {
                                for a in [Axis::X, Axis::Y, Axis::Z] {
                                    ui.selectable_value(&mut s.axis, a, format!("{a:?}"));
                                }
                            });
                            crate::widgets::angle_field(ui, ("anim_a", i), &mut s.angle_deg);
                            if ui.small_button("🗑").on_hover_text(tr("Delete")).clicked() {
                                delete = Some(i);
                            }
                            ui.end_row();
                        }
                    });
                });
            });
        if let Some(i) = delete {
            steps.remove(i);
            self.anim.step = None;
        }
        let tab = self.tab();
        if tab.doc.animation.steps != steps {
            tab.doc.animation.steps = steps;
            tab.dirty = true;
        }
    }
}
