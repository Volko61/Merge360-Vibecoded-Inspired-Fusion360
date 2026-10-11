//! Local file handling: in-app file browser, recent files, Data Panel.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};

use crate::app::{Action, Merge360App};
use crate::icons::{self, Icon};
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMode {
    Open,
    Save,
    ExportStl,
    ExportStep,
    ImportStl,
    ImportDxf,
}

impl FileMode {
    fn title(self) -> &'static str {
        match self {
            FileMode::Open => "Open",
            FileMode::Save => "Save As",
            FileMode::ExportStl => "Export STL",
            FileMode::ExportStep => "Export STEP",
            FileMode::ImportStl => "Insert Mesh",
            FileMode::ImportDxf => "Insert DXF",
        }
    }
    fn ext(self) -> &'static str {
        match self {
            FileMode::Open | FileMode::Save => "m360",
            FileMode::ExportStl | FileMode::ImportStl => "stl",
            FileMode::ExportStep => "step",
            FileMode::ImportDxf => "dxf",
        }
    }
}

pub struct FileDialog {
    pub mode: FileMode,
    pub dir: PathBuf,
    pub dir_text: String,
    pub name: String,
    pub selected: Option<PathBuf>,
    pub overwrite: Option<PathBuf>,
    pub focus_name: u8,
}

impl FileDialog {
    pub fn new(mode: FileMode, dir: PathBuf, name: &str) -> Self {
        let dir_text = dir.display().to_string();
        Self { mode, dir, dir_text, name: name.to_string(), selected: None, overwrite: None, focus_name: 3 }
    }
}

fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("merge360"))
}

pub fn load_recent() -> Vec<PathBuf> {
    let Some(f) = config_dir().map(|d| d.join("recent.txt")) else { return Vec::new() };
    std::fs::read_to_string(f)
        .map(|s| s.lines().filter(|l| !l.is_empty()).map(PathBuf::from).filter(|p| p.exists()).collect())
        .unwrap_or_default()
}

pub fn save_recent(list: &[PathBuf]) {
    if let Some(d) = config_dir() {
        let _ = std::fs::create_dir_all(&d);
        let text: Vec<String> = list.iter().map(|p| p.display().to_string()).collect();
        let _ = std::fs::write(d.join("recent.txt"), text.join("\n"));
    }
}

fn list_dir(dir: &Path, ext: &str) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            let hidden = p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'));
            if hidden {
                continue;
            }
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)) {
                files.push(p);
            }
        }
    }
    dirs.sort();
    files.sort();
    (dirs, files)
}

impl Merge360App {
    /// Insert ▸ Insert Mesh: add an STL file as a polyhedral body.
    pub(crate) fn import_stl(&mut self, path: &Path) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => return self.toast(crate::app::ToastKind::Error, format!("Could not read {}: {e}", path.display())),
        };
        match m360_kernel::io::read_stl(&bytes) {
            Ok(tris) if tris.len() > 60_000 => self.toast(
                crate::app::ToastKind::Warning,
                format!("The mesh has {} triangles; meshes up to 60 000 triangles are supported.", tris.len()),
            ),
            Ok(triangles) => {
                let name = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                let n = triangles.len();
                self.tab().edit("Insert Mesh", |d| {
                    d.add_feature(m360_core::FeatureKind::ImportMesh { name, triangles });
                });
                let err = self.tabs[self.active].model.errors.values().next().cloned();
                match err {
                    Some(e) => self.toast(crate::app::ToastKind::Warning, format!("Mesh inserted with errors: {e}")),
                    None => self.toast(crate::app::ToastKind::Info, format!("Inserted mesh with {n} triangles")),
                }
                self.tab().fit_pending = true;
            }
            Err(e) => self.toast(crate::app::ToastKind::Error, format!("Not a valid STL file: {e}")),
        }
    }

    /// Insert ▸ Insert DXF: 2D geometry into a new sketch on the selected
    /// plane (the ground plane by default).
    pub(crate) fn import_dxf(&mut self, path: &Path) {
        let text = match std::fs::read(path) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) => return self.toast(crate::app::ToastKind::Error, format!("Could not read {}: {e}", path.display())),
        };
        match m360_core::dxf::read_dxf(&text) {
            Ok(entities) => {
                let n = entities.len();
                let plane = self.dxf_plane.take().unwrap_or(m360_core::PlaneRef::XZ);
                let tab = self.tab();
                tab.edit("Insert DXF", |d| {
                    d.add_feature(m360_core::FeatureKind::Sketch { plane, entities, constraints: Vec::new() });
                });
                tab.fit_pending = true;
                self.toast(crate::app::ToastKind::Info, format!("Inserted {n} sketch curves"));
            }
            Err(e) => self.toast(crate::app::ToastKind::Error, format!("Not a usable DXF file: {e}")),
        }
    }

    pub(crate) fn file_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(dlg) = &mut self.file_dialog else { return };
        let mut open = true;
        let mut result: Option<PathBuf> = None;
        let mut close = false;
        egui::Window::new(dlg.mode.title())
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([560.0, 420.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("⬆").on_hover_text("Parent folder").clicked() {
                        if let Some(p) = dlg.dir.parent() {
                            dlg.dir = p.to_path_buf();
                            dlg.dir_text = dlg.dir.display().to_string();
                        }
                    }
                    if ui.button("⌂").on_hover_text("Home").clicked() {
                        if let Some(h) = std::env::var_os("HOME") {
                            dlg.dir = PathBuf::from(h);
                            dlg.dir_text = dlg.dir.display().to_string();
                        }
                    }
                    let r = ui.add(egui::TextEdit::singleline(&mut dlg.dir_text).desired_width(f32::INFINITY));
                    if r.lost_focus() {
                        let p = PathBuf::from(&dlg.dir_text);
                        if p.is_dir() {
                            dlg.dir = p;
                        } else {
                            dlg.dir_text = dlg.dir.display().to_string();
                        }
                    }
                });
                ui.separator();
                let (dirs, files) = list_dir(&dlg.dir, dlg.mode.ext());
                egui::ScrollArea::vertical().max_height(280.0).auto_shrink([false, false]).show(ui, |ui| {
                    for d in dirs {
                        let name = d.file_name().unwrap_or_default().to_string_lossy().into_owned();
                        let r = ui.horizontal(|ui| {
                            let (ir, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                            icons::paint(ui.painter(), ir, Icon::Folder, None);
                            ui.selectable_label(false, name)
                        });
                        if r.inner.double_clicked() || r.inner.clicked() {
                            dlg.dir = d.clone();
                            dlg.dir_text = d.display().to_string();
                        }
                    }
                    for f in files {
                        let name = f.file_name().unwrap_or_default().to_string_lossy().into_owned();
                        let sel = dlg.selected.as_ref() == Some(&f);
                        let r = ui.horizontal(|ui| {
                            let (ir, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                            icons::paint(ui.painter(), ir, Icon::Component, None);
                            ui.selectable_label(sel, &name)
                        });
                        if r.inner.clicked() {
                            dlg.selected = Some(f.clone());
                            dlg.name = f.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                        }
                        if r.inner.double_clicked() {
                            result = Some(f.clone());
                        }
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    let r = ui.add(egui::TextEdit::singleline(&mut dlg.name).desired_width(300.0));
                    // Focus the name field for the first frames (the window's
                    // sizing pass swallows a single request).
                    if dlg.focus_name > 0 {
                        dlg.focus_name -= 1;
                        r.request_focus();
                    }
                    ui.label(RichText::new(format!(".{}", dlg.mode.ext())).color(theme::TEXT_DIM));
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::secondary_button(ui, "Cancel").clicked() {
                            close = true;
                        }
                        let label = match dlg.mode {
                            FileMode::Open => "Open",
                            FileMode::ImportStl | FileMode::ImportDxf => "Insert",
                            _ => "Save",
                        };
                        if (theme::primary_button(ui, label).clicked() || enter) && !dlg.name.trim().is_empty() {
                            let mut p = dlg.dir.join(dlg.name.trim());
                            if p.extension().is_none_or(|e| !e.eq_ignore_ascii_case(dlg.mode.ext())) {
                                p = PathBuf::from(format!("{}.{}", p.display(), dlg.mode.ext()));
                            }
                            result = Some(p);
                        }
                    });
                });
                if let Some(p) = &dlg.overwrite {
                    ui.colored_label(theme::WARNING, format!("{} exists. Click Save again to overwrite.", p.display()));
                }
            });
        let mode = dlg.mode;
        if let Some(p) = result {
            match mode {
                FileMode::Open => {
                    if p.exists() {
                        self.file_dialog = None;
                        self.open_path(&p);
                    }
                }
                FileMode::ImportStl => {
                    if p.exists() {
                        self.file_dialog = None;
                        self.import_stl(&p);
                    }
                }
                FileMode::ImportDxf => {
                    if p.exists() {
                        self.file_dialog = None;
                        self.import_dxf(&p);
                    }
                }
                _ => {
                    if p.exists() && dlg.overwrite.as_ref() != Some(&p) {
                        dlg.overwrite = Some(p);
                    } else {
                        self.file_dialog = None;
                        match mode {
                            FileMode::Save => self.save_to(&p),
                            m => self.export_to(m, &p),
                        }
                    }
                }
            }
            return;
        }
        if close || !open {
            self.file_dialog = None;
        }
    }

    /// Fusion's Data Panel, backed by the local file system.
    pub(crate) fn data_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("data_panel")
            .resizable(true)
            .default_size(260.0)
            .frame(egui::Frame::new().fill(theme::PANEL_BG).inner_margin(8).stroke(egui::Stroke::new(1.0, theme::PANEL_BORDER)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("DATA PANEL").strong().size(12.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕").clicked() {
                            self.flags.data_panel = false;
                        }
                    });
                });
                ui.label(RichText::new("Local designs (no cloud)").color(theme::TEXT_DIM).small());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if theme::primary_button(ui, "New Design").clicked() {
                        self.actions.push(Action::NewDesign);
                    }
                    if theme::secondary_button(ui, "Open…").clicked() {
                        self.actions.push(Action::Open);
                    }
                });
                ui.separator();
                ui.label(RichText::new("RECENT").small().color(theme::TEXT_DIM));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.recent.is_empty() {
                        ui.label(RichText::new("Saved designs will appear here.").color(theme::TEXT_DIM));
                    }
                    for p in self.recent.clone() {
                        let name = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                        let folder = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
                        let r = ui.horizontal(|ui| {
                            let (ir, _) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
                            ui.painter().rect_filled(ir, 3.0, egui::Color32::WHITE);
                            icons::paint(ui.painter(), ir.shrink(6.0), Icon::Body, None);
                            ui.vertical(|ui| {
                                let r = ui.add(egui::Label::new(RichText::new(&name).strong()).sense(egui::Sense::click()));
                                ui.label(RichText::new(folder).small().color(theme::TEXT_DIM));
                                r
                            })
                            .inner
                        });
                        if r.inner.double_clicked() || r.inner.clicked() {
                            self.actions.push(Action::OpenPath(p.clone()));
                        }
                        ui.add_space(2.0);
                    }
                });
            });
    }
}
