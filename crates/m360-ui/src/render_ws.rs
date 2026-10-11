//! RENDER workspace: presentation view (no grid/edges, ground shadow) and
//! high-resolution image capture.

use eframe::egui::{self, RichText};

use crate::app::{Merge360App, ToastKind};
use crate::state::Environment;
use crate::theme;
use crate::i18n::tr;

pub const RESOLUTIONS: [(u32, u32, &str); 4] =
    [(1280, 720, "1280 × 720 (HD)"), (1920, 1080, "1920 × 1080 (Full HD)"), (2560, 1440, "2560 × 1440 (QHD)"), (3840, 2160, "3840 × 2160 (4K)")];

#[derive(Debug, Clone)]
pub struct RenderSettings {
    pub resolution: usize,
    pub ground_shadow: bool,
    pub show_edges: bool,
    /// Pending capture request (processed by the canvas, which owns the GPU).
    pub capture: Option<(u32, u32)>,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self { resolution: 1, ground_shadow: true, show_edges: false, capture: None }
    }
}

impl Merge360App {
    pub(crate) fn render_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(tr("SETUP")).small().strong().color(theme::TEXT_DIM));
            let env = self.tabs[self.active].display.environment;
            egui::ComboBox::from_id_salt("render_env").selected_text(tr(env.label())).show_ui(ui, |ui| {
                for e in Environment::ALL {
                    if ui.selectable_label(env == e, tr(e.label())).clicked() {
                        self.tab().display.environment = e;
                    }
                }
            });
            ui.checkbox(&mut self.render_settings.ground_shadow, tr("Ground shadow"));
            ui.checkbox(&mut self.render_settings.show_edges, tr("Show edges"));
            ui.separator();
            ui.label(RichText::new(tr("RENDER")).small().strong().color(theme::TEXT_DIM));
            let res = self.render_settings.resolution;
            egui::ComboBox::from_id_salt("render_res").selected_text(RESOLUTIONS[res].2).show_ui(ui, |ui| {
                for (i, r) in RESOLUTIONS.iter().enumerate() {
                    ui.selectable_value(&mut self.render_settings.resolution, i, r.2);
                }
            });
            if theme::primary_button(ui, tr("Render Image")).clicked() {
                let (w, h, _) = RESOLUTIONS[self.render_settings.resolution];
                self.render_settings.capture = Some((w, h));
            }
        });
    }

    /// Save a captured image as PNG next to the design.
    pub(crate) fn save_render(&mut self, w: u32, h: u32, rgba: Vec<u8>) {
        let name = self.tabs[self.active].doc.name.replace(['/', '\\'], "_");
        let path = self.default_dir().join(format!("{name}-render-{w}x{h}.png"));
        match image::RgbaImage::from_raw(w, h, rgba).map(|img| img.save(&path)) {
            Some(Ok(())) => self.toast(ToastKind::Info, format!("Rendered image saved to {}", path.display())),
            Some(Err(e)) => self.toast(ToastKind::Error, format!("Could not save image: {e}")),
            None => self.toast(ToastKind::Error, tr("Could not read back the rendered image.")),
        }
    }
}
