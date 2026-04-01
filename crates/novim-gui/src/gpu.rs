//! GPU state management — wgpu surface, device, queue + glyphon text rendering.

use glyphon::{
    Attrs, Buffer as TextBuffer, Cache, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextRenderer, Viewport,
};
use std::sync::Arc;
use wgpu::{
    CommandEncoderDescriptor, CompositeAlphaMode, DeviceDescriptor, Instance, InstanceDescriptor,
    LoadOp, MultisampleState, Operations, PresentMode, RenderPassColorAttachment,
    RenderPassDescriptor, RequestAdapterOptions, SurfaceConfiguration, TextureFormat,
    TextureUsages, TextureViewDescriptor,
};
use winit::window::Window;

/// A colored rectangle to draw behind text (cell background).
#[derive(Clone, Copy)]
pub struct BgRect {
    /// Top-left X in physical pixels.
    pub x: f32,
    /// Top-left Y in physical pixels.
    pub y: f32,
    /// Width in physical pixels.
    pub w: f32,
    /// Height in physical pixels.
    pub h: f32,
    /// RGBA color, each component 0.0–1.0.
    pub color: [f32; 4],
}

/// Vertex for the background-quad pipeline.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct BgVertex {
    position: [f32; 2],
    color: [f32; 4],
}

/// WGSL shader for colored rectangles.
const BG_SHADER_SRC: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.color = input.color;
    return output;
}

// sRGB → linear conversion (inverse of the display gamma curve).
fn srgb_to_linear(c: f32) -> f32 {
    if (c <= 0.04045) {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(
        srgb_to_linear(input.color.r),
        srgb_to_linear(input.color.g),
        srgb_to_linear(input.color.b),
        input.color.a
    );
}
"#;

/// Holds all GPU + text rendering state.
pub struct GpuState {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    pub surface_config: SurfaceConfiguration,

    // Text rendering
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub viewport: Viewport,
    pub atlas: TextAtlas,
    pub text_renderer: TextRenderer,

    // Background quad rendering
    bg_pipeline: wgpu::RenderPipeline,

    // Layout
    pub physical_width: u32,
    pub physical_height: u32,
    pub scale_factor: f64,

    /// Monospace cell dimensions in physical pixels.
    pub cell_width: f32,
    pub cell_height: f32,

    /// Font metrics used for text buffers.
    pub font_size: f32,
    pub line_height: f32,
}

/// Background color — dark terminal style.
/// Values are in **linear** space (sRGB 0.08, 0.08, 0.10 converted).
const BG: wgpu::Color = wgpu::Color {
    r: 0.0072,
    g: 0.0072,
    b: 0.0100,
    a: 1.0,
};

impl GpuState {
    /// Create GPU state for the given window.
    pub async fn new(window: Arc<Window>) -> Self {
        let physical_size = window.inner_size();
        let scale_factor = window.scale_factor();

        let instance = Instance::new(&InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&RequestAdapterOptions::default())
            .await
            .expect("Failed to find a suitable GPU adapter");
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor::default())
            .await
            .expect("Failed to create GPU device");

        let surface = instance
            .create_surface(window.clone())
            .expect("Failed to create surface");

        let swapchain_format = TextureFormat::Bgra8UnormSrgb;

        // Prefer Mailbox (non-blocking, low-latency), fall back to Fifo.
        let caps = surface.get_capabilities(&adapter);
        let present_mode = if caps.present_modes.contains(&PresentMode::Mailbox) {
            PresentMode::Mailbox
        } else {
            PresentMode::Fifo
        };

        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: swapchain_format,
            width: physical_size.width,
            height: physical_size.height,
            present_mode,
            alpha_mode: CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        // Text rendering setup
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(&device);
        let viewport = Viewport::new(&device, &cache);
        let mut atlas = TextAtlas::new(&device, &queue, &cache, swapchain_format);
        let text_renderer =
            TextRenderer::new(&mut atlas, &device, MultisampleState::default(), None);

        // Measure monospace cell dimensions.
        let font_size = 14.0 * scale_factor as f32;
        let line_height = (font_size * 1.4).ceil();

        let (cell_width, cell_height) = measure_cell(&mut font_system, font_size, line_height);

        // Background quad pipeline — draws colored rectangles behind text.
        let bg_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bg_shader"),
            source: wgpu::ShaderSource::Wgsl(BG_SHADER_SRC.into()),
        });
        let bg_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bg_pipeline_layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let bg_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bg_pipeline"),
            layout: Some(&bg_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &bg_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<BgVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 8,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &bg_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: swapchain_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            device,
            queue,
            surface,
            surface_config,
            font_system,
            swash_cache,
            viewport,
            atlas,
            text_renderer,
            bg_pipeline,
            physical_width: physical_size.width,
            physical_height: physical_size.height,
            scale_factor,
            cell_width,
            cell_height,
            font_size,
            line_height,
        }
    }

    /// Handle resize.
    pub fn resize(&mut self, width: u32, height: u32, scale_factor: f64) {
        if width == 0 || height == 0 {
            return;
        }
        self.physical_width = width;
        self.physical_height = height;
        self.scale_factor = scale_factor;
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);

        // Recompute font metrics for new scale.
        self.font_size = 14.0 * scale_factor as f32;
        self.line_height = (self.font_size * 1.4).ceil();
        let (cw, ch) = measure_cell(&mut self.font_system, self.font_size, self.line_height);
        self.cell_width = cw;
        self.cell_height = ch;
    }

    /// Grid dimensions in character cells.
    pub fn grid_cols(&self) -> u16 {
        if self.cell_width < 0.001 { return 80; }
        (self.physical_width as f32 / self.cell_width).floor().max(1.0) as u16
    }

    pub fn grid_rows(&self) -> u16 {
        if self.cell_height < 0.001 { return 24; }
        (self.physical_height as f32 / self.cell_height).floor().max(1.0) as u16
    }

    /// Render background rectangles + text areas to the screen.
    pub fn render_frame(&mut self, bg_rects: &[BgRect], text_areas: &[TextArea<'_>]) {
        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.surface_config.width,
                height: self.surface_config.height,
            },
        );

        self.text_renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                text_areas.iter().cloned(),
                &mut self.swash_cache,
            )
            .expect("Failed to prepare text rendering");

        // Build background vertex buffer from rects.
        let w = self.physical_width as f32;
        let h = self.physical_height as f32;
        let mut bg_vertices: Vec<BgVertex> = Vec::with_capacity(bg_rects.len() * 6);
        for r in bg_rects {
            // Convert pixel coords to NDC (-1..1)
            let x0 = r.x / w * 2.0 - 1.0;
            let y0 = 1.0 - r.y / h * 2.0;
            let x1 = (r.x + r.w) / w * 2.0 - 1.0;
            let y1 = 1.0 - (r.y + r.h) / h * 2.0;
            let c = r.color;
            // Two triangles
            bg_vertices.push(BgVertex { position: [x0, y0], color: c });
            bg_vertices.push(BgVertex { position: [x1, y0], color: c });
            bg_vertices.push(BgVertex { position: [x0, y1], color: c });
            bg_vertices.push(BgVertex { position: [x0, y1], color: c });
            bg_vertices.push(BgVertex { position: [x1, y0], color: c });
            bg_vertices.push(BgVertex { position: [x1, y1], color: c });
        }

        let bg_vertex_buffer = if !bg_vertices.is_empty() {
            Some(wgpu::util::DeviceExt::create_buffer_init(
                &self.device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("bg_vertex_buffer"),
                    contents: bytemuck::cast_slice(&bg_vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                },
            ))
        } else {
            None
        };

        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Timeout | wgpu::SurfaceError::Outdated) => {
                return;
            }
            Err(wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.surface_config);
                return;
            }
            Err(e) => {
                log::error!("Surface error: {e:?}");
                return;
            }
        };

        let view = frame.texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor { label: None });

        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: Operations {
                        load: LoadOp::Clear(BG),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            // Draw background quads first.
            if let Some(ref buf) = bg_vertex_buffer {
                pass.set_pipeline(&self.bg_pipeline);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..bg_vertices.len() as u32, 0..1);
            }

            // Draw text on top.
            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .expect("Failed to render text");
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
    }

    /// Trim the glyph atlas to free unused entries. Call periodically, not every frame.
    pub fn trim_atlas(&mut self) {
        self.atlas.trim();
    }
}

/// Measure the width and height of a single monospace cell.
fn measure_cell(font_system: &mut FontSystem, font_size: f32, line_height: f32) -> (f32, f32) {
    let mut buf = TextBuffer::new(font_system, Metrics::new(font_size, line_height));
    buf.set_size(font_system, Some(font_size * 10.0), Some(line_height * 2.0));
    buf.set_text(
        font_system,
        "M",
        &Attrs::new().family(Family::Monospace),
        Shaping::Basic,
        None,
    );
    buf.shape_until_scroll(font_system, false);

    let width = buf
        .layout_runs()
        .flat_map(|run| run.glyphs.iter())
        .map(|g| g.w)
        .next()
        .unwrap_or(font_size * 0.6);

    (width, line_height)
}
