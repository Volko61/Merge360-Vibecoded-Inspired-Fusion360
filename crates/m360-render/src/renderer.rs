use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use eframe::egui;
use eframe::egui_wgpu::{self, wgpu, wgpu::util::DeviceExt};
use m360_kernel::BodyMesh;

use crate::scene::{FlatVertex, LineInst, Scene};

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SLOT: u64 = 256;
const BODY_EDGE_WIDTH: f32 = 1.4;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    view_dir: [f32; 4],
    viewport: [f32; 4],
    bg_top: [f32; 4],
    bg_bottom: [f32; 4],
    light0: [f32; 4],
    light1: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DrawUniform {
    color: [f32; 4],
    edge_color: [f32; 4],
    highlight: [f32; 4],
    hover_face: i32,
    n_sel: i32,
    flags: u32,
    _pad: u32,
    sel: [i32; 8],
}

impl DrawUniform {
    fn plain(flags: u32) -> Self {
        Self {
            color: [1.0; 4],
            edge_color: [1.0; 4],
            highlight: [0.0; 4],
            hover_face: -1,
            n_sel: 0,
            flags,
            _pad: 0,
            sel: [-1; 8],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MeshVertex {
    pos: [f32; 3],
    nrm: [f32; 3],
    face: u32,
}

struct GpuBody {
    _keep: Arc<BodyMesh>,
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    n_idx: u32,
    ebuf: Option<wgpu::Buffer>,
    n_edges: u32,
    used: bool,
}

struct Targets {
    size: [u32; 2],
    msaa: Option<wgpu::TextureView>,
    resolve: wgpu::Texture,
    resolve_view: wgpu::TextureView,
    depth: wgpu::TextureView,
}

struct Pipelines {
    bg: wgpu::RenderPipeline,
    mesh: wgpu::RenderPipeline,
    mesh_blend: wgpu::RenderPipeline,
    line: wgpu::RenderPipeline,
    line_overlay: wgpu::RenderPipeline,
    flat: wgpu::RenderPipeline,
    flat_overlay: wgpu::RenderPipeline,
}

pub struct Viewport3D {
    samples: u32,
    pipelines: Pipelines,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    draw_layout: wgpu::BindGroupLayout,
    draw_buf: wgpu::Buffer,
    draw_bg: wgpu::BindGroup,
    draw_capacity: u64,
    bodies: HashMap<usize, GpuBody>,
    targets: Option<Targets>,
    texture_id: Option<egui::TextureId>,
}

fn blend_state() -> Option<wgpu::BlendState> {
    Some(wgpu::BlendState::ALPHA_BLENDING)
}

impl Viewport3D {
    pub fn new(rs: &egui_wgpu::RenderState) -> Self {
        let device = &rs.device;
        let flags = rs.adapter.get_texture_format_features(COLOR_FORMAT).flags;
        let samples = if flags.sample_count_supported(4) { 4 } else { 1 };

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("m360 shaders"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders.wgsl").into()),
        });

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("draw"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<DrawUniform>() as u64),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("m360 layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&draw_layout)],
            immediate_size: 0,
        });

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() }],
        });

        let mesh_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Uint32];
        let mesh_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &mesh_attrs,
        };
        let line_attrs =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32];
        let line_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineInst>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &line_attrs,
        };
        let flat_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let flat_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<FlatVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &flat_attrs,
        };

        let make = |label: &str,
                    vs: &str,
                    fs: &str,
                    buffers: &[Option<wgpu::VertexBufferLayout>],
                    depth_write: bool,
                    compare: wgpu::CompareFunction,
                    blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(compare),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        use wgpu::CompareFunction::*;
        let pipelines = Pipelines {
            bg: make("bg", "vs_bg", "fs_bg", &[], false, Always, None),
            mesh: make("mesh", "vs_mesh", "fs_mesh", &[Some(mesh_layout.clone())], true, Less, None),
            mesh_blend: make("mesh_blend", "vs_mesh", "fs_mesh", &[Some(mesh_layout)], false, Less, blend_state()),
            line: make("line", "vs_line", "fs_line", &[Some(line_layout.clone())], false, LessEqual, blend_state()),
            line_overlay: make("line_ov", "vs_line", "fs_line", &[Some(line_layout)], false, Always, blend_state()),
            flat: make("flat", "vs_flat", "fs_flat", &[Some(flat_layout.clone())], false, LessEqual, blend_state()),
            flat_overlay: make("flat_ov", "vs_flat", "fs_flat", &[Some(flat_layout)], false, Always, blend_state()),
        };

        let draw_capacity = 64;
        let (draw_buf, draw_bg) = Self::make_draw_buffer(device, &draw_layout, draw_capacity);

        Self {
            samples,
            pipelines,
            globals_buf,
            globals_bg,
            draw_layout,
            draw_buf,
            draw_bg,
            draw_capacity,
            bodies: HashMap::new(),
            targets: None,
            texture_id: None,
        }
    }

    fn make_draw_buffer(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        slots: u64,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("draw uniforms"),
            size: slots * SLOT,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("draw"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<DrawUniform>() as u64),
                }),
            }],
        });
        (buf, bg)
    }

    fn ensure_targets(&mut self, rs: &egui_wgpu::RenderState, size: [u32; 2]) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let device = &rs.device;
        let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let tex = |label: &str, format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let resolve = tex(
            "viewport color",
            COLOR_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let resolve_view = resolve.create_view(&Default::default());
        let msaa = (self.samples > 1).then(|| {
            tex("viewport msaa", COLOR_FORMAT, self.samples, wgpu::TextureUsages::RENDER_ATTACHMENT)
                .create_view(&Default::default())
        });
        let depth = tex("viewport depth", DEPTH_FORMAT, self.samples, wgpu::TextureUsages::RENDER_ATTACHMENT)
            .create_view(&Default::default());

        let mut renderer = rs.renderer.write();
        match self.texture_id {
            Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                device,
                &resolve_view,
                wgpu::FilterMode::Linear,
                id,
            ),
            None => {
                self.texture_id =
                    Some(renderer.register_native_texture(device, &resolve_view, wgpu::FilterMode::Linear))
            }
        }
        self.targets = Some(Targets { size, msaa, resolve, resolve_view, depth });
    }

    fn upload_body(device: &wgpu::Device, mesh: &Arc<BodyMesh>) -> GpuBody {
        let verts: Vec<MeshVertex> = (0..mesh.positions.len())
            .map(|i| MeshVertex { pos: mesh.positions[i], nrm: mesh.normals[i], face: mesh.face_ids[i] })
            .collect();
        let edges: Vec<LineInst> = mesh
            .edges
            .iter()
            .flat_map(|pl| {
                pl.windows(2).map(|w| LineInst { a: w[0], b: w[1], color: [1.0; 4], width: BODY_EDGE_WIDTH })
            })
            .collect();
        let zero_vertex = [MeshVertex::zeroed()];
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("body vertices"),
            contents: bytemuck::cast_slice(if verts.is_empty() { &zero_vertex[..] } else { &verts }),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("body indices"),
            contents: bytemuck::cast_slice(if mesh.indices.is_empty() { &[0u32; 3][..] } else { &mesh.indices }),
            usage: wgpu::BufferUsages::INDEX,
        });
        let ebuf = (!edges.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("body edges"),
                contents: bytemuck::cast_slice(&edges),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
        GpuBody {
            _keep: mesh.clone(),
            vbuf,
            ibuf,
            n_idx: mesh.indices.len() as u32,
            ebuf,
            n_edges: edges.len() as u32,
            used: true,
        }
    }

    fn vertex_buffer<T: Pod>(device: &wgpu::Device, data: &[T]) -> Option<wgpu::Buffer> {
        (!data.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("dynamic"),
                contents: bytemuck::cast_slice(data),
                usage: wgpu::BufferUsages::VERTEX,
            })
        })
    }

    /// Render `scene` at `size` physical pixels and return the egui texture.
    pub fn render(
        &mut self,
        rs: &egui_wgpu::RenderState,
        size: [u32; 2],
        scene: &Scene,
    ) -> egui::TextureId {
        let size = [size[0].clamp(1, 8192), size[1].clamp(1, 8192)];
        self.ensure_targets(rs, size);
        let device = &rs.device;
        let queue = &rs.queue;

        // Globals
        let cam = &scene.camera;
        let aspect = size[0] as f32 / size[1] as f32;
        let fwd = cam.forward();
        let key = (-fwd + cam.up() * 0.8 - cam.right() * 0.5).normalize();
        let fill = (-fwd - cam.up() * 0.2 + cam.right() * 0.9).normalize();
        let globals = Globals {
            view_proj: cam.view_proj(aspect).to_cols_array_2d(),
            eye: cam.eye().extend(if cam.ortho { 1.0 } else { 0.0 }).to_array(),
            view_dir: fwd.extend(0.0).to_array(),
            viewport: [size[0] as f32, size[1] as f32, scene.pixels_per_point, 0.0],
            bg_top: scene.bg_top,
            bg_bottom: scene.bg_bottom,
            light0: key.extend(0.0).to_array(),
            light1: fill.extend(0.0).to_array(),
        };
        queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));

        // Per-draw uniforms: slot 0 = plain, slot 1 = overlay (no depth bias), then bodies.
        let mut slots = vec![DrawUniform::plain(0), DrawUniform::plain(1)];
        for b in &scene.bodies {
            let mut u = DrawUniform::plain(0);
            u.color = b.color;
            u.edge_color = b.edge_color;
            u.highlight = b.highlight;
            u.hover_face = b.hover_face.map_or(-1, |f| f as i32);
            for (k, f) in b.selected_faces.iter().take(8).enumerate() {
                u.sel[k] = *f as i32;
            }
            u.n_sel = b.selected_faces.len().min(8) as i32;
            slots.push(u);
        }
        if slots.len() as u64 > self.draw_capacity {
            self.draw_capacity = (slots.len() as u64).next_power_of_two();
            let (buf, bg) = Self::make_draw_buffer(device, &self.draw_layout, self.draw_capacity);
            self.draw_buf = buf;
            self.draw_bg = bg;
        }
        let mut bytes = vec![0u8; slots.len() * SLOT as usize];
        for (i, s) in slots.iter().enumerate() {
            let off = i * SLOT as usize;
            bytes[off..off + std::mem::size_of::<DrawUniform>()].copy_from_slice(bytemuck::bytes_of(s));
        }
        queue.write_buffer(&self.draw_buf, 0, &bytes);

        // Body geometry cache.
        for gb in self.bodies.values_mut() {
            gb.used = false;
        }
        for b in &scene.bodies {
            let key = Arc::as_ptr(&b.mesh) as usize;
            self.bodies
                .entry(key)
                .or_insert_with(|| Self::upload_body(device, &b.mesh))
                .used = true;
        }
        self.bodies.retain(|_, gb| gb.used);

        let lines = Self::vertex_buffer(device, &scene.lines);
        let overlay_lines = Self::vertex_buffer(device, &scene.overlay_lines);
        let tris = Self::vertex_buffer(device, &scene.tris);
        let overlay_tris = Self::vertex_buffer(device, &scene.overlay_tris);

        let targets = self.targets.as_ref().unwrap();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("viewport") });
        {
            let (view, resolve) = match &targets.msaa {
                Some(m) => (m, Some(&targets.resolve_view)),
                None => (&targets.resolve_view, None),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_bg, &[]);
            let slot = |i: usize| (i as u64 * SLOT) as u32;

            pass.set_pipeline(&self.pipelines.bg);
            pass.set_bind_group(1, &self.draw_bg, &[slot(0)]);
            pass.draw(0..3, 0..1);

            // Opaque faces.
            pass.set_pipeline(&self.pipelines.mesh);
            for (i, b) in scene.bodies.iter().enumerate() {
                if !b.show_faces || b.color[3] < 0.999 {
                    continue;
                }
                let gb = &self.bodies[&(Arc::as_ptr(&b.mesh) as usize)];
                pass.set_bind_group(1, &self.draw_bg, &[slot(i + 2)]);
                pass.set_vertex_buffer(0, gb.vbuf.slice(..));
                pass.set_index_buffer(gb.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..gb.n_idx, 0, 0..1);
            }

            // Edges and scene lines.
            pass.set_pipeline(&self.pipelines.line);
            for (i, b) in scene.bodies.iter().enumerate() {
                if !b.show_edges {
                    continue;
                }
                let gb = &self.bodies[&(Arc::as_ptr(&b.mesh) as usize)];
                if let Some(eb) = &gb.ebuf {
                    pass.set_bind_group(1, &self.draw_bg, &[slot(i + 2)]);
                    pass.set_vertex_buffer(0, eb.slice(..));
                    pass.draw(0..6, 0..gb.n_edges);
                }
            }
            if let Some(buf) = &lines {
                pass.set_bind_group(1, &self.draw_bg, &[slot(0)]);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..6, 0..scene.lines.len() as u32);
            }

            // Translucent bodies and triangles.
            pass.set_pipeline(&self.pipelines.mesh_blend);
            for (i, b) in scene.bodies.iter().enumerate() {
                if !b.show_faces || b.color[3] >= 0.999 {
                    continue;
                }
                let gb = &self.bodies[&(Arc::as_ptr(&b.mesh) as usize)];
                pass.set_bind_group(1, &self.draw_bg, &[slot(i + 2)]);
                pass.set_vertex_buffer(0, gb.vbuf.slice(..));
                pass.set_index_buffer(gb.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..gb.n_idx, 0, 0..1);
            }
            if let Some(buf) = &tris {
                pass.set_pipeline(&self.pipelines.flat);
                pass.set_bind_group(1, &self.draw_bg, &[slot(0)]);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..scene.tris.len() as u32, 0..1);
            }

            // Overlays.
            if let Some(buf) = &overlay_tris {
                pass.set_pipeline(&self.pipelines.flat_overlay);
                pass.set_bind_group(1, &self.draw_bg, &[slot(1)]);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..scene.overlay_tris.len() as u32, 0..1);
            }
            if let Some(buf) = &overlay_lines {
                pass.set_pipeline(&self.pipelines.line_overlay);
                pass.set_bind_group(1, &self.draw_bg, &[slot(1)]);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..6, 0..scene.overlay_lines.len() as u32);
            }
        }
        queue.submit(Some(encoder.finish()));
        let _ = &targets.resolve;
        self.texture_id.unwrap()
    }
}
