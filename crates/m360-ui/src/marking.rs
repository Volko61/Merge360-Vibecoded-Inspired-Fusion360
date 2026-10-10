//! Right-click marking menu and the S-key toolbox (searchable tools).

use eframe::egui::{self, pos2, vec2, Color32, Pos2, RichText, Sense, Stroke};

use crate::app::{Action, Merge360App};
use crate::icons::{self, Icon};
use crate::{theme, tools};

pub struct MarkingMenu {
    pub center: Pos2,
    /// Frames since opening (the opening click must not close it).
    pub age: u32,
}

impl MarkingMenu {
    pub fn new(center: Pos2) -> Self {
        Self { center, age: 0 }
    }
}

pub struct Toolbox {
    pub pos: Pos2,
    pub query: String,
    pub index: usize,
    pub focused: bool,
}

impl Toolbox {
    pub fn new(pos: Pos2) -> Self {
        Self { pos, query: String::new(), index: 0, focused: false }
    }
}

enum Item {
    Tool(&'static str),
    Repeat,
    Undo,
    Redo,
    ToggleVisibility,
    FinishSketch,
}

impl Merge360App {
    fn item_label(&self, item: &Item) -> (String, Icon, bool) {
        match item {
            Item::Tool(id) => {
                let t = tools::find(id);
                (t.map_or(id.to_string(), |t| t.label.to_string()), t.map_or(Icon::Generic, |t| t.icon), tools::is_implemented(id))
            }
            Item::Repeat => match self.last_tool.and_then(tools::find) {
                Some(t) => (format!("Repeat {}", t.label), t.icon, true),
                None => ("Repeat".into(), Icon::Generic, false),
            },
            Item::Undo => ("Undo".into(), Icon::Undo, self.tabs[self.active].history.can_undo()),
            Item::Redo => ("Redo".into(), Icon::Redo, self.tabs[self.active].history.can_redo()),
            Item::ToggleVisibility => ("Show/Hide".into(), Icon::Eye, !self.tabs[self.active].selected_bodies().is_empty()),
            Item::FinishSketch => ("Finish Sketch".into(), Icon::FinishSketch, true),
        }
    }

    fn run_item(&mut self, item: &Item) {
        match item {
            Item::Tool(id) => self.actions.push(Action::Tool(id)),
            Item::Repeat => {
                if let Some(t) = self.last_tool {
                    self.actions.push(Action::Tool(t));
                }
            }
            Item::Undo => self.actions.push(Action::Undo),
            Item::Redo => self.actions.push(Action::Redo),
            Item::ToggleVisibility => {
                let bodies = self.tab().selected_bodies();
                self.tab().edit("Show/Hide", |d| {
                    for b in &bodies {
                        let mut m = d.body_meta(*b);
                        m.visible = !m.visible;
                        d.bodies.insert(*b, m);
                    }
                });
                self.tab().selection.clear();
            }
            Item::FinishSketch => self.actions.push(Action::FinishSketch),
        }
    }

    pub(crate) fn marking_menu(&mut self, ctx: &egui::Context) {
        let Some(menu) = &mut self.flags.marking else { return };
        menu.age += 1;
        let fresh = menu.age <= 1;
        let center = menu.center;
        let in_sketch = self.tabs[self.active].sketch.is_some();
        // N, NE, E, SE, S, SW, W, NW
        let ring: [Item; 8] = if in_sketch {
            [Item::Repeat, Item::Tool("delete"), Item::Tool("sk_line"), Item::Tool("sk_rect"), Item::FinishSketch, Item::Tool("sk_circle"), Item::Undo, Item::Tool("sk_point")]
        } else {
            [Item::Repeat, Item::Tool("delete"), Item::Tool("extrude"), Item::Tool("move"), Item::Tool("create_sketch"), Item::Undo, Item::Redo, Item::Tool("fillet")]
        };
        let overflow: Vec<Item> = vec![
            Item::Tool("measure"),
            Item::Tool("appearance"),
            Item::Tool("properties"),
            Item::ToggleVisibility,
            Item::Tool("select_all"),
            Item::Tool("clear_selection"),
            Item::Tool("box"),
            Item::Tool("combine"),
        ];
        let pointer = ctx.pointer_hover_pos();
        let mut chosen: Option<usize> = None;
        let mut close = false;
        let screen = ctx.content_rect();
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("marking_menu"));
        let painter = ctx.layer_painter(layer);

        // Wedge highlighting by angle.
        let hovered_wedge = pointer.and_then(|p| {
            let d = p - center;
            if d.length() < 22.0 || d.length() > 170.0 {
                return None;
            }
            let ang = (-d.y).atan2(d.x).to_degrees(); // 0 = E, 90 = N
            let idx_from_n = (((90.0 - ang).rem_euclid(360.0) + 22.5) / 45.0).floor() as usize % 8;
            Some(idx_from_n)
        });
        painter.circle(center, 18.0, Color32::from_rgba_unmultiplied(255, 255, 255, 200), Stroke::new(1.0, theme::PANEL_BORDER));
        if let Some(w) = hovered_wedge {
            let a = (90.0 - w as f32 * 45.0).to_radians();
            let dir = vec2(a.cos(), -a.sin());
            painter.line_segment([center + dir * 6.0, center + dir * 18.0], Stroke::new(3.0, theme::ACCENT));
        }

        let mut item_rects = Vec::new();
        for (i, item) in ring.iter().enumerate() {
            let (label, icon, enabled) = self.item_label(item);
            if label.is_empty() {
                continue;
            }
            let a = (90.0 - i as f32 * 45.0).to_radians();
            let dir = vec2(a.cos(), -a.sin());
            let galley = painter.layout_no_wrap(label.clone(), egui::FontId::proportional(12.0), theme::TEXT);
            let w = galley.size().x + 36.0;
            let anchor = center + dir * 95.0;
            let rect = egui::Rect::from_center_size(anchor + vec2(dir.x * (w / 2.0 - 20.0).max(0.0), 0.0), vec2(w, 26.0));
            let hot = hovered_wedge == Some(i);
            painter.rect(rect, 3.0, if hot { theme::SELECT_BG } else { Color32::WHITE }, Stroke::new(1.0, if hot { theme::ACCENT } else { theme::PANEL_BORDER }), egui::StrokeKind::Inside);
            icons::paint(&painter, egui::Rect::from_min_size(rect.min + vec2(5.0, 4.0), vec2(18.0, 18.0)), icon, (!enabled).then_some(Color32::from_gray(170)));
            painter.galley(rect.min + vec2(28.0, (26.0 - galley.size().y) / 2.0), galley, if enabled { theme::TEXT } else { theme::TEXT_DIM });
            item_rects.push((i, rect, enabled));
        }

        // Overflow menu below.
        let list_top = center + vec2(-90.0, 150.0);
        let list_rect = egui::Rect::from_min_size(list_top, vec2(180.0, overflow.len() as f32 * 24.0 + 6.0));
        let list_rect = list_rect.translate(vec2(0.0, (screen.bottom() - list_rect.bottom() - 8.0).min(0.0)));
        painter.rect(list_rect, 3.0, Color32::WHITE, Stroke::new(1.0, theme::PANEL_BORDER), egui::StrokeKind::Inside);
        let mut overflow_hit = None;
        for (k, item) in overflow.iter().enumerate() {
            let (label, icon, enabled) = self.item_label(item);
            let r = egui::Rect::from_min_size(list_rect.min + vec2(3.0, 3.0 + k as f32 * 24.0), vec2(174.0, 24.0));
            let hot = pointer.is_some_and(|p| r.contains(p));
            if hot {
                painter.rect_filled(r, 2.0, theme::HOVER_BG);
                overflow_hit = Some(k);
            }
            icons::paint(&painter, egui::Rect::from_min_size(r.min + vec2(4.0, 3.0), vec2(18.0, 18.0)), icon, (!enabled).then_some(Color32::from_gray(170)));
            painter.text(r.min + vec2(28.0, 12.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.0), if enabled { theme::TEXT } else { theme::TEXT_DIM });
        }

        let clicked = !fresh && ctx.input(|i| i.pointer.any_click());
        if clicked {
            if let Some(p) = pointer {
                if let Some((i, _, en)) = item_rects.iter().find(|(_, r, _)| r.contains(p)) {
                    if *en {
                        chosen = Some(*i);
                    }
                } else if let Some(k) = overflow_hit {
                    if self.item_label(&overflow[k]).2 {
                        self.run_item(&overflow[k]);
                    }
                } else if let Some(w) = hovered_wedge {
                    if item_rects.iter().any(|(i, _, en)| *i == w && *en) {
                        chosen = Some(w);
                    }
                }
            }
            close = true;
        }
        if let Some(i) = chosen {
            self.run_item(&ring[i]);
        }
        // Swallow clicks so the canvas does not also handle them.
        egui::Area::new(egui::Id::new("marking_blocker")).fixed_pos(screen.min).order(egui::Order::Foreground).show(ctx, |ui| {
            ui.allocate_exact_size(screen.size(), Sense::click());
        });
        if close {
            self.flags.marking = None;
        }
    }

    pub(crate) fn toolbox(&mut self, ctx: &egui::Context) {
        let Some(tb) = &mut self.flags.toolbox else { return };
        let in_sketch = self.tabs[self.active].sketch.is_some();
        let all = tools::all_tools(in_sketch);
        let q = tb.query.to_lowercase();
        let mut matches: Vec<tools::Tool> = all.into_iter().filter(|t| q.is_empty() || t.label.to_lowercase().contains(&q)).collect();
        matches.sort_by_key(|t| (!tools::is_implemented(t.id), !t.label.to_lowercase().starts_with(&q)));
        matches.truncate(12);
        tb.index = tb.index.min(matches.len().saturating_sub(1));
        let mut run: Option<&'static str> = None;
        let mut close = false;
        let area = egui::Area::new(egui::Id::new("toolbox")).fixed_pos(tb.pos - vec2(130.0, 20.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(260.0);
                ui.label(RichText::new("TOOLBOX").size(11.0).strong().color(theme::TEXT_DIM));
                let r = ui.add(egui::TextEdit::singleline(&mut tb.query).hint_text("Search tools…").desired_width(250.0));
                if !tb.focused {
                    r.request_focus();
                    tb.focused = true;
                }
                if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    tb.index = (tb.index + 1).min(matches.len().saturating_sub(1));
                }
                if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    tb.index = tb.index.saturating_sub(1);
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    run = matches.get(tb.index).map(|t| t.id);
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
                ui.add_space(4.0);
                for (k, t) in matches.iter().enumerate() {
                    let implemented = tools::is_implemented(t.id);
                    let resp = ui.horizontal(|ui| {
                        let (ir, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                        icons::paint(ui.painter(), ir, t.icon, None);
                        let text = RichText::new(t.label).color(if implemented { theme::TEXT } else { theme::TEXT_DIM });
                        ui.add(egui::Button::new(text).shortcut_text(t.shortcut.unwrap_or("")).selected(k == tb.index).min_size(vec2(226.0, 22.0)))
                    });
                    if resp.inner.clicked() {
                        run = Some(t.id);
                    }
                }
                if matches.is_empty() {
                    ui.label(RichText::new("No matching tools").color(theme::TEXT_DIM));
                }
            });
        });
        let area_rect = area.response.rect;
        let clicked_outside = ctx.input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !area_rect.contains(p)));
        if let Some(id) = run {
            self.flags.toolbox = None;
            self.actions.push(Action::Tool(id));
        } else if close || clicked_outside {
            self.flags.toolbox = None;
        }
        let _ = pos2;
    }
}
