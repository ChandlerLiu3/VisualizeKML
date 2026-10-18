use bytemuck::{Pod, Zeroable};
use glam::{Vec3, Vec4};
use std::error::Error;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;
use visualize_kml::analysis::{AnalysisResult, AnalyzedMarker, MarkerKind, MarkerSource};
use visualize_kml::camera::OrbitCamera;
use visualize_kml::geometry::ProjectedTracks;
use visualize_kml::TrackMarker;
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

struct SceneMarker {
    position: [f32; 3],
    route_direction: Option<Vec3>,
    kind: MarkerKind,
    source: MarkerSource,
    name: String,
    kml_metadata: Option<TrackMarker>,
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
    egui_context: egui::Context,
    egui_renderer: egui_wgpu::Renderer,
    markers: Vec<SceneMarker>,
    warnings: Vec<String>,
    system_warnings: Vec<String>,
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

        let egui_context = egui::Context::default();
        let has_cjk_font = load_cjk_font(&egui_context);
        let egui_renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);
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
            egui_context,
            egui_renderer,
            markers: Vec::new(),
            system_warnings: if has_cjk_font {
                Vec::new()
            } else {
                vec![
                    "No system CJK font was found; Chinese marker labels may not display"
                        .to_owned(),
                ]
            },
            warnings: if has_cjk_font {
                Vec::new()
            } else {
                vec![
                    "No system CJK font was found; Chinese marker labels may not display"
                        .to_owned(),
                ]
            },
        })
    }

    pub fn set_tracks(
        &mut self,
        tracks: ProjectedTracks,
        analysis: AnalysisResult,
    ) -> Result<(), RendererError> {
        let scene_markers: Vec<SceneMarker> = analysis
            .markers
            .into_iter()
            .map(|marker| scene_marker(&tracks, marker))
            .collect();
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
        self.markers = scene_markers;
        self.warnings.clone_from(&self.system_warnings);
        self.warnings.extend(analysis.warnings);
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

        let pixels_per_point = self.window.scale_factor() as f32;
        let screen_rect = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(
                self.config.width as f32 / pixels_per_point,
                self.config.height as f32 / pixels_per_point,
            ),
        );
        let view_projection = self
            .camera
            .view_projection(self.config.width as f32 / self.config.height as f32);
        let markers = &self.markers;
        let warnings = &self.warnings;
        let width = self.config.width;
        let height = self.config.height;
        let full_output = self.egui_context.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |context| {
                paint_scene_markers(
                    context,
                    markers,
                    warnings,
                    view_projection,
                    width,
                    height,
                    pixels_per_point,
                );
            },
        );
        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point,
        };
        let paint_jobs = self
            .egui_context
            .tessellate(full_output.shapes, pixels_per_point);
        let egui_command_buffers = self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("trajectory marker overlay pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            self.egui_renderer
                .render(&mut pass.forget_lifetime(), &paint_jobs, &screen_descriptor);
        }
        self.queue
            .submit(std::iter::once(encoder.finish()).chain(egui_command_buffers));
        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
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

fn scene_marker(tracks: &ProjectedTracks, marker: AnalyzedMarker) -> SceneMarker {
    let position = tracks.project_point(&marker.point);
    let route_direction = tracks
        .segments
        .iter()
        .find(|segment| segment.len() > 1)
        .map(|segment| Vec3::from_array(segment[1]) - Vec3::from_array(segment[0]));
    SceneMarker {
        position,
        route_direction,
        kind: marker.kind,
        source: marker.source,
        name: marker.name,
        kml_metadata: marker.kml_metadata,
    }
}

fn paint_scene_markers(
    context: &egui::Context,
    markers: &[SceneMarker],
    warnings: &[String],
    view_projection: glam::Mat4,
    width: u32,
    height: u32,
    pixels_per_point: f32,
) {
    let painter = context.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("scene-markers"),
    ));
    let logical_width = width as f32 / pixels_per_point;
    let logical_height = height as f32 / pixels_per_point;
    for (index, marker) in markers.iter().enumerate() {
        let clip = view_projection
            * Vec4::new(
                marker.position[0],
                marker.position[1],
                marker.position[2],
                1.0,
            );
        if clip.w <= 0.0 {
            continue;
        }
        let ndc = clip.truncate() / clip.w;
        if !(-1.0..=1.0).contains(&ndc.x)
            || !(-1.0..=1.0).contains(&ndc.y)
            || !(0.0..=1.0).contains(&ndc.z)
        {
            continue;
        }
        let center = egui::pos2(
            (ndc.x + 1.0) * 0.5 * logical_width,
            (1.0 - ndc.y) * 0.5 * logical_height,
        );
        let route_direction = marker.route_direction.and_then(|direction| {
            let next_position = Vec3::from_array(marker.position) + direction;
            let next_clip = view_projection * next_position.extend(1.0);
            (next_clip.w > 0.0).then(|| {
                let next_ndc = next_clip.truncate() / next_clip.w;
                egui::vec2(
                    (next_ndc.x - ndc.x) * logical_width * 0.5,
                    (ndc.y - next_ndc.y) * logical_height * 0.5,
                )
            })
        });
        let color = marker_color(marker.kind);
        paint_marker_shape(&painter, center, marker.kind, color, route_direction);

        if let Some(label) = marker_label(&marker.name) {
            painter.text(
                center + egui::vec2(9.0, -14.0 - (index % 3) as f32 * 12.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(13.0),
                egui::Color32::WHITE,
            );
        }
    }

    egui::Window::new("轨迹标记")
        .id(egui::Id::new("track-marker-list"))
        .default_pos(egui::pos2(12.0, 12.0))
        .resizable(false)
        .movable(false)
        .collapsible(false)
        .title_bar(false)
        .default_width(285.0)
        .show(context, |ui| {
            ui.label(format!("标记总数：{}", markers.len()));
            ui.separator();
            for marker in markers.iter().take(14) {
                let response = ui.colored_label(
                    marker_color(marker.kind),
                    marker_label(&marker.name).unwrap_or(marker.kind.label()),
                );
                if let Some(metadata) = &marker.kml_metadata {
                    let mut tooltip = String::new();
                    if let Some(id) = &metadata.id {
                        tooltip.push_str(&format!("ID: {id}\n"));
                    }
                    if let Some(style_url) = &metadata.style_url {
                        tooltip.push_str(&format!("Style: {style_url}\n"));
                    }
                    if !metadata.description.is_empty() {
                        let description: String = metadata.description.chars().take(500).collect();
                        tooltip.push_str(&description);
                        if metadata.description.chars().count() > 500 {
                            tooltip.push('…');
                        }
                    }
                    if !tooltip.is_empty() {
                        response.on_hover_text(tooltip);
                    }
                }
                let source = match marker.source {
                    MarkerSource::Kml => "KML 标注",
                    MarkerSource::TrackEndpoint => "轨迹端点推断",
                    MarkerSource::ElevationProfile => "轨迹剖面推断",
                };
                ui.small(format!("{} · {}", marker.kind.label(), source));
            }
            if markers.len() > 14 {
                ui.small(format!("其余 {} 个标记显示在场景中", markers.len() - 14));
            }
            for warning in warnings {
                ui.colored_label(egui::Color32::YELLOW, warning);
            }
        });
}

fn marker_color(kind: MarkerKind) -> egui::Color32 {
    match kind {
        MarkerKind::Start => egui::Color32::from_rgb(80, 220, 125),
        MarkerKind::End => egui::Color32::from_rgb(250, 90, 90),
        MarkerKind::Highest | MarkerKind::Peak | MarkerKind::PeakCandidate => {
            egui::Color32::from_rgb(255, 190, 65)
        }
        MarkerKind::Lowest => egui::Color32::from_rgb(90, 180, 255),
        MarkerKind::Pass | MarkerKind::PassCandidate => egui::Color32::from_rgb(195, 130, 255),
        MarkerKind::Valley | MarkerKind::ValleyCandidate => egui::Color32::from_rgb(90, 180, 255),
        MarkerKind::Other => egui::Color32::LIGHT_GRAY,
    }
}

fn marker_label(name: &str) -> Option<&str> {
    let name = name.trim();
    (!name.is_empty()).then_some(name)
}

fn start_triangle_vertices(center: egui::Pos2, direction: Option<egui::Vec2>) -> [egui::Pos2; 3] {
    let direction = direction
        .filter(|direction| direction.is_finite() && direction.length_sq() > 1e-6)
        .map(egui::Vec2::normalized)
        .unwrap_or(egui::vec2(0.0, -1.0));
    let side = egui::vec2(-direction.y, direction.x);
    [
        center + direction * 10.0,
        center - direction * 6.0 + side * 6.0,
        center - direction * 6.0 - side * 6.0,
    ]
}

fn endpoint_flag_cells(
    center: egui::Pos2,
    color: egui::Color32,
) -> [(egui::Rect, egui::Color32); 4] {
    std::array::from_fn(|index| {
        let row = index / 2;
        let column = index % 2;
        let rect = egui::Rect::from_min_size(
            egui::pos2(
                center.x - 5.0 + column as f32 * 5.0,
                center.y - 9.0 + row as f32 * 5.0,
            ),
            egui::vec2(5.0, 5.0),
        );
        let color = if (row + column) % 2 == 0 {
            color
        } else {
            egui::Color32::WHITE
        };
        (rect, color)
    })
}

fn paint_marker_shape(
    painter: &egui::Painter,
    center: egui::Pos2,
    kind: MarkerKind,
    color: egui::Color32,
    route_direction: Option<egui::Vec2>,
) {
    let stroke = egui::Stroke::new(1.5_f32, egui::Color32::WHITE);
    match kind {
        MarkerKind::Start => {
            painter.add(egui::Shape::convex_polygon(
                start_triangle_vertices(center, route_direction).to_vec(),
                color,
                stroke,
            ));
        }
        MarkerKind::End => {
            let pole_x = center.x - 5.0;
            painter.line_segment(
                [
                    egui::pos2(pole_x, center.y + 8.0),
                    egui::pos2(pole_x, center.y - 9.0),
                ],
                egui::Stroke::new(2.0_f32, color),
            );
            for (square, square_color) in endpoint_flag_cells(center, color) {
                painter.rect_filled(square, 0.0, square_color);
            }
        }
        MarkerKind::Highest
        | MarkerKind::Lowest
        | MarkerKind::Peak
        | MarkerKind::Pass
        | MarkerKind::Valley
        | MarkerKind::PeakCandidate
        | MarkerKind::PassCandidate
        | MarkerKind::ValleyCandidate
        | MarkerKind::Other => {
            painter.circle_filled(center, 4.0, color);
            painter.circle_stroke(center, 4.0, stroke);
        }
    }
}

fn load_cjk_font(context: &egui::Context) -> bool {
    #[cfg(target_os = "windows")]
    let candidates = [
        "C:\\Windows\\Fonts\\simhei.ttf",
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
    ];
    #[cfg(target_os = "macos")]
    let candidates = [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
    ];
    #[cfg(target_os = "linux")]
    let candidates = [
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    ];
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let candidates: [&str; 0] = [];

    for candidate in candidates {
        if std::path::Path::new(candidate).exists() {
            let bytes = match std::fs::read(candidate) {
                Ok(bytes) => bytes,
                Err(error) => {
                    eprintln!("Could not read CJK font {candidate}: {error}");
                    return false;
                }
            };
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("system-cjk".to_owned(), egui::FontData::from_owned(bytes));
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "system-cjk".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "system-cjk".to_owned());
            context.set_fonts(fonts);
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_labels_only_use_non_empty_source_names() {
        assert_eq!(marker_label("  trail marker  "), Some("trail marker"));
        assert_eq!(marker_label("   "), None);
        assert_eq!(marker_label(""), None);
    }

    #[test]
    fn start_triangle_follows_route_direction_and_defaults_to_screen_up() {
        let center = egui::pos2(20.0, 30.0);
        let north_facing = start_triangle_vertices(center, None);
        let route_facing = start_triangle_vertices(center, Some(egui::vec2(1.0, 0.0)));

        assert!(north_facing[0].y < center.y);
        assert!(route_facing[0].x > center.x);
    }

    #[test]
    fn endpoint_flag_uses_a_two_by_two_checkerboard() {
        let color = egui::Color32::from_rgb(250, 90, 90);
        let cells = endpoint_flag_cells(egui::pos2(20.0, 30.0), color);

        assert_eq!(cells[0].1, color);
        assert_eq!(cells[1].1, egui::Color32::WHITE);
        assert_eq!(cells[2].1, egui::Color32::WHITE);
        assert_eq!(cells[3].1, color);
        assert!(cells.iter().all(|(rect, _)| {
            (rect.width() - 5.0).abs() < f32::EPSILON && (rect.height() - 5.0).abs() < f32::EPSILON
        }));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn system_font_renders_chinese_marker_labels() {
        let context = egui::Context::default();
        assert!(load_cjk_font(&context));
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, 120.0),
                )),
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    ui.label("起点 · 最低点");
                });
            },
        );
        let meshes = context.tessellate(output.shapes, 1.0);
        assert!(!meshes.is_empty());
    }
}
