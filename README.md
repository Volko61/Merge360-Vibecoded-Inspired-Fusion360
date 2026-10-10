# Merge360

A parametric 3D CAD application written in Rust, modeled on the **Autodesk Fusion 360**
user interface and workflow (offline only, no cloud features).

![Merge360](docs/screenshot.png)

![layout](docs/layout.txt)

## Running

```bash
cargo run --release -p merge360            # empty design
cargo run --release -p merge360 -- part.m360   # open a design
```

`Help ▸ Open Sample Design` loads a demo part.

Linux needs a Vulkan or GL driver (works under WSLg with Mesa).

## The interface

| Area | What it does |
|------|--------------|
| **Application bar** | Data Panel, File menu (New/Open/Recent/Save/Save As/Export STL·STEP), Save, Undo/Redo, document tabs |
| **Toolbar** | Workspace switcher (DESIGN), tabs SOLID · SURFACE · MESH · SHEET METAL · PLASTIC · UTILITIES · MANAGE, panels with dropdown menus listing every Fusion tool |
| **SKETCH contextual tab** | Line, 2‑point / center rectangle, circle, point, constraints palette, FINISH SKETCH |
| **Browser** | Document Settings (units), Named Views, Origin (planes/axes), Bodies, Sketches, visibility eyes, rename, context menus |
| **Canvas** | Shaded + edges rendering (wgpu, MSAA), adaptive grid, origin planes, hover/selection highlight, window selection (left→right contained, right→left crossing) |
| **ViewCube** | Click faces/edges/corners to snap views, drag to orbit, Home, perspective/orthographic |
| **Navigation bar** | Orbit, Look At, Pan, Zoom, Fit, Display Settings (visual style, environment, camera), Grid & Snaps |
| **Timeline** | Feature history with rollback marker (drag it), playback buttons, double‑click to edit, suppress, rename, delete, error highlighting |
| **Marking menu** | Right‑click radial menu + overflow list |
| **Toolbox** | Press **S** and type to search any tool |

### Working features

* **Create:** Sketch (on origin planes or planar faces), Extrude (new body / join / cut / intersect,
  one side or symmetric, multiple profiles, profiles with holes), Box, Cylinder, Sphere, Torus —
  all with live preview and drag manipulators.
* **Modify:** Combine (join / cut / intersect, keep tools), Move/Copy, Delete, Appearance, Change Parameters, Compute All.
* **Inspect:** Measure (face area, body volume, minimum distance), Center of Mass, Properties.
* **Files:** native `.m360` (RON, full parametric history), STL and STEP export.
* Fully parametric: every feature is re‑editable from the timeline; undo/redo for everything.

Tools that are not implemented yet (fillet, chamfer, shell, revolve, joints, the other
workspaces, …) are present in the UI and report “not available yet”.

### Mouse & keys (Fusion defaults)

| Input | Action |
|-------|--------|
| Middle drag / Shift + middle drag / wheel | Pan / Orbit / Zoom at cursor |
| Left click / double click | Select face or profile / select body |
| Right click | Marking menu |
| S | Toolbox |
| E, M, I, A | Extrude, Move, Measure, Appearance |
| L, R, C, P (in sketch) | Line, Rectangle, Circle, Point |
| Enter / Esc / Delete | Repeat last command / cancel / delete |
| F6 | Fit |
| Ctrl+Z / Ctrl+Y / Ctrl+S / Ctrl+O / Ctrl+N | Undo / Redo / Save / Open / New |

## Architecture

```
crates/
  m360-kernel   B-rep geometry on the pure-Rust `truck` kernel: primitives, profile
                extrusion, booleans, tessellation (with smooth/seam edge filtering), STL/STEP
  m360-core     Document model: features, timeline + marker, incremental regeneration
                cache, undo/redo, units, sketch profile detection, .m360 persistence
  m360-render   wgpu offscreen renderer: shaded meshes, screen-space thick edges, grid,
                overlays, orbit camera
  m360-ui       egui application (Fusion-style layout, commands, sketching, picking)
app/            `merge360` binary
```

## Development

```bash
cargo test            # kernel, document, and sample-design tests
cargo clippy --all-targets
```

Dev hooks (environment variables) used for automated visual checks:

* `M360_SAMPLE=1` — open the sample design
* `M360_SCREENSHOT=out.png` — save a screenshot after 40 frames and exit
* `M360_CAMERA="yaw,pitch,dist,tx,ty,tz"` — set the initial camera
* `M360_SCRIPT=file` — replay scripted input (see `crates/m360-ui/src/script.rs`)
