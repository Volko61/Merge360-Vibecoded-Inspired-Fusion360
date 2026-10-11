//! Developer input-replay scripts (`M360_SCRIPT=file`) used for automated
//! end-to-end UI checks. One command per line:
//!
//! ```text
//! wait 20                 # idle frames
//! move c 0.5 0.5          # pointer to canvas fraction (`a x y` absolute, `w x y z` world)
//! click c 0.5 0.5         # left click (also: rclick, dclick)
//! drag c 0.4 0.5 c 0.6 0.5
//! mdrag c 0.4 0.5 c 0.6 0.5 shift   # middle drag (+ optional modifier)
//! scroll c 0.5 0.5 -120
//! key S [ctrl|shift]      # key press
//! click w x y z [shift]   # click (+ optional modifier)
//! text 25 mm              # text input
//! tool box                # run a toolbar tool by id
//! ok                      # press OK on the command dialog
//! param distance -30      # set a command parameter (distance, op, height, …)
//! shot /path/file.png     # screenshot
//! dump /path/doc.ron      # write the document (RON)
//! workspace drawing       # switch workspace (design, render, drawing)
//! quit
//! ```

use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};

#[derive(Debug, Clone)]
pub enum P {
    Canvas(f32, f32),
    Abs(f32, f32),
    /// A world-space point (projected with the current camera).
    World(f32, f32, f32),
}

#[derive(Debug, Clone)]
enum Step {
    Wait(u32),
    Move(P),
    Click(P, PointerButton, bool, Modifiers),
    Drag(P, P, PointerButton, Modifiers),
    Scroll(P, f32),
    Key(Key, Modifiers),
    Text(String),
    Tool(String),
    Ok,
    Param(String, String),
    Shot(String),
    Dump(String),
    Workspace(String),
    Quit,
}

pub struct Script {
    steps: Vec<Step>,
    idx: usize,
    /// Sub-step phase for multi-frame steps.
    phase: u32,
    wait: u32,
    pub ok: bool,
    pub tool: Option<String>,
    pub shot: Option<String>,
    pub quit: bool,
    pub params: Vec<(String, String)>,
    pub dump: Option<String>,
    pub workspace: Option<String>,
}

fn parse_p(t: &[&str]) -> Option<(P, usize)> {
    let x: f32 = t.get(1)?.parse().ok()?;
    let y: f32 = t.get(2)?.parse().ok()?;
    if t[0] == "w" {
        let z: f32 = t.get(3)?.parse().ok()?;
        return Some((P::World(x, y, z), 4));
    }
    match t[0] {
        "c" => Some((P::Canvas(x, y), 3)),
        "a" => Some((P::Abs(x, y), 3)),
        _ => None,
    }
}

fn mods(s: Option<&&str>) -> Modifiers {
    match s.copied() {
        Some("ctrl") => Modifiers::COMMAND,
        Some("shift") => Modifiers::SHIFT,
        Some("alt") => Modifiers::ALT,
        _ => Modifiers::NONE,
    }
}

impl Script {
    pub fn load(path: &str) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let mut steps = Vec::new();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let t: Vec<&str> = line.split_whitespace().collect();
            let rest = &t[1..];
            let step = match t[0] {
                "wait" => Step::Wait(rest.first().and_then(|s| s.parse().ok()).unwrap_or(10)),
                "move" => Step::Move(parse_p(rest)?.0),
                "click" | "rclick" | "dclick" => {
                    let (p, n) = parse_p(rest)?;
                    let button = if t[0] == "rclick" { PointerButton::Secondary } else { PointerButton::Primary };
                    Step::Click(p, button, t[0] == "dclick", mods(rest.get(n)))
                }
                "drag" | "mdrag" => {
                    let (a, n) = parse_p(rest)?;
                    let (b, m) = parse_p(&rest[n..])?;
                    let button = if t[0] == "drag" { PointerButton::Primary } else { PointerButton::Middle };
                    Step::Drag(a, b, button, mods(rest.get(n + m)))
                }
                "scroll" => {
                    let (a, n) = parse_p(rest)?;
                    Step::Scroll(a, rest.get(n)?.parse().ok()?)
                }
                "key" => Step::Key(Key::from_name(rest.first()?)?, mods(rest.get(1))),
                "text" => Step::Text(line[5..].to_string()),
                "tool" => Step::Tool(rest.first()?.to_string()),
                "ok" => Step::Ok,
                "param" => Step::Param(rest.first()?.to_string(), rest.get(1)?.to_string()),
                "shot" => Step::Shot(rest.first()?.to_string()),
                "dump" => Step::Dump(rest.first()?.to_string()),
                "workspace" => Step::Workspace(rest.first()?.to_string()),
                "quit" => Step::Quit,
                other => {
                    eprintln!("script: unknown command {other}");
                    continue;
                }
            };
            steps.push(step);
        }
        Some(Self { steps, idx: 0, phase: 0, wait: 30, ok: false, tool: None, shot: None, quit: false, params: Vec::new(), dump: None, workspace: None })
    }

    pub fn running(&self) -> bool {
        self.idx < self.steps.len()
    }

    /// Inject this frame's events. `canvas` maps canvas-fraction positions.
    pub fn feed(&mut self, raw: &mut egui::RawInput, canvas: egui::Rect, world: &dyn Fn(f32, f32, f32) -> Pos2) {
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        let Some(step) = self.steps.get(self.idx).cloned() else { return };
        let pos = |p: &P| match p {
            P::Canvas(x, y) => canvas.min + egui::vec2(canvas.width() * x, canvas.height() * y),
            P::Abs(x, y) => Pos2::new(*x, *y),
            P::World(x, y, z) => world(*x, *y, *z),
        };
        let ev = &mut raw.events;
        let mut next = true;
        match &step {
            Step::Wait(n) => self.wait = *n,
            Step::Move(p) => ev.push(egui::Event::PointerMoved(pos(p))),
            Step::Click(p, button, double, m) => {
                let at = pos(p);
                let clicks = if *double { 2 } else { 1 };
                match self.phase {
                    0 => {
                        ev.push(egui::Event::PointerMoved(at));
                        ev.push(egui::Event::ModifiersChanged(*m));
                        next = false;
                    }
                    k if k <= 2 * clicks => {
                        let pressed = k % 2 == 1;
                        ev.push(egui::Event::PointerButton { pos: at, button: *button, pressed, modifiers: *m });
                        ev.push(egui::Event::ModifiersChanged(*m));
                        next = false;
                    }
                    k if k == 2 * clicks + 1 => ev.push(egui::Event::ModifiersChanged(Modifiers::NONE)),
                    _ => {}
                }
            }
            Step::Drag(a, b, button, m) => {
                let (pa, pb) = (pos(a), pos(b));
                let n = 12;
                match self.phase {
                    0 => {
                        ev.push(egui::Event::PointerMoved(pa));
                        next = false;
                    }
                    1 => {
                        ev.push(egui::Event::PointerButton { pos: pa, button: *button, pressed: true, modifiers: *m });
                        ev.push(egui::Event::ModifiersChanged(*m));
                        next = false;
                    }
                    k if k < n + 2 => {
                        let t = (k - 1) as f32 / n as f32;
                        ev.push(egui::Event::PointerMoved(pa + (pb - pa) * t));
                        ev.push(egui::Event::ModifiersChanged(*m));
                        next = false;
                    }
                    _ => {
                        ev.push(egui::Event::PointerButton { pos: pb, button: *button, pressed: false, modifiers: *m });
                        ev.push(egui::Event::ModifiersChanged(Modifiers::NONE));
                    }
                }
            }
            Step::Scroll(p, amount) => {
                ev.push(egui::Event::PointerMoved(pos(p)));
                ev.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, *amount),
                    modifiers: Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                });
            }
            Step::Key(k, m) => {
                for pressed in [true, false] {
                    ev.push(egui::Event::Key { key: *k, physical_key: None, pressed, repeat: false, modifiers: *m });
                }
            }
            Step::Text(t) => ev.push(egui::Event::Text(t.clone())),
            Step::Tool(t) => self.tool = Some(t.clone()),
            Step::Ok => self.ok = true,
            Step::Param(k, v) => self.params.push((k.clone(), v.clone())),
            Step::Shot(p) => self.shot = Some(p.clone()),
            Step::Dump(p) => self.dump = Some(p.clone()),
            Step::Workspace(w) => self.workspace = Some(w.clone()),
            Step::Quit => self.quit = true,
        }
        if next {
            self.idx += 1;
            self.phase = 0;
            // Give the UI a few frames to settle after each step.
            if self.wait == 0 {
                self.wait = 5;
            }
        } else {
            self.phase += 1;
        }
    }
}
