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
    // Prefer the native GPU APIs (Vulkan/Metal/DX12). Probing GL as well only
    // produces noisy EGL errors on systems where it is unavailable (e.g. WSL);
    // GL is used as a fallback if no native adapter works. `WGPU_BACKEND`
    // overrides this.
    let env_backends = eframe::wgpu::Backends::from_env();
    let first = env_backends.unwrap_or(eframe::wgpu::Backends::PRIMARY);
    match launch(open.clone(), first) {
        Err(e) if env_backends.is_none() => {
            log::warn!("native graphics backend failed ({e}); retrying with OpenGL");
            launch(open, eframe::wgpu::Backends::GL)
        }
        r => r,
    }
}

fn launch(open: Option<std::path::PathBuf>, backends: eframe::wgpu::Backends) -> eframe::Result {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("Merge360")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([900.0, 600.0]);
    if std::env::var_os("M360_FULLSCREEN").is_some() {
        viewport = viewport.with_fullscreen(true);
    }
    let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut wgpu_options.wgpu_setup {
        setup.instance_descriptor.backends = backends;
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        wgpu_options,
        ..Default::default()
    };
    eframe::run_native("Merge360", options, Box::new(move |cc| Ok(Box::new(Merge360App::new(cc, open)))))
}
