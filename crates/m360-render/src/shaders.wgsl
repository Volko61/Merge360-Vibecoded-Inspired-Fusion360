// Merge360 viewport shaders. All colors are linear; the target is sRGB.

struct Globals {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,        // xyz eye, w = 1 if orthographic
    view_dir: vec4<f32>,   // camera forward
    viewport: vec4<f32>,   // width, height (physical px), pixels_per_point, unused
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    light0: vec4<f32>,     // key light direction (towards light)
    light1: vec4<f32>,     // fill light direction
    clip: vec4<f32>,       // section plane: keep dot(n, p) <= d (xyz = n, w = d)
    clip_on: vec4<f32>,    // x = 1 when a section plane is active
};

struct Draw {
    color: vec4<f32>,
    edge_color: vec4<f32>,
    highlight: vec4<f32>,  // rgb, a = blend strength for the whole body
    hover_face: i32,
    n_sel: i32,
    flags: u32,
    _pad: u32,
    sel: array<vec4<i32>, 2>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> d: Draw;

// ---------------------------------------------------------------- background

struct BgOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) t: f32,
};

@vertex
fn vs_bg(@builtin(vertex_index) vi: u32) -> BgOut {
    var p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    var o: BgOut;
    o.clip = vec4(p[vi], 0.0, 1.0);
    o.t = p[vi].y * 0.5 + 0.5;
    return o;
}

@fragment
fn fs_bg(i: BgOut) -> @location(0) vec4<f32> {
    return vec4(mix(g.bg_bottom.rgb, g.bg_top.rgb, clamp(i.t, 0.0, 1.0)), 1.0);
}

// ---------------------------------------------------------------- shaded mesh

struct MeshIn {
    @location(0) pos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) face: u32,
};

struct MeshOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) wpos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) @interpolate(flat) face: u32,
};

@vertex
fn vs_mesh(v: MeshIn) -> MeshOut {
    var o: MeshOut;
    o.clip = g.view_proj * vec4(v.pos, 1.0);
    o.wpos = v.pos;
    o.nrm = v.nrm;
    o.face = v.face;
    return o;
}

fn is_selected(face: i32) -> bool {
    for (var k = 0; k < d.n_sel; k = k + 1) {
        if (d.sel[k / 4][k % 4] == face) {
            return true;
        }
    }
    return false;
}

fn clipped(p: vec3<f32>) -> bool {
    return g.clip_on.x > 0.5 && dot(g.clip.xyz, p) > g.clip.w;
}

@fragment
fn fs_mesh(i: MeshOut) -> @location(0) vec4<f32> {
    if (clipped(i.wpos)) {
        discard;
    }
    var n = normalize(i.nrm);
    var v = normalize(g.eye.xyz - i.wpos);
    if (g.eye.w > 0.5) {
        v = -g.view_dir.xyz;
    }
    if (dot(n, v) < 0.0) {
        if (g.clip_on.x > 0.5) {
            // Inside of a sectioned body: draw it as a flat section cap.
            return vec4(0.93, 0.47, 0.13, 1.0);
        }
        n = -n;
    }

    var base = d.color.rgb;
    var hl = d.highlight;
    let face = i32(i.face);
    if (is_selected(face)) {
        hl = vec4(0.03, 0.35, 0.85, 0.55);
    } else if (face == d.hover_face) {
        hl = vec4(0.25, 0.62, 1.0, 0.28);
    }
    base = mix(base, hl.rgb, hl.a);

    let sky = 0.62;
    let ground = 0.38;
    let amb = mix(ground, sky, n.y * 0.5 + 0.5);
    let l0 = normalize(g.light0.xyz);
    let l1 = normalize(g.light1.xyz);
    let diff = max(dot(n, l0), 0.0) * 0.55 + max(dot(n, l1), 0.0) * 0.2 + max(dot(n, v), 0.0) * 0.15;
    let h = normalize(l0 + v);
    let spec = pow(max(dot(n, h), 0.0), 60.0) * 0.22;
    let rim = pow(1.0 - max(dot(n, v), 0.0), 3.0) * 0.06;
    let c = base * (amb + diff) + vec3(spec + rim);
    return vec4(c, d.color.a);
}

// ---------------------------------------------------------------- thick lines

struct LineIn {
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) width: f32,
};

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) wpos: vec3<f32>,
};

fn clip_near(p: vec4<f32>, q: vec4<f32>) -> vec4<f32> {
    // Move p towards q until it is in front of the near plane (z >= 0).
    let eps = 1e-5;
    if (p.z >= eps) {
        return p;
    }
    let t = (eps - p.z) / (q.z - p.z);
    return mix(p, q, t);
}

@vertex
fn vs_line(@builtin(vertex_index) vi: u32, l: LineIn) -> LineOut {
    var o: LineOut;
    // Pull lines slightly towards the viewer (scale-independent) so edges win
    // over the faces they bound. Overlay draws (flag 1) skip this.
    var pa = l.a;
    var pb = l.b;
    if ((d.flags & 1u) == 0u) {
        pa = pa + (g.eye.xyz - pa) * 0.001;
        pb = pb + (g.eye.xyz - pb) * 0.001;
    }
    var ca = g.view_proj * vec4(pa, 1.0);
    var cb = g.view_proj * vec4(pb, 1.0);
    if (ca.z < 0.0 && cb.z < 0.0) {
        o.clip = vec4(2.0, 2.0, 2.0, 1.0);
        o.color = vec4(0.0);
        o.wpos = l.a;
        return o;
    }
    let ca2 = clip_near(ca, cb);
    let cb2 = clip_near(cb, ca);
    ca = ca2;
    cb = cb2;

    let half_vp = g.viewport.xy * 0.5;
    let sa = ca.xy / ca.w * half_vp;
    let sb = cb.xy / cb.w * half_vp;
    var dir = sb - sa;
    if (length(dir) < 1e-6) {
        dir = vec2(1.0, 0.0);
    } else {
        dir = normalize(dir);
    }
    let nrm = vec2(-dir.y, dir.x);

    var ts = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    var ss = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let t = ts[vi];
    let s = ss[vi];
    var base = ca;
    if (t > 0.5) {
        base = cb;
    }
    let w = max(l.width * g.viewport.z, 1.5) * 0.5;
    let off = nrm * s * w + dir * (t * 2.0 - 1.0) * w * 0.5;
    var clip = base;
    clip = vec4(clip.xy + off / half_vp * clip.w, clip.z, clip.w);
    clip.z = max(clip.z, 0.0);
    o.clip = clip;
    o.color = l.color * d.edge_color;
    o.wpos = select(l.a, l.b, t > 0.5);
    return o;
}

@fragment
fn fs_line(i: LineOut) -> @location(0) vec4<f32> {
    if (i.color.a <= 0.001 || clipped(i.wpos)) {
        discard;
    }
    return i.color;
}

// ---------------------------------------------------------------- flat colored triangles

struct FlatIn {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct FlatOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_flat(v: FlatIn) -> FlatOut {
    var o: FlatOut;
    o.clip = g.view_proj * vec4(v.pos, 1.0);
    o.color = v.color;
    return o;
}

@fragment
fn fs_flat(i: FlatOut) -> @location(0) vec4<f32> {
    return i.color;
}
