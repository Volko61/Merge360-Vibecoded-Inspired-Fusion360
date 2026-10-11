//! Static description of the Fusion-style toolbar.

use crate::icons::Icon;

#[derive(Debug, Clone, Copy)]
pub struct Tool {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: Icon,
    pub shortcut: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub struct Panel {
    pub name: &'static str,
    /// Number of leading tools shown as big buttons on the ribbon.
    pub featured: usize,
    pub tools: &'static [Tool],
    pub accent: Option<egui_color::Accent>,
}

#[derive(Debug, Clone, Copy)]
pub struct Tab {
    pub name: &'static str,
    pub panels: &'static [Panel],
}

pub mod egui_color {
    /// Optional panel accent (e.g. the green FINISH SKETCH panel).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Accent {
        Finish,
    }
}

const fn t(id: &'static str, label: &'static str, icon: Icon) -> Tool {
    Tool { id, label, icon, shortcut: None }
}

const fn k(id: &'static str, label: &'static str, icon: Icon, shortcut: &'static str) -> Tool {
    Tool { id, label, icon, shortcut: Some(shortcut) }
}

const fn p(name: &'static str, featured: usize, tools: &'static [Tool]) -> Panel {
    Panel { name, featured, tools, accent: None }
}

use Icon::*;

// ------------------------------------------------------------------ DESIGN › SOLID

const SOLID_CREATE: &[Tool] = &[
    t("new_component", "New Component", NewComponent),
    t("create_sketch", "Create Sketch", Sketch),
    k("extrude", "Extrude", Extrude, "E"),
    t("revolve", "Revolve", Revolve),
    k("hole", "Hole", Hole, "H"),
    t("rect_pattern", "Rectangular Pattern", Pattern),
    t("sweep", "Sweep", Sweep),
    t("loft", "Loft", Loft),
    t("rib", "Rib", Generic),
    t("web", "Web", Generic),
    t("emboss", "Emboss", Generic),
    t("thread", "Thread", Coil),
    t("box", "Box", Box),
    t("cylinder", "Cylinder", Cylinder),
    t("sphere", "Sphere", Sphere),
    t("torus", "Torus", Torus),
    t("coil", "Coil", Coil),
    t("pipe", "Pipe", Pipe),
    t("circ_pattern", "Circular Pattern", Pattern),
    t("path_pattern", "Pattern on Path", Pattern),
    t("mirror", "Mirror", Mirror),
    t("thicken", "Thicken", Generic),
    t("boundary_fill", "Boundary Fill", Generic),
    t("base_feature", "Create Base Feature", Generic),
];

const SOLID_MODIFY: &[Tool] = &[
    k("press_pull", "Press Pull", PressPull, "Q"),
    k("fillet", "Fillet", Fillet, "F"),
    t("chamfer", "Chamfer", Chamfer),
    t("shell", "Shell", Shell),
    t("combine", "Combine", Combine),
    t("scale", "Scale", Generic),
    t("draft", "Draft", Generic),
    t("replace_face", "Replace Face", Generic),
    t("split_face", "Split Face", Generic),
    t("split_body", "Split Body", Generic),
    t("silhouette_split", "Silhouette Split", Generic),
    k("move", "Move/Copy", Move, "M"),
    t("align", "Align", Generic),
    k("delete", "Delete", Delete, "Del"),
    t("physical_material", "Physical Material", Appearance),
    k("appearance", "Appearance", Appearance, "A"),
    t("manage_materials", "Manage Materials", Generic),
    t("change_parameters", "Change Parameters", Parameters),
    t("compute_all", "Compute All", Generic),
];

const SOLID_ASSEMBLE: &[Tool] = &[
    t("new_component", "New Component", NewComponent),
    k("joint", "Joint", Joint, "J"),
    t("as_built_joint", "As-built Joint", Joint),
    t("joint_origin", "Joint Origin", Generic),
    t("rigid_group", "Rigid Group", Generic),
    t("drive_joints", "Drive Joints", Generic),
    t("motion_link", "Motion Link", Generic),
    t("contact_sets", "Enable Contact Sets", Generic),
    t("motion_study", "Motion Study", Generic),
];

const SOLID_CONSTRUCT: &[Tool] = &[
    t("offset_plane", "Offset Plane", Plane),
    t("plane_at_angle", "Plane at Angle", Plane),
    t("tangent_plane", "Tangent Plane", Plane),
    t("midplane", "Midplane", Plane),
    t("plane_two_edges", "Plane Through Two Edges", Plane),
    t("plane_three_points", "Plane Through Three Points", Plane),
    t("plane_tangent_point", "Plane Tangent to Face at Point", Plane),
    t("plane_along_path", "Plane Along Path", Plane),
    t("axis_cylinder", "Axis Through Cylinder/Cone/Torus", Axis),
    t("axis_perp_point", "Axis Perpendicular at Point", Axis),
    t("axis_two_planes", "Axis Through Two Planes", Axis),
    t("axis_two_points", "Axis Through Two Points", Axis),
    t("axis_edge", "Axis Through Edge", Axis),
    t("axis_perp_face", "Axis Perpendicular to Face at Point", Axis),
    t("point_vertex", "Point at Vertex", Point),
    t("point_two_edges", "Point Through Two Edges", Point),
    t("point_three_planes", "Point Through Three Planes", Point),
    t("point_circle_center", "Point at Center of Circle/Sphere/Torus", Point),
    t("point_edge_plane", "Point at Edge and Plane", Point),
    t("point_along_path", "Point Along Path", Point),
];

const SOLID_INSPECT: &[Tool] = &[
    k("measure", "Measure", Measure, "I"),
    t("interference", "Interference", Interference),
    t("section_analysis", "Section Analysis", Section),
    t("curvature_comb", "Curvature Comb Analysis", Generic),
    t("zebra", "Zebra Analysis", Generic),
    t("draft_analysis", "Draft Analysis", Generic),
    t("curvature_map", "Curvature Map Analysis", Generic),
    t("accessibility", "Accessibility Analysis", Generic),
    t("min_radius", "Minimum Radius Analysis", Generic),
    t("center_of_mass", "Center of Mass", CenterOfMass),
    t("component_colors", "Display Component Colors", Appearance),
];

const SOLID_INSERT: &[Tool] = &[
    t("insert_derive", "Insert Derive", Insert),
    t("decal", "Decal", Insert),
    t("canvas", "Canvas", Insert),
    t("insert_mesh", "Insert Mesh", Insert),
    t("insert_svg", "Insert SVG", Insert),
    t("insert_dxf", "Insert DXF", Insert),
    t("mcmaster", "Insert McMaster-Carr Component", Insert),
    t("manufacturer_part", "Insert a manufacturer part", Insert),
];

const SELECT_TOOLS: &[Tool] = &[
    t("select", "Select", Select),
    t("window_select", "Window Selection", Select),
    t("freeform_select", "Freeform Selection", Select),
    t("paint_select", "Paint Selection", Select),
    t("select_all", "Select All", Select),
    t("invert_selection", "Invert Selection", Select),
    t("clear_selection", "Clear Selection", Select),
];

pub const SOLID: Tab = Tab {
    name: "SOLID",
    panels: &[
        p("CREATE", 6, SOLID_CREATE),
        p("MODIFY", 4, SOLID_MODIFY),
        p("ASSEMBLE", 2, SOLID_ASSEMBLE),
        p("CONSTRUCT", 1, SOLID_CONSTRUCT),
        p("INSPECT", 1, SOLID_INSPECT),
        p("INSERT", 1, SOLID_INSERT),
        p("SELECT", 1, SELECT_TOOLS),
    ],
};

// ------------------------------------------------------------------ other DESIGN tabs

pub const SURFACE: Tab = Tab {
    name: "SURFACE",
    panels: &[
        p(
            "CREATE",
            5,
            &[
                t("create_sketch", "Create Sketch", Sketch),
                t("srf_extrude", "Extrude", Extrude),
                t("patch", "Patch", Generic),
                t("srf_revolve", "Revolve", Revolve),
                t("srf_offset", "Offset", Generic),
                t("srf_sweep", "Sweep", Sweep),
                t("srf_loft", "Loft", Loft),
                t("ruled", "Ruled", Generic),
                t("extend_srf", "Extend", Generic),
            ],
        ),
        p(
            "MODIFY",
            4,
            &[
                t("trim", "Trim", Generic),
                t("untrim", "Untrim", Generic),
                t("stitch", "Stitch", Generic),
                t("unstitch", "Unstitch", Generic),
                t("reverse_normal", "Reverse Normal", Generic),
                t("srf_fillet", "Fillet", Fillet),
                t("srf_move", "Move/Copy", Move),
                t("srf_delete", "Delete", Delete),
            ],
        ),
        p("ASSEMBLE", 1, SOLID_ASSEMBLE),
        p("CONSTRUCT", 1, SOLID_CONSTRUCT),
        p("INSPECT", 1, SOLID_INSPECT),
        p("INSERT", 1, SOLID_INSERT),
        p("SELECT", 1, SELECT_TOOLS),
    ],
};

pub const MESH: Tab = Tab {
    name: "MESH",
    panels: &[
        p("CREATE", 2, &[t("insert_mesh", "Insert Mesh", Insert), t("tessellate", "Tessellate", Generic)]),
        p(
            "PREPARE",
            3,
            &[
                t("mesh_repair", "Repair", Generic),
                t("generate_face_groups", "Generate Face Groups", Generic),
                t("mesh_section", "Mesh Section Sketch", Generic),
            ],
        ),
        p(
            "MODIFY",
            3,
            &[
                t("remesh", "Remesh", Generic),
                t("reduce", "Reduce", Generic),
                t("plane_cut", "Plane Cut", Generic),
                t("mesh_smooth", "Smooth", Generic),
                t("mesh_shell", "Shell", Shell),
                t("mesh_combine", "Combine", Combine),
            ],
        ),
        p("CONVERT", 1, &[t("convert_mesh", "Convert Mesh", Generic)]),
        p("EXPORT", 1, &[t("export_3dprint", "3D Print", Print3D)]),
        p("INSPECT", 1, SOLID_INSPECT),
        p("SELECT", 1, SELECT_TOOLS),
    ],
};

pub const SHEET_METAL: Tab = Tab {
    name: "SHEET METAL",
    panels: &[
        p(
            "CREATE",
            4,
            &[
                t("create_sketch", "Create Sketch", Sketch),
                t("flange", "Flange", Generic),
                t("contour_flange", "Contour Flange", Generic),
                t("bend", "Bend", Generic),
                t("sm_extrude", "Extrude", Extrude),
                t("sm_hole", "Hole", Hole),
            ],
        ),
        p(
            "MODIFY",
            3,
            &[
                t("unfold", "Unfold", Generic),
                t("refold", "Refold Faces", Generic),
                t("create_flat_pattern", "Create Flat Pattern", Generic),
                t("sm_rules", "Sheet Metal Rules", Parameters),
            ],
        ),
        p("ASSEMBLE", 1, SOLID_ASSEMBLE),
        p("CONSTRUCT", 1, SOLID_CONSTRUCT),
        p("INSPECT", 1, SOLID_INSPECT),
        p("INSERT", 1, SOLID_INSERT),
        p("SELECT", 1, SELECT_TOOLS),
    ],
};

pub const PLASTIC: Tab = Tab {
    name: "PLASTIC",
    panels: &[
        p(
            "CREATE",
            5,
            &[
                t("create_sketch", "Create Sketch", Sketch),
                t("boss", "Boss", Generic),
                t("rest", "Rest", Generic),
                t("snap_fit", "Snap Fit", Generic),
                t("lip_groove", "Lip / Groove", Generic),
                t("plastic_rib", "Rib", Generic),
                t("plastic_web", "Web", Generic),
            ],
        ),
        p(
            "MODIFY",
            3,
            &[
                t("rule_fillet", "Rule Fillet", Fillet),
                t("plastic_shell", "Shell", Shell),
                t("plastic_draft", "Draft", Generic),
                t("plastic_rules", "Plastic Rules", Parameters),
            ],
        ),
        p("ASSEMBLE", 1, SOLID_ASSEMBLE),
        p("CONSTRUCT", 1, SOLID_CONSTRUCT),
        p("INSPECT", 1, SOLID_INSPECT),
        p("SELECT", 1, SELECT_TOOLS),
    ],
};

pub const UTILITIES: Tab = Tab {
    name: "UTILITIES",
    panels: &[
        p(
            "MAKE",
            1,
            &[t("export_3dprint", "3D Print", Print3D), t("export_step", "Export STEP", Insert)],
        ),
        p(
            "ADD-INS",
            1,
            &[t("scripts", "Scripts and Add-Ins", Generic), t("app_store", "Fusion App Store", Generic)],
        ),
        p(
            "UTILITY",
            3,
            &[
                t("change_parameters", "Change Parameters", Parameters),
                t("compute_all", "Compute All", Generic),
                t("doc_settings", "Document Settings", Parameters),
                t("preferences", "Preferences", Generic),
            ],
        ),
        p("INSPECT", 1, SOLID_INSPECT),
        p("SELECT", 1, SELECT_TOOLS),
    ],
};

pub const MANAGE: Tab = Tab {
    name: "MANAGE",
    panels: &[
        p(
            "MANAGE",
            3,
            &[
                t("properties", "Properties", Parameters),
                t("release", "Release", Generic),
                t("lifecycle", "Lifecycle", Generic),
            ],
        ),
        p("SELECT", 1, SELECT_TOOLS),
    ],
};

pub const DESIGN_TABS: &[Tab] = &[SOLID, SURFACE, MESH, SHEET_METAL, PLASTIC, UTILITIES, MANAGE];

// ------------------------------------------------------------------ contextual SKETCH tab

pub const SKETCH_TAB: Tab = Tab {
    name: "SKETCH",
    panels: &[
        p(
            "CREATE",
            6,
            &[
                k("sk_line", "Line", Line, "L"),
                k("sk_rect", "2-Point Rectangle", Rectangle, "R"),
                t("sk_center_rect", "Center Rectangle", Rectangle),
                k("sk_circle", "Center Diameter Circle", Circle, "C"),
                t("sk_arc", "3-Point Arc", Arc),
                t("sk_polygon", "Inscribed Polygon", Polygon),
                t("sk_point", "Point", Point),
                t("sk_ellipse", "Ellipse", Circle),
                t("sk_slot", "Center to Center Slot", Generic),
                t("sk_spline", "Fit Point Spline", Spline),
                t("sk_text", "Text", Generic),
                t("sk_mirror", "Mirror", Mirror),
                t("sk_circ_pattern", "Circular Pattern", Pattern),
                t("sk_rect_pattern", "Rectangular Pattern", Pattern),
                t("sk_project", "Project", Generic),
                k("sk_dimension", "Sketch Dimension", Dimension, "D"),
            ],
        ),
        p(
            "MODIFY",
            3,
            &[
                t("sk_fillet", "Fillet", Fillet),
                k("sk_trim", "Trim", Trim, "T"),
                t("sk_extend", "Extend", Generic),
                t("sk_break", "Break", Generic),
                k("sk_offset", "Offset", Generic, "O"),
                t("sk_scale", "Sketch Scale", Generic),
                t("sk_move", "Move/Copy", Move),
            ],
        ),
        p(
            "CONSTRAINTS",
            6,
            &[
                t("con_horizontal", "Horizontal/Vertical", ConHorizontal),
                t("con_coincident", "Coincident", ConCoincident),
                t("con_tangent", "Tangent", ConTangent),
                t("con_equal", "Equal", ConEqual),
                t("con_parallel", "Parallel", ConParallel),
                t("con_perpendicular", "Perpendicular", ConPerpendicular),
                t("con_fix", "Fix/Unfix", Generic),
                t("con_midpoint", "Midpoint", Generic),
                t("con_concentric", "Concentric", Generic),
                t("con_collinear", "Collinear", Generic),
                t("con_symmetry", "Symmetry", Generic),
                t("con_curvature", "Curvature", Generic),
            ],
        ),
        p("INSPECT", 1, &[k("measure", "Measure", Measure, "I")]),
        p("INSERT", 1, SOLID_INSERT),
        p("SELECT", 1, SELECT_TOOLS),
        Panel {
            name: "FINISH SKETCH",
            featured: 1,
            tools: &[t("finish_sketch", "Finish Sketch", FinishSketch)],
            accent: Some(egui_color::Accent::Finish),
        },
    ],
};

pub const WS_DESIGN: usize = 0;
pub const WS_RENDER: usize = 2;
pub const WS_DRAWING: usize = 6;

pub const WORKSPACES: &[(&str, bool)] = &[
    ("DESIGN", true),
    ("GENERATIVE DESIGN", false),
    ("RENDER", true),
    ("ANIMATION", false),
    ("SIMULATION", false),
    ("MANUFACTURE", false),
    ("DRAWING", true),
];

/// Tools that are wired up in this build.
pub const IMPLEMENTED: &[&str] = &[
    "create_sketch",
    "extrude",
    "box",
    "cylinder",
    "sphere",
    "torus",
    "coil",
    "combine",
    "revolve",
    "hole",
    "rect_pattern",
    "circ_pattern",
    "mirror",
    "offset_plane",
    "joint",
    "as_built_joint",
    "new_component",
    "insert_mesh",
    "sweep",
    "pipe",
    "loft",
    "split_body",
    "shell",
    "scale",
    "section_analysis",
    "sk_arc",
    "sk_dimension",
    "sk_trim",
    "sk_offset",
    "sk_fillet",
    "sk_text",
    "con_horizontal",
    "con_coincident",
    "con_tangent",
    "con_equal",
    "con_parallel",
    "con_perpendicular",
    "con_fix",
    "con_concentric",
    "con_midpoint",
    "con_collinear",
    "con_symmetry",
    "sk_polygon",
    "sk_slot",
    "sk_mirror",
    "sk_circ_pattern",
    "sk_rect_pattern",
    "sk_move",
    "sk_scale",
    "fillet",
    "chamfer",
    "press_pull",
    "move",
    "delete",
    "appearance",
    "measure",
    "center_of_mass",
    "change_parameters",
    "compute_all",
    "export_3dprint",
    "export_step",
    "doc_settings",
    "select",
    "window_select",
    "select_all",
    "invert_selection",
    "clear_selection",
    "sk_line",
    "sk_rect",
    "sk_center_rect",
    "sk_circle",
    "sk_point",
    "finish_sketch",
    "properties",
];

pub fn is_implemented(id: &str) -> bool {
    IMPLEMENTED.contains(&id)
}

/// Every unique tool, for the S-key toolbox search.
pub fn all_tools(sketch: bool) -> Vec<Tool> {
    let mut out: Vec<Tool> = Vec::new();
    let tabs: Vec<&Tab> = if sketch {
        std::iter::once(&SKETCH_TAB).chain(DESIGN_TABS.iter()).collect()
    } else {
        DESIGN_TABS.iter().collect()
    };
    for tab in tabs {
        for panel in tab.panels {
            for tool in panel.tools {
                // Unique by id, and by label (keep the implemented variant).
                if let Some(i) = out.iter().position(|t| t.label == tool.label) {
                    if !is_implemented(out[i].id) && is_implemented(tool.id) {
                        out[i] = *tool;
                    }
                    continue;
                }
                if !out.iter().any(|t| t.id == tool.id) {
                    out.push(*tool);
                }
            }
        }
    }
    out
}

pub fn find(id: &str) -> Option<Tool> {
    std::iter::once(&SKETCH_TAB)
        .chain(DESIGN_TABS.iter())
        .flat_map(|t| t.panels.iter())
        .flat_map(|p| p.tools.iter())
        .find(|t| t.id == id)
        .copied()
}
