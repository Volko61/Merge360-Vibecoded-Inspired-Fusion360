//! User preferences (persisted in `~/.config/merge360/preferences.ron`):
//! language, keyboard shortcut presets (with per-action overrides), mouse
//! navigation presets, units and display defaults.

use std::sync::RwLock;

use eframe::egui::{self, Key, Modifiers};
use m360_core::Units;
use serde::{Deserialize, Serialize};

use crate::i18n::Language;

/// Keyboard shortcut scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum KeymapPreset {
    #[default]
    Fusion,
    Blender,
    SolidWorks,
}

impl KeymapPreset {
    pub const ALL: [KeymapPreset; 3] = [KeymapPreset::Fusion, KeymapPreset::Blender, KeymapPreset::SolidWorks];
    pub fn label(self) -> &'static str {
        match self {
            KeymapPreset::Fusion => "Fusion 360",
            KeymapPreset::Blender => "Blender",
            KeymapPreset::SolidWorks => "SolidWorks",
        }
    }
}

/// Mouse navigation scheme (which buttons orbit, pan and zoom).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NavPreset {
    #[default]
    Fusion,
    Blender,
    SolidWorks,
    Inventor,
    Tinkercad,
}

impl NavPreset {
    pub const ALL: [NavPreset; 5] = [NavPreset::Fusion, NavPreset::Blender, NavPreset::SolidWorks, NavPreset::Inventor, NavPreset::Tinkercad];
    pub fn label(self) -> &'static str {
        match self {
            NavPreset::Fusion => "Fusion 360",
            NavPreset::Blender => "Blender",
            NavPreset::SolidWorks => "SolidWorks",
            NavPreset::Inventor => "Inventor",
            NavPreset::Tinkercad => "Tinkercad",
        }
    }

    /// The camera gesture for a drag with `button` and modifiers.
    pub fn gesture(self, button: egui::PointerButton, m: Modifiers) -> Option<Gesture> {
        use egui::PointerButton::{Middle, Secondary};
        use Gesture::*;
        let (shift, ctrl) = (m.shift, m.command || m.ctrl);
        Some(match (self, button) {
            (NavPreset::Fusion | NavPreset::Inventor, Middle) if shift => Orbit,
            (NavPreset::Fusion | NavPreset::Inventor, Middle) => Pan,
            (NavPreset::Blender, Middle) if shift => Pan,
            (NavPreset::Blender, Middle) if ctrl => Zoom,
            (NavPreset::Blender, Middle) => Orbit,
            (NavPreset::SolidWorks, Middle) if ctrl => Pan,
            (NavPreset::SolidWorks, Middle) if shift => Zoom,
            (NavPreset::SolidWorks, Middle) => Orbit,
            (NavPreset::Tinkercad, Secondary) if shift => Pan,
            (NavPreset::Tinkercad, Secondary) => Orbit,
            (NavPreset::Tinkercad, Middle) => Pan,
            _ => return None,
        })
    }

    /// Human-readable description of the mapping.
    pub fn describe(self) -> [(&'static str, &'static str); 3] {
        match self {
            NavPreset::Fusion | NavPreset::Inventor => [("Orbit", "Shift + Middle drag"), ("Pan", "Middle drag"), ("Zoom", "Wheel")],
            NavPreset::Blender => [("Orbit", "Middle drag"), ("Pan", "Shift + Middle drag"), ("Zoom", "Wheel / Ctrl + Middle drag")],
            NavPreset::SolidWorks => [("Orbit", "Middle drag"), ("Pan", "Ctrl + Middle drag"), ("Zoom", "Wheel / Shift + Middle drag")],
            NavPreset::Tinkercad => [("Orbit", "Right drag"), ("Pan", "Middle drag / Shift + Right drag"), ("Zoom", "Wheel")],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    Orbit,
    Pan,
    Zoom,
}

/// Where a shortcut applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyContext {
    /// Everywhere.
    Global,
    /// Outside sketches.
    Model,
    /// While editing a sketch.
    Sketch,
}

impl KeyContext {
    pub fn label(self) -> &'static str {
        match self {
            KeyContext::Global => "Global",
            KeyContext::Model => "Model",
            KeyContext::Sketch => "Sketch",
        }
    }
}

/// A key chord such as `Ctrl+Shift+S`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    pub const fn new(key: Key) -> Self {
        Self { key, ctrl: false, shift: false, alt: false }
    }
    pub const fn ctrl(key: Key) -> Self {
        Self { key, ctrl: true, shift: false, alt: false }
    }
    pub const fn shift(key: Key) -> Self {
        Self { key, ctrl: false, shift: true, alt: false }
    }
    pub const fn ctrl_shift(key: Key) -> Self {
        Self { key, ctrl: true, shift: true, alt: false }
    }
    pub const fn alt(key: Key) -> Self {
        Self { key, ctrl: false, shift: false, alt: true }
    }

    pub fn modifiers(&self) -> Modifiers {
        let mut m = Modifiers::NONE;
        if self.ctrl {
            m |= Modifiers::COMMAND;
        }
        if self.shift {
            m |= Modifiers::SHIFT;
        }
        if self.alt {
            m |= Modifiers::ALT;
        }
        m
    }

    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        s.push_str(self.key.symbol_or_name());
        s
    }

    pub fn parse(text: &str) -> Option<Chord> {
        let mut c = Chord::new(Key::A);
        let mut key = None;
        for part in text.split('+') {
            match part.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" => c.ctrl = true,
                "shift" => c.shift = true,
                "alt" => c.alt = true,
                _ => key = Key::from_name(part.trim()),
            }
        }
        c.key = key?;
        Some(c)
    }
}

/// One shortcut: a chord runs an action (a tool id, or an `@` command).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Binding {
    pub chord: Chord,
    pub action: &'static str,
    pub ctx: KeyContext,
}

/// User override of an action's shortcut.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyOverride {
    pub action: String,
    pub ctx: KeyContext,
    /// Chord text (`Ctrl+E`), empty to remove the shortcut.
    pub chord: String,
}

/// Commands that are not toolbar tools.
pub const COMMANDS: &[(&str, &str)] = &[
    ("@undo", "Undo"),
    ("@redo", "Redo"),
    ("@new", "New Design"),
    ("@open", "Open"),
    ("@save", "Save"),
    ("@save_as", "Save As"),
    ("@delete", "Delete selection"),
    ("@fit", "Fit to view"),
    ("@toolbox", "Toolbox (search tools)"),
    ("@repeat", "Repeat last command"),
    ("@select_all", "Select all"),
    ("@hide", "Hide selected bodies"),
    ("@unhide_all", "Show all bodies"),
    ("@toggle_sketch", "Create / finish sketch"),
    ("@view_home", "Home view"),
    ("@view_front", "Front view"),
    ("@view_back", "Back view"),
    ("@view_top", "Top view"),
    ("@view_bottom", "Bottom view"),
    ("@view_right", "Right view"),
    ("@view_left", "Left view"),
    ("@toggle_ortho", "Toggle orthographic / perspective"),
];

fn b(chord: Chord, action: &'static str, ctx: KeyContext) -> Binding {
    Binding { chord, action, ctx }
}

/// Default shortcuts of a preset.
pub fn preset_bindings(p: KeymapPreset) -> Vec<Binding> {
    use KeyContext::{Global as G, Model as M, Sketch as S};
    let mut v = vec![
        b(Chord::ctrl(Key::Z), "@undo", G),
        b(Chord::ctrl_shift(Key::Z), "@redo", G),
        b(Chord::ctrl_shift(Key::S), "@save_as", G),
        b(Chord::ctrl(Key::S), "@save", G),
        b(Chord::ctrl(Key::N), "@new", G),
        b(Chord::ctrl(Key::O), "@open", G),
        b(Chord::new(Key::Delete), "@delete", G),
    ];
    match p {
        KeymapPreset::Fusion => v.extend([
            b(Chord::ctrl(Key::Y), "@redo", G),
            b(Chord::new(Key::Backspace), "@delete", G),
            b(Chord::new(Key::F6), "@fit", G),
            b(Chord::new(Key::S), "@toolbox", G),
            b(Chord::new(Key::Enter), "@repeat", G),
            b(Chord::new(Key::E), "extrude", G),
            b(Chord::new(Key::Q), "press_pull", M),
            b(Chord::new(Key::F), "fillet", M),
            b(Chord::new(Key::M), "move", M),
            b(Chord::new(Key::J), "joint", M),
            b(Chord::new(Key::H), "hole", M),
            b(Chord::new(Key::I), "measure", G),
            b(Chord::new(Key::A), "appearance", M),
            b(Chord::new(Key::L), "sk_line", G),
            b(Chord::new(Key::R), "sk_rect", G),
            b(Chord::new(Key::C), "sk_circle", G),
            b(Chord::new(Key::P), "sk_point", S),
            b(Chord::new(Key::D), "sk_dimension", S),
            b(Chord::new(Key::T), "sk_trim", S),
            b(Chord::new(Key::O), "sk_offset", S),
        ]),
        KeymapPreset::Blender => v.extend([
            b(Chord::new(Key::X), "@delete", G),
            b(Chord::new(Key::Home), "@fit", G),
            b(Chord::new(Key::F3), "@toolbox", G),
            b(Chord::shift(Key::A), "@toolbox", G),
            b(Chord::shift(Key::R), "@repeat", G),
            b(Chord::new(Key::A), "@select_all", M),
            b(Chord::alt(Key::A), "clear_selection", G),
            b(Chord::new(Key::H), "@hide", M),
            b(Chord::alt(Key::H), "@unhide_all", M),
            b(Chord::new(Key::Tab), "@toggle_sketch", G),
            b(Chord::new(Key::Num1), "@view_front", G),
            b(Chord::ctrl(Key::Num1), "@view_back", G),
            b(Chord::new(Key::Num3), "@view_right", G),
            b(Chord::ctrl(Key::Num3), "@view_left", G),
            b(Chord::new(Key::Num7), "@view_top", G),
            b(Chord::ctrl(Key::Num7), "@view_bottom", G),
            b(Chord::new(Key::Num0), "@view_home", G),
            b(Chord::new(Key::Num5), "@toggle_ortho", G),
            b(Chord::new(Key::E), "extrude", G),
            b(Chord::new(Key::G), "move", M),
            b(Chord::new(Key::R), "move", M),
            b(Chord::shift(Key::D), "move", M),
            b(Chord::new(Key::S), "scale", M),
            b(Chord::ctrl(Key::B), "fillet", M),
            b(Chord::ctrl_shift(Key::B), "chamfer", M),
            b(Chord::ctrl(Key::R), "split_body", M),
            b(Chord::new(Key::I), "measure", M),
            b(Chord::new(Key::L), "sk_line", S),
            b(Chord::new(Key::R), "sk_rect", S),
            b(Chord::new(Key::C), "sk_circle", S),
            b(Chord::new(Key::D), "sk_dimension", S),
            b(Chord::new(Key::T), "sk_trim", S),
            b(Chord::new(Key::O), "sk_offset", S),
            b(Chord::new(Key::G), "sk_move", S),
            b(Chord::new(Key::S), "sk_scale", S),
            b(Chord::new(Key::M), "sk_mirror", S),
        ]),
        KeymapPreset::SolidWorks => v.extend([
            b(Chord::ctrl(Key::Y), "@redo", G),
            b(Chord::new(Key::F), "@fit", G),
            b(Chord::new(Key::S), "@toolbox", G),
            b(Chord::new(Key::Enter), "@repeat", G),
            b(Chord::ctrl(Key::Num1), "@view_front", G),
            b(Chord::ctrl(Key::Num2), "@view_back", G),
            b(Chord::ctrl(Key::Num3), "@view_left", G),
            b(Chord::ctrl(Key::Num4), "@view_right", G),
            b(Chord::ctrl(Key::Num5), "@view_top", G),
            b(Chord::ctrl(Key::Num6), "@view_bottom", G),
            b(Chord::ctrl(Key::Num7), "@view_home", G),
            b(Chord::ctrl(Key::A), "@select_all", M),
            b(Chord::new(Key::L), "sk_line", S),
            b(Chord::new(Key::C), "sk_circle", S),
            b(Chord::new(Key::R), "sk_rect", S),
            b(Chord::new(Key::D), "sk_dimension", S),
            b(Chord::new(Key::T), "sk_trim", S),
        ]),
    }
    v
}

/// Preferences, stored as RON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub language: Language,
    pub keymap: KeymapPreset,
    pub key_overrides: Vec<KeyOverride>,
    pub navigation: NavPreset,
    pub zoom_speed: f32,
    pub orbit_speed: f32,
    pub pan_speed: f32,
    pub invert_zoom: bool,
    pub zoom_to_cursor: bool,
    pub ui_scale: f32,
    pub default_units: Units,
    pub show_grid: bool,
    pub snap_to_grid: bool,
    pub show_view_cube: bool,
    pub show_fps: bool,
    /// Recovery copy of open designs every N minutes (0 = off).
    pub autosave_minutes: u32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: Language::English,
            keymap: KeymapPreset::Fusion,
            key_overrides: Vec::new(),
            navigation: NavPreset::Fusion,
            zoom_speed: 1.0,
            orbit_speed: 1.0,
            pan_speed: 1.0,
            invert_zoom: false,
            zoom_to_cursor: true,
            ui_scale: 1.0,
            default_units: Units::Millimeter,
            show_grid: true,
            snap_to_grid: true,
            show_view_cube: true,
            show_fps: false,
            autosave_minutes: 5,
        }
    }
}

fn path() -> Option<std::path::PathBuf> {
    crate::files::config_dir().map(|d| d.join("preferences.ron"))
}

impl Preferences {
    pub fn load() -> Self {
        let file = path();
        let text = file.as_ref().and_then(|p| std::fs::read_to_string(p).ok());
        let p: Preferences = match (&file, text) {
            (Some(f), Some(t)) => ron::from_str(&t).unwrap_or_else(|e| {
                // Keep the unreadable file: the next save would otherwise wipe every setting.
                let bak = f.with_extension("ron.bak");
                log::warn!("invalid preferences file {} ({e}); moved to {}", f.display(), bak.display());
                let _ = std::fs::rename(f, &bak);
                Preferences::default()
            }),
            _ => Preferences::default(),
        };
        p.apply_globals();
        p
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(text) = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()) {
            let _ = std::fs::write(p, text);
        }
    }

    /// Publish the settings read by free functions (translations, shortcut labels).
    pub fn apply_globals(&self) {
        crate::i18n::set_language(self.language);
        if let Ok(mut b) = ACTIVE_BINDINGS.write() {
            *b = self.bindings();
        }
    }

    /// Effective shortcuts: the preset with the user's overrides applied.
    pub fn bindings(&self) -> Vec<Binding> {
        let mut v = preset_bindings(self.keymap);
        for o in &self.key_overrides {
            let Some(action) = intern_action(&o.action) else { continue };
            v.retain(|b| !(b.action == action && b.ctx == o.ctx));
            if let Some(chord) = Chord::parse(&o.chord) {
                // A chord does one thing per context.
                v.retain(|b| !(b.chord == chord && (b.ctx == o.ctx || b.ctx == KeyContext::Global || o.ctx == KeyContext::Global)));
                v.push(Binding { chord, action, ctx: o.ctx });
            }
        }
        v
    }

    pub fn set_override(&mut self, action: &str, ctx: KeyContext, chord: Option<Chord>) {
        self.key_overrides.retain(|o| !(o.action == action && o.ctx == ctx));
        self.key_overrides.push(KeyOverride { action: action.to_string(), ctx, chord: chord.map(|c| c.text()).unwrap_or_default() });
    }
}

/// Static name of an action id (tool ids and `@` commands).
pub fn intern_action(id: &str) -> Option<&'static str> {
    if let Some((c, _)) = COMMANDS.iter().find(|(c, _)| *c == id) {
        return Some(c);
    }
    crate::tools::find(id).map(|t| t.id)
}

/// Display name of an action.
pub fn action_label(id: &str) -> &str {
    if let Some((_, l)) = COMMANDS.iter().find(|(c, _)| *c == id) {
        return l;
    }
    crate::tools::find(id).map_or(id, |t| t.label)
}

static ACTIVE_BINDINGS: RwLock<Vec<Binding>> = RwLock::new(Vec::new());

/// Shortcut text of a tool in the active keymap (for menus and tooltips).
pub fn shortcut_label(id: &str) -> Option<String> {
    let b = ACTIVE_BINDINGS.read().ok()?;
    let found: Vec<String> = b.iter().filter(|b| b.action == id).map(|b| b.chord.text()).collect();
    (!found.is_empty()).then(|| found.join(" / "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_round_trip() {
        for c in [Chord::ctrl_shift(Key::S), Chord::new(Key::F6), Chord::alt(Key::H), Chord::ctrl(Key::Num1)] {
            assert_eq!(Chord::parse(&c.text()), Some(c), "{}", c.text());
        }
    }

    #[test]
    fn presets_reference_known_actions() {
        for p in KeymapPreset::ALL {
            for b in preset_bindings(p) {
                assert!(intern_action(b.action).is_some(), "{p:?}: unknown action {}", b.action);
            }
        }
    }

    #[test]
    fn overrides_replace_preset_keys() {
        let mut p = Preferences::default();
        p.set_override("extrude", KeyContext::Global, Some(Chord::ctrl(Key::E)));
        let b = p.bindings();
        assert!(b.iter().any(|b| b.action == "extrude" && b.chord == Chord::ctrl(Key::E)));
        assert!(!b.iter().any(|b| b.action == "extrude" && b.chord == Chord::new(Key::E)));
        p.set_override("extrude", KeyContext::Global, None);
        assert!(!p.bindings().iter().any(|b| b.action == "extrude"));
    }
}
