mod renderer;

use rfd::{FileDialog, MessageDialog, MessageLevel};
use std::error::Error;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::Arc;
use visualize_kml::analysis::analyze_document;
use visualize_kml::geometry::project_document;
use visualize_kml::parse_kml;
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowBuilder};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

struct ViewerApp {
    window: Option<Arc<Window>>,
    renderer: Option<renderer::Renderer>,
    initial_path: Option<PathBuf>,
    modifiers: ModifiersState,
    dragging: bool,
    last_cursor_position: Option<(f64, f64)>,
}

impl ViewerApp {
    fn new(initial_path: Option<PathBuf>) -> Self {
        Self {
            window: None,
            renderer: None,
            initial_path,
            modifiers: ModifiersState::empty(),
            dragging: false,
            last_cursor_position: None,
        }
    }

    fn initialize(&mut self, event_loop: &winit::event_loop::EventLoopWindowTarget<()>) {
        let window = match WindowBuilder::new()
            .with_title(format!(
                "VisualizeKML {APP_VERSION} - Ctrl+O to open a KML file"
            ))
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0))
            .build(event_loop)
        {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("Could not create window: {error}");
                event_loop.exit();
                return;
            }
        };
        match pollster::block_on(renderer::Renderer::new(window.clone())) {
            Ok(renderer) => {
                self.window = Some(window);
                self.renderer = Some(renderer);
                if let Some(path) = self.initial_path.take() {
                    self.load_file(path);
                }
            }
            Err(error) => {
                show_error("Graphics initialization failed", &error.to_string());
                event_loop.exit();
            }
        }
    }

    fn load_file(&mut self, path: PathBuf) {
        let result = (|| {
            let xml = std::fs::read_to_string(&path)?;
            let document = parse_kml(&xml)?;
            let projected = project_document(&document)?;
            let analysis = analyze_document(&document);
            let renderer = self
                .renderer
                .as_mut()
                .ok_or_else(|| std::io::Error::other("graphics renderer is not ready"))?;
            renderer
                .set_tracks(projected, analysis)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            Ok::<_, Box<dyn Error>>(document)
        })();

        match result {
            Ok(document) => {
                if let Some(window) = &self.window {
                    let title = if document.name.is_empty() {
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("KML")
                            .to_owned()
                    } else {
                        document.name
                    };
                    window.set_title(&format!(
                        "VisualizeKML {APP_VERSION} - {title} (Ctrl+O to open another file)"
                    ));
                    window.request_redraw();
                }
            }
            Err(error) => {
                eprintln!("Could not load {}: {error}", path.display());
                show_error(
                    "Could not load KML file",
                    &format!("{}\n\n{error}", path.display()),
                );
            }
        }
    }

    fn choose_file(&mut self) {
        if let Some(path) = FileDialog::new()
            .add_filter("KML files", &["kml"])
            .pick_file()
        {
            self.load_file(path);
        }
    }

    fn handle_window_event(
        &mut self,
        event: WindowEvent,
        event_loop: &winit::event_loop::EventLoopWindowTarget<()>,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.dragging = state == ElementState::Pressed;
                self.last_cursor_position = None;
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.dragging {
                    if let Some((last_x, last_y)) = self.last_cursor_position {
                        let scale = self
                            .window
                            .as_ref()
                            .map_or(1.0, |window| window.scale_factor());
                        let dx = ((position.x - last_x) / scale).clamp(-1000.0, 1000.0) as f32;
                        let dy = ((position.y - last_y) / scale).clamp(-1000.0, 1000.0) as f32;
                        if let Some(renderer) = &mut self.renderer {
                            renderer.orbit(dx, dy);
                        }
                        self.request_redraw();
                    }
                }
                self.last_cursor_position = Some((position.x, position.y));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(position) => (position.y / 60.0) as f32,
                };
                if let Some(renderer) = &mut self.renderer {
                    renderer.zoom(amount);
                }
                self.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && !event.repeat
                    && self.modifiers.control_key()
                    && event.physical_key == PhysicalKey::Code(KeyCode::KeyO) =>
            {
                self.choose_file();
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = &mut self.renderer {
                    match renderer.render() {
                        Ok(()) => {}
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            if let Some(window) = &self.window {
                                let size = window.inner_size();
                                renderer.resize(size.width, size.height);
                            }
                            self.request_redraw();
                        }
                        Err(wgpu::SurfaceError::Timeout) => {
                            eprintln!("Graphics surface timed out; the next redraw will retry");
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            show_error(
                                "Graphics memory exhausted",
                                "The graphics device ran out of memory. The application will close.",
                            );
                            event_loop.exit();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn show_error(title: &str, description: &str) {
    let _ = MessageDialog::new()
        .set_level(MessageLevel::Error)
        .set_title(title)
        .set_description(description)
        .show();
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let first_arg = args.next();
    if first_arg.as_deref() == Some(OsStr::new("--version")) {
        println!("VisualizeKML {APP_VERSION}");
        return Ok(());
    }
    let initial_path = first_arg.map(PathBuf::from).or_else(|| {
        FileDialog::new()
            .add_filter("KML files", &["kml"])
            .pick_file()
    });
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = ViewerApp::new(initial_path);
    event_loop.run(move |event, event_loop| match event {
        Event::Resumed if app.window.is_none() => app.initialize(event_loop),
        Event::WindowEvent { window_id, event }
            if app
                .window
                .as_ref()
                .is_some_and(|window| window.id() == window_id) =>
        {
            app.handle_window_event(event, event_loop)
        }
        _ => {}
    })?;
    Ok(())
}
