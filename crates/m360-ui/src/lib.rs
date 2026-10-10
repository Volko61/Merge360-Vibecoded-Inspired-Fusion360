//! Merge360 user interface (egui), modeled on the Fusion 360 layout.

mod app;
mod app_bar;
mod browser;
mod commands;
mod files;
mod icons;
mod marking;
mod overlays;
mod sample;
mod script;
mod sketch;
mod state;
mod theme;
mod timeline;
mod tools;
mod viewport;
mod widgets;

pub use app::Merge360App;

/// Launch the application window.
pub fn run(open: Option<std::path::PathBuf>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Merge360")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 600.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native("Merge360", options, Box::new(move |cc| Ok(Box::new(Merge360App::new(cc, open)))))
}
