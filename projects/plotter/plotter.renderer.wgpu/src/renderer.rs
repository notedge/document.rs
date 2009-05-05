//! WGPU 离屏渲染器。

use std::num::NonZeroU64;

use plotter_renderer::{Capability, FrameReport, PreparedScene, RenderTarget, Renderer, RendererCapabilities, RgbaImage};
use plotter_types::plot::{Diagnostic, DiagnosticCode, Result};
use plotter_view::Scene;
use wgpu::util::DeviceExt;

use crate::{
    geometry::{LineVertex, MeshVertex, collect_geometry},
    shader::LINE_SHADER,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    viewport: [f32; 2],
    _pad: [f32; 2],
}

struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    line_pipeline: wgpu::RenderPipeline,
    mesh_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

/// WGPU 渲染器（离屏 → RGBA8）。
pub struct WgpuRenderer {
    ctx: Option<GpuContext>,
}

impl std::fmt::Debug for WgpuRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WgpuRenderer").field("ready", &self.ctx.is_some()).finish()
    }
}

impl Default for WgpuRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl WgpuRenderer {
    /// 构造（延迟创建设备）。
    pub const fn new() -> Self {
        Self { ctx: None }
    }

    /// 探测本机是否有可用 adapter。
    pub fn is_available() -> bool {
        pollster::block_on(async {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
            instance.request_adapter(&wgpu::RequestAdapterOptions::default()).await.is_some()
        })
    }

    fn ensure_ctx(&mut self) -> Result<&GpuContext> {
        if self.ctx.is_none() {
            self.ctx = Some(pollster::block_on(create_context())?);
        }
        Ok(self.ctx.as_ref().expect("ctx just initialized"))
    }
}

impl Renderer for WgpuRenderer {
    fn capabilities(&self) -> RendererCapabilities {
        let gpu = if self.ctx.is_some() || Self::is_available() { Capability::Accelerated } else { Capability::Unsupported };
        RendererCapabilities { raster_2d: Capability::Available, svg: Capability::Unsupported, gpu }
    }

    fn prepare(&mut self, scene: &Scene) -> Result<PreparedScene> {
        self.ensure_ctx()?;
        Ok(PreparedScene::from_scene(scene))
    }

    fn render(&mut self, prepared: &PreparedScene, target: &mut RenderTarget) -> Result<FrameReport> {
        let RenderTarget::Rgba8(image) = target
        else {
            return Err(Diagnostic::error(DiagnosticCode::UnsupportedTarget, "WgpuRenderer 需要 Rgba8 目标"));
        };

        let (lines, mesh) = collect_geometry(&prepared.scene)?;
        let width = prepared.scene.viewport.width.max(1.0).round() as u32;
        let height = prepared.scene.viewport.height.max(1.0).round() as u32;
        *image = render_offscreen(self.ensure_ctx()?, width, height, &lines, &mesh)?;
        Ok(FrameReport { primitive_count: (lines.len() / 2 + mesh.len() / 3) as u32 })
    }
}

/// 便捷：Scene → RGBA8（WGPU 离屏）。无 GPU 时返回诊断错误。
pub fn render_rgba8_wgpu(scene: &Scene) -> Result<RgbaImage> {
    let mut renderer = WgpuRenderer::new();
    let prepared = renderer.prepare(scene)?;
    let mut target = RenderTarget::Rgba8(RgbaImage::from_viewport(scene.viewport));
    renderer.render(&prepared, &mut target)?;
    match target {
        RenderTarget::Rgba8(image) => Ok(image),
        RenderTarget::Svg(_) => unreachable!("wgpu renderer only writes rgba8"),
    }
}

async fn create_context() -> Result<GpuContext> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
        .ok_or_else(|| Diagnostic::error(DiagnosticCode::RenderFailed, "无可用 GPU adapter"))?;

    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: Some("apollo-wgpu-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        )
        .await
        .map_err(|err| Diagnostic::error(DiagnosticCode::RenderFailed, format!("创建设备失败：{err}")))?;

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("apollo-line-shader"),
        source: wgpu::ShaderSource::Wgsl(LINE_SHADER.into()),
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("apollo-uniform-bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: NonZeroU64::new(std::mem::size_of::<Uniforms>() as u64),
            },
            count: None,
        }],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("apollo-line-pipeline-layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    let vertex_buffers = [wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<LineVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
    }];

    let make_pipeline = |topology: wgpu::PrimitiveTopology, label: &str| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &vertex_buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { topology, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    };

    let line_pipeline = make_pipeline(wgpu::PrimitiveTopology::LineList, "apollo-line-pipeline");
    let mesh_pipeline = make_pipeline(wgpu::PrimitiveTopology::TriangleList, "apollo-mesh-pipeline");

    Ok(GpuContext { device, queue, line_pipeline, mesh_pipeline, bind_group_layout })
}

fn render_offscreen(ctx: &GpuContext, width: u32, height: u32, lines: &[LineVertex], mesh: &[MeshVertex]) -> Result<RgbaImage> {
    let texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("apollo-offscreen"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let uniforms = Uniforms { viewport: [width as f32, height as f32], _pad: [0.0, 0.0] };
    let uniform_buffer = ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("apollo-uniforms"),
        contents: bytemuck::bytes_of(&uniforms),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("apollo-uniform-bg"),
        layout: &ctx.bind_group_layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform_buffer.as_entire_binding() }],
    });

    let line_buffer = if lines.is_empty() {
        None
    }
    else {
        Some(ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("apollo-line-vertices"),
            contents: bytemuck::cast_slice(lines),
            usage: wgpu::BufferUsages::VERTEX,
        }))
    };
    let mesh_buffer = if mesh.is_empty() {
        None
    }
    else {
        Some(ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("apollo-mesh-vertices"),
            contents: bytemuck::cast_slice(mesh),
            usage: wgpu::BufferUsages::VERTEX,
        }))
    };

    let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("apollo-encode") });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("apollo-line-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_bind_group(0, &bind_group, &[]);
        if let Some(buffer) = &mesh_buffer {
            pass.set_pipeline(&ctx.mesh_pipeline);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..mesh.len() as u32, 0..1);
        }
        if let Some(buffer) = &line_buffer {
            pass.set_pipeline(&ctx.line_pipeline);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..lines.len() as u32, 0..1);
        }
    }

    let bytes_per_pixel = 4_u32;
    let unpadded_bytes_per_row = width * bytes_per_pixel;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
    let buffer_size = u64::from(padded_bytes_per_row) * u64::from(height);
    let output_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("apollo-readback"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &output_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );

    ctx.queue.submit(Some(encoder.finish()));
    let buffer_slice = output_buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    ctx.device.poll(wgpu::Maintain::wait());
    receiver
        .recv()
        .map_err(|_| Diagnostic::error(DiagnosticCode::RenderFailed, "readback channel 关闭"))?
        .map_err(|err| Diagnostic::error(DiagnosticCode::RenderFailed, format!("map_async 失败：{err}")))?;

    let data = buffer_slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height as usize {
        let start = row * padded_bytes_per_row as usize;
        let end = start + unpadded_bytes_per_row as usize;
        pixels.extend_from_slice(&data[start..end]);
    }
    drop(data);
    output_buffer.unmap();

    let mut flipped = vec![0_u8; pixels.len()];
    let row_bytes = (width * 4) as usize;
    for y in 0..height as usize {
        let src = y * row_bytes;
        let dst = (height as usize - 1 - y) * row_bytes;
        flipped[dst..dst + row_bytes].copy_from_slice(&pixels[src..src + row_bytes]);
    }

    Ok(RgbaImage { width, height, pixels: flipped })
}
