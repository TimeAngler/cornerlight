use std::sync::Arc;
use anyhow::Context;
use wgpu::util::DeviceExt;
use raw_window_handle::{HasWindowHandle, HasDisplayHandle};
use log;
use crate::texture::Texture;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct SpriteVertex {
    position: [f32; 3],
    uv: [f32; 2],
}

impl SpriteVertex {
    const ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array!
        [0 => Float32x3, 1 => Float32x2];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    projection: [[f32; 4]; 4],
}

pub struct WgpuRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,

    render_pipeline: wgpu::RenderPipeline,

    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    texture_bind_group: wgpu::BindGroup,
    texture_size: (u32, u32),
}

impl WgpuRenderer {
    pub fn device(&self) -> &wgpu::Device { &self.device }
    pub fn queue(&self) -> &wgpu::Queue { &self.queue }
    pub fn config(&self) -> &wgpu::SurfaceConfiguration { &self.config }
    // 核心：泛型 W，只要它提供 raw window handle 即可。
    // 传入 Arc<W> 的所有权，这样 wgpu 内部可以克隆 Arc，从而让 Surface 获得 'static 生命周期。
    pub async fn new<W>(
        window: Arc<W>,
        width: u32,
        height: u32,
    ) -> anyhow::Result<Self>
    where
        W: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static,
    {
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor {
                backends: wgpu::Backends::PRIMARY,
                flags: Default::default(),
                memory_budget_thresholds: Default::default(),
                backend_options: Default::default(),
                display: None,
            }
        );

        // 关键：因为传入的是 Arc<W>，create_surface 会自动处理 raw-window-handle
        // 并返回一个拥有 'static 生命周期的 Surface
        let surface = instance.create_surface(window)?;

        let adapter = instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            }
        ).await.context("找不到合适的显卡适配器")?;

        let (device, queue) =
            adapter.request_device(
            &wgpu::DeviceDescriptor::default(),
        ).await?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let alpha_mode = surface_caps
            .alpha_modes
            .iter()
            .copied()
            .find(|m| matches!(m, wgpu::CompositeAlphaMode::Opaque))
            .unwrap_or(surface_caps.alpha_modes[0]);

        let present_mode = [
            wgpu::PresentMode::Mailbox,
            wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Fifo,
        ]
            .into_iter()
            .find(|m| surface_caps.present_modes.contains(m))
            .unwrap_or(wgpu::PresentMode::Fifo);

        log::info!("alpha_mode: {:?}, available: {:?}", alpha_mode, surface_caps.alpha_modes);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode,
            alpha_mode,
            desired_maximum_frame_latency: 2,
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };

        surface.configure(&device, &config);

//-------- 四边形顶点（像素坐标，原点左上角，Z 用于层序）--------
        let vertex_buffer = device.create_buffer(
            &wgpu::BufferDescriptor {
                label: Some("Sprite Vertex Buffer"),
                size: (std::mem::size_of::<SpriteVertex>() * 4) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }
        );
        let index_buffer = device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("Sprite Index Buffer"),
                contents: bytemuck::cast_slice(&[0u16, 1, 2, 0, 2, 3]),
                usage: wgpu::BufferUsages::INDEX,
            }
        );

// -------- 相机正交投影：像素坐标 → NDC --------
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Camera Buffer"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

// -------- Camera BindGroupLayout and Texture BindGroupLayout --------
        let camera_bgl = device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label: Some("Camera BGL"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            }
        );

        let texture_bgl = device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label: Some("Texture BGL"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            }
        );

        let camera_bind_group = device.create_bind_group(
            &wgpu::BindGroupDescriptor {
                label: Some("Camera BG"),
                layout: &camera_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                }],
            }
        );

// -------- 加载 character图片 --------
        let texture_bytes = include_bytes!("../../../assets/character.png");
        let texture = Texture::from_bytes(
            &device,
            &queue,
            texture_bytes,
            "character"
        )?;

        let tex_w = texture.texture.width();
        let tex_h = texture.texture.height();

        let texture_bind_group = device.create_bind_group(
            &wgpu::BindGroupDescriptor {
                label: Some("Texture BG"),
                layout: &texture_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&texture.sampler),
                    },
                ],
            }
        );

//---------------------------------------------------------------------------

//---------------------------------------------------------------------------
        let shader = device.create_shader_module(
            wgpu::ShaderModuleDescriptor {
                label: Some("Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
            }
        );

        let render_pipeline_layout = device.create_pipeline_layout(
            &wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[
                    Some(&camera_bgl),
                    Some(&texture_bgl)
                ],
                immediate_size: 0,
            }
        );

        let render_pipeline = device.create_render_pipeline(
            &wgpu::RenderPipelineDescriptor {
                label: Some("Render Pipeline"),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Some(SpriteVertex::layout())],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview_mask: None,
                cache: None,
            }
        );

//---------------------------------------------------------------------------

        let renderer = Self {
            surface,
            device,
            queue,
            config,
            render_pipeline,
            vertex_buffer,
            index_buffer,
            index_count: 6,
            camera_buffer,
            camera_bind_group,
            texture_bind_group,
            texture_size: (tex_w, tex_h),
        };
        renderer.update_viewport(width, height);

        Ok(renderer)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.update_viewport(width, height);
        }
    }

    fn update_viewport(&self, width: u32, height: u32) {
        let win_w = width as f32;
        let win_h = height as f32;
        let (tex_w, tex_h) = (self.texture_size.0 as f32, self.texture_size.1 as f32);

        // contain：取较小缩放比，保证图片完整装进窗口
        let scale = (win_w / tex_w).min(win_h / tex_h);
        let draw_w = tex_w * scale;
        let draw_h = tex_h * scale;
        let x = (win_w - draw_w) * 0.5;
        let y = (win_h - draw_h) * 0.5;

        let vertices = [
            SpriteVertex { position: [x,         y,         0.0], uv: [0.0, 0.0] },
            SpriteVertex { position: [x + draw_w, y,         0.0], uv: [1.0, 0.0] },
            SpriteVertex { position: [x + draw_w, y + draw_h, 0.0], uv: [1.0, 1.0] },
            SpriteVertex { position: [x,         y + draw_h, 0.0], uv: [0.0, 1.0] },
        ];

        self.queue.write_buffer(
            &self.vertex_buffer,
            0,
            bytemuck::cast_slice(&vertices),
        );

        // 更新相机投影
        let projection = glam::camera::rh::proj::directx::orthographic(
            0.0, win_w, win_h, 0.0, -1.0, 1.0,
        );
        let uniform = CameraUniform { projection: projection.to_cols_array_2d() };
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::cast_slice(&[uniform]),
        );
    }

    pub fn render(&mut self) -> anyhow::Result<()> {
        self.render_with(|_, _| {})
    }

    /// 主渲染循环。场景 pass 完成后调用 `extra_pass`，允许调用方插入额外的渲染 pass
    /// （比如 egui）。`extra_pass` 在 `encoder.finish()` 之前、`present` 之前被调用。
    pub fn render_with<F>(&mut self, extra_pass: F) -> anyhow::Result<()>
    where
        F: FnOnce(&mut wgpu::CommandEncoder, &wgpu::TextureView),
    {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(surface_texture) => surface_texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(surface_texture) => surface_texture,
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                anyhow::bail!("失去device");
            }
        };

        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            }
        );

        // 场景 pass
        {
            let mut render_pass = encoder.begin_render_pass(
                &wgpu::RenderPassDescriptor {
                    label: Some("Render Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    occlusion_query_set: None,
                    timestamp_writes: None,
                    multiview_mask: None,
                }
            );

            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            render_pass.set_bind_group(1, &self.texture_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(0..self.index_count, 0, 0..1);
        }

        // 👇 关键：在 finish 和 present 之前，让调用方插入额外 pass（比如 egui）
        extra_pass(&mut encoder, &view);

        self.queue.submit(std::iter::once(encoder.finish()));
        self.queue.present(frame);

        Ok(())
    }

}