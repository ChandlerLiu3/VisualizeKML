use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use std::error::Error;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;
use visualize_kml::camera::OrbitCamera;
use visualize_kml::geometry::ProjectedTracks;
use wgpu::util::DeviceExt;
use winit::window::Window;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TrajectoryVertex {
    position: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
}

#[derive(Debug)]
pub struct RendererError(String);

impl fmt::Display for RendererError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for RendererError {}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    render_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    segment_ranges: Vec<Range<u32>>,
    camera: OrbitCamera,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    depth_view: wgpu::TextureView,
    window: Arc<Window>,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, RendererError> {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .map_err(|error| RendererError(format!("could not create window surface: {error}")))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| RendererError("no compatible graphics adapter was found".to_owned()))?;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("VisualizeKML device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .map_err(|error| RendererError(format!("could not create graphics device: {error}")))?;

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or_else(|| RendererError("graphics surface exposes no color format".to_owned()))?;
        let present_mode = if capabilities
            .present_modes
            .contains(&wgpu::PresentMode::Fifo)
        {
            wgpu::PresentMode::Fifo
        } else {
            *capabilities.present_modes.first().ok_or_else(|| {
                RendererError("graphics surface exposes no present mode".to_owned())
            })?
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: capabilities
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let camera = OrbitCamera::new(Vec3::ZERO, 3.2);
        let initial_uniform = CameraUniform {
            view_proj: camera
                .view_projection(config.width as f32 / config.height as f32)
                .to_cols_array_2d(),
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera uniform buffer"),
            contents: bytemuck::bytes_of(&initial_uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera bind group layout"),
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
        });
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("trajectory shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("trajectory pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("trajectory render pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TrajectoryVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    }],
                }],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24Plus,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("trajectory vertex buffer"),
            contents: bytemuck::bytes_of(&TrajectoryVertex { position: [0.0; 3] }),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let depth_view = create_depth_view(&device, &config);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            render_pipeline,
            vertex_buffer,
            segment_ranges: Vec::new(),
            camera,
            uniform_buffer,
            uniform_bind_group,
            depth_view,
            window,
        })
    }

    pub fn set_tracks(&mut self, tracks: ProjectedTracks) -> Result<(), RendererError> {
        let mut vertices = Vec::new();
        let mut ranges = Vec::new();
        for segment in tracks.segments {
            if segment.is_empty() {
                continue;
            }
            let start = u32::try_from(vertices.len())
                .map_err(|_| RendererError("trajectory has too many vertices".to_owned()))?;
            vertices.extend(
                segment
                    .into_iter()
                    .map(|position| TrajectoryVertex { position }),
            );
            let end = u32::try_from(vertices.len())
                .map_err(|_| RendererError("trajectory has too many vertices".to_owned()))?;
            ranges.push(start..end);
        }

        if vertices.is_empty() {
            return Err(RendererError(
                "trajectory contains no drawable points".to_owned(),
            ));
        }
        self.vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("trajectory vertex buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        self.segment_ranges = ranges;
        self.camera.target = Vec3::ZERO;
        self.camera.distance = 3.2;
        self.window.request_redraw();
        Ok(())
    }

    pub fn orbit(&mut self, delta_x: f32, delta_y: f32) {
        self.camera.orbit(delta_x, delta_y);
    }

    pub fn zoom(&mut self, delta: f32) {
        self.camera.zoom(delta);
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, &self.config);
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let frame = self.surface.get_current_texture()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let camera_uniform = CameraUniform {
            view_proj: self
                .camera
                .view_projection(self.config.width as f32 / self.config.height as f32)
                .to_cols_array_2d(),
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&camera_uniform));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("trajectory render encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("trajectory render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.045,
                            g: 0.065,
                            b: 0.09,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.render_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            for range in &self.segment_ranges {
                if range.end - range.start >= 2 {
                    pass.draw(range.clone(), 0..1);
                }
            }
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}

fn create_depth_view(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("trajectory depth texture"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth24Plus,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}
